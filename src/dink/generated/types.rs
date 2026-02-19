//! Generated-equivalent Rust types mirroring proto messages.
//!
//! Dink codegen doesn't support Rust output, so these are manually maintained
//! to match the JSON wire format used by Dink (camelCase field names).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// AgentToolsService messages (from sandbox.proto)
// ---------------------------------------------------------------------------

// -- ExecCommand --

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecCommandRequest {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub timeout_ms: i32,
    #[serde(default)]
    pub sandbox_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResult {
    #[serde(default)]
    pub exit_code: i32,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecCommandResponse {
    #[serde(default)]
    pub result: ExecResult,
}

// -- ReadFile --

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileRequest {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub encoding: String,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub length: i64,
    #[serde(default)]
    pub sandbox_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default, rename = "type")]
    pub r#type: i32,
    #[serde(default)]
    pub mode: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileResponse {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub info: Option<FileInfo>,
}

// -- WriteFile --

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFileRequest {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub encoding: String,
    #[serde(default)]
    pub create_dirs: bool,
    #[serde(default)]
    pub mode: i32,
    #[serde(default)]
    pub sandbox_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFileResponse {
    #[serde(default)]
    pub info: Option<FileInfo>,
}

// -- SearchCodebase --

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCodebaseRequest {
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub file_pattern: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub max_results: i32,
    #[serde(default)]
    pub context_lines: i32,
    #[serde(default)]
    pub sandbox_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub line: i32,
    #[serde(default)]
    pub column: i32,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub context_before: Vec<String>,
    #[serde(default)]
    pub context_after: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCodebaseResponse {
    #[serde(default)]
    pub matches: Vec<SearchMatch>,
    #[serde(default)]
    pub total_matches: i32,
    #[serde(default)]
    pub truncated: bool,
}

// -- ListFiles --

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListFilesRequest {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default)]
    pub sandbox_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListFilesResponse {
    #[serde(default)]
    pub files: Vec<FileInfo>,
}

// ---------------------------------------------------------------------------
// ZeroClawService messages (from zeroclaw.proto)
// ---------------------------------------------------------------------------

// -- SendMessage --

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZcSendMessageRequest {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZcSendMessageResponse {
    #[serde(default)]
    pub response: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallRecord>,
    #[serde(default)]
    pub iterations: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    #[serde(default, rename = "type")]
    pub r#type: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub tool_args: String,
}

// -- GetStatus --

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZcGetStatusRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZcGetStatusResponse {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub memory_mb: f64,
    #[serde(default)]
    pub uptime_seconds: i64,
    #[serde(default)]
    pub messages_handled: i32,
    #[serde(default)]
    pub tool_calls_total: i32,
}

// -- RecallMemory --

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallMemoryRequest {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub limit: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub relevance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallMemoryResponse {
    #[serde(default)]
    pub entries: Vec<MemoryEntry>,
}

// -- UpdateConfig --

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConfigRequest {
    #[serde(default)]
    pub overrides: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConfigResponse {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub error: String,
}

// -- Shutdown --

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownRequest {
    #[serde(default)]
    pub graceful: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownResponse {
    #[serde(default)]
    pub acknowledged: bool,
}

// ---------------------------------------------------------------------------
// Default impls for nested structs used in response defaults
// ---------------------------------------------------------------------------

impl Default for ExecResult {
    fn default() -> Self {
        Self {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}
