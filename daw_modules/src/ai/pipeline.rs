// daw_modules/src/ai/pipeline.rs

use super::ai_schema::{AiAction, validate_payload, normalize_actions};
use super::AIExecutionTrace;

pub struct AIPipeline;

impl AIPipeline {
    /// 🧠 The Unified Engine Pipeline
    /// Takes the raw JSON string from the LLM and produces an execution trace.
    pub fn process_raw_response(raw_json: &str) -> AIExecutionTrace {
        let mut trace = AIExecutionTrace {
            raw_response: raw_json.to_string(),
            ..Default::default()
        };

        // 1. Parse & Validate Schema
        let envelope = match validate_payload(raw_json) {
            Ok(env) => env,
            Err(e) => {
                trace.errors.push(format!("Schema Validation Error: {}", e));
                return trace;
            }
        };
        trace.message = envelope.message.clone();
        trace.parsed_actions = envelope.commands.clone();

        // 2. Normalize & Govern (Clamp parameters, expand commands)
        let normalized = normalize_actions(envelope.commands);
        trace.normalized_actions = normalized.clone();

        // 3. Plan Execution (Sort by DSP Priority)
        // 1: Gain staging, 2: EQ, 3: Compression, 4: Automation, 5: Reverb
        let mut execution_order = normalized;
        execution_order.sort_by_key(|action| Self::get_priority(action));
        trace.execution_order = execution_order;

        trace
    }

    /// Ported TS Logic: DSP Signal Flow Priority
    fn get_priority(action: &AiAction) -> u8 {
        match action {
            AiAction::AutoGainStage { .. } => 1,
            AiAction::SetGain { .. } => 2,
            AiAction::AutoEq { .. } => 3,
            AiAction::UpdateEq { .. } => 4,
            AiAction::AutoCompress { .. } => 5,
            AiAction::UpdateCompressor { .. } => 6,
            AiAction::ClearVolumeAutomation { .. } => 7,
            AiAction::DuckVolume { .. } => 8,
            AiAction::RideVocalLevel { .. } => 9,
            AiAction::AutoReverb { .. } => 10,
            AiAction::UpdateReverb { .. } => 11,
            _ => 99, // Transport, UI, and structural commands go last
        }
    }
}