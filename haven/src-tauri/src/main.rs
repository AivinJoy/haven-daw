// src-tauri/src/main.rs
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod stem_separation;
mod ai_transaction;
mod automation;
pub mod effects;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration};
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
pub struct PendingStemGroup {
    pub stems: HashMap<String, String>, // key: Stem Name (e.g., "vocals"), value: File Path
    pub original_track_id: u32,
}

// --- 1. Global State ---
pub struct AppState {
    pub audio: Mutex<AudioRuntime>,
    pub recorder: Mutex<Option<Recorder>>,
    pub cache: Mutex<HashMap<String, ImportResult>>,
    pub pending_stems: Mutex<HashMap<String, PendingStemGroup>>,
    pub master_meter: Arc<daw_modules::engine::metering::TrackMeters>,
    pub meter_registry: Arc<Mutex<HashMap<u32, Arc<daw_modules::engine::metering::TrackMeters>>>>,
}

// --- 2. Define Return Struct ---
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub mins: Vec<f32>,
    pub maxs: Vec<f32>,
    pub duration: f64,
    pub bins_per_second: f64,
    pub bpm: Option<f32>, // New field for BPM
    pub color: String,
}

// Helper function to build the UI state from the raw track list
fn build_ui_state(
    tracks_info: Vec<daw_modules::audio_runtime::FrontendTrackInfo>,
    bpm: f32,
    master_gain: f32,
    silent: bool,
    cache_store: &Mutex<HashMap<String, ImportResult>>,
    fx_data: Vec<(
        Vec<daw_modules::effects::equalizer::EqParams>, 
        daw_modules::effects::compressor::CompressorParams,
        daw_modules::effects::reverb::ReverbParams
    )>
) -> Result<ProjectState, String> {
    
    let mut results = Vec::new();

    for (i, info) in tracks_info.iter().enumerate() {
        
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
                clip_number: clip_info.clip_number, // <--- NEW
            });
        }

        // NEW: If a track has no clips, assume it's an empty track for recording
        let source_type = if info.clips.is_empty() {
            "mic".to_string()
        } else {
            "media".to_string()
        };

        let (eq, compressor, reverb) = fx_data[i].clone();

        results.push(LoadedTrack {
            id: track_id,
            name: info.name.clone(),
            color,
            clips: loaded_clips,
            gain: info.gain,
            pan: info.pan,
            muted: info.muted,
            solo: info.solo,
            source: source_type,
            volume_automation: info.volume_automation.clone(),
            eq,           // <--- Attach EQ to UI Payload
            compressor,
            reverb,
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
fn reload_audio_device(state: State<AppState>) -> Result<(), String> {
    let mut audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.reload_device().map_err(|e| e.to_string())?;
    Ok(())
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
fn set_output_device(device_name: String, state: State<AppState>) -> Result<(), String> {
    let mut audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.set_output_device(device_name).map_err(|e| e.to_string())?;
    Ok(())
}

// Helper: Resolve Stable ID -> Mutable Index
// Returns the current index of the track with the given ID.
pub fn resolve_track_index(
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
    pub message: String,
    pub progress: f64,
    pub visible: bool,
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

        // --- STEP 2: DECODING (Heavy) ---
        let _ = app.emit("progress-update", ProgressPayload { 
            message: format!("Decoding Audio Data {}...", file_num),
            progress: base_progress + (step_size * 0.15), 
            visible: true 
        });

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
async fn analyze_file(path: String, state: State<'_, AppState>) -> Result<ImportResult, String> {
    // Offload the heavy DSP work to a background thread
    let path_clone = path.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let (samples, sr, channels) = bpm::adapter::decode_to_vec(&path_clone)
            .map_err(|e| format!("Failed to decode: {}", e))?;
        
        let wf = Waveform::build_from_samples(&samples, sr, channels, 512);
        
        let mut det = bpm::BpmDetector::new(2048);
        let opts = bpm::BpmOptions { compute_beats: true, ..Default::default() };
        let detected_bpm = det.detect(&samples, channels, sr, opts).map(|res| res.bpm);

        let pixels_per_second = 100.0;
        let spp = (sr as f64) / pixels_per_second;
        let (mins, maxs, _) = wf.bins_for(spp, 0, 0, usize::MAX);

        Ok::<ImportResult, String>(ImportResult {
            mins: mins.to_vec(),
            maxs: maxs.to_vec(),
            duration: wf.duration_secs,
            bins_per_second: if wf.duration_secs > 0.0 { (mins.len() as f64) / wf.duration_secs } else { 0.0 },
            bpm: detected_bpm,
            color: "".to_string(), 
        })
    }).await.map_err(|e| e.to_string())??; // Double unwrap for thread panic & our error

    // Lock state quickly just to update cache
    if let Ok(mut cache) = state.cache.lock() {
        cache.insert(path, result.clone());
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
    hold_l: f32,
    hold_r : f32,
    rms_l: f32,
    rms_r: f32,
}

#[tauri::command]
fn get_master_meter(state: tauri::State<AppState>) -> Result<MasterMeterState, String> {
    // 100% Lock Free - Reads directly from atomics!
    let peak_l = f32::from_bits(state.master_meter.peak_l.load(std::sync::atomic::Ordering::Relaxed));
    let peak_r = f32::from_bits(state.master_meter.peak_r.load(std::sync::atomic::Ordering::Relaxed));

    let hold_l = f32::from_bits(state.master_meter.hold_l.load(std::sync::atomic::Ordering::Relaxed));
    let hold_r = f32::from_bits(state.master_meter.hold_r.load(std::sync::atomic::Ordering::Relaxed));

    let rms_l = f32::from_bits(state.master_meter.rms_l.load(std::sync::atomic::Ordering::Relaxed));
    let rms_r = f32::from_bits(state.master_meter.rms_r.load(std::sync::atomic::Ordering::Relaxed));
    
    Ok(MasterMeterState {
        peak_l,
        peak_r,
        hold_l,
        hold_r,
        rms_l,
        rms_r,
    })
}

#[tauri::command]
fn get_all_meters(
    state: State<AppState>,
) -> Result<Vec<daw_modules::audio_runtime::MeterSnapshot>, String> {

    let reg = state
        .meter_registry
        .lock()
        .map_err(|_| "meter registry poisoned")?;

    let mut results = Vec::with_capacity(reg.len());

    for (&track_id, meters) in reg.iter() {
        results.push(daw_modules::audio_runtime::MeterSnapshot {
            track_id,
            peak_l: f32::from_bits(meters.peak_l.load(std::sync::atomic::Ordering::Relaxed)),
            peak_r: f32::from_bits(meters.peak_r.load(std::sync::atomic::Ordering::Relaxed)),
            hold_l: f32::from_bits(meters.hold_l.load(std::sync::atomic::Ordering::Relaxed)),
            hold_r: f32::from_bits(meters.hold_r.load(std::sync::atomic::Ordering::Relaxed)),
            rms_l: f32::from_bits(meters.rms_l.load(std::sync::atomic::Ordering::Relaxed)),
            rms_r: f32::from_bits(meters.rms_r.load(std::sync::atomic::Ordering::Relaxed)),
        });
    }

    // ❌ Remove sorting (registry should not change order during playback)
    // results.sort_by_key(|m| m.track_id);

    Ok(results)
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
fn set_time_signature(numerator: u32, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    // We pass the numerator from the UI, and default the denominator to 4
    audio.set_time_signature(numerator, 4); 
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
        solo: false,
        source: "mic".to_string(),
        volume_automation: vec![],
        eq: vec![],
        compressor: daw_modules::effects::compressor::CompressorParams {
            is_active: false,
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 50.0,
            makeup_gain_db: 0.0,
        },
        reverb: daw_modules::effects::reverb::ReverbParams { 
            is_active: false, 
            room_size: 0.8, 
            damping: 0.5, 
            mix: 0.3, 
            width: 1.0, 
            pre_delay_ms: 10.0, 
            low_cut_hz: 100.0, 
            high_cut_hz: 8000.0 
        },
    })
}


// --- NEW: Tauri command for AI analysis ---
#[tauri::command]
async fn get_track_analysis(state: State<'_, AppState>) -> Result<Vec<daw_modules::audio_runtime::TrackAnalysisPayload>, String> {
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
    _app: tauri::AppHandle, 
    state: State<'_, AppState>
) -> Result<ProjectState, String> {
    
    // 1. Fetch Data from Memory (NO Disk I/O)
    let audio_runtime = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    
    let bpm = audio_runtime.bpm();
    let master_gain = audio_runtime.master_gain();
    let tracks_info = audio_runtime.get_tracks_list();
    let mut fx_data = Vec::new();
    for info in &tracks_info {
        let index = resolve_track_index(&tracks_info, info.id as u32)?;
        let eq = audio_runtime.get_eq_state(index);
        let comp = audio_runtime.get_compressor_state(index);
        let rev = audio_runtime.get_reverb_state(index); // Reverb included!
        fx_data.push((eq, comp, rev));
    }
    drop(audio_runtime); // Release lock

    // 2. Build UI State (Reuse Helper)
    // Pass cache AND color store
    let state_ui = build_ui_state(tracks_info, bpm, master_gain, false, &state.cache, fx_data)?;
    Ok(state_ui)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedClip {
    pub id: String,
    pub track_id: u32,
    pub name: String,
    pub path: String,
    pub start_time: f64,
    pub duration: f64,
    pub offset: f64,
    pub waveform: ImportResult,
    pub color: String,
    pub clip_number: usize, // <--- NEW
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedTrack {
    pub id: u32,
    pub name: String,
    pub color: String,
    pub clips: Vec<LoadedClip>,
    pub gain: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub source: String,
    pub volume_automation: Vec<daw_modules::engine::automation::AutomationNode<f32>>,
    pub eq: Vec<daw_modules::effects::equalizer::EqParams>,
    pub compressor: daw_modules::effects::compressor::CompressorParams,
    pub reverb: daw_modules::effects::reverb::ReverbParams,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectState {
    pub tracks: Vec<LoadedTrack>,
    pub bpm: f32,
    pub master_gain: f32,
}


#[tauri::command]
fn save_project(path: String, state: State<AppState>) -> Result<(), String> {
    let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
    audio.save_project(path)
}

#[tauri::command]
async fn export_project(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let _ = app.emit("progress-update", ProgressPayload { 
        message: "Rendering Project...".into(), progress: 0.0, visible: true 
    });
    
    // 1. Clone the app handle so we can move it into the background thread safely
    let app_clone = app.clone();
    
    // 2. Offload rendering to prevent UI freeze
    tauri::async_runtime::spawn_blocking(move || {
        // Extract the state and lock the mutex INSIDE the thread
        let state = app_clone.state::<AppState>();
        let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
        
        audio.export_project(path)
    }).await.map_err(|e| e.to_string())??; // Double unwrap: one for thread panic, one for our Result

    let _ = app.emit("progress-update", ProgressPayload { 
        message: "Export Complete".into(), progress: 100.0, visible: false 
    });
    
    Ok(())
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
    // --- FETCH FX STATES BEFORE DROPPING THE LOCK ---
    let mut fx_data = Vec::new();
    for info in &tracks_info {
        let index = resolve_track_index(&tracks_info, info.id as u32)?;
        let eq = audio_runtime.get_eq_state(index);
        let comp = audio_runtime.get_compressor_state(index);
        let rev = audio_runtime.get_reverb_state(index); // Reverb included!
        fx_data.push((eq, comp, rev));
    }
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
    let state_ui = build_ui_state(tracks_info, bpm, master_gain, false, &state.cache, fx_data)?;
    let _ = app.emit("load-percent", 100.0);
    let _ = app.emit("load-progress", "Ready");

    Ok(state_ui)
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
    active_track_id: Option<u32>, // <--- NEW: UI state directly passed
    playhead_time: f64,           // <--- NEW: Playhead context
    chat_history: Vec<GroqMessage>,
    state: tauri::State<'_, AppState> // <--- NEW: Access native audio engine!
) -> Result<String, String> {
    
    // 1. Setup Client with Strict Timeout
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8)) 
        .build()
        .map_err(|e| e.to_string())?;

    let api_key = option_env!("GROQ_API_KEY")
        .map(|s| s.to_string())
        .unwrap_or_else(|| std::env::var("GROQ_API_KEY").unwrap_or_default());
    
    if api_key.is_empty() {
         return Ok(serde_json::to_string(&AiErrorResponse {
             action: "none".into(),
             message: "Error: Missing GROQ_API_KEY environment variable.".into()
         }).unwrap());
    }

    // ==========================================
    // 🧠 2. THE RUST INTENT ENGINE (Scoring)
    // ==========================================
    let input_lower = user_input.to_lowercase();
    let input_words: Vec<&str> = input_lower
        .split(|c: char| c.is_ascii_punctuation() || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect();

    let mut intent_score = 0;
    let mut edit_score = 0;

    for &word in &input_words {
        match word {
            "mix" | "master" | "level" | "ride" | "duck" | "eq" | "compressor" | 
            "loudness" | "balance" | "automate" | "stage" | "dynamics" | "punchy" | "harsh" => intent_score += 2,
            "vocal" | "peak" | "plosive" | "gain" | "normalize" => intent_score += 1,
            "split" | "cut" | "move" | "merge" | "delete" | "clip" | "slice" => edit_score += 2,
            "timing" | "arrange" | "time" => edit_score += 1,
            _ => {}
        }
    }

    let is_global_request = input_words.iter().any(|&w| matches!(w, "master" | "mix" | "overall" | "final" | "entire" | "everything" | "all"));

    let mode = if intent_score >= 2 { "MIXING" } else if edit_score >= 2 { "EDITING" } else { "MINIMAL" };
    println!("🤖 AI Intent Evaluated -> Mode: {}, Global: {}", mode, is_global_request);

    // ==========================================
    // 🏗️ 3. CONTEXT BUILDER (Lock Audio State)
    // ==========================================
    let track_context_str = {
        let audio_runtime = state.audio.lock().map_err(|_| "Failed to lock audio")?;
        let tracks_info = audio_runtime.get_tracks_list();
        let bpm = audio_runtime.bpm();

        // Target Resolution
        let mut target_track_ids = Vec::new();
        if let Some(id) = active_track_id {
            target_track_ids.push(id);
        } else if !is_global_request {
            if let Some(track) = tracks_info.iter().find(|t| {
                input_words.contains(&t.id.to_string().as_str()) ||
                t.name.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|token| input_words.contains(&token))
            }) {
                target_track_ids.push(track.id as u32);
            }
        }

        // Fetch Heavy DSP only if needed
        let analysis_data = if mode == "MIXING" { audio_runtime.get_all_track_analysis() } else { Vec::new() };

        // Build Payload
        let mut track_payloads = Vec::new();
        for info in &tracks_info {
            let is_target = is_global_request || target_track_ids.contains(&(info.id as u32));

            let mut track_obj = serde_json::json!({
                "id": info.id,
                "name": info.name.to_lowercase(),
                "gain": (info.gain * 100.0).round() / 100.0,
                "pan": (info.pan * 100.0).round() / 100.0,
                "muted": info.muted,
                "solo": info.solo,
            });

            if mode != "MINIMAL" && is_target {
                let track_idx = resolve_track_index(&tracks_info, info.id as u32).unwrap_or(0);
                let obj = track_obj.as_object_mut().unwrap();

                // Add Clips
                let clips_json: Vec<_> = info.clips.iter().map(|c| {
                    serde_json::json!({
                        "clip_number": c.clip_number,
                        "start_time": (c.start_time * 100.0).round() / 100.0,
                        "duration": (c.duration * 100.0).round() / 100.0
                    })
                }).collect();
                obj.insert("clips".to_string(), serde_json::Value::Array(clips_json));

                // Add DSP
                if mode == "MIXING" {
                    obj.insert("eq".to_string(), serde_json::to_value(audio_runtime.get_eq_state(track_idx)).unwrap());
                    obj.insert("compressor".to_string(), serde_json::to_value(audio_runtime.get_compressor_state(track_idx)).unwrap());
                    obj.insert("reverb".to_string(), serde_json::to_value(audio_runtime.get_reverb_state(track_idx)).unwrap());

                    if let Some(analysis) = analysis_data.iter().find(|a| (a.track_id as u32) == (info.id as u32)) {
                        obj.insert("analysis".to_string(), serde_json::to_value(&analysis.analysis).unwrap());
                    }
                }
            }
            track_payloads.push(track_obj);
        }

        let context_json = serde_json::json!({
            "system_directive": format!("MODE: {}. You MUST ONLY apply actions to target_track_ids.", mode),
            "mode": mode,
            "target_track_ids": target_track_ids,
            "project": { "playhead_position_seconds": playhead_time, "bpm": bpm },
            "tracks": track_payloads
        });

        serde_json::to_string(&context_json).map_err(|e| e.to_string())?
    }; // 🔓 MUTEX LOCK DROPPED HERE! 

    // ==========================================
    // 🌐 4. SYSTEM PROMPT & API CALL
    // ==========================================
    let system_prompt = format!(
    "You are a strict Natural Language to JSON transducer for a DAW.

    OUTPUT RULES:
    - Return ONLY valid JSON.
    - Root keys: 'version' (MUST be '1.0'), 'commands', optional 'message', 'confidence', 'error'.
    - 'commands' is an array of OBJECTS. Do NOT output raw strings.
    - Every command object MUST have an 'action' key.
    - ALL parameters must be flat (no nesting).
    - NEVER explain your reasoning.

    CONTEXT:
    {}

    MISSING DATA FALLBACK (CRITICAL):
    - You MUST ONLY use 'target_track_ids' provided in the context.
    - If the user request requires a track ID or context not present, DO NOT GUESS.
    - Output EXACTLY: {{\"version\": \"1.0\", \"commands\": [], \"error\": \"missing_data\"}}

    ACTIONS:
    play, pause, record, rewind, seek, set_bpm, set_gain, set_master_gain, set_pan,
    toggle_mute, unmute, toggle_solo, unsolo, toggle_monitor,
    split_clip, move_clip, merge_clips, delete_clip, delete_track, create_track,
    undo, redo, update_eq, update_compressor, update_reverb, separate_stems,
    ride_vocal_level, duck_volume, auto_gain_stage, clear_volume_automation,
    auto_compress, auto_eq, auto_reverb, none.

    RULES:
    - Use 'playhead_position_seconds' for relative timing ('here').
    - split_clip → use 'time'.
    - merge_clips → use 'clip_number'.
    - Let the backend handle math and defaults. Provide ONLY the requested intent.

    SEMANTIC ACTIONS (Do not invent parameters, use exactly these):
    - auto_compress(style, intensity)
    - auto_eq(intent, intensity)
    - auto_reverb(space, intensity)
    - ride_vocal_level(target_lufs)
    - auto_gain_stage(target_lufs)

    MIX/MASTER CHAIN (If user asks for studio quality, output these exact OBJECTS):
    1. {{\"action\": \"auto_eq\", \"track_id\": <id>, \"intent\": \"clarity\", \"intensity\": 0.6}}
    2. {{\"action\": \"auto_compress\", \"track_id\": <id>, \"style\": \"vocal\", \"intensity\": 0.5}}
    3. {{\"action\": \"ride_vocal_level\", \"track_id\": <id>}}
    4. {{\"action\": \"auto_reverb\", \"track_id\": <id>, \"space\": \"room\"}}

    Respond strictly as JSON.",
    track_context_str
    );

    let mut messages_payload = Vec::new();
    messages_payload.push(serde_json::json!({ "role": "system", "content": system_prompt }));

    let history_limit = 6;
    let start_index = if chat_history.len() > history_limit { chat_history.len() - history_limit } else { 0 };
    
    for msg in &chat_history[start_index..] {
        let clean_role = match msg.role.as_str() {
            "user" => "user",
            "system" => "system",
            _ => "assistant",
        };
        messages_payload.push(serde_json::json!({ "role": clean_role, "content": msg.content }));
    }

    messages_payload.push(serde_json::json!({ "role": "user", "content": user_input }));

    let payload = serde_json::json!({
        "model": "llama-3.3-70b-versatile",
        "messages": messages_payload,
        "response_format": { "type": "json_object" },
        "temperature" : 0.0,
        "max_tokens" : 600,
        "stream": false
    });

    let res = client.post("https://api.groq.com/openai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Network Error: {}", e))?;

    if !res.status().is_success() {
        let error_text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        println!("❌ AI Error Body: {}", error_text); 
        return Ok(serde_json::to_string(&AiErrorResponse {
            action: "none".into(),
            message: "I'm having trouble thinking right now (API Error).".into() 
        }).unwrap());
    }

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

// IMPORTANT: Make sure this import is at the top of your main.rs file
// use tauri::Manager; 

#[tauri::command]
async fn commit_pending_stems(
    app: tauri::AppHandle,
    job_id: String,
    import_action: String,
    state: State<'_, AppState>
) -> Result<(), String> {
    
    // Notify UI immediately (No sleep required, optimistic updates handle the rest)
    let _ = app.emit("ai-progress", ProgressPayload { 
        message: "Importing stems...".into(), progress: 0.0, visible: true 
    });

    // 1. Retrieve the pending data
    let group = {
        let mut pending = state.pending_stems.lock().map_err(|_| "Failed to lock pending")?;
        pending.remove(&job_id).ok_or("Job ID not found or already processed")?
    };

    let app_clone = app.clone();
    
    // 2. Offload BOTH Engine Modification and Analysis to a blocking thread
    let computed_results = tauri::async_runtime::spawn_blocking(move || -> Result<Vec<(String, ImportResult)>, String> {
        
        // Extract AppState securely inside the spawned thread
        let state_handle = app_clone.state::<AppState>();
        let mut analysis_tasks: Vec<(String, String)> = Vec::new();

        // AUDIO LOCK SCOPE
        {
            let audio = state_handle.audio.lock().map_err(|_| "Failed to lock audio")?;

            // A. Handle Original Track
            let list = audio.get_tracks_list();
            if let Ok(index) = resolve_track_index(&list, group.original_track_id) { 
                match import_action.as_str() {
                    "replace" => { let _ = audio.delete_track(index); },
                    "mute" => {
                        let track_info = &list[index];
                        if !track_info.muted { audio.toggle_mute(index); }
                    },
                    _ => {} // "keep" does nothing
                }
            }

            // B. Add New Stems to Engine (Disk I/O)
            for (stem_name, path) in &group.stems {
                if !std::path::Path::new(path).exists() { continue; }
                
                if audio.add_track(path.clone()).is_ok() {
                    let list = audio.get_tracks_list();
                    let idx = list.len() - 1; 
                    audio.set_track_name(idx, stem_name.clone());
                    analysis_tasks.push((path.clone(), list[idx].color.clone()));
                }
            }
        } // <--- Audio Lock Drops Here (Playback continues smoothly)

        // Waveform Analysis (Heavy CPU)
        let mut results = Vec::new();
        for (path, color) in analysis_tasks {
            match analyze_audio_internal(&path, color) {
                Ok(res) => results.push((path, res)),
                Err(e) => println!("Failed to analyze stem {}: {}", path, e),
            }
        }
        Ok(results)
    }).await.map_err(|e| e.to_string())??;

    let total = computed_results.len();

    // 4. Update Cache & UI Loop
    for (i, (path, result)) in computed_results.into_iter().enumerate() {
        let _ = app.emit("ai-progress", ProgressPayload { 
            message: format!("Analyzing stem {}/{}...", i + 1, total), 
            progress: ((i as f64) / (total as f64)) * 100.0, 
            visible: true 
        });
        
        if let Ok(mut cache) = state.cache.lock() {
            cache.insert(path, result);
        }
    }

    let _ = app.emit("ai-progress", ProgressPayload { 
        message: "Done.".into(), progress: 100.0, visible: false 
    });

    Ok(())
}

#[tauri::command]
fn discard_pending_stems(job_id: String, state: State<AppState>) -> Result<(), String> {
    let mut pending = state.pending_stems.lock().map_err(|_| "Failed to lock pending")?;
    
    if let Some(group) = pending.remove(&job_id) {
        // Garbage collect orphaned audio files to prevent storage leaks
        for (_, path) in group.stems {
            let _ = std::fs::remove_file(&path);
        }
    }
    
    Ok(())
}

// --- COMMAND VALIDATION MIDDLEWARE ---
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct AiStep {
    pub action: String,
    pub parameters: Option<serde_json::Value>,
}

#[tauri::command]
fn sanitize_ai_batch(mut steps: Vec<AiStep>) -> Result<Vec<AiStep>, String> {
    // RULE 1: Engine handles solo isolation. 
    // Reject any AI-hallucinated mute commands if a solo command exists in the same batch.
    let has_solo = steps.iter().any(|s| s.action == "toggle_solo" || s.action == "unsolo");
    
    if has_solo {
        let original_len = steps.len();
        steps.retain(|s| s.action != "toggle_mute" && s.action != "unmute");
        
        if steps.len() < original_len {
            println!("🛡️ Engine Guard: Stripped illegal mute commands during a Solo operation.");
        }
    }

    // RULE 2: Prevent Contradictory Transport Commands
    let has_play = steps.iter().any(|s| s.action == "play");
    let has_pause = steps.iter().any(|s| s.action == "pause");
    
    if has_play && has_pause {
        let original_len = steps.len();
        // Safety first: if AI says both play and pause, keep pause and drop play
        steps.retain(|s| s.action != "play");
        if steps.len() < original_len {
            println!("🛡️ Engine Guard: Removed 'play' command because 'pause' was also requested.");
        }
    }

    // RULE 3: Gain/Volume Safety Limiter (Prevent speaker blowout)
    for step in &mut steps {
        if step.action == "set_gain" || step.action == "set_master_gain" {
            if let Some(params) = &mut step.parameters {
                if let Some(val) = params.get_mut("value") {
                    if let Some(mut float_val) = val.as_f64() {
                        // Clamp between 0.0 (mute) and 2.0 (+6dB max)
                        if float_val > 2.0 {
                            println!("🛡️ Engine Guard: Clamped dangerously high gain ({} -> 2.0)", float_val);
                            float_val = 2.0;
                        } else if float_val < 0.0 {
                            float_val = 0.0;
                        }
                        *val = serde_json::json!(float_val);
                    }
                }
            }
        }
    }

    Ok(steps)
}

fn main() {

    dotenv().ok();
    let runtime = AudioRuntime::new(None).expect("Failed to init Audio Engine");

    let master_meter = runtime.master_meter.clone();
    let meter_registry = runtime.meter_registry.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .manage(AppState {
            audio: Mutex::new(runtime),
            recorder: Mutex::new(None),
            cache: Mutex::new(HashMap::new()),
            pending_stems: Mutex::new(HashMap::new()),
            master_meter,
            meter_registry,
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
            set_time_signature,
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
            delete_track,
            delete_clip,
            update_eq,
            get_eq_state,
            update_compressor,
            get_compressor_state,
            effects::set_effect_param, 
            effects::get_reverb_state,
            reload_audio_device,
            get_output_devices,
            get_input_devices,
            set_output_device,
            undo,
            redo,
            ask_ai,
            ai_transaction::execute_ai_transaction,
            stem_separation::separate_stems,
            stem_separation::cancel_ai_job,
            commit_pending_stems,
            discard_pending_stems,
            sanitize_ai_batch,
            automation::get_volume_automation,
            automation::add_volume_automation_node,
            automation::remove_volume_automation_node
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
    
}