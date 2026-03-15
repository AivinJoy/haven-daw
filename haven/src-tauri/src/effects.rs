use tauri::State;
use daw_modules::effects::reverb::ReverbParams;

// Assuming AppState and resolve_track_index are defined in main.rs or lib.rs and accessible via crate::
use crate::{AppState, resolve_track_index}; 

#[tauri::command]
pub fn set_effect_param(
    track_id: u32, 
    effect: String, 
    param: String, 
    value: f32, 
    state: State<'_, AppState>
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    let list = audio.get_tracks_list();
    
    // Resolve the frontend track_id to the internal engine index
    let index = resolve_track_index(&list, track_id)?;
    
    audio.set_effect_param(index, effect, param, value);
    Ok(())
}

#[tauri::command]
pub fn get_reverb_state(
    track_id: u32, 
    state: State<'_, AppState>
) -> Result<ReverbParams, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    let list = audio.get_tracks_list();
    
    let index = resolve_track_index(&list, track_id)?;

    Ok(audio.get_reverb_state(index))
}