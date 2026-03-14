use crate::AppState; // Adjust this path to where your state type is defined
use daw_modules::engine::automation::AutomationNode;
use tauri::State;

#[tauri::command]
pub fn get_volume_automation(
    state: State<'_, SharedAudioEngine>,
    track_id: String,
) -> Result<Vec<AutomationNode<f32>>, String> {
    let engine = state.lock().map_err(|e| e.to_string())?;
    
    if let Some(track) = engine.tracks().iter().find(|t| t.id == track_id) {
        Ok(track.volume_automation.nodes().to_vec())
    } else {
        Err(format!("Track {} not found", track_id))
    }
}

#[tauri::command]
pub fn add_volume_automation_node(
    state: State<'_, SharedAudioEngine>,
    track_id: String,
    time: u64,
    value: f32,
) -> Result<(), String> {
    let mut engine = state.lock().map_err(|e| e.to_string())?;
    
    if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == track_id) {
        track.volume_automation.insert_node(time, value);
        Ok(())
    } else {
        Err(format!("Track {} not found", track_id))
    }
}

#[tauri::command]
pub fn remove_volume_automation_node(
    state: State<'_, SharedAudioEngine>,
    track_id: String,
    time: u64,
) -> Result<(), String> {
    let mut engine = state.lock().map_err(|e| e.to_string())?;
    
    if let Some(track) = engine.tracks_mut().iter_mut().find(|t| t.id == track_id) {
        track.volume_automation.remove_node_at_time(time);
        Ok(())
    } else {
        Err(format!("Track {} not found", track_id))
    }
}