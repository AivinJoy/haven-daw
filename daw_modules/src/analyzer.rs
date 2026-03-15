// daw_modules/src/analyzer.rs

use rustfft::{FftPlanner, num_complex::Complex};
use serde::Serialize;

// -------------------------------------------------------------------------
// COMPACT AI PAYLOAD STRUCTURES
// -------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct PeakEvent {
    #[serde(rename = "t")]  // Minify JSON key to save LLM tokens
    pub time_sec: f32,
    #[serde(rename = "db")] // Minify JSON key
    pub peak_db: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoudnessWindow {
    #[serde(rename = "t")]
    pub time_sec: f32,
    #[serde(rename = "db")]
    pub rms_db: f32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AnalysisProfile {
    // Macro Dynamics
    pub integrated_loudness_db: f32,
    pub max_sample_peak_db: f32,
    pub crest_factor_db: f32,

    // Loudness Distribution (Gated)
    pub loudness_p95_db: f32,
    pub loudness_p50_db: f32,
    pub loudness_p10_db: f32,

    // AI Automation Telemetry
    pub peak_events: Vec<PeakEvent>,
    pub loud_windows: Vec<LoudnessWindow>,
    pub quiet_windows: Vec<LoudnessWindow>,

    // Spectral Analysis (From Original)
    pub spectral_centroid_hz: f32,
    pub energy_lows_pct: f32,
    pub energy_mids_pct: f32,
    pub energy_highs_pct: f32,
}

// -------------------------------------------------------------------------
// ROUNDING HELPER (For JSON Payload Minification)
// -------------------------------------------------------------------------
fn round_2(val: f32) -> f32 {
    (val * 100.0).round() / 100.0
}

fn round_1(val: f32) -> f32 {
    (val * 10.0).round() / 10.0
}

// -------------------------------------------------------------------------
// CORE ANALYZER
// -------------------------------------------------------------------------
pub fn analyze_audio_buffer(buffer: &[f32], channels: usize, sample_rate: u32) -> AnalysisProfile {
    if buffer.is_empty() || channels == 0 {
        return AnalysisProfile {
            integrated_loudness_db: -70.0,
            max_sample_peak_db: -70.0,
            ..Default::default()
        };
    }

    let frames = buffer.len() / channels;
    let sample_rate_f = sample_rate as f32;

    // --- Configuration ---
    let window_ms = 100.0;
    let window_frames = ((window_ms / 1000.0) * sample_rate_f) as usize;
    
    let peak_threshold_db = -3.0;
    let merge_window_ms = 120.0; // The 120ms smoothing window
    let merge_window_frames = ((merge_window_ms / 1000.0) * sample_rate_f) as usize;
    let max_peak_events = 50;
    let extreme_window_count = 20;

    // --- Time-Domain Tracking Variables ---
    let mut max_global_peak = 0.0_f32;
    let mut peak_events: Vec<PeakEvent> = Vec::with_capacity(max_peak_events);
    
    // Dynamic Cluster State
    let mut in_cluster = false;
    let mut cluster_max_db = -70.0;
    let mut cluster_max_frame = 0;
    let mut cluster_last_frame = 0;

    let mut all_windows: Vec<LoudnessWindow> = Vec::new();
    let mut current_window_sum_sq = 0.0_f64;
    let mut window_frame_count = 0;

    // ==========================================
    // 1. Time-Domain Pass (Peaks & 100ms Windows)
    // ==========================================
    for frame_idx in 0..frames {
        let mut frame_peak = 0.0_f32;
        let mut frame_sum_sq = 0.0_f64;

        // Mixdown to Mono for Loudness & get highest peak across channels
        for c in 0..channels {
            let sample = buffer[frame_idx * channels + c];
            let abs_s = sample.abs();
            
            if abs_s > frame_peak { frame_peak = abs_s; }
            if abs_s > max_global_peak { max_global_peak = abs_s; }
            
            frame_sum_sq += (sample as f64) * (sample as f64);
        }

        let mono_sq = frame_sum_sq / (channels as f64);
        current_window_sum_sq += mono_sq;
        window_frame_count += 1;

        // --- Peak Event Extraction ---
        // --- Peak Event Extraction (Dynamic Clustering) ---
        if frame_peak > 0.0 {
            let peak_db = 20.0 * frame_peak.log10();
            if peak_db > peak_threshold_db {
                if !in_cluster {
                    // Start a new cluster
                    in_cluster = true;
                    cluster_max_db = peak_db;
                    cluster_max_frame = frame_idx;
                    cluster_last_frame = frame_idx;
                } else {
                    // Are we within 120ms of the LAST peak in this cluster?
                    if frame_idx - cluster_last_frame <= merge_window_frames {
                        // Extend cluster and track the absolute loudest moment
                        if peak_db > cluster_max_db {
                            cluster_max_db = peak_db;
                            cluster_max_frame = frame_idx;
                        }
                        cluster_last_frame = frame_idx; // Push the window forward
                    } else {
                        // We passed the 120ms window. Close the previous cluster!
                        if peak_events.len() < max_peak_events {
                            peak_events.push(PeakEvent {
                                time_sec: round_2(cluster_max_frame as f32 / sample_rate_f),
                                peak_db: round_1(cluster_max_db),
                            });
                        }
                        // Start a new cluster with the current peak
                        cluster_max_db = peak_db;
                        cluster_max_frame = frame_idx;
                        cluster_last_frame = frame_idx;
                    }
                }
            }
        }

        // --- Commit 100ms Window ---
        if window_frame_count >= window_frames {
            let rms = (current_window_sum_sq / window_frames as f64).sqrt() as f32;
            let rms_db = if rms > 1e-5 { 20.0 * rms.log10() } else { -70.0 };

            all_windows.push(LoudnessWindow {
                time_sec: round_2(frame_idx as f32 / sample_rate_f),
                rms_db: round_1(rms_db),
            });

            current_window_sum_sq = 0.0;
            window_frame_count = 0;
        }
    }

    // Push the final cluster if the audio ended while we were still inside one
    if in_cluster && peak_events.len() < max_peak_events {
        peak_events.push(PeakEvent {
            time_sec: round_2(cluster_max_frame as f32 / sample_rate_f),
            peak_db: round_1(cluster_max_db),
        });
    }

    let max_sample_peak_db = if max_global_peak > 1e-5 { 20.0 * max_global_peak.log10() } else { -70.0 };

    // ==========================================
    // 2. EBU R128 Style Gating & Percentiles
    // ==========================================
    // Absolute Gate: -70 LUFS (dB)
    let mut gated_windows: Vec<LoudnessWindow> = all_windows.into_iter()
        .filter(|w| w.rms_db > -70.0)
        .collect();

    // Calculate preliminary integrated loudness to apply Relative Gate
    let (integrated_loudness_db, p95, p50, p10) = if gated_windows.is_empty() {
        (-70.0, -70.0, -70.0, -70.0)
    } else {
        // Average Power (Linear) -> dB
        let mut power_sum = 0.0;
        for w in &gated_windows {
            power_sum += 10.0_f32.powf(w.rms_db / 10.0);
        }
        let prelim_integrated = 10.0 * (power_sum / gated_windows.len() as f32).log10();

        // Relative Gate: -10 LU from preliminary
        let relative_threshold = prelim_integrated - 10.0;
        gated_windows.retain(|w| w.rms_db >= relative_threshold);

        if gated_windows.is_empty() {
            (-70.0, -70.0, -70.0, -70.0)
        } else {
            // Final Integrated
            let mut final_power_sum = 0.0;
            for w in &gated_windows {
                final_power_sum += 10.0_f32.powf(w.rms_db / 10.0);
            }
            let final_integrated = 10.0 * (final_power_sum / gated_windows.len() as f32).log10();

            // Sort for Percentiles
            gated_windows.sort_by(|a, b| a.rms_db.partial_cmp(&b.rms_db).unwrap());
            
            let len = gated_windows.len();
            let p95_val = gated_windows[(len as f32 * 0.95).floor() as usize].rms_db;
            let p50_val = gated_windows[(len as f32 * 0.50).floor() as usize].rms_db;
            let p10_val = gated_windows[(len as f32 * 0.10).floor() as usize].rms_db;

            (round_1(final_integrated), p95_val, p50_val, p10_val)
        }
    };

    let crest_factor_db = round_1((max_sample_peak_db - integrated_loudness_db).max(0.0));

    // Extract Extremes (from the sorted gated windows)
    let mut quiet_windows = Vec::new();
    let mut loud_windows = Vec::new();
    
    if !gated_windows.is_empty() {
        // Top 20 Quietest (First 20 in sorted list)
        quiet_windows = gated_windows.iter()
            .take(extreme_window_count)
            .cloned()
            .collect();
        
        // Top 20 Loudest (Last 20 in sorted list, reversed)
        loud_windows = gated_windows.iter()
            .rev()
            .take(extreme_window_count)
            .cloned()
            .collect();
    }

    // ==========================================
    // 3. Frequency-Domain Pass (Spectral Centroid)
    // ==========================================
    let fft_size = 4096;
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let mut total_lows = 0.0_f32;
    let mut total_mids = 0.0_f32;
    let mut total_highs = 0.0_f32;
    let mut weighted_freq_sum = 0.0_f32;
    let mut total_magnitude = 0.0_f32;

    let mut complex_buffer = vec![Complex { re: 0.0, im: 0.0 }; fft_size];

    for chunk_start in (0..frames).step_by(fft_size) {
        if chunk_start + fft_size > frames { break; }

        for i in 0..fft_size {
            let frame_idx = chunk_start + i;
            let mut mono_sample = 0.0;
            for c in 0..channels {
                mono_sample += buffer[frame_idx * channels + c];
            }
            mono_sample /= channels as f32;

            let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
            complex_buffer[i] = Complex { re: mono_sample * window, im: 0.0 };
        }

        fft.process(&mut complex_buffer);

        for i in 0..=fft_size / 2 {
            let mag = complex_buffer[i].norm();
            let freq = (i as f32 * sample_rate_f) / fft_size as f32;

            total_magnitude += mag;
            weighted_freq_sum += freq * mag;

            if freq >= 20.0 && freq < 250.0 { total_lows += mag; } 
            else if freq >= 250.0 && freq < 4000.0 { total_mids += mag; } 
            else if freq >= 4000.0 && freq <= 20000.0 { total_highs += mag; }
        }
    }

    let spectral_centroid_hz = if total_magnitude > 0.0 { weighted_freq_sum / total_magnitude } else { 0.0 };
    let energy_total = total_lows + total_mids + total_highs;
    let (energy_lows_pct, energy_mids_pct, energy_highs_pct) = if energy_total > 0.0 {
        (
            round_2(total_lows / energy_total), 
            round_2(total_mids / energy_total), 
            round_2(total_highs / energy_total)
        )
    } else { (0.0, 0.0, 0.0) };

    AnalysisProfile {
        integrated_loudness_db,
        max_sample_peak_db: round_1(max_sample_peak_db),
        crest_factor_db,
        loudness_p95_db: round_1(p95),
        loudness_p50_db: round_1(p50),
        loudness_p10_db: round_1(p10),
        peak_events,
        loud_windows,
        quiet_windows,
        spectral_centroid_hz: spectral_centroid_hz.round(),
        energy_lows_pct,
        energy_mids_pct,
        energy_highs_pct,
    }
}