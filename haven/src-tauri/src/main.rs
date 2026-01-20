// src-tauri/src/main.rs
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use std::collections::HashMap;
use tauri::{State, Emitter};
use cpal::traits::{HostTrait, DeviceTrait};
use dotenv::dotenv;

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
    cache: Mutex<HashMap<String, ImportResult>>,
    track_colors: Mutex<HashMap<u32, String>>,
}

// --- 2. Define Return Struct ---
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ImportResult {
    mins: Vec<f32>,
    maxs: Vec<f32>,
    duration: f64,
    bins_per_second: f64,
    bpm: Option<f32>, // New field for BPM
    color: String,
}

// Helper function to build the UI state from the raw track list
fn build_ui_state(
    app: &tauri::AppHandle, 
    tracks_info: Vec<daw_modules::audio_runtime::FrontendTrackInfo>,
    bpm: f32,
    master_gain: f32,
    silent: bool,
    cache_store: &Mutex<HashMap<String, ImportResult>>,
    color_store: &Mutex<HashMap<u32, String>>
) -> Result<ProjectState, String> {
    
    let mut results = Vec::new();
    let available_colors = [
        "bg-brand-blue", "bg-brand-red", "bg-purple-500", 
        "bg-emerald-500", "bg-orange-500", "bg-pink-500",
        "bg-cyan-500", "bg-indigo-500", "bg-rose-500"
    ];

    for info in tracks_info.iter() {
        let raw_id = info.id; // Frontend uses 1-based ID
        // 1. Try to find existing color
        let stored_color = {
            let map = color_store.lock().map_err(|_| "Failed to lock colors")?;
            map.get(&raw_id).cloned()
        };

        let color = if let Some(c) = stored_color {
            c // ✅ Found it! Use the permanent color.
        } else {
            // 🎲 New Track? Pick a Color deterministically based on ID 
            // to avoid "random" changes if ID persists (e.g. during Undo)
            let random_idx = (raw_id as usize) % available_colors.len();
            let new_color = available_colors[random_idx].to_string();

            // Save it forever (for this session)
            if let Ok(mut map) = color_store.lock() {
                map.insert(raw_id, new_color.clone());
            }
            new_color
        };

        let track_id = raw_id + 1;
        
        let mut loaded_clips = Vec::new();

        for (j, clip_info) in info.clips.iter().enumerate() {
            // Emit progress event
            // 1. LOOKUP IN CACHE
            let cached_data = {
                let lock = cache_store.lock().map_err(|_| "Failed to lock cache")?;
                lock.get(&clip_info.path).cloned()
            };

            // 2. DECIDE: HIT OR MISS?
            let import_result = if let Some(data) = cached_data {
                data // ✅ Instant Hit
            } else {
                // ⚠️ Miss: Return Placeholder (Do NOT compute here to prevent freezing)
                if !silent {
                     println!("⚠️ Cache miss for {}", clip_info.path);
                }
                ImportResult {
                    mins: vec![],
                    maxs: vec![],
                    duration: clip_info.duration, // Use duration from backend info
                    bins_per_second: 100.0,
                    bpm: None,
                    color: "".to_string(),
                }
            };

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
                waveform: import_result, // <--- Use the cached result
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

// 1. Define this struct to send richer data to Frontend
#[derive(serde::Serialize)]
struct AudioDeviceInfo {
    name: String,
    is_default: bool,
}

#[tauri::command]
fn get_output_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    let host = cpal::default_host();
    
    // 1. Get the exact name of the system default device
    let default_name = host.default_output_device()
        .and_then(|d| d.name().ok());

    let devices = host.output_devices().map_err(|e| e.to_string())?;
    
    // 2. Map devices to our struct, checking if they match the default
    let list: Vec<AudioDeviceInfo> = devices
        .filter_map(|d| {
            let name = d.name().ok()?;
            // Check exact name match
            let is_default = Some(name.clone()) == default_name;
            
            Some(AudioDeviceInfo { name, is_default })
        })
        .collect();
        
    Ok(list)
}

#[tauri::command]
fn get_input_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    let host = cpal::default_host();
    
    // 1. Get the exact name of the system default device
    let default_name = host.default_input_device()
        .and_then(|d| d.name().ok());

    let devices = host.input_devices().map_err(|e| e.to_string())?;
    
    let list: Vec<AudioDeviceInfo> = devices
        .filter_map(|d| {
            let name = d.name().ok()?;
            let is_default = Some(name.clone()) == default_name;
            
            Some(AudioDeviceInfo { name, is_default })
        })
        .collect();
        
    Ok(list)
}

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
async fn import_tracks( // <--- CHANGED to 'async fn' for better UI behavior
    app: tauri::AppHandle,
    paths: Vec<String>, 
    state: State<'_, AppState>
) -> Result<Vec<ImportResult>, String> { 
    
    let total_files = paths.len() as f64;
    let mut results = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        let file_num = i + 1;
        let step_size = 100.0 / total_files;
        let base_progress = (i as f64) * step_size;

        // --- STEP 1: PREPARING (Fast) ---
        let _ = app.emit("progress-update", ProgressPayload { 
            message: format!("Preparing file {} of {}...", file_num, total_files),
            progress: base_progress + (step_size * 0.05), 
            visible: true 
        });

        // LOCK SCOPE: Only lock audio for the split second we need to add the track
        // LOCK SCOPE: Add track AND Set Name
        let track_id_for_color = {
            let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
            audio.add_track(path.clone()).map_err(|e| e.to_string())?;
            
            // FIX 1: Set the Name in the Backend immediately
            let track_list = audio.get_tracks_list();
            let id = track_list.len() - 1; // Assuming new track is last
            let raw_id = track_list[id].id; // Get the actual ID
            
            let filename = std::path::Path::new(&path)
                .file_name().unwrap_or_default().to_string_lossy().to_string();
            
            audio.set_track_name(id, filename);
            
            raw_id // Return ID for color generation
        };

        // Force UI Update
        tokio::time::sleep(Duration::from_millis(50)).await;

        // --- STEP 2: DECODING (Heavy) ---
        let _ = app.emit("progress-update", ProgressPayload { 
            message: format!("Decoding Audio Data {}...", file_num),
            progress: base_progress + (step_size * 0.15), 
            visible: true 
        });
        
        // Force UI Update
        tokio::time::sleep(Duration::from_millis(50)).await;

        // We clone path so we can move it into the thread
        let path_clone = path.clone();

        // RUN HEAVY TASK ON SEPARATE THREAD so UI doesn't freeze
        let (samples, sr, channels) = tauri::async_runtime::spawn_blocking(move || {
            bpm::adapter::decode_to_vec(&path_clone)
        }).await.map_err(|e| e.to_string())?.map_err(|e| format!("Failed to decode: {}", e))?;

        // --- STEP 3: WAVEFORM (Heavy) ---
        let _ = app.emit("progress-update", ProgressPayload { 
            message: format!("Generating Waveform {}...", file_num),
            progress: base_progress + (step_size * 0.50), 
            visible: true 
        });

        // Force UI Update
        tokio::time::sleep(Duration::from_millis(50)).await;

        let samples_clone = samples.clone();
        
        // RUN HEAVY TASK ON SEPARATE THREAD
        let wf = tauri::async_runtime::spawn_blocking(move || {
            Waveform::build_from_samples(&samples_clone, sr, channels, 512)
        }).await.map_err(|e| e.to_string())?;

        // --- STEP 4: BPM (Heavy) ---
        let _ = app.emit("progress-update", ProgressPayload { 
            message: format!("Detecting Tempo {}...", file_num),
            progress: base_progress + (step_size * 0.80), 
            visible: true 
        });

        // Force UI Update
        tokio::time::sleep(Duration::from_millis(50)).await;

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


        let samples_bpm = samples.clone();
        let detected_bpm = tauri::async_runtime::spawn_blocking(move || {
            let mut det = bpm::BpmDetector::new(2048);
            let opts = bpm::BpmOptions { compute_beats: true, ..Default::default() };
            det.detect(&samples_bpm, channels, sr, opts).map(|res| res.bpm)
        }).await.map_err(|e| e.to_string())?;

        // --- STEP 5: FINALIZE ---
        let _ = app.emit("progress-update", ProgressPayload { 
            message: format!("Finalizing {}...", file_num),
            progress: base_progress + (step_size * 0.95), 
            visible: true 
        });

        let pixels_per_second = 100.0;
        let spp = (sr as f64) / pixels_per_second;
        let (mins, maxs, _) = wf.bins_for(spp, 0, 0, usize::MAX);

        // FIX 2: Generate Permanent Color
        let available_colors = [
            "bg-brand-blue", "bg-brand-red", "bg-purple-500", 
            "bg-emerald-500", "bg-orange-500", "bg-pink-500",
            "bg-cyan-500", "bg-indigo-500", "bg-rose-500"
        ];
        // [Refinement 2] Deterministic color based on ID
        let random_idx = (track_id_for_color as usize) % available_colors.len();
        let assigned_color = available_colors[random_idx].to_string();

        // SAVE Color to State (So Undo remembers it)
        if let Ok(mut colors) = state.track_colors.lock() {
            colors.insert(track_id_for_color, assigned_color.clone());
        }

        // 1. Create the result object first
        let result = ImportResult {
            mins: mins.to_vec(),
            maxs: maxs.to_vec(),
            duration: wf.duration_secs,
            bins_per_second: if wf.duration_secs > 0.0 { (mins.len() as f64) / wf.duration_secs } else { 0.0 },
            bpm: detected_bpm,
            color: assigned_color,
        };

        // 2. WRITE TO CACHE (This is the critical fix)
        // We lock the cache briefly to store the data for future lookups
        if let Ok(mut cache) = state.cache.lock() {
            cache.insert(path.clone(), result.clone());
        }

        // 3. Push to results for the UI
        results.push(result);
    }

    // --- DONE ---
    let _ = app.emit("progress-update", ProgressPayload { 
        message: "Import Complete!".into(), 
        progress: 100.0, 
        visible: false 
    });

    Ok(results)
}

#[tauri::command]
fn analyze_file(path: String, state: State<AppState>) -> Result<ImportResult, String> {
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

    let result = ImportResult {
        mins: mins.to_vec(),
        maxs: maxs.to_vec(),
        duration: wf.duration_secs,
        bins_per_second: if wf.duration_secs > 0.0 { (mins.len() as f64) / wf.duration_secs } else { 0.0 },
        bpm: detected_bpm,
        color: "".to_string(), // Frontend assigns color for clips, or we ignore
    };

    // CRITICAL FIX: Save to cache so Undo can find it
    if let Ok(mut cache) = state.cache.lock() {
        cache.insert(path.clone(), result.clone());
    }

    Ok(result)
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

// [Refinement 1] Create Track: Source of Truth
// Returns the fully formed track data (ID, Name, Color) to the frontend.
#[tauri::command]
fn create_track(state: State<AppState>) -> Result<LoadedTrack, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.create_empty_track().map_err(|e| e.to_string())?;
    
    // 1. Fetch the track we just created (It is the last one in the list)
    let tracks = audio.get_tracks_list();
    let info = tracks.last().ok_or("Track creation failed")?;
    let raw_id = info.id; // Monotonic ID
    let track_id_display = raw_id + 1; // 1-based for UI display

    // 2. Generate Metadata (Name & Color)
    // [Refinement 2] Standardized naming: "Track-01", "Track-02"
    let new_name = format!("Track-{:02}", track_id_display);
    
    let available_colors = [
        "bg-brand-blue", "bg-brand-red", "bg-purple-500", 
        "bg-emerald-500", "bg-orange-500", "bg-pink-500",
        "bg-cyan-500", "bg-indigo-500", "bg-rose-500"
    ];
    let color_idx = (raw_id as usize) % available_colors.len();
    let color = available_colors[color_idx].to_string();
    
    // 3. Persist Metadata in Backend
    // Set Name in Engine
    let index = tracks.len() - 1; 
    audio.set_track_name(index, new_name.clone());
    
    // Set Color in AppState
    state.track_colors.lock().unwrap().insert(raw_id, color.clone());
    
    // 4. Return to Frontend
    Ok(LoadedTrack {
        id: track_id_display, // Matches the convention in build_ui_state
        name: new_name,
        color: color,
        clips: vec![],
        gain: 1.0,
        pan: 0.0,
        muted: false,
        solo: false
    })
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

#[tauri::command]
fn undo(state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.undo();
    Ok(())
}

#[tauri::command]
fn redo(state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.redo();
    Ok(())
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
    // Pass cache AND color store
    let state = build_ui_state(&app, tracks_info, bpm, master_gain, true, &state.cache, &state.track_colors)?;
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

    for info in &tracks_info {
        for clip in &info.clips {
             let path_key = clip.path.clone();
             let needs_load = {
                 let cache = state.cache.lock().unwrap();
                 !cache.contains_key(&path_key)
             };

             if needs_load {
                 let _ = app.emit("load-progress", format!("Loading {}", clip.path));
                 if let Ok((samples, sr, ch)) = daw_modules::bpm::adapter::decode_to_vec(&clip.path) {
                      let wf = Waveform::build_from_samples(&samples, sr, ch, 512);
                      let pixels_per_second = 100.0;
                      let spp = (sr as f64) / pixels_per_second;
                      let (mins, maxs, _) = wf.bins_for(spp, 0, 0, usize::MAX);
                      
                      let data = ImportResult {
                            mins: mins.to_vec(),
                            maxs: maxs.to_vec(), 
                            duration: wf.duration_secs,
                            bins_per_second: pixels_per_second,
                            bpm: None,
                            color: String::new(),
                      };
                      
                      state.cache.lock().unwrap().insert(path_key, data);
                 }
             }
        }
    }

    // 3. Build UI State (Reuse Helper)
    // Pass cache AND color store
    let state = build_ui_state(&app, tracks_info, bpm, master_gain, false, &state.cache, &state.track_colors)?;

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

// ==========================================================
// 🚀 AI CHATBOT IMPLEMENTATION (NEW)
// ==========================================================

#[derive(serde::Deserialize)]
struct GroqApiResponse {
    choices: Vec<GroqChoice>,
}

#[derive(serde::Deserialize)]
struct GroqChoice {
    message: GroqMessage,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GroqMessage {
    role: String,
    content: String,
}

#[derive(serde::Serialize)]
struct AiErrorResponse {
    action: String,
    message: String,
}

#[tauri::command]
async fn ask_ai(user_input: String, track_context: String) -> Result<String, String> {
    // 1. Setup Client with Strict Timeout (Prevent UI Freeze)
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8)) // 8-second hard limit
        .build()
        .map_err(|e| e.to_string())?;

    // 2. Get API Key from Environment
    // NOTE: In production, you might want to load this from a user config file
    let api_key = std::env::var("GROQ_API_KEY").unwrap_or_else(|_| "".to_string());
    
    if api_key.is_empty() {
         return Ok(serde_json::to_string(&AiErrorResponse {
             action: "none".into(),
             message: "Error: Missing GROQ_API_KEY environment variable.".into()
         }).unwrap());
    }

    // 3. Construct System Prompt (Strict JSON Schema)
    let system_prompt = format!(
        "You are an intelligent assistant for a DAW (Haven). \
        You control the app via JSON commands. \
        \n\nCONTEXT:\nTracks: [{}]\n\n\
        USER REQUEST: '{}'\n\n\
        RESPONSE SCHEMA (Strict JSON Only):\n\
        {{ \n\
          \"action\": \"set_gain\" | \"set_pan\" | \"toggle_mute\" | \"toggle_solo\" | \"split_clip\" | \"delete_track\" | \"create_track\" | \"undo\" | \"redo\" | \"clarify\" | \"none\", \n\
          \"parameters\": {{ \n\
            \"track_id\": number (optional), \n\
            \"value\": number (optional), \n\
            \"time\": number (optional) \n\
          }}, \n\
          \"message\": \"User-friendly confirmation text\", \n\
          \"confidence\": 0.0-1.0 \n\
        }}\n\n\
        RULES:\n\
        1. If user input is ambiguous or missing track info, return action='clarify'.\n\
        2. If unrelated to audio/DAW, return action='none'.\n\
        3. Tracks are 1-based IDs.\n\
        4. Do NOT output markdown or explanations outside JSON.",
        track_context, user_input
    );

    // 4. Construct Request Payload
    let payload = serde_json::json!({
        "model":   "qwen/qwen3-32b", //"qwen-2.5-72b-instruct",  //"llama3-70b-8192", // Fast & Good at JSON
        "messages": [
            {   "role": "system",
                "content": system_prompt }
        ],
        "response_format": { "type": "json_object" },

        "temperature" : 0, // Low creativity = High accuracy for code
        "max_tokens" : 600, // Prevent rambling
        "top_p": 1.0,   // Standard sampling
        "stream": false // We need full JSON to execute
    });

    // 5. Send Request
    let res = client.post("https://api.groq.com/openai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Network Error: {}", e))?;

    if !res.status().is_success() {
        return Ok(serde_json::to_string(&AiErrorResponse {
            action: "none".into(),
            message: format!("AI Service Error: {}", res.status())
        }).unwrap());
    }

    // 6. Parse Response
    let chat_res: GroqApiResponse = res.json().await.map_err(|e| format!("Parse Error: {}", e))?;
    
    if let Some(choice) = chat_res.choices.first() {
        Ok(choice.message.content.clone())
    } else {
         Ok(serde_json::to_string(&AiErrorResponse {
             action: "none".into(),
             message: "AI returned empty response.".into()
         }).unwrap())
    }
}

fn main() {

    dotenv().ok();
    let runtime = AudioRuntime::new(None).expect("Failed to init Audio Engine");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .manage(AppState {
            audio: Mutex::new(runtime),
            recorder: Mutex::new(None),
            cache: Mutex::new(HashMap::new()),
            track_colors: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            play,
            pause,
            import_tracks,
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
            get_eq_state,
            get_output_devices,
            get_input_devices,
            undo,
            redo,
            ask_ai
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
    
}