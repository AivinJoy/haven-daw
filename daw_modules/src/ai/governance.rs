use super::ai_schema::AiAction;

#[derive(Debug)]
pub enum GovernanceError {
    InvalidParameter(String),
}

/// The Rules Engine: Enforces safe DSP limits by modifying actions in place.
/// It validates and clamps parameters but DOES NOT reject valid structural or UI commands.
pub fn enforce_limits(action: &mut AiAction) {
    match action {
        AiAction::SetGain { value, .. } => {
            *value = value.clamp(0.0, 2.0);
        }
        AiAction::SetMasterGain { value } => {
            *value = value.clamp(0.0, 1.5);
        }
        AiAction::SetPan { value, .. } => {
            *value = value.clamp(-1.0, 1.0);
        }
        AiAction::SplitClip { time, .. } => {
            *time = time.max(0.0);
        }
        AiAction::MoveClip { new_time, .. } => {
            *new_time = new_time.max(0.0);
        }
        AiAction::UpdateEq { freq, q, gain, is_active, .. } => {
            *freq = freq.clamp(20.0, 20_000.0);
            *q = q.clamp(0.1, 10.0);
            *gain = gain.clamp(-18.0, 18.0);
            if is_active.is_none() { *is_active = Some(true); }
        }
        AiAction::UpdateCompressor { threshold_db, ratio, attack_ms, release_ms, makeup_gain_db, is_active, .. } => {
            *threshold_db = threshold_db.clamp(-60.0, 0.0);
            *ratio = ratio.clamp(1.0, 20.0);
            *attack_ms = attack_ms.clamp(0.1, 200.0);
            *release_ms = release_ms.clamp(10.0, 1000.0);
            *makeup_gain_db = makeup_gain_db.clamp(0.0, 24.0);
            if is_active.is_none() { *is_active = Some(true); }
        }
        AiAction::UpdateReverb { room_size, damping, pre_delay_ms, mix, width, low_cut_hz, high_cut_hz, is_active, .. } => {
            *room_size = Some(room_size.unwrap_or(0.8).clamp(0.0, 1.0));
            *damping = Some(damping.unwrap_or(0.5).clamp(0.0, 1.0));
            *pre_delay_ms = Some(pre_delay_ms.unwrap_or(10.0).clamp(0.0, 500.0));
            *mix = Some(mix.unwrap_or(0.3).clamp(0.0, 1.0));
            *width = Some(width.unwrap_or(1.0).clamp(0.0, 1.0));
            *low_cut_hz = Some(low_cut_hz.unwrap_or(100.0).clamp(20.0, 1000.0));
            *high_cut_hz = Some(high_cut_hz.unwrap_or(8000.0).clamp(1000.0, 20000.0));
            if is_active.is_none() { *is_active = Some(true); }
        }
        
        AiAction::AddVolumeAutomation { value, .. } => {
            *value = value.clamp(-60.0, 12.0);
        }
        AiAction::DuckVolume { depth_db, .. } => {
            *depth_db = depth_db.clamp(-60.0, 0.0);
        }
        
        AiAction::RideVocalLevel { target_lufs, max_boost_db, max_cut_db, smoothness, analysis_window_ms, noise_floor_db, preserve_dynamics, .. } => {
            *target_lufs = Some(target_lufs.unwrap_or(-16.0).clamp(-36.0, 0.0));
            *max_boost_db = Some(max_boost_db.unwrap_or(4.0).clamp(0.0, 24.0));
            *max_cut_db = Some(max_cut_db.unwrap_or(-12.0).clamp(-60.0, 0.0));
            *smoothness = Some(smoothness.unwrap_or(0.85).clamp(0.0, 1.0));
            *analysis_window_ms = Some(analysis_window_ms.unwrap_or(300).clamp(50, 2000));
            *noise_floor_db = Some(noise_floor_db.unwrap_or(-60.0).clamp(-100.0, 0.0));
            if preserve_dynamics.is_none() { *preserve_dynamics = Some(true); }
        }
        AiAction::AutoGainStage { target_lufs, .. } => {
            *target_lufs = target_lufs.clamp(-36.0, 0.0);
        }
        // UI & System Commands (CreateTrack, SetBpm, etc.) pass through harmlessly.
        // Translation to Commands happens downstream in the executor.
        _ => {}
    }
}