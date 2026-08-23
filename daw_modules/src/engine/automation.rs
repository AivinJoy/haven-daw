// daw_modules/src/engine/automation.rs

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationNode<T> {
    pub time: f64, // Position in seconds
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
// [daw_modules/src/engine/automation.rs]

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
// --- PRO SETTINGS ---
    let step_ms = analysis_window_ms as f32;
    
    let lookahead_sec = 0.050; 
    let lookahead_frames = (lookahead_sec * sample_rate as f64) as usize;
    
    // 🚀 FIX 4: Pro Tuning Node Density
    let min_delta_db = 0.35; 
    let min_time_delta = 0.06; 

    let base_attack_alpha = 0.4 + (smoothness * 0.2);
    let base_release_alpha = 0.85 + (smoothness * 0.1);

    // 🚀 FIX 1: Raise effective noise floor to catch real-world room tone/mic bleed
    let effective_noise_floor = noise_floor_db.max(-45.0);

    let mut current_gain_db: f32 = 0.0;
    let mut last_written_gain: f32 = 999.0;
    let mut last_written_time: f64 = -1.0;

    let mut prev_env_db: f32 = -60.0;

    let mut breath_timer_ms: f32 = 0.0;
    let mut silence_timer_ms: f32 = 0.0;

    for chunk_idx in 0..=(frames / window_frames) {
        let start_idx = chunk_idx * window_frames;
        if start_idx >= frames { break; }

        let end_idx = (start_idx + window_frames).min(frames);
        let actual_frames = end_idx - start_idx;

        let mut sum_sq = 0.0_f32;
        let mut peak = 0.0_f32;
        let mut zero_crossings = 0;
        let mut prev_sample = 0.0_f32;

        // 🚀 FIX 3: True Lookahead Buffer reading
        for i in start_idx..end_idx {
            // Read ahead into the future!
            let read_idx = (i + lookahead_frames).min(frames - 1);
            
            let mut mono = 0.0;
            for c in 0..channels {
                mono += audio_buffer[read_idx * channels + c];
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
        let is_silence = env_db < (effective_noise_floor - 5.0);
        
        // 🚀 FIX 4: Faster loud detection (+0.5 LUFS)
        let is_loud = env_db > (target_lufs + 0.5);
        let is_vocal = env_db > (target_lufs - 8.0);
        
        let vocal_strength = ((env_db - (target_lufs - 20.0)) / 20.0).clamp(0.0, 1.0);

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

        let is_breath = breath_timer_ms > 80.0 && env_db > effective_noise_floor;

        if is_silence {
            silence_timer_ms += step_ms;
        } else {
            silence_timer_ms = 0.0;
        }

        let is_long_silence = silence_timer_ms > 120.0;

       // --- PRIORITY LOGIC & TARGET GAIN ---
        // 🚀 FIX 6: Dynamic Silence Floor
        let silence_floor = if preserve_dynamics { -6.0 } else { -10.0 };

        let mut target_gain: f32 = if is_long_silence {
            silence_floor 

        } else if is_breath {
            // 🚀 FIX 2: Breath Boosting Bug (Hard clamp below 0.0)
            let breath_target = (-4.0_f32 * breath_conf).min(0.0); 
            breath_target.min(current_gain_db) 

        } else if is_loud {
            (target_lufs - env_db) * 0.5 

        } else if env_db < (target_lufs - 12.0) {
            let diff = target_lufs - env_db;
            let scaled = diff / (1.0 + diff.abs() * 0.10);
            scaled.min(max_boost_db)

        } else {
            let mut diff = target_lufs - env_db;
            let softness = if preserve_dynamics { 0.45 } else { 0.65 };
            diff *= softness; 
            diff
        };

        if !is_long_silence && !is_breath {
            target_gain *= 0.5 + (vocal_strength * 0.5);
        }

        target_gain = target_gain.clamp(max_cut_db, max_boost_db);

        // Calculate time before the silence bypass
       let time_sec = start_idx as f64 / sample_rate as f64;
        let absolute_time = start_time_sec + time_sec; 

        if is_long_silence {
            if (current_gain_db - silence_floor).abs() > 0.1 {
                current_gain_db = silence_floor;
                
                // 🚀 FIX 6: First Node Bias (0.7)
                let output_value = if nodes.is_empty() { current_gain_db * 0.7 } else { current_gain_db };
                
                nodes.push(AutomationNode {
                    time: absolute_time,
                    value: output_value,
                });
                last_written_gain = output_value;
                last_written_time = absolute_time;
            }
            continue; 
        }

        // --- ADAPTIVE SMOOTHING ---
        let (current_attack, current_release) = if is_loud || is_breath {
            // 🚀 FIX 2: Faster attack (0.3) to catch peaks before they overshoot
            (0.3, 0.5) 
        } else {
            (base_attack_alpha, base_release_alpha) 
        };

       if target_gain > current_gain_db {
            current_gain_db = current_attack * current_gain_db + (1.0 - current_attack) * target_gain;
        } else {
            current_gain_db = current_release * current_gain_db + (1.0 - current_release) * target_gain;
        }

        // --- MAX DELTA CLAMP (Slew Rate Limiter) ---
        // 🚀 FIX 5: Adaptive Slew Rate
        let max_step = if is_loud { 0.8 } else { 1.2 }; 
        
        if (current_gain_db - last_written_gain).abs() > max_step && last_written_gain != 999.0 {
            if current_gain_db > last_written_gain {
                current_gain_db = last_written_gain + max_step;
            } else {
                current_gain_db = last_written_gain - max_step;
            }
        }

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

        // --- NODE OUTPUT ---
        if (current_gain_db - last_written_gain).abs() >= min_delta_db
            && (absolute_time - last_written_time) > min_time_delta
        {
            // 🚀 FIX 6: First Node Bias (0.7)
            let output_value = if nodes.is_empty() {
                current_gain_db * 0.7 
            } else {
                current_gain_db
            };

            nodes.push(AutomationNode {
                time: absolute_time,
                value: output_value,
            });

            last_written_gain = output_value;
            last_written_time = absolute_time;
        }
    } // <-- End of chunk_idx loop

    // 🚀 FIX: Smooth 300ms exit fade
    let final_audio_sec = start_time_sec + (frames as f64 / sample_rate as f64);
    if last_written_gain.abs() > 0.01 {
         nodes.push(AutomationNode {
             time: final_audio_sec, 
             value: current_gain_db,
         });
         nodes.push(AutomationNode {
             time: final_audio_sec + 0.300, 
             value: 0.0,
         });
    }

    nodes
}