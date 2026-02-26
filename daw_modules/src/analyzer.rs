// daw_modules/src/analyzer.rs

use rustfft::{FftPlanner, num_complex::Complex};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AnalysisProfile {
    pub integrated_rms_db: f32,
    pub max_sample_peak_db: f32,
    pub crest_factor_db: f32,
    
    // Spectral Analysis
    pub spectral_centroid_hz: f32,
    pub energy_lows_pct: f32,   // 20Hz - 250Hz
    pub energy_mids_pct: f32,   // 250Hz - 4kHz
    pub energy_highs_pct: f32,  // 4kHz - 20kHz
}

pub fn analyze_audio_buffer(buffer: &[f32], channels: usize, sample_rate: u32) -> AnalysisProfile {
    if buffer.is_empty() || channels == 0 {
        return AnalysisProfile {
            integrated_rms_db: -60.0,
            max_sample_peak_db: -60.0,
            ..Default::default()
        };
    }

    let mut max_peak = 0.0_f32;
    let mut sum_sq = 0.0_f64; // Use f64 to prevent precision loss on massive files
    
    // ==========================================
    // 1. Time-Domain Pass (Peak & RMS)
    // ==========================================
    for &sample in buffer {
        let abs_s = sample.abs();
        if abs_s > max_peak {
            max_peak = abs_s;
        }
        sum_sq += (sample as f64) * (sample as f64);
    }

    let rms = (sum_sq / buffer.len() as f64).sqrt() as f32;
    
    let max_sample_peak_db = if max_peak > 1e-5 { 20.0 * max_peak.log10() } else { -60.0 };
    let integrated_rms_db = if rms > 1e-5 { 20.0 * rms.log10() } else { -60.0 };
    let crest_factor_db = (max_sample_peak_db - integrated_rms_db).max(0.0);

    // ==========================================
    // 2. Frequency-Domain Pass (Chunked FFT)
    // ==========================================
    let fft_size = 4096;
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let mut total_lows = 0.0_f32;
    let mut total_mids = 0.0_f32;
    let mut total_highs = 0.0_f32;
    let mut weighted_freq_sum = 0.0_f32;
    let mut total_magnitude = 0.0_f32;

    let frames = buffer.len() / channels;
    let mut complex_buffer = vec![Complex { re: 0.0, im: 0.0 }; fft_size];

    // Process in sequential chunks
    for chunk_start in (0..frames).step_by(fft_size) {
        let chunk_end = chunk_start + fft_size;
        
        // Skip the very last partial chunk to keep math simple
        if chunk_end > frames {
            break; 
        }

        // Fill complex buffer (mix to mono and apply Hann window)
        for i in 0..fft_size {
            let frame_idx = chunk_start + i;
            let mut mono_sample = 0.0;
            
            // Downmix to mono
            for c in 0..channels {
                mono_sample += buffer[frame_idx * channels + c];
            }
            mono_sample /= channels as f32;

            // Apply Hann window to prevent spectral leakage
            let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
            complex_buffer[i] = Complex { re: mono_sample * window, im: 0.0 };
        }

        // Execute FFT
        fft.process(&mut complex_buffer);

        // Analyze bins (Only need the first half, up to Nyquist frequency)
        for i in 0..=fft_size / 2 {
            let mag = complex_buffer[i].norm(); // Magnitude of the complex number
            let freq = (i as f32 * sample_rate as f32) / fft_size as f32;

            total_magnitude += mag;
            weighted_freq_sum += freq * mag;

            if freq >= 20.0 && freq < 250.0 {
                total_lows += mag;
            } else if freq >= 250.0 && freq < 4000.0 {
                total_mids += mag;
            } else if freq >= 4000.0 && freq <= 20000.0 {
                total_highs += mag;
            }
        }
    }

    let spectral_centroid_hz = if total_magnitude > 0.0 {
        weighted_freq_sum / total_magnitude
    } else {
        0.0
    };

    // Normalize energy percentages
    let energy_total = total_lows + total_mids + total_highs;
    let (energy_lows_pct, energy_mids_pct, energy_highs_pct) = if energy_total > 0.0 {
        (total_lows / energy_total, total_mids / energy_total, total_highs / energy_total)
    } else {
        (0.0, 0.0, 0.0)
    };

    AnalysisProfile {
        integrated_rms_db,
        max_sample_peak_db,
        crest_factor_db,
        spectral_centroid_hz,
        energy_lows_pct,
        energy_mids_pct,
        energy_highs_pct,
    }
}