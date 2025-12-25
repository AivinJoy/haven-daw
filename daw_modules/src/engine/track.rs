// src/engine/track.rs

use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::thread::JoinHandle;
use std::time::Duration;

use ringbuf::traits::{Split, Consumer};
use ringbuf::HeapRb;
use ringbuf::wrap::caching::Caching;
use ringbuf::storage::Heap;
use ringbuf::SharedRb;
// use ringbuf::traits::Consumer;

use crate::decoder::{spawn_decoder_with_ctrl, DecoderCmd};
use crate::bpm::adapter;

/// Identifier for a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackState {
    Stopped,
    Playing,
    Paused,
}

/// Concrete decoder handle for one track:
/// owns decoder thread + ringbuffer consumer.
pub struct DecoderHandle {
    consumer: Caching<Arc<SharedRb<Heap<f32>>>, false, true>,
    _decoder_thread: JoinHandle<()>,
    is_playing: Arc<AtomicBool>,
    seek_tx: Sender<DecoderCmd>,
    #[allow(dead_code)]
    output_sample_rate: u32,
    #[allow(dead_code)]
    output_channels: usize,
}

impl DecoderHandle {
    pub fn new_for_engine(
        path: String,
        source_channels: usize,
        output_channels: usize,
        source_sample_rate: u32,
        output_sample_rate: u32,
    ) -> anyhow::Result<Self> {
        let rb = HeapRb::<f32>::new(131_072);
        let (producer, consumer) = rb.split();

        let is_playing = Arc::new(AtomicBool::new(true));

        // spawn_decoder_with_ctrl returns (JoinHandle, Sender<DecoderCmd>).
        let (decoder_thread, seek_tx) = spawn_decoder_with_ctrl(
            path,
            producer,
            is_playing.clone(),
            source_channels,
            output_channels,
            source_sample_rate,
            output_sample_rate,
        );

        Ok(Self {
            consumer,
            _decoder_thread: decoder_thread,
            is_playing,
            seek_tx,
            output_sample_rate,
            output_channels,
        })
    }

    pub fn set_playing(&self, playing: bool) {
        self.is_playing.store(playing, Ordering::Relaxed);
    }

    // --- UPDATED: Seek now clears buffer to fix delay ---
    pub fn seek(&mut self, pos: Duration) {
        // 1. Tell decoder to seek
        let _ = self.seek_tx.send(DecoderCmd::Seek(pos));
        
        // 2. Clear buffer instantly to remove old audio
        // FIX: Use try_pop() instead of pop()
        while self.consumer.try_pop().is_some() {}
    }

    /// Read up to `frames` of interleaved f32 into `dst`. Returns frames actually written.
    /// Read samples and ADD them to the destination buffer (Mixing).
    /// Returns the number of frames actually mixed.
    pub fn mix_interleaved(&mut self, dst: &mut [f32], frames: usize, channels: usize) -> usize {
        let samples_needed = frames * channels;
        let mut mixed_count = 0usize;

        // Loop through the buffer and ADD the sample from the decoder
        // This allows multiple clips to overlap without cutting each other off
        for i in 0..samples_needed {
            if let Some(sample) = self.consumer.try_pop() {
                dst[i] += sample; 
                mixed_count += 1;
            } else {
                break; // Buffer empty
            }
        }

        mixed_count / channels
    }
    /// This keeps the ring buffer in sync when the track is muted.
    pub fn consume(&mut self, frames: usize, channels: usize) {
        let samples_needed = frames * channels;
        for _ in 0..samples_needed {
            if self.consumer.try_pop().is_none() {
                break;
            }
        }
    }
}

pub struct Clip {
    pub path: String,
    pub start_time: Duration, // Position on timeline
    pub offset: Duration,     // Start offset in the file (trimming)
    pub duration: Duration,   // Duration on timeline
    decoder: DecoderHandle,
}

impl Clip {
    pub fn new(path: String, start_time: Duration, output_sr: u32, output_ch: usize) -> anyhow::Result<Self> {
        
        // 1. Probe to get metadata AND Calculate Duration
        // We need the exact duration to prevent "Seek out of range" errors.
        let (samples, source_sr, source_ch) = match adapter::decode_to_vec(&path) {
            Ok((s, sr, ch)) => (s, sr, ch),
            Err(e) => {
                println!("⚠️ Clip Probe Failed, using fallback: {}", e);
                (Vec::new(), 44100, 2) // Fallback
            }
        };

        // Calculate Duration: Total Samples / Channels / Sample Rate
        let duration_secs = if source_sr > 0 && source_ch > 0 {
            samples.len() as f64 / source_ch as f64 / source_sr as f64
        } else {
            0.0
        };
        let duration = Duration::from_secs_f64(duration_secs);

        println!("📎 Clip: {} | Dur: {:.2}s | {}Hz {}ch", path, duration_secs, source_sr, source_ch);

        // 2. Create Decoder
        let decoder = DecoderHandle::new_for_engine(
            path.clone(), 
            source_ch, 
            output_ch, 
            source_sr, 
            output_sr
        )?;

        decoder.set_playing(false);

        Ok(Self {
            path,
            start_time,
            offset: Duration::ZERO,
            duration, // <--- FIX: Use Actual Duration
            decoder,
        })
    }

    pub fn set_playing(&self, playing: bool) {
        self.decoder.set_playing(playing);
    }

    pub fn seek(&mut self, global_pos: Duration) {
        if global_pos >= self.start_time {
            let offset_into_clip = global_pos - self.start_time + self.offset;
            
            // --- FIX: Guard against seeking past the end of the file ---
            if offset_into_clip > self.duration {
                // We are past the end of the clip. Do nothing.
                return;
            }
            // -----------------------------------------------------------

            self.decoder.seek(offset_into_clip);
        } else {
            self.decoder.seek(self.offset);
        }
    }
}

/// A single audio track in the engine.
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub gain: f32,
    pub pan: f32, // -1.0 left, 0 center, +1.0 right
    pub muted: bool,
    pub solo: bool,
    state: TrackState,
    pub clips: Vec<Clip>,
    // --- Track Start Time (for Drag & Drop) ---
}

impl Track {
    /// `engine_sample_rate` and `engine_channels` should match the output device.
    pub fn new(
        id: TrackId,
        name: String,
    ) -> Self {
        Self {
            id,
            name,
            gain: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            state: TrackState::Stopped,
            clips: Vec::new(),
        }
    }

    // Helper to add a clip (used by Engine)
    pub fn add_clip(
        &mut self, 
        path: String, 
        start_time: Duration, 
        sr: u32, 
        ch: usize,
        current_time: Option<Duration> // <--- NEW ARGUMENT
    ) -> anyhow::Result<()> {
        
        // 1. Create the clip
        let mut clip = Clip::new(path, start_time, sr, ch)?;

        // 3. Sync Position: If we know the current engine time, seek the clip immediately!
        if let Some(time) = current_time {
            clip.seek(time);
        }
        
        // 2. Sync State: If track is playing, set clip to playing
        if matches!(self.state, TrackState::Playing) {
             clip.set_playing(true);
        }

        self.clips.push(clip);
        Ok(())
    }
    

    pub fn state(&self) -> TrackState {
        self.state
    }

    pub fn set_state(&mut self, st: TrackState) {
        self.state = st;
        for clip in &self.clips {
            clip.set_playing(matches!(st, TrackState::Playing));
        }
            
    }

    pub fn seek(&mut self, global_pos: Duration) {
        // Seek ALL clips so they are ready when the playhead hits them
        for clip in &mut self.clips {
            clip.seek(global_pos);
        }
    }

    // pub fn is_active(&self) -> bool {
    //     matches!(self.state, TrackState::Playing) && self.gain > 0.0
    // }
    // --- FIX: Check Transport State only, NOT Gain/Mute ---
    pub fn is_playing(&self) -> bool {
        matches!(self.state, TrackState::Playing)
    }

    /// Pull `frames` of interleaved f32 into `dst`.
    /// Handles start_time offset logic.
    pub fn render_into(
        &mut self, 
        dst: &mut [f32], 
        channels: usize, 
        engine_time: Duration, 
        sample_rate: u32
    ) -> usize {
        dst.fill(0.0);

        if !self.is_playing(){
            return 0;
        }

        // 1. Calculate time overlap
        
        // let current_secs = engine_time.as_secs_f64();
        let buffer_duration = (dst.len() / channels) as f64 / sample_rate as f64;
        let start_secs = engine_time.as_secs_f64();
        let end_secs = start_secs + buffer_duration;

        let mut active_clips = 0;

        // Determine if we should actually mix audio or just discard it
        // Note: Solo logic is usually handled by the caller (Mixer) setting 'muted' effectively,
        // or passing a flag. Here we rely on self.muted being set correctly.
        let is_audible = !self.muted && self.gain > 0.0;

        // 1. Loop through all clips and mix them
        // 1. Loop through all clips and mix them
        for clip in &mut self.clips {
            let clip_start = clip.start_time.as_secs_f64();
            let clip_end = clip_start + clip.duration.as_secs_f64(); // <--- FIX: Use duration

            // --- FIX: Check if we are entirely past the clip ---
            // If the buffer starts AFTER the clip ends, skip it.
            if start_secs >= clip_end {
                continue;
            }
            // If the buffer ends BEFORE the clip starts, skip it.
            if end_secs <= clip_start {
                continue;
            }
            // ---------------------------------------------------

            // Calculate buffer offset (silence before clip starts in this block)
            let mut offset_frames = 0;
            if start_secs < clip_start {
                let diff = clip_start - start_secs;
                offset_frames = (diff * sample_rate as f64).round() as usize;
            }
            
            if offset_frames * channels >= dst.len() { continue; }

            let mix_dst = &mut dst[(offset_frames * channels)..];
            let frames_to_mix = mix_dst.len() / channels;

            if is_audible {
                clip.decoder.mix_interleaved(mix_dst, frames_to_mix, channels);
                active_clips += 1;
            } else {
                clip.decoder.consume(frames_to_mix, channels);
            }
        }

        // Apply Gain/Pan only if we actually mixed something
        if active_clips > 0 && is_audible {
            let gain = self.gain;
            let pan = self.pan.clamp(-1.0, 1.0);
            
            let (pan_l, pan_r) = if channels >= 2 {
                let angle = (pan + 1.0) * 0.25 * std::f32::consts::PI;
                (angle.cos(), angle.sin())
            } else {
                (1.0, 1.0)
            };

            for i in (0..dst.len()).step_by(channels) {
                if channels >= 2 {
                    dst[i] *= gain * pan_l;   
                    dst[i+1] *= gain * pan_r; 
                    for c in 2..channels {
                        dst[i+c] *= gain;
                    }
                } else {
                    dst[i] *= gain;
                }
            }
        }

        dst.len() / channels
    }
}