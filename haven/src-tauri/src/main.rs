// src-tauri/src/main.rs
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{State, Emitter};

// Import modules
use daw_modules::audio_runtime::AudioRuntime;
use daw_modules::recorder::Recorder;
use daw_modules::waveform::Waveform;
use daw_modules::bpm; // Import the new BPM module
use daw_modules::engine::time::GridLine; // Import GridLine


// --- 1. Global State ---
struct AppState {
    audio: Mutex<AudioRuntime>,
    recorder: Mutex<Option<Recorder>>,
}

// --- 2. Define Return Struct ---
#[derive(serde::Serialize)]
struct ImportResult {
    mins: Vec<f32>,
    maxs: Vec<f32>,
    duration: f64,
    bpm: Option<f32>, // New field for BPM
}

// --- 3. Commands ---

#[tauri::command]
fn play(state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.play();
    Ok(())
}

#[tauri::command]
fn pause(state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.pause();
    Ok(())
}

#[tauri::command]
fn get_position(state: State<AppState>) -> Result<f64, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    Ok(audio.position().as_secs_f64())
}



#[tauri::command]
fn import_track(path: String, state: State<AppState>) -> Result<ImportResult, String> {
    // 1. Add to Audio Engine (Playback)
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.add_track(path.clone()).map_err(|e| e.to_string())?;

    // 2. Decode Once (Analysis) - Using the FIXED decode_to_vec
    let (samples, sr, channels) = bpm::adapter::decode_to_vec(&path)
        .map_err(|e| format!("Failed to decode: {}", e))?;

    // 3. Build Waveform (Visual) - Using the FIXED build_from_samples
    let wf = Waveform::build_from_samples(&samples, sr, channels, 512);

    // --- ADD THIS DEBUG BLOCK ---
    println!("--------------------------------------------------");
    println!("📊 BACKEND TRUTH:");
    println!("   - Duration:     {:.6} seconds", wf.duration_secs);
    println!("   - Samples:      {}", samples.len());
    println!("   - Channels:     {}", channels);
    println!("   - Rate:         {}", sr);
    
    let target_width = wf.duration_secs * 50.0;
    println!("   - Target Width: {:.4} px (at 1x Zoom)", target_width);
    println!("--------------------------------------------------");
    // ----------------------------

    // 4. Detect BPM (Musical)
    let mut det = bpm::BpmDetector::new(2048);
    let opts = bpm::BpmOptions { compute_beats: true, ..Default::default() };
    let detected_bpm = det.detect(&samples, channels, sr, opts).map(|res| res.bpm);

    // 5. Send to Frontend
    let pixels_per_second = 100.0;
    let spp = (sr as f64) / pixels_per_second;
    
    // FIX: Pass 4 arguments (spp, channel, start_bin, columns)
    let (mins, maxs, _) = wf.bins_for(spp, 0, 0, usize::MAX);

    Ok(ImportResult {
        mins: mins.to_vec(),
        maxs: maxs.to_vec(),
        duration: wf.duration_secs,
        bpm: detected_bpm,
    })
}

#[tauri::command]
fn set_track_start(track_index: usize, start_time: f64, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.set_track_start_time(track_index, start_time);
    Ok(())
}

#[tauri::command]
fn start_recording(state: State<AppState>) -> Result<(), String> {
    let mut rec_guard = state.recorder.lock().map_err(|_| "Failed to lock recorder")?;
    let new_recorder = Recorder::start(PathBuf::from("recording.wav")).map_err(|e| e.to_string())?;
    *rec_guard = Some(new_recorder);
    Ok(())
}

#[tauri::command]
fn stop_recording(state: State<AppState>) -> Result<(), String> {
    let mut rec_guard = state.recorder.lock().map_err(|_| "Failed to lock recorder")?;
    if let Some(rec) = rec_guard.take() {
        rec.stop();
    }
    Ok(())
}

#[tauri::command]
fn seek(pos: f64, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    // Convert float seconds to Duration
    let position = Duration::from_secs_f64(pos.max(0.0));
    audio.seek(position);
    Ok(())
}

#[tauri::command]
fn set_track_gain(track_index: usize, gain: f32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.set_track_gain(track_index, gain);
    Ok(())
}

#[tauri::command]
fn set_master_gain(gain: f32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.set_master_gain(gain);
    Ok(())
}

#[tauri::command]
fn set_track_pan(track_index: usize, pan: f32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.set_track_pan(track_index, pan);
    Ok(())
}

#[tauri::command]
fn toggle_mute(track_index: usize, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.toggle_mute(track_index);
    Ok(())
}

// src-tauri/src/main.rs

#[tauri::command]
fn toggle_solo(track_index: usize, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    // Call the new simpler logic
    audio.toggle_solo(track_index); 
    Ok(())
}

#[tauri::command]
fn set_bpm(bpm: f32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    // You'll need to expose a set_bpm method on AudioRuntime that calls Engine::set_bpm
    audio.set_bpm(bpm); 
    Ok(())
}

#[tauri::command]
fn get_grid_lines(
    start: f64, 
    end: f64, 
    resolution: u32, 
    state: State<AppState>
) -> Result<Vec<GridLine>, String> { // UPDATED Return Type
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    
    let start_dur = Duration::from_secs_f64(start.max(0.0));
    let end_dur = Duration::from_secs_f64(end.max(0.0));
    
    Ok(audio.get_grid_lines(start_dur, end_dur, resolution))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadedTrack {
    id: u32,
    name: String,
    path: String,
    color: String,
    duration: f64,
    start_time: f64,
    waveform: ImportResult, // Reusing existing ImportResult
    gain: f32,
    pan: f32,
    muted: bool,
    solo: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectState {
    tracks: Vec<LoadedTrack>,
    bpm: f32,
    master_gain: f32,
}


#[tauri::command]
fn save_project(path: String, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.save_project(path)
}

#[tauri::command]
fn export_project(path: String, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.export_project(path)
}

#[tauri::command]
async fn load_project(
    app: tauri::AppHandle, 
    path: String, 
    state: State<'_, AppState>, // <--- FIX: Added <'_, ...>
) -> Result<ProjectState, String> {
    
    // We clone the inner Runtime so we don't hold the State lock across await points
    // (This is a best practice in async Rust to prevent deadlocks)
    let audio_runtime = state.audio.lock().map_err(|_| "Failed to lock audio")?;

    let bpm = audio_runtime.bpm();
    let master_gain = audio_runtime.master_gain();
    
    // A. Restore Backend State
    // Note: Since load_project is blocking, we wrap it if we want true non-blocking,
    // but for now, just calling it here is fine since we are in an async command.
    audio_runtime.load_project(path.clone())?;
    
    // B. Fetch the list of tracks now in the engine
    let tracks_info = audio_runtime.get_tracks_list();
    
    // Drop the lock here so we don't hold it while doing the heavy loop below
    drop(audio_runtime); 

    let mut results = Vec::new();
    let total = tracks_info.len();

    let colors = [
        "bg-brand-blue", "bg-brand-red", "bg-purple-500", 
        "bg-emerald-500", "bg-orange-500", "bg-pink-500"
    ];

    // C. Re-Analyze Audio for Visualization
    for (i, info) in tracks_info.iter().enumerate() {
        
        // 1. Calculate Percentage & Emit
        let percent = ((i as f64) / (total as f64)) * 100.0;
        let _ = app.emit("load-progress", format!("Loading Track {}/{}", i + 1, total));
        let _ = app.emit("load-percent", percent);

        // 2. Decode
        // Note: decode_to_vec is heavy. In a perfect world, we'd spawn_blocking this.
        // But since this entire function is async, the UI should remain responsive enough for updates.
        let (samples, sr, channels) = bpm::adapter::decode_to_vec(&info.path)
            .map_err(|e| format!("Failed to decode {}: {}", info.path, e))?;
        
        // 3. Build Waveform
        let wf = Waveform::build_from_samples(&samples, sr, channels, 512);
        
        // 4. Bins
        let pixels_per_second = 100.0;
        let spp = (sr as f64) / pixels_per_second;
        let (mins, maxs, _) = wf.bins_for(spp, 0, 0, usize::MAX);

        let color = colors[i % colors.len()].to_string();
        
        let name = std::path::Path::new(&info.path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        results.push(LoadedTrack {
            id: info.id + 1, // Visual ID (1-based)
            name,
            path: info.path.clone(),
            color,
            duration: wf.duration_secs,
            start_time: info.start_time,
            waveform: ImportResult {
                mins: mins.to_vec(),
                maxs: maxs.to_vec(),
                duration: wf.duration_secs,
                bpm: None, 
            },
            gain: info.gain,
            pan: info.pan,
            muted: info.muted,
            solo: info.solo
        });
    }
    
    // Finish
    let _ = app.emit("load-percent", 100.0);
    let _ = app.emit("load-progress", "Finalizing...");
    
    Ok(ProjectState {
        tracks: results,
        bpm,
        master_gain,
    })
}
// Add these to the invoke_handler list!

fn main() {
    let runtime = AudioRuntime::new(None).expect("Failed to init Audio Engine");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .manage(AppState {
            audio: Mutex::new(runtime),
            recorder: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            play,
            pause,
            import_track,
            get_position,
            start_recording,
            stop_recording,
            set_bpm,
            get_grid_lines,
            set_track_start,
            seek,
            set_track_gain,
            set_track_pan,
            toggle_mute,
            toggle_solo,
            set_master_gain,
            save_project,
            load_project,
            export_project
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}