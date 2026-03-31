// src-tauri/src/automation.rs
use crate::AppState;
use tauri::State;

#[derive(serde::Serialize)]
pub struct UiAutomationNode {
    pub time: f64, // Time in SECONDS
    pub value: f32, // Linear Gain (for Svelte)
}

#[tauri::command]
pub fn get_volume_automation(
    track_id: u32,
    state: State<'_, AppState>,
) -> Result<Vec<UiAutomationNode>, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    
    // 🚀 NO MORE SAMPLE RATE EXTRACTION NEEDED HERE
    
    let nodes = audio.get_volume_automation(track_id).map_err(|e| e.to_string())?;
    
    // UI expects Linear. We must convert the backend's dB values to Linear.
    Ok(nodes.into_iter().map(|n| UiAutomationNode {
        time: n.time, // 🚀 FIX: Pass the absolute seconds directly to Svelte!
        value: 10.0_f32.powf(n.value / 20.0) 
    }).collect())
}

#[tauri::command]
pub fn add_volume_automation_node(
    track_id: u32,
    time: f64, // Svelte sends pure SECONDS
    value: f32, 
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    let db_value = if value <= 0.0001 { -80.0 } else { 20.0 * value.log10() };
    
    audio.add_volume_automation_node(track_id, time, db_value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_volume_automation_node(
    track_id: u32,
    time: f64, // Svelte sends pure SECONDS
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    
    audio.remove_volume_automation_node(track_id, time).map_err(|e| e.to_string())
}