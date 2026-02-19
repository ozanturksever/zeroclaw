//! DinkChannel — bridges Dink edge RPC into ZeroClaw's channel-based message bus.
//!
//! This implements the [`Channel`] trait so ZeroClaw treats Dink RPC messages
//! exactly like Telegram, Discord, etc. The OOSS platform sends messages via
//! `ZeroClawService.SendMessage` RPC → `ZeroClawEdgeService` forwards to an
//! `mpsc` channel → `DinkChannel::listen()` converts to `ChannelMessage` and
//! feeds into the standard dispatch loop.
//!
//! Responses flow back via a `oneshot` per request:
//! `dispatch_loop` → `DinkChannel::send()` → resolves the pending oneshot.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, warn};

use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use crate::dink::edge_service::{AgentRequest, AgentResponse};
use crate::dink::generated::ToolCallRecord;

/// A Dink-backed channel that receives messages from [`ZeroClawEdgeService`]
/// and sends responses back via oneshot channels.
///
/// # Lifecycle
///
/// 1. `ZeroClawEdgeService::new()` returns `Receiver<AgentRequest>`.
/// 2. Pass that receiver to `DinkChannel::new(rx)`.
/// 3. Add the `DinkChannel` to the channels vec in `start_channels`.
/// 4. The dispatch loop calls `send()` to deliver the agent's response.
pub struct DinkChannel {
    /// Receives AgentRequests from the edge service.
    rx: Mutex<Option<mpsc::Receiver<AgentRequest>>>,
    /// Maps message IDs to pending response oneshot senders.
    /// Key = message ID (generated in listen()), Value = oneshot::Sender.
    pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<anyhow::Result<AgentResponse>>>>>,
}

impl DinkChannel {
    pub fn new(rx: mpsc::Receiver<AgentRequest>) -> Self {
        Self {
            rx: Mutex::new(Some(rx)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Channel for DinkChannel {
    fn name(&self) -> &str {
        "dink"
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let mut rx = self
            .rx
            .lock()
            .await
            .take()
            .ok_or_else(|| anyhow::anyhow!("DinkChannel::listen called more than once"))?;

        let pending = Arc::clone(&self.pending);
        let mut msg_counter: u64 = 0;

        while let Some(agent_req) = rx.recv().await {
            msg_counter += 1;
            let msg_id = format!("dink-{msg_counter}");

            debug!(
                msg_id = %msg_id,
                channel = agent_req.channel,
                "DinkChannel received message from edge service"
            );

            // Stash the response sender so `send()` can resolve it later
            {
                let mut map = pending.lock().await;
                map.insert(msg_id.clone(), agent_req.response_tx);
            }

            let channel_msg = ChannelMessage {
                id: msg_id,
                sender: agent_req
                    .metadata
                    .get("sender")
                    .cloned()
                    .unwrap_or_else(|| "dink-rpc".to_string()),
                reply_target: agent_req
                    .metadata
                    .get("reply_to")
                    .cloned()
                    .unwrap_or_else(|| "dink-rpc".to_string()),
                content: agent_req.message,
                channel: if agent_req.channel.is_empty() {
                    "dink".to_string()
                } else {
                    agent_req.channel
                },
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                thread_ts: agent_req.metadata.get("thread_ts").cloned(),
            };

            if tx.send(channel_msg).await.is_err() {
                warn!("DinkChannel: message bus closed, stopping listener");
                break;
            }
        }

        debug!("DinkChannel listener finished — edge service channel closed");
        Ok(())
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        // The recipient field carries the message ID (set by the dispatch loop
        // as the reply_target from ChannelMessage).
        let msg_id = &message.recipient;

        let sender = {
            let mut map = self.pending.lock().await;
            map.remove(msg_id)
        };

        match sender {
            Some(tx) => {
                let response = AgentResponse {
                    response: message.content.clone(),
                    tool_calls: Vec::<ToolCallRecord>::new(),
                    iterations: 0,
                };
                // Ignore send error — receiver may have timed out
                let _ = tx.send(Ok(response));
                debug!(msg_id = %msg_id, "DinkChannel: sent response back to edge service");
            }
            None => {
                warn!(
                    msg_id = %msg_id,
                    "DinkChannel: no pending request for this message ID — response dropped"
                );
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn listen_converts_agent_request_to_channel_message() {
        let (edge_tx, edge_rx) = mpsc::channel::<AgentRequest>(8);
        let channel = DinkChannel::new(edge_rx);
        let (bus_tx, mut bus_rx) = mpsc::channel::<ChannelMessage>(8);

        // Spawn listener in background
        let listen_handle = tokio::spawn(async move { channel.listen(bus_tx).await });

        // Send a message through the edge service path
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
        edge_tx
            .send(AgentRequest {
                message: "hello from dink".to_string(),
                channel: "test".to_string(),
                metadata: HashMap::from([("sender".to_string(), "user-1".to_string())]),
                response_tx,
                stream_delta_tx: None,
            })
            .await
            .unwrap();

        // Should appear on the channel bus
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), bus_rx.recv())
            .await
            .expect("timed out")
            .expect("channel closed");

        assert_eq!(msg.content, "hello from dink");
        assert_eq!(msg.sender, "user-1");
        assert_eq!(msg.channel, "test");
        assert!(msg.id.starts_with("dink-"));

        // Drop edge_tx to close the channel and let listener finish
        drop(edge_tx);
        let _ = listen_handle.await;
    }

    #[tokio::test]
    async fn send_resolves_pending_oneshot() {
        let (edge_tx, edge_rx) = mpsc::channel::<AgentRequest>(8);
        let channel = DinkChannel::new(edge_rx);
        let (bus_tx, mut bus_rx) = mpsc::channel::<ChannelMessage>(8);

        let listen_handle = tokio::spawn({
            let pending = Arc::clone(&channel.pending);
            async move {
                // We need the channel to own its rx for listen, so we do it differently:
                // Just directly test send() after manually inserting a pending entry
                drop(bus_tx); // unused here
                pending
            }
        });

        // Create a oneshot and insert into pending
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        {
            let mut map = channel.pending.lock().await;
            map.insert("dink-42".to_string(), response_tx);
        }

        // Send response
        channel
            .send(&SendMessage::new("agent reply", "dink-42"))
            .await
            .unwrap();

        // Verify the oneshot resolved
        let result = response_rx.await.unwrap().unwrap();
        assert_eq!(result.response, "agent reply");

        let _ = listen_handle.await;
    }
}
