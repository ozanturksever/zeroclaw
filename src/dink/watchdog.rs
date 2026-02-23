//! Dink connection liveness tracking and watchdog.
//!
//! [`DinkLiveness`] tracks whether the NATS connection is alive.
//! [`spawn_watchdog`] monitors liveness and exits the process (or returns)
//! when the connection has been dead longer than the configured grace period.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// Tracks whether the Dink/NATS connection is alive.
///
/// Cheap to clone — all clones share the same underlying state.
#[derive(Clone)]
pub struct DinkLiveness {
    alive: Arc<AtomicBool>,
    notify_dead: Arc<Notify>,
}

impl DinkLiveness {
    pub fn new() -> Self {
        Self {
            alive: Arc::new(AtomicBool::new(true)),
            notify_dead: Arc::new(Notify::new()),
        }
    }

    /// Mark the connection as alive (recovered).
    pub fn mark_alive(&self) {
        self.alive.store(true, Ordering::SeqCst);
    }

    /// Mark the connection as dead. Wakes any task waiting in `wait_until_dead`.
    pub fn mark_dead(&self) {
        self.alive.store(false, Ordering::SeqCst);
        self.notify_dead.notify_waiters();
    }

    /// Returns `true` if the connection is currently alive.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Waits until `mark_dead()` is called. Returns immediately if already dead.
    pub async fn wait_until_dead(&self) {
        loop {
            if !self.is_alive() {
                return;
            }
            self.notify_dead.notified().await;
            if !self.is_alive() {
                return;
            }
        }
    }
}

/// Configuration for [`spawn_watchdog`].
pub struct WatchdogConfig {
    /// How long to wait after detecting a dead connection before taking action.
    pub grace_period: Duration,
    /// If true, call `std::process::exit(1)` on timeout. Set to false in tests.
    pub exit_on_timeout: bool,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            grace_period: Duration::from_secs(120),
            exit_on_timeout: true,
        }
    }
}

/// Spawn a watchdog task that monitors liveness.
///
/// Behaviour:
/// 1. Wait until liveness becomes dead.
/// 2. Start grace period countdown.
/// 3. If still dead after grace period: exit process (or return `true` if
///    `exit_on_timeout` is false).
/// 4. If recovered during grace period: go back to step 1.
///
/// Returns a `JoinHandle<bool>` — resolves to `true` if the watchdog triggered
/// (only reachable when `exit_on_timeout` is false).
pub fn spawn_watchdog(liveness: DinkLiveness, config: WatchdogConfig) -> JoinHandle<bool> {
    tokio::spawn(async move {
        loop {
            // Step 1: wait for dead
            liveness.wait_until_dead().await;
            tracing::warn!(
                "Dink watchdog: connection lost, grace period {:?}",
                config.grace_period
            );

            // Step 2: grace period
            tokio::time::sleep(config.grace_period).await;

            // Step 3: check if still dead
            if !liveness.is_alive() {
                tracing::error!(
                    "Dink watchdog: connection dead after grace period — triggering exit"
                );
                if config.exit_on_timeout {
                    std::process::exit(1);
                }
                return true;
            }

            // Step 4: recovered during grace — loop back
            tracing::info!("Dink watchdog: connection recovered during grace period");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_alive() {
        let l = DinkLiveness::new();
        assert!(l.is_alive());
    }

    #[test]
    fn mark_dead_flips_state() {
        let l = DinkLiveness::new();
        l.mark_dead();
        assert!(!l.is_alive());
    }

    #[test]
    fn mark_alive_restores_state() {
        let l = DinkLiveness::new();
        l.mark_dead();
        assert!(!l.is_alive());
        l.mark_alive();
        assert!(l.is_alive());
    }

    #[tokio::test]
    async fn watchdog_does_not_trigger_when_alive() {
        let l = DinkLiveness::new();
        let handle = spawn_watchdog(
            l,
            WatchdogConfig {
                grace_period: Duration::from_millis(50),
                exit_on_timeout: false,
            },
        );

        // Wait longer than grace period — handle should NOT complete
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!handle.is_finished());
        handle.abort();
    }

    #[tokio::test]
    async fn watchdog_triggers_after_grace_period() {
        let l = DinkLiveness::new();
        l.mark_dead();

        let handle = spawn_watchdog(
            l,
            WatchdogConfig {
                grace_period: Duration::from_millis(100),
                exit_on_timeout: false,
            },
        );

        let result = handle.await.expect("watchdog task panicked");
        assert!(result, "watchdog should return true when triggered");
    }

    #[tokio::test]
    async fn watchdog_cancels_if_recovered_during_grace() {
        let l = DinkLiveness::new();
        l.mark_dead();

        let l2 = l.clone();
        let handle = spawn_watchdog(
            l,
            WatchdogConfig {
                grace_period: Duration::from_millis(500),
                exit_on_timeout: false,
            },
        );

        // Recover after 100ms (well within the 500ms grace)
        tokio::time::sleep(Duration::from_millis(100)).await;
        l2.mark_alive();

        // Wait past the original grace period — handle should NOT have triggered
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            !handle.is_finished(),
            "watchdog should not trigger after recovery"
        );
        handle.abort();
    }
}
