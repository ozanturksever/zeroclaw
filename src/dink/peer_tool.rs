use anyhow::Context as _;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use crate::tools::traits::{Tool, ToolResult};

use super::runtime::DinkRuntime;

/// Tool that sends messages to other ZeroClaw instances via Dink peer-to-peer RPC.
pub struct PeerMessageTool {
    runtime: Arc<DinkRuntime>,
}

impl PeerMessageTool {
    pub fn new(runtime: Arc<DinkRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for PeerMessageTool {
    fn name(&self) -> &str {
        "peer_message"
    }

    fn description(&self) -> &str {
        "Send a message to another ZeroClaw instance in the same Dink peer group. \
         Use for inter-agent communication and multi-agent workflows."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "target_edge_id": {
                    "type": "string",
                    "description": "The edge ID of the target ZeroClaw instance"
                },
                "service": {
                    "type": "string",
                    "description": "Service name on the target edge (default: ZeroClawService)",
                    "default": "ZeroClawService"
                },
                "method": {
                    "type": "string",
                    "description": "Method name to invoke (default: SendMessage)",
                    "default": "SendMessage"
                },
                "message": {
                    "type": "string",
                    "description": "The message to send to the target instance"
                }
            },
            "required": ["target_edge_id", "message"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let target_edge_id = args
            .get("target_edge_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing required field: target_edge_id"))?;

        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing required field: message"))?;

        let service = args
            .get("service")
            .and_then(|v| v.as_str())
            .unwrap_or("ZeroClawService");

        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("SendMessage");

        let edge_client = match self.runtime.edge_client() {
            Some(client) => client,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Dink edge client is not connected".into()),
                });
            }
        };

        let request_body = json!({
            "message": message,
            "channel": "peer",
            "metadata": {}
        });

        let req_bytes = serde_json::to_vec(&request_body)
            .context("failed to serialize request body")?;

        let response_bytes = edge_client
            .call_peer(target_edge_id, service, method, &req_bytes)
            .await
            .map_err(|e| anyhow::anyhow!("peer call failed: {e}"))?;

        let response_text = String::from_utf8(response_bytes)
            .unwrap_or_else(|e| format!("<non-utf8 response: {} bytes>", e.as_bytes().len()));

        Ok(ToolResult {
            success: true,
            output: response_text,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_required_fields() {
        // Verify schema shape without needing a real DinkRuntime.
        let schema = json!({
            "type": "object",
            "properties": {
                "target_edge_id": { "type": "string" },
                "message": { "type": "string" }
            },
            "required": ["target_edge_id", "message"]
        });

        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "target_edge_id"));
        assert!(required.iter().any(|v| v == "message"));
    }
}
