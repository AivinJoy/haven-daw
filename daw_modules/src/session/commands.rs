// src/session/commands.rs

use crate::engine::{Engine, TrackId};
use anyhow::Result;
use crate::effects::equalizer::EqParams;
use crate::effects::compressor::CompressorParams;
use crate::effects::reverb::ReverbParams;
use std::time::Duration;

/// The Command trait defines an action that can be executed and undone.
/// We require Send + Sync so commands can be moved between threads if necessary.
pub trait Command: Send + Sync {
    /// Apply the change to the engine.
    fn execute(&self, engine: &mut Engine) -> Result<()>;
    
    /// Revert the change on the engine.
    fn undo(&self, engine: &mut Engine) -> Result<()>;
    
    /// A description for the UI (e.g., "Set Volume")
    fn name(&self) -> &str;
}

/// Manages the history of commands.
pub struct CommandManager {
    undo_stack: Vec<Box<dyn Command>>,
    redo_stack: Vec<Box<dyn Command>>,
    // We can set a max limit later to save memory, e.g., 50 steps.
    max_history: usize,
}

impl CommandManager {
    pub fn new(max_history: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_history,
        }
    }

    /// Execute a new command and push it onto the undo stack.
    /// Clears the redo stack because a new history branch is created.
    pub fn push(&mut self, command: Box<dyn Command>, engine: &mut Engine) -> Result<()> {
        command.execute(engine)?;
        self.undo_stack.push(command);
        self.redo_stack.clear();
        
        // Trim history if too long
        if self.undo_stack.len() > self.max_history {
            self.undo_stack.remove(0);
        }
        Ok(())
    }

    pub fn undo(&mut self, engine: &mut Engine) -> Result<bool> {
        if let Some(cmd) = self.undo_stack.pop() {
            cmd.undo(engine)?;
            self.redo_stack.push(cmd);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn redo(&mut self, engine: &mut Engine) -> Result<bool> {
        if let Some(cmd) = self.redo_stack.pop() {
            cmd.execute(engine)?;
            self.undo_stack.push(cmd);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    
    pub fn can_undo(&self) -> bool { !self.undo_stack.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo_stack.is_empty() }
}

// ==========================================
// CONCRETE COMMANDS
// ==========================================

pub struct SetTrackGain {
    pub track_id: TrackId,
    pub old_gain: f32,
    pub new_gain: f32,
}

impl Command for SetTrackGain {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.gain = self.new_gain;
        }
        Ok(())
    }

    fn undo(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.gain = self.old_gain;
        }
        Ok(())
    }

    fn name(&self) -> &str { "Change Gain" }
}

pub struct SetTrackPan {
    pub track_id: TrackId,
    pub old_pan: f32,
    pub new_pan: f32,
}

impl Command for SetTrackPan {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.pan = self.new_pan;
        }
        Ok(())
    }

    fn undo(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.pan = self.old_pan;
        }
        Ok(())
    }
    
    fn name(&self) -> &str { "Change Pan" }
}

pub struct SetTrackMute {
    pub track_id: TrackId,
    pub new_state: bool,
}

impl Command for SetTrackMute {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.muted = self.new_state;
        }
        Ok(())
    }

    fn undo(&self, engine: &mut Engine) -> Result<()> {
        // Toggle back
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.muted = !self.new_state;
        }
        Ok(())
    }
    
    fn name(&self) -> &str { "Toggle Mute" }
}

pub struct ToggleSolo {
    pub track_id: TrackId,
}

impl Command for ToggleSolo {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.solo = !track.solo;
        }
        Ok(())
    }
    fn undo(&self, engine: &mut Engine) -> Result<()> {
        self.execute(engine) // Toggle is its own undo
    }
    fn name(&self) -> &str { "Toggle Solo" }
}

pub struct MoveClip {
    pub track_id: TrackId,
    pub clip_index: usize,
    pub old_start: Duration,
    pub new_start: Duration,
}

impl Command for MoveClip {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.move_clip(self.clip_index, self.new_start);
        }
        Ok(())
    }
    fn undo(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.move_clip(self.clip_index, self.old_start);
        }
        Ok(())
    }
    fn name(&self) -> &str { "Move Clip" }
}

// Data needed to restore a clip
pub struct DeletedClipData {
    pub path: String,
    pub start_time: Duration,
    pub offset: Duration,
    pub duration: Duration,
    pub source_duration: Duration,
    pub source_sr: u32,
    pub source_ch: usize,
}

pub struct DeleteClip {
    pub track_id: TrackId,
    pub clip_index: usize,
    pub clip_data: DeletedClipData, // Saved state for Undo
}

impl Command for DeleteClip {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.delete_clip(self.clip_index)?;
        }
        Ok(())
    }
    fn undo(&self, engine: &mut Engine) -> Result<()> {
        let sr = engine.sample_rate;
        let ch = engine.channels;

        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.restore_deleted_clip( // <--- CHANGED HERE
                self.clip_index,
                self.clip_data.path.clone(),
                self.clip_data.start_time,
                self.clip_data.offset,
                self.clip_data.duration,
                self.clip_data.source_duration, // <--- ADDED
                self.clip_data.source_sr,       // <--- ADDED
                self.clip_data.source_ch,       // <--- ADDED
                sr,
                ch
            )?;
        }
        Ok(())
    }
    fn name(&self) -> &str { "Delete Clip" }
}

pub struct SplitClip {
    pub track_id: TrackId,
    pub split_time: Duration,
}

impl Command for SplitClip {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
         // FIX: Capture variables before borrowing tracks_mut()
         let sr = engine.sample_rate;
         let ch = engine.channels;
         
         if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
             track.split_at_time(self.split_time, sr, ch)?;
         }
         Ok(())
    }
    fn undo(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            // Find the clip that ends at split_time (the left half)
            // merge_next takes the index of the LEFT clip.
            // We need to find index `i` where `clips[i].end == split_time`
            
            let eps = 0.001;
            let split_secs = self.split_time.as_secs_f64();
            
            if let Some(idx) = track.clips.iter().position(|c| {
                let end = c.start_time.as_secs_f64() + c.duration.as_secs_f64();
                (end - split_secs).abs() < eps
            }) {
                track.merge_next(idx)?;
            }
        }
        Ok(())
    }
    fn name(&self) -> &str { "Split Clip" }
}

pub struct MergeClip {
    pub track_id: TrackId,
    pub clip_index: usize,
    pub original_duration: Duration,
    pub right_clip_data: DeletedClipData,
}

impl Command for MergeClip {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.merge_next(self.clip_index)?;
        }
        Ok(())
    }
    
    fn undo(&self, engine: &mut Engine) -> Result<()> {
        let sr = engine.sample_rate;
        let ch = engine.channels;

        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            // 1. Restore left clip's original duration
            if let Some(left) = track.clips.get_mut(self.clip_index) {
                left.duration = self.original_duration;
            }
            
            // 2. Restore the deleted right clip
            track.restore_deleted_clip( // <--- CHANGED HERE
                self.clip_index + 1,
                self.right_clip_data.path.clone(),
                self.right_clip_data.start_time,
                self.right_clip_data.offset,
                self.right_clip_data.duration,
                self.right_clip_data.source_duration, // <--- ADDED
                self.right_clip_data.source_sr,       // <--- ADDED
                self.right_clip_data.source_ch,       // <--- ADDED
                sr,
                ch
            )?;
        }
        Ok(())
    }
    
    fn name(&self) -> &str { "Merge Clips" }
}

pub struct UpdateEq {
    pub track_id: TrackId,
    pub band_index: usize,
    pub old_params: EqParams,
    pub new_params: EqParams,
}

impl Command for UpdateEq {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.track_eq.update_band(self.band_index, self.new_params);
        }
        Ok(())
    }
    fn undo(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.track_eq.update_band(self.band_index, self.old_params);
        }
        Ok(())
    }
    fn name(&self) -> &str { "EQ Change" }
}

pub struct UpdateCompressor {
    pub track_id: TrackId,
    pub old_params: CompressorParams,
    pub new_params: CompressorParams,
}

impl Command for UpdateCompressor {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.track_compressor.set_params(self.new_params);
        }
        Ok(())
    }
    fn undo(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.track_compressor.set_params(self.old_params);
        }
        Ok(())
    }
    fn name(&self) -> &str { "Compressor Change" }
}

pub struct UpdateReverb {
    pub track_id: TrackId,
    pub old_params: ReverbParams,
    pub new_params: ReverbParams,
}

impl Command for UpdateReverb {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            // Assuming your Track struct has a track_reverb field similar to track_compressor
            track.track_reverb.set_params(self.new_params);
        }
        Ok(())
    }
    
    fn undo(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.track_reverb.set_params(self.old_params);
        }
        Ok(())
    }
    
    fn name(&self) -> &str { "Reverb Change" }
}

/// ==========================================
// AUTOMATION COMMANDS
// ==========================================

pub struct ClearVolumeAutomationCmd {
    pub track_id: TrackId,
}

impl Command for ClearVolumeAutomationCmd {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            track.volume_automation.clear(); 
        }
        Ok(())
    }
    fn undo(&self, _engine: &mut Engine) -> Result<()> {
        Ok(())
    }
    fn name(&self) -> &str { "Clear Volume Automation" }
}

pub struct AddVolumeAutomationCmd {
    pub track_id: TrackId,
    pub time: f64,
    pub value: f32,
}

impl Command for AddVolumeAutomationCmd {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        // 1. EXTRACT FIRST (Before the mutable borrow)
        let sr = engine.sample_rate as f64; 
        
        // 2. MUTABLY BORROW TRACKS
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            let time_in_samples = (self.time * sr) as u64;
            track.volume_automation.insert_node(time_in_samples, self.value);
        }
        Ok(())
    }
    fn undo(&self, _engine: &mut Engine) -> Result<()> {
        Ok(())
    }
    fn name(&self) -> &str { "Add Automation Node" }
}

pub struct DuckVolumeCmd {
    pub track_id: TrackId,
    pub time: f64,
    pub depth_db: f32,
}

impl Command for DuckVolumeCmd {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        // 1. EXTRACT FIRST
        let sr = engine.sample_rate as f64; 
        
        // 2. MUTABLY BORROW TRACKS
        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            
            // Create a quick "V" shape dip for ducking plosives/peaks
            let t_start = ((self.time - 0.015).max(0.0) * sr) as u64; // 15ms before peak
            let t_center = (self.time * sr) as u64;                   // At the peak
            let t_end = ((self.time + 0.050) * sr) as u64;            // 50ms recovery

            // Drop from 0dB, hit the depth, and recover to 0dB
            track.volume_automation.insert_node(t_start, 0.0);
            track.volume_automation.insert_node(t_center, self.depth_db);
            track.volume_automation.insert_node(t_end, 0.0);
        }
        Ok(())
    }
    fn undo(&self, _engine: &mut Engine) -> Result<()> { Ok(()) }
    fn name(&self) -> &str { "Duck Peak Volume" }
}

pub struct RideVocalLevelCmd {
    pub track_id: TrackId,
    pub target_lufs: f32,
    pub max_boost_db: f32,
    pub max_cut_db: f32,
    pub smoothness: f32,
    pub analysis_window_ms: u32,
    pub noise_floor_db: f32,
}

impl Command for RideVocalLevelCmd {
    fn execute(&self, engine: &mut Engine) -> Result<()> {
        let engine_sr = engine.sample_rate as f64;

        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            
            // 1. Clone clip metadata to avoid Rust borrowing issues while we decode heavily
            let clips_info: Vec<_> = track.clips.iter().map(|c| {
                (
                    c.path.clone(), 
                    c.start_time.as_secs_f64(), 
                    c.offset.as_secs_f64(), 
                    c.duration.as_secs_f64()
                )
            }).collect();

            for (path, start_sec, offset_sec, dur_sec) in clips_info {
                // 2. Decode the raw audio file (Offline Background Processing)
                if let Ok((samples, source_sr, source_ch)) = crate::bpm::adapter::decode_to_vec(&path) {
                    
                    // 3. Slice the buffer to strictly match the clip's trim (offset) and length
                    let start_sample_idx = (offset_sec * source_sr as f64) as usize * source_ch;
                    let end_sample_idx = ((offset_sec + dur_sec) * source_sr as f64) as usize * source_ch;

                    let active_samples = if end_sample_idx <= samples.len() {
                        &samples[start_sample_idx..end_sample_idx]
                    } else if start_sample_idx < samples.len() {
                        &samples[start_sample_idx..]
                    } else {
                        &[]
                    };

                    // 4. Generate the automation nodes for this specific trimmed slice
                    let nodes = crate::engine::automation::generate_rider_automation(
                        active_samples,
                        source_ch,
                        source_sr,
                        start_sec, // Use the Timeline position so the nodes draw in the right place!
                        self.target_lufs,
                        self.max_boost_db,
                        self.max_cut_db,
                        self.smoothness,
                        self.analysis_window_ms,
                        self.noise_floor_db,
                    );

                    // 5. Insert nodes into the track, translating Source Hz to Engine Hz to prevent drift
                    for node in nodes {
                        let time_in_engine_samples = (node.time as f64 / source_sr as f64 * engine_sr) as u64;
                        track.volume_automation.insert_node(time_in_engine_samples, node.value);
                    }
                }
            }
        }
        Ok(())
    }
    
    fn undo(&self, _engine: &mut Engine) -> Result<()> { Ok(()) }
    fn name(&self) -> &str { "Vocal Rider" }
}