//! Discovers Dink edge services and creates Tool instances for each allowed method.

use std::sync::Arc;

use anyhow::Result;
use serde_json::json;
use tracing::warn;

use crate::dink::runtime::DinkRuntime;
use crate::dink::service_tool::DinkServiceTool;
use crate::tools::traits::Tool;

/// Discovers Dink edge services and creates [`DinkServiceTool`] instances
/// for each allowed service+method combination.
pub struct DinkToolProvider;

/// Known methods for each service. Since EdgeInfo.services is just a list of
/// service name strings (no method enumeration), we hardcode known methods.
fn known_methods(service: &str) -> &'static [&'static str] {
    match service {
        "AgentToolsService" => &[
            "ExecCommand",
            "ReadFile",
            "WriteFile",
            "ListFiles",
            "SearchCodebase",
            "RunTests",
            "InstallPackage",
            "ExportPatch",
            "DeleteFile",
        ],
        "ZeroClawService" => &[
            "SendMessage",
            "GetStatus",
            "RecallMemory",
            "UpdateConfig",
            "Shutdown",
        ],
        "WorkspaceService" => &[
            "CreateSandbox",
            "DestroySandbox",
            "GetStatus",
            "ListSandboxes",
        ],
        "AgentService" => &[
            "CreateSession",
            "SendMessage",
            "GetEvents",
            "TerminateSession",
            "Health",
            "ListAgents",
        ],
        _ => &[],
    }
}

impl DinkToolProvider {
    /// Discover edge services and produce one [`Tool`] per allowed method.
    ///
    /// Returns an empty `Vec` (with warnings) when:
    /// - no center client is available
    /// - discovery fails
    /// - `config.services` is empty
    pub async fn discover(
        config: &crate::config::DinkConfig,
        runtime: Arc<DinkRuntime>,
    ) -> Result<Vec<Box<dyn Tool>>> {
        if config.services.is_empty() {
            return Ok(Vec::new());
        }

        let center = match runtime.center_client() {
            Some(c) => c,
            None => {
                warn!("DinkToolProvider: no center client available — skipping discovery");
                return Ok(Vec::new());
            }
        };

        let edges = match center
            .discover_edges(dink_sdk::DiscoverOptions {
                online_only: Some(true),
                ..Default::default()
            })
            .await
        {
            Ok(edges) => edges,
            Err(e) => {
                warn!("DinkToolProvider: edge discovery failed: {e:#} — returning no tools");
                return Ok(Vec::new());
            }
        };

        let wildcard = config.services.contains(&"*".to_string());
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();

        for edge in &edges {
            // edge.services is Vec<String> — just service names
            for service_name in &edge.services {
                if !wildcard && !config.services.contains(service_name) {
                    continue;
                }

                // Get known methods for this service
                let methods = known_methods(service_name);
                if methods.is_empty() {
                    // Unknown service — can't enumerate methods without metadata
                    continue;
                }

                for method in methods {
                    let schema = known_schema(service_name, method)
                        .unwrap_or_else(generic_schema);
                    let description = known_description(service_name, method);

                    tools.push(Box::new(DinkServiceTool::new(
                        runtime.clone(),
                        edge.id.clone(),
                        service_name.clone(),
                        method.to_string(),
                        description,
                        schema,
                    )));
                }
            }
        }

        Ok(tools)
    }
}

// ── Known schemas ────────────────────────────────────────────────────

/// Returns a hardcoded JSON Schema for well-known service methods.
fn known_schema(service: &str, method: &str) -> Option<serde_json::Value> {
    match (service, method) {
        // AgentToolsService
        ("AgentToolsService", "ExecCommand") => Some(json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to execute" },
                "cwd": { "type": "string", "description": "Working directory" },
                "env": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Environment variables" },
                "timeoutMs": { "type": "number", "description": "Timeout in milliseconds" },
                "sandboxId": { "type": "string", "description": "Target sandbox identifier" }
            },
            "required": ["command", "sandboxId"]
        })),
        ("AgentToolsService", "ReadFile") => Some(json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to read" },
                "encoding": { "type": "string", "description": "File encoding (default utf-8)" },
                "sandboxId": { "type": "string", "description": "Target sandbox identifier" }
            },
            "required": ["path", "sandboxId"]
        })),
        ("AgentToolsService", "WriteFile") => Some(json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to write" },
                "content": { "type": "string", "description": "File content" },
                "encoding": { "type": "string", "description": "File encoding (default utf-8)" },
                "createDirs": { "type": "boolean", "description": "Create parent directories if missing" },
                "sandboxId": { "type": "string", "description": "Target sandbox identifier" }
            },
            "required": ["path", "content", "sandboxId"]
        })),
        ("AgentToolsService", "ListFiles") => Some(json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path to list" },
                "recursive": { "type": "boolean", "description": "List recursively" },
                "pattern": { "type": "string", "description": "Glob pattern filter" },
                "sandboxId": { "type": "string", "description": "Target sandbox identifier" }
            },
            "required": ["path", "sandboxId"]
        })),
        ("AgentToolsService", "SearchCodebase") => Some(json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Search pattern (regex)" },
                "path": { "type": "string", "description": "Search root path" },
                "filePattern": { "type": "string", "description": "Glob to filter files" },
                "maxResults": { "type": "number", "description": "Maximum results to return" },
                "sandboxId": { "type": "string", "description": "Target sandbox identifier" }
            },
            "required": ["pattern", "sandboxId"]
        })),

        // ZeroClawService
        ("ZeroClawService", "SendMessage") => Some(json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "Message text" },
                "channel": { "type": "string", "description": "Target channel" },
                "metadata": { "type": "object", "additionalProperties": true, "description": "Arbitrary metadata" }
            },
            "required": ["message"]
        })),
        ("ZeroClawService", "GetStatus") => Some(json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })),
        ("ZeroClawService", "RecallMemory") => Some(json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Memory recall query" },
                "limit": { "type": "number", "description": "Maximum memories to return" }
            },
            "required": ["query"]
        })),
        ("ZeroClawService", "Shutdown") => Some(json!({
            "type": "object",
            "properties": {
                "graceful": { "type": "boolean", "description": "Wait for in-flight work to finish" }
            }
        })),

        _ => None,
    }
}

/// Returns a generic permissive schema for unknown services.
fn generic_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": true
    })
}

/// Returns a human-readable description for known methods, or a generic one.
fn known_description(service: &str, method: &str) -> String {
    match (service, method) {
        // AgentToolsService
        ("AgentToolsService", "ExecCommand") => {
            "Execute a shell command in a sandbox".into()
        }
        ("AgentToolsService", "ReadFile") => {
            "Read a file from a sandbox filesystem".into()
        }
        ("AgentToolsService", "WriteFile") => {
            "Write a file to a sandbox filesystem".into()
        }
        ("AgentToolsService", "ListFiles") => {
            "List files in a sandbox directory".into()
        }
        ("AgentToolsService", "SearchCodebase") => {
            "Search code in a sandbox using ripgrep".into()
        }

        // ZeroClawService
        ("ZeroClawService", "SendMessage") => {
            "Send a message to a ZeroClaw instance".into()
        }
        ("ZeroClawService", "GetStatus") => {
            "Get status of a ZeroClaw instance".into()
        }
        ("ZeroClawService", "RecallMemory") => {
            "Recall memories from a ZeroClaw instance".into()
        }
        ("ZeroClawService", "Shutdown") => {
            "Shutdown a ZeroClaw instance".into()
        }

        _ => format!("Call {service}.{method} via Dink RPC"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_schemas_are_valid_json_schema() {
        let cases = vec![
            ("AgentToolsService", "ExecCommand"),
            ("AgentToolsService", "ReadFile"),
            ("AgentToolsService", "WriteFile"),
            ("AgentToolsService", "ListFiles"),
            ("AgentToolsService", "SearchCodebase"),
            ("ZeroClawService", "SendMessage"),
            ("ZeroClawService", "GetStatus"),
            ("ZeroClawService", "RecallMemory"),
            ("ZeroClawService", "Shutdown"),
        ];

        for (svc, method) in &cases {
            let schema = known_schema(svc, method)
                .unwrap_or_else(|| panic!("missing schema for {svc}.{method}"));
            assert_eq!(schema["type"], "object", "{svc}.{method} schema type");
            assert!(
                schema.get("properties").is_some(),
                "{svc}.{method} should have properties"
            );
        }
    }

    #[test]
    fn unknown_service_returns_none() {
        assert!(known_schema("UnknownService", "Foo").is_none());
    }

    #[test]
    fn generic_schema_is_permissive() {
        let s = generic_schema();
        assert_eq!(s["type"], "object");
        assert_eq!(s["additionalProperties"], true);
    }

    #[test]
    fn known_description_returns_specific_text() {
        let desc = known_description("AgentToolsService", "ExecCommand");
        assert!(desc.contains("shell command"), "got: {desc}");
    }

    #[test]
    fn unknown_description_includes_service_and_method() {
        let desc = known_description("Foo", "Bar");
        assert!(desc.contains("Foo") && desc.contains("Bar"), "got: {desc}");
    }

    #[test]
    fn known_methods_returns_expected_counts() {
        assert!(known_methods("AgentToolsService").len() >= 5);
        assert!(known_methods("ZeroClawService").len() >= 4);
        assert!(known_methods("UnknownService").is_empty());
    }
}
