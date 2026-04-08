pub mod ai_schema;
pub mod governance;
pub mod pipeline;

use serde::{Deserialize, Serialize};
use ai_schema::AiAction;

/// 🛠️ Observability Trace for Debugging AI Decisions
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AIExecutionTrace {
    pub raw_response: String,
    pub message: Option<String>,
    pub parsed_actions: Vec<AiAction>,
    pub normalized_actions: Vec<AiAction>,
    pub execution_order: Vec<AiAction>,
    pub errors: Vec<String>,
}