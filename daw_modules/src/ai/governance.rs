// daw_modules/src/ai/governance.rs

use super::ai_schema::{AiAction, EqFilterType as SchemaEqFilterType};
use crate::session::commands::*; 
use crate::effects::equalizer::{EqParams, EqFilterType as CoreEqFilterType};
use crate::effects::compressor::CompressorParams;
use crate::engine::TrackId; 
use std::time::Duration;

#[derive(Debug)]
pub enum GovernanceError {
    InvalidParameter(String),
}

/// The Rules Engine: Translates validated AI Actions into safe DSP Commands.
pub fn translate_action(action: AiAction) -> Result<Box<dyn Command>, GovernanceError> {
    match action {
        AiAction::SetGain { track_id, value } => {
            let safe_gain = value.clamp(0.0, 2.0);
            
            Ok(Box::new(SetTrackGain {
                track_id: TrackId(track_id as u32), // <--- Cast to u32
                old_gain: 1.0, 
                new_gain: safe_gain,
            }))
        }

        AiAction::SetMasterGain { value } => {
            let safe_master = value.clamp(0.0, 1.5);
            Ok(Box::new(SetTrackGain { 
                track_id: TrackId(999), // Master bus placeholder
                old_gain: 1.0, 
                new_gain: safe_master 
            }))
        }

        AiAction::SetPan { track_id, value } => {
            let safe_pan = value.clamp(-1.0, 1.0);
            Ok(Box::new(SetTrackPan {
                track_id: TrackId(track_id as u32), // <--- Cast to u32
                old_pan: 0.0,
                new_pan: safe_pan,
            }))
        }

        AiAction::ToggleMute { track_id } => {
            Ok(Box::new(SetTrackMute {
                track_id: TrackId(track_id as u32), // <--- Cast to u32
                new_state: true, 
            }))
        }

        AiAction::ToggleSolo { track_id } => {
            Ok(Box::new(ToggleSolo { 
                track_id: TrackId(track_id as u32) // <--- Cast to u32
            }))
        }

        AiAction::SplitClip { track_id, time } => {
            if time < 0.0 {
                return Err(GovernanceError::InvalidParameter("Split time cannot be negative".into()));
            }
            Ok(Box::new(SplitClip {
                track_id: TrackId(track_id as u32), // <--- Cast to u32
                split_time: Duration::from_secs_f64(time),
            }))
        }

        AiAction::UpdateEq { track_id, band_index, filter_type, freq, q, gain } => {
            let safe_freq = freq.clamp(20.0, 20_000.0);
            let safe_q = q.clamp(0.1, 10.0);
            let safe_gain = gain.clamp(-18.0, 18.0);

            let mapped_filter = match filter_type {
                SchemaEqFilterType::Peaking => CoreEqFilterType::Peaking,
                SchemaEqFilterType::LowShelf => CoreEqFilterType::LowShelf,
                SchemaEqFilterType::HighShelf => CoreEqFilterType::HighShelf,
                SchemaEqFilterType::LowPass => CoreEqFilterType::LowPass,
                SchemaEqFilterType::HighPass => CoreEqFilterType::HighPass,
            };

            let new_params = EqParams {
                filter_type: mapped_filter,
                freq: safe_freq,
                q: safe_q,
                gain: safe_gain,
                active: true,
            };

            Ok(Box::new(UpdateEq {
                track_id: TrackId(track_id as u32), // <--- Cast to u32
                band_index,
                old_params: new_params.clone(), 
                new_params,
            }))
        }

        AiAction::UpdateCompressor { track_id, threshold_db, ratio, attack_ms, release_ms, makeup_gain_db } => {
            let safe_thresh = threshold_db.clamp(-60.0, 0.0);
            let safe_ratio = ratio.clamp(1.0, 20.0);
            let safe_attack = attack_ms.clamp(0.1, 200.0);
            let safe_release = release_ms.clamp(10.0, 1000.0);
            let safe_makeup = makeup_gain_db.clamp(0.0, 24.0);

            let new_params = CompressorParams {
                is_active: true,
                threshold_db: safe_thresh,
                ratio: safe_ratio,
                attack_ms: safe_attack,
                release_ms: safe_release,
                makeup_gain_db: safe_makeup,
            };

            Ok(Box::new(UpdateCompressor {
                track_id: TrackId(track_id as u32), // <--- Cast to u32
                old_params: new_params.clone(),
                new_params,
            }))
        }

        AiAction::DeleteTrack { track_id: _ } => {
            Err(GovernanceError::InvalidParameter("Delete track not fully implemented".into()))
        }
        
        _ => Err(GovernanceError::InvalidParameter("Command translation not yet mapped".into()))
    }
}