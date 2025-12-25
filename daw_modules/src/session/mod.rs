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
                gain: t.gain,
                pan: t.pan,
                muted: t.muted,
                solo: t.solo,
                clips, 
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
                track.gain = t_state.gain;
                track.pan = t_state.pan;
                track.muted = t_state.muted;
                track.solo = t_state.solo;
                
                for clip_state in t_state.clips {
                    let start = std::time::Duration::from_secs_f64(clip_state.start_time);
                    
                    // FIX: Use the captured 'sample_rate' and 'channels' variables here
                    let _ = track.add_clip(
                        clip_state.path, 
                        start, 
                        sample_rate, 
                        channels,
                        None
                    );
                }
            }
        }

        Ok(manifest.master_gain)
    }
}