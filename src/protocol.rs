use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ExecCommandInput {
    pub cmd: String,
    pub yield_time_ms: Option<u64>,
    pub workdir: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WriteStdinInput {
    pub session_id: u64,
    pub chars: Option<String>,
    pub yield_time_ms: Option<u64>,
    pub kill_process: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolOutput {
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}
