// src/recorder/mod.rs

pub mod input;
pub mod file_writer;
pub mod monitor;
pub mod live_waveform;

use crate::recorder::{
    file_writer::FileWriter,
    input::AudioInput,
    live_waveform::LiveWaveform,
    monitor::Monitor,
};
use anyhow::Result;
use ringbuf::{HeapRb, traits::Split};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
    Mutex,
};
use std::thread;

pub struct Recorder {
    input: AudioInput,
    writer_handle: Option<thread::JoinHandle<()>>,
    pub monitor: Option<Monitor>, // <--- CHANGED to Option
    pub monitor_enabled: Arc<AtomicBool>, // <--- NEW: Lock-free toggle
    live_waveform: Arc<Mutex<LiveWaveform>>,
    record_samples: Arc<AtomicU64>,
}

impl Recorder {
    // Use the real input sample rate from AudioInput.
    pub fn start(path: PathBuf) -> Result<Self> {
        // Ring buffer for recording
        let rec_capacity = 192_000;
        let rb_rec = HeapRb::<f32>::new(rec_capacity);
        let (prod_rec, cons_rec) = rb_rec.split();

        // Ring buffer for monitoring (smaller, low-latency)
        let mon_capacity = 192_000;
        let rb_mon = HeapRb::<f32>::new(mon_capacity);
        let (prod_mon, cons_mon) = rb_mon.split();

        // Input feeds both ring buffers and returns channels + sample rate
        let (input, channels, input_sample_rate) = AudioInput::new(prod_rec, prod_mon)?;

        // Live waveform accumulator (~512 samples per bin)
        let live_waveform = Arc::new(Mutex::new(LiveWaveform::new(512)));
        let wf_clone = live_waveform.clone();

        // Recording sample counter
        let record_samples = Arc::new(AtomicU64::new(0));
        let record_samples_clone = record_samples.clone();

        // Writer thread: write WAV + update waveform + sample counter
        let writer = FileWriter::new(&path, input_sample_rate, channels)?;

        // 5. Spawn Writer Thread
        let writer_handle = thread::spawn(move || {
            // Run the writer loop. We handle errors inside the thread gracefully.
            if let Err(e) = writer.run_with_waveform(cons_rec, wf_clone, channels, record_samples_clone) {
                eprintln!("Audio Recorder Thread Error: {}", e);
            }
        });


        // FIX 2: Pass 'channels' to the monitor so it doesn't interleave stereo into mono
        let monitor = Monitor::new(cons_mon, channels)?;
        let monitor_enabled = monitor.enabled.clone();

        Ok(Self {
            input,
            writer_handle: Some(writer_handle),
            monitor: Some(monitor),
            monitor_enabled,
            live_waveform,
            record_samples,
        })
    }

    pub fn is_monitor_enabled(&self) -> bool {
        self.monitor_enabled.load(Ordering::Relaxed)
    }

    pub fn stop(mut self) {
        // Drop input to stop capture
        drop(self.input);
        if let Some(h) = self.writer_handle.take() {
            let _ = h.join();
        }
    }

    // Recording time based on samples written and input sample rate.
    pub fn get_record_time(&self) -> std::time::Duration {
        let samples = self.record_samples.load(Ordering::Relaxed) as f64;
        let secs = samples / self.input.sample_rate as f64;
        std::time::Duration::from_secs_f64(secs)
    }

    pub fn toggle_monitor(&mut self) -> Result<()> {
        // Flip the lock-free boolean
        let cur = self.monitor_enabled.load(Ordering::Relaxed);
        self.monitor_enabled.store(!cur, Ordering::Relaxed);
        
        if self.is_monitor_enabled() {
            println!("\n🎧 Monitor ON");
        } else {
            println!("\n🎧 Monitor OFF");
        }
        Ok(())
    }

    /// For UI: clone the Arc so main.rs can snapshot bins.
    pub fn live_waveform(&self) -> Arc<Mutex<LiveWaveform>> {
        self.live_waveform.clone()
    }
}
