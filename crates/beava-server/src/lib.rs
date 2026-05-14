//! beava-server: the server binary's logic crate.
//!
//! The mio data plane (`apply_shard` + `runtime_core_glue` + `ServerV18`) is
//! the sole data-plane runtime per the mio-only invariant. The tokio admin
//! sidecar in `http_admin` binds on a separate port (`cfg.admin_addr`) and
//! must remain the only home for axum symbols.

pub mod apply_shard;
pub mod cli;
pub mod feature_query;
pub mod http_admin;
pub mod idem_cache;
pub mod logging;
pub mod quickstart;
pub mod recovery;
pub mod register;
pub mod registry_debug;
pub mod runtime_core_glue;
pub mod server;
pub mod shutdown;
pub mod snapshot_task;
pub mod wal_config;

#[cfg(any(feature = "testing", test))]
pub mod testing;

pub use beava_core::config::{self, Config, ConfigError};
pub use server::{ServerError, ServerV18};

use crate::http_admin::{RegistrySnapshot, SharedRegistrySnapshot};
use crate::idem_cache::IdemCache;
use crate::registry_debug::DevAggState;
use beava_persistence::WalSink;
use std::sync::{Arc, RwLock};

/// Shared per-process state: registry + state tables + event-id counters
/// (`DevAggState`), WAL sink, and idempotency cache. Cloned by reference
/// across HTTP and TCP handlers.
#[derive(Clone)]
pub struct AppState {
    pub dev_agg: DevAggState,
    pub wal_sink: WalSink,
    pub idem_cache: Arc<IdemCache>,
    /// Gates the data-plane `/registry` shim (404 when false). Held in an
    /// `Arc<AtomicBool>` so `TestServer.dev_endpoints(true)` can flip it
    /// after spawn.
    pub dev_endpoints: Arc<std::sync::atomic::AtomicBool>,
    /// Effective `test_mode`, resolved at boot as `cfg.test_mode ||
    /// BEAVA_TEST_MODE=1`. When false, `OP_RESET` is rejected with
    /// `reset_disabled_in_production` (HTTP 403 / wire 0xFFFF). Boot-time
    /// resolution prevents runtime escalation.
    pub effective_test_mode: bool,
    /// Memory-governance enforcement flag, resolved at boot. `true` by
    /// default; `BEAVA_MEMORY_GOV_ENFORCE=0` or
    /// `.memory_governance_enforce(false)` opts out. Read on the cold
    /// register path (never re-read on the hot path).
    pub memory_governance_enforce: bool,
    /// Shared registry-snapshot Arc read by the tokio admin sidecar
    /// (`/registry` JSON + Prometheus gauges) and written by the mio
    /// register dispatch path on every successful register. Single Arc
    /// constructed at `ServerV18::bind` time and threaded into both the
    /// admin server and `AppState`.
    pub admin_snapshot: SharedRegistrySnapshot,
}

impl AppState {
    pub fn new(dev_agg: DevAggState, wal_sink: WalSink, idem_cache: Arc<IdemCache>) -> Self {
        Self {
            dev_agg,
            wal_sink,
            idem_cache,
            dev_endpoints: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            effective_test_mode: false,
            memory_governance_enforce: true,
            admin_snapshot: Arc::new(RwLock::new(RegistrySnapshot::default())),
        }
    }

    /// `true` iff the data-plane `/registry` shim should serve. Only set via
    /// `TestServer.dev_endpoints(true)`; production observability flows
    /// through the tokio admin sidecar on `cfg.admin_addr`.
    pub fn dev_endpoints_enabled(&self) -> bool {
        self.dev_endpoints
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Semantic version of the server binary.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Human-readable banner used by `main.rs` and logs.
pub fn banner() -> String {
    format!("beava v{} (skeleton)", VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_includes_version() {
        let b = banner();
        assert!(b.contains(VERSION));
        assert!(b.starts_with("beava v"));
    }
}
