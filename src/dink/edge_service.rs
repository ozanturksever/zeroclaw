//! Exposes a ZeroClaw agent instance as a callable Dink edge service.
//!
//! The OOSS platform communicates with running ZeroClaw instances through
//! `ZeroClawEdgeService`, which implements `dink_sdk::ServiceHandler`.
//! Incoming RPC calls are forwarded to the agent loop via an mpsc channel.

use async_trait::async_trait;
use dink_sdk::{DinkError, ServiceDefinition, ServiceHandler};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A request forwarded from Dink RPC to the agent loop.
pub struct AgentRequest {
    pub message: String,
    pub channel: String,
    pub metadata: HashMap<String, String>,
    pub response_tx: tokio::sync::oneshot::Sender<anyhow::Result<AgentResponse>>,
}

/// The agent loop's response to a forwarded request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub response: String,
    pub tool_calls: Vec<super::generated::ToolCallRecord>,
    pub iterations: i32,
}

/// Snapshot of agent instance health/activity metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstanceStatus {
    pub status: String,
    pub memory_mb: f64,
    pub uptime_seconds: i64,
    pub messages_handled: i32,
    pub tool_calls_total: i32,
}

// ---------------------------------------------------------------------------
// RPC request/response envelopes (JSON-over-NATS)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ZcSendMessageRequest {
    message: String,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    metadata: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct ZcSendMessageResponse {
    response: String,
    tool_calls: Vec<super::generated::ToolCallRecord>,
    iterations: i32,
}

#[derive(Debug, Serialize)]
struct ZcGetStatusResponse {
    status: String,
    memory_mb: f64,
    uptime_seconds: i64,
    messages_handled: i32,
    tool_calls_total: i32,
}

#[derive(Debug, Serialize)]
struct ZcShutdownResponse {
    acknowledged: bool,
}

#[derive(Debug, Deserialize)]
struct ZcRecallMemoryRequest {
    query: String,
    #[serde(default = "default_recall_limit")]
    limit: i32,
}

fn default_recall_limit() -> i32 { 10 }

#[derive(Debug, Serialize)]
struct ZcRecallMemoryResponse {
    memories: Vec<serde_json::Value>,
    total: i32,
}

#[derive(Debug, Deserialize)]
struct ZcUpdateConfigRequest {
    #[serde(default)]
    config: HashMap<String, serde_json::Value>,
    #[serde(default)]
    restart: bool,
}

#[derive(Debug, Serialize)]
struct ZcUpdateConfigResponse {
    applied: bool,
    restart_scheduled: bool,
}

#[derive(Debug, Serialize)]
struct ZcStreamEvent {
    event_type: String,
    data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Service implementation
// ---------------------------------------------------------------------------

fn dink_err(msg: impl Into<String>) -> DinkError {
    DinkError::Service {
        service: "ZeroClawService".to_string(),
        method: String::new(),
        code: "HANDLER_ERROR".to_string(),
        message: msg.into(),
    }
}

/// Exposes a ZeroClaw agent as a `"ZeroClawService"` on the Dink edge mesh.
///
/// Create via [`ZeroClawEdgeService::new`] which returns both the handler
/// (for `EdgeClient::expose_service`) and a receiver channel (for the agent
/// loop to consume incoming messages).
pub struct ZeroClawEdgeService {
    agent_sender: Arc<RwLock<Option<tokio::sync::mpsc::Sender<AgentRequest>>>>,
    status: Arc<RwLock<InstanceStatus>>,
}

impl ZeroClawEdgeService {
    /// Creates a new edge service and returns the matching receiver channel.
    ///
    /// The agent loop should read from the returned `Receiver<AgentRequest>`,
    /// process each request, and send back a result on `response_tx`.
    pub fn new() -> (Self, tokio::sync::mpsc::Receiver<AgentRequest>) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let service = Self {
            agent_sender: Arc::new(RwLock::new(Some(tx))),
            status: Arc::new(RwLock::new(InstanceStatus {
                status: "initializing".to_string(),
                ..Default::default()
            })),
        };
        (service, rx)
    }

    /// Replace the current instance status snapshot.
    pub async fn update_status(&self, new_status: InstanceStatus) {
        let mut guard = self.status.write().await;
        *guard = new_status;
    }

    // -- private helpers ----------------------------------------------------

    /// Send a message through the agent channel and await the response with a
    /// 30-second timeout.
    async fn send_to_agent(
        &self,
        message: String,
        channel: String,
        metadata: HashMap<String, String>,
    ) -> dink_sdk::Result<AgentResponse> {
        let sender_guard = self.agent_sender.read().await;
        let sender = sender_guard
            .as_ref()
            .ok_or_else(|| dink_err("agent not started — no sender channel available"))?;

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        sender
            .send(AgentRequest {
                message,
                channel,
                metadata,
                response_tx,
            })
            .await
            .map_err(|_| dink_err("agent channel closed"))?;

        let resp = tokio::time::timeout(std::time::Duration::from_secs(30), response_rx)
            .await
            .map_err(|_| dink_err("agent response timed out after 30s"))?
            .map_err(|_| dink_err("agent response channel dropped"))?
            .map_err(|e| dink_err(format!("agent error: {e}")))?;

        Ok(resp)
    }
}

#[async_trait]
impl ServiceHandler for ZeroClawEdgeService {
    fn definition(&self) -> ServiceDefinition {
        ServiceDefinition {
            name: "ZeroClawService",
            version: "1.0.0",
            methods: &[
                "SendMessage",
                "StreamMessage",
                "GetStatus",
                "RecallMemory",
                "UpdateConfig",
                "Shutdown",
            ],
        }
    }

    async fn handle_request(&self, method: &str, req_data: &[u8]) -> dink_sdk::Result<Vec<u8>> {
        match method {
            "SendMessage" => {
                let req: ZcSendMessageRequest = serde_json::from_slice(req_data)
                    .map_err(|e| dink_err(format!("malformed SendMessage request: {e}")))?;

                let agent_resp = self
                    .send_to_agent(req.message, req.channel, req.metadata)
                    .await?;

                let resp = ZcSendMessageResponse {
                    response: agent_resp.response,
                    tool_calls: agent_resp.tool_calls,
                    iterations: agent_resp.iterations,
                };
                serde_json::to_vec(&resp)
                    .map_err(|e| dink_err(format!("serialization error: {e}")))
            }

            "GetStatus" => {
                let status = self.status.read().await;
                let resp = ZcGetStatusResponse {
                    status: status.status.clone(),
                    memory_mb: status.memory_mb,
                    uptime_seconds: status.uptime_seconds,
                    messages_handled: status.messages_handled,
                    tool_calls_total: status.tool_calls_total,
                };
                serde_json::to_vec(&resp)
                    .map_err(|e| dink_err(format!("serialization error: {e}")))
            }

            "Shutdown" => {
                {
                    let mut status = self.status.write().await;
                    status.status = "stopping".to_string();
                }
                let resp = ZcShutdownResponse {
                    acknowledged: true,
                };
                serde_json::to_vec(&resp)
                    .map_err(|e| dink_err(format!("serialization error: {e}")))
            }

            "RecallMemory" => {
                let req: ZcRecallMemoryRequest = serde_json::from_slice(req_data)
                    .map_err(|e| dink_err(format!("malformed RecallMemory request: {e}")))?;
                // TODO: Wire up actual memory/RAG retrieval from agent state.
                // For now return empty results.
                let resp = ZcRecallMemoryResponse {
                    memories: vec![],
                    total: 0,
                };
                serde_json::to_vec(&resp)
                    .map_err(|e| dink_err(format!("serialization error: {e}")))
            }

            "UpdateConfig" => {
                let req: ZcUpdateConfigRequest = serde_json::from_slice(req_data)
                    .map_err(|e| dink_err(format!("malformed UpdateConfig request: {e}")))?;
                // TODO: Apply config changes to the running agent instance.
                // For now acknowledge without applying.
                tracing::info!(keys = ?req.config.keys().collect::<Vec<_>>(), restart = req.restart, "UpdateConfig RPC received");
                let resp = ZcUpdateConfigResponse {
                    applied: !req.config.is_empty(),
                    restart_scheduled: req.restart,
                };
                serde_json::to_vec(&resp)
                    .map_err(|e| dink_err(format!("serialization error: {e}")))
            }

            other => Err(dink_err(format!("unknown method: {other}")))
        }
    }

    async fn handle_stream(
        &self,
        method: &str,
        req_data: &[u8],
        emit: Box<dyn Fn(Vec<u8>) -> dink_sdk::Result<()> + Send + Sync>,
    ) -> dink_sdk::Result<()> {
        match method {
            "StreamMessage" => {
                let req: ZcSendMessageRequest = serde_json::from_slice(req_data)
                    .map_err(|e| dink_err(format!("malformed StreamMessage request: {e}")))?;

                let agent_resp = self
                    .send_to_agent(req.message, req.channel, req.metadata)
                    .await?;

                // Emit the full response as a single "done" event.
                // TODO: Wire up true streaming once the agent loop supports it.
                let event = ZcStreamEvent {
                    event_type: "done".to_string(),
                    data: serde_json::to_value(&ZcSendMessageResponse {
                        response: agent_resp.response,
                        tool_calls: agent_resp.tool_calls,
                        iterations: agent_resp.iterations,
                    })
                    .map_err(|e| dink_err(format!("serialization error: {e}")))?,
                };
                let bytes = serde_json::to_vec(&event)
                    .map_err(|e| dink_err(format!("serialization error: {e}")))?;
                emit(bytes)?;

                Ok(())
            }

            other => Err(dink_err(format!(
                "streaming not supported for method: {other}"
            ))),
        }
    }
}
