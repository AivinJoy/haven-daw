// src/engine/metering.rs

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// The Lock-Free bridge. The Audio Thread writes to this, the UI Thread reads from it.
pub struct TrackMeters {
    pub peak_l: AtomicU32,
    pub peak_r: AtomicU32,
    pub hold_l: AtomicU32,
    pub hold_r: AtomicU32,
    pub rms_l: AtomicU32,
    pub rms_r: AtomicU32,
}

impl TrackMeters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
            hold_l: AtomicU32::new(0),
            hold_r: AtomicU32::new(0),
            rms_l: AtomicU32::new(0),
            rms_r: AtomicU32::new(0),
        })
    }
}

/// The stateful DSP Calculator (Owned strictly by the Audio Thread)
pub struct MeterState {
    decay_coeff: f32,
    stored_peak_l: f32,
    stored_peak_r: f32,
    hold_frames_l: usize,
    hold_frames_r: usize,
    hold_duration_frames: usize,
}

impl MeterState {
    pub fn new(sample_rate: f32) -> Self {
        let release_time_sec = 0.300; // 300ms visual falloff
        
        // Block-size independent decay coefficient
        let decay_coeff = (-1.0 / (release_time_sec * sample_rate)).exp();
        
        // 500ms Peak Hold 
        let hold_duration_frames = (0.500 * sample_rate) as usize; 

        Self {
            decay_coeff,
            stored_peak_l: 0.0,
            stored_peak_r: 0.0,
            hold_frames_l: 0,
            hold_frames_r: 0,
            hold_duration_frames,
        }
    }

    pub fn process_block(&mut self, buffer: &[f32], channels: usize, meters: &TrackMeters) {
        let block_size = buffer.len() / channels;
        if block_size == 0 { return; }

        let mut max_l = 0.0_f32;
        let mut max_r = 0.0_f32;
        let mut sum_sq_l = 0.0_f32;
        let mut sum_sq_r = 0.0_f32;

        // 1. Find absolute max and sum of squares
        for chunk in buffer.chunks_exact(channels) {
            let l = chunk[0];
            max_l = max_l.max(l.abs());
            sum_sq_l += l * l;
            
            if channels > 1 {
                let r = chunk[1];
                max_r = max_r.max(r.abs());
                sum_sq_r += r * r;
            } else {
                max_r = max_l;
                sum_sq_r = sum_sq_l;
            }
        }

        let rms_l = (sum_sq_l / block_size as f32).sqrt();
        let rms_r = (sum_sq_r / block_size as f32).sqrt();

        // 2. Scale decay perfectly to the current block size
        let block_decay = self.decay_coeff.powf(block_size as f32);

        // 3. Process Left Channel (Instant Attack, Peak Hold, Scaled Decay)
        if max_l > self.stored_peak_l {
            self.stored_peak_l = max_l;
            self.hold_frames_l = self.hold_duration_frames;
        } else {
            if self.hold_frames_l > 0 {
                self.hold_frames_l = self.hold_frames_l.saturating_sub(block_size);
            } else {
                self.stored_peak_l *= block_decay;
                self.stored_peak_l += 1e-20; // Denormal protection
                self.stored_peak_l -= 1e-20;
            }
        }

        // 4. Process Right Channel
        if max_r > self.stored_peak_r {
            self.stored_peak_r = max_r;
            self.hold_frames_r = self.hold_duration_frames;
        } else {
            if self.hold_frames_r > 0 {
                self.hold_frames_r = self.hold_frames_r.saturating_sub(block_size);
            } else {
                self.stored_peak_r *= block_decay;
                self.stored_peak_r += 1e-20;
                self.stored_peak_r -= 1e-20;
            }
        }

        // 5. Write to Lock-Free Atomics
        meters.peak_l.store(max_l.to_bits(), Ordering::Relaxed);
        meters.peak_r.store(max_r.to_bits(), Ordering::Relaxed);
        meters.hold_l.store(self.stored_peak_l.to_bits(), Ordering::Relaxed);
        meters.hold_r.store(self.stored_peak_r.to_bits(), Ordering::Relaxed);
        meters.rms_l.store(rms_l.to_bits(), Ordering::Relaxed);
        meters.rms_r.store(rms_r.to_bits(), Ordering::Relaxed);
    }
}