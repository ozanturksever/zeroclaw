//! Dink edge mesh integration for ZeroClaw.
//!
//! Provides connectivity to the OOSS platform via Dink RPC:
//! - `DinkRuntime`: manages EdgeClient/CenterClient lifecycle
//! - `DinkToolProvider`: discovers edge services and creates Tool instances
//! - `DinkServiceTool`: wraps a single RPC method as a ZeroClaw Tool
//! - `PeerMessageTool`: inter-instance messaging via peer groups
//! - `ZeroClawEdgeService`: exposes this agent as a callable Dink service

pub mod generated;
pub mod runtime;
pub mod tool_provider;
pub mod service_tool;
pub mod peer_tool;
pub mod edge_service;
pub mod channel;

pub use runtime::DinkRuntime;
pub use tool_provider::DinkToolProvider;
pub use service_tool::DinkServiceTool;
pub use peer_tool::PeerMessageTool;
pub use edge_service::{ZeroClawEdgeService, AgentRequest, AgentResponse, InstanceStatus};
pub use channel::DinkChannel;

use std::sync::Arc;
use crate::tools::traits::Tool;

/// Discover and add Dink tools to an existing tool registry.
/// Must be called from an async context since discovery requires network I/O.
pub async fn add_dink_tools(
    tools: &mut Vec<Box<dyn Tool>>,
    config: &crate::config::Config,
    dink_runtime: Arc<DinkRuntime>,
) {
    if !config.dink.enabled {
        return;
    }
    match DinkToolProvider::discover(&config.dink, dink_runtime.clone()).await {
        Ok(dink_tools) => {
            let count = dink_tools.len();
            tracing::info!("discovered {count} Dink tools");
            tools.extend(dink_tools);
        }
        Err(e) => tracing::warn!(error = %e, "Dink tool discovery failed"),
    }
    if config.dink.services.iter().any(|s| s == "*" || s.contains("peer")) {
        tools.push(Box::new(PeerMessageTool::new(dink_runtime)));
    }
}

/// Start the Dink edge listener as a standalone agent loop.
///
/// This function:
/// 1. Connects a `DinkRuntime` and exposes `ZeroClawEdgeService`
/// 2. Creates an `Agent` from the config
/// 3. Reads `AgentRequest` messages from the edge service's mpsc channel
/// 4. Processes each through `Agent::turn()` and sends the response back
///
/// Runs indefinitely until the edge service channel closes.
/// Should be spawned as a tokio task alongside `start_channels`.
pub async fn start_dink_listener(config: &crate::config::Config) -> anyhow::Result<()> {
    if !config.dink.enabled {
        return Ok(());
    }

    let runtime = DinkRuntime::new(&config.dink)
        .await
        .map_err(|e| anyhow::anyhow!("Dink runtime connection failed: {e:#}"))?;

    let (edge_service, mut agent_rx) = ZeroClawEdgeService::new();
    runtime.expose_service(Arc::new(edge_service)).await?;
    tracing::info!("Dink listener started \u{2014} ZeroClawService exposed, awaiting messages");

    let mut agent = crate::agent::Agent::from_config(config)?;

    while let Some(req) = agent_rx.recv().await {
        tracing::debug!(
            channel = %req.channel,
            "Dink listener: processing message"
        );

        let result = agent.turn(&req.message).await;

        let response = match result {
            Ok(text) => Ok(AgentResponse {
                response: text,
                tool_calls: Vec::new(),
                iterations: 0,
            }),
            Err(e) => Err(e),
        };

        // Send response back; ignore error if caller timed out
        let _ = req.response_tx.send(response);
    }

    tracing::info!("Dink listener finished \u{2014} edge service channel closed");
    Ok(())
}