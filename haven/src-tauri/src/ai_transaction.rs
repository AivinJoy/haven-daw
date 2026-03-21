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

    // 🛠️ DEBUG LOG 4: RUST ENTRY POINT
    println!("🦀 [AI_TRANSACTION] Received {} commands from Frontend. Version: {}", commands.len(), version);
    for (i, cmd) in commands.iter().enumerate() {
        println!("   👉 Incoming Command {}: {:?}", i, cmd);
    }
    
    // 1. Version Check
    if version != "1.0" {
        return Err(format!("Unsupported AI Contract Version: {}", version));
    }

    // 2. The Automation Firewall (Safety Guard)
    let mut automation_counts: HashMap<usize, usize> = HashMap::new();
    let mut rider_counts: HashMap<usize, usize> = HashMap::new(); // <--- NEW: Track rider calls
    
    for cmd in &commands {
        match cmd {
            AiAction::AddVolumeAutomation { track_id, .. } => {
                let count = automation_counts.entry(*track_id).or_insert(0);
                *count += 1;
                if *count > MAX_AUTOMATION_NODES { return Err("Security Block: Too many nodes".into()); }
            }
            AiAction::DuckVolume { track_id, .. } => {
                let count = automation_counts.entry(*track_id).or_insert(0);
                *count += 3;
                if *count > MAX_AUTOMATION_NODES { return Err("Security Block: Too many nodes".into()); }
            }
            AiAction::RideVocalLevel { track_id, .. } => { // <--- NEW: Rider Firewall Limit
                let count = rider_counts.entry(*track_id).or_insert(0);
                *count += 1;
                // SECURITY: Strictly allow only ONE rider process per track per transaction 
                // to prevent blocking the audio thread with heavy offline analysis.
                if *count > 1 { return Err("Security Block: Only one Vocal Rider per track allowed per transaction.".into()); }
            }
            _ => {} // Ignore other commands for firewall purposes
        }
    }

    // 3. Execution Phase
    let audio_runtime = state.audio.lock().map_err(|_| "Failed to lock audio state")?;
    
    match audio_runtime.apply_ai_batch(commands) {
        Ok(_) => Ok("Transaction applied safely and successfully.".to_string()),
        Err(e) => Err(format!("DSP Engine Execution Error: {}", e))
    }
}