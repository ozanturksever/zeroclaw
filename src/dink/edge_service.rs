//! Exposes a ZeroClaw agent instance as a callable Dink edge service.
//!
//! The OOSS platform communicates with running ZeroClaw instances through
//! `ZeroClawEdgeService`, which implements the generated `ZeroClawServiceServer`
//! trait. Incoming RPC calls are forwarded to the agent loop via an mpsc channel.

use async_trait::async_trait;
use dink_sdk::{DinkError, Result as DinkResult};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::generated::{
    GetStatusRequest, GetStatusResponse, MemoryEntry, RecallMemoryRequest, RecallMemoryResponse,
    SendMessageRequest, SendMessageResponse, ShutdownRequest, ShutdownResponse, ToolCallRecord,
    UpdateConfigRequest, UpdateConfigResponse, ZeroClawServiceServer,
};

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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentResponse {
    pub response: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub iterations: i32,
}

/// Snapshot of agent instance health/activity metrics.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct InstanceStatus {
    pub status: String,
    pub memory_mb: f64,
    pub uptime_seconds: i64,
    pub messages_handled: i32,
    pub tool_calls_total: i32,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn dink_err(msg: impl Into<String>) -> DinkError {
    DinkError::Service {
        service: "ZeroClawService".to_string(),
        method: String::new(),
        code: "HANDLER_ERROR".to_string(),
        message: msg.into(),
    }
}

/// Get current process RSS in MB (platform-specific).
fn get_process_memory_mb() -> f64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(contents) = std::fs::read_to_string("/proc/self/status") {
            for line in contents.lines() {
                if let Some(val) = line.strip_prefix("VmRSS:") {
                    let kb_str = val.trim().trim_end_matches(" kB").trim();
                    if let Ok(kb) = kb_str.parse::<u64>() {
                        return kb as f64 / 1024.0;
                    }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let pid = std::process::id();
        if let Ok(output) = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
        {
            if let Ok(rss_str) = String::from_utf8(output.stdout) {
                if let Ok(kb) = rss_str.trim().parse::<u64>() {
                    return kb as f64 / 1024.0;
                }
            }
        }
    }
    0.0
}

// ---------------------------------------------------------------------------
// Service implementation
// ---------------------------------------------------------------------------

/// Exposes a ZeroClaw agent as a `"ZeroClawService"` on the Dink edge mesh.
///
/// Create via [`ZeroClawEdgeService::new`] which returns both the service
/// (for wrapping with `ZeroClawServiceHandler`) and a receiver channel (for
/// the agent loop to consume incoming messages).
pub struct ZeroClawEdgeService {
    agent_sender: Arc<RwLock<Option<tokio::sync::mpsc::Sender<AgentRequest>>>>,
    status: Arc<RwLock<InstanceStatus>>,
    memory: Arc<RwLock<Option<Arc<dyn crate::memory::Memory>>>>,
    config_tx: tokio::sync::mpsc::Sender<crate::agent::RuntimeConfigUpdate>,
    started_at: std::time::Instant,
    messages_handled: std::sync::atomic::AtomicI32,
    tool_calls_total: std::sync::atomic::AtomicI32,
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
            started_at: std::time::Instant::now(),
            messages_handled: std::sync::atomic::AtomicI32::new(0),
            tool_calls_total: std::sync::atomic::AtomicI32::new(0),
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
    ) -> DinkResult<AgentResponse> {
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

        self.messages_handled
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.tool_calls_total.fetch_add(
            resp.tool_calls.len() as i32,
            std::sync::atomic::Ordering::Relaxed,
        );
        Ok(resp)
    }
}

#[async_trait]
impl ZeroClawServiceServer for ZeroClawEdgeService {
    async fn send_message(&self, req: SendMessageRequest) -> DinkResult<SendMessageResponse> {
        let agent_resp = self
            .send_to_agent(req.message, req.session_id.clone(), req.context)
            .await?;

        Ok(SendMessageResponse {
            response: agent_resp.response,
            session_id: req.session_id,
            tool_calls: agent_resp.tool_calls,
            duration_ms: 0,
            metadata: HashMap::new(),
        })
    }

    async fn stream_message(
        &self,
        req: SendMessageRequest,
        emit: Box<dyn Fn(Vec<u8>) -> DinkResult<()> + Send + Sync>,
    ) -> DinkResult<()> {
        tracing::info!(session = %req.session_id, "StreamMessage: starting");

        let (delta_tx, mut delta_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(128);
        let sender_guard = self.agent_sender.read().await;
        let sender = sender_guard
            .as_ref()
            .ok_or_else(|| dink_err("agent not started"))?;

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        sender
            .send(AgentRequest {
                message: req.message,
                channel: req.session_id,
                metadata: req.context,
                response_tx,
                stream_delta_tx: Some(delta_tx),
            })
            .await
            .map_err(|_| dink_err("agent channel closed"))?;
        tracing::debug!("StreamMessage: request sent to agent, awaiting deltas");

        let mut event_count = 0u32;
        while let Some(event_value) = delta_rx.recv().await {
            event_count += 1;
            let event_type = event_value
                .get("event_type")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            tracing::debug!(event_count, event_type, "StreamMessage: emitting event");
            let bytes = serde_json::to_vec(&event_value)
                .map_err(|e| dink_err(format!("serialization error: {e}")))?;
            emit(bytes)?;
        }

        tracing::info!(
            event_count,
            "StreamMessage: delta channel closed, awaiting final response"
        );
        let _resp = tokio::time::timeout(std::time::Duration::from_secs(120), response_rx)
            .await
            .map_err(|_| dink_err("stream response timed out"))?
            .map_err(|_| dink_err("agent response channel dropped"))?
            .map_err(|e| dink_err(format!("agent error: {e}")))?;
        tracing::info!("StreamMessage: complete");
        self.messages_handled
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Small delay to let the last emit task flush to NATS before
        // the edge SDK publishes the .done signal that closes the client subscription.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Ok(())
    }

    async fn get_status(&self, _req: GetStatusRequest) -> DinkResult<GetStatusResponse> {
        let status = self.status.read().await;
        let uptime_ms = self.started_at.elapsed().as_millis() as i64;
        let msgs = self
            .messages_handled
            .load(std::sync::atomic::Ordering::Relaxed);
        let tools = self
            .tool_calls_total
            .load(std::sync::atomic::Ordering::Relaxed);
        let memory_bytes = (get_process_memory_mb() * 1024.0 * 1024.0) as i64;

        Ok(GetStatusResponse {
            session_id: String::new(),
            status: status.status.clone(),
            model: String::new(),
            uptime_ms,
            messages_processed: msgs,
            tool_calls_made: tools,
            memory_usage_bytes: memory_bytes,
            config: HashMap::new(),
            metadata: HashMap::new(),
        })
    }

    async fn recall_memory(&self, req: RecallMemoryRequest) -> DinkResult<RecallMemoryResponse> {
        let memory_guard = self.memory.read().await;
        let entries = if let Some(mem) = memory_guard.as_ref() {
            let limit = if req.limit > 0 {
                req.limit as usize
            } else {
                10
            };
            mem.recall(&req.query, limit, None)
                .await
                .unwrap_or_default()
        } else {
            vec![]
        };

        let total = entries.len() as i32;
        let proto_entries: Vec<MemoryEntry> = entries
            .into_iter()
            .map(|e| MemoryEntry {
                id: e.id,
                content: e.content,
                relevance: e.score.unwrap_or(0.0) as f32,
                timestamp: e.timestamp.parse::<i64>().unwrap_or(0),
                tags: vec![e.category.to_string()],
                metadata: HashMap::new(),
            })
            .collect();

        Ok(RecallMemoryResponse {
            entries: proto_entries,
            total_matches: total,
        })
    }

    async fn update_config(&self, req: UpdateConfigRequest) -> DinkResult<UpdateConfigResponse> {
        tracing::info!(keys = ?req.config.keys().collect::<Vec<_>>(), restart = req.restart, "UpdateConfig RPC received");

        let mut update = crate::agent::RuntimeConfigUpdate::default();
        if let Some(model) = req.config.get("model") {
            update.model = Some(model.to_string());
        }
        if let Some(temp) = req.config.get("temperature") {
            if let Ok(t) = temp.parse::<f64>() {
                update.temperature = Some(t);
            }
        }
        if let Some(max_iter) = req.config.get("max_tool_iterations") {
            if let Ok(m) = max_iter.parse::<usize>() {
                update.max_tool_iterations = Some(m);
            }
        }
        if let Some(auto_save) = req.config.get("auto_save") {
            if let Ok(a) = auto_save.parse::<bool>() {
                update.auto_save = Some(a);
            }
        }

        let applied = update.model.is_some()
            || update.temperature.is_some()
            || update.max_tool_iterations.is_some()
            || update.auto_save.is_some();

        if applied {
            let _ = self.config_tx.send(update).await;
        }
        Ok(UpdateConfigResponse {
            applied,
            effective_config: HashMap::new(),
            restart_required: req.restart,
        })
    }

    async fn shutdown(&self, _req: ShutdownRequest) -> DinkResult<ShutdownResponse> {
        {
            let mut status = self.status.write().await;
            status.status = "stopping".to_string();
        }
        {
            let mut sender = self.agent_sender.write().await;
            *sender = None;
        }
        tracing::info!("Shutdown RPC received — agent channel closed");
        let msgs = self
            .messages_handled
            .load(std::sync::atomic::Ordering::Relaxed);
        let uptime_ms = self.started_at.elapsed().as_millis() as i64;
        Ok(ShutdownResponse {
            shutdown: true,
            messages_processed: msgs,
            uptime_ms,
        })
    }
}

// Delegation for Arc<ZeroClawEdgeService> so it can be shared while
// the ZeroClawServiceHandler owns one clone.
#[async_trait]
impl ZeroClawServiceServer for Arc<ZeroClawEdgeService> {
    async fn send_message(&self, req: SendMessageRequest) -> DinkResult<SendMessageResponse> {
        (**self).send_message(req).await
    }
    async fn stream_message(
        &self,
        req: SendMessageRequest,
        emit: Box<dyn Fn(Vec<u8>) -> DinkResult<()> + Send + Sync>,
    ) -> DinkResult<()> {
        (**self).stream_message(req, emit).await
    }
    async fn get_status(&self, req: GetStatusRequest) -> DinkResult<GetStatusResponse> {
        (**self).get_status(req).await
    }
    async fn recall_memory(&self, req: RecallMemoryRequest) -> DinkResult<RecallMemoryResponse> {
        (**self).recall_memory(req).await
    }
    async fn update_config(&self, req: UpdateConfigRequest) -> DinkResult<UpdateConfigResponse> {
        (**self).update_config(req).await
    }
    async fn shutdown(&self, req: ShutdownRequest) -> DinkResult<ShutdownResponse> {
        (**self).shutdown(req).await
    }
}
