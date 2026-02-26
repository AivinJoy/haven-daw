// src/session/mod.rs

pub mod commands;
pub mod serialization; // <--- ADD THIS
pub mod export;

use crate::engine::Engine;
use commands::{Command, CommandManager};
use serialization::{ProjectManifest, TrackState, ClipState}; // <--- USE THIS
use std::sync::{Arc, Mutex};
use anyhow::Result;

pub struct Session {
    pub command_manager: CommandManager,
}

impl Session {
    pub fn new() -> Self {
        Self {
            command_manager: CommandManager::new(100),
        }
    }

    pub fn apply(&mut self, engine: &Arc<Mutex<Engine>>, cmd: Box<dyn Command>) -> Result<()> {
        let mut guard = engine.lock().unwrap();
        self.command_manager.push(cmd, &mut guard)
    }

    pub fn undo(&mut self, engine: &Arc<Mutex<Engine>>) -> Result<bool> {
        let mut guard = engine.lock().unwrap();
        self.command_manager.undo(&mut guard)
    }

    pub fn redo(&mut self, engine: &Arc<Mutex<Engine>>) -> Result<bool> {
        let mut guard = engine.lock().unwrap();
        self.command_manager.redo(&mut guard)
    }

    // --- SAVE / LOAD IMPLEMENTATION ---

    pub fn save_project(&self, engine: &Arc<Mutex<Engine>>, path: &str, master_gain: f32) -> Result<()> {
        let eng = engine.lock().unwrap();

        // 1. Gather state from Engine tracks
        let tracks: Vec<TrackState> = eng.tracks().iter().map(|t| {
            // FIX: Use a code block { } to define variables before returning the struct
            let clips = t.clips.iter().map(|c| ClipState {
                path: c.path.clone(), 
                start_time: c.start_time.as_secs_f64(),
                offset: c.offset.as_secs_f64(),
                duration: c.duration.as_secs_f64(),
            }).collect();

            // Return the struct at the end of the block
            TrackState {
                name: t.name.clone(),
                color: t.color.clone(), 
                gain: t.gain,
                pan: t.pan,
                muted: t.muted,
                solo: t.solo,
                clips,
                compressor: Some(t.track_compressor.get_params()), // <--- ADD THIS 
                eq: Some(t.track_eq.get_state()),
            }    
        }).collect();

        // 2. Create Manifest
        let manifest = ProjectManifest {
            version: 1,
            master_gain,
            bpm: eng.transport.tempo.bpm as f32,
            tracks,
        };

        // 3. Write to disk
        manifest.save_to_disk(path)?;
        Ok(())
    }

    pub fn load_project(&mut self, engine: &Arc<Mutex<Engine>>, path: &str) -> Result<f32> {
        let manifest = ProjectManifest::load_from_disk(path)?;
        let mut eng = engine.lock().unwrap();

        eng.clear_tracks();
        eng.transport.tempo.bpm = manifest.bpm as f64;
        self.command_manager = CommandManager::new(100);

        // FIX: Capture these values BEFORE the loop starts
        let sample_rate = eng.sample_rate;
        let channels = eng.channels;

        for t_state in manifest.tracks {
            let id = eng.add_empty_track();
            
            if let Some(track) = eng.tracks_mut().iter_mut().find(|t| t.id == id) {
                track.name = t_state.name;
                track.color = t_state.color;
                track.gain = t_state.gain;
                track.pan = t_state.pan;
                track.muted = t_state.muted;
                track.solo = t_state.solo;

                if let Some(comp_params) = t_state.compressor {
                    track.track_compressor.set_params(comp_params);
                }

                if let Some(eq_state) = t_state.eq {
                    track.track_eq.set_state(eq_state);
                }
                
                for clip_state in t_state.clips {
                    let start = std::time::Duration::from_secs_f64(clip_state.start_time);
                    let offset = std::time::Duration::from_secs_f64(clip_state.offset);
                    let duration = std::time::Duration::from_secs_f64(clip_state.duration);
                    
                    // FIX: Use restore_clip instead of add_clip.
                    // This ensures we respect the saved Offset and Duration (Split/Trim data).
                    let _ = track.restore_clip(
                        track.clips.len(), // Append to the end
                        clip_state.path, 
                        start, 
                        offset,
                        duration,
                        sample_rate, 
                        channels
                    );
                }
            }
        }

        Ok(manifest.master_gain)
    }
}