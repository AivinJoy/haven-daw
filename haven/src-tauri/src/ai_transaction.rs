// haven/src-tauri/src/ai_transaction.rs

use daw_modules::ai::ai_schema::AiAction;
use crate::AppState;
use std::collections::HashMap;

// --- SECURITY LIMIT ---
// Prevents the LLM from hallucinating thousands of nodes and stalling the audio thread.
const MAX_AUTOMATION_NODES: usize = 200;

#[tauri::command]
pub async fn execute_ai_transaction(
    version: String,
    commands: Vec<AiAction>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    
    // 1. Version Check
    if version != "1.0" {
        return Err(format!("Unsupported AI Contract Version: {}", version));
    }

    // 2. The Automation Firewall (Safety Guard)
    let mut automation_counts: HashMap<usize, usize> = HashMap::new();
    
    for cmd in &commands {
        if let AiAction::AddVolumeAutomation { track_id, .. } = cmd {
            let count = automation_counts.entry(*track_id).or_insert(0);
            *count += 1;
            if *count > MAX_AUTOMATION_NODES { return Err("Security Block".into()); }
        } else if let AiAction::DuckVolume { track_id, .. } = cmd {
            let count = automation_counts.entry(*track_id).or_insert(0);
            *count += 3; // Ducking creates an anchor, a duck, and a release node
            if *count > MAX_AUTOMATION_NODES { return Err("Security Block".into()); }
        }
    }

    // 3. Execution Phase
    let audio_runtime = state.audio.lock().map_err(|_| "Failed to lock audio state")?;
    
    match audio_runtime.apply_ai_batch(commands) {
        Ok(_) => Ok("Transaction applied safely and successfully.".to_string()),
        Err(e) => Err(format!("DSP Engine Execution Error: {}", e))
    }
}