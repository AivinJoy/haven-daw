// haven/src-tauri/src/ai_transaction.rs

use daw_modules::ai::ai_schema::AiAction;
use crate::AppState;

#[tauri::command]
pub async fn execute_ai_transaction(
    version: String,
    commands: Vec<AiAction>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if version != "1.0" {
        return Err(format!("Unsupported AI Contract Version: {}", version));
    }

    let audio_runtime = state.audio.lock().map_err(|_| "Failed to lock audio state")?;
    
    match audio_runtime.apply_ai_batch(commands) {
        Ok(_) => Ok("Transaction applied safely and successfully.".to_string()),
        Err(e) => Err(format!("DSP Engine Execution Error: {}", e))
    }
}