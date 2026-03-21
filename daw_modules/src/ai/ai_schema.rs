// daw_modules/src/ai/ai_schema.rs

use serde::{Deserialize, Serialize};
use std::fmt;

/// The top-level payload envelope.
/// #[serde(deny_unknown_fields)] ensures the AI cannot sneak in extra root keys.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AiCommandEnvelope {
    pub version: String,
    pub commands: Vec<AiAction>,
    // We allow message and confidence to pass through, 
    // but they are ignored by DSP.
    pub message: Option<String>,
    pub confidence: Option<f32>,
}

/// The Strict Enum of Actions.
/// `tag = "action"` tells Serde to look at the "action" JSON key to determine which struct to build.
/// `rename_all = "snake_case"` ensures JSON matches Rust conventions.
/// `deny_unknown_fields` strictly rejects hallucinated parameters inside the command.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AiAction {
    SetGain {
        track_id: usize,
        value: f32, // Must be provided, must be a float. No strings.
    },
    SetMasterGain {
        value: f32,
    },
    SetPan {
        track_id: usize,
        value: f32,
    },
    ToggleMute {
        track_id: usize,
    },
    Unmute { 
        track_id: usize 
    }, // <--- ADDED
    ToggleSolo {
        track_id: usize,
    },
    Unsolo { 
        track_id: usize 
    },
    SplitClip {
        track_id: usize,
        time: f64, // Required
        clip_number: Option<usize>,
    },
    MergeClips {
        track_id: usize,
        clip_number: usize,
    },
    DeleteClip {
        track_id: usize,
        clip_number: usize,
    },
    DeleteTrack {
        track_id: usize,
    },
    SeparateStems {          // <--- ADD THIS
        track_id: usize,
    },
    MoveClip {
        track_id: usize,
        clip_number: usize,
        new_time: f64, 
    },
    SetBpm {
        bpm: f32,
    },
    CreateTrack {
        // Only logically optional fields get Option<>
        count: Option<usize>, 
        track_id: Option<usize>,
    },
    UpdateEq {
        track_id: usize,
        band_index: usize,
        filter_type: EqFilterType, // Strict sub-enum
        freq: f32,
        q: f32,
        gain: f32,
    },
    UpdateCompressor {
        track_id: usize,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_gain_db: f32,
    },
    UpdateReverb { // <--- ADD THIS BLOCK
        track_id: usize,
        room_size: Option<f32>,
        damping: Option<f32>,
        pre_delay_ms: Option<f32>,
        mix: Option<f32>,
        width: Option<f32>,
        low_cut_hz: Option<f32>,
        high_cut_hz: Option<f32>,
        is_active: Option<bool>,
    },
    ClearVolumeAutomation {
        track_id: usize,
    },
    AddVolumeAutomation {
        track_id: usize,
        time: f64,  // Exact time in seconds
        value: f32, // STRICTLY IN dB (-inf to +12.0)
    },
    DuckVolume {
        track_id: usize,
        time: f64,
        depth_db: f32,
    },
    RideVocalLevel {               // <--- NEW ACTION ADDED
        track_id: usize,
        target_lufs: f32,
        max_boost_db: Option<f32>, 
        max_cut_db: Option<f32>,   
        smoothness: Option<f32>,   
        analysis_window_ms: Option<u32>, // Brilliant addition
        noise_floor_db: Option<f32>,
    },
    Undo,
    Redo,
}

/// Strict sub-enum for EQ Filter Types.
/// Rejects anything other than these exact strings.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum EqFilterType {
    Peaking,
    LowShelf,
    HighShelf,
    HighPass,
    LowPass,
    Notch,
    BandPass,
}

// Custom Error Type for Schema Validation
#[derive(Debug)]
pub enum SchemaError {
    UnsupportedVersion(String),
    ParseError(String),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { // <--- FIXED
        match self {
            SchemaError::UnsupportedVersion(v) => write!(f, "Unsupported AI Contract Version: {}", v),
            SchemaError::ParseError(msg) => write!(f, "AI Payload Parse Error: {}", msg),
        }
    }
}

impl std::error::Error for SchemaError {}

/// The pure validation function. No DSP logic here.
/// It only accepts or rejects.
pub fn validate_payload(raw_json: &str) -> Result<AiCommandEnvelope, SchemaError> {
    // 1. Enforce strict JSON parsing (This handles unknown fields, missing fields, type mismatches)
    let payload: AiCommandEnvelope = serde_json::from_str(raw_json)
        .map_err(|e| SchemaError::ParseError(e.to_string()))?;

    // 2. Enforce Version Contract
    if payload.version != "1.0" {
        return Err(SchemaError::UnsupportedVersion(payload.version));
    }

    Ok(payload)
}