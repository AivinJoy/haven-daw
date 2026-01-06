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
    pub duration: Duration,
     pub source_duration: Duration, // full file length (never changes)
    pub source_sr: u32,
    pub source_ch: usize,   // Duration on timeline
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
            source_duration: duration,
            source_sr: source_sr,
            source_ch: source_ch,
            decoder,
        })
    }

    pub fn new_known(
        path: String, 
        start_time: Duration, 
        offset: Duration,
        duration: Duration,          // timeline length
        source_duration: Duration,   // full file length
        source_sr: u32,
        source_ch: usize,
        output_sr: u32, 
        output_ch: usize
    ) -> anyhow::Result<Self> {
        
        // Create Decoder immediately (IO cost is low compared to decoding)
        let decoder = DecoderHandle::new_for_engine(
            path.clone(), 
            source_ch, 
            output_ch, 
            source_sr, 
            output_sr
        )?;

        // If the clip has an offset, we must seek the decoder so it's ready to play
        let mut clip = Self {
            path,
            start_time,
            offset,
            duration,
            source_duration,
            source_sr,
            source_ch,
            decoder,
        };
        
        // Ensure the decoder internal buffer is at the right spot
        clip.seek(start_time);

        Ok(clip)
    }

    pub fn set_playing(&self, playing: bool) {
        self.decoder.set_playing(playing);
    }

    pub fn seek(&mut self, global_pos: Duration) {
        // Source-file playback position (seconds into the original file)
        let file_pos = if global_pos >= self.start_time {
            global_pos - self.start_time + self.offset
        } else {
            self.offset
        };

        // Guard against seeking past end of the *source file*.
        // NOTE: This requires you to store the full source duration in the Clip.
        // If you don't have it yet, temporarily remove this guard.
        if file_pos >= self.source_duration {
            return;
        }

        self.decoder.seek(file_pos);
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

fn apply_edge_fades(
    buf: &mut [f32],
    frames: usize,
    channels: usize,
    fade_frames: usize,
    fade_in: bool,
    fade_out: bool,
) {
    if fade_frames == 0 || frames == 0 { return; }

    for f in 0..frames {
        let mut g = 1.0f32;

        if fade_in {
            let gi = (f as f32 / fade_frames as f32).clamp(0.0, 1.0);
            g = g.min(gi);
        }

        if fade_out {
            let rem = (frames - 1).saturating_sub(f);
            let go = (rem as f32 / fade_frames as f32).clamp(0.0, 1.0);
            g = g.min(go);
        }

        let base = f * channels;
        for c in 0..channels {
            buf[base + c] *= g;
        }
    }
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

    pub fn merge_next(&mut self, clip_index: usize) -> anyhow::Result<()> {
       if clip_index + 1 >= self.clips.len() {
           return Err(anyhow::anyhow!("No next clip to merge"));
       }
    
       let eps = 0.001; // 1ms tolerance
    
       let left = &self.clips[clip_index];
       let right = &self.clips[clip_index + 1];
    
       let left_start = left.start_time.as_secs_f64();
       let left_dur = left.duration.as_secs_f64();
       let left_end = left_start + left_dur;
       let right_start = right.start_time.as_secs_f64();
    
       // Must touch on timeline
       if (right_start - left_end).abs() > eps {
           return Err(anyhow::anyhow!("Clips are not adjacent on the timeline"));
       }
    
       // Must be same source file
       if left.path != right.path {
           return Err(anyhow::anyhow!("Clips have different source paths"));
       }
    
       // Must be contiguous in source file
       let left_src_end = (left.offset + left.duration).as_secs_f64();
       let right_src_start = right.offset.as_secs_f64();
       if (right_src_start - left_src_end).abs() > eps {
           return Err(anyhow::anyhow!("Clips are not contiguous in source"));
       }
    
       // Apply merge: extend left, remove right
       let right_duration = self.clips[clip_index + 1].duration;
       self.clips[clip_index].duration += right_duration;
       self.clips.remove(clip_index + 1);
    
       Ok(())
    }


    pub fn split_at_time(
        &mut self,
        split_time: Duration,
        output_sr: u32,
        output_ch: usize
    ) -> anyhow::Result<()> {

        let split_secs = split_time.as_secs_f64();

        // Find the index first so we can insert next to it

        for (i, clip) in self.clips.iter_mut().enumerate() {
            let start = clip.start_time.as_secs_f64();
            let end = start + clip.duration.as_secs_f64();

            if split_secs > start && split_secs < end {

                let relative_split_secs = split_secs - start;
                let relative_split = Duration::from_secs_f64(relative_split_secs);

                let right_start = split_time;
                let right_offset = clip.offset + relative_split;
                let right_duration = clip.duration - relative_split;

                // Left side becomes shorter on the timeline
                clip.duration = relative_split;

                let new_clip = Clip::new_known(
                    clip.path.clone(),
                    right_start,
                    right_offset,
                    right_duration,
                    clip.source_duration,
                    clip.source_sr,
                    clip.source_ch,
                    output_sr,
                    output_ch
                )?;

                // IMPORTANT: preserve full file duration + metadata
                // If your new_known doesn't set these yet, update it to do so.
                self.clips.insert(i + 1, new_clip);

                println!("✂️ Split successful @ {:.2}s", split_secs);
                break;
            }
        }

        Ok(())
    }


    pub fn move_clip(&mut self, clip_index: usize, new_start: Duration) {
        if let Some(clip) = self.clips.get_mut(clip_index) {
            clip.start_time = new_start;
            // Optionally seek immediately if the clip is currently playing 
            // so the change is audible instantly without restart
            // clip.seek(...) 
        }
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

            let fade_frames = ((sample_rate as f32) * 0.005) as usize; // 5ms

            if is_audible {
                // Render clip audio into a temp buffer first
                let mut temp = vec![0.0f32; frames_to_mix * channels];
                let written = clip.decoder.mix_interleaved(&mut temp, frames_to_mix, channels);
            
                if written > 0 {
                    // Detect whether this engine block contains the clip start or end
                    let start_edge_in_block = start_secs < clip_start && end_secs > clip_start;
                    let end_edge_in_block = start_secs < clip_end && end_secs > clip_end;
                
                    // Apply fades only if we are near an edge
                    if fade_frames > 0 && (start_edge_in_block || end_edge_in_block) {
                        apply_edge_fades(
                            &mut temp,
                            written,
                            channels,
                            fade_frames.max(1),
                            start_edge_in_block,
                            end_edge_in_block,
                        );
                    }
                
                    // Mix into the track buffer at the correct offset
                    let samples = written * channels;
                    for i in 0..samples {
                        mix_dst[i] += temp[i];
                    }
                
                    active_clips += 1;
                }
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