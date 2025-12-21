// src/engine/mixer.rs

use super::track::Track;
use std::time::Duration;

pub struct Mixer {
    channels: usize,
    // temp_mix: Vec<f32>,
    mix_buffer: Vec<f32>,
    scratch_buffer: Vec<f32>,
}

impl Mixer {
    pub fn new(channels: usize) -> Self {
        let initial_capacity = 2048 * channels;
        Self {
            channels,
            mix_buffer: Vec::with_capacity(initial_capacity),
            scratch_buffer: Vec::with_capacity(initial_capacity),
        }
    }

    pub fn begin_block(&mut self, frames: usize) {
        let needed = frames * self.channels;
        if self.mix_buffer.len() != needed {
            self.mix_buffer.resize(needed, 0.0);
        } 
        if self.scratch_buffer.len() < needed {
            self.scratch_buffer.resize(needed, 0.0);
        }
        
        self.mix_buffer[..needed].fill(0.0);
    }

    // UPDATED Signature
    pub fn render_track(
        &mut self, 
        track: &mut Track, 
        frames: usize, 
        channels: usize, 
        engine_time: Duration, 
        sample_rate: u32,
        is_audible: bool
    ) {
        debug_assert_eq!(channels, self.channels);

        let total_samples = frames * self.channels;
        // let mut temp = vec![0.0f32; frames * channels];

        // Pass time info to track
        let written_frames = track.render_into(
            &mut self.scratch_buffer[..total_samples],
            channels, 
            engine_time, 
            sample_rate
        );

        if is_audible && written_frames > 0 {
            let samples = written_frames * channels;
            for i in 0..samples {
                self.mix_buffer[i] += self.scratch_buffer[i]
            }
        }
    }

    pub fn mix_into(&self, out: &mut [f32], channels: usize) {
        debug_assert_eq!(channels, self.channels);
        let len = out.len().min(self.mix_buffer.len());
        
        for i in 0..len {
            let sample = self.mix_buffer[i];
            if sample.abs() < 1e-10 {
                out[i] = 0.0;
                continue;
            }
            out[i] = sample.tanh();
        }
    }
}