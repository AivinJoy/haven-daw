use std::sync::atomic::{AtomicU32, AtomicBool, Ordering}; // <--- ADDED AtomicBool

// Note: In later steps, we will map these into audio_processor_dynamics::Compressor 
// and audio_processor_analysis::peak_detector::PeakDetector. 
// For now, we establish the lock-free, allocation-free boundary for the Audio Thread.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CompressorParams {
    pub is_active: bool, // <--- ADDED BYPASS FLAG
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_gain_db: f32,
}

/// Helper to safely store f32 in an AtomicU32 for real-time safe parameter updates
fn f32_to_atomic(val: f32) -> AtomicU32 {
    AtomicU32::new(val.to_bits())
}

fn atomic_to_f32(atomic: &AtomicU32) -> f32 {
    f32::from_bits(atomic.load(Ordering::Relaxed))
}

/// A real-time safe Audio Compressor.
/// Guarantees no heap allocations or blocking mutexes in the `process` loop.
pub struct CompressorNode {
    // --- User Controls (Lock-free for UI updates from Svelte) ---
    is_active: AtomicBool, // <--- ADDED BYPASS FLAG
    threshold_db: AtomicU32,
    ratio: AtomicU32,
    attack_ms: AtomicU32,
    release_ms: AtomicU32,
    makeup_gain_db: AtomicU32,

    // --- Internal DSP State ---
    sample_rate: f32,
    envelope: f32,
}

impl CompressorNode {
    /// Initialize a new Compressor with default tracking settings
    pub fn new(sample_rate: f32) -> Self {
        Self {
            is_active: AtomicBool::new(true), // <--- Default to ON
            // Default: -20dB threshold, 4:1 ratio, 5ms attack, 50ms release
            threshold_db: f32_to_atomic(-20.0),
            ratio: f32_to_atomic(4.0),
            attack_ms: f32_to_atomic(5.0),
            release_ms: f32_to_atomic(50.0),
            makeup_gain_db: f32_to_atomic(0.0),

            sample_rate,
            envelope: 0.0,
        }
    }

    // --- Parameter Setters (Called by the UI/Tauri Commands) ---
    
    pub fn set_active(&self, active: bool) {
        self.is_active.store(active, Ordering::Relaxed);
    }

    pub fn set_threshold(&self, db: f32) {
        self.threshold_db.store(db.to_bits(), Ordering::Relaxed);
    }
    
    pub fn set_ratio(&self, r: f32) {
        self.ratio.store(r.to_bits(), Ordering::Relaxed);
    }
    
    pub fn set_attack(&self, ms: f32) {
        self.attack_ms.store(ms.to_bits(), Ordering::Relaxed);
    }
    
    pub fn set_release(&self, ms: f32) {
        self.release_ms.store(ms.to_bits(), Ordering::Relaxed);
    }
    
    pub fn set_makeup_gain(&self, db: f32) {
        self.makeup_gain_db.store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn get_params(&self) -> CompressorParams {
        CompressorParams {
            is_active: self.is_active.load(Ordering::Relaxed), // <--- READ BYPASS
            threshold_db: atomic_to_f32(&self.threshold_db),
            ratio: atomic_to_f32(&self.ratio),
            attack_ms: atomic_to_f32(&self.attack_ms),
            release_ms: atomic_to_f32(&self.release_ms),
            makeup_gain_db: atomic_to_f32(&self.makeup_gain_db),
        }
    }

    pub fn set_params(&self, params: CompressorParams) {
        self.set_active(params.is_active); // <--- WRITE BYPASS
        self.set_threshold(params.threshold_db);
        self.set_ratio(params.ratio);
        self.set_attack(params.attack_ms);
        self.set_release(params.release_ms);
        self.set_makeup_gain(params.makeup_gain_db);
    }

    // --- DSP Processing (Called continuously by the Audio Engine Thread) ---

    /// Processes a chunk of audio samples in place. 
    /// GUARANTEE: No locks, no blocking, no allocations.
    pub fn process(&mut self, buffer: &mut [f32]) {
        // --- ZERO CPU TRUE BYPASS ---
        // If the compressor is turned off, skip processing entirely!
        if !self.is_active.load(Ordering::Relaxed) {
            return; 
        }

        // 1. Load current parameters atomically once per block to save CPU overhead
        let threshold = atomic_to_f32(&self.threshold_db);
        let ratio = atomic_to_f32(&self.ratio);
        let attack = atomic_to_f32(&self.attack_ms);
        let release = atomic_to_f32(&self.release_ms);
        let makeup = atomic_to_f32(&self.makeup_gain_db);

        // Calculate time constants based on sample rate
        let attack_coef = (-1.0 / (attack * 0.001 * self.sample_rate)).exp();
        let release_coef = (-1.0 / (release * 0.001 * self.sample_rate)).exp();
        
        let makeup_linear = 10.0_f32.powf(makeup / 20.0);

        for sample in buffer.iter_mut() {
            // Step A: Detect signal level (Peak analysis)
            let input_level = sample.abs();
            
            // Step B: Envelope Follower
            if input_level > self.envelope {
                self.envelope = attack_coef * (self.envelope - input_level) + input_level;
            } else {
                self.envelope = release_coef * (self.envelope - input_level) + input_level;
            }

            // Step C: Convert envelope to decibels
            let env_db = 20.0 * self.envelope.max(1e-5).log10();

            // Step D: Calculate Gain Reduction
            let mut gain_reduction_db = 0.0;
            if env_db > threshold {
                let overshoot = env_db - threshold;
                // Apply the ratio calculation
                gain_reduction_db = overshoot * (1.0 - (1.0 / ratio));
            }

            // Step E: Convert Gain Reduction back to linear multiplier
            let gain_reduction_linear = 10.0_f32.powf(-gain_reduction_db / 20.0);

            // Step F: Apply gain reduction and makeup gain to the audio sample
            *sample *= gain_reduction_linear * makeup_linear;
        }
    }
}