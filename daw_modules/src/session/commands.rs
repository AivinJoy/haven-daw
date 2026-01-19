// src/session/commands.rs

use crate::engine::{Engine, TrackId};
use anyhow::Result;
use crate::effects::equalizer::EqParams;
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
        // FIX: Capture variables before borrowing tracks_mut()
        let sr = engine.sample_rate;
        let ch = engine.channels;

        if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == self.track_id) {
            // NOTE: This call will fail to compile UNTIL you update track.rs in the next step.
            track.restore_clip(
                self.clip_index,
                self.clip_data.path.clone(),
                self.clip_data.start_time,
                self.clip_data.offset,
                self.clip_data.duration,
                self.clip_data.source_duration,
                self.clip_data.source_sr,
                self.clip_data.source_ch,
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