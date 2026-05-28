//! `ServerV18` (mio data plane) + tokio admin sidecar.
//!
//! `ServerV18` is the only data-plane runtime per the mio-only invariant;
//! the admin sidecar in `http_admin.rs` binds on `cfg.admin_addr`
//! (default 8090) and is the only legitimate axum surface in beava-server.

use crate::idem_cache::IdemCache;
use crate::recovery::{replay_handrolled_wal_dir, replay_wal_from_lsn};
use crate::snapshot_task::{spawn_snapshot_task, SnapshotTaskConfig, SnapshotTriggerTx};
use crate::AppState;
use beava_core::registry::Registry;
use beava_persistence::{Persistence, SyncMode, WalSink, WalSinkConfig};
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Server boot configuration carried via [`ServerV18::bind_with_config`].
///
/// `Persistence::Memory` boots without a WAL writer thread, snapshot
/// writer, or recovery call — pure in-RAM state for embed mode and tests.
/// `Persistence::Disk { .. }` runs the full WAL + snapshot + recovery
/// path. The struct is named `ServerV18Config` rather than `Config` to
/// avoid clashing with the `beava_core::config::Config` re-export at the
/// crate root.
#[derive(Debug, Clone)]
pub struct ServerV18Config {
    /// Persistence mode. `Persistence::Disk { .. }` is the production
    /// default; `Persistence::Memory` is opt-in for embed mode and tests.
    pub persistence: Persistence,
    /// Gate for `OP_RESET` in production. `false` (default) blocks reset;
    /// `true` permits it.
    pub test_mode: bool,
    /// TCP wire-decoder frame cap, plumbed through `WorkerConfig` rather
    /// than read from env per-frame so parallel `TestServer` instances
    /// don't contaminate each other.
    pub tcp_max_frame_bytes: u32,
    /// WAL ring buffer count override. `None` = `WalConfig::DEFAULT_BUFFERS`
    /// (4); `Some(n)` clamps to `[BUFFERS_MIN, BUFFERS_MAX]`. Production
    /// reads `BEAVA_WAL_BUFFERS` once via `from_env()`; tests pass explicit
    /// values through `TestServerBuilder` to avoid process-global env
    /// contamination.
    pub wal_buffers: Option<usize>,
    /// WAL ring buffer size (MiB). `None` = `WalConfig::DEFAULT_BUFFER_SIZE_MB`
    /// (32); `Some(mb)` clamps to `[BUFFER_SIZE_MB_MIN, BUFFER_SIZE_MB_MAX]`.
    /// Production reads `BEAVA_WAL_BUFFER_SIZE_MB` once via `from_env()`.
    pub wal_buffer_size_mb: Option<usize>,
    /// WAL writer-thread tick interval (ms). `None` =
    /// `WalConfig::DEFAULT_TICK_MS` (20); `Some(ms)` clamps to
    /// `[TICK_MS_MIN, TICK_MS_MAX]`. Production reads `BEAVA_WAL_TICK_MS`
    /// once via `from_env()`.
    pub wal_tick_ms: Option<u64>,
    /// IoPool worker thread count. `None` = `default_io_threads()` heuristic
    /// (`max(2, available_parallelism / 4)`); `Some(n)` clamps to
    /// `n.max(1)`. Production reads `BEAVA_IO_THREADS` once via `from_env()`.
    pub io_threads: Option<usize>,
    /// Memory-governance enforcement override. `None` = default ON;
    /// `Some(false)` = explicit escape hatch (legacy
    /// `BEAVA_MEMORY_GOV_ENFORCE=0` semantic); `Some(true)` = explicit ON.
    /// Production reads `BEAVA_MEMORY_GOV_ENFORCE` once via `from_env()`.
    pub memory_governance_enforce: Option<bool>,
}

impl Default for ServerV18Config {
    fn default() -> Self {
        Self {
            persistence: Persistence::default(),
            test_mode: false,
            tcp_max_frame_bytes: 4 * 1024 * 1024,
            wal_buffers: None,
            wal_buffer_size_mb: None,
            wal_tick_ms: None,
            io_threads: None,
            memory_governance_enforce: None,
        }
    }
}

impl ServerV18Config {
    /// Production env-var resolution: reads `BEAVA_*` once at config-load
    /// time and stamps the values into the struct. This is the only
    /// legitimate env-read site for the per-server tunables; the hot path
    /// reads struct fields, never process env. Tests must NOT call this —
    /// they use `TestServerBuilder` methods to avoid process-global env
    /// contamination, enforced by the `phase13_5_3_no_env_var_pokes_in_tests`
    /// architectural tripwire.
    ///
    /// Recognised env vars:
    /// - `BEAVA_WAL_BUFFERS` (usize, `[2, 32]`, default 4 — clamps with WARN)
    /// - `BEAVA_WAL_BUFFER_SIZE_MB` (usize, `[4, 256]`, default 32)
    /// - `BEAVA_WAL_TICK_MS` (u64, `[1, 1000]`, default 20)
    /// - `BEAVA_IO_THREADS` (usize, `>= 1`, default heuristic)
    /// - `BEAVA_TEST_MODE` (strict `== "1"`)
    /// - `BEAVA_MEMORY_GOV_ENFORCE` (`"0"` → `Some(false)`; unset → `None`
    ///   = default ON; any other value → `Some(true)`)
    pub fn from_env() -> Self {
        let wal_buffers = parse_clamp_usize_with_warn(
            "BEAVA_WAL_BUFFERS",
            crate::wal_config::WalConfig::BUFFERS_MIN,
            crate::wal_config::WalConfig::BUFFERS_MAX,
        );
        let wal_buffer_size_mb = parse_clamp_usize_with_warn(
            "BEAVA_WAL_BUFFER_SIZE_MB",
            crate::wal_config::WalConfig::BUFFER_SIZE_MB_MIN,
            crate::wal_config::WalConfig::BUFFER_SIZE_MB_MAX,
        );
        let wal_tick_ms = parse_clamp_u64_with_warn(
            "BEAVA_WAL_TICK_MS",
            crate::wal_config::WalConfig::TICK_MS_MIN,
            crate::wal_config::WalConfig::TICK_MS_MAX,
        );
        // Parse-failure on `BEAVA_IO_THREADS` falls through to `None` and
        // the default heuristic.
        let io_threads = match std::env::var("BEAVA_IO_THREADS") {
            Ok(s) => match s.parse::<usize>() {
                Ok(n) => Some(n.max(1)),
                Err(_) => None,
            },
            Err(_) => None,
        };
        let test_mode = std::env::var("BEAVA_TEST_MODE")
            .map(|v| v == "1")
            .unwrap_or(false);
        // Split explicit-off / default / explicit-on so an absent env var
        // stays visibly `None` on the config; the `AppState` reader applies
        // the default-ON when it sees `None`.
        let memory_governance_enforce = match std::env::var("BEAVA_MEMORY_GOV_ENFORCE") {
            Ok(v) if v == "0" => Some(false),
            Ok(_) => Some(true),
            Err(_) => None,
        };

        Self {
            persistence: Persistence::default(),
            test_mode,
            tcp_max_frame_bytes: 4 * 1024 * 1024,
            wal_buffers,
            wal_buffer_size_mb,
            wal_tick_ms,
            io_threads,
            memory_governance_enforce,
        }
    }
}

/// Parse a usize env var; on parse failure or unset returns `None`. On
/// parse success clamps to `[lo, hi]` and logs a WARN.
fn parse_clamp_usize_with_warn(name: &str, lo: usize, hi: usize) -> Option<usize> {
    match std::env::var(name) {
        Ok(s) => match s.parse::<usize>() {
            Ok(v) => {
                let clamped = v.clamp(lo, hi);
                if clamped != v {
                    tracing::warn!(
                        target: "beava.wal",
                        kind = "wal.config.clamp",
                        env_var = %name,
                        requested = v,
                        clamped = clamped,
                        range_lo = lo,
                        range_hi = hi,
                        "WAL env var clamped to safe range"
                    );
                }
                Some(clamped)
            }
            Err(e) => {
                tracing::warn!(
                    target: "beava.wal",
                    kind = "wal.config.parse_error",
                    env_var = %name,
                    value = %s,
                    error = %e,
                    "WAL env var parse failed; falling back to default"
                );
                None
            }
        },
        Err(_) => None,
    }
}

fn parse_clamp_u64_with_warn(name: &str, lo: u64, hi: u64) -> Option<u64> {
    match std::env::var(name) {
        Ok(s) => match s.parse::<u64>() {
            Ok(v) => {
                let clamped = v.clamp(lo, hi);
                if clamped != v {
                    tracing::warn!(
                        target: "beava.wal",
                        kind = "wal.config.clamp",
                        env_var = %name,
                        requested = v,
                        clamped = clamped,
                        range_lo = lo,
                        range_hi = hi,
                        "WAL env var clamped to safe range"
                    );
                }
                Some(clamped)
            }
            Err(e) => {
                tracing::warn!(
                    target: "beava.wal",
                    kind = "wal.config.parse_error",
                    env_var = %name,
                    value = %s,
                    error = %e,
                    "WAL env var parse failed; falling back to default"
                );
                None
            }
        },
        Err(_) => None,
    }
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("failed to bind {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to bind TCP wire listener on {host}:{port}: {source}")]
    BindTcp {
        host: String,
        port: u16,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid listen address `{0}`: {1}")]
    InvalidAddr(String, String),
    #[error("server error: {0}")]
    Serve(#[source] std::io::Error),
    #[error("failed to spawn WAL sink: {0}")]
    WalSpawn(String),
    #[error("recovery failed: {0}")]
    Recovery(String),
}

/// A server with the mio event loop on the data plane and tokio/axum on the
/// admin plane. Created by [`ServerV18::bind`].
///
/// HTTP and TCP event-plane listeners run on the `beava-runtime-core`
/// `EventLoop` (mio); the admin plane runs on a separate tokio runtime so
/// `/health`, `/metrics`, and `/registry` stay responsive even when the
/// event loop is saturated. This split is the mio-only invariant: the data
/// plane must never touch tokio.
pub struct ServerV18 {
    http_addr: std::net::SocketAddr,
    tcp_addr: std::net::SocketAddr,
    admin: crate::http_admin::BoundAdminServer,
    // Event-plane listeners bound at construction time and handed to the
    // mio HTTP/TCP accept loops in `serve_with_dirs`.
    http_listener: std::net::TcpListener,
    tcp_listener: std::net::TcpListener,
    /// Shared registry-snapshot Arc constructed once in `bind()`. Cloned
    /// into `BoundAdminServer::bind` (read by `/registry` + Prometheus
    /// gauges) and into `AppState.admin_snapshot` (written by the mio
    /// register dispatch path on every successful register). Single Arc so
    /// both ends see the same `RegistrySnapshot`.
    admin_snapshot: crate::http_admin::SharedRegistrySnapshot,
    /// Pre-built runtime state populated by `bind_with_state` /
    /// `bind_with_config`. When `Some`, `serve_with_dirs` consumes this
    /// instead of rebuilding the AppState / Registry / WalSink / snapshot
    /// trigger; `TestServer` needs the `app_state()` / `registry()` /
    /// `snapshot_trigger_handle()` accessors before `serve()` is called.
    /// Plain `bind()` callers leave this `None`.
    prebuilt_state: Option<ServerV18State>,
}

/// State pre-built by `ServerV18::bind_with_state` /
/// `ServerV18::bind_with_config` so `TestServer` can grab `Arc` clones
/// before `serve_with_dirs` consumes the server. Every field is extracted
/// during the run loop.
struct ServerV18State {
    app_state: Arc<AppState>,
    registry: Arc<Registry>,
    idem_cache: Arc<IdemCache>,
    wal_sink: WalSink,
    legacy_wal_worker: JoinHandle<()>,
    wal_ring: Arc<beava_runtime_core::wal_buffer::WalBufferRing>,
    wal_lsn: Arc<beava_runtime_core::wal_lsn::WalLsn>,
    wal_writer_handle: std::thread::JoinHandle<()>,
    wal_reclaim: Option<beava_runtime_core::wal_writer::WalReclaimHandle>,
    /// TCP frame-size cap plumbed through `WorkerConfig` rather than read
    /// from env per-frame, so parallel `TestServer` instances don't
    /// contaminate each other.
    tcp_max_frame_bytes: u32,
    /// IoPool worker thread count override. `None` keeps the default
    /// heuristic; `Some(n)` overrides without reading `BEAVA_IO_THREADS`.
    io_threads_override: Option<usize>,
    /// Shutdown flag for the WalWriter loop; set on shutdown to trigger
    /// the writer's final seal + drain + fsync.
    wal_writer_shutdown: Arc<std::sync::atomic::AtomicBool>,
    /// `None` when `Persistence::Memory` (snapshot task not spawned);
    /// `Some((cancel, worker))` for `Persistence::Disk`.
    snapshot_task: Option<(CancellationToken, JoinHandle<()>)>,
    /// Always `Some` — memory mode keeps a never-served sender so
    /// `force_snapshot_now` callers get a clean ack channel instead of
    /// panicking; disk mode plumbs through to the real scheduler trigger.
    snapshot_trigger: SnapshotTriggerTx,
    wal_dir: std::path::PathBuf,
    snapshot_dir: std::path::PathBuf,
}

impl ServerV18 {
    /// Bind HTTP, TCP, and admin listeners.
    ///
    /// - `http_addr` — event-plane HTTP listener (hand-rolled mio loop)
    /// - `tcp_addr`  — event-plane TCP framed listener (hand-rolled mio loop)
    /// - `admin_addr` — admin HTTP listener (tokio/axum)
    ///
    /// All three ports may be `0` for OS assignment.  The actual bound
    /// addresses are available via [`http_addr`], [`tcp_addr`], [`admin_addr`].
    pub async fn bind(
        http_addr: std::net::SocketAddr,
        tcp_addr: std::net::SocketAddr,
        admin_addr: std::net::SocketAddr,
    ) -> Result<Self, ServerError> {
        // Opt this process out of Transparent Huge Pages (Linux), warn if
        // the system default is `[always]`. THP makes fork()-snapshot COW
        // granularity 2 MB instead of 4 KB — a 500× amplifier on COW
        // memory overhead during BGSAVE-style work. See crate::thp.
        crate::thp::detect_and_opt_out();

        // Bind event-plane listeners (std::net — they'll be handed to mio later).
        let http_listener =
            std::net::TcpListener::bind(http_addr).map_err(|e| ServerError::Bind {
                addr: http_addr,
                source: e,
            })?;
        http_listener
            .set_nonblocking(true)
            .map_err(|e| ServerError::Bind {
                addr: http_addr,
                source: e,
            })?;
        let http_bound = http_listener.local_addr().map_err(ServerError::Serve)?;
        // `server.http_bound` is parsed by python/tests/conftest.py and
        // read_bench.py to discover the OS-assigned port when
        // `listen_addr=127.0.0.1:0`.
        tracing::info!(
            target: "beava.server",
            kind = "server.http_bound",
            addr = %http_bound,
            "HTTP server bound"
        );

        let tcp_listener =
            std::net::TcpListener::bind(tcp_addr).map_err(|e| ServerError::BindTcp {
                host: tcp_addr.ip().to_string(),
                port: tcp_addr.port(),
                source: e,
            })?;
        tcp_listener
            .set_nonblocking(true)
            .map_err(|e| ServerError::BindTcp {
                host: tcp_addr.ip().to_string(),
                port: tcp_addr.port(),
                source: e,
            })?;
        let tcp_bound = tcp_listener.local_addr().map_err(ServerError::Serve)?;
        tracing::info!(
            target: "beava.server",
            kind = "server.tcp_bound",
            addr = %tcp_bound,
            "TCP wire listener bound"
        );

        // Bind admin (tokio/axum). The snapshot Arc is also threaded into
        // `AppState.admin_snapshot` in `build_runtime_state*` so the mio
        // register dispatch path writes the new
        // `RegistrySnapshot{version, node_count}` into the same Arc the
        // admin handler reads.
        let admin_snapshot = std::sync::Arc::new(std::sync::RwLock::new(
            crate::http_admin::RegistrySnapshot::default(),
        ));
        let admin = crate::http_admin::BoundAdminServer::bind(
            admin_addr,
            std::sync::Arc::clone(&admin_snapshot),
        )
        .await
        .map_err(|e| ServerError::Bind {
            addr: admin_addr,
            source: e,
        })?;

        Ok(Self {
            http_addr: http_bound,
            tcp_addr: tcp_bound,
            admin,
            http_listener,
            tcp_listener,
            admin_snapshot,
            prebuilt_state: None,
        })
    }

    /// Constructor that takes a [`ServerV18Config`] for persistence +
    /// test-mode + per-server tunables. Memory mode skips the WAL writer
    /// thread, snapshot writer, and recovery; Disk mode runs them.
    ///
    /// Builds the full runtime state at bind time so `TestServer` can grab
    /// `Arc` clones via `app_state()` / `registry()` /
    /// `snapshot_trigger_handle()` before `serve_with_dirs` consumes the
    /// server.
    ///
    /// `tcp_addr` is `Option<SocketAddr>` for API forward-compatibility
    /// (HTTP-only embed mode is in scope for v0.1+); when `None` an
    /// ephemeral 127.0.0.1:0 listener still binds so the mio event loop
    /// has a uniform listener set.
    pub async fn bind_with_config(
        http_addr: std::net::SocketAddr,
        tcp_addr: Option<std::net::SocketAddr>,
        admin_addr: std::net::SocketAddr,
        cfg: ServerV18Config,
    ) -> Result<Self, ServerError> {
        // The mio event loop expects two listeners (HTTP + TCP); when the
        // caller passes `None` for tcp_addr we bind an ephemeral
        // 127.0.0.1:0 throwaway.
        let resolved_tcp_addr = tcp_addr.unwrap_or_else(|| "127.0.0.1:0".parse().unwrap());
        let mut sv18 = Self::bind(http_addr, resolved_tcp_addr, admin_addr).await?;
        // `cfg.test_mode` already carries the env-resolved value from
        // `from_env()` (production) or `.test_mode(true)` (tests). Reading
        // env again here would re-introduce cross-test pollution.
        let effective_test_mode = cfg.test_mode;
        // 60 s default snapshot interval mirrors production tuning; memory
        // mode ignores it (snapshot task not spawned).
        let state = build_runtime_state_with_persistence(
            cfg.persistence,
            60_000,
            2,
            effective_test_mode,
            cfg.tcp_max_frame_bytes,
            crate::wal_config::WalConfigOverrides {
                buffers: cfg.wal_buffers,
                buffer_size_mb: cfg.wal_buffer_size_mb,
                tick_ms: cfg.wal_tick_ms,
            },
            cfg.io_threads,
            cfg.memory_governance_enforce,
            std::sync::Arc::clone(&sv18.admin_snapshot),
        )
        .await?;
        sv18.prebuilt_state = Some(state);
        Ok(sv18)
    }

    /// Variant of `bind` that also eagerly builds `AppState`, `Registry`,
    /// `WalSink`, the WAL ring + writer, and the snapshot task so
    /// `TestServer` can return `Arc` clones via `app_state()`,
    /// `registry()`, and `snapshot_trigger_handle()` before
    /// `serve_with_dirs` is called. Production callers use `bind()` +
    /// `serve()`.
    // reason: TestServer construction surface — every parameter is an
    // independently-meaningful production knob (HTTP/TCP/admin addresses,
    // WAL/snapshot dirs, intervals, frame cap). A struct-bag would obscure
    // call-site intent across the test harness.
    #[allow(clippy::too_many_arguments)]
    pub async fn bind_with_state(
        http_addr: std::net::SocketAddr,
        tcp_addr: std::net::SocketAddr,
        admin_addr: std::net::SocketAddr,
        wal_dir: std::path::PathBuf,
        snapshot_dir: std::path::PathBuf,
        snapshot_interval_ms: u64,
        wal_fsync_interval_ms: u64,
        tcp_max_frame_bytes: u32,
    ) -> Result<Self, ServerError> {
        let mut sv18 = Self::bind(http_addr, tcp_addr, admin_addr).await?;
        let admin_snapshot = std::sync::Arc::clone(&sv18.admin_snapshot);
        let state = build_runtime_state(
            wal_dir,
            snapshot_dir,
            snapshot_interval_ms,
            wal_fsync_interval_ms,
            tcp_max_frame_bytes,
            admin_snapshot,
        )
        .await?;
        sv18.prebuilt_state = Some(state);
        Ok(sv18)
    }

    /// `bind_with_state` plus `ServerV18Config` overrides for the
    /// per-server tunables (`test_mode`, `wal_buffers`, `wal_buffer_size_mb`,
    /// `wal_tick_ms`, `io_threads`, `memory_governance_enforce`). Used by
    /// `TestServerBuilder::spawn()` so override values plumb through
    /// without process-env reads. The `snapshot_interval_ms` and
    /// `wal_fsync_interval_ms` knobs stay separate because tests need
    /// finer control than the production env interface exposes (most
    /// `TestServer`s pass `1` to keep macOS `F_FULLSYNC` latency from
    /// dominating wall-clock).
    // reason: see `bind_with_state` above — TestServer construction surface;
    // independently-meaningful parameters with finer-grained control than
    // the production env interface exposes.
    #[allow(clippy::too_many_arguments)]
    pub async fn bind_with_state_and_overrides(
        http_addr: std::net::SocketAddr,
        tcp_addr: std::net::SocketAddr,
        admin_addr: std::net::SocketAddr,
        wal_dir: std::path::PathBuf,
        snapshot_dir: std::path::PathBuf,
        snapshot_interval_ms: u64,
        wal_fsync_interval_ms: u64,
        cfg: ServerV18Config,
    ) -> Result<Self, ServerError> {
        let mut sv18 = Self::bind(http_addr, tcp_addr, admin_addr).await?;
        // `cfg.persistence` is intentionally ignored — `wal_dir` /
        // `snapshot_dir` take precedence because `TestServer` always wants
        // disk persistence with explicit dirs. Collapsing
        // `bind_with_state*` into `bind_with_config` would require
        // exposing the snapshot/fsync intervals on `ServerV18Config`.
        let state = build_runtime_state_with_persistence(
            Persistence::Disk {
                wal_dir,
                snapshot_dir,
                sync_mode: SyncMode::Periodic,
            },
            snapshot_interval_ms,
            wal_fsync_interval_ms,
            cfg.test_mode,
            cfg.tcp_max_frame_bytes,
            crate::wal_config::WalConfigOverrides {
                buffers: cfg.wal_buffers,
                buffer_size_mb: cfg.wal_buffer_size_mb,
                tick_ms: cfg.wal_tick_ms,
            },
            cfg.io_threads,
            cfg.memory_governance_enforce,
            std::sync::Arc::clone(&sv18.admin_snapshot),
        )
        .await?;
        sv18.prebuilt_state = Some(state);
        Ok(sv18)
    }

    /// Clone of the shared `Arc<AppState>` populated by
    /// `bind_with_state` / `bind_with_config`. Panics on plain-`bind()`
    /// servers.
    pub fn app_state(&self) -> Arc<AppState> {
        Arc::clone(
            &self
                .prebuilt_state
                .as_ref()
                .expect(
                    "ServerV18::app_state requires bind_with_state() — \
                     plain bind() leaves prebuilt_state None",
                )
                .app_state,
        )
    }

    /// Clone of the shared `Arc<Registry>` populated by `bind_with_state`
    /// / `bind_with_config`. Panics on plain-`bind()` servers.
    pub fn registry(&self) -> Arc<Registry> {
        Arc::clone(
            &self
                .prebuilt_state
                .as_ref()
                .expect(
                    "ServerV18::registry requires bind_with_state() — \
                     plain bind() leaves prebuilt_state None",
                )
                .registry,
        )
    }

    /// Clone of the snapshot-trigger channel populated by
    /// `bind_with_state` / `bind_with_config`. Panics on plain-`bind()`
    /// servers. `TestServer::force_snapshot_now` keeps the cloned sender
    /// alive after `serve_with_dirs` consumes the server.
    pub fn snapshot_trigger_handle(&self) -> SnapshotTriggerTx {
        self.prebuilt_state
            .as_ref()
            .expect(
                "ServerV18::snapshot_trigger_handle requires bind_with_state() — \
                 plain bind() leaves prebuilt_state None",
            )
            .snapshot_trigger
            .clone()
    }

    /// The bound HTTP event-plane address.
    pub fn http_addr(&self) -> std::net::SocketAddr {
        self.http_addr
    }

    /// The bound TCP event-plane address.
    pub fn tcp_addr(&self) -> std::net::SocketAddr {
        self.tcp_addr
    }

    /// The bound admin HTTP address.
    pub fn admin_addr(&self) -> std::net::SocketAddr {
        self.admin.local_addr
    }

    /// Run the server until `shutdown` completes. Boots a temporary WAL
    /// in `std::env::temp_dir()` for the duration of the call; callers
    /// that need a durable WAL path use [`Self::serve_with_dirs`].
    pub async fn serve<F>(self, shutdown: F) -> Result<(), ServerError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        // Use temp dirs for WAL + snapshot (bench / test use case).
        let wal_dir = std::env::temp_dir().join(format!(
            "beava-v18-wal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        let snapshot_dir = std::env::temp_dir().join(format!(
            "beava-v18-snap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        self.serve_with_dirs(shutdown, wal_dir, snapshot_dir).await
    }

    /// Run with explicit WAL + snapshot directories. Called by `serve()`
    /// with temp dirs and by the bench harness with configured paths.
    ///
    /// Threading model:
    ///   - 1 apply thread (mio Poll + `EventLoop::tick` +
    ///     `ApplyShard::dispatch`)
    ///   - N IoPool workers (parallel read-parse + write-serialize)
    ///   - 1 WalWriter thread (drains sealed WAL buffers → write + fsync)
    ///   - 1 tokio runtime for admin endpoints only
    ///
    /// If `bind_with_state` / `bind_with_config` populated `prebuilt_state`,
    /// it's consumed here instead of rebuilding from scratch.
    pub async fn serve_with_dirs<F>(
        self,
        shutdown: F,
        wal_dir: std::path::PathBuf,
        snapshot_dir: std::path::PathBuf,
    ) -> Result<(), ServerError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let state = match self.prebuilt_state {
            Some(state) => state,
            None => {
                build_runtime_state(
                    wal_dir,
                    snapshot_dir,
                    60_000,
                    2,
                    4 * 1024 * 1024,
                    std::sync::Arc::clone(&self.admin_snapshot),
                )
                .await?
            }
        };

        run_serve_loop(
            self.http_listener,
            self.tcp_listener,
            self.admin,
            state,
            shutdown,
        )
        .await
    }

    /// Gracefully shut down the admin server without running serve().
    /// Use this only when serve() was never called (e.g. bind-only tests).
    pub async fn shutdown(self) {
        self.admin.shutdown().await;
    }
}

/// Build the shared `AppState` / `Registry` / `WalSink` / snapshot stack.
/// Extracted so `TestServer` can grab `Arc` clones from `bind_with_state`
/// before `serve_with_dirs` consumes the server.
async fn build_runtime_state(
    wal_dir: std::path::PathBuf,
    snapshot_dir: std::path::PathBuf,
    snapshot_interval_ms: u64,
    wal_fsync_interval_ms: u64,
    tcp_max_frame_bytes: u32,
    admin_snapshot: crate::http_admin::SharedRegistrySnapshot,
) -> Result<ServerV18State, ServerError> {
    // Production callers go through `bind_with_config` / `from_env()`;
    // this path is only used by tests that don't construct a config and
    // is safe to default to `test_mode = false`, default WAL overrides,
    // default IoPool, and memory-governance ON.
    build_runtime_state_with_persistence(
        Persistence::Disk {
            wal_dir,
            snapshot_dir,
            sync_mode: SyncMode::Periodic,
        },
        snapshot_interval_ms,
        wal_fsync_interval_ms,
        false,
        tcp_max_frame_bytes,
        crate::wal_config::WalConfigOverrides::default(),
        None,
        None,
        admin_snapshot,
    )
    .await
}

/// Build runtime state branched on the [`Persistence`] variant.
///
/// `Persistence::Disk { .. }` runs the full path: snapshot + WAL recovery,
/// real `WalSink::spawn`, real `WalWriter`, real snapshot task.
///
/// `Persistence::Memory` skips filesystem interaction entirely: no
/// recovery, `WalSink::spawn_no_op()`, a no-op writer that drains sealed
/// buffers back to the free pool without fsync, and the snapshot task
/// stays unspawned (the trigger channel is created so
/// `TestServer::force_snapshot_now` has somewhere to send).
///
/// `wal_fsync_interval_ms` is honoured only in Disk mode; Memory mode
/// drives the no-op writer at `wal_cfg.tick_ms`.
// reason: persistence-aware constructor for the runtime state; each
// parameter is an independently-meaningful subsystem knob (persistence
// mode, intervals, test_mode, frame cap, WAL overrides, IO threads, memory
// governance). Group structs would split into N partial-struct refactors.
#[allow(clippy::too_many_arguments)]
async fn build_runtime_state_with_persistence(
    persistence: Persistence,
    snapshot_interval_ms: u64,
    wal_fsync_interval_ms: u64,
    effective_test_mode: bool,
    tcp_max_frame_bytes: u32,
    wal_overrides: crate::wal_config::WalConfigOverrides,
    io_threads_override: Option<usize>,
    memory_governance_enforce: Option<bool>,
    admin_snapshot: crate::http_admin::SharedRegistrySnapshot,
) -> Result<ServerV18State, ServerError> {
    use beava_runtime_core::wal_buffer::WalBufferRing;
    use beava_runtime_core::wal_lsn::WalLsn;
    use beava_runtime_core::wal_writer::WalWriter;

    let idem_cache = Arc::new(IdemCache::new());
    let registry = Arc::new(beava_core::registry::Registry::new());
    let dev_agg = crate::registry_debug::DevAggState::new(registry.clone());

    // Memory mode uses placeholder paths that are never read or written.
    let (wal_dir, snapshot_dir, sync_mode, is_memory) = match &persistence {
        Persistence::Memory => (
            std::path::PathBuf::from("<memory-mode-no-wal>"),
            std::path::PathBuf::from("<memory-mode-no-snapshots>"),
            SyncMode::Periodic,
            true,
        ),
        Persistence::Disk {
            wal_dir,
            snapshot_dir,
            sync_mode,
        } => (wal_dir.clone(), snapshot_dir.clone(), *sync_mode, false),
    };

    // Recovery sequence:
    //   1. Install the latest `*.bvs` snapshot (registry descriptors +
    //      state tables + counters); skipping this would lose every
    //      event that landed between the snapshot and the previous
    //      shutdown. Returns `snapshot_lsn` so the WAL replays can gate
    //      on it.
    //   2. Replay `*.log` records with `lsn > snapshot_lsn` (RegistryBumps
    //      and any persistence-WAL events from the legacy `WalSink`
    //      path).
    //   3. Replay `*.wal` data-plane events from `apply_shard`.
    // Memory mode skips this whole block — state starts fresh.
    let initial_start_lsn = if is_memory {
        tracing::info!(
            target: "beava.recovery",
            kind = "recovery.skipped_memory_mode",
            "Persistence::Memory: recovery skipped"
        );
        1
    } else {
        let snapshot_load = crate::recovery::load_snapshot_if_any(&snapshot_dir, &dev_agg)
            .ok()
            .unwrap_or_default();
        let snapshot_lsn = snapshot_load.snapshot_lsn;

        let persistence_lsn = if wal_dir.exists() {
            match replay_wal_from_lsn(&wal_dir, snapshot_lsn, &dev_agg) {
                Ok(outcome) => outcome.last_lsn.max(snapshot_lsn),
                Err(_) => snapshot_lsn,
            }
        } else {
            snapshot_lsn
        };

        // Replay hand-rolled `*.wal` data-plane events past the state already
        // covered by the snapshot body. The snapshot header LSN is a mixed
        // legacy/data-plane checkpoint; older snapshots can have a header LSN
        // lower than their serialized data-plane state, and registry `.log`
        // records can have higher LSNs than post-snapshot data-plane records.
        // The body watermark is the only safe floor for hand-rolled replay.
        let handrolled_from_lsn = snapshot_load.applied_lsn;
        let handrolled_outcome =
            replay_handrolled_wal_dir(&wal_dir, handrolled_from_lsn, &dev_agg).unwrap_or_default();
        let initial = handrolled_outcome
            .last_lsn
            .max(persistence_lsn)
            .max(snapshot_lsn)
            .max(snapshot_load.applied_lsn)
            + 1;

        tracing::debug!(
            target: "beava.recovery",
            kind = "recovery.serve_with_dirs",
            persistence_lsn,
            handrolled_events = handrolled_outcome.replay_event_count,
            initial_start_lsn = initial,
            "serve_with_dirs recovery complete"
        );

        initial
    };

    // `WalSink` is still used for the `/register` cold path; data-plane
    // push goes through `WalBufferRing` directly. `initial_start_lsn`
    // keeps the new `*.log` segment from colliding with the previous
    // server instance's tail. Memory mode uses `spawn_no_op` so the
    // sink never touches disk.
    let (wal_sink, legacy_wal_worker) = if is_memory {
        let (sink, worker) = WalSink::spawn_no_op();
        tracing::info!(
            target: "beava.wal",
            kind = "wal.no_op_sink_spawned",
            "Persistence::Memory: WalSink::spawn_no_op"
        );
        (sink, worker)
    } else {
        WalSink::spawn(WalSinkConfig {
            dir: wal_dir.clone(),
            initial_start_lsn,
            initial_registry_version: dev_agg.registry.version() as u32,
            fsync_interval_ms: wal_fsync_interval_ms,
            fsync_bytes: 0,
            segment_bytes: 64 * 1024 * 1024,
            sync_mode,
        })
        .map_err(|e| ServerError::WalSpawn(e.to_string()))?
    };

    let mut app_state_inner = AppState::new(dev_agg, wal_sink.clone(), idem_cache.clone());
    // `effective_test_mode` is stamped at boot only — read-only after bind
    // so reset cannot be re-enabled at runtime.
    app_state_inner.effective_test_mode = effective_test_mode;
    // Default-ON when no explicit override; `Some(false)` is the escape
    // hatch (`BEAVA_MEMORY_GOV_ENFORCE=0` /
    // `.memory_governance_enforce(false)`).
    app_state_inner.memory_governance_enforce = memory_governance_enforce.unwrap_or(true);
    // Replace the default snapshot Arc that `AppState::new` created with
    // the shared one constructed in `ServerV18::bind`, so the mio register
    // dispatch path writes into the same Arc the tokio admin sidecar
    // reads (otherwise `/registry` + Prometheus gauges stay at 0/0).
    app_state_inner.admin_snapshot = admin_snapshot;
    let app_state = Arc::new(app_state_inner);
    if effective_test_mode {
        tracing::warn!(
            target: "beava.server",
            kind = "server.test_mode_enabled",
            "test_mode ENABLED: OP_RESET will accept reset requests. \
             Disable for production."
        );
    }

    let wal_lsn = Arc::new(WalLsn::new_at(initial_start_lsn.saturating_sub(1)));
    // Resolve WAL config from explicit overrides — hot path never reads
    // env. Production resolves from `ServerV18Config::from_env()` at
    // boot; tests pass explicit values via `TestServerBuilder`. The
    // underlying WAL invariants (lock-free apply, single writer + fsync,
    // `O_APPEND`, four-watermark discipline) are untouched; only buffer
    // count, size, and tick interval are tunable.
    let wal_cfg = crate::wal_config::WalConfig::resolve(wal_overrides);
    tracing::debug!(
        target: "beava.wal",
        kind = "wal.config.resolved",
        buffers = wal_cfg.buffers,
        buffer_size_mb = wal_cfg.buffer_size_mb,
        tick_ms = wal_cfg.tick_ms,
        "WAL config resolved"
    );
    let buf_bytes = wal_cfg.buffer_size_mb * 1024 * 1024;
    let wal_ring = Arc::new(WalBufferRing::new(
        wal_cfg.buffers,
        buf_bytes,
        Arc::clone(&wal_lsn),
    ));

    // Disk mode: real `WalWriter` drains sealed buffers via write +
    // fsync. Memory mode: drain sealed buffers back to the free pool
    // with no file I/O so the apply hot path can't backpressure-block
    // once buffers fill.
    let (wal_writer_shutdown, wal_writer_handle, wal_reclaim) = if is_memory {
        let (shutdown, handle) =
            spawn_no_op_wal_writer(Arc::clone(&wal_ring), Arc::clone(&wal_lsn), wal_cfg.tick_ms);
        (shutdown, handle, None)
    } else {
        let wal_writer = WalWriter::new(
            &wal_dir,
            Arc::clone(&wal_ring),
            Arc::clone(&wal_lsn),
            wal_cfg.tick_ms,
        )
        .map_err(|e| ServerError::WalSpawn(e.to_string()))?;
        let reclaim = wal_writer.reclaim_handle();
        // Capture the shutdown flag BEFORE `spawn()` consumes the writer.
        // Without it, the writer loop would never see the shutdown signal
        // and `JoinHandle` drop would detach the thread mid-tick, losing
        // any active-buffer contents that hadn't been sealed yet.
        let shutdown = wal_writer.shutdown_flag();
        let handle = wal_writer.spawn();
        (shutdown, handle, Some(reclaim))
    };

    // Memory mode skips the snapshot task entirely (zero file I/O), but
    // still creates a `(sender, receiver)` pair so external trigger
    // clones (e.g. `TestServer::force_snapshot_now`) hit a structured
    // closed-channel error instead of panicking.
    let (snapshot_task, snapshot_trigger) = if is_memory {
        let (trigger_tx, _trigger_rx) =
            tokio::sync::mpsc::channel::<tokio::sync::oneshot::Sender<Result<(), String>>>(8);
        tracing::info!(
            target: "beava.snapshot",
            kind = "snapshot.task_skipped_memory_mode",
            "Persistence::Memory: snapshot task not spawned"
        );
        (None, trigger_tx)
    } else {
        let snapshot_cancel = CancellationToken::new();
        let (snapshot_worker, snapshot_trigger) = spawn_snapshot_task(
            SnapshotTaskConfig {
                interval: Duration::from_millis(snapshot_interval_ms.max(1)),
                snapshot_dir: snapshot_dir.clone(),
                retain: 2,
                // Read env once at boot — these are the SOLE legitimate
                // env-read sites. Tests pass config fields directly per
                // `phase13_5_3_no_env_var_pokes_in_tests`.
                min_events_per_snapshot: crate::snapshot_task::min_events_from_env(),
                use_fork_snapshot: crate::snapshot_fork::fork_enabled(),
                snapshot_lsn_capture_tx: None,
            },
            Arc::clone(&app_state),
            wal_sink.clone(),
            wal_reclaim.clone(),
            snapshot_cancel.clone(),
        );
        (Some((snapshot_cancel, snapshot_worker)), snapshot_trigger)
    };

    Ok(ServerV18State {
        app_state,
        registry,
        idem_cache,
        wal_sink,
        legacy_wal_worker,
        wal_ring,
        wal_lsn,
        wal_writer_handle,
        wal_reclaim,
        wal_writer_shutdown,
        snapshot_task,
        snapshot_trigger,
        wal_dir,
        snapshot_dir,
        tcp_max_frame_bytes,
        io_threads_override,
    })
}

/// No-op WAL writer thread for memory mode. Mirrors the loop shape of
/// `WalWriter::run_writer_loop` (sleep → seal_active → drain → check
/// shutdown) but replaces every `write()` / `fsync()` with
/// `return_to_free`, so buffers recycle without disk I/O. The four-watermark
/// LSN state is still advanced (`mark_written` + `mark_synced`) so any
/// durable-LSN waiters unblock immediately.
///
/// Returns `(shutdown_flag, join_handle)` matching `WalWriter::shutdown_flag`
/// + `WalWriter::spawn`.
fn spawn_no_op_wal_writer(
    ring: Arc<beava_runtime_core::wal_buffer::WalBufferRing>,
    lsn: Arc<beava_runtime_core::wal_lsn::WalLsn>,
    tick_ms: u64,
) -> (
    Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    use std::sync::atomic::{AtomicBool, Ordering as AOrdering};

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_thread = Arc::clone(&shutdown);
    let tick = Duration::from_millis(tick_ms.max(1));

    let handle = std::thread::Builder::new()
        .name("beava-wal-writer-noop".to_owned())
        .spawn(move || loop {
            std::thread::sleep(tick);

            // Force-seal the active buffer so the apply thread doesn't
            // accumulate beyond `tick_ms` worth of data.
            ring.seal_active();

            // Drain sealed buffers without disk I/O; advance the written
            // and synced watermarks so any PerEvent-style waiter unblocks
            // even though nothing is durable.
            while let Some(buf) = ring.pop_sealed() {
                let hi = buf.lsn_hi();
                lsn.mark_written(hi);
                lsn.mark_synced(hi);
                ring.return_to_free(buf);
            }

            if shutdown_thread.load(AOrdering::Acquire) {
                ring.seal_active();
                while let Some(buf) = ring.pop_sealed() {
                    let hi = buf.lsn_hi();
                    lsn.mark_written(hi);
                    lsn.mark_synced(hi);
                    ring.return_to_free(buf);
                }
                break;
            }
        })
        .expect("failed to spawn no-op WAL writer thread");

    (shutdown, handle)
}

/// Run the apply thread + admin server until `shutdown` resolves, then
/// drain everything cleanly. Shared by `serve_with_dirs` and the
/// `bind_with_state` / `bind_with_config` paths.
async fn run_serve_loop<F>(
    http_listener: std::net::TcpListener,
    tcp_listener: std::net::TcpListener,
    admin: crate::http_admin::BoundAdminServer,
    state: ServerV18State,
    shutdown: F,
) -> Result<(), ServerError>
where
    F: Future<Output = ()> + Send + 'static,
{
    use crate::apply_shard::ApplyShard;
    use std::sync::atomic::{AtomicBool, Ordering as AOrdering};

    let ServerV18State {
        app_state,
        registry: _registry,
        idem_cache: _idem_cache,
        wal_sink,
        legacy_wal_worker,
        wal_ring,
        wal_lsn,
        wal_writer_handle,
        wal_reclaim: _wal_reclaim,
        wal_writer_shutdown,
        snapshot_task,
        snapshot_trigger,
        wal_dir: _wal_dir,
        snapshot_dir: _snapshot_dir,
        tcp_max_frame_bytes,
        io_threads_override,
    } = state;

    // Drop our snapshot-trigger copy. External clones (`TestServer`) keep
    // the sender count > 0 until they're dropped; without external clones
    // the channel becomes unreachable, which is fine because the snapshot
    // task owns the receiver and keeps servicing scheduled ticks.
    drop(snapshot_trigger);

    let apply_shard = ApplyShard::new(
        Arc::clone(&app_state),
        Arc::clone(&wal_ring),
        Arc::clone(&wal_lsn),
    );

    // ── Shutdown flag (shared between tokio + apply thread) ───────────────
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let shutdown_flag_apply = Arc::clone(&shutdown_flag);
    let shutdown_flag_signal = Arc::clone(&shutdown_flag);

    // ── Spawn apply thread (mio EventLoop) ───────────────────────────────
    let apply_join = std::thread::Builder::new()
        .name("beava-apply".to_owned())
        .spawn(move || {
            run_mio_event_loop(
                http_listener,
                tcp_listener,
                apply_shard,
                shutdown_flag_apply,
                tcp_max_frame_bytes,
                io_threads_override,
            );
        })
        .map_err(ServerError::Serve)?;

    // Wait for the external shutdown future, then signal the apply thread.
    shutdown.await;
    shutdown_flag_signal.store(true, AOrdering::Release);

    // Wait for the apply thread to drain.
    let _ = apply_join.join();

    // Signal the WalWriter to seal + drain + fsync the active buffer
    // (which may hold ~tick_ms worth of post-snapshot pushes that would
    // otherwise be lost when the JoinHandle drops) and join the thread so
    // we wait for the final fsync to land BEFORE the WAL ring + lsn Arcs
    // drop.
    wal_writer_shutdown.store(true, AOrdering::Release);
    let _ = wal_writer_handle.join();

    // Stop snapshot task (Disk mode only — memory mode never spawned one).
    if let Some((snapshot_cancel, snapshot_worker)) = snapshot_task {
        snapshot_cancel.cancel();
        let _ = snapshot_worker.await;
    }

    // Drain legacy WalSink (used only for /register cold path).
    let _ = app_state.wal_sink.clone().shutdown().await;
    let _ = legacy_wal_worker.await;
    // Make sure wal_sink (the explicit clone we held) is dropped before
    // returning so the channel sender count drops to zero.
    drop(wal_sink);

    // Stop admin server.
    admin.shutdown().await;

    Ok(())
}

/// Token assignments for the mio event loop.
const TOKEN_HTTP_LISTENER: mio::Token = mio::Token(0);
const TOKEN_TCP_LISTENER: mio::Token = mio::Token(1);
/// Token used by `mio::Waker` registered with the apply thread's listener
/// `EventLoop`. Workers fire this waker after pushing `RingItem`s to
/// `read_rx` so apply doesn't sleep in `tick(timeout)` while there's
/// already work waiting in the channel.
const TOKEN_APPLY_WAKER: mio::Token = mio::Token(usize::MAX);
/// Client connections start at token 2.
///
/// Currently unused — connections are owned by per-worker IoBackends in
/// `beava-runtime-core`. Retained for the legacy IoPool helpers below
/// (still compiled but never invoked at runtime).
// reason: legacy IoPool scaffolding retained as build-time reference for
// the per-tick lifecycle; the runtime path uses per-worker IoBackends.
// Removing the constants/types/fns risks losing the architecture-doc
// mapping; the dead_code allows are localized to this scaffolding block.
#[allow(dead_code)]
const TOKEN_CLIENT_BASE: usize = 2;

/// Maximum concurrent clients supported by the legacy per-tick IoPool
/// lifecycle. See `TOKEN_CLIENT_BASE` — unused at runtime.
// reason: legacy IoPool scaffolding — see TOKEN_CLIENT_BASE above.
#[allow(dead_code)]
const MAX_CONCURRENT_CLIENTS: usize = 8192;

/// Per-client connection state for the mio event loop.
// reason: legacy IoPool scaffolding — see TOKEN_CLIENT_BASE above.
#[allow(dead_code)]
struct MioClient {
    stream: mio::net::TcpStream,
    token: mio::Token,
    /// Protocol: HTTP or TCP framed wire.
    proto: MioProto,
    /// Inbound read buffer.
    read_buf: bytes::BytesMut,
    /// Queue of `GlueResponse`s produced by the apply phase. Populated
    /// directly by the apply thread's drain loop as it consumes `RingItem`s
    /// from the crossbeam channel; emptied by the write-phase IoPool worker
    /// when it serialises into `write_buf`.
    output_queue: std::collections::VecDeque<crate::runtime_core_glue::GlueResponse>,
    /// Serialized response bytes waiting to be written to the socket.
    /// Populated by the write-phase IoPool worker (off-apply).
    write_buf: bytes::BytesMut,
    /// Currently-registered mio interest. Tracked so we only reregister
    /// when the desired interest changes (avoids per-tick reregister syscall).
    /// `true` = WRITABLE bit currently set; `false` = READABLE-only.
    interest_writable: bool,
    /// True when the client has been closed / should be removed.
    closed: bool,
}

// reason: legacy IoPool scaffolding — see TOKEN_CLIENT_BASE above.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq)]
enum MioProto {
    Http,
    Tcp,
}

/// Observability hooks asserting the off-apply invariant: parse and encode
/// MUST run on IoPool worker threads, never on the apply thread. In
/// production each call is a single `AtomicUsize` bump; tests reset and
/// assert that `apply_*_count()` stays at 0 while `off_apply_*_count()`
/// grows.
pub mod iopool_observer {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Apply-thread id, set by `run_mio_event_loop` at startup. Workers
    /// compare `std::thread::current().id() == APPLY_TID` to decide
    /// which counter pair to bump. `Mutex<Option<ThreadId>>` because
    /// `ThreadId` isn't representable as a plain `AtomicUsize`.
    pub(crate) static APPLY_TID: Mutex<Option<std::thread::ThreadId>> = Mutex::new(None);

    static APPLY_PARSE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static APPLY_ENCODE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static OFF_APPLY_PARSE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static OFF_APPLY_ENCODE_COUNT: AtomicUsize = AtomicUsize::new(0);

    /// Reset all counters to 0 and clear the apply thread id.
    /// Called by tests before each scenario.
    pub fn reset() {
        APPLY_PARSE_COUNT.store(0, Ordering::Release);
        APPLY_ENCODE_COUNT.store(0, Ordering::Release);
        OFF_APPLY_PARSE_COUNT.store(0, Ordering::Release);
        OFF_APPLY_ENCODE_COUNT.store(0, Ordering::Release);
        *APPLY_TID.lock().unwrap() = None;
    }

    /// Record the apply thread's id. Called once by `run_mio_event_loop`
    /// at startup. Subsequent observers compare against this id.
    pub(crate) fn set_apply_tid() {
        *APPLY_TID.lock().unwrap() = Some(std::thread::current().id());
    }

    /// Bump the appropriate parse counter based on whether we're on the
    /// apply thread. Currently unreached at runtime — parse lives inside
    /// per-worker `IoBackend` threads in `beava-runtime-core`, which
    /// can't reach back into this module — but kept for potential
    /// re-instrumentation.
    // reason: optional re-instrumentation point per the doc-comment above;
    // the lint observer module keeps the symbol available without forcing
    // callers in beava-runtime-core to reach back.
    #[allow(dead_code)]
    pub(crate) fn record_parse() {
        if is_apply_thread() {
            APPLY_PARSE_COUNT.fetch_add(1, Ordering::AcqRel);
        } else {
            OFF_APPLY_PARSE_COUNT.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Bump the appropriate encode counter based on whether we're on the
    /// apply thread.
    pub(crate) fn record_encode() {
        if is_apply_thread() {
            APPLY_ENCODE_COUNT.fetch_add(1, Ordering::AcqRel);
        } else {
            OFF_APPLY_ENCODE_COUNT.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn is_apply_thread() -> bool {
        let me = std::thread::current().id();
        match *APPLY_TID.lock().unwrap() {
            Some(tid) => tid == me,
            None => false,
        }
    }

    /// Number of parse calls made by the apply thread. MUST be 0 in
    /// healthy IoPool wiring.
    pub fn apply_parse_count() -> usize {
        APPLY_PARSE_COUNT.load(Ordering::Acquire)
    }

    /// Number of encode calls made by the apply thread. MUST be 0 in
    /// healthy IoPool wiring.
    pub fn apply_encode_count() -> usize {
        APPLY_ENCODE_COUNT.load(Ordering::Acquire)
    }

    /// Number of parse calls made by IoPool worker threads.
    pub fn off_apply_parse_count() -> usize {
        OFF_APPLY_PARSE_COUNT.load(Ordering::Acquire)
    }

    /// Number of encode calls made by IoPool worker threads.
    pub fn off_apply_encode_count() -> usize {
        OFF_APPLY_ENCODE_COUNT.load(Ordering::Acquire)
    }
}

// Apply-thread busy-poll instrumentation (test hooks).

/// Cumulative count of `read_rx.recv_timeout(50µs)` calls made by the
/// apply thread when the spin budget elapses without work. Test hook only.
static APPLY_RECV_TIMEOUT_CALLS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Max drain count observed in a single apply-loop iteration (fetch_max'd
/// on every drain; never reset). Test hook for the drain-until-empty
/// invariant.
static APPLY_MAX_DRAIN_PER_ITER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Apply-thread `pthread_t`, set by `run_mio_event_loop` at startup.
/// Stored as `usize` because `pthread_t` is opaque + Send-unfriendly
/// across `AtomicUsize`. Test hook for per-thread CPU accounting
/// (`mach_thread_basic_info` on macOS; `pthread_getcpuclockid` on Linux).
static APPLY_PTHREAD_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Cumulative count of `flush_response_batch` calls that flushed a
/// non-empty batch. Test hook only.
static APPLY_RESPONSE_BATCH_FLUSHES: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Cumulative count of apply-thread idle fall-throughs into
/// `read_rx.recv_timeout(50µs)` since process start.
#[doc(hidden)]
pub fn apply_recv_timeout_calls() -> u64 {
    APPLY_RECV_TIMEOUT_CALLS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Max `RingItem`s drained in a single apply-loop iteration since process
/// start. Used by tests to verify the drain-until-empty invariant.
#[doc(hidden)]
pub fn apply_max_drain_per_iter() -> u64 {
    APPLY_MAX_DRAIN_PER_ITER.load(std::sync::atomic::Ordering::Relaxed)
}

/// Cumulative count of response-batch flushes (a flush fires when the
/// batch reaches `BATCH_SIZE_FLUSH=16` OR `BATCH_TIME_FLUSH=100µs`
/// elapses).
#[doc(hidden)]
pub fn response_batch_flushes() -> u64 {
    APPLY_RESPONSE_BATCH_FLUSHES.load(std::sync::atomic::Ordering::Relaxed)
}

/// Returns the apply thread's `pthread_t` once it has booted, or `None`
/// if the apply thread has not yet registered. Used by tests for
/// per-thread CPU accounting.
#[doc(hidden)]
pub fn apply_pthread_id() -> Option<libc::pthread_t> {
    let raw = APPLY_PTHREAD_ID.load(std::sync::atomic::Ordering::Acquire);
    if raw == 0 {
        None
    } else {
        // SAFETY: we stored a `pthread_t` (opaque pointer-sized handle)
        // earlier; `pthread_t` is layout-compatible with `usize` on Linux
        // and macOS. We never dereference the value — we only hand it
        // back to libc.
        Some(raw as libc::pthread_t)
    }
}

/// IoPool thread count default heuristic: `max(2, available_parallelism
/// / 4)`. Conservative because IoPool threads spin briefly between ticks
/// and shouldn't burn full cores. Tests pass overrides via
/// `TestServerBuilder.io_threads(n)` so production env reads stay
/// consolidated in `ServerV18Config::from_env()`.
fn default_io_threads() -> usize {
    let p = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    std::cmp::max(2, p / 4)
}

// Architectural note: the mio event loop runs on a dedicated std::thread.
//
// Per-tick lifecycle:
//   1. EventLoop::tick — poll mio (up to 5 ms timeout).
//   2. Accept new connections; classify ready clients into read/write sets.
//   3. Read phase — IoPool::publish parse work → join_all. Workers run
//      socket.read + parse_*_request on their own threads.
//   4. Apply phase — single-threaded on this thread. Drain each client's
//      parsed requests → apply_shard.dispatch_wire_request_sync → push
//      GlueResponses into the client's output_queue.
//   5. Write phase — IoPool::publish write work → join_all. Workers run
//      serialize + socket.write.
//   6. Cleanup closed clients; check shutdown flag.
//
// clients: Vec<Option<MioClient>> is pre-allocated to MAX_CONCURRENT_CLIENTS
// at startup and never resized — IoPool worker threads hold raw pointers
// (as_mut_ptr().add(idx)) into the Vec for the duration of each publish +
// join_all cycle. The two phases are serialised by IoPool::join_all()
// Acquire barriers so the apply thread never touches the same client a
// worker is touching.

/// Per-iteration response batch entry: `(worker_index, slot_idx,
/// encoder)`. The worker is selected by the apply thread
/// (`slot_idx % n_workers`); the encoder runs on the IO worker thread
/// once the batch is flushed.
type ResponseBatchEntry = (
    usize,
    u64,
    beava_runtime_core::io_thread_worker::WriteEncoder,
);

/// Dispatch a single `RingItem` from the apply thread. Extracted into a
/// free helper so the top-of-loop `try_recv()` drain and the idle-backoff
/// `recv_timeout` Ok arm share one dispatch shape.
///
/// Pushes responses into the per-iteration `response_batch` rather than
/// calling `write_txs[w].send` immediately; the caller flushes when the
/// batch reaches `BATCH_SIZE_FLUSH=16` OR `BATCH_TIME_FLUSH=100µs`
/// elapsed. `batch_started_at` is set on the FIRST push into an empty
/// batch so the timer starts from "first response of this batch", not
/// "last drain pass".
///
/// The encoder closure takes a `&BytesMutPool` from the worker side, so
/// each response acquires a pool buffer, encodes into it, extends
/// `client_write_buf` from the pool buffer (reusing the allocation),
/// then returns the pool buffer.
#[inline]
fn dispatch_one_ring_item(
    item: beava_runtime_core::work_ring::RingItem,
    apply_shard: &crate::apply_shard::ApplyShard,
    n_workers: usize,
    response_batch: &mut smallvec::SmallVec<[ResponseBatchEntry; 16]>,
    batch_started_at: &mut Option<Instant>,
) {
    use beava_runtime_core::io_thread_worker::{WorkerProto, WriteEncoder};
    use beava_runtime_core::work_ring::{ParseErrorKind, RingItem};
    let _ = apply_shard; // referenced via match arm below; silence linter on early returns
    match item {
        RingItem::Request {
            slot_idx,
            keep_alive: _,
            request,
            parsed_row,
        } => {
            let responses = apply_shard.dispatch_wire_request_with_row(request, parsed_row);
            let slot_u64 = slot_idx as u64;
            let w = (slot_u64 as usize) % n_workers;
            for resp in responses {
                let encoder: WriteEncoder = Box::new(move |proto, pool, client_buf| {
                    let mut tmp = pool.acquire();
                    match proto {
                        WorkerProto::Tcp => encode_glue_response_tcp(&resp, &mut tmp),
                        WorkerProto::Http => encode_glue_response_http(&resp, &mut tmp),
                    }
                    client_buf.extend_from_slice(&tmp);
                    pool.release(tmp);
                });
                if batch_started_at.is_none() {
                    *batch_started_at = Some(Instant::now());
                }
                response_batch.push((w, slot_u64, encoder));
            }
        }
        RingItem::ParseError { slot_idx, kind } => {
            use crate::runtime_core_glue::GlueResponse;
            let slot_u64 = slot_idx as u64;
            let w = (slot_u64 as usize) % n_workers;
            let resp = match kind {
                ParseErrorKind::TcpFrameTooLarge { declared, limit } => GlueResponse::TcpError {
                    code: "frame_too_large",
                    message: format!("declared frame length {declared} exceeds limit {limit}",),
                    extras: serde_json::json!({"limit": limit, "declared": declared}),
                },
                ParseErrorKind::TcpFrame => GlueResponse::PushError {
                    code: "frame_error",
                    registry_version: 0,
                },
                ParseErrorKind::HttpProtocol => GlueResponse::PushError {
                    code: "http_protocol_error",
                    registry_version: 0,
                },
            };
            let encoder: WriteEncoder = Box::new(move |proto, pool, client_buf| {
                let mut tmp = pool.acquire();
                match proto {
                    WorkerProto::Tcp => encode_glue_response_tcp(&resp, &mut tmp),
                    WorkerProto::Http => encode_glue_response_http(&resp, &mut tmp),
                }
                client_buf.extend_from_slice(&tmp);
                pool.release(tmp);
            });
            if batch_started_at.is_none() {
                *batch_started_at = Some(Instant::now());
            }
            response_batch.push((w, slot_u64, encoder));
        }
    }
}

/// Flush the per-iteration response batch. Groups by worker index `w`,
/// sends each worker's slice via `WriteRingExt::send_batch` (one
/// `channel.send` per item; amortisation comes from firing each worker's
/// waker once per flush rather than once per response). Resets
/// `batch_started_at` to `None` and returns the total items flushed.
#[inline]
fn flush_response_batch(
    response_batch: &mut smallvec::SmallVec<[ResponseBatchEntry; 16]>,
    batch_started_at: &mut Option<Instant>,
    write_txs: &[crossbeam_channel::Sender<(
        u64,
        beava_runtime_core::io_thread_worker::WriteEncoder,
    )>],
    worker_wakers: &[Arc<dyn beava_runtime_core::io_backend::WakerHandle>],
    n_workers: usize,
) -> usize {
    use beava_runtime_core::io_thread_worker::WriteEncoder;
    use beava_runtime_core::work_ring::WriteRingExt;
    if response_batch.is_empty() {
        return 0;
    }
    let total = response_batch.len();
    // `n_workers` is small (≤ 32 with `default_io_threads`); a fixed-size
    // stack array beats a HashMap here.
    let mut per_worker: smallvec::SmallVec<[Vec<(u64, WriteEncoder)>; 32]> =
        smallvec::SmallVec::with_capacity(n_workers);
    for _ in 0..n_workers {
        per_worker.push(Vec::new());
    }
    for (w, slot, enc) in response_batch.drain(..) {
        per_worker[w].push((slot, enc));
    }
    let mut affected_workers: u32 = 0;
    for (w, items) in per_worker.iter_mut().enumerate() {
        if items.is_empty() {
            continue;
        }
        let _ = write_txs[w].send_batch(std::mem::take(items));
        affected_workers |= 1u32 << (w & 31);
    }
    if affected_workers != 0 {
        for (w, waker) in worker_wakers.iter().enumerate() {
            if (affected_workers >> (w & 31)) & 1 == 1 {
                let _ = waker.wake();
            }
        }
    }
    *batch_started_at = None;
    APPLY_RESPONSE_BATCH_FLUSHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    total
}

fn run_mio_event_loop(
    http_listener_std: std::net::TcpListener,
    tcp_listener_std: std::net::TcpListener,
    apply_shard: crate::apply_shard::ApplyShard,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    tcp_max_frame_bytes: u32,
    io_threads_override: Option<usize>,
) {
    // Per-worker continuous-loop model (Valkey 8 shape): each worker owns
    // its own `MioBackend` (its own `mio::Poll` + `Waker` + a disjoint
    // subset of clients keyed by `slot_idx % n_workers`). The apply
    // thread owns only the two listeners and the dispatch path:
    //   - polls the listeners on its own `EventLoop`
    //   - drains a shared MPSC `read_rx` (workers parse and forward
    //     `RingItem`s)
    //   - dispatches via `apply_shard`, encodes responses, sends bytes
    //     back to the owning worker via `write_tx[w]` and wakes that
    //     worker
    //   - on accept, hands the new client to a worker via
    //     `new_client_tx[w]`.
    use beava_runtime_core::event_loop::EventLoop;
    use beava_runtime_core::http_listener::HttpListener;
    use beava_runtime_core::io_backend::MioBackend;
    use beava_runtime_core::io_thread_worker::{
        start_worker, NewClient, WorkerConfig, WorkerHandle, WorkerProto,
    };
    use beava_runtime_core::tcp_listener::TcpListener as MioTcpListener;
    use beava_runtime_core::work_ring::RingItem;
    use std::sync::atomic::Ordering as AOrdering;

    // Record this thread as the apply thread; the iopool observer
    // compares parse/encode call sites against this id.
    iopool_observer::set_apply_tid();

    // Record the apply thread's `pthread_t` for per-thread CPU accounting
    // — process `rusage` is useless here because it sums across apply +
    // N IO workers + admin tokio workers. macOS uses
    // `mach_thread_basic_info` via `pthread_mach_thread_np`; Linux uses
    // `pthread_getcpuclockid`.
    {
        let pid: libc::pthread_t = unsafe { libc::pthread_self() };
        APPLY_PTHREAD_ID.store(pid as usize, std::sync::atomic::Ordering::Release);
    }

    // Build the apply-thread `EventLoop` BEFORE the workers so we can
    // register the apply-side waker with its `mio::Registry` and clone
    // it into each worker's config.
    let mut event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            tracing::error!("apply thread: EventLoop::new failed: {e}");
            return;
        }
    };

    // `mio::Waker` bound to apply's listener `EventLoop`. Workers fire
    // this after pushing `RingItem`s to `read_rx` so apply doesn't sit in
    // `event_loop.tick(timeout)` while the channel has work waiting.
    let apply_waker = match mio::Waker::new(event_loop.registry(), TOKEN_APPLY_WAKER) {
        Ok(w) => Arc::new(w),
        Err(e) => {
            tracing::error!("apply thread: mio::Waker::new failed: {e}");
            return;
        }
    };

    // Explicit override wins; falls back to `default_io_threads()` when
    // `None`. The env-read lives in `ServerV18Config::from_env()`.
    let n_workers = io_threads_override
        .map(|n| n.max(1))
        .unwrap_or_else(default_io_threads);

    // Single MPSC for parsed `RingItem`s: every worker clones the sender;
    // apply owns the receiver. 16384 capacity gives ~4× headroom over
    // the legacy IoPool budget.
    let (read_tx, read_rx) = crossbeam_channel::bounded::<RingItem>(16_384);

    // Per-worker `write_tx` (apply → worker, encoder closures) plus
    // matching `WorkerHandle`s. Wakers are cached in a parallel `Vec` so
    // the hot dispatch loop doesn't re-`Arc::clone` per send.
    use beava_runtime_core::io_thread_worker::WriteEncoder;
    let mut write_txs: Vec<crossbeam_channel::Sender<(u64, WriteEncoder)>> =
        Vec::with_capacity(n_workers);
    let mut workers: Vec<WorkerHandle> = Vec::with_capacity(n_workers);
    let mut worker_wakers: Vec<Arc<dyn beava_runtime_core::io_backend::WakerHandle>> =
        Vec::with_capacity(n_workers);

    for w in 0..n_workers {
        let (write_tx, write_rx) = crossbeam_channel::bounded::<(u64, WriteEncoder)>(4_096);
        let (new_client_tx, new_client_rx) = crossbeam_channel::bounded::<NewClient>(256);

        let cfg = WorkerConfig {
            worker_id: w,
            n_workers,
            read_tx: read_tx.clone(),
            write_rx,
            new_client_rx,
            stop: Arc::clone(&shutdown),
            apply_waker: Some(Arc::clone(&apply_waker)),
            tcp_max_frame_bytes,
        };
        let handle = start_worker::<MioBackend>(cfg, new_client_tx, write_tx.clone());
        worker_wakers.push(handle.waker());
        workers.push(handle);
        write_txs.push(write_tx);
    }
    // Apply reads from `read_rx`; drop our spare sender clone so the
    // channel disconnects cleanly when all workers exit.
    drop(read_tx);

    tracing::debug!(
        target: "beava.server",
        kind = "workers.started",
        threads = n_workers,
        "per-worker continuous-loop pool started"
    );
    let mut http_listener = match HttpListener::from_std(http_listener_std) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("apply thread: HttpListener::from_std failed: {e}");
            return;
        }
    };
    let mut tcp_listener = match MioTcpListener::from_std(tcp_listener_std) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("apply thread: MioTcpListener::from_std failed: {e}");
            return;
        }
    };
    if let Err(e) = event_loop.register(
        http_listener.inner_mut(),
        TOKEN_HTTP_LISTENER,
        mio::Interest::READABLE,
    ) {
        tracing::error!("apply thread: register http listener failed: {e}");
        return;
    }
    if let Err(e) = event_loop.register(
        tcp_listener.inner_mut(),
        TOKEN_TCP_LISTENER,
        mio::Interest::READABLE,
    ) {
        tracing::error!("apply thread: register tcp listener failed: {e}");
        return;
    }

    // Apply-side mirror of the per-slot proto so the response encoder
    // can pick the right wire shape. Workers don't signal close back, so
    // entries leak slowly until process exit (TODO once close-notify
    // lands).
    let mut slot_proto: std::collections::HashMap<u64, WorkerProto> =
        std::collections::HashMap::new();
    let mut accept_seq: u64 = 0;

    tracing::debug!(target: "beava.server", "apply thread: dispatcher loop started");

    // Adaptive busy-poll: tight-spin on `read_rx.try_recv()` for up to
    // `SPIN_BUDGET_K` consecutive empty iterations, then fall through to
    // a single blocking `read_rx.recv_timeout(50µs)`. The blocking call
    // wakes immediately when a worker sends; otherwise it returns
    // `Err(Timeout)` so the apply core doesn't burn 100% CPU at no-load.
    //
    // The listener-poll cadence ALWAYS runs every `LISTENER_POLL_EVERY`
    // iterations as a non-blocking `event_loop.tick(0)` — accept latency
    // stays bounded regardless of read pressure. Idleness lives entirely
    // on the channel side via `recv_timeout` (no blocking listener poll).
    //
    // Per-iteration `response_batch`: responses queue into a SmallVec
    // (inline-cap 16); flush fires at `BATCH_SIZE_FLUSH=16` OR
    // `BATCH_TIME_FLUSH=100µs` elapsed since first push. The flush groups
    // by worker and wakes each affected worker ONCE — N response wakes
    // collapse into 1 wake per batch.
    const LISTENER_POLL_EVERY: u32 = 1024;
    const SPIN_BUDGET_K: u32 = 10_000;
    const BATCH_SIZE_FLUSH: usize = 16;
    const BATCH_TIME_FLUSH: Duration = Duration::from_micros(100);
    let mut iter_counter: u32 = 0;
    let mut idle_iters: u32 = 0;
    let mut response_batch: smallvec::SmallVec<[ResponseBatchEntry; 16]> =
        smallvec::SmallVec::new();
    let mut batch_started_at: Option<Instant> = None;

    loop {
        // Drain `read_rx` until empty: apply is single-threaded so we want
        // to dispatch every queued item in arrival order before checking
        // accepts. `read_rx` is bounded(16_384) so even under burst load
        // the drain is bounded by channel capacity (workers backpressure
        // on the send). The accept cadence runs every
        // `LISTENER_POLL_EVERY=1024` OUTER iterations, not every 1024
        // items. Inside the drain we also size-flush at
        // `BATCH_SIZE_FLUSH=16` so the batch can't grow unboundedly.
        let mut drained: u64 = 0;
        let mut read_rx_disconnected = false;
        loop {
            let item = match read_rx.try_recv() {
                Ok(it) => it,
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    tracing::info!(
                        target: "beava.server",
                        "apply thread: read_rx disconnected during drain"
                    );
                    read_rx_disconnected = true;
                    break;
                }
            };
            drained += 1;
            dispatch_one_ring_item(
                item,
                &apply_shard,
                n_workers,
                &mut response_batch,
                &mut batch_started_at,
            );
            // The SmallVec inline cap is 16; flush + restart accumulation
            // once we'd otherwise spill to heap.
            if response_batch.len() >= BATCH_SIZE_FLUSH {
                flush_response_batch(
                    &mut response_batch,
                    &mut batch_started_at,
                    &write_txs,
                    &worker_wakers,
                    n_workers,
                );
            }
        }

        // Time-flush trigger: under sparse load the size-16 flush never
        // fires, so the 100 µs floor bounds response p99 latency.
        if !response_batch.is_empty()
            && batch_started_at.is_some_and(|t| t.elapsed() >= BATCH_TIME_FLUSH)
        {
            flush_response_batch(
                &mut response_batch,
                &mut batch_started_at,
                &write_txs,
                &worker_wakers,
                n_workers,
            );
        }

        iter_counter = iter_counter.wrapping_add(1);
        if drained > 0 {
            APPLY_MAX_DRAIN_PER_ITER.fetch_max(drained, std::sync::atomic::Ordering::Relaxed);
            idle_iters = 0;
        } else {
            idle_iters = idle_iters.saturating_add(1);
        }

        // Listener poll is ALWAYS non-blocking; idle backoff lives in the
        // `recv_timeout` branch below. This way listener accepts never
        // block on the channel and channel recv never blocks on listener
        // events.
        if iter_counter % LISTENER_POLL_EVERY == 0 {
            let tokens: Vec<mio::Token> = match event_loop.tick(Some(Duration::from_millis(0))) {
                Ok(events) => events.map(|e| e.token()).collect(),
                Err(e) => {
                    tracing::warn!("apply thread: poll error: {e}");
                    continue;
                }
            };

            for token in tokens {
                if token == TOKEN_HTTP_LISTENER {
                    accept_clients_to_workers(
                        &mut http_listener,
                        WorkerProto::Http,
                        &workers,
                        &mut slot_proto,
                        &mut accept_seq,
                    );
                } else if token == TOKEN_TCP_LISTENER {
                    accept_clients_to_workers(
                        &mut tcp_listener,
                        WorkerProto::Tcp,
                        &workers,
                        &mut slot_proto,
                        &mut accept_seq,
                    );
                } else if token == TOKEN_APPLY_WAKER {
                    // Worker pushed `RingItem`s to `read_rx`; the next
                    // iteration's drain pass picks them up. No work here.
                }
                // Client-token events stay on the workers' `EventLoop`s —
                // apply thread should never see them. Defensive: ignore
                // unknown tokens.
            }
        }

        // Idle backoff: after `SPIN_BUDGET_K` consecutive empty
        // `try_recv` passes, block on the channel for up to 50 µs. A
        // worker push wakes us within ~50 ns (in-process signal; no
        // kqueue/epoll round-trip). On `Timeout` we re-enter the spin
        // loop and listener-poll cadence is unchanged.
        //
        // Before blocking we flush any pending `response_batch` — the
        // `recv_timeout` might block for the full 50 µs and queued
        // responses shouldn't pay that latency while we're idle.
        //
        // Listener cross-wake: a non-blocking `event_loop.tick(0)` runs
        // BEFORE `recv_timeout` so accept latency stays bounded. Without
        // it, a sparse-load accept would have to wait
        // `LISTENER_POLL_EVERY × recv_timeout ≈ 1024 × 50 µs ≈ 50 ms` —
        // a 1000× first-connection-latency regression. The extra
        // `tick(0)` is ~1 µs and bounded.
        if idle_iters >= SPIN_BUDGET_K {
            if !response_batch.is_empty() {
                flush_response_batch(
                    &mut response_batch,
                    &mut batch_started_at,
                    &write_txs,
                    &worker_wakers,
                    n_workers,
                );
            }
            // Drain pending listener events before blocking.
            if let Ok(events) = event_loop.tick(Some(Duration::from_millis(0))) {
                for token in events.map(|e| e.token()) {
                    if token == TOKEN_HTTP_LISTENER {
                        accept_clients_to_workers(
                            &mut http_listener,
                            WorkerProto::Http,
                            &workers,
                            &mut slot_proto,
                            &mut accept_seq,
                        );
                    } else if token == TOKEN_TCP_LISTENER {
                        accept_clients_to_workers(
                            &mut tcp_listener,
                            WorkerProto::Tcp,
                            &workers,
                            &mut slot_proto,
                            &mut accept_seq,
                        );
                    }
                }
            }
            APPLY_RECV_TIMEOUT_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            match read_rx.recv_timeout(Duration::from_micros(50)) {
                Ok(item) => {
                    dispatch_one_ring_item(
                        item,
                        &apply_shard,
                        n_workers,
                        &mut response_batch,
                        &mut batch_started_at,
                    );
                    // Flush immediately — we were just blocked, no point
                    // holding the response back further.
                    flush_response_batch(
                        &mut response_batch,
                        &mut batch_started_at,
                        &write_txs,
                        &worker_wakers,
                        n_workers,
                    );
                    idle_iters = 0;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // Don't reset `idle_iters` — if still idle next
                    // iteration we want to immediately re-enter
                    // `recv_timeout`.
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    tracing::debug!(
                        target: "beava.server",
                        "apply thread: read_rx disconnected (all workers gone), exiting"
                    );
                    break;
                }
            }
        }

        if shutdown.load(AOrdering::Acquire) {
            tracing::debug!(
                target: "beava.server",
                "apply thread: shutdown signal received, stopping workers"
            );
            break;
        }
        if read_rx_disconnected {
            break;
        }
    }

    // Final flush on shutdown so queued acks aren't lost.
    if !response_batch.is_empty() {
        flush_response_batch(
            &mut response_batch,
            &mut batch_started_at,
            &write_txs,
            &worker_wakers,
            n_workers,
        );
    }

    for w in &workers {
        if let Err(error) = w.waker().wake() {
            tracing::error!(%error, "failed to wake worker")
        }
    }
    for w in workers {
        w.join();
    }
    tracing::debug!(target: "beava.server", "apply thread: exiting");
}

/// Route accepted clients to per-worker `IoBackend`s. Each accepted stream
/// gets a monotonic `slot_idx` and is dispatched to `worker[slot_idx %
/// n_workers]` via `send_new_client_with_proto`. Apply records the slot's
/// protocol in `slot_proto` so it can encode responses correctly; the
/// worker tracks the same proto independently for parse.
fn accept_clients_to_workers<L>(
    listener: &mut L,
    proto: beava_runtime_core::io_thread_worker::WorkerProto,
    workers: &[beava_runtime_core::io_thread_worker::WorkerHandle],
    slot_proto: &mut std::collections::HashMap<
        u64,
        beava_runtime_core::io_thread_worker::WorkerProto,
    >,
    accept_seq: &mut u64,
) where
    L: AcceptStream,
{
    let n_workers = workers.len();
    if n_workers == 0 {
        return;
    }
    loop {
        match listener.accept_stream() {
            Ok(stream) => {
                let slot_idx = *accept_seq;
                *accept_seq = accept_seq.wrapping_add(1);
                let w = (slot_idx as usize) % n_workers;
                slot_proto.insert(slot_idx, proto);
                if let Err(e) = workers[w].send_new_client_with_proto(stream, slot_idx, proto) {
                    tracing::warn!(
                        target: "beava.server",
                        "apply thread: send_new_client to worker {} failed: {}",
                        w,
                        e
                    );
                    slot_proto.remove(&slot_idx);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => {
                tracing::warn!(target: "beava.server", "apply thread: accept failed: {e}");
                break;
            }
        }
    }
}

/// Newtype wrapping a raw mut pointer so it can ride inside a `Send`
/// `WorkItem` closure without the per-call `unsafe impl Send` boilerplate.
///
/// SAFETY (`Send` / `Sync` impls): the pointer always points into a `Vec`
/// pre-allocated to `MAX_CONCURRENT_CLIENTS` and never resized. Aliasing is
/// bounded by the IoPool's Release/Acquire barrier — only one worker
/// touches a given slot index per tick, and the apply thread waits at
/// `join_all()` before reading.
// reason: legacy IoPool scaffolding — see TOKEN_CLIENT_BASE above.
#[allow(dead_code)]
#[derive(Clone, Copy)]
struct ClientsPtr(*mut Option<MioClient>);

// SAFETY: see `ClientsPtr` docs — aliasing is bounded by IoPool barriers.
unsafe impl Send for ClientsPtr {}
unsafe impl Sync for ClientsPtr {}

// reason: legacy IoPool scaffolding — see TOKEN_CLIENT_BASE above.
#[allow(dead_code)]
impl ClientsPtr {
    /// Access the slot at `idx`. Method form (rather than direct field
    /// access) forces closures to capture the whole `ClientsPtr` instead
    /// of the inner pointer disjointly (RFC 2229).
    ///
    /// SAFETY: see the struct-level docs.
    #[inline]
    unsafe fn slot_mut(self, idx: usize) -> *mut Option<MioClient> {
        self.0.add(idx)
    }
}

// reason: legacy IoPool scaffolding — see TOKEN_CLIENT_BASE above.
#[allow(dead_code)]
impl MioClient {
    /// True if the client has bytes to write — either un-serialised
    /// `GlueResponse`s in `output_queue` or partially-flushed bytes in
    /// `write_buf`.
    fn has_write_work(&self) -> bool {
        !self.output_queue.is_empty() || !self.write_buf.is_empty()
    }
}

/// Accept all pending connections from `listener` until WouldBlock, allocate
/// a free slot for each, and register with the event loop.
// reason: legacy IoPool scaffolding — see TOKEN_CLIENT_BASE above.
#[allow(dead_code)]
fn accept_clients<L>(
    listener: &mut L,
    proto: MioProto,
    clients: &mut [Option<MioClient>],
    free_slots: &mut Vec<usize>,
    event_loop: &mut beava_runtime_core::event_loop::EventLoop,
) where
    L: AcceptStream,
{
    loop {
        match listener.accept_stream() {
            Ok(stream) => {
                let slot_idx = match free_slots.pop() {
                    Some(i) => i,
                    None => {
                        tracing::warn!(
                            target: "beava.server",
                            "apply thread: no free client slot — dropping connection (>= {} concurrent clients)",
                            MAX_CONCURRENT_CLIENTS
                        );
                        // Drop the stream by letting it leave scope.
                        drop(stream);
                        break;
                    }
                };
                let client_token = mio::Token(slot_idx + TOKEN_CLIENT_BASE);
                let mut client = MioClient {
                    stream,
                    token: client_token,
                    proto,
                    read_buf: bytes::BytesMut::with_capacity(8 * 1024),
                    output_queue: std::collections::VecDeque::new(),
                    write_buf: bytes::BytesMut::new(),
                    interest_writable: false,
                    closed: false,
                };
                if let Err(e) =
                    event_loop.register(&mut client.stream, client_token, mio::Interest::READABLE)
                {
                    tracing::warn!("apply thread: register client failed: {e}");
                    free_slots.push(slot_idx);
                } else {
                    clients[slot_idx] = Some(client);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
}

/// Trait abstraction over `HttpListener` / `TcpListener` so `accept_clients`
/// can drive both. Each impl just calls its `accept(...)` method and returns
/// the underlying `mio::net::TcpStream`.
trait AcceptStream {
    fn accept_stream(&mut self) -> std::io::Result<mio::net::TcpStream>;
}

impl AcceptStream for beava_runtime_core::http_listener::HttpListener {
    fn accept_stream(&mut self) -> std::io::Result<mio::net::TcpStream> {
        self.accept().map(|(s, _)| s)
    }
}

impl AcceptStream for beava_runtime_core::tcp_listener::TcpListener {
    fn accept_stream(&mut self) -> std::io::Result<mio::net::TcpStream> {
        self.accept().map(|(s, _)| s)
    }
}

/// Read-phase that pushes each decoded frame straight into a crossbeam
/// channel rather than batching into `client.parsed_requests` /
/// `client.parsed_rows` `Vec`s. Lets the apply thread dispatch events the
/// instant a single worker has parsed one — removes the per-tick
/// `IoPool::join_all` spin barrier (the dominant source of inter-event
/// gap on macOS pre-fix: ~218 µs every ~128 events at p=4/pd=64).
///
/// The channel is bounded; if apply falls behind, `send` blocks the
/// worker briefly. In normal operation apply is faster than parse (apply
/// ~0.9 µs vs parse ~4 µs per push), so contention is rare.
// reason: legacy IoPool scaffolding — see TOKEN_CLIENT_BASE above.
#[allow(dead_code)]
fn read_and_parse_client_to_channel(
    client: &mut MioClient,
    slot_idx: u32,
    sender: &crossbeam_channel::Sender<beava_runtime_core::work_ring::RingItem>,
) {
    use beava_core::row::Row;
    use beava_core::wire::CT_MSGPACK;
    use beava_runtime_core::http_listener::parse_http_request;
    use beava_runtime_core::tcp_listener::parse_wire_request;
    use beava_runtime_core::wire_request::WireRequest;
    use beava_runtime_core::work_ring::{ParseErrorKind, RingItem};
    use std::io::Read;

    if client.closed {
        return;
    }

    // Drain socket → read_buf.
    let mut tmp_buf = [0u8; 16 * 1024];
    loop {
        match client.stream.read(&mut tmp_buf) {
            Ok(0) => {
                client.closed = true;
                return;
            }
            Ok(n) => {
                client.read_buf.extend_from_slice(&tmp_buf[..n]);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => {
                client.closed = true;
                return;
            }
        }
    }

    if client.read_buf.is_empty() {
        return;
    }

    // Parse each frame, do body→Row inline, push to channel.
    iopool_observer::record_parse();

    // Deserialize body → `Row` for push variants. `None` for non-push or
    // when deserialization fails — apply will surface `invalid_event`.
    let body_to_row = |req: &WireRequest| -> Option<Row> {
        match req {
            WireRequest::TcpPush {
                body, body_format, ..
            }
            | WireRequest::HttpPush {
                body, body_format, ..
            }
            | WireRequest::HttpPushSync {
                body, body_format, ..
            }
            | WireRequest::HttpPushBatch {
                body, body_format, ..
            } => {
                if *body_format == CT_MSGPACK {
                    rmp_serde::from_slice::<Row>(body).ok()
                } else {
                    sonic_rs::from_slice::<Row>(body).ok()
                }
            }
            _ => None,
        }
    };

    match client.proto {
        MioProto::Tcp => loop {
            match parse_wire_request(&mut client.read_buf, 4 * 1024 * 1024) {
                Ok(Some(req)) => {
                    let parsed_row = body_to_row(&req);
                    if sender
                        .send(RingItem::Request {
                            slot_idx,
                            keep_alive: false,
                            request: req,
                            parsed_row,
                        })
                        .is_err()
                    {
                        // Receiver dropped (server shutting down).
                        return;
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    let _ = sender.send(RingItem::ParseError {
                        slot_idx,
                        kind: ParseErrorKind::TcpFrame,
                    });
                    break;
                }
            }
        },
        MioProto::Http => loop {
            match parse_http_request(&mut client.read_buf) {
                Ok(Some((req, keep_alive))) => {
                    let parsed_row = body_to_row(&req);
                    if sender
                        .send(RingItem::Request {
                            slot_idx,
                            keep_alive,
                            request: req,
                            parsed_row,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    let _ = sender.send(RingItem::ParseError {
                        slot_idx,
                        kind: ParseErrorKind::HttpProtocol,
                    });
                    break;
                }
            }
        },
    }
}

/// Drain the work-ring receiver concurrently with IoPool workers
/// running. Returns when both:
///   1. all IoPool worker threads have signalled `pending = 0`
///   2. the receiver channel is empty.
///
/// Replaces the prior `IoPool::join_all` + `drain_parsed_requests`
/// two-step that forced apply to wait for every worker to finish parsing
/// before dispatching any event. Now apply dispatches as events arrive,
/// overlapping parse-on-IoPool with apply-on-apply-thread.
///
/// Per-event flow:
/// - `try_recv` → `dispatch_wire_request_with_row` → push response to
///   `clients[slot_idx].output_queue`
/// - `ParseError` → push error response, mark client closed.
// reason: legacy IoPool scaffolding — see TOKEN_CLIENT_BASE above.
#[allow(dead_code)]
fn drain_channel_until_workers_done(
    io_pool: &beava_runtime_core::io_pool::IoPool,
    receiver: &crossbeam_channel::Receiver<beava_runtime_core::work_ring::RingItem>,
    apply_shard: &crate::apply_shard::ApplyShard,
    clients: &mut [Option<MioClient>],
) {
    use beava_runtime_core::work_ring::{ParseErrorKind, RingItem};
    use std::sync::atomic::Ordering;

    const SPIN_ITERS: u32 = 1024;
    let mut idle_count: u32 = 0;

    loop {
        // Greedy drain — pull as many as we can.
        let mut drained_any = false;
        while let Ok(item) = receiver.try_recv() {
            drained_any = true;
            match item {
                RingItem::Request {
                    slot_idx,
                    keep_alive: _,
                    request,
                    parsed_row,
                } => {
                    let responses = apply_shard.dispatch_wire_request_with_row(request, parsed_row);
                    if let Some(slot) = clients.get_mut(slot_idx as usize) {
                        if let Some(client) = slot.as_mut() {
                            for resp in responses {
                                client.output_queue.push_back(resp);
                            }
                        }
                    }
                }
                RingItem::ParseError { slot_idx, kind } => {
                    if let Some(slot) = clients.get_mut(slot_idx as usize) {
                        if let Some(client) = slot.as_mut() {
                            use crate::runtime_core_glue::GlueResponse;
                            client.output_queue.push_back(match kind {
                                ParseErrorKind::TcpFrameTooLarge { declared, limit } => {
                                    GlueResponse::TcpError {
                                        code: "frame_too_large",
                                        message: format!(
                                            "declared frame length {declared} exceeds limit {limit}",
                                        ),
                                        extras: serde_json::json!({
                                            "limit": limit,
                                            "declared": declared,
                                        }),
                                    }
                                }
                                ParseErrorKind::TcpFrame => GlueResponse::PushError {
                                    code: "frame_error",
                                    registry_version: 0,
                                },
                                ParseErrorKind::HttpProtocol => GlueResponse::PushError {
                                    code: "http_protocol_error",
                                    registry_version: 0,
                                },
                            });
                            client.closed = true;
                        }
                    }
                }
            }
        }

        // Termination: all workers done AND channel empty.
        let all_workers_done = io_pool
            .slots
            .iter()
            .all(|s| s.pending.load(Ordering::Acquire) == 0);
        if all_workers_done && receiver.is_empty() {
            break;
        }

        // Apply backoff if we didn't drain anything this iteration. Workers are
        // still busy parsing — give them CPU.
        if !drained_any {
            idle_count = idle_count.saturating_add(1);
            if idle_count < SPIN_ITERS {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        } else {
            idle_count = 0;
        }
    }
}

/// Write-phase work item body. Runs on an IoPool worker thread.
///
/// 1. Drain `output_queue`, serialize each `GlueResponse` into `write_buf`
///    using the proto-appropriate encoder.
/// 2. Loop on `socket.write` from the head of `write_buf` until `WouldBlock`
///    or `write_buf` is empty.
/// 3. On EOF / error, mark closed.
// reason: legacy IoPool scaffolding — see TOKEN_CLIENT_BASE above.
#[allow(dead_code)]
fn serialize_and_write_client(client: &mut MioClient) {
    use std::io::Write;

    if client.closed {
        return;
    }

    if !client.output_queue.is_empty() {
        iopool_observer::record_encode();
        let queue = std::mem::take(&mut client.output_queue);
        for resp in queue {
            match client.proto {
                MioProto::Tcp => encode_glue_response_tcp(&resp, &mut client.write_buf),
                MioProto::Http => encode_glue_response_http(&resp, &mut client.write_buf),
            }
        }
    }

    while !client.write_buf.is_empty() {
        match client.stream.write(&client.write_buf) {
            Ok(0) => break,
            Ok(n) => {
                let _ = client.write_buf.split_to(n);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => {
                client.closed = true;
                return;
            }
        }
    }
}

/// Encode a GlueResponse as a TCP framed response into `buf`.
fn encode_glue_response_tcp(
    resp: &crate::runtime_core_glue::GlueResponse,
    buf: &mut bytes::BytesMut,
) {
    use crate::runtime_core_glue::GlueResponse;
    use beava_core::wire::{CT_JSON, OP_ERROR_RESPONSE, OP_GET_RESPONSE, OP_PING, OP_PUSH};

    match resp {
        GlueResponse::Pong { registry_version } => {
            let body = serde_json::json!({
                "server_version": crate::VERSION,
                "registry_version": registry_version,
            });
            let b = serde_json::to_vec(&body).unwrap_or_default();
            encode_tcp_frame_bytes(OP_PING, CT_JSON, &b, buf);
        }
        GlueResponse::PushAck {
            ack_lsn,
            registry_version,
        } => {
            // Include `idempotent_replay: false` so callers can
            // discriminate fresh ack from dedupe replay.
            let body = serde_json::json!({
                "ack_lsn": ack_lsn,
                "idempotent_replay": false,
                "registry_version": registry_version,
            });
            let b = serde_json::to_vec(&body).unwrap_or_default();
            encode_tcp_frame_bytes(OP_PUSH, CT_JSON, &b, buf);
        }
        GlueResponse::PushReplay {
            registry_version,
            ack_lsn,
            cached_body: _,
        } => {
            // TCP has no idempotent-replay header (HTTP uses
            // `X-Beava-Idempotent-Replay: 1`); on the TCP wire, the body
            // flag IS the discriminator. Include the original `ack_lsn`
            // so callers can assert `ack1.ack_lsn == ack2.ack_lsn`.
            let body = match ack_lsn {
                Some(lsn) => serde_json::json!({
                    "ack_lsn": lsn,
                    "idempotent_replay": true,
                    "registry_version": registry_version,
                }),
                None => serde_json::json!({
                    "idempotent_replay": true,
                    "registry_version": registry_version,
                }),
            };
            let b = serde_json::to_vec(&body).unwrap_or_default();
            encode_tcp_frame_bytes(OP_PUSH, CT_JSON, &b, buf);
        }
        // Register response (success and error paths funnel through
        // here). `body` is pre-serialised by
        // `register::register_outcome_to_glue`; `tcp_op` is `OP_REGISTER`
        // on success and `OP_ERROR_RESPONSE` on failure.
        GlueResponse::Register { body, tcp_op, .. } => {
            encode_tcp_frame_bytes(*tcp_op, CT_JSON, body, buf);
        }
        GlueResponse::PushError {
            code,
            registry_version,
        } => {
            // Body shape is `{"error": {"code": "..."}}` so callers can
            // pattern-match on `body["error"]["code"]`.
            let body = serde_json::json!({
                "error": {"code": code},
                "registry_version": registry_version,
            });
            let b = serde_json::to_vec(&body).unwrap_or_default();
            encode_tcp_frame_bytes(OP_ERROR_RESPONSE, CT_JSON, &b, buf);
        }
        // TCP `/get` response framing. Echo back the (already-serialised)
        // body framed as `OP_GET_RESPONSE` (0x0023). FIFO correlation on
        // the connection ties this frame to its request — no request_id
        // needed (Redis-shaped strict-FIFO). `*format` is the request
        // frame's content-type byte propagated end-to-end so msgpack-in
        // produces msgpack-out.
        GlueResponse::QueryResult { body, format } => {
            encode_tcp_frame_bytes(OP_GET_RESPONSE, *format, body, buf);
        }
        GlueResponse::QueryNotFound { code } => {
            let body = serde_json::json!({"error": {"code": code}});
            let b = serde_json::to_vec(&body).unwrap_or_default();
            encode_tcp_frame_bytes(OP_ERROR_RESPONSE, CT_JSON, &b, buf);
        }
        // `OP_RESET` success — body `{"reset": true, "registry_version":
        // N}`. Frame opcode is `OP_GET_RESPONSE` (0x0023), the generic
        // JSON success frame.
        GlueResponse::ResetOk { registry_version } => {
            let body = serde_json::json!({
                "reset": true,
                "registry_version": registry_version,
            });
            let b = serde_json::to_vec(&body).unwrap_or_default();
            encode_tcp_frame_bytes(OP_GET_RESPONSE, CT_JSON, &b, buf);
        }
        // `OP_RESET` rejected — frame opcode is the dedicated 0xFFFF
        // error; body matches the HTTP 403 body verbatim.
        GlueResponse::ResetForbidden => {
            let body = reset_forbidden_body();
            let b = serde_json::to_vec(&body).unwrap_or_default();
            encode_tcp_frame_bytes(OP_ERROR_RESPONSE, CT_JSON, &b, buf);
        }
        // Rich TCP error frame: `{"error": {"code": <code>, "message":
        // <msg>, ...extras}}`. `extras` is merged into the error object
        // so callers can carry structured fields (e.g.
        // `frame_too_large.limit`) without new variants.
        GlueResponse::TcpError {
            code,
            message,
            extras,
        } => {
            let mut error_obj = serde_json::Map::new();
            error_obj.insert("code".to_string(), serde_json::json!(code));
            error_obj.insert("message".to_string(), serde_json::json!(message));
            if let serde_json::Value::Object(extras_obj) = extras {
                for (k, v) in extras_obj {
                    error_obj.insert(k.clone(), v.clone());
                }
            }
            let body = serde_json::json!({"error": serde_json::Value::Object(error_obj)});
            let b = serde_json::to_vec(&body).unwrap_or_default();
            encode_tcp_frame_bytes(OP_ERROR_RESPONSE, CT_JSON, &b, buf);
        }
        // Legacy request shape rejected. TCP frame `OP_ERROR_RESPONSE`
        // (0xFFFF); body mirrors the HTTP 400 response.
        GlueResponse::UnsupportedRequestShape { hint } => {
            let body = serde_json::json!({
                "error": {
                    "code": "unsupported_request_shape",
                    "message": hint,
                }
            });
            let b = serde_json::to_vec(&body).unwrap_or_default();
            encode_tcp_frame_bytes(OP_ERROR_RESPONSE, CT_JSON, &b, buf);
        }
        // Structured server-side error. Mirrors the HTTP 500 body shape
        // verbatim — `{"error": {"code": "internal_error", "reason":
        // <msg>}}` — so callers see the same `feature_not_found: ...`
        // diagnostic on both wires. Frame opcode is `OP_ERROR_RESPONSE`
        // (0xFFFF) with `CT_JSON` content type.
        GlueResponse::InternalError { reason } => {
            let body = serde_json::json!({
                "error": {"code": "internal_error", "reason": reason},
            });
            let b = serde_json::to_vec(&body).unwrap_or_default();
            encode_tcp_frame_bytes(OP_ERROR_RESPONSE, CT_JSON, &b, buf);
        }
        _ => {
            encode_tcp_frame_bytes(
                OP_ERROR_RESPONSE,
                CT_JSON,
                b"{\"code\":\"unsupported\"}",
                buf,
            );
        }
    }
}

/// Encode a GlueResponse as a full HTTP/1.1 response into `buf`.
fn encode_glue_response_http(
    resp: &crate::runtime_core_glue::GlueResponse,
    buf: &mut bytes::BytesMut,
) {
    use crate::runtime_core_glue::GlueResponse;

    // Extra response headers (e.g. `x-beava-idempotent-replay: 1` on
    // `PushReplay`). Empty when no extras apply; appended verbatim to
    // the standard HTTP header block below.
    let mut extra_headers = String::new();

    let (status, body_bytes): (u16, Vec<u8>) = match resp {
        GlueResponse::PushAck {
            ack_lsn,
            registry_version,
        } => {
            let body = serde_json::json!({"ack_lsn": ack_lsn, "registry_version": registry_version, "idempotent_replay": false});
            (200, serde_json::to_vec(&body).unwrap_or_default())
        }
        GlueResponse::PushReplay {
            registry_version,
            ack_lsn: _,
            cached_body,
        } => {
            // Byte-identical replay: `cached_body` IS the verbatim
            // original `/push` response (e.g. `{ack_lsn:N,
            // idempotent_replay:false, registry_version:V}`); pass it
            // through unchanged. On cache miss, synthesise the generic
            // `{idempotent_replay: true, registry_version: V}` shape.
            //
            // The `X-Beava-Idempotent-Replay: 1` header is the wire
            // discriminator on HTTP (TCP uses the body flag instead).
            extra_headers.push_str("X-Beava-Idempotent-Replay: 1\r\n");
            if let Some(cached) = cached_body {
                (200, cached.to_vec())
            } else {
                let body = serde_json::json!({"idempotent_replay": true, "registry_version": registry_version});
                (200, serde_json::to_vec(&body).unwrap_or_default())
            }
        }
        // HTTP `/register` response — body is pre-serialised by
        // `register::register_outcome_to_glue`. Status: 200 on success,
        // 400/409/503 on failure.
        GlueResponse::Register {
            http_status, body, ..
        } => (*http_status, body.to_vec()),
        GlueResponse::PushError { code, .. } => {
            let body = serde_json::json!({"error": {"code": code}});
            let status = if *code == "event_not_found" { 404 } else { 400 };
            (status, serde_json::to_vec(&body).unwrap_or_default())
        }
        // HTTP `/get` is JSON-only — the request format byte is ignored
        // here, and the response header below always sets
        // `Content-Type: application/json`.
        GlueResponse::QueryResult { body, format: _ } => (200, body.to_vec()),
        GlueResponse::QueryNotFound { code } => {
            let body = serde_json::json!({"error": {"code": code}});
            (404, serde_json::to_vec(&body).unwrap_or_default())
        }
        GlueResponse::Pong { registry_version } => {
            let body = serde_json::json!({"pong": true, "registry_version": registry_version});
            (200, serde_json::to_vec(&body).unwrap_or_default())
        }
        // `/health` on the data plane. Always 200.
        GlueResponse::HealthOk => (200, br#"{"status":"ok"}"#.to_vec()),
        // `/ready` on the data plane. Always 200 once the listener is
        // up — readiness state lives on the admin sidecar; the data-
        // plane mirror returns 200 unconditionally so test fixtures
        // polling `base_url/ready` converge.
        GlueResponse::ReadyOk => (200, br#"{"status":"ready"}"#.to_vec()),
        // `/registry` on the data plane. Body is the live registry
        // snapshot (the dev-endpoint shape).
        GlueResponse::RegistrySnapshot { body } => (200, body.to_vec()),
        GlueResponse::HttpRouteNotFound { path } => {
            let body = serde_json::json!({"error": {"code": "not_found", "path": path}});
            (404, serde_json::to_vec(&body).unwrap_or_default())
        }
        GlueResponse::HttpMethodNotAllowed { method, path } => {
            let body = serde_json::json!({
                "error": {"code": "method_not_allowed", "method": method, "path": path},
            });
            (405, serde_json::to_vec(&body).unwrap_or_default())
        }
        GlueResponse::HttpUnsupportedMediaType { received, path } => {
            let _ = received;
            let body = serde_json::json!({
                "error": {
                    "code": "unsupported_media_type",
                    "path": path,
                    "reason": "expected application/json",
                },
                "registry_version": 0u64,
            });
            (415, serde_json::to_vec(&body).unwrap_or_default())
        }
        GlueResponse::InternalError { reason } => {
            let body = serde_json::json!({"error": {"code": "internal_error", "reason": reason}});
            (500, serde_json::to_vec(&body).unwrap_or_default())
        }
        GlueResponse::ResetOk { registry_version } => {
            let body = serde_json::json!({"reset": true, "registry_version": registry_version});
            (200, serde_json::to_vec(&body).unwrap_or_default())
        }
        GlueResponse::ResetForbidden => {
            let body = reset_forbidden_body();
            (403, serde_json::to_vec(&body).unwrap_or_default())
        }
        // Legacy request shape rejected. HTTP 400 with body
        // `{"error":{"code":"unsupported_request_shape","message":<hint>}}`.
        GlueResponse::UnsupportedRequestShape { hint } => {
            let body = serde_json::json!({
                "error": {
                    "code": "unsupported_request_shape",
                    "message": hint,
                }
            });
            (400, serde_json::to_vec(&body).unwrap_or_default())
        }
        _ => (501, b"{\"error\":{\"code\":\"unsupported\"}}".to_vec()),
    };

    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "OK",
    };

    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Runtime: hand-rolled\r\nConnection: keep-alive\r\n{}\r\n",
        status,
        status_text,
        body_bytes.len(),
        extra_headers,
    );
    buf.extend_from_slice(header.as_bytes());
    buf.extend_from_slice(&body_bytes);
}

/// Canonical body for `reset_disabled_in_production`. Used by the HTTP
/// (403) and TCP (`OP_ERROR_RESPONSE`) encoders so callers see an
/// identical shape regardless of transport. The reason text mentions
/// both opt-in paths (`BEAVA_TEST_MODE` env, `test_mode` kwarg) so the
/// error is actionable.
fn reset_forbidden_body() -> serde_json::Value {
    serde_json::json!({
        "error": {
            "code": "reset_disabled_in_production",
            "reason": "OP_RESET requires server test_mode (set \
                       BEAVA_TEST_MODE=1 or pass Config { test_mode: true \
                       } at server construction). See docs/error-codes.md."
        }
    })
}

/// Encode a raw TCP framed response into `buf`.
/// Wire format: `[u32 length BE][u16 op BE][u8 content_type][payload]`.
fn encode_tcp_frame_bytes(op: u16, content_type: u8, payload: &[u8], buf: &mut bytes::BytesMut) {
    use bytes::BufMut;
    // Length covers op(2) + content_type(1) + payload.
    let frame_len = 2 + 1 + payload.len() as u32;
    buf.put_u32(frame_len);
    buf.put_u16(op);
    buf.put_u8(content_type);
    buf.extend_from_slice(payload);
}
#[cfg(test)]
mod tests {
    use super::*;

    /// `QueryResult` must frame as `OP_GET_RESPONSE` so TCP `/get`
    /// clients can read the JSON body back under the response opcode.
    #[test]
    fn test_encode_tcp_query_result_emits_op_get_response_frame() {
        use crate::runtime_core_glue::GlueResponse;
        use beava_core::wire::{decode_frame, CT_JSON, OP_GET_RESPONSE};

        let mut buf = bytes::BytesMut::new();
        let resp = GlueResponse::QueryResult {
            body: bytes::Bytes::from_static(br#"{"value":42}"#),
            format: CT_JSON,
        };
        encode_glue_response_tcp(&resp, &mut buf);
        let frame = decode_frame(&mut buf, 4 * 1024 * 1024)
            .expect("decode_frame")
            .expect("complete frame");
        assert_eq!(
            frame.op, OP_GET_RESPONSE,
            "expected OP_GET_RESPONSE (0x0023), got {:#06x}",
            frame.op
        );
        assert_eq!(frame.content_type, CT_JSON);
        assert_eq!(frame.payload.as_ref(), br#"{"value":42}"#);
    }

    /// `QueryNotFound` emits an `OP_ERROR_RESPONSE` frame carrying the
    /// error code in the payload.
    #[test]
    fn test_encode_tcp_query_not_found_emits_error_response() {
        use crate::runtime_core_glue::GlueResponse;
        use beava_core::wire::{decode_frame, OP_ERROR_RESPONSE};

        let mut buf = bytes::BytesMut::new();
        let resp = GlueResponse::QueryNotFound {
            code: "key_not_found",
        };
        encode_glue_response_tcp(&resp, &mut buf);
        let frame = decode_frame(&mut buf, 4 * 1024 * 1024)
            .expect("decode_frame")
            .expect("complete frame");
        assert_eq!(
            frame.op, OP_ERROR_RESPONSE,
            "expected OP_ERROR_RESPONSE for QueryNotFound, got {:#06x}",
            frame.op
        );
        let payload_str = std::str::from_utf8(&frame.payload).expect("utf8 payload");
        assert!(
            payload_str.contains("key_not_found"),
            "payload must carry error code, got: {payload_str}"
        );
    }
}

/// Env-var plumbing unit tests for `ServerV18Config::from_env()` +
/// `WalConfig::resolve()`. Lives in `src/` (not `tests/`) so the
/// `phase13_5_3_no_env_var_pokes_in_tests` tripwire doesn't false-positive
/// on the legitimate env mutation these tests perform. `std::env::*` is
/// process-global, so the `ENV_LOCK` mutex serialises this module's
/// tests.
#[cfg(test)]
mod env_var_plumbing_tests {
    use super::*;
    use crate::wal_config::{WalConfig, WalConfigOverrides};

    /// Serialises tests in this module that mutate process-global env
    /// vars; without it `cargo --test-threads N` would race them against
    /// each other and any other test that reads the same vars.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const ALL_VARS: &[&str] = &[
        "BEAVA_WAL_BUFFERS",
        "BEAVA_WAL_BUFFER_SIZE_MB",
        "BEAVA_WAL_TICK_MS",
        "BEAVA_IO_THREADS",
        "BEAVA_TEST_MODE",
        "BEAVA_MEMORY_GOV_ENFORCE",
    ];

    fn clear_env() {
        for v in ALL_VARS {
            std::env::remove_var(v);
        }
    }

    #[test]
    fn test_default_returns_all_none_overrides() {
        let cfg = ServerV18Config::default();
        assert_eq!(cfg.wal_buffers, None, "default wal_buffers must be None");
        assert_eq!(
            cfg.wal_buffer_size_mb, None,
            "default wal_buffer_size_mb must be None"
        );
        assert_eq!(cfg.wal_tick_ms, None, "default wal_tick_ms must be None");
        assert_eq!(cfg.io_threads, None, "default io_threads must be None");
        assert!(!cfg.test_mode, "default test_mode must be false");
        assert_eq!(
            cfg.memory_governance_enforce, None,
            "default memory_governance_enforce must be None"
        );
    }

    #[test]
    fn test_from_env_unset_returns_defaults() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_env();
        let cfg = ServerV18Config::from_env();
        assert_eq!(cfg.wal_buffers, None);
        assert_eq!(cfg.wal_buffer_size_mb, None);
        assert_eq!(cfg.wal_tick_ms, None);
        assert_eq!(cfg.io_threads, None);
        assert!(!cfg.test_mode);
        assert_eq!(cfg.memory_governance_enforce, None);
    }

    #[test]
    fn test_from_env_populates_all_fields() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_env();
        std::env::set_var("BEAVA_WAL_BUFFERS", "8");
        std::env::set_var("BEAVA_WAL_BUFFER_SIZE_MB", "64");
        std::env::set_var("BEAVA_WAL_TICK_MS", "100");
        std::env::set_var("BEAVA_IO_THREADS", "4");
        std::env::set_var("BEAVA_TEST_MODE", "1");
        std::env::set_var("BEAVA_MEMORY_GOV_ENFORCE", "0");

        let cfg = ServerV18Config::from_env();
        clear_env();

        assert_eq!(cfg.wal_buffers, Some(8));
        assert_eq!(cfg.wal_buffer_size_mb, Some(64));
        assert_eq!(cfg.wal_tick_ms, Some(100));
        assert_eq!(cfg.io_threads, Some(4));
        assert!(cfg.test_mode, "BEAVA_TEST_MODE=1 must enable test_mode");
        assert_eq!(
            cfg.memory_governance_enforce,
            Some(false),
            "BEAVA_MEMORY_GOV_ENFORCE=0 must produce Some(false) (escape hatch)"
        );
    }

    /// `BEAVA_TEST_MODE` is strict `== "1"`; "true", "yes", "0" all
    /// leave test_mode disabled.
    #[test]
    fn test_from_env_test_mode_strict_eq_one() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_env();
        std::env::set_var("BEAVA_TEST_MODE", "true");
        let cfg_true = ServerV18Config::from_env();
        clear_env();
        assert!(
            !cfg_true.test_mode,
            "BEAVA_TEST_MODE=true (NOT `=1`) MUST NOT enable test_mode (D-03 strict check)"
        );

        std::env::set_var("BEAVA_TEST_MODE", "yes");
        let cfg_yes = ServerV18Config::from_env();
        clear_env();
        assert!(
            !cfg_yes.test_mode,
            "BEAVA_TEST_MODE=yes (NOT `=1`) MUST NOT enable test_mode"
        );

        std::env::set_var("BEAVA_TEST_MODE", "0");
        let cfg_zero = ServerV18Config::from_env();
        clear_env();
        assert!(
            !cfg_zero.test_mode,
            "BEAVA_TEST_MODE=0 MUST NOT enable test_mode"
        );

        std::env::set_var("BEAVA_TEST_MODE", "1");
        let cfg_one = ServerV18Config::from_env();
        clear_env();
        assert!(cfg_one.test_mode, "BEAVA_TEST_MODE=1 MUST enable test_mode");
    }

    /// `BEAVA_MEMORY_GOV_ENFORCE`: `"0"` → `Some(false)` (escape hatch),
    /// unset → `None` (= default ON), any other value → `Some(true)`.
    #[test]
    fn test_from_env_memory_governance_enforce_semantics() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_env();
        let cfg_unset = ServerV18Config::from_env();
        assert_eq!(
            cfg_unset.memory_governance_enforce, None,
            "unset BEAVA_MEMORY_GOV_ENFORCE must be None (default ON)"
        );

        std::env::set_var("BEAVA_MEMORY_GOV_ENFORCE", "0");
        let cfg_zero = ServerV18Config::from_env();
        clear_env();
        assert_eq!(cfg_zero.memory_governance_enforce, Some(false));

        std::env::set_var("BEAVA_MEMORY_GOV_ENFORCE", "1");
        let cfg_one = ServerV18Config::from_env();
        clear_env();
        assert_eq!(cfg_one.memory_governance_enforce, Some(true));

        std::env::set_var("BEAVA_MEMORY_GOV_ENFORCE", "anything");
        let cfg_other = ServerV18Config::from_env();
        clear_env();
        assert_eq!(
            cfg_other.memory_governance_enforce,
            Some(true),
            "any non-\"0\" value should produce Some(true)"
        );
    }

    /// `from_env()` clamps WAL fields to their documented ranges.
    #[test]
    fn test_from_env_clamp_ranges_reject_oom_typos() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_env();
        std::env::set_var("BEAVA_WAL_BUFFERS", "99999");
        std::env::set_var("BEAVA_WAL_BUFFER_SIZE_MB", "10000");
        std::env::set_var("BEAVA_WAL_TICK_MS", "99999");
        let cfg = ServerV18Config::from_env();
        clear_env();
        assert_eq!(
            cfg.wal_buffers,
            Some(WalConfig::BUFFERS_MAX),
            "buffers must clamp to <= {} (got {:?})",
            WalConfig::BUFFERS_MAX,
            cfg.wal_buffers
        );
        assert_eq!(
            cfg.wal_buffer_size_mb,
            Some(WalConfig::BUFFER_SIZE_MB_MAX),
            "buffer_size_mb must clamp to <= {} (got {:?})",
            WalConfig::BUFFER_SIZE_MB_MAX,
            cfg.wal_buffer_size_mb
        );
        assert_eq!(
            cfg.wal_tick_ms,
            Some(WalConfig::TICK_MS_MAX),
            "tick_ms must clamp to <= {} (got {:?})",
            WalConfig::TICK_MS_MAX,
            cfg.wal_tick_ms
        );

        clear_env();
        std::env::set_var("BEAVA_WAL_BUFFERS", "0");
        std::env::set_var("BEAVA_WAL_BUFFER_SIZE_MB", "0");
        std::env::set_var("BEAVA_WAL_TICK_MS", "0");
        let cfg = ServerV18Config::from_env();
        clear_env();
        assert_eq!(cfg.wal_buffers, Some(WalConfig::BUFFERS_MIN));
        assert_eq!(cfg.wal_buffer_size_mb, Some(WalConfig::BUFFER_SIZE_MB_MIN));
        assert_eq!(cfg.wal_tick_ms, Some(WalConfig::TICK_MS_MIN));
    }

    /// `WalConfig::resolve(WalConfigOverrides)` honours explicit values
    /// without consulting env. Sets a poisonous env, calls resolve with
    /// overrides, asserts the override wins.
    #[test]
    fn test_wal_config_resolve_overrides_win_over_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_env();
        std::env::set_var("BEAVA_WAL_BUFFERS", "99999");
        std::env::set_var("BEAVA_WAL_BUFFER_SIZE_MB", "99999");
        std::env::set_var("BEAVA_WAL_TICK_MS", "99999");

        let cfg = WalConfig::resolve(WalConfigOverrides {
            buffers: Some(8),
            buffer_size_mb: Some(64),
            tick_ms: Some(100),
        });
        clear_env();

        assert_eq!(
            cfg.buffers, 8,
            "explicit override must win — env=99999 ignored"
        );
        assert_eq!(cfg.buffer_size_mb, 64);
        assert_eq!(cfg.tick_ms, 100);
    }

    /// `WalConfig::resolve(WalConfigOverrides::default())` returns the
    /// documented defaults (4 × 32 MiB, tick=20 ms) without consulting
    /// env.
    #[test]
    fn test_wal_config_resolve_none_returns_defaults_no_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_env();
        std::env::set_var("BEAVA_WAL_BUFFERS", "99999");

        let cfg = WalConfig::resolve(WalConfigOverrides::default());
        clear_env();

        assert_eq!(
            cfg.buffers,
            WalConfig::DEFAULT_BUFFERS,
            "None override must return DEFAULT_BUFFERS, env IGNORED"
        );
        assert_eq!(cfg.buffer_size_mb, WalConfig::DEFAULT_BUFFER_SIZE_MB);
        assert_eq!(cfg.tick_ms, WalConfig::DEFAULT_TICK_MS);
    }
}
