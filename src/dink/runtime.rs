//! Dink edge mesh runtime — manages EdgeClient/CenterClient lifecycle.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use dink_sdk::center::CenterClient;
use dink_sdk::edge::{EdgeClient, ConnectionMonitor};
use dink_sdk::{CenterConfig, EdgeConfig, ServiceHandler};
use tracing::{debug, info, warn};

use crate::config::DinkConfig;

/// Manages the Dink SDK connection lifecycle for a ZeroClaw instance.
///
/// Optionally exposes the agent as an edge (when `expose_as_edge = true`)
/// and creates a center client for calling other edges when configured.
pub struct DinkRuntime {
    edge_client: Option<Arc<EdgeClient>>,
    center_client: Option<Arc<CenterClient>>,
    config: DinkConfig,
}

impl DinkRuntime {
    /// Create a new `DinkRuntime` from the given config.
    ///
    /// - If `config.expose_as_edge` is true and a non-empty `edge_key` is
    ///   provided, an `EdgeClient` is connected and made available.
    /// - A `CenterClient` is created when `center_api_key` is provided.
    pub async fn new(config: &DinkConfig) -> Result<Self> {
        let timeout = Duration::from_millis(config.request_timeout_ms);

        // ── Edge client (optional) ──────────────────────────────────
        let edge_client = if config.expose_as_edge && !config.edge_key.is_empty() {
            info!(
                "Dink: connecting as edge (labels: {:?})",
                config.edge_labels
            );

            let edge_config = EdgeConfig {
                api_key: config.edge_key.clone(),
                server_url: if config.server_url.is_empty() {
                    None
                } else {
                    Some(config.server_url.clone())
                },
                labels: config.edge_labels.clone(),
                timeout,
                ..EdgeConfig::default()
            };

            let client = EdgeClient::connect(edge_config)
                .await
                .context("Failed to connect Dink EdgeClient")?;

            info!(
                "Dink: edge connected (app={}, edge={})",
                client.app_id(),
                client.edge_id()
            );
            Some(Arc::new(client))
        } else {
            if config.expose_as_edge && config.edge_key.is_empty() {
                warn!("Dink: expose_as_edge is true but edge_key is empty — skipping EdgeClient");
            }
            None
        };

        // ── Center client (for calling other edges) ─────────────────
        let has_center_key = config
            .center_api_key
            .as_ref()
            .is_some_and(|k| !k.is_empty());

        let center_client = if has_center_key {
            debug!("Dink: connecting center client");

            let server_url = if config.server_url.is_empty() {
                // Fall back to Dink cloud default
                "nats://connect.dink.cloud:4222".to_string()
            } else {
                config.server_url.clone()
            };

            let center_config = CenterConfig {
                api_key: config.center_api_key.clone(),
                server_url,
                app_id: if config.app_id.is_empty() {
                    None
                } else {
                    Some(config.app_id.clone())
                },
                timeout,
            };

            let client = CenterClient::connect(center_config)
                .await
                .context("Failed to connect Dink CenterClient")?;

            info!("Dink: center client connected");
            Some(Arc::new(client))
        } else {
            debug!("Dink: no center_api_key configured — CenterClient unavailable");
            None
        };

        Ok(Self {
            edge_client,
            center_client,
            config: config.clone(),
        })
    }

    /// Returns the edge client if this instance is exposed as an edge.
    pub fn edge_client(&self) -> Option<&Arc<EdgeClient>> {
        self.edge_client.as_ref()
    }

    /// Returns the center client for calling other edges.
    pub fn center_client(&self) -> Option<&Arc<CenterClient>> {
        self.center_client.as_ref()
    }

    /// Returns a reference to the config used to create this runtime.
    pub fn config(&self) -> &DinkConfig {
        &self.config
    }

    /// Returns the ConnectionMonitor from the EdgeClient, if available.
    ///
    /// This provides real-time NATS connection state tracking via the
    /// dink-sdk 0.3 event_callback mechanism.
    pub fn connection_monitor(&self) -> Option<&ConnectionMonitor> {
        self.edge_client
            .as_ref()
            .map(|c| c.connection_monitor())
    }

    /// Whether the edge connection is currently alive.
    ///
    /// Returns `true` if no edge client is configured (nothing to be dead).
    pub fn is_connected(&self) -> bool {
        self.connection_monitor()
            .map_or(true, |m| m.is_connected())
    }

    /// Expose a service handler on the edge client.
    ///
    /// Fails if no edge client is available (i.e. `expose_as_edge` was false
    /// or `edge_key` was empty).
    pub async fn expose_service(&self, handler: Arc<dyn ServiceHandler>) -> Result<()> {
        let client = self.edge_client.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot expose service: EdgeClient is not connected \
                 (set expose_as_edge = true and provide an edge_key)"
            )
        })?;

        let def = handler.definition();
        info!(
            "Dink: exposing service '{}' ({} methods)",
            def.name,
            def.methods.len()
        );
        client.expose_service(handler).await?;
        Ok(())
    }

    /// Call a method on a specific edge via the center client.
    ///
    /// Fails if the center client is not available.
    pub async fn call_edge(
        &self,
        edge_id: &str,
        service: &str,
        method: &str,
        req: &[u8],
    ) -> Result<Vec<u8>> {
        let client = self.center_client.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot call edge: CenterClient is not connected \
                 (provide a center_api_key in dink config)"
            )
        })?;

        debug!(edge_id, service, method, "Dink: calling edge");
        let resp = client.call_edge(edge_id, service, method, req).await?;
        Ok(resp)
    }

    /// Call a typed method on a specific edge, serializing request and
    /// deserializing response as JSON.
    pub async fn call_typed<Req, Resp>(
        &self,
        edge_id: &str,
        service: &str,
        method: &str,
        req: &Req,
    ) -> Result<Resp>
    where
        Req: serde::Serialize,
        Resp: serde::de::DeserializeOwned,
    {
        let client = self.center_client.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot call edge: CenterClient is not connected \
                 (provide a center_api_key in dink config)"
            )
        })?;

        debug!(edge_id, service, method, "Dink: calling edge (typed)");
        let resp = client
            .call_typed::<Req, Resp>(edge_id, service, method, req)
            .await?;
        Ok(resp)
    }

    /// Disconnect both edge and center clients.
    pub async fn disconnect(&self) -> Result<()> {
        if let Some(ref client) = self.edge_client {
            info!("Dink: disconnecting edge client");
            client.disconnect().await?;
        }

        if let Some(ref client) = self.center_client {
            info!("Dink: disconnecting center client");
            client.disconnect().await?;
        }

        info!("Dink: fully disconnected");
        Ok(())
    }
}
