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
/// Includes Temporal Breath Detection, Mid-Level Vocal Protection, and Energy Scaling.
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
    preserve_dynamics: bool,
) -> Vec<AutomationNode<f32>> {

    let mut nodes = Vec::new();
    if audio_buffer.is_empty() || channels == 0 {
        return nodes;
    }

    let frames = audio_buffer.len() / channels;
    let window_frames = ((analysis_window_ms as f64 / 1000.0) * sample_rate as f64) as usize;
    if window_frames == 0 { return nodes; }

    // --- CONSTANTS ---
    const BREATH_REDUCTION_DB: f32 = -3.0;
    const LOUD_CUT_MIN_DB: f32 = -1.0;

    let lookahead_sec = 0.015;
    let min_delta_db = 0.12;
    let min_time_delta = 0.02;

    let attack_alpha = 0.4 + (smoothness * 0.2);
    let release_alpha = 0.85 + (smoothness * 0.1);

    let mut current_gain_db: f32 = 0.0;
    let mut last_written_gain: f32 = 999.0;
    let mut last_written_time: f64 = -1.0;

    let mut prev_env_db: f32 = -60.0;

    let mut breath_timer_ms: f32 = 0.0;
    let mut silence_timer_ms: f32 = 0.0;

    let step_ms = analysis_window_ms as f32;

    for chunk_idx in 0..=(frames / window_frames) {
        let start_idx = chunk_idx * window_frames;
        if start_idx >= frames { break; }

        let end_idx = (start_idx + window_frames).min(frames);
        let actual_frames = end_idx - start_idx;

        let mut sum_sq = 0.0_f32;
        let mut peak = 0.0_f32;
        let mut zero_crossings = 0;
        let mut prev_sample = 0.0_f32;

        for i in start_idx..end_idx {
            let mut mono = 0.0;
            for c in 0..channels {
                mono += audio_buffer[i * channels + c];
            }
            mono /= channels as f32;

            sum_sq += mono * mono;
            peak = peak.max(mono.abs());

            if (mono > 0.0 && prev_sample <= 0.0) || (mono < 0.0 && prev_sample >= 0.0) {
                zero_crossings += 1;
            }
            prev_sample = mono;
        }

        let rms = (sum_sq / actual_frames as f32).sqrt();
        let raw_db = if rms > 1e-5 { 20.0 * rms.log10() } else { -90.0 };

        let env_db = 0.6 * prev_env_db + 0.4 * raw_db;
        prev_env_db = env_db;

        let crest = if rms > 1e-5 { peak / rms } else { 0.0 };
        let zcr = zero_crossings as f32 / actual_frames as f32;

        // --- CLASSIFICATION ---
        let is_silence = env_db < noise_floor_db;
        let is_loud = env_db > (target_lufs + 2.0);
        let is_vocal = env_db > (target_lufs - 8.0);

        let breath_score =
            ((target_lufs - env_db).max(0.0) / 18.0)
            + (zcr * 1.5)
            + ((crest - 1.8).max(0.0) * 0.4);

        let breath_conf = breath_score.clamp(0.0, 1.0);

        if breath_conf > 0.6 && !is_vocal {
            breath_timer_ms += step_ms;
        } else {
            breath_timer_ms = 0.0;
        }

        let is_breath = breath_timer_ms > 80.0;

        if is_silence {
            silence_timer_ms += step_ms;
        } else {
            silence_timer_ms = 0.0;
        }

        let is_long_silence = silence_timer_ms > 120.0;

        // --- PRIORITY LOGIC ---
        let target_gain: f32 = if is_long_silence {
            0.0

        } else if is_breath {
            BREATH_REDUCTION_DB.min(current_gain_db)

        } else if is_loud {
            (target_lufs - env_db).min(LOUD_CUT_MIN_DB)

        } else if env_db < (target_lufs - 12.0) {
            0.0

        } else {
            let mut g = target_lufs - env_db;

            if is_vocal {
                g *= if preserve_dynamics { 0.6 } else { 1.0 };
            }

            g
        };

        let target_gain = target_gain.clamp(max_cut_db, max_boost_db);

        // --- SMOOTHING ---
        if target_gain > current_gain_db {
            current_gain_db =
                attack_alpha * current_gain_db + (1.0 - attack_alpha) * target_gain;
        } else {
            current_gain_db =
                release_alpha * current_gain_db + (1.0 - release_alpha) * target_gain;
        }

        let time_sec = start_idx as f64 / sample_rate as f64;
        let absolute_time = start_time_sec + (time_sec - lookahead_sec).max(0.0);

        // DEBUG
        println!(
            "[RIDER] t={:.2}s env={:.1}dB gain={:.2} | silence={} breath={} vocal={} loud={}",
            absolute_time,
            env_db,
            current_gain_db,
            is_silence,
            is_breath,
            is_vocal,
            is_loud
        );

        if (current_gain_db - last_written_gain).abs() >= min_delta_db
            && (absolute_time - last_written_time) > min_time_delta
        {
            nodes.push(AutomationNode {
                time: absolute_time,
                value: current_gain_db,
            });

            last_written_gain = current_gain_db;
            last_written_time = absolute_time;
        }
    }

    nodes
}