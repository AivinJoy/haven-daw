// src/audio_runtime.rs

use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, SyncSender};
use std::sync::atomic::{Ordering};
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::Stream;

use crate::audio::setup_output_device;
use crate::engine::Engine;
use crate::session::{Session, commands::*}; 
use crate::engine::time::GridLine;
use crate::ai::ai_schema::{AiAction, EqFilterType as SchemaEqFilterType};
use crate::effects::equalizer::EqParams; // <--- Import this
use crate::effects::compressor::CompressorParams;
use crate::analyzer::AnalysisProfile;


// --- ADDED: The Lock-Free AI / UI Command Queue ---
pub enum EngineCommand {
    Play,
    Pause,
    TogglePlay,
    Seek(Duration),
    SetMasterGain(f32),
    SetBpm(f32),
    SetTimeSignature(u32, u32),
    ToggleMute(usize),
    SetTrackMute(usize, bool), // <--- NEW: Strict Mute/Unmute
    ToggleSolo(usize),
    ClearSolo,
    SetTrackGain(usize, f32),
    SetTrackPan(usize, f32),
    UpdateCompressor(usize, CompressorParams),
    UpdateEq(usize, usize, EqParams), // <--- NEW: Lock-Free EQ
    SetMonitor(crate::recorder::monitor::Monitor), // <--- NEW
    ClearMonitor, // <--- NEW
}


#[derive(serde::Serialize)]
pub struct MeterSnapshot {
    pub track_id: u32,
    pub peak_l: f32,
    pub peak_r: f32,
    pub hold_l: f32,
    pub hold_r: f32,
    pub rms_l: f32,
    pub rms_r: f32,
}

/// Owns Engine + CPAL stream and exposes a simple control API.
pub struct AudioRuntime {
    engine: Arc<Mutex<Engine>>,
    master_gain: Arc<Mutex<f32>>,
    session: Mutex<Session>,
    stream: Option<Stream>, // Changed to Option to allow hot-swapping
    command_tx: Mutex<SyncSender<EngineCommand>>, // Wrapped in Mutex to allow channel recreation
    // --- ADDED: A safe map of Track ID -> Lock-Free Atomics ---
    pub meter_registry: Arc<Mutex<std::collections::HashMap<u32, std::sync::Arc<crate::engine::metering::TrackMeters>>>>,
    pub master_meter: Arc<crate::engine::metering::TrackMeters>, // <--- CHANGED TYPE
    pub recorder: Arc<Mutex<Option<crate::recorder::Recorder>>>, // <--- NEW
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
    pub clip_number: usize,
}

pub struct FrontendTrackInfo {
    pub id: u32,
    pub name: String,
    pub color: String,
    pub gain: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub clips: Vec<FrontendClipInfo>,
    pub compressor: Option<CompressorParams>,
    pub eq: Option<Vec<EqParams>>,
    pub volume_automation: Vec<crate::engine::automation::AutomationNode<f32>>,
}

pub struct EngineSnapshot {
    pub tracks: Vec<TrackSnapshot>,
}

// --- NEW: Struct for AI Context ---
#[derive(serde::Serialize)]
pub struct TrackAnalysisPayload {
    pub track_id: u32,
    pub analysis: Option<AnalysisProfile>,
}

impl AudioRuntime {
    /// Create engine + output stream. Optionally add one initial track.
    pub fn new(initial_track: Option<String>) -> anyhow::Result<Self> {
        let master_gain = Arc::new(Mutex::new(1.0_f32));
        let mut engine = Engine::new(44100, 2); 

        if let Some(path) = initial_track {
            let _ = engine.add_track(path)?;
            engine.play();
        }

        let engine = Arc::new(Mutex::new(engine));
        let session = Mutex::new(Session::new());
        let (command_tx, command_rx) = mpsc::sync_channel::<EngineCommand>(1024);
        
        let recorder = Arc::new(Mutex::new(None::<crate::recorder::Recorder>));
        let master_meter = engine.lock().unwrap().master_meter.clone(); 
        let meter_registry = Arc::new(Mutex::new(std::collections::HashMap::new()));

        let mut runtime = Self {
            engine,
            master_gain,
            session,
            stream: None,
            command_tx: Mutex::new(command_tx), 
            meter_registry,
            master_meter,
            recorder,
        };

        if let Err(e) = runtime.build_and_start_stream(command_rx) {
            eprintln!("⚠️ Warning: Failed to hook default audio device on startup: {}", e);
        }

        Ok(runtime)
    }

    pub fn reload_device(&mut self) -> anyhow::Result<()> {
        println!("🔄 Reloading Audio Device...");
        self.stream = None; 
        let (new_tx, new_rx) = mpsc::sync_channel::<EngineCommand>(1024);
        
        if let Ok(mut tx_guard) = self.command_tx.lock() {
            *tx_guard = new_tx;
        }

        self.build_and_start_stream(new_rx)?;
        println!("✅ Audio Device Successfully Reloaded.");
        Ok(())
    }

    fn build_and_start_stream(&mut self, command_rx: mpsc::Receiver<EngineCommand>) -> anyhow::Result<()> {
        let output = setup_output_device()?;
        let sample_rate = output.output_sample_rate;
        let device_channels = output.output_channels;

        if let Ok(mut eng) = self.engine.lock() {
            eng.sample_rate = sample_rate;
        }

        println!("🔊 AudioRuntime: Device running at {} Hz with {} channels", sample_rate, device_channels);

        let device = output.device;
        let config = output.config;
        let engine_cb = self.engine.clone();
        let gain_cb = self.master_gain.clone();

        let mut scratch_buffer: Vec<f32> = Vec::with_capacity(1024);
        let mut live_scratch: Vec<f32> = Vec::with_capacity(1024);
        let mut active_monitor: Option<crate::recorder::monitor::Monitor> = None; 

        let err_fn = |err| eprintln!("AudioRuntime stream error: {}", err);

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                if let Ok(mut eng) = engine_cb.lock() {
                    while let Ok(cmd) = command_rx.try_recv() {
                        match cmd {
                            EngineCommand::SetMonitor(m) => active_monitor = Some(m),
                            EngineCommand::ClearMonitor => active_monitor = None,
                            EngineCommand::Play => eng.play(),
                            EngineCommand::Pause => eng.pause(),
                            EngineCommand::TogglePlay => {
                                if eng.transport.playing { eng.pause(); } else { eng.play(); }
                            }
                            EngineCommand::Seek(pos) => eng.seek(pos),
                            EngineCommand::SetMasterGain(g) => eng.master_gain = g,
                            EngineCommand::SetBpm(bpm) => eng.set_bpm(bpm),
                            EngineCommand::SetTimeSignature(num, den) => {
                                eng.transport.tempo.signature.numerator = num;
                                eng.transport.tempo.signature.denominator = den;
                            }
                            EngineCommand::ToggleMute(idx) => {
                                if let Some(t) = eng.tracks_mut().get_mut(idx) { t.muted = !t.muted; }
                            }
                            // --- NEW HANDLERS ---
                            EngineCommand::SetTrackMute(idx, state) => {
                                if let Some(t) = eng.tracks_mut().get_mut(idx) { t.muted = state; }
                            }
                            EngineCommand::UpdateEq(track_idx, band_idx, params) => {
                                if let Some(t) = eng.tracks_mut().get_mut(track_idx) {
                                    t.track_eq.update_band(band_idx, params);
                                }
                            }
                            EngineCommand::SetTrackGain(idx, gain) => {
                                if let Some(t) = eng.tracks_mut().get_mut(idx) { t.gain = gain; }
                            }
                            EngineCommand::SetTrackPan(idx, pan) => {
                                if let Some(t) = eng.tracks_mut().get_mut(idx) { t.pan = pan; }
                            }
                            EngineCommand::UpdateCompressor(idx, params) => {
                                if let Some(t) = eng.tracks_mut().get_mut(idx) {
                                    t.track_compressor.set_params(params);
                                }
                            }
                            EngineCommand::ToggleSolo(idx) => {
                                let target_id = eng.tracks().get(idx).map(|t| t.id);
                                if let Some(tid) = target_id {
                                    for t in eng.tracks_mut() {
                                        if t.id == tid { t.solo = !t.solo; }
                                    }
                                }
                            }
                            EngineCommand::ClearSolo => {
                                for t in eng.tracks_mut() { t.solo = false; }
                            }
                        }
                    }

                    if let Ok(g) = gain_cb.try_lock() {
                        eng.master_gain = *g;
                    }
                    
                    let frames = data.len() / device_channels as usize;
                    if scratch_buffer.len() != frames * 2 {
                        scratch_buffer.resize(frames * 2, 0.0);
                        live_scratch.resize(frames * 2, 0.0);
                    }
                    
                    live_scratch.fill(0.0);
                    if let Some(mon) = active_monitor.as_mut() {
                        mon.process_into(&mut live_scratch, 2);
                    }

                    eng.render(&mut scratch_buffer, &live_scratch);
                
                    let mut scratch_idx = 0;
                    for frame in data.chunks_mut(device_channels as usize) {
                        if frame.len() >= 2 {
                            frame[0] = scratch_buffer[scratch_idx];    
                            frame[1] = scratch_buffer[scratch_idx + 1]; 
                        }
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
        self.stream = Some(stream);
        Ok(())
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
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::Play);
    }

    pub fn pause(&self) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::Pause);
    }

    pub fn toggle_play(&self) {
       let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::TogglePlay);
    }

    pub fn is_playing(&self) -> bool {
        if let Ok(eng) = self.engine.lock() {
            eng.transport.playing
        } else {
            false
        }
    }

    pub fn seek(&self, pos: Duration) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::Seek(pos));
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

    // --- GLOBAL SETTINGS ---

    // --- GLOBAL SETTINGS ---

    pub fn get_master_meter(&self) -> (f32, f32, f32, f32) {
        // FIX: Pull hold_l/hold_r (decayed) instead of peak_l/peak_r (instant)
        let p_l = f32::from_bits(self.master_meter.hold_l.load(Ordering::Relaxed));
        let p_r = f32::from_bits(self.master_meter.hold_r.load(Ordering::Relaxed));
        let r_l = f32::from_bits(self.master_meter.rms_l.load(Ordering::Relaxed));
        let r_r = f32::from_bits(self.master_meter.rms_r.load(Ordering::Relaxed));
        (p_l, p_r, r_l, r_r)
    }

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
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::SetBpm(bpm));
    }

    // 3. ADD THIS PUBLIC METHOD
    pub fn set_time_signature(&self, numerator: u32, denominator: u32) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::SetTimeSignature(numerator, denominator));
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
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::ToggleMute(track_index));
    }

    // Non-destructive solo logic
    pub fn toggle_solo(&self, track_index: usize) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::ToggleSolo(track_index));
    }

    pub fn solo_track(&self, track_index: usize) {
        self.toggle_solo(track_index);
    }

    pub fn clear_solo(&self) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::ClearSolo);
    }

    // Absolute Gain Setter (for Sliders)
    pub fn set_track_gain(&self, track_index: usize, gain: f32) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::SetTrackGain(track_index, gain.clamp(0.0, 2.0)));
    }

    // Absolute Pan Setter
    pub fn set_track_pan(&self, track_index: usize, pan: f32) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::SetTrackPan(track_index, pan.clamp(-1.0, 1.0)));
    }

    pub fn set_monitor(&self, monitor: crate::recorder::monitor::Monitor) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::SetMonitor(monitor));
    }
    pub fn clear_monitor(&self) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::ClearMonitor);
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
        let (track_id, original_duration, right_clip_data) = {
            let eng = self.engine.lock().unwrap();
            let track = eng.tracks().get(track_index).ok_or(anyhow::anyhow!("Track not found"))?;
            
            let left = track.clips.get(clip_index).ok_or(anyhow::anyhow!("Left clip not found"))?;
            let right = track.clips.get(clip_index + 1).ok_or(anyhow::anyhow!("Right clip not found"))?;
            
            let right_data = DeletedClipData {
                path: right.path.clone(),
                start_time: right.start_time,
                offset: right.offset,
                duration: right.duration,
                source_duration: right.source_duration,
                source_sr: right.source_sr,
                source_ch: right.source_ch,
            };
            
            (track.id, left.duration, right_data)
        };

        let cmd = Box::new(crate::session::commands::MergeClip {
            track_id,
            clip_index,
            original_duration,
            right_clip_data,
        });

        if let Ok(mut session) = self.session.lock() {
            session.apply(&self.engine, cmd)?;
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

    // KEEP THIS ONE
    pub fn update_eq(&self, track_index: usize, band_index: usize, params: EqParams) {
        // PRO FIX: Send directly to the lock-free queue! Bypasses the Undo Session for AI adjustments.
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::UpdateEq(track_index, band_index, params));
    }

    // NEW Helper
    pub fn set_track_mute(&self, track_index: usize, state: bool) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::SetTrackMute(track_index, state));
    }

    pub fn get_eq_state(&self, track_index: usize) -> Vec<EqParams> {
        if let Ok(eng) = self.engine.lock() {
            if let Some(track) = eng.tracks().get(track_index) {
                return track.track_eq.get_state();
            }
        }
        Vec::new()
    }

    pub fn update_compressor(&self, track_index: usize, params: CompressorParams) {
        let _ = self.command_tx.lock().unwrap().try_send(EngineCommand::UpdateCompressor(track_index, params));
    }

    pub fn get_compressor_state(&self, track_index: usize) -> CompressorParams {
        if let Ok(eng) = self.engine.lock() {
            if let Some(track) = eng.tracks().get(track_index) {
                return track.track_compressor.get_params();
            }
        }
        // Fallback default if track isn't found
        CompressorParams {
            is_active: false,
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 50.0,
            makeup_gain_db: 0.0,
        }
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
                color: t.color.clone(),
                gain: t.gain,
                pan: t.pan,
                muted: t.muted,
                solo: t.solo,
                clips, // Add the list of clips
                volume_automation: t.volume_automation.clone(),
                compressor: Some(t.track_compressor.get_params()),
                eq: Some(t.track_eq.get_state()),
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

            // --- ADDED: Sync the registry quietly whenever the UI asks for track data ---
            if let Ok(mut reg) = self.meter_registry.lock() {
                reg.clear();
                for t in eng.tracks() {
                    reg.insert(t.id.0, t.meters.clone());
                }
            }

            eng.tracks().iter().map(|t| {
                // Map the clips
                let clips = t.clips.iter().map(|c| FrontendClipInfo {
                    path: c.path.clone(),
                    start_time: c.start_time.as_secs_f64(),
                    duration: c.duration.as_secs_f64(),
                    offset: c.offset.as_secs_f64(),
                    clip_number: c.clip_number, // <--- NEW
                }).collect();

                FrontendTrackInfo {
                    id: t.id.0,
                    name: t.name.clone(),
                    color: t.color.clone(),
                    gain: t.gain,
                    pan: t.pan,
                    muted: t.muted,
                    solo: t.solo,
                    clips, // <--- Add the clips here
                    compressor: Some(t.track_compressor.get_params()),
                    eq: Some(t.track_eq.get_state()),
                    volume_automation: t.volume_automation.nodes().to_vec(),
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

    
    // UPDATED: Now takes 'track_id: u32' instead of index
    pub fn set_clip_duration(&self, track_id: u32, duration: f64) -> Result<(), String> {
        if let Ok(mut eng) = self.engine.lock() {
            // Iterate to find the track with the matching ID
            if let Some(track) = eng.tracks_mut().iter_mut().find(|t| t.id.0 == track_id) {
                if let Some(clip) = track.clips.first_mut() {
                    clip.duration = Duration::from_secs_f64(duration);
                    return Ok(());
                } else {
                    return Err(format!("Track {} exists but has no clips (Empty Track)", track_id));
                }
            }
            return Err(format!("Track ID {} not found", track_id));
        }
        Err("Failed to lock engine".to_string())
    }

    pub fn get_volume_automation(&self, track_id: u32) -> Result<Vec<crate::engine::automation::AutomationNode<f32>>, String> {
        if let Ok(eng) = self.engine.lock() {
            if let Some(track) = eng.tracks().iter().find(|t| t.id.0 == track_id) {
                return Ok(track.volume_automation.nodes().to_vec());
            }
            return Err(format!("Track ID {} not found", track_id));
        }
        Err("Failed to lock engine".to_string())
    }

    pub fn add_volume_automation_node(&self, track_id: u32, time: u64, value: f32) -> Result<(), String> {
        if let Ok(mut eng) = self.engine.lock() {
            if let Some(track) = eng.tracks_mut().iter_mut().find(|t| t.id.0 == track_id) {
                track.volume_automation.insert_node(time, value);
                return Ok(());
            }
            return Err(format!("Track ID {} not found", track_id));
        }
        Err("Failed to lock engine".to_string())
    }

    pub fn remove_volume_automation_node(&self, track_id: u32, time: u64) -> Result<(), String> {
        if let Ok(mut eng) = self.engine.lock() {
            if let Some(track) = eng.tracks_mut().iter_mut().find(|t| t.id.0 == track_id) {
                track.volume_automation.remove_node_at_time(time);
                return Ok(());
            }
            return Err(format!("Track ID {} not found", track_id));
        }
        Err("Failed to lock engine".to_string())
    }

    // --- ADD NEW METHOD TO AUDIORUNTIME ---
    pub fn get_meters(&self) -> Vec<MeterSnapshot> {
        let mut results = Vec::new();
        // UI thread quickly locks the registry map
        if let Ok(reg) = self.meter_registry.lock() {
            for (&track_id, meters) in reg.iter() {
                results.push(MeterSnapshot {
                    track_id,
                    peak_l: f32::from_bits(meters.peak_l.load(std::sync::atomic::Ordering::Relaxed)),
                    peak_r: f32::from_bits(meters.peak_r.load(std::sync::atomic::Ordering::Relaxed)),
                    hold_l: f32::from_bits(meters.hold_l.load(std::sync::atomic::Ordering::Relaxed)),
                    hold_r: f32::from_bits(meters.hold_r.load(std::sync::atomic::Ordering::Relaxed)),
                    rms_l: f32::from_bits(meters.rms_l.load(std::sync::atomic::Ordering::Relaxed)),
                    rms_r: f32::from_bits(meters.rms_r.load(std::sync::atomic::Ordering::Relaxed)),
                });
            }
        }
        
        // 🐛 BUG FIX: HashMaps are unordered! We MUST sort them by track_id 
        // so the UI doesn't assign meters to random tracks every frame.
        results.sort_by_key(|m| m.track_id);
        
        results
    }

    // --- NEW: Expose offline analysis data for AI ---
    pub fn get_all_track_analysis(&self) -> Vec<TrackAnalysisPayload> {
        let mut results = Vec::new();
        if let Ok(eng) = self.engine.lock() {
            for t in eng.tracks() {
                // Safely lock the analysis profile. If it's still computing, it will return None.
                let profile = if let Ok(guard) = t.analysis.lock() {
                    guard.clone()
                } else {
                    None
                };
                
                results.push(TrackAnalysisPayload {
                    track_id: t.id.0,
                    analysis: profile,
                });
            }
        }
        results
    }

    // --- AI TRANSACTION BATCH EXECUTION ---
    // --- AI TRANSACTION BATCH EXECUTION ---
    pub fn apply_ai_batch(&self, commands: Vec<AiAction>) -> anyhow::Result<()> {
        
        // 🛠️ FIX: Helper to map stable track_id to the current array track_index
        let resolve = |tid: usize| -> Option<usize> {
            self.get_tracks_list().into_iter().position(|t| t.id == tid as u32)
        };

        for action in commands {
            match action {
                // 🚀 ALL MIXING COMMANDS NOW ROUTE THROUGH THE LOCK-FREE QUEUE
                AiAction::SetGain { track_id, value } => {
                    if let Some(idx) = resolve(track_id) { self.set_track_gain(idx, value); }
                },
                AiAction::SetMasterGain { value } => self.set_master_gain(value),
                AiAction::SetPan { track_id, value } => {
                    if let Some(idx) = resolve(track_id) { self.set_track_pan(idx, value); }
                },
                AiAction::ToggleMute { track_id } => {
                    if let Some(idx) = resolve(track_id) { self.toggle_mute(idx); }
                },
                AiAction::Unmute { track_id } => {
                    if let Some(idx) = resolve(track_id) { self.set_track_mute(idx, false); }
                },
                AiAction::ToggleSolo { track_id } => {
                    if let Some(idx) = resolve(track_id) { self.toggle_solo(idx); }
                },
                AiAction::Unsolo { track_id: _ } => self.clear_solo(),
                
                AiAction::SplitClip { track_id, time, clip_number: _ } => { 
                    if let Some(idx) = resolve(track_id) { let _ = self.split_clip(idx, time); }
                },
                AiAction::MergeClips { track_id, clip_number } => {
                    if let Some(idx) = resolve(track_id) {
                        let clip_idx = clip_number.saturating_sub(1); 
                        let _ = self.merge_clip_with_next(idx, clip_idx);
                    }
                },
                AiAction::DeleteClip { track_id, clip_number } => {
                    if let Some(idx) = resolve(track_id) {
                        let clip_idx = clip_number.saturating_sub(1);
                        let _ = self.delete_clip(idx, clip_idx);
                    }
                },
                AiAction::MoveClip { track_id, clip_number, new_time } => {
                    if let Some(idx) = resolve(track_id) {
                        let clip_idx = clip_number.saturating_sub(1);
                        let _ = self.move_clip(idx, clip_idx, new_time);
                    }
                },
                AiAction::SetBpm { bpm } => {
                    self.set_bpm(bpm);
                },
                AiAction::DeleteTrack { track_id } => { 
                    if let Some(idx) = resolve(track_id) { let _ = self.delete_track(idx); }
                },
                AiAction::CreateTrack { count: _, track_id: _ } => { let _ = self.create_empty_track(); },
                
                AiAction::UpdateEq { track_id, band_index, filter_type, freq, q, gain } => {
                    if let Some(idx) = resolve(track_id) {
                        let mapped_filter = match filter_type {
                            SchemaEqFilterType::Peaking => crate::effects::equalizer::EqFilterType::Peaking,
                            SchemaEqFilterType::LowShelf => crate::effects::equalizer::EqFilterType::LowShelf,
                            SchemaEqFilterType::HighShelf => crate::effects::equalizer::EqFilterType::HighShelf,
                            SchemaEqFilterType::LowPass => crate::effects::equalizer::EqFilterType::LowPass,
                            SchemaEqFilterType::HighPass => crate::effects::equalizer::EqFilterType::HighPass,
                        };
                        let params = crate::effects::equalizer::EqParams {
                            filter_type: mapped_filter,
                            freq: freq.clamp(20.0, 20_000.0),
                            q: q.clamp(0.1, 10.0),
                            gain: gain.clamp(-18.0, 18.0),
                            active: true,
                        };
                        self.update_eq(idx, band_index, params);
                    }
                },
                AiAction::UpdateCompressor { track_id, threshold_db, ratio, attack_ms, release_ms, makeup_gain_db } => {
                    if let Some(idx) = resolve(track_id) {
                        let params = crate::effects::compressor::CompressorParams {
                            is_active: true,
                            threshold_db: threshold_db.clamp(-60.0, 0.0),
                            ratio: ratio.clamp(1.0, 20.0),
                            attack_ms: attack_ms.clamp(0.1, 200.0),
                            release_ms: release_ms.clamp(10.0, 1000.0),
                            makeup_gain_db: makeup_gain_db.clamp(0.0, 24.0),
                        };
                        self.update_compressor(idx, params);
                    }
                },
                AiAction::SeparateStems { .. } => {
                    // Handled async by UI
                },
                AiAction::Undo => self.undo(),
                AiAction::Redo => self.redo(),
            }
        }

        Ok(())
    }

}