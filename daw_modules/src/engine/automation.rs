// daw_modules/src/engine/automation.rs

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationNode<T> {
    pub time: u64, // Position in samples
    pub value: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutomationCurve<T> {
    nodes: Vec<AutomationNode<T>>,
}

impl<T> AutomationCurve<T> {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn nodes(&self) -> &[AutomationNode<T>] {
        &self.nodes
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
    }
}

// We specifically implement the math/interpolation for f32 (Gain, Pan, etc.)
// This keeps the engine pure and lock-free during real-time evaluation.
impl AutomationCurve<f32> {
    /// Inserts a node. If a node at the exact sample time exists, it overwrites it.
    /// Uses binary_search_by_key to guarantee O(log n) sorted insertion.
    pub fn insert_node(&mut self, time: u64, value: f32) {
        let node = AutomationNode { time, value };
        match self.nodes.binary_search_by_key(&time, |n| n.time) {
            Ok(pos) => self.nodes[pos].value = value, // Exact time match, overwrite
            Err(pos) => self.nodes.insert(pos, node), // Insert in sorted position
        }
    }

    /// Removes a node at a specific time, returning true if found and removed.
    pub fn remove_node_at_time(&mut self, time: u64) -> bool {
        if let Ok(pos) = self.nodes.binary_search_by_key(&time, |n| n.time) {
            self.nodes.remove(pos);
            true
        } else {
            false
        }
    }

    /// Pure math evaluation. Returns the exact interpolated value at a given sample position.
    pub fn get_value_at_time(&self, time: u64, default_value: f32) -> f32 {
        if self.nodes.is_empty() {
            return default_value;
        }

        let first = &self.nodes[0];
        if time <= first.time {
            return first.value;
        }

        let last = &self.nodes[self.nodes.len() - 1];
        if time >= last.time {
            return last.value;
        }

        // We are somewhere between the first and last node.
        match self.nodes.binary_search_by_key(&time, |n| n.time) {
            Ok(pos) => self.nodes[pos].value, // Exact hit on a node
            Err(pos) => {
                // pos is the insertion index, meaning:
                // pos - 1 is the previous node
                // pos is the next node
                let prev = &self.nodes[pos - 1];
                let next = &self.nodes[pos];

                let range = (next.time - prev.time) as f64; // Use f64 to prevent overflow in division
                let progress = (time - prev.time) as f64 / range;

                // Linear interpolation: v1 + (v2 - v1) * progress
                prev.value + ((next.value - prev.value) * progress as f32)
            }
        }
    }
}

// In daw_modules/src/engine/automation.rs (Add to the bottom of the file)

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
    noise_floor_db: f32,
) -> Vec<AutomationNode<f32>> {
    let mut nodes = Vec::new();
    if audio_buffer.is_empty() || channels == 0 {
        return nodes;
    }

    let frames = audio_buffer.len() / channels;
    let window_frames = ((analysis_window_ms as f64 / 1000.0) * sample_rate as f64) as usize;
    if window_frames == 0 { return nodes; }

    let start_sample_offset = (start_time_sec * sample_rate as f64) as u64;

    let mut prev_smoothed_gain = 0.0;
    let mut last_emitted_gain = 0.0;
    let mut is_first_node = true;

    // We process the buffer in chunks (windows)
    for chunk_idx in 0..=(frames / window_frames) {
        let start_idx = chunk_idx * window_frames;
        if start_idx >= frames { break; }
        
        let end_idx = std::cmp::min(start_idx + window_frames, frames);
        let actual_frames = end_idx - start_idx;
        if actual_frames == 0 { break; }

        // 1 & 2. Calculate RMS for the window (Mixdown to mono for analysis)
        let mut sum_sq = 0.0;
        for i in start_idx..end_idx {
            let mut mono_sample = 0.0;
            for c in 0..channels {
                mono_sample += audio_buffer[i * channels + c];
            }
            mono_sample /= channels as f32;
            sum_sq += mono_sample * mono_sample;
        }
        
        let rms = (sum_sq / actual_frames as f32).sqrt();
        let rms_db = if rms > 1e-5 { 20.0 * rms.log10() } else { -70.0 };

        // 3. Silence Gate (-45 dB threshold)
        // If it's silence, we want the rider to gracefully return to 0.0 dB (Unity)
        let mut raw_gain = 0.0; 
        if rms_db > noise_floor_db {
            // 4. Compute Raw Gain
            raw_gain = target_lufs - rms_db;
        }

        // 5. Clamp Gain Limits
        raw_gain = raw_gain.clamp(max_cut_db, max_boost_db);

        // 6. Apply EMA Smoothing (V2: Asymmetric Attack/Release)
        let smoothed_gain = if is_first_node {
            raw_gain
        } else {
            // Determine if the fader is moving DOWN (cutting peaks/closing gate) 
            // or moving UP (boosting quiet words/opening gate)
            let is_moving_down = raw_gain < prev_smoothed_gain;

            // Attack (Fast): ~0.3 multiplier allows the fader to move quickly.
            // Release (Slow): We use the user's `smoothness` (e.g., 0.7 to 0.9) to glide smoothly.
            let current_coeff = if is_moving_down {
                0.3 // Fast reaction to loud peaks
            } else {
                smoothness // Slow, natural recovery for quiet words
            };

            (prev_smoothed_gain * current_coeff) + (raw_gain * (1.0 - current_coeff))
        };
        prev_smoothed_gain = smoothed_gain;

        // 7. Node Thinning & 8. Generation
        // Only emit a node if it's the first one, or if it deviates by >= 0.15 dB
        let time_in_samples = start_sample_offset + start_idx as u64;

        if is_first_node || (smoothed_gain - last_emitted_gain).abs() >= 0.15 {
            nodes.push(AutomationNode {
                time: time_in_samples,
                value: smoothed_gain,
            });
            last_emitted_gain = smoothed_gain;
            is_first_node = false;
        }
    }
    
    // Safety Wrap-up: Ensure the automation lane returns to 0 dB at the end of the clip
    if let Some(last) = nodes.last() {
        if last.value.abs() > 0.01 {
             let final_time = start_sample_offset + frames as u64;
             nodes.push(AutomationNode {
                 time: final_time,
                 value: 0.0,
             });
        }
    }

    nodes
}