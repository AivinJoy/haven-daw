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

// 🧠 NEW: Future-Proof Explanations Enum
#[derive(Debug, Clone, Serialize)]
pub enum FrequencyBand {
    Sub,     // 20 - 60 Hz
    Bass,    // 60 - 250 Hz
    LowMid,  // 250 - 500 Hz
    Mid,     // 500 - 2000 Hz
    HighMid, // 2000 - 6000 Hz
    Air,     // 6000 - 20000 Hz
}

// 🧠 NEW: Hybrid Analysis Structure
#[derive(Debug, Clone, Default, Serialize)]
pub struct SpectralAnalysis {
    pub avg_band_energy_pct: [f32; 6],  // Normalized average energy per band
    pub peak_band_energy_pct: [f32; 6], // Normalized peak energy per band (transients/dynamics)
    pub peak_frequency_hz: f32,         // The single strongest resonant frequency
    pub peak_magnitude: f32,            // The raw magnitude of that peak
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

    // 🧠 NEW: Advanced Hybrid Spectral Data
    pub spectral: SpectralAnalysis,
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

        // --- Peak Event Extraction (Dynamic Clustering) ---
        if frame_peak > 0.0 {
            let peak_db = 20.0 * frame_peak.log10();
            if peak_db > peak_threshold_db {
                if !in_cluster {
                    in_cluster = true;
                    cluster_max_db = peak_db;
                    cluster_max_frame = frame_idx;
                    cluster_last_frame = frame_idx;
                } else {
                    if frame_idx - cluster_last_frame <= merge_window_frames {
                        if peak_db > cluster_max_db {
                            cluster_max_db = peak_db;
                            cluster_max_frame = frame_idx;
                        }
                        cluster_last_frame = frame_idx;
                    } else {
                        if peak_events.len() < max_peak_events {
                            peak_events.push(PeakEvent {
                                time_sec: round_2(cluster_max_frame as f32 / sample_rate_f),
                                peak_db: round_1(cluster_max_db),
                            });
                        }
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
    let mut gated_windows: Vec<LoudnessWindow> = all_windows.into_iter()
        .filter(|w| w.rms_db > -70.0)
        .collect();

    let (integrated_loudness_db, p95, p50, p10) = if gated_windows.is_empty() {
        (-70.0, -70.0, -70.0, -70.0)
    } else {
        let mut power_sum = 0.0;
        for w in &gated_windows {
            power_sum += 10.0_f32.powf(w.rms_db / 10.0);
        }
        let prelim_integrated = 10.0 * (power_sum / gated_windows.len() as f32).log10();

        let relative_threshold = prelim_integrated - 10.0;
        gated_windows.retain(|w| w.rms_db >= relative_threshold);

        if gated_windows.is_empty() {
            (-70.0, -70.0, -70.0, -70.0)
        } else {
            let mut final_power_sum = 0.0;
            for w in &gated_windows {
                final_power_sum += 10.0_f32.powf(w.rms_db / 10.0);
            }
            let final_integrated = 10.0 * (final_power_sum / gated_windows.len() as f32).log10();

            gated_windows.sort_by(|a, b| a.rms_db.partial_cmp(&b.rms_db).unwrap());
            
            let len = gated_windows.len();
            let p95_val = gated_windows[(len as f32 * 0.95).floor() as usize].rms_db;
            let p50_val = gated_windows[(len as f32 * 0.50).floor() as usize].rms_db;
            let p10_val = gated_windows[(len as f32 * 0.10).floor() as usize].rms_db;

            (round_1(final_integrated), p95_val, p50_val, p10_val)
        }
    };

    let crest_factor_db = round_1((max_sample_peak_db - integrated_loudness_db).max(0.0));

    let mut quiet_windows = Vec::new();
    let mut loud_windows = Vec::new();
    
    if !gated_windows.is_empty() {
        quiet_windows = gated_windows.iter().take(extreme_window_count).cloned().collect();
        loud_windows = gated_windows.iter().rev().take(extreme_window_count).cloned().collect();
    }

    // ==========================================
    // 3. 🧠 HYBRID SPECTRAL ANALYSIS
    // ==========================================
    let fft_size = 4096;
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let mut complex_buffer = vec![Complex { re: 0.0, im: 0.0 }; fft_size];

    let mut total_band_energy = [0.0_f32; 6];
    let mut peak_band_energy = [0.0_f32; 6];
    let mut global_peak_freq = 0.0_f32;
    let mut global_peak_mag = 0.0_f32;
    let mut total_magnitude = 0.0_f32;

    for chunk_start in (0..frames).step_by(fft_size) {
        if chunk_start + fft_size > frames { break; }

        for i in 0..fft_size {
            let frame_idx = chunk_start + i;
            let mut mono_sample = 0.0;
            for c in 0..channels { mono_sample += buffer[frame_idx * channels + c]; }
            mono_sample /= channels as f32;

            let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
            complex_buffer[i] = Complex { re: mono_sample * window, im: 0.0 };
        }

        fft.process(&mut complex_buffer);

        let mut chunk_band_energy = [0.0_f32; 6];

        for i in 0..=fft_size / 2 {
            let mag = complex_buffer[i].norm();
            let freq = (i as f32 * sample_rate_f) / fft_size as f32;

            if mag > global_peak_mag {
                global_peak_mag = mag;
                global_peak_freq = freq;
            }
            
            total_magnitude += mag;

            let band_idx = if freq < 20.0 { continue; } 
                else if freq < 60.0 { 0 }
                else if freq < 250.0 { 1 }
                else if freq < 500.0 { 2 }
                else if freq < 2000.0 { 3 }
                else if freq < 6000.0 { 4 }
                else if freq <= 20000.0 { 5 }
                else { continue; };

            chunk_band_energy[band_idx] += mag;
            total_band_energy[band_idx] += mag;
        }

        // Track local peaks per band to understand dynamics
        for b in 0..6 {
            if chunk_band_energy[b] > peak_band_energy[b] {
                peak_band_energy[b] = chunk_band_energy[b];
            }
        }
    }

    // 🧠 ENERGY NORMALIZATION
    let mut avg_band_energy_pct = [0.0_f32; 6];
    let mut peak_band_energy_pct = [0.0_f32; 6];
    let peak_total = peak_band_energy.iter().sum::<f32>();

    if total_magnitude > 0.0 {
        for b in 0..6 { avg_band_energy_pct[b] = round_2(total_band_energy[b] / total_magnitude); }
    }
    if peak_total > 0.0 {
        for b in 0..6 { peak_band_energy_pct[b] = round_2(peak_band_energy[b] / peak_total); }
    }

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
        spectral: SpectralAnalysis {
            avg_band_energy_pct,
            peak_band_energy_pct,
            peak_frequency_hz: global_peak_freq.round(),
            peak_magnitude: round_2(global_peak_mag),
        }
    }
}