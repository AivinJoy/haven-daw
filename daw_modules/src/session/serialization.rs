use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use anyhow::Result;

// Represents a single audio clip within a track
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClipState {
    pub path: String,       // Source file path
    pub start_time: f64,    // Position on timeline (seconds)
    pub offset: f64,        // Start offset in the file (trimming)
    pub duration: f64,      // Playback duration (seconds)
}

#[derive(Serialize, Deserialize)]
pub struct TrackState {
    pub name: String,
    pub gain: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub clips: Vec<ClipState>,
}

#[derive(Serialize, Deserialize)]
pub struct ProjectManifest {
    pub version: u32,
    pub master_gain: f32,
    pub bpm: f32, // <--- NEW: Save the Global Tempo
    pub tracks: Vec<TrackState>,
}

impl ProjectManifest {
    pub fn save_to_disk(&self, path: &str) -> Result<()> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)?;
        Ok(())
    }

    pub fn load_from_disk(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let manifest = serde_json::from_reader(reader)?;
        Ok(manifest)
    }
}