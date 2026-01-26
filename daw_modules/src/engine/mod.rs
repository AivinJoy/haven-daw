// src/engine/mod.rs

pub mod track;
pub mod mixer;
pub mod time;

pub use track::{Track, TrackId, TrackState};
pub use mixer::Mixer;
pub use time::TempoMap;

use std::time::Duration;
use rand::seq::IndexedRandom;

#[derive(Clone, Debug)]
pub struct Transport {
    pub position: Duration,
    pub playing: bool,
    pub tempo: TempoMap,
}

pub struct Engine {
    pub transport: Transport,
    pub sample_rate: u32,
    pub channels: usize,
    pub master_gain: f32, // <--- New Field
    tracks: Vec<Track>,
    mixer: Mixer,
    next_id: u32,
}

impl Engine {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            transport: Transport {
                position: Duration::from_secs(0),
                playing: false,
                tempo: TempoMap::default(),
            },
            sample_rate,
            channels,
            master_gain: 1.0, // <--- FIXED: Initialized here (Default 1.0 = 100%)
            tracks: Vec::new(),
            mixer: Mixer::new(channels),
            next_id: 0,
        }
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.transport.tempo.bpm = bpm as f64;
    }

    pub fn clear_tracks(&mut self) {
        self.tracks.clear();
    }

    // --- NEW: Create a generic empty track ---
    // --- NEW: Create a generic empty track ---
    // --- NEW: Create a generic empty track ---
    pub fn add_empty_track(&mut self) -> TrackId {
        let id = TrackId(self.next_id);
        self.next_id += 1; 

        // 1. Define Palette
        let colors = [
            "bg-brand-blue", "bg-brand-red", "bg-purple-500", 
            "bg-emerald-500", "bg-orange-500", "bg-pink-500",
            "bg-cyan-500", "bg-indigo-500", "bg-rose-500"
        ];

        // 2. Pick Random Color
        let mut rng = rand::rng(); // For rand 0.9+
        // If using older rand (0.8), use: let mut rng = rand::thread_rng();
        // Based on your cargo.toml having rand 0.9.0, rand::rng() is correct.
        
        let chosen_color = colors.choose(&mut rng)
            .unwrap_or(&"bg-brand-blue")
            .to_string();

        // 3. Create Track with Color
        let track = Track::new(
            id, 
            format!("Track {}", id.0 + 1), 
            chosen_color, // <--- Pass Color Here
            self.sample_rate, 
            self.channels
        );
        self.tracks.push(track);
        id
    }

    // --- NEW: Add a Clip to an existing Track ---
    pub fn add_clip(&mut self, track_index: usize, path: String, start_time_secs: f64) -> anyhow::Result<()> {
        let sample_rate = self.sample_rate;
        let channels = self.channels;
        let start_time = Duration::from_secs_f64(start_time_secs);

        // Ensure this is still here!
        let current_pos = Some(self.transport.position); 

        if let Some(track) = self.tracks.get_mut(track_index) {
            track.add_clip(path, start_time, sample_rate, channels, current_pos)?;
        }
        Ok(())
    }

    pub fn remove_track(&mut self, index: usize) -> anyhow::Result<()> {
        if index < self.tracks.len() {
            self.tracks.remove(index);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Track index out of bounds"))
        }
    }

    // --- UPDATED: Wrapper for backward compatibility ---
    // Creates a track and adds the file as the first clip at 0.0s
    pub fn add_track(&mut self, path: String) -> anyhow::Result<TrackId> {
        let id = self.add_empty_track();
        // Add the file as a clip starting at 0.0
        self.add_clip(id.0 as usize, path, 0.0)?;
        Ok(id)
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn split_clip(&mut self, track_index: usize, time_secs: f64) -> anyhow::Result<()> {
        let split_time = Duration::from_secs_f64(time_secs);
        
        if let Some(track) = self.tracks.get_mut(track_index) {
             track.split_at_time(split_time, self.sample_rate, self.channels)?;
             Ok(())
        } else {
            Err(anyhow::anyhow!("Track not found"))
        }
    }

    pub fn merge_clip_with_next(&mut self, track_index: usize, clip_index: usize) -> anyhow::Result<()> {
        if let Some(track) = self.tracks.get_mut(track_index) {
            track.merge_next(clip_index)
        } else {
            Err(anyhow::anyhow!("Track not found"))
        }
    }

    pub fn delete_clip(&mut self, track_index: usize, clip_index: usize) -> anyhow::Result<()> {
        if let Some(track) = self.tracks.get_mut(track_index) {
            track.delete_clip(clip_index)
        } else {
            Err(anyhow::anyhow!("Track not found"))
        }
    }

    pub fn tracks_mut(&mut self) -> &mut [Track] {
        &mut self.tracks
    }

    pub fn play(&mut self) {
        self.transport.playing = true;
        for t in &mut self.tracks {
            t.set_state(TrackState::Playing);
        }
    }

    pub fn pause(&mut self) {
        self.transport.playing = false;
        for t in &mut self.tracks {
            t.set_state(TrackState::Paused);
        }
    }

    pub fn seek(&mut self, pos: Duration) {
        self.transport.position = pos;
        for t in &mut self.tracks {
            t.seek(pos);
        }
    }

    pub fn move_clip(&mut self, track_index: usize, clip_index: usize, new_start: f64) -> anyhow::Result<()> {
        if let Some(track) = self.tracks.get_mut(track_index) {
            track.move_clip(clip_index, std::time::Duration::from_secs_f64(new_start));
            Ok(())
        } else {
            Err(anyhow::anyhow!("Track index {} out of bounds", track_index))
        }
    }

    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);

        if !self.transport.playing {
            return;
        }

        let channels = self.channels;
        let frames = out.len() / channels;

        self.mixer.begin_block(frames);

        let current_pos = self.transport.position;
        let sr = self.sample_rate;

        // --- NON-DESTRUCTIVE SOLO LOGIC ---
        // Check if ANY track has solo enabled
        let any_solo = self.tracks.iter().any(|t| t.solo);

        for track in &mut self.tracks {
            // Determine if this specific track should make sound
            let is_audible = if any_solo {
                // Solo Mode: Ignore Mute, only play if THIS track is soloed
                track.solo
            } else {
                // Normal Mode: Play if NOT muted
                !track.muted
            };

            let effectively_audible = is_audible && track.gain > 0.001;

            // Note: We access track.state inside render_track or check it here
            // Assuming render_into handles the "is state == Playing" check, 
            // but we can check here to save a function call:
            if matches!(track.state(), TrackState::Playing) {
                 self.mixer.render_track(
                    track, 
                    frames, 
                    channels, 
                    current_pos,
                    sr, 
                    effectively_audible);
            }
        }

        self.mixer.mix_into(out, channels);

        // Apply Master Gain
        if (self.master_gain - 1.0).abs() > 0.001 {
            for sample in out.iter_mut() {
                *sample *= self.master_gain;
            }
        }

        let secs = frames as f64 / self.sample_rate as f64;
        self.transport.position += Duration::from_secs_f64(secs);
    }
}