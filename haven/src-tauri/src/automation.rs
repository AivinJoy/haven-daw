// src-tauri/src/automation.rs
use crate::AppState;
use tauri::State;

// Create a UI-friendly struct that uses seconds instead of samples
#[derive(serde::Serialize)]
pub struct UiAutomationNode {
    pub time: f64, // Time in SECONDS
    pub value: f32,
}

#[tauri::command]
pub fn get_volume_automation(
    track_id: u32,
    state: State<'_, AppState>,
) -> Result<Vec<UiAutomationNode>, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    let sr = audio.sample_rate() as f64; // Get the TRUE hardware sample rate
    let nodes = audio.get_volume_automation(track_id).map_err(|e| e.to_string())?;
    
    // Convert samples to seconds for the UI
    Ok(nodes.into_iter().map(|n| UiAutomationNode {
        time: (n.time as f64) / sr,
        value: n.value
    }).collect())
}

#[tauri::command]
pub fn add_volume_automation_node(
    track_id: u32,
    time: f64, // Time in SECONDS
    value: f32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    let sr = audio.sample_rate() as f64;
    // Safely convert seconds to exact hardware samples
    let sample_time = (time * sr).round() as u64; 
    
    audio.add_volume_automation_node(track_id, sample_time, value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_volume_automation_node(
    track_id: u32,
    time: f64, // Time in SECONDS
    state: State<'_, AppState>,
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    let sr = audio.sample_rate() as f64;
    let sample_time = (time * sr).round() as u64;
    
    audio.remove_volume_automation_node(track_id, sample_time).map_err(|e| e.to_string())
}