// daw_modules/src/ai/governance.rs

use super::ai_schema::{AiAction, EqFilterType as SchemaEqFilterType};
use crate::session::commands::*; 
use crate::effects::equalizer::{EqParams, EqFilterType as CoreEqFilterType};
use crate::effects::compressor::CompressorParams;
use crate::effects::reverb::ReverbParams;
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

        AiAction::SplitClip { track_id, time, clip_number: _ } => {
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
                SchemaEqFilterType::Notch => CoreEqFilterType::Notch,
                SchemaEqFilterType::BandPass => CoreEqFilterType::BandPass,
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

        AiAction::UpdateReverb { track_id, room_size, damping, pre_delay_ms, mix, width, low_cut_hz, high_cut_hz, is_active } => {
            // 1. Unwrap AI optionals with safe default studio settings
            // 2. Clamp strictly to DSP engine limits
            let safe_room = room_size.unwrap_or(0.8).clamp(0.0, 1.0);
            let safe_damp = damping.unwrap_or(0.5).clamp(0.0, 1.0);
            let safe_pre = pre_delay_ms.unwrap_or(10.0).clamp(0.0, 500.0);
            let safe_mix = mix.unwrap_or(0.3).clamp(0.0, 1.0);
            let safe_width = width.unwrap_or(1.0).clamp(0.0, 1.0);
            let safe_low = low_cut_hz.unwrap_or(100.0).clamp(20.0, 1000.0);
            let safe_high = high_cut_hz.unwrap_or(8000.0).clamp(1000.0, 20000.0);
            let active = is_active.unwrap_or(true); // If AI calls this, assume they want it ON

            let new_params = ReverbParams {
                is_active: active,
                room_size: safe_room,
                damping: safe_damp,
                pre_delay_ms: safe_pre,
                mix: safe_mix,
                width: safe_width,
                low_cut_hz: safe_low,
                high_cut_hz: safe_high,
            };

            Ok(Box::new(UpdateReverb {
                track_id: TrackId(track_id as u32),
                old_params: new_params.clone(), 
                new_params,
            }))
        }

        AiAction::ClearVolumeAutomation { track_id } => {
            Ok(Box::new(ClearVolumeAutomationCmd {
                track_id: TrackId(track_id as u32),
            }))
        }

        AiAction::AddVolumeAutomation { track_id, time, value } => {
            let safe_value = value.clamp(-60.0, 12.0); // Don't let AI blow out speakers
            Ok(Box::new(AddVolumeAutomationCmd {
                track_id: TrackId(track_id as u32),
                time,
                value: safe_value,
            }))
        }

        AiAction::DuckVolume { track_id, time, depth_db } => {
            let safe_depth = depth_db.clamp(-60.0, 0.0); // Ducking can only reduce volume
            Ok(Box::new(DuckVolumeCmd {
                track_id: TrackId(track_id as u32),
                time,
                depth_db: safe_depth,
            }))
        }

        AiAction::RideVocalLevel { track_id, target_lufs, max_boost_db, max_cut_db, smoothness, analysis_window_ms, noise_floor_db } => {
            // 1. Unwrap AI optionals with safe default studio settings
            // 2. Clamp strictly to DSP engine limits
            let safe_target = target_lufs.clamp(-36.0, 0.0);
            let safe_boost = max_boost_db.unwrap_or(6.0).clamp(0.0, 24.0);
            let safe_cut = max_cut_db.unwrap_or(-12.0).clamp(-60.0, 0.0);
            let safe_smooth = smoothness.unwrap_or(0.5).clamp(0.0, 1.0);
            let safe_window = analysis_window_ms.unwrap_or(300).clamp(50, 2000);
            let safe_noise = noise_floor_db.unwrap_or(-60.0).clamp(-100.0, 0.0);

            Ok(Box::new(RideVocalLevelCmd {
                track_id: TrackId(track_id as u32),
                target_lufs: safe_target,
                max_boost_db: safe_boost,
                max_cut_db: safe_cut,
                smoothness: safe_smooth,
                analysis_window_ms: safe_window,
                noise_floor_db: safe_noise,
            }))
        }

        AiAction::DeleteTrack { track_id: _ } => {
            Err(GovernanceError::InvalidParameter("Delete track not fully implemented".into()))
        }
        
        // 🛠️ DEBUG LOG 6: THE REJECTION WALL (Update your catch-all)
        unmapped_action => {
            eprintln!("❌ [GOVERNANCE REJECTED] Command not mapped to DSP: {:?}", unmapped_action);
            Err(GovernanceError::InvalidParameter(format!("Command translation not yet mapped: {:?}", unmapped_action)))
        }
    }
}