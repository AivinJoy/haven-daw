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
#[serde(rename_all = "camelCase")]
struct ImportResult {
    mins: Vec<f32>,
    maxs: Vec<f32>,
    duration: f64,
    bins_per_second: f64,
    bpm: Option<f32>, // New field for BPM
}

// Helper function to build the UI state from the raw track list
fn build_ui_state(
    app: &tauri::AppHandle, 
    tracks_info: Vec<daw_modules::audio_runtime::FrontendTrackInfo>,
    bpm: f32,
    master_gain: f32
) -> Result<ProjectState, String> {
    
    let mut results = Vec::new();
    let colors = [
        "bg-brand-blue", "bg-brand-red", "bg-purple-500", 
        "bg-emerald-500", "bg-orange-500", "bg-pink-500",
        "bg-cyan-500", "bg-indigo-500", "bg-rose-500"
    ];

    for (i, info) in tracks_info.iter().enumerate() {
        let track_id = info.id + 1; // Frontend uses 1-based ID
        let color = colors[i % colors.len()].to_string();
        
        let mut loaded_clips = Vec::new();

        for (j, clip_info) in info.clips.iter().enumerate() {
            // Emit progress event
            let _ = app.emit("load-progress", format!("Analyzing Track {} Clip {}", track_id, j + 1));

            // Decode Audio for Waveform
            // Note: In a real app, you might want to cache this to avoid re-analyzing unchanged clips
            let (samples, sr, channels) = daw_modules::bpm::adapter::decode_to_vec(&clip_info.path)
                .map_err(|e| format!("Failed to decode {}: {}", clip_info.path, e))?;
            
            let wf = Waveform::build_from_samples(&samples, sr, channels, 512);
            let pixels_per_second = 100.0;
            let spp = (sr as f64) / pixels_per_second;
            let (mins, maxs, _) = wf.bins_for(spp, 0, 0, usize::MAX);

            let clip_id = format!("clip_{}_{}", track_id, j);
            let clip_name = std::path::Path::new(&clip_info.path)
                .file_name().unwrap_or_default().to_string_lossy().to_string();

            loaded_clips.push(LoadedClip {
                id: clip_id,
                track_id,
                name: clip_name,
                path: clip_info.path.clone(),
                start_time: clip_info.start_time,
                duration: clip_info.duration, 
                offset: clip_info.offset,
                color: color.clone(),
                waveform: ImportResult {
                    mins: mins.to_vec(),
                    maxs: maxs.to_vec(),
                    duration: wf.duration_secs,
                    bins_per_second: pixels_per_second,
                    bpm: None,
                },
            });
        }

        results.push(LoadedTrack {
            id: track_id,
            name: info.name.clone(),
            color,
            clips: loaded_clips,
            gain: info.gain,
            pan: info.pan,
            muted: info.muted,
            solo: info.solo
        });
    }
    
    Ok(ProjectState {
        tracks: results,
        bpm,
        master_gain,
    })
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

#[derive(Clone, serde::Serialize)]
struct ProgressPayload {
    message: String,
    progress: f64,
    visible: bool,
}

#[tauri::command]
fn import_track(app: tauri::AppHandle,path: String, state: State<AppState>) -> Result<ImportResult, String> {
    
    // 0. Start Loader
    let _ = app.emit("progress-update", ProgressPayload { 
        message: "Initializing Import...".into(), 
        progress: 5.0, 
        visible: true 
    });

    // 1. Add to Audio Engine (Playback)
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.add_track(path.clone()).map_err(|e| e.to_string())?;

    // Update Progress
    let _ = app.emit("progress-update", ProgressPayload { 
        message: "Decoding Audio...".into(), 
        progress: 20.0, 
        visible: true 
    });

    // 2. Decode Once (Analysis) - Using the FIXED decode_to_vec
    let (samples, sr, channels) = bpm::adapter::decode_to_vec(&path)
        .map_err(|e| format!("Failed to decode: {}", e))?;

    // Update Progress
    let _ = app.emit("progress-update", ProgressPayload { 
        message: "Analyzing Waveform...".into(), 
        progress: 60.0, 
        visible: true 
    });

    // 3. Build Waveform (Visual) - Using the FIXED build_from_samples
    let wf = Waveform::build_from_samples(&samples, sr, channels, 512);

    // Update Progress
    let _ = app.emit("progress-update", ProgressPayload { 
        message: "Detecting BPM...".into(), 
        progress: 80.0, 
        visible: true 
    });

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

    // 6. Finish Loader
    let _ = app.emit("progress-update", ProgressPayload { 
        message: "Done".into(), 
        progress: 100.0, 
        visible: false // Hides the loader
    });

    Ok(ImportResult {
        mins: mins.to_vec(),
        maxs: maxs.to_vec(),
        duration: wf.duration_secs,
        bins_per_second: if wf.duration_secs > 0.0 { (mins.len() as f64) / wf.duration_secs } else { 0.0 },

        bpm: detected_bpm,
    })
}

#[tauri::command]
fn analyze_file(path: String) -> Result<ImportResult, String> {
    // 1. Decode (Analysis)
    let (samples, sr, channels) = bpm::adapter::decode_to_vec(&path)
        .map_err(|e| format!("Failed to decode: {}", e))?;

    // 2. Build Waveform (Visual)
    let wf = Waveform::build_from_samples(&samples, sr, channels, 512);

    // 3. Detect BPM
    let mut det = bpm::BpmDetector::new(2048);
    let opts = bpm::BpmOptions { compute_beats: true, ..Default::default() };
    let detected_bpm = det.detect(&samples, channels, sr, opts).map(|res| res.bpm);

    // 4. Calculate Bins for UI
    let pixels_per_second = 100.0;
    let spp = (sr as f64) / pixels_per_second;
    let (mins, maxs, _) = wf.bins_for(spp, 0, 0, usize::MAX);

    Ok(ImportResult {
        mins: mins.to_vec(),
        maxs: maxs.to_vec(),
        duration: wf.duration_secs,
        bins_per_second: if wf.duration_secs > 0.0 { (mins.len() as f64) / wf.duration_secs } else { 0.0 },
        bpm: detected_bpm,
    })
}

#[tauri::command]
fn move_clip(
    track_index: usize, 
    clip_index: usize, 
    new_time: f64, 
    state: State<AppState>
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    
    // Call the runtime logic we just added
    audio.move_clip(track_index, clip_index, new_time)
        .map_err(|e| e.to_string())?;
        
    Ok(())
}

#[derive(serde::Serialize)]
struct RecordingState {
    is_recording: bool,
    duration: f64,
    // We send a simplified volume level (RMS) or a small chunk of waveform points
    // For now, let's send the current RMS (volume) for the meter
    current_rms: f32,
    is_monitoring: bool, 
}

#[tauri::command]
fn start_recording(path: String, state: State<AppState>) -> Result<(), String> {
    let mut rec_guard = state.recorder.lock().map_err(|_| "Failed to lock recorder")?;
    
    // Use the path provided by the frontend
    let new_recorder = Recorder::start(PathBuf::from(path)).map_err(|e| e.to_string())?;
    
    *rec_guard = Some(new_recorder);
    Ok(())
}

#[tauri::command]
fn get_recording_status(state: State<AppState>) -> Result<RecordingState, String> {
    let rec_guard = state.recorder.lock().map_err(|_| "Failed to lock recorder")?;
    
    if let Some(rec) = rec_guard.as_ref() {
        let duration = rec.get_record_time().as_secs_f64();
        let current_rms = 0.5; // Placeholder RMS
        
        Ok(RecordingState {
            is_recording: true,
            duration,
            current_rms,
            is_monitoring: rec.is_monitor_enabled(), // <--- Fetch real state
        })
    } else {
        Ok(RecordingState {
            is_recording: false,
            duration: 0.0,
            current_rms: 0.0,
            is_monitoring: false, // Default off
        })
    }
}

#[tauri::command]
fn toggle_monitor_cmd(state: State<AppState>) -> Result<bool, String> {
    let mut rec_guard = state.recorder.lock().map_err(|_| "Failed to lock recorder")?;
    if let Some(rec) = rec_guard.as_mut() {
        rec.toggle_monitor().map_err(|e| e.to_string())?;
        Ok(rec.is_monitor_enabled())
    } else {
        // If not recording, we can't toggle the hardware monitor yet.
        Ok(false)
    }
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

#[tauri::command]
fn add_clip(
    track_id: usize, 
    path: String, 
    start_time: f64, 
    state: State<AppState>
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    // Note: track_id from frontend is 1-based, engine uses 0-based index?
    // Adjust index as needed based on your logic.
    audio.add_clip(track_id - 1, path, start_time).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn create_track(state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.create_empty_track().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn split_clip(
    track_index: usize, 
    time: f64, 
    state: State<AppState>
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    
    // Frontend uses 1-based track IDs usually? 
    // If your frontend passes the array index (0-based), keep as is.
    // If frontend passes ID (1, 2...), subtract 1.
    // Based on 'move_clip' in your file, it seems direct mapping or handled there.
    // Let's assume track_index matches the Vec index.
    
    audio.split_clip(track_index, time).map_err(|e| e.to_string())?;
    
    Ok(())
}

#[tauri::command]
fn merge_clip_with_next(track_index: usize, clip_index: usize, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    audio.merge_clip_with_next(track_index, clip_index).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_track(track_index: usize, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    audio.delete_track(track_index).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_clip(
    track_index: usize, 
    clip_index: usize, 
    state: State<AppState>
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    audio.delete_clip(track_index, clip_index).map_err(|e| e.to_string())
}

// 1. Argument Struct
#[derive(serde::Deserialize)]
struct EqUpdateArgs {
    track_index: usize,
    band_index: usize,
    filter_type: String, 
    freq: f32,
    q: f32,
    gain: f32,
    active: bool,
}

// 2. Commands

#[tauri::command]
fn update_eq(args: EqUpdateArgs, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    
    // Map String to Enum
    let filter_type = match args.filter_type.as_str() {
        "LowPass" => daw_modules::effects::equalizer::EqFilterType::LowPass,
        "HighPass" => daw_modules::effects::equalizer::EqFilterType::HighPass,
        "Peaking" => daw_modules::effects::equalizer::EqFilterType::Peaking,
        "LowShelf" => daw_modules::effects::equalizer::EqFilterType::LowShelf,
        "HighShelf" => daw_modules::effects::equalizer::EqFilterType::HighShelf,
        "Notch" => daw_modules::effects::equalizer::EqFilterType::Notch,
        "BandPass" => daw_modules::effects::equalizer::EqFilterType::BandPass,
        _ => daw_modules::effects::equalizer::EqFilterType::Peaking,
    };

    let params = daw_modules::effects::equalizer::EqParams {
        filter_type,
        freq: args.freq,
        q: args.q,
        gain: args.gain,
        active: args.active,
    };

    audio.update_eq(args.track_index, args.band_index, params);
    Ok(())
}

#[tauri::command]
fn get_eq_state(track_index: usize, state: State<AppState>) -> Result<Vec<daw_modules::effects::equalizer::EqParams>, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    Ok(audio.get_eq_state(track_index))
}

#[tauri::command]
async fn get_project_state(
    app: tauri::AppHandle, 
    state: State<'_, AppState>
) -> Result<ProjectState, String> {
    
    // 1. Fetch Data from Memory (NO Disk I/O)
    let audio_runtime = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    
    let bpm = audio_runtime.bpm();
    let master_gain = audio_runtime.master_gain();
    let tracks_info = audio_runtime.get_tracks_list();
    drop(audio_runtime); // Release lock

    // 2. Build UI State (Reuse Helper)
    let state = build_ui_state(&app, tracks_info, bpm, master_gain)?;

    Ok(state)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadedClip {
    id: String,
    track_id: u32,
    name: String,
    path: String,
    start_time: f64,
    duration: f64,
    offset: f64,
    waveform: ImportResult,
    color: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadedTrack {
    id: u32,
    name: String,
    color: String,
    clips: Vec<LoadedClip>,
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
fn export_project(app: tauri::AppHandle,path: String, state: State<AppState>) -> Result<(), String> {
    // 1. Show Loader
    let _ = app.emit("progress-update", ProgressPayload { 
        message: "Rendering Project...".into(), 
        progress: 0.0, // Indeterminate start
        visible: true 
    });
    
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;

    // NOTE: If audio.export_project takes a long time, it will block this thread.
    // Ideally, export_project inside AudioRuntime should accept a callback closure 
    // to report progress. For now, this ensures the loader at least appears.
    let result = audio.export_project(path);

    // 2. Hide Loader
    let _ = app.emit("progress-update", ProgressPayload { 
        message: "Export Complete".into(), 
        progress: 100.0, 
        visible: false 
    });
    result
}

#[tauri::command]
async fn load_project(
    app: tauri::AppHandle, 
    path: String, 
    state: State<'_, AppState>,
) -> Result<ProjectState, String> {
    
    // 1. Perform the Load (Disk I/O)
    let audio_runtime = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio_runtime.load_project(path.clone())?;

    // 2. Fetch Data from Memory
    let bpm = audio_runtime.bpm();
    let master_gain = audio_runtime.master_gain();
    let tracks_info = audio_runtime.get_tracks_list();
    drop(audio_runtime); // Release lock

    // 3. Build UI State (Reuse Helper)
    let state = build_ui_state(&app, tracks_info, bpm, master_gain)?;

    let _ = app.emit("load-percent", 100.0);
    let _ = app.emit("load-progress", "Ready");

    Ok(state)
}
// Add these to the invoke_handler list!
#[tauri::command]
fn get_temp_path(filename: String) -> String {
    let mut path = std::env::temp_dir();
    path.push(filename);
    path.to_string_lossy().to_string()
}

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
            analyze_file,
            create_track,
            get_position,
            start_recording,
            toggle_monitor_cmd,
            stop_recording,
            get_recording_status,
            set_bpm,
            get_grid_lines,
            move_clip,
            seek,
            set_track_gain,
            set_track_pan,
            toggle_mute,
            toggle_solo,
            set_master_gain,
            save_project,
            load_project,
            export_project,
            get_temp_path,
            add_clip,
            split_clip,
            get_project_state,
            merge_clip_with_next,
            delete_clip,
            delete_track,
            update_eq,
            get_eq_state
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}