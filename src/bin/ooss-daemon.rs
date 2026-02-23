//! Minimal OOSS daemon entrypoint.
//!
//! Skips the full CLI (which depends on private upstream modules)
//! and goes straight to `daemon::run()` — the only mode OOSS containers use.

use std::process::ExitCode;

use zeroclaw::config::Config;

#[tokio::main]
async fn main() -> ExitCode {
    // Initialize tracing
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let config = match Config::load_or_init().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {e:#}");
            return ExitCode::FAILURE;
        }
    };

    let host = std::env::var("ZEROCLAW_GATEWAY_HOST").unwrap_or_else(|_| "[::]".to_string());
    let port: u16 = std::env::var("ZEROCLAW_GATEWAY_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(config.gateway.port);

    if let Err(e) = zeroclaw::daemon::run(config, host, port).await {
        eprintln!("Daemon error: {e:#}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
