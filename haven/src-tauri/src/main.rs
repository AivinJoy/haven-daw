// src-tauri/src/main.rs
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use std::collections::HashMap;
use tauri::{State, Emitter, Manager};
use cpal::traits::{HostTrait, DeviceTrait};
use dotenv::dotenv;

// Import modules
use daw_modules::audio_runtime::AudioRuntime;
use daw_modules::recorder::Recorder;
use daw_modules::waveform::Waveform;
use daw_modules::bpm; // Import the new BPM module
use daw_modules::engine::time::GridLine; // Import GridLine


// [NEW STRUCT] Holds separation results waiting for user confirmation
struct PendingStemGroup {
    stems: HashMap<String, String>, // key: Stem Name (e.g., "vocals"), value: File Path
    original_track_id: u32,
    replace_original: bool,
    mute_original: bool,
}

// --- 1. Global State ---
struct AppState {
    audio: Mutex<AudioRuntime>,
    recorder: Mutex<Option<Recorder>>,
    cache: Mutex<HashMap<String, ImportResult>>,
    pending_stems: Mutex<HashMap<String, PendingStemGroup>>,
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
    _app: &tauri::AppHandle, 
    tracks_info: Vec<daw_modules::audio_runtime::FrontendTrackInfo>,
    bpm: f32,
    master_gain: f32,
    silent: bool,
    cache_store: &Mutex<HashMap<String, ImportResult>>,
) -> Result<ProjectState, String> {
    
    let mut results = Vec::new();

    for info in tracks_info.iter() {
        
        // 1. Try to find existing color
        let color = info.color.clone();

        let track_id = info.id as u32;
        
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

// Helper: Resolve Stable ID -> Mutable Index
// Returns the current index of the track with the given ID.
fn resolve_track_index(
    tracks: &[daw_modules::audio_runtime::FrontendTrackInfo], 
    target_id: u32
) -> Result<usize, String> {
    tracks.iter()
        .position(|t| t.id as u32 == target_id)
        .ok_or_else(|| format!("Track ID {} not found (it may have been deleted)", target_id))
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
        // Capture the assigned color directly from the backend
        let assigned_color = {
            let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
            audio.add_track(path.clone()).map_err(|e| e.to_string())?;
            
            // Set Name and Get Color
            let track_list = audio.get_tracks_list();
            let id = track_list.len() - 1; 
            
            let filename = std::path::Path::new(&path)
                .file_name().unwrap_or_default().to_string_lossy().to_string();
            
            audio.set_track_name(id, filename);
            
            // Return the color the backend generated
            track_list[id].color.clone() 
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
    track_id: u32, 
    clip_index: usize, 
    new_time: f64, 
    state: State<AppState>
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;

    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;
    
    // Call the runtime logic we just added
    audio.move_clip(index, clip_index, new_time)
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
    let mut new_recorder = Recorder::start(PathBuf::from(path)).map_err(|e| e.to_string())?;
    
    // Detach the monitor and send it to the Audio Thread natively!
    if let Some(monitor) = new_recorder.monitor.take() {
        if let Ok(audio) = state.audio.lock() {
            audio.set_monitor(monitor);
        }
    }
    
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
    // Tell the audio thread to drop the monitor connection
    if let Ok(audio) = state.audio.lock() {
        audio.clear_monitor();
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
fn set_track_gain(track_id: u32, gain: f32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;

    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;

    audio.set_track_gain(index, gain);
    Ok(())
}

#[tauri::command]
fn get_master_gain(state: tauri::State<AppState>) -> Result<f32, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    Ok(audio.master_gain())
}

#[tauri::command]
fn set_master_gain(gain: f32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.set_master_gain(gain);
    Ok(())
}

#[derive(serde::Serialize)]
struct MasterMeterState {
    peak_l: f32,
    peak_r: f32,
    rms_l: f32,
    rms_r: f32,
}

#[tauri::command]
fn get_master_meter(state: tauri::State<AppState>) -> Result<MasterMeterState, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    
    // NOTE: Update your AudioRuntime::get_master_meter() to return a 4-tuple: 
    // (peak_l, peak_r, rms_l, rms_r) decoded from the atomic bits
    let (peak_l, peak_r, rms_l, rms_r) = audio.get_master_meter();
    
    Ok(MasterMeterState {
        peak_l,
        peak_r,
        rms_l,
        rms_r,
    })
}

#[tauri::command]
fn get_track_meters(state: tauri::State<AppState>) -> Result<Vec<daw_modules::audio_runtime::MeterSnapshot>, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    Ok(audio.get_meters())
}

#[tauri::command]
fn set_track_pan(track_id: u32, pan: f32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;

    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;

    audio.set_track_pan(index, pan);
    Ok(())
}

#[tauri::command]
fn toggle_mute(track_id: u32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;

    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;

    audio.toggle_mute(index);
    Ok(())
}

// src-tauri/src/main.rs

#[tauri::command]
fn toggle_solo(track_id: u32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;

    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;

    // Call the new simpler logic
    audio.toggle_solo(index); 
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
    track_id: u32, 
    path: String, 
    start_time: f64, 
    state: State<AppState>
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    // Note: track_id from frontend is 1-based, engine uses 0-based index?
    // Adjust index as needed based on your logic.
    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;

    audio.add_clip(index, path, start_time).map_err(|e| e.to_string())?;
    Ok(())
}

// [Refinement 1] Create Track: Source of Truth
// Returns the fully formed track data (ID, Name, Color) to the frontend.
#[tauri::command]
fn create_track(state: State<AppState>) -> Result<LoadedTrack, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;

    audio.create_empty_track().map_err(|e| e.to_string())?; //Creates new Track

    let tracks = audio.get_tracks_list();
    let info = tracks.last().ok_or("Track Creation Failed")?;

    let new_name = format!("Track-{}", info.id); //Track name
    let index = tracks.len() - 1;
    audio.set_track_name(index, new_name.clone());
    
    Ok(LoadedTrack {
        id: info.id as u32, 
        name: new_name,
        color: info.color.clone(),
        clips: vec![],
        gain: 1.0,
        pan: 0.0,
        muted: false,
        solo: false
    })
}

#[tauri::command]
fn get_all_meters(state: State<AppState>) -> Result<Vec<daw_modules::audio_runtime::MeterSnapshot>, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    Ok(audio.get_meters())
}

// --- NEW: Tauri command for AI analysis ---
#[tauri::command]
fn get_track_analysis(state: State<AppState>) -> Result<Vec<daw_modules::audio_runtime::TrackAnalysisPayload>, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    Ok(audio.get_all_track_analysis())
}

#[tauri::command]
fn split_clip(
    track_id: u32, 
    time: f64, 
    state: State<AppState>
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    
    // Frontend uses 1-based track IDs usually? 
    // If your frontend passes the array index (0-based), keep as is.
    // If frontend passes ID (1, 2...), subtract 1.
    // Based on 'move_clip' in your file, it seems direct mapping or handled there.
    // Let's assume track_index matches the Vec index.

    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;
    
    audio.split_clip(index, time).map_err(|e| e.to_string())?;
    
    Ok(())
}

#[tauri::command]
fn merge_clip_with_next(track_id: u32, clip_index: usize, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;

    audio.merge_clip_with_next(index, clip_index).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_track(track_id: u32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;

    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;

    audio.delete_track(index).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_clip(
    track_id: u32, 
    clip_index: usize, 
    state: State<AppState>
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;

    audio.delete_clip(index, clip_index).map_err(|e| e.to_string())
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
    track_id: u32,
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

    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, args.track_id)?;
    
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

    audio.update_eq(index, args.band_index, params);
    Ok(())
}

#[tauri::command]
fn get_eq_state(track_id: u32, state: State<AppState>) -> Result<Vec<daw_modules::effects::equalizer::EqParams>, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;

    Ok(audio.get_eq_state(index))
}

#[tauri::command]
fn update_compressor(
    track_id: u32, 
    params: daw_modules::effects::compressor::CompressorParams, 
    state: State<AppState>
) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;
    
    // Note: You will need to add this wrapper method to audio_runtime.rs / engine.rs 
    // exactly like you did for 'audio.update_eq(...)'!
    audio.update_compressor(index, params);
    Ok(())
}

#[tauri::command]
fn get_compressor_state(
    track_id: u32, 
    state: State<AppState>
) -> Result<daw_modules::effects::compressor::CompressorParams, String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock engine")?;
    let list = audio.get_tracks_list();
    let index = resolve_track_index(&list, track_id)?;

    // Note: You will need to add this wrapper method to audio_runtime.rs / engine.rs
    Ok(audio.get_compressor_state(index))
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
    let state = build_ui_state(&app, tracks_info, bpm, master_gain, true, &state.cache)?;
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
    let state = build_ui_state(&app, tracks_info, bpm, master_gain, false, &state.cache)?;

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

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
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
async fn ask_ai(
    user_input: String, 
    track_context: String,
    chat_history: Vec<GroqMessage>
) -> Result<String, String> {
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
    // 3. System Prompt (Strict JSON-Only API)
    // 3. System Prompt (Refined for Reset Logic & JSON Stability)
    let system_prompt = format!(
        "You are a strict JSON API for a DAW. You speak ONLY JSON.\n\
        \n\
        CONTEXT:\nTracks: [{}]\n\
        USER REQUEST: '{}'\n\
        \n\
        SCHEMA:\n\
        {{ \n\
          \"steps\": [ \n\
            {{ \n\
              \"action\": \"play\" | \"pause\" | \"record\" | \"seek\" | \"set_gain\" | \"set_master_gain\" | \"set_pan\" | \"toggle_monitor\" | \"toggle_mute\" | \"toggle_solo\" | \"unmute\" | \"unsolo\" | \"separate_stems\" | \"cancel_job\" | \"split_clip\" | \"delete_track\" | \"create_track\" | \"undo\" | \"redo\" | \"update_eq\" | \"update_compressor\" | \"none\", \n\
              \"parameters\": {{ \n\
                \"track_id\": number (optional, default to 0), \n\
                \"value\": number (optional), \n\
                \"time\": number (optional), \n\
                \"mute_original\": boolean (optional), \n\
                \"replace_original\": boolean (optional), \n\
                \"job_id\": string (optional), \n\
                \"band_index\": number (optional, 0-3), \n\
                \"filter_type\": \"LowPass\" | \"HighPass\" | \"Peaking\" | \"LowShelf\" | \"HighShelf\" | \"Notch\" | \"BandPass\" (optional), \n\
                \"freq\": number (optional), \n\
                \"q\": number (optional), \n\
                \"gain\": number (optional, -24.0 to 24.0), \n\
                \"threshold_db\": number (optional, -60.0 to 0.0), \n\
                \"ratio\": number (optional, 1.0 to 20.0), \n\
                \"attack_ms\": number (optional), \n\
                \"release_ms\": number (optional), \n\
                \"makeup_gain_db\": number (optional) \n\
              }} \n\
            }} \n\
          ], \n\
          \"message\": \"Short confirmation text or detailed conversational answer\" \n\
        }}\n\
        \n\
        RULES:\n\
        1. CRITICAL: YOU MUST OUTPUT A VALID JSON OBJECT. NO PLAIN TEXT.\n\
        2. STRICT DATA TYPES (PREVENT API CRASH): \n\
           - For 'value', 'track_id', and all numerical fields, YOU MUST USE REAL NUMBERS (e.g., 1.0, -1.0, 0). \n\
           - NEVER use strings like \"full\", \"max\", \"left\", or \"right\" for numbers. \n\
           - If the user does not specify a track number, you MUST assume \"track_id\": 0.\n\
           - NEVER output a parameter with a 'null' value. If a parameter is not needed, omit the key completely.\n\
        3. CONVERSATION VS COMMANDS:\n\
           - If user ASKS A QUESTION: Use action \"none\" and put the answer in \"message\".\n\
           - If user ISSUES A COMMAND: Use the correct action ('set_gain', 'set_pan', etc.). NEVER use \"none\" for a command.\n\
        4. GAIN/VOLUME SCALE:\n\
            - Range is 0.0 (Silence) to 2.0 (Max Volume). 1.0 is Unity.\n\
            - If user says 'max volume', use 'set_gain' with \"value\": 2.0.\n\
            - If user says 'half volume', use 'set_gain' with \"value\": 0.5.\n\
        5. PAN SCALE:\n\
            - Range is -1.0 (Full Left) to 1.0 (Full Right). 0.0 is Center.\n\
            - If user says 'pan right' or 'right pan full', use 'set_pan' with \"value\": 1.0.\n\
            - If user says 'pan left' or 'left pan full', use 'set_pan' with \"value\": -1.0.\n\
        6. MUTE & SOLO LOGIC:\n\
            - Use 'toggle_mute', 'unmute', 'toggle_solo', or 'unsolo' as actions where appropriate.\n\
        7. RESET LOGIC: When asked to 'Reset' a track or 'Reset Everything', neutralize active states:\n\
           - GAIN: Always set to 1.0.\n\
           - PAN: Always set to 0.0.\n\
           - MUTE: If context shows 'muted: true', generate 'unmute'.\n\
           - SOLO: If context shows 'solo: true', generate 'unsolo'.\n\
           - MONITOR: If context shows 'monitoring: true', generate 'toggle_monitor'.\n\
        8. EQ & COMPRESSION LOGIC:\n\
           - To EQ, use 'update_eq'. Bands: 0=Lows, 1=LowMids, 2=HighMids, 3=Highs. Default Q is 1.0.\n\
           - To Compress, use 'update_compressor'. Standard: threshold -20.0, ratio 4.0, attack 5.0, release 50.0.\n\
        ",
        track_context, user_input
    );


    // 4. Construct Message Chain (System -> History -> User)
    let mut messages_payload = Vec::new();
    
    // A. System Prompt
    messages_payload.push(serde_json::json!({ "role": "system", "content": system_prompt }));

    // B. Chat History (The Memory)
    // We limit history to last 6 messages to save tokens/speed
    let history_limit = 6;
    let start_index = if chat_history.len() > history_limit { chat_history.len() - history_limit } else { 0 };
    
    for msg in &chat_history[start_index..] {
        // Map 'ai' or anything else to strict 'assistant' just in case
        let clean_role = match msg.role.as_str() {
            "user" => "user",
            "system" => "system",
            _ => "assistant", // Fallback for 'ai' or invalid roles
        };
        messages_payload.push(serde_json::json!({ "role": clean_role, "content": msg.content }));
    }

    // C. Current User Input
    messages_payload.push(serde_json::json!({ "role": "user", "content": user_input }));

    // 4. Construct Request Payload
    let payload = serde_json::json!({
        "model":   "qwen/qwen3-32b", //"qwen-2.5-72b-instruct",  //"llama3-70b-8192", // Fast & Good at JSON
        "messages": messages_payload,
        "response_format": { "type": "json_object" },

        "temperature" : 0.0, // Low creativity = High accuracy for code
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

    // FIX 3: Capture the ACTUAL error message from Groq
    if !res.status().is_success() {
        let error_text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        println!("❌ AI Error Body: {}", error_text); // Prints to your terminal
        
        return Ok(serde_json::to_string(&AiErrorResponse {
            action: "none".into(),
            // Send a sanitized message to UI, but you see full error in terminal
            message: "I'm having trouble thinking right now (API Error).".into() 
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

// --- SHARED HELPER: Decodes audio & generates waveform data ---
fn analyze_audio_internal(path: &str, color: String) -> Result<ImportResult, String> {
    // 1. Decode (Heavy CPU)
    let (samples, sr, channels) = bpm::adapter::decode_to_vec(path)
        .map_err(|e| format!("Failed to decode: {}", e))?;

    // 2. Build Waveform
    let wf = Waveform::build_from_samples(&samples, sr, channels, 512);

    // 3. Calculate Bins
    let pixels_per_second = 100.0;
    let spp = (sr as f64) / pixels_per_second;
    let (mins, maxs, _) = wf.bins_for(spp, 0, 0, usize::MAX);

    let actual_bps = if wf.duration_secs > 0.0 { 
        (mins.len() as f64) / wf.duration_secs 
    } else { 
        0.0 
    };

    Ok(ImportResult {
        mins: mins.to_vec(),
        maxs: maxs.to_vec(),
        duration: wf.duration_secs,
        bins_per_second: actual_bps,
        bpm: None, // Stems inherit project BPM, so we skip detection to be faster
        color,
    })
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct SidecarJobResponse {
    pub job_id: String,
    pub status: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct SidecarStatusResponse {
    pub status: String,
    pub result: Option<HashMap<String, String>>, // Maps "vocals" -> "path/to/vocals.mp3"
    pub error: Option<String>,
}

// --- AI SIDECAR ---
// In production, use std::env::var or a config file
fn get_ai_base_url() -> String {
    std::env::var("AI_SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:8000".to_string())
}

#[tauri::command]
async fn separate_stems(
    app: tauri::AppHandle,
    track_id: u32,
    mute_original: bool,
    replace_original: bool,
    state: State<'_, AppState>
) -> Result<(), String> {
    
    let base_url = get_ai_base_url(); // <--- FIX 1: Capture string once

    // 1. PREPARATION (Brief Lock)
    let (file_path, duration_secs) = {
        let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
        let list = audio.get_tracks_list();
        let index = resolve_track_index(&list, track_id)?;
        
        if index >= list.len() {
            return Err("Track index out of bounds".into());
        }
        
        let clip = list[index].clips.first()
            .ok_or("Track has no audio clips to separate")?;
            
        (clip.path.clone(), clip.duration)
    };

    // Timeout: 4x realtime or min 60s
    let timeout_seconds = (duration_secs * 4.0).max(60.0) as usize;

    println!("✂️ Separating: {} (Timeout: {}s)", file_path, timeout_seconds);

    let app_handle = app.clone();
    
    tauri::async_runtime::spawn(async move {

        let state_handle = app_handle.state::<AppState>();
        let client = reqwest::Client::new();

        // 2. CONNECT (Fast Fail)
        let _ = app_handle.emit("ai-progress", ProgressPayload { 
            message: "Connecting...".into(), progress: 5.0, visible: true 
        });

        // FIX 2: Use base_url reference or clone to avoid move error
        let health_check = client.get(format!("{}/health", base_url))
            .timeout(Duration::from_secs(3)) 
            .send().await;

        if health_check.is_err() {
            // [CHANGE] Tell chat it failed
            let _ = app_handle.emit("ai-progress", ProgressPayload { 
                message: "Server unreachable.".into(), progress: 0.0, visible: false 
            });
            return
        }

        // 3. START JOB
        let _ = app_handle.emit("ai-progress", ProgressPayload { 
            message: "Uploading to GPU...".into(), progress: 10.0, visible: true 
        });

        // Send full config for Demucs
        let res = client.post(format!("{}/process/separate", base_url))
            .timeout(Duration::from_secs(10))
            .json(&serde_json::json!({ 
                "file_path": file_path, 
                "stem_count": 4,
                "model": "htdemucs",
                "device": "cuda",
                "format": "mp3",   
                "bitrate": 320       
            }))
            .send().await;
        
        // Handle Start Error
        let response = match res {
            Ok(r) => r,
            Err(e) => {
                let _ = app_handle.emit("ai-progress", ProgressPayload { 
                    message: format!("Failed: {}", e), progress: 0.0, visible: false 
                });
                return;
            }
        };

        let job_data_res: Result<SidecarJobResponse, _> = response.json().await;
        if job_data_res.is_err() {
             let _ = app_handle.emit("ai-progress", ProgressPayload { 
                message: "Invalid API response".into(), progress: 0.0, visible: false 
            });
            return;
        }

        let job_id = job_data_res.unwrap().job_id;
        // --- NEW: Tell Frontend the Job ID so it can be cancelled ---
        let _ = app_handle.emit("ai-job-started", job_id.clone());

        // 4. SMART POLLING LOOP
        let mut attempts = 0;
        loop {
            if attempts > timeout_seconds {
                // TIMEOUT: Try to cancel on server side
                let _ = client.post(format!("{}/jobs/{}/cancel", base_url, job_id))
                    .send().await;
                let _ = app_handle.emit("ai-progress", ProgressPayload { 
                    message: "Timed out.".into(), progress: 0.0, visible: false 
                });
                return
            }

            tokio::time::sleep(Duration::from_millis(1000)).await;

            // Poll Status
            let status_res = client.get(format!("{}/jobs/{}", base_url, job_id))
                .timeout(Duration::from_secs(5))
                .send().await;

            // Handle Polling Errors gracefully
            if status_res.is_err() {
                attempts += 1;
                continue; 
            }    

            // Unwrap safely because we checked is_err
            let status_data: SidecarStatusResponse = match status_res.unwrap().json().await {
                Ok(d) => d,
                Err(_) => continue,
            };
            // PROGRESS LOGIC: "Asymptotic Approach" (Never resets, slows down as it gets higher)
            // Formula: 20 + (70 * (1 - e^(-0.05 * t))) -> Approaches 90%
            let raw_progress = 1.0 - (-0.05 * (attempts as f64)).exp(); 
            let visual_progress = 20.0 + (70.0 * raw_progress);

            let stage_msg = match status_data.status.as_str() {
                "pending" => "Queued...",
                "processing" => "Separating Audio...",
                "completed" => "Finalizing...",
                "cancelled" => "Cancelled by user.",
                _ => "Thinking..."
            };

            let _ = app_handle.emit("ai-progress", ProgressPayload { 
                message: stage_msg.into(), 
                progress: visual_progress, 
                visible: true 
            });

            if status_data.status == "completed" {
                if let Some(stems) = status_data.result {

                    // 1. Create the Pending Group
                    let group = PendingStemGroup {
                        stems: stems.clone(),
                        original_track_id: track_id,
                        replace_original,
                        mute_original
                    };
                
                    // 2. Store it in AppState (Do NOT touch the audio engine yet)
                    if let Ok(mut pending) = state_handle.pending_stems.lock() {
                        pending.insert(job_id.clone(), group);
                    }
                
                    // 3. Notify Frontend to ask for confirmation
                    let _ = app_handle.emit("ai-job-complete", job_id); 
                }
                break;
            }
            else if status_data.status == "failed" {
                let _ = app_handle.emit("ai-progress", ProgressPayload { 
                    message: "Failed.".into(), progress: 0.0, visible: false 
                });
                return;
            } 
            else if status_data.status == "cancelled" {
                 let _ = app_handle.emit("ai-progress", ProgressPayload { 
                    message: "Cancelled.".into(), progress: 0.0, visible: false 
                });
                return
            }

            attempts += 1;
        }
    });
    
    Ok(())
}

#[tauri::command]
async fn cancel_ai_job(job_id: String) -> Result<(), String> {
    let base_url = get_ai_base_url();
    let client = reqwest::Client::new();
    
    println!("🛑 Requesting cancellation for Job ID: {}", job_id);

    let _ = client.post(format!("{}/jobs/{}/cancel", base_url, job_id))
        .send().await
        .map_err(|e| format!("Failed to send cancel request: {}", e))?;
        
    Ok(())
}


#[tauri::command]
async fn commit_pending_stems(
    app: tauri::AppHandle,
    job_id: String,
    state: State<'_, AppState>
) -> Result<(), String> {
    
    // 1. Retrieve the pending data
    let group = {
        let mut pending = state.pending_stems.lock().map_err(|_| "Failed to lock pending")?;
        pending.remove(&job_id).ok_or("Job ID not found or already processed")?
    };

    // Store tasks for analysis so we can do it WITHOUT holding the audio lock
    let mut analysis_tasks: Vec<(String, String)> = Vec::new();

    // 2. AUDIO LOCK SCOPE (Short duration to prevent stuttering)
    {
        let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;

        // A. Handle Original Track
        let list = audio.get_tracks_list();
        if let Ok(index) = resolve_track_index(&list, group.original_track_id) { 
            if group.replace_original {
                audio.delete_track(index).map_err(|e| e.to_string())?;
            } else if group.mute_original {
                audio.toggle_mute(index); 
            }
        }

        // B. Add New Stems to Engine
        for (stem_name, path) in &group.stems {
            if !std::path::Path::new(path).exists() { continue; }
            
            match audio.add_track(path.clone()) {
                Ok(_) => {
                    let list = audio.get_tracks_list();
                    let idx = list.len() - 1; 
                    audio.set_track_name(idx, stem_name.clone());
                    
                    // Capture path and assigned color for analysis
                    analysis_tasks.push((path.clone(), list[idx].color.clone()));
                },
                Err(e) => println!("Failed to commit stem {}: {}", path, e)
            }
        }
    } // <--- Audio Lock Drops Here (Playback continues smoothly)

    // Notify UI: Start
    let _ = app.emit("ai-progress", ProgressPayload { 
        message: "Importing stems...".into(), progress: 0.0, visible: true 
    });

    let total = analysis_tasks.len();

    // Move heavy work to a blocking thread to keep UI responsive
    // Clone cache (expensive but safe) or just pass State if possible. 
    // Actually, we can't easily pass State into spawn_blocking without Arc. 
    // Easier approach: Calculate results in blocking, return them, then lock cache in async.

    let computed_results = tauri::async_runtime::spawn_blocking(move || {
        let mut results = Vec::new();
        for (path, color) in analysis_tasks {
            // Internal Helper (Heavy CPU)
            match analyze_audio_internal(&path, color) {
                Ok(res) => results.push((path, res)),
                Err(e) => println!("Failed to analyze stem {}: {}", path, e),
            }
        }
        results
    }).await.map_err(|e| e.to_string())?;

    // 4. Update Cache & UI Loop
    for (i, (path, result)) in computed_results.into_iter().enumerate() {
        
        // Update UI Bubble
        let _ = app.emit("ai-progress", ProgressPayload { 
            message: format!("Analyzing stem {}/{}...", i + 1, total), 
            progress: ((i as f64) / (total as f64)) * 100.0, 
            visible: true 
        });
        
        // Small delay to let user see the bubble (optional, feels more 'reasoning-like')
        tokio::time::sleep(Duration::from_millis(120)).await;

        // Save to cache
        if let Ok(mut cache) = state.cache.lock() {
            cache.insert(path, result);
        }
    }

    // Notify UI: Done
    let _ = app.emit("ai-progress", ProgressPayload { 
        message: "Done.".into(), progress: 100.0, visible: false 
    });

    Ok(())
}

#[tauri::command]
fn discard_pending_stems(job_id: String, state: State<AppState>) -> Result<(), String> {
    let mut pending = state.pending_stems.lock().map_err(|_| "Failed to lock pending")?;
    pending.remove(&job_id); // Just remove from memory
    Ok(())
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
            pending_stems: Mutex::new(HashMap::new()),
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
            get_master_gain,
            get_master_meter,
            get_track_meters,
            save_project,
            load_project,
            export_project,
            get_temp_path,
            add_clip,
            get_all_meters,
            get_track_analysis,
            split_clip,
            get_project_state,
            merge_clip_with_next,
            delete_clip,
            delete_track,
            update_eq,
            get_eq_state,
            update_compressor,
            get_compressor_state,
            get_output_devices,
            get_input_devices,
            undo,
            redo,
            ask_ai,
            separate_stems,
            cancel_ai_job,
            commit_pending_stems,
            discard_pending_stems
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
    
}