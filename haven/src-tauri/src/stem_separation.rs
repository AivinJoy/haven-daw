use std::collections::HashMap;
use std::time::Instant;
use tauri::{State, Emitter, Manager};
use stem_splitter_core::{split_file, SplitOptions, SplitProgress};

use crate::{AppState, PendingStemGroup, ProgressPayload, resolve_track_index};

#[tauri::command]
pub async fn separate_stems(
    app: tauri::AppHandle,
    track_id: u32,
    state: State<'_, AppState>
) -> Result<(), String> {
    
    // 1. PREPARATION (Brief Lock - Air-gapped from inference)
    let file_path = {
        let audio = state.audio.lock().map_err(|_| "Failed to lock audio")?;
        let list = audio.get_tracks_list();
        let index = resolve_track_index(&list, track_id)?;
        
        if index >= list.len() {
            return Err("Track index out of bounds".into());
        }
        
        let clip = list[index].clips.first()
            .ok_or("Track has no audio clips to separate")?;
            
        clip.path.clone()
    };

    let app_handle = app.clone();
    let job_id = uuid::Uuid::new_v4().to_string(); 
    
    // Tell Frontend the Job ID
    let _ = app_handle.emit("ai-job-started", job_id.clone());

    // 2. OFF-MAIN-THREAD EXECUTION
    tauri::async_runtime::spawn_blocking(move || {
        // We extract the state INSIDE the thread using the cloned app_handle
        // This solves the E0597 lifetime error perfectly!
        let state_handle = app_handle.state::<AppState>();

        log::info!("🧠 Haven AI Runtime: Initializing Stem Splitter...");
        let start_time = Instant::now();

        // Native Progress Routing (Download)
        let app_clone_dl = app_handle.clone();
        stem_splitter_core::set_download_progress_callback(move |downloaded, total| {
            let percent = if total > 0 { (downloaded as f64 / total as f64) * 100.0 } else { 0.0 };
            let _ = app_clone_dl.emit("ai-progress", ProgressPayload { 
                message: format!("Downloading AI Model... {:.0}%", percent), 
                progress: percent, 
                visible: true 
            });
        });

        // Native Progress Routing (Inference)
        let app_clone_split = app_handle.clone();
        stem_splitter_core::set_split_progress_callback(move |progress| {
            match progress {
                SplitProgress::Stage(stage) => {
                    let _ = app_clone_split.emit("ai-progress", ProgressPayload { 
                        message: format!("AI Engine: {}", stage), progress: 10.0, visible: true 
                    });
                }
                SplitProgress::Chunks { percent, .. } => {
                    let _ = app_clone_split.emit("ai-progress", ProgressPayload { 
                        message: format!("Processing audio chunks... {:.0}%", percent), 
                        progress: 10.0 + (percent as f64 * 0.8),
                        visible: true 
                    });
                }
                SplitProgress::Writing { stem, percent, .. } => {
                    let _ = app_clone_split.emit("ai-progress", ProgressPayload { 
                        message: format!("Writing {} stem... {:.0}%", stem, percent), 
                        progress: 90.0, visible: true 
                    });
                }
                SplitProgress::Finished => {
                    let _ = app_clone_split.emit("ai-progress", ProgressPayload { 
                        message: "Finalizing...".into(), progress: 100.0, visible: false 
                    });
                }
                // Removed the _ => {} warning
            }
        });

        // Save stems in a new folder right next to the original audio file
        let original_path = std::path::Path::new(&file_path);
        let parent_dir = original_path.parent().unwrap_or(std::path::Path::new("."));
        let file_stem = original_path.file_stem().unwrap_or_default().to_string_lossy();
        
        let mut out_dir = parent_dir.to_path_buf();
        out_dir.push(format!("{}_stems", file_stem)); // e.g., "Guitar_stems"
        let _ = std::fs::create_dir_all(&out_dir);

        let options = SplitOptions {
            output_dir: out_dir.to_string_lossy().to_string(),
            model_name: "htdemucs_ort_v1".to_string(),
            manifest_url_override: None,
        };

        log::info!("🚀 AI Engine: Executing ONNX Graph...");
        let inference_start = Instant::now();

        // INFERENCE EXECUTION
        match split_file(&file_path, options) {
            Ok(result) => {
                let duration = inference_start.elapsed();
                log::info!("✅ AI Engine: Inference complete in {:.2?}", duration);

                let mut stems = HashMap::new();
                stems.insert("vocals".to_string(), result.vocals_path);
                stems.insert("drums".to_string(), result.drums_path);
                stems.insert("bass".to_string(), result.bass_path);
                stems.insert("other".to_string(), result.other_path);

                let group = PendingStemGroup { stems, original_track_id: track_id };
            
                if let Ok(mut pending) = state_handle.pending_stems.lock() {
                    pending.insert(job_id.clone(), group);
                }
            
                let _ = app_handle.emit("ai-job-complete", job_id); 
            }
            Err(e) => {
                log::error!("❌ AI Engine Error: {}", e);
                let _ = app_handle.emit("ai-progress", ProgressPayload { 
                    message: format!("Inference Failed: {}", e), progress: 0.0, visible: false 
                });
            }
        }
        
        log::info!("⏱️ Total AI Task Time: {:.2?}", start_time.elapsed());
    });
    
    Ok(())
}

#[tauri::command]
pub async fn cancel_ai_job(app: tauri::AppHandle, job_id: String, state: State<'_, AppState>) -> Result<(), String> {
    log::warn!("🛑 Cancelling UI state for Job ID: {}.", job_id);
    
    let _ = app.emit("ai-progress", ProgressPayload { 
        message: "Cancelled.".into(), progress: 0.0, visible: false 
    });

    let mut pending = state.pending_stems.lock().map_err(|_| "Failed to lock pending")?;
    pending.remove(&job_id);
        
    Ok(())
}