//! Wraps a single Dink RPC method as a ZeroClaw [`Tool`].

use async_trait::async_trait;
use std::sync::Arc;

use crate::tools::traits::{Tool, ToolResult};
use super::runtime::DinkRuntime;

/// Maximum response size included in tool output (50 KB).
const MAX_OUTPUT_BYTES: usize = 50 * 1024;

/// Converts a CamelCase string to snake_case.
///
/// "AgentToolsService" → "agent_tools_service"
/// "ExecCommand" → "exec_command"
fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// A ZeroClaw [`Tool`] backed by a single Dink RPC call.
///
/// The tool serializes its arguments as JSON, forwards them to the target edge
/// via [`DinkRuntime::call_edge`], and returns the response as a [`ToolResult`].
pub struct DinkServiceTool {
    runtime: Arc<DinkRuntime>,
    edge_id: String,
    service_name: String,
    method_name: String,
    tool_name: String,
    tool_description: String,
    params_schema: serde_json::Value,
}

impl DinkServiceTool {
    /// Create a new tool wrapping `service_name.method_name` on `edge_id`.
    ///
    /// `tool_name` is derived automatically:
    /// `"dink_" + snake(service_name) + "_" + snake(method_name)`.
    pub fn new(
        runtime: Arc<DinkRuntime>,
        edge_id: String,
        service_name: String,
        method_name: String,
        description: String,
        params_schema: serde_json::Value,
    ) -> Self {
        let svc = service_name.strip_suffix("Service").unwrap_or(&service_name);
        let tool_name = format!(
            "dink_{}_{}",
            to_snake_case(svc),
            to_snake_case(&method_name),
        );

        Self {
            runtime,
            edge_id,
            service_name,
            method_name,
            tool_name,
            tool_description: description,
            params_schema,
        }
    }
}

#[async_trait]
impl Tool for DinkServiceTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.params_schema.clone()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        // Normalise empty/null args to `{}`.
        let args = if args.is_null() || args.as_object().map_or(false, |m| m.is_empty()) {
            serde_json::Value::Object(serde_json::Map::new())
        } else {
            args
        };

        let request_bytes = serde_json::to_vec(&args)?;

        let response_bytes = match self
            .runtime
            .call_edge(&self.edge_id, &self.service_name, &self.method_name, &request_bytes)
            .await
        {
            Ok(bytes) => bytes,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                });
            }
        };

        // Try to deserialise as a JSON value for clean output.
        let output = match serde_json::from_slice::<serde_json::Value>(&response_bytes) {
            Ok(val) => serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string()),
            // Fall back to raw UTF-8.
            Err(_) => String::from_utf8_lossy(&response_bytes).into_owned(),
        };

        // Truncate large outputs to avoid blowing up the LLM context.
        let output = if output.len() > MAX_OUTPUT_BYTES {
            // Find a valid UTF-8 boundary at or before the limit.
            let boundary = output.floor_char_boundary(MAX_OUTPUT_BYTES);
            let mut truncated = output[..boundary].to_string();
            truncated.push_str("\n... [truncated]");
            truncated
        } else {
            output
        };

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_conversion() {
        assert_eq!(to_snake_case("AgentToolsService"), "agent_tools_service");
        assert_eq!(to_snake_case("ExecCommand"), "exec_command");
        assert_eq!(to_snake_case("A"), "a");
        assert_eq!(to_snake_case("already_snake"), "already_snake");
        assert_eq!(to_snake_case("HTMLParser"), "h_t_m_l_parser");
        assert_eq!(to_snake_case(""), "");
    }

    #[test]
    fn tool_name_derivation() {
        // Verify the name logic: strip "Service" suffix, then snake_case.
        let svc = "AgentToolsService".strip_suffix("Service").unwrap_or("AgentToolsService");
        let name = format!(
            "dink_{}_{}",
            to_snake_case(svc),
            to_snake_case("ExecCommand"),
        );
        assert_eq!(name, "dink_agent_tools_exec_command");
    }
}
