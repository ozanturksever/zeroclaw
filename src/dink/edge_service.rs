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
    /// When set, the agent loop should use `turn_streaming` and relay deltas here.
    pub stream_delta_tx: Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
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
    memory: Arc<RwLock<Option<Arc<dyn crate::memory::Memory>>>>,
    config_tx: tokio::sync::mpsc::Sender<crate::agent::RuntimeConfigUpdate>,
}

impl ZeroClawEdgeService {
    /// Creates a new edge service and returns the matching receiver channel.
    ///
    /// The agent loop should read from the returned `Receiver<AgentRequest>`,
    /// process each request, and send back a result on `response_tx`.
    pub fn new() -> (
        Self,
        tokio::sync::mpsc::Receiver<AgentRequest>,
        tokio::sync::mpsc::Receiver<crate::agent::RuntimeConfigUpdate>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let (config_tx, config_rx) = tokio::sync::mpsc::channel(16);
        let service = Self {
            agent_sender: Arc::new(RwLock::new(Some(tx))),
            status: Arc::new(RwLock::new(InstanceStatus {
                status: "initializing".to_string(),
                ..Default::default()
            })),
            memory: Arc::new(RwLock::new(None)),
            config_tx,
        };
        (service, rx, config_rx)
    }

    /// Attach a memory backend for RecallMemory RPC.
    pub async fn set_memory(&self, memory: Arc<dyn crate::memory::Memory>) {
        let mut guard = self.memory.write().await;
        *guard = Some(memory);
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
                stream_delta_tx: None,
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

                let memory_guard = self.memory.read().await;
                let entries = if let Some(mem) = memory_guard.as_ref() {
                    let limit = if req.limit > 0 { req.limit as usize } else { 10 };
                    mem.recall(&req.query, limit, None)
                        .await
                        .unwrap_or_default()
                } else {
                    vec![]
                };

                let total = entries.len() as i32;
                let memories: Vec<serde_json::Value> = entries
                    .into_iter()
                    .map(|e| serde_json::json!({
                        "id": e.id,
                        "key": e.key,
                        "content": e.content,
                        "category": e.category.to_string(),
                        "timestamp": e.timestamp,
                        "score": e.score,
                    }))
                    .collect();

                let resp = ZcRecallMemoryResponse { memories, total };
                serde_json::to_vec(&resp)
                    .map_err(|e| dink_err(format!("serialization error: {e}")))
            }

            "UpdateConfig" => {
                let req: ZcUpdateConfigRequest = serde_json::from_slice(req_data)
                    .map_err(|e| dink_err(format!("malformed UpdateConfig request: {e}")))?;
                tracing::info!(keys = ?req.config.keys().collect::<Vec<_>>(), restart = req.restart, "UpdateConfig RPC received");

                // Build a RuntimeConfigUpdate from the JSON payload
                let mut update = crate::agent::RuntimeConfigUpdate::default();
                if let Some(model) = req.config.get("model").and_then(|v| v.as_str()) {
                    update.model = Some(model.to_string());
                }
                if let Some(temp) = req.config.get("temperature").and_then(|v| v.as_f64()) {
                    update.temperature = Some(temp);
                }
                if let Some(max_iter) = req.config.get("max_tool_iterations").and_then(|v| v.as_u64()) {
                    update.max_tool_iterations = Some(max_iter as usize);
                }
                if let Some(auto_save) = req.config.get("auto_save").and_then(|v| v.as_bool()) {
                    update.auto_save = Some(auto_save);
                }

                let applied = update.model.is_some()
                    || update.temperature.is_some()
                    || update.max_tool_iterations.is_some()
                    || update.auto_save.is_some();

                if applied {
                    let _ = self.config_tx.send(update).await;
                }
                let resp = ZcUpdateConfigResponse {
                    applied,
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
                tracing::info!(channel = %req.channel, "StreamMessage: starting");

                let (delta_tx, mut delta_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(128);
                let sender_guard = self.agent_sender.read().await;
                let sender = sender_guard
                    .as_ref()
                    .ok_or_else(|| dink_err("agent not started"))?;

                let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                sender
                    .send(AgentRequest {
                        message: req.message,
                        channel: req.channel,
                        metadata: req.metadata,
                        response_tx,
                        stream_delta_tx: Some(delta_tx),
                    })
                    .await
                    .map_err(|_| dink_err("agent channel closed"))?;
                tracing::debug!("StreamMessage: request sent to agent, awaiting deltas");

                let mut event_count = 0u32;
                while let Some(event_value) = delta_rx.recv().await {
                    event_count += 1;
                    let event_type = event_value.get("event_type").and_then(|v| v.as_str()).unwrap_or("?");
                    tracing::debug!(event_count, event_type, "StreamMessage: emitting event");
                    let bytes = serde_json::to_vec(&event_value)
                        .map_err(|e| dink_err(format!("serialization error: {e}")))?;
                    emit(bytes)?;
                }

                tracing::info!(event_count, "StreamMessage: delta channel closed, awaiting final response");
                let _resp = tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    response_rx,
                )
                .await
                .map_err(|_| dink_err("stream response timed out"))?
                .map_err(|_| dink_err("agent response channel dropped"))?
                .map_err(|e| dink_err(format!("agent error: {e}")))?;
                tracing::info!("StreamMessage: complete");
                // Small delay to let the last emit task flush to NATS before
                // the edge SDK publishes the .done signal that closes the client subscription.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                Ok(())
            }

            other => Err(dink_err(format!(
                "streaming not supported for method: {other}"
            ))),
        }
    }
}
