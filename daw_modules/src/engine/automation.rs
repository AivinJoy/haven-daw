// daw_modules/src/engine/automation.rs

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationNode<T> {
    pub time: f64, // Position in samples
    pub value: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutomationCurve<T> {
    nodes: Vec<AutomationNode<T>>,
}

impl<T> AutomationCurve<T> {
    pub fn new() -> Self { Self { nodes: Vec::new() } }
    pub fn nodes(&self) -> &[AutomationNode<T>] { &self.nodes }
    pub fn clear(&mut self) { self.nodes.clear(); }
}

// We specifically implement the math/interpolation for f32 (Gain, Pan, etc.)
// This keeps the engine pure and lock-free during real-time evaluation.
impl AutomationCurve<f32> {
    /// Inserts a node. If a node at the exact sample time exists, it overwrites it.
    /// Uses binary_search_by_key to guarantee O(log n) sorted insertion.
    pub fn insert_node(&mut self, time: f64, value: f32) {
        let node = AutomationNode { time, value };
        // Use total_cmp to safely binary search floats
        match self.nodes.binary_search_by(|n| n.time.total_cmp(&time)) {
            Ok(pos) => self.nodes[pos].value = value, 
            Err(pos) => self.nodes.insert(pos, node), 
        }
    }

    /// Removes a node at a specific time, returning true if found and removed.
    pub fn remove_node_at_time(&mut self, time: f64) -> bool {
        if let Ok(pos) = self.nodes.binary_search_by(|n| n.time.total_cmp(&time)) {
            self.nodes.remove(pos);
            true
        } else {
            false
        }
    }

    /// Pure math evaluation. Returns the exact interpolated value at a given sample position.
    pub fn get_value_at_time(&self, time: f64, default_value: f32) -> f32 {
        if self.nodes.is_empty() { return default_value; }

        let first = &self.nodes[0];
        if time <= first.time { return first.value; }

        let last = &self.nodes[self.nodes.len() - 1];
        if time >= last.time { return last.value; }

        match self.nodes.binary_search_by(|n| n.time.total_cmp(&time)) {
            Ok(pos) => self.nodes[pos].value,
            Err(pos) => {
                let prev = &self.nodes[pos - 1];
                let next = &self.nodes[pos];

                // Pure absolute float math! No more u64 overflow risks.
                let range = next.time - prev.time; 
                let progress = (time - prev.time) / range;

                prev.value + ((next.value - prev.value) * progress as f32)
            }
        }
    }
}

/// Generates a sparse, smoothed automation curve for Vocal Riding.
/// Designed to run offline (non-realtime) when triggered by the AI Agent.
pub fn generate_rider_automation(
    audio_buffer: &[f32],
    channels: usize,
    sample_rate: u32,
    start_time_sec: f64,
    target_lufs: f32,
    max_boost_db: f32,
    max_cut_db: f32,
    smoothness: f32,
    analysis_window_ms: u32,
    _noise_floor_db: f32, // Deprecated in favor of Zone C
    preserve_dynamics: bool,
) -> Vec<AutomationNode<f32>> {
    let mut nodes = Vec::new();
    if audio_buffer.is_empty() || channels == 0 {
        return nodes;
    }

    let frames = audio_buffer.len() / channels;
    let window_frames = ((analysis_window_ms as f64 / 1000.0) * sample_rate as f64) as usize;
    if window_frames == 0 { return nodes; }

    // 🚀 REMOVED start_sample_offset u64 calculation here

    let mut prev_smoothed_gain = 0.0;
    let mut last_emitted_gain = 0.0;
    let mut is_first_node = true;

    for chunk_idx in 0..=(frames / window_frames) {
        let start_idx = chunk_idx * window_frames;
        if start_idx >= frames { break; }
        
        let end_idx = std::cmp::min(start_idx + window_frames, frames);
        let actual_frames = end_idx - start_idx;
        if actual_frames == 0 { break; }

        let mut sum_sq = 0.0;
        let mut zero_crossings = 0;
        let mut prev_mono = 0.0;

        // 1. Calculate RMS & Zero-Crossing Rate (Spectral proxy)
        for i in start_idx..end_idx {
            let mut mono_sample = 0.0;
            for c in 0..channels {
                mono_sample += audio_buffer[i * channels + c];
            }
            mono_sample /= channels as f32;
            sum_sq += mono_sample * mono_sample;

            // Zero crossing detection for high-frequency breath/noise check
            if (mono_sample > 0.0 && prev_mono <= 0.0) || (mono_sample < 0.0 && prev_mono >= 0.0) {
                zero_crossings += 1;
            }
            prev_mono = mono_sample;
        }
        
        let rms = (sum_sq / actual_frames as f32).sqrt();
        let rms_db = if rms > 1e-5 { 20.0 * rms.log10() } else { -70.0 };
        
        // ZCR > ~0.15 indicates heavily high-frequency dominant signal
        let zcr_rate = zero_crossings as f32 / actual_frames as f32;
        let is_breath_or_noise = zcr_rate > 0.15;

        // 2. The 3-Zone Logic + Spectral Check
        let raw_gain: f32;
        let attack_coeff: f32; // Lower is faster

        if rms_db <= target_lufs - 18.0 || is_breath_or_noise {
            // ZONE C: Noise / Breath / Silence -> Return to 0dB, FAST drop
            raw_gain = 0.0;
            attack_coeff = 0.1; 
        } else if rms_db > target_lufs - 18.0 && rms_db <= target_lufs - 8.0 {
            // ZONE B: Weak Vocal / Transition -> Soft approach (Limited boost)
            let calculated_boost = (target_lufs - rms_db) * 0.4; 
            raw_gain = calculated_boost.min(max_boost_db * 0.5); 
            attack_coeff = smoothness.max(0.85); 
        } else {
            // ZONE A: Strong Vocal -> Normal Riding
            let mut calculated_boost = target_lufs - rms_db;
            
            if calculated_boost.abs() < 1.5 {
                calculated_boost = 0.0;
            }
            if preserve_dynamics {
                calculated_boost *= 0.5; 
            }
            raw_gain = calculated_boost.clamp(max_cut_db, max_boost_db);
            attack_coeff = 0.4; 
        }

        // 3. Apply Dynamic EMA Smoothing
        let smoothed_gain = if is_first_node {
            raw_gain
        } else {
            (prev_smoothed_gain * attack_coeff) + (raw_gain * (1.0 - attack_coeff))
        };
        prev_smoothed_gain = smoothed_gain;

        // 4. Time Calculation (🚀 PURE SECONDS `f64`)
        let chunk_time_sec = start_idx as f64 / sample_rate as f64;
        let absolute_time_sec = start_time_sec + chunk_time_sec;

        // 5. Node Thinning & Emission
        if is_first_node || (smoothed_gain - last_emitted_gain).abs() >= 0.15 {
            nodes.push(AutomationNode {
                time: absolute_time_sec, // 🚀 Now perfectly matches f64
                value: smoothed_gain,
            });
            last_emitted_gain = smoothed_gain;
            is_first_node = false;
        }
    }
    
    // Safety Wrap-up: Ensure the automation lane returns to 0 dB at the end
    if let Some(last) = nodes.last() {
        if last.value.abs() > 0.01 {
             // 🚀 Calculate final time in pure seconds
             let final_time_sec = start_time_sec + (frames as f64 / sample_rate as f64);
             nodes.push(AutomationNode {
                 time: final_time_sec, // 🚀 Now perfectly matches f64
                 value: 0.0,
             });
        }
    }

    nodes
}