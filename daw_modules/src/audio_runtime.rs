// src/audio_runtime.rs

use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::Stream;

use crate::audio::setup_output_device;
use crate::engine::Engine;
use crate::session::{Session, commands::*}; 
use crate::engine::time::GridLine;
use crate::effects::equalizer::EqParams; // <--- Import this

/// Owns Engine + CPAL stream and exposes a simple control API.
pub struct AudioRuntime {
    engine: Arc<Mutex<Engine>>,
    master_gain: Arc<Mutex<f32>>,
    session: Mutex<Session>,
    _stream: Stream,
}

pub struct TrackSnapshot {
    pub gain: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
}

pub struct FrontendClipInfo {
    pub path: String,
    pub start_time: f64,
    pub duration: f64,
    pub offset: f64,
}

pub struct FrontendTrackInfo {
    pub id: u32,
    pub name: String,
    pub gain: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub clips: Vec<FrontendClipInfo>,
}

pub struct EngineSnapshot {
    pub tracks: Vec<TrackSnapshot>,
}

impl AudioRuntime {
    /// Create engine + output stream. Optionally add one initial track.
    pub fn new(initial_track: Option<String>) -> anyhow::Result<Self> {
        let output = setup_output_device()?;
        let sample_rate = output.output_sample_rate;
        let device_channels = output.output_channels;

        // Debug log to confirm what the device is doing
         println!("🔊 AudioRuntime: Device running at {} Hz with {} channels", sample_rate, device_channels);

        let master_gain = Arc::new(Mutex::new(1.0_f32));
        // Initialize Engine with default master gain
        let mut engine = Engine::new(sample_rate, 2);

        if let Some(path) = initial_track {
            let _ = engine.add_track(path)?;
            engine.play();
        }

        let engine = Arc::new(Mutex::new(engine));
        let session = Mutex::new(Session::new());

        // Build CPAL stream that pulls from Engine::render
        let device = output.device;
        let config = output.config;
        let err_fn = |err| eprintln!("AudioRuntime output error: {err}");
        let engine_cb = engine.clone();
        let gain_cb = master_gain.clone();

        let mut scratch_buffer: Vec<f32> = Vec::with_capacity(1024);

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                if let Ok(mut eng) = engine_cb.lock() {
                    // Sync master gain from Runtime -> Engine before render
                    if let Ok(g) = gain_cb.lock() {
                        eng.master_gain = *g;
                    }
                    // 3. CALCULATE FRAMES: How many "moments in time" are in this buffer?
                    let frames = data.len() / device_channels as usize;
                    // 4. PREPARE SCRATCH: Resize to hold exactly 2 samples per frame (Stereo)
                    if scratch_buffer.len() != frames * 2 {
                        scratch_buffer.resize(frames * 2, 0.0);
                    }

                    // 5. RENDER STEREO: Engine writes strictly to our 2-channel scratch buffer
                    eng.render(&mut scratch_buffer);

                    // 6. MAP CHANNELS: Copy Scratch -> Device Buffer
                    let mut scratch_idx = 0;
                    // Process the device buffer in chunks (one chunk = one time frame across all channels)
                    for frame in data.chunks_mut(device_channels as usize) {

                        // Fast copy of Left and Right
                        let l = scratch_buffer[scratch_idx];
                        let r = scratch_buffer[scratch_idx + 1];

                        if frame.len() >= 2 {
                            frame[0] = l; // Channel 1 (Left)
                            frame[1] = r; // Channel 2 (Right)
                        }

                        // Silence remaining channels (3, 4, 5, 6, 7, 8...)
                        for sample in frame.iter_mut().skip(2) {
                            *sample = 0.0;
                        }

                        scratch_idx += 2;
                    }
                } else {
                    data.fill(0.0);
                }
            },
            err_fn,
            None,
        )?;

        stream.play()?;

        Ok(Self {
            engine,
            master_gain,
            session,
            _stream: stream,
        })
    }

    // --- UNDO / REDO ---

    pub fn undo(&self) {
        if let Ok(mut session) = self.session.lock() {
            if let Ok(success) = session.undo(&self.engine) {
                if success { println!("Using Undo"); }
                else { println!("Nothing to Undo"); }
            }
        }
    }

    pub fn redo(&self) {
        if let Ok(mut session) = self.session.lock() {
            if let Ok(success) = session.redo(&self.engine) {
                if success { println!("Using Redo"); }
                else { println!("Nothing to Redo"); }
            }
        }
    }

    // --- TRANSPORT ---

    pub fn play(&self) {
        if let Ok(mut eng) = self.engine.lock() {
            eng.play();
        }
    }

    pub fn pause(&self) {
        if let Ok(mut eng) = self.engine.lock() {
            eng.pause();
        }
    }

    pub fn toggle_play(&self) {
        if let Ok(mut eng) = self.engine.lock() {
            if eng.transport.playing {
                eng.pause();
            } else {
                eng.play();
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        if let Ok(eng) = self.engine.lock() {
            eng.transport.playing
        } else {
            false
        }
    }

    pub fn seek(&self, pos: Duration) {
        if let Ok(mut eng) = self.engine.lock() {
            eng.seek(pos);
        }
    }

    pub fn position(&self) -> Duration {
        if let Ok(eng) = self.engine.lock() {
            eng.transport.position
        } else {
            Duration::ZERO
        }
    }

    pub fn sample_rate(&self) -> u32 {
        if let Ok(eng) = self.engine.lock() {
            eng.sample_rate
        } else {
            44100
        }
    }

    pub fn add_track(&self, path: String) -> anyhow::Result<()> {
        if let Ok(mut eng) = self.engine.lock() {
            let _ = eng.add_track(path)?;
        }
        Ok(())
    }

    pub fn delete_track(&self, index: usize) -> anyhow::Result<()> {
        if let Ok(mut eng) = self.engine.lock() {
            eng.remove_track(index)?;
        }
        Ok(())
    }

    // --- ADD THIS NEW METHOD ---
    pub fn create_empty_track(&self) -> anyhow::Result<()> {
        if let Ok(mut eng) = self.engine.lock() {
            let id = eng.add_empty_track(); 
            
            // FIX: If the engine is currently playing/recording, 
            // force the new track to wake up and play.
            if eng.transport.playing {
                if let Some(track) = eng.tracks_mut().iter_mut().find(|t| t.id == id) {
                    track.set_state(crate::engine::track::TrackState::Playing);
                }
            }
        }
        Ok(())
    }

    pub fn add_clip(&self, track_index: usize, path: String, start_time: f64) -> anyhow::Result<()> {
        println!("➡️ Backend: Attempting to add clip to Track Index {}", track_index); // <--- DEBUG LOG
        
        if let Ok(mut eng) = self.engine.lock() {
            match eng.add_clip(track_index, path.clone(), start_time) {
                Ok(_) => println!("✅ Backend: Successfully added clip: {}", path),
                Err(e) => {
                    println!("❌ Backend: Failed to add clip! Error: {}", e);
                    return Err(e); // Pass error up
                }
            }
        }
        Ok(())
    }

    pub fn move_clip(&self, track_index: usize, clip_index: usize, new_start: f64) -> anyhow::Result<()> {
        let (track_id, old_start) = {
             let eng = self.engine.lock().unwrap();
             let track = eng.tracks().get(track_index).ok_or(anyhow::anyhow!("Track not found"))?;
             let clip = track.clips.get(clip_index).ok_or(anyhow::anyhow!("Clip not found"))?;
             (track.id, clip.start_time)
        };

        let cmd = Box::new(MoveClip {
            track_id,
            clip_index,
            old_start,
            new_start: Duration::from_secs_f64(new_start),
        });

        if let Ok(mut session) = self.session.lock() {
            session.apply(&self.engine, cmd)?;
        }
        Ok(())
    }

    pub fn split_clip(&self, track_index: usize, time: f64) -> anyhow::Result<()> {
        let track_id = {
             let eng = self.engine.lock().unwrap();
             eng.tracks().get(track_index).map(|t| t.id)
        }.ok_or(anyhow::anyhow!("Track not found"))?;
    
        let cmd = Box::new(SplitClip {
            track_id,
            split_time: Duration::from_secs_f64(time),
        });
    
        if let Ok(mut session) = self.session.lock() {
            session.apply(&self.engine, cmd)?;
        }
        Ok(())
    }

    // --- GLOBAL SETTINGS ---

    pub fn set_master_gain(&self, gain: f32) {
        if let Ok(mut g) = self.master_gain.lock() {
            *g = gain.clamp(0.0, 2.0);
        }
    }

    pub fn master_gain(&self) -> f32 {
        if let Ok(g) = self.master_gain.lock() {
            *g
        } else {
            1.0
        }
    }

    pub fn set_bpm(&self, bpm: f32) {
        if let Ok(mut eng) = self.engine.lock() {
            eng.set_bpm(bpm);
        }
    }

    pub fn bpm(&self) -> f32 {
        if let Ok(eng) = self.engine.lock() {
            eng.transport.tempo.bpm as f32
        } else {
            120.0
        }
    }

    pub fn get_grid_lines(&self, start: Duration, end: Duration, resolution: u32) -> Vec<GridLine> {
        if let Ok(eng) = self.engine.lock() {
            eng.transport.tempo.get_grid_lines(start, end, resolution)
        } else {
            Vec::new()
        }
    }

    // --- TRACK CONTROLS ---

    pub fn toggle_mute(&self, track_index: usize) {
        let (track_id, current_mute) = {
            let eng = self.engine.lock().unwrap();
            if let Some(t) = eng.tracks().get(track_index) {
                (t.id, t.muted)
            } else { return; }
        };

        let cmd = Box::new(SetTrackMute {
            track_id,
            new_state: !current_mute,
        });

        if let Ok(mut session) = self.session.lock() {
            let _ = session.apply(&self.engine, cmd);
            println!("Track {} mute toggled", track_index);
        }
    }

    // Non-destructive solo logic
    pub fn toggle_solo(&self, track_index: usize) {
        let track_id = {
            let eng = self.engine.lock().unwrap();
            eng.tracks().get(track_index).map(|t| t.id)
        };

        if let Some(tid) = track_id {
            let cmd = Box::new(ToggleSolo { track_id: tid });
            if let Ok(mut session) = self.session.lock() {
                let _ = session.apply(&self.engine, cmd);
                println!("Track {} solo toggled", track_index);
            }
        }
    }

    pub fn solo_track(&self, track_index: usize) {
        self.toggle_solo(track_index);
    }

    pub fn clear_solo(&self) {
        if let Ok(mut eng) = self.engine.lock() {
            for track in eng.tracks_mut().iter_mut() {
                track.solo = false;
                // Note: We don't clear muted here, only solo
            }
            println!("Solo cleared");
        }
    }

    // Absolute Gain Setter (for Sliders)
    pub fn set_track_gain(&self, track_index: usize, gain: f32) {
        let (track_id, old_gain) = {
            let eng = self.engine.lock().unwrap();
            if let Some(t) = eng.tracks().get(track_index) {
                (t.id, t.gain)
            } else { return; }
        };

        let cmd = Box::new(SetTrackGain {
            track_id,
            old_gain,
            new_gain: gain.clamp(0.0, 2.0),
        });

        if let Ok(mut session) = self.session.lock() {
            let _ = session.apply(&self.engine, cmd);
        }
    }

    // Absolute Pan Setter
    pub fn set_track_pan(&self, track_index: usize, pan: f32) {
        let (track_id, old_pan) = {
            let eng = self.engine.lock().unwrap();
            if let Some(t) = eng.tracks().get(track_index) {
                (t.id, t.pan)
            } else { return; }
        };

        let cmd = Box::new(SetTrackPan {
            track_id,
            old_pan,
            new_pan: pan.clamp(-1.0, 1.0),
        });

        if let Ok(mut session) = self.session.lock() {
            let _ = session.apply(&self.engine, cmd);
        }
    }

    // Relative Adjusters (Kept for Keyboard Shortcuts in daw_controller)
    pub fn adjust_track_gain(&self, track_index: usize, delta: f32) {
        let (track_id, old_gain) = {
            let eng = self.engine.lock().unwrap();
            if let Some(t) = eng.tracks().get(track_index) {
                (t.id, t.gain)
            } else { return; }
        };
        let new_gain = (old_gain + delta).clamp(0.0, 2.0);
        let cmd = Box::new(SetTrackGain { track_id, old_gain, new_gain });
        if let Ok(mut session) = self.session.lock() {
            let _ = session.apply(&self.engine, cmd);
        }
    }

    pub fn adjust_track_pan(&self, track_index: usize, delta: f32) {
        let (track_id, old_pan) = {
            let eng = self.engine.lock().unwrap();
            if let Some(t) = eng.tracks().get(track_index) {
                (t.id, t.pan)
            } else { return; }
        };
        let new_pan = (old_pan + delta).clamp(-1.0, 1.0);
        let cmd = Box::new(SetTrackPan { track_id, old_pan, new_pan });
        if let Ok(mut session) = self.session.lock() {
            let _ = session.apply(&self.engine, cmd);
        }
    }

    pub fn merge_clip_with_next(&self, track_index: usize, clip_index: usize) -> anyhow::Result<()> {
       if let Ok(mut eng) = self.engine.lock() {
           eng.merge_clip_with_next(track_index, clip_index)?;
       }
       Ok(())
    }

    pub fn delete_clip(&self, track_index: usize, clip_index: usize) -> anyhow::Result<()> {
        let (track_id, clip_data) = {
            let eng = self.engine.lock().unwrap();
            let track = eng.tracks().get(track_index).ok_or(anyhow::anyhow!("Track not found"))?;
            let clip = track.clips.get(clip_index).ok_or(anyhow::anyhow!("Clip not found"))?;
            
            let data = DeletedClipData {
                path: clip.path.clone(),
                start_time: clip.start_time,
                offset: clip.offset,
                duration: clip.duration,
                source_duration: clip.source_duration,
                source_sr: clip.source_sr,
                source_ch: clip.source_ch,
            };
            (track.id, data)
        };
    
        let cmd = Box::new(DeleteClip {
            track_id,
            clip_index,
            clip_data,
        });
        
        if let Ok(mut session) = self.session.lock() {
            session.apply(&self.engine, cmd)?;
        }
        Ok(())
    }

    // --- EQ COMMANDS ---

    pub fn update_eq(&self, track_index: usize, band_index: usize, params: EqParams) {
        let (track_id, old_params) = {
            let eng = self.engine.lock().unwrap();
            if let Some(track) = eng.tracks().get(track_index) {
                let current_state = track.track_eq.get_state(); 
                if band_index < current_state.len() {
                    (Some(track.id), Some(current_state[band_index]))
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        };

        if let (Some(tid), Some(old)) = (track_id, old_params) {
            let cmd = Box::new(UpdateEq {
                track_id: tid,
                band_index,
                old_params: old,
                new_params: params,
            });
            
            if let Ok(mut session) = self.session.lock() {
                let _ = session.apply(&self.engine, cmd);
            }
        }
    }

    pub fn get_eq_state(&self, track_index: usize) -> Vec<EqParams> {
        if let Ok(eng) = self.engine.lock() {
            if let Some(track) = eng.tracks().get(track_index) {
                return track.track_eq.get_state();
            }
        }
        Vec::new()
    }

    // FIX: Corrected Reset Methods (No Delta, Just Reset)
    pub fn reset_track_gain(&self, track_index: usize) {
        self.set_track_gain(track_index, 1.0);
    }

    pub fn reset_track_pan(&self, track_index: usize) {
        self.set_track_pan(track_index, 0.0);
    }

    // --- SAVE / LOAD / EXPORT (Primary for Main.rs) ---

    pub fn save_project(&self, path: String) -> Result<(), String> {
        let session = self.session.lock().map_err(|_| "Lock error")?;
        let master_gain = self.master_gain();
        session.save_project(&self.engine, &path, master_gain)
            .map_err(|e| e.to_string())
    }

    pub fn load_project(&self, path: String) -> Result<(), String> {
        let mut session = self.session.lock().map_err(|_| "Lock error")?;
        let new_master_gain = session.load_project(&self.engine, &path)
            .map_err(|e| e.to_string())?;
            
        if let Ok(mut g) = self.master_gain.lock() {
            *g = new_master_gain;
        }
        Ok(())
    }

    pub fn export_project(&self, path: String) -> Result<(), String> {
        // FIX: Rename session to _session to suppress unused variable warning
        let _session = self.session.lock().map_err(|_| "Lock error")?;
        let eng = self.engine.lock().unwrap();

        let tracks: Vec<crate::session::serialization::TrackState> = eng.tracks().iter().map(|t| {
            
            // 1. Map the clips first
            let clips = t.clips.iter().map(|c| crate::session::serialization::ClipState {
                path: c.path.clone(),
                start_time: c.start_time.as_secs_f64(),
                offset: c.offset.as_secs_f64(),
                duration: c.duration.as_secs_f64(),
            }).collect();

            // 2. Create the TrackState
            crate::session::serialization::TrackState {
                name: t.name.clone(), // Used to be 'path', now 'name'
                gain: t.gain,
                pan: t.pan,
                muted: t.muted,
                solo: t.solo,
                clips, // Add the list of clips
            }
        }).collect();

        let manifest = crate::session::serialization::ProjectManifest {
            version: 1,
            master_gain: eng.master_gain,
            bpm: eng.transport.tempo.bpm as f32,
            tracks,
        };

        crate::session::export::export_project_to_wav(&manifest, &path)
            .map_err(|e| e.to_string())
    }

    // --- COMPATIBILITY WRAPPERS (For daw_controller.rs) ---
    
    pub fn save_session(&self, path: String) -> anyhow::Result<()> {
        self.save_project(path).map_err(|e| anyhow::anyhow!(e))
    }

    pub fn load_session(&self, path: String) -> anyhow::Result<()> {
        self.load_project(path).map_err(|e| anyhow::anyhow!(e))
    }

    pub fn get_tracks_list(&self) -> Vec<FrontendTrackInfo> {
        if let Ok(eng) = self.engine.lock() {
            eng.tracks().iter().map(|t| {
                // Map the clips
                let clips = t.clips.iter().map(|c| FrontendClipInfo {
                    path: c.path.clone(),
                    start_time: c.start_time.as_secs_f64(),
                    duration: c.duration.as_secs_f64(),
                    offset: c.offset.as_secs_f64(),
                }).collect();

                FrontendTrackInfo {
                    id: t.id.0,
                    name: t.name.clone(),
                    gain: t.gain,
                    pan: t.pan,
                    muted: t.muted,
                    solo: t.solo,
                    clips, // <--- Add the clips here
                }
            }).collect()
        } else {
            Vec::new()
        }
    }       

    // --- DEBUG ---
    pub fn debug_snapshot(&self) -> Option<EngineSnapshot> {
        if let Ok(eng) = self.engine.lock() {
            let tracks = eng
                .tracks()
                .iter()
                .map(|t| TrackSnapshot {
                    gain: t.gain,
                    pan: t.pan,
                    muted: t.muted,
                    solo: t.solo,
                })
                .collect();
            Some(EngineSnapshot { tracks })
        } else {
            None
        }
    }

    pub fn set_track_name(&self, track_index: usize, name: String) {
        if let Ok(mut eng) = self.engine.lock() {
            if let Some(track) = eng.tracks_mut().get_mut(track_index) {
                track.name = name;
            }
        }
    }
}