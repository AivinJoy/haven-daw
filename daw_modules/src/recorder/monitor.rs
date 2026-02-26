// src/recorder/monitor.rs

use anyhow::Result;
// --- FIX: Import the new traits for ringbuf v0.3+ ---
use ringbuf::traits::Consumer; 
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub trait AudioPop: Send {
    fn pop_sample(&mut self) -> Option<f32>;
    fn available(&self) -> usize;
}

impl<C: Consumer<Item = f32> + Send> AudioPop for C {
    fn pop_sample(&mut self) -> Option<f32> {
        self.try_pop()
    }
    fn available(&self) -> usize {
        self.occupied_len() // --- FIX: Changed from len() to occupied_len() ---
    }
}

pub struct Monitor {
    consumer: Box<dyn AudioPop>,
    pub enabled: Arc<AtomicBool>,
    input_channels: usize,
}

impl Monitor {
    // Accepts input_channels so we know how to route the audio
    pub fn new<C>(consumer: C, input_channels: usize) -> Result<Self>
    where
        C: Consumer<Item = f32> + Send + 'static,
    {
        let enabled = Arc::new(AtomicBool::new(false));
        Ok(Self { 
            consumer: Box::new(consumer), 
            enabled,
            input_channels 
        })
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn process_into(&mut self, out: &mut [f32], out_channels: usize) {
        if !self.is_enabled() {
            // Keep the ringbuffer empty when not monitoring so we don't get a blast of old audio
            while self.consumer.pop_sample().is_some() {}
            return;
        }

        // ANTI-LATENCY / SAMPLE RATE PROTECTION
        // If mic is faster than output, the buffer fills up. We force-drain it to stay real-time.
        let max_backlog = self.input_channels * 512; 
        while self.consumer.available() > max_backlog {
            self.consumer.pop_sample();
        }

        for frame in out.chunks_mut(out_channels) {
            if self.input_channels == 1 {
                // MONO INPUT -> Stereo Output
                let raw = self.consumer.pop_sample().unwrap_or(0.0) * 0.7; // 0.7 limits harsh clipping
                for sample in frame.iter_mut() {
                    *sample = raw;
                }
            } else {
                // STEREO INPUT -> Stereo Output
                let l = self.consumer.pop_sample().unwrap_or(0.0) * 0.7;
                let r = self.consumer.pop_sample().unwrap_or(0.0) * 0.7;
                
                // Drain any extra channels if input is 3+ (unlikely but safe)
                for _ in 2..self.input_channels {
                    let _ = self.consumer.pop_sample();
                }

                if frame.len() >= 2 {
                    frame[0] = l;
                    frame[1] = r;
                } else if frame.len() == 1 {
                    frame[0] = (l + r) * 0.5;
                }
            }
        }
    }
}