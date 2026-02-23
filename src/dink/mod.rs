//! Dink edge mesh integration for ZeroClaw.
//!
//! Provides connectivity to the OOSS platform via Dink RPC:
//! - `DinkRuntime`: manages EdgeClient/CenterClient lifecycle
//! - `DinkToolProvider`: discovers edge services and creates Tool instances
//! - `DinkServiceTool`: wraps a single RPC method as a ZeroClaw Tool
//! - `PeerMessageTool`: inter-instance messaging via peer groups
//! - `ZeroClawEdgeService`: exposes this agent as a callable Dink service

pub mod channel;
pub mod edge_service;
pub mod generated;
pub mod peer_tool;
pub mod runtime;
pub mod service_tool;
pub mod tool_provider;
pub mod watchdog;

pub use channel::DinkChannel;
pub use edge_service::{AgentRequest, AgentResponse, InstanceStatus, ZeroClawEdgeService};
pub use peer_tool::PeerMessageTool;
pub use runtime::DinkRuntime;
pub use service_tool::DinkServiceTool;
pub use tool_provider::DinkToolProvider;

use crate::tools::traits::Tool;
use std::sync::Arc;

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
    if config
        .dink
        .services
        .iter()
        .any(|s| s == "*" || s.contains("peer"))
    {
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
pub async fn start_dink_listener(
    config: &crate::config::Config,
    liveness: watchdog::DinkLiveness,
) -> anyhow::Result<()> {
    if !config.dink.enabled {
        return Ok(());
    }

    let runtime = DinkRuntime::new(&config.dink)
        .await
        .map_err(|e| anyhow::anyhow!("Dink runtime connection failed: {e:#}"))?;

    // -- Wire ConnectionMonitor → DinkLiveness --
    // The dink-sdk 0.3 EdgeClient fires event callbacks on NATS
    // disconnect/reconnect. We bridge those to our watchdog liveness.
    if let Some(monitor) = runtime.connection_monitor() {
        let mon = monitor.clone();
        let liv = liveness.clone();
        tokio::spawn(async move {
            loop {
                // Wait for disconnect
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    if !mon.is_connected() {
                        liv.mark_dead();
                        tracing::warn!("ConnectionMonitor: NATS disconnected → liveness marked dead");
                        break;
                    }
                }
                // Wait for reconnect
                mon.wait_for_reconnect().await;
                liv.mark_alive();
                tracing::info!("ConnectionMonitor: NATS reconnected → liveness marked alive");
            }
        });
    }

    // -- Watchdog: monitors liveness and exits when dead too long --
    let _watchdog = watchdog::spawn_watchdog(liveness.clone(), watchdog::WatchdogConfig::default());

    let (edge_service, mut agent_rx, mut config_rx) = ZeroClawEdgeService::new();
    let edge_service = Arc::new(edge_service);
    // Wrap in generated handler for dispatch + envelope unwrap
    let handler = Arc::new(generated::ZeroClawServiceHandler::new(edge_service.clone()));
    runtime.expose_service(handler).await?;
    tracing::info!("Dink listener started \u{2014} ZeroClawService exposed, awaiting messages");

    let mut agent = crate::agent::Agent::from_config(config)?;
    // Share the agent's memory with the edge service for RecallMemory RPC
    edge_service.set_memory(agent.memory_ref().clone()).await;

    // Mark as running
    edge_service
        .update_status(edge_service::InstanceStatus {
            status: "running".to_string(),
            ..Default::default()
        })
        .await;

    loop {
        tokio::select! {
            Some(req) = agent_rx.recv() => {
                tracing::debug!(
                    channel = %req.channel,
                    streaming = req.stream_delta_tx.is_some(),
                    "Dink listener: processing message"
                );
        let response = if let Some(delta_tx) = req.stream_delta_tx {
                    match agent.turn_streaming(&req.message, delta_tx).await {
                        Ok(text) => Ok(AgentResponse {
                            response: text,
                            tool_calls: Vec::new(),
                            iterations: 0,
                        }),
                        Err(e) => Err(e),
                    }
                } else {
                    match agent.turn(&req.message).await {
                        Ok(text) => Ok(AgentResponse {
                            response: text,
                            tool_calls: Vec::new(),
                            iterations: 0,
                        }),
                        Err(e) => Err(e),
                    }
                };
        let _ = req.response_tx.send(response);
            }
            Some(update) = config_rx.recv() => {
                tracing::info!(?update, "Applying runtime config update");
                agent.apply_config_update(&update);
            }
            else => break,
        }
    }

    tracing::info!("Dink listener finished \u{2014} edge service channel closed");
    Ok(())
}

/// Minimal HTTP health server for OOSS sandbox health checks.
/// Responds to GET /v1/health with 200 OK when alive, 503 when dead.
pub async fn start_health_server(liveness: Option<watchdog::DinkLiveness>) {
    use axum::{routing::get, Router};
    let port: u16 = std::env::var("OOSS_HEALTH_PORT")
        .unwrap_or_else(|_| "9468".to_string())
        .parse()
        .unwrap_or(9468);
    let app = Router::new()
        .route("/v1/health", get(move || {
            let alive = liveness.as_ref().map_or(true, |l| l.is_alive());
            async move {
                if alive {
                    (axum::http::StatusCode::OK, axum::Json(serde_json::json!({"status": "ok"})))
                } else {
                    (axum::http::StatusCode::SERVICE_UNAVAILABLE, axum::Json(serde_json::json!({"status": "unhealthy", "reason": "nats_disconnected"})))
                }
            }
        }));
    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("Health server failed to bind port {port}: {e}");
            return;
        }
    };
    tracing::info!("Health server listening on port {port}");
    let _ = axum::serve(listener, app).await;
}
