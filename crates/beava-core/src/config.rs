//! Server configuration.
//!
//! Loading order (later sources override earlier):
//! 1. Defaults baked into `Config::default()`
//! 2. YAML file at the path passed to `load_config`
//! 3. Environment variables with `BEAVA_*` prefix

use crate::defaults::{DEFAULT_TCP_HOST, DEFAULT_TCP_MAX_FRAME_BYTES, DEFAULT_TCP_PORT};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address:port to bind the HTTP server to (e.g. "127.0.0.1:8080").
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    /// Log level for tracing filter (trace|debug|info|warn|error).
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// TCP wire listener config. Enabled by default alongside HTTP.
    #[serde(default)]
    pub tcp: TcpConfig,
    /// WAL + idempotency cache knobs.
    #[serde(default)]
    pub durability: DurabilityConfig,
    /// Admin endpoint address (for /metrics, /registry, /debug).
    /// The data-plane (HTTP /push, /get; TCP) binds to `listen_addr` and
    /// `tcp.host:tcp.port`; the admin plane binds separately so prometheus
    /// scrapers and ops dashboards don't share a port with high-throughput
    /// data traffic. Env override: `BEAVA_ADMIN_ADDR`.
    #[serde(default = "default_admin_addr")]
    pub admin_addr: String,
}

fn default_listen_addr() -> String {
    "127.0.0.1:8080".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_admin_addr() -> String {
    "127.0.0.1:8090".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: default_listen_addr(),
            log_level: default_log_level(),
            tcp: TcpConfig::default(),
            durability: DurabilityConfig::default(),
            admin_addr: default_admin_addr(),
        }
    }
}

/// Durability + idempotency-cache tuning. All fields env-override via
/// `BEAVA_WAL_DIR`, `BEAVA_WAL_FSYNC_INTERVAL_MS`, `BEAVA_WAL_FSYNC_BYTES`,
/// `BEAVA_WAL_SEGMENT_BYTES`, and `BEAVA_DEDUPE_SWEEP_SECS`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DurabilityConfig {
    #[serde(default = "default_wal_dir")]
    pub wal_dir: PathBuf,
    #[serde(default = "default_wal_fsync_interval_ms")]
    pub wal_fsync_interval_ms: u64,
    #[serde(default = "default_wal_fsync_bytes")]
    pub wal_fsync_bytes: u64,
    #[serde(default = "default_wal_segment_bytes")]
    pub wal_segment_bytes: u64,
    #[serde(default = "default_dedupe_sweep_interval_secs")]
    pub dedupe_sweep_interval_secs: u64,
    /// Directory where snapshot files are written.
    #[serde(default = "default_snapshot_dir")]
    pub snapshot_dir: PathBuf,
    /// Periodic snapshot cadence in milliseconds (default 30_000).
    #[serde(default = "default_snapshot_interval_ms")]
    pub snapshot_interval_ms: u64,
    /// Number of snapshots to retain after pruning (default 2).
    #[serde(default = "default_snapshot_retain_count")]
    pub snapshot_retain_count: usize,
    /// WAL sync semantics for the default `/push` endpoint.
    /// `Periodic` (default) ACKs after in-memory append (Kafka acks=1);
    /// `PerEvent` ACKs after fsync (Kafka acks=all). The `/push-sync`
    /// endpoint always uses PerEvent regardless of this value.
    /// Env override: `BEAVA_WAL_SYNC_MODE=periodic|per-event`.
    #[serde(default = "default_wal_sync_mode")]
    pub wal_sync_mode: WalSyncMode,
}

/// Serializable mirror of `beava_persistence::SyncMode`. The enum is
/// duplicated here so beava-core (WASM-portable) doesn't need to depend on
/// beava-persistence (syscall-bearing). The conversion lives in the server
/// crate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WalSyncMode {
    #[default]
    Periodic,
    PerEvent,
}

fn default_wal_sync_mode() -> WalSyncMode {
    WalSyncMode::Periodic
}

fn default_wal_dir() -> PathBuf {
    PathBuf::from("./beava-wal")
}
fn default_wal_fsync_interval_ms() -> u64 {
    2
}
fn default_wal_fsync_bytes() -> u64 {
    1 << 20 // 1 MiB
}
fn default_wal_segment_bytes() -> u64 {
    128 << 20 // 128 MiB
}
fn default_dedupe_sweep_interval_secs() -> u64 {
    60
}
fn default_snapshot_dir() -> PathBuf {
    PathBuf::from("./beava-snapshots")
}
fn default_snapshot_interval_ms() -> u64 {
    30_000
}
fn default_snapshot_retain_count() -> usize {
    2
}

impl Default for DurabilityConfig {
    fn default() -> Self {
        Self {
            wal_dir: default_wal_dir(),
            wal_fsync_interval_ms: default_wal_fsync_interval_ms(),
            wal_fsync_bytes: default_wal_fsync_bytes(),
            wal_segment_bytes: default_wal_segment_bytes(),
            dedupe_sweep_interval_secs: default_dedupe_sweep_interval_secs(),
            snapshot_dir: default_snapshot_dir(),
            snapshot_interval_ms: default_snapshot_interval_ms(),
            snapshot_retain_count: default_snapshot_retain_count(),
            wal_sync_mode: default_wal_sync_mode(),
        }
    }
}

/// TCP binary-framed wire listener configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TcpConfig {
    #[serde(default = "default_tcp_enabled")]
    pub enabled: bool,
    #[serde(default = "default_tcp_host")]
    pub host: String,
    #[serde(default = "default_tcp_port")]
    pub port: u16,
    #[serde(default = "default_tcp_max_frame_bytes")]
    pub max_frame_bytes: u32,
}

fn default_tcp_enabled() -> bool {
    true
}
fn default_tcp_host() -> String {
    DEFAULT_TCP_HOST.to_string()
}
fn default_tcp_port() -> u16 {
    DEFAULT_TCP_PORT
}
fn default_tcp_max_frame_bytes() -> u32 {
    DEFAULT_TCP_MAX_FRAME_BYTES
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            enabled: default_tcp_enabled(),
            host: default_tcp_host(),
            port: default_tcp_port(),
            max_frame_bytes: default_tcp_max_frame_bytes(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("failed to read config file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse YAML config {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("invalid config field `{field}`: {reason}")]
    Validation { field: &'static str, reason: String },
}

/// Load a Config from the given YAML file path, apply env-var overrides, then validate.
///
/// Env-var overrides follow the `BEAVA_*` prefix convention.
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config, ConfigError> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(ConfigError::FileNotFound(path.to_path_buf()));
    }
    let raw = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut cfg: Config = serde_yaml::from_str(&raw).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        source: e,
    })?;

    apply_env_overrides(&mut cfg)?;
    validate(&cfg)?;
    Ok(cfg)
}

/// Build a `Config` from `Config::default()` + `BEAVA_*` env-var overrides
/// + validation, **without** requiring a YAML file on disk.
///
/// Used by the `beava` binary when invoked without `-c`/`--config` and no
/// `./beava.yaml` exists — operators can still override port / WAL paths
/// via `BEAVA_LISTEN_ADDR`, `BEAVA_ADMIN_ADDR`, `BEAVA_WAL_DIR`, etc., but
/// they don't have to author a YAML file just to start the server.
pub fn defaults_with_env_overrides() -> Result<Config, ConfigError> {
    let mut cfg = Config::default();
    apply_env_overrides(&mut cfg)?;
    validate(&cfg)?;
    Ok(cfg)
}

fn apply_env_overrides(cfg: &mut Config) -> Result<(), ConfigError> {
    if let Ok(v) = std::env::var("BEAVA_LISTEN_ADDR") {
        cfg.listen_addr = v;
    }
    if let Ok(v) = std::env::var("BEAVA_LOG_LEVEL") {
        cfg.log_level = v;
    }
    if let Ok(v) = std::env::var("BEAVA_TCP_ENABLED") {
        // Accept "0"/"1"/"true"/"false" (case-insensitive).
        cfg.tcp.enabled = matches!(v.to_ascii_lowercase().as_str(), "1" | "true");
    }
    if let Ok(v) = std::env::var("BEAVA_TCP_HOST") {
        cfg.tcp.host = v;
    }
    if let Ok(v) = std::env::var("BEAVA_TCP_PORT") {
        cfg.tcp.port = v
            .parse()
            .map_err(|e: std::num::ParseIntError| ConfigError::Validation {
                field: "tcp.port",
                reason: format!("BEAVA_TCP_PORT=`{}`: {}", v, e),
            })?;
    }
    if let Ok(v) = std::env::var("BEAVA_TCP_MAX_FRAME_BYTES") {
        cfg.tcp.max_frame_bytes =
            v.parse()
                .map_err(|e: std::num::ParseIntError| ConfigError::Validation {
                    field: "tcp.max_frame_bytes",
                    reason: format!("BEAVA_TCP_MAX_FRAME_BYTES=`{}`: {}", v, e),
                })?;
    }
    if let Ok(v) = std::env::var("BEAVA_WAL_DIR") {
        cfg.durability.wal_dir = PathBuf::from(v);
    }
    if let Ok(v) = std::env::var("BEAVA_WAL_FSYNC_INTERVAL_MS") {
        cfg.durability.wal_fsync_interval_ms =
            v.parse()
                .map_err(|e: std::num::ParseIntError| ConfigError::Validation {
                    field: "durability.wal_fsync_interval_ms",
                    reason: format!("BEAVA_WAL_FSYNC_INTERVAL_MS=`{}`: {}", v, e),
                })?;
    }
    if let Ok(v) = std::env::var("BEAVA_WAL_FSYNC_BYTES") {
        cfg.durability.wal_fsync_bytes =
            v.parse()
                .map_err(|e: std::num::ParseIntError| ConfigError::Validation {
                    field: "durability.wal_fsync_bytes",
                    reason: format!("BEAVA_WAL_FSYNC_BYTES=`{}`: {}", v, e),
                })?;
    }
    if let Ok(v) = std::env::var("BEAVA_WAL_SEGMENT_BYTES") {
        cfg.durability.wal_segment_bytes =
            v.parse()
                .map_err(|e: std::num::ParseIntError| ConfigError::Validation {
                    field: "durability.wal_segment_bytes",
                    reason: format!("BEAVA_WAL_SEGMENT_BYTES=`{}`: {}", v, e),
                })?;
    }
    if let Ok(v) = std::env::var("BEAVA_DEDUPE_SWEEP_SECS") {
        cfg.durability.dedupe_sweep_interval_secs =
            v.parse()
                .map_err(|e: std::num::ParseIntError| ConfigError::Validation {
                    field: "durability.dedupe_sweep_interval_secs",
                    reason: format!("BEAVA_DEDUPE_SWEEP_SECS=`{}`: {}", v, e),
                })?;
    }
    if let Ok(v) = std::env::var("BEAVA_SNAPSHOT_DIR") {
        cfg.durability.snapshot_dir = PathBuf::from(v);
    }
    if let Ok(v) = std::env::var("BEAVA_SNAPSHOT_INTERVAL_MS") {
        cfg.durability.snapshot_interval_ms =
            v.parse()
                .map_err(|e: std::num::ParseIntError| ConfigError::Validation {
                    field: "durability.snapshot_interval_ms",
                    reason: format!("BEAVA_SNAPSHOT_INTERVAL_MS=`{}`: {}", v, e),
                })?;
    }
    if let Ok(v) = std::env::var("BEAVA_SNAPSHOT_RETAIN_COUNT") {
        cfg.durability.snapshot_retain_count =
            v.parse()
                .map_err(|e: std::num::ParseIntError| ConfigError::Validation {
                    field: "durability.snapshot_retain_count",
                    reason: format!("BEAVA_SNAPSHOT_RETAIN_COUNT=`{}`: {}", v, e),
                })?;
    }
    if let Ok(v) = std::env::var("BEAVA_WAL_SYNC_MODE") {
        cfg.durability.wal_sync_mode = match v.to_ascii_lowercase().as_str() {
            "periodic" => WalSyncMode::Periodic,
            "per-event" | "perevent" | "per_event" => WalSyncMode::PerEvent,
            other => {
                return Err(ConfigError::Validation {
                    field: "durability.wal_sync_mode",
                    reason: format!(
                        "BEAVA_WAL_SYNC_MODE=`{}` (expected `periodic` or `per-event`)",
                        other
                    ),
                })
            }
        };
    }
    if let Ok(v) = std::env::var("BEAVA_ADMIN_ADDR") {
        cfg.admin_addr = v;
    }
    Ok(())
}

fn validate(cfg: &Config) -> Result<(), ConfigError> {
    cfg.listen_addr
        .parse::<std::net::SocketAddr>()
        .map_err(|e| ConfigError::Validation {
            field: "listen_addr",
            reason: format!("`{}` is not a valid socket address: {}", cfg.listen_addr, e),
        })?;

    // Validate admin_addr parses as a SocketAddr.
    cfg.admin_addr
        .parse::<std::net::SocketAddr>()
        .map_err(|e| ConfigError::Validation {
            field: "admin_addr",
            reason: format!("`{}` is not a valid socket address: {}", cfg.admin_addr, e),
        })?;

    match cfg.log_level.to_ascii_lowercase().as_str() {
        "trace" | "debug" | "info" | "warn" | "error" => Ok(()),
        other => Err(ConfigError::Validation {
            field: "log_level",
            reason: format!("`{}` is not one of: trace|debug|info|warn|error", other),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Mutex, MutexGuard};
    use tempfile::NamedTempFile;

    /// Process-global mutex serializing all env-var-touching tests.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn write_yaml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("create temp file");
        f.write_all(content.as_bytes()).expect("write temp");
        f
    }

    /// Helper to isolate env-var writes per test. Holds the global lock while live.
    struct EnvGuard<'a> {
        vars: Vec<(&'static str, Option<String>)>,
        _lock: MutexGuard<'a, ()>,
    }
    impl<'a> EnvGuard<'a> {
        fn set(keys: &[&'static str]) -> Self {
            let lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let vars = keys.iter().map(|&k| (k, std::env::var(k).ok())).collect();
            for &k in keys {
                std::env::remove_var(k);
            }
            EnvGuard { vars, _lock: lock }
        }
    }
    impl Drop for EnvGuard<'_> {
        fn drop(&mut self) {
            for (k, v) in &self.vars {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    #[test]
    fn parses_minimal_yaml() {
        let _guard = EnvGuard::set(&["BEAVA_LISTEN_ADDR", "BEAVA_LOG_LEVEL"]);
        let f = write_yaml("listen_addr: \"0.0.0.0:9000\"\nlog_level: debug\n");
        let cfg = load_config(f.path()).expect("load ok");
        assert_eq!(cfg.listen_addr, "0.0.0.0:9000");
        assert_eq!(cfg.log_level, "debug");
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let _guard = EnvGuard::set(&["BEAVA_LISTEN_ADDR", "BEAVA_LOG_LEVEL"]);
        let err = load_config("/nonexistent/path/to/beava.yaml").unwrap_err();
        assert!(matches!(err, ConfigError::FileNotFound(_)));
    }

    #[test]
    fn malformed_yaml_returns_parse_error() {
        let _guard = EnvGuard::set(&["BEAVA_LISTEN_ADDR", "BEAVA_LOG_LEVEL"]);
        let f = write_yaml("listen_addr: [not a string\n");
        let err = load_config(f.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn env_var_overrides_listen_addr() {
        let _guard = EnvGuard::set(&["BEAVA_LISTEN_ADDR", "BEAVA_LOG_LEVEL"]);
        std::env::set_var("BEAVA_LISTEN_ADDR", "127.0.0.1:9999");
        let f = write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\n");
        let cfg = load_config(f.path()).expect("load ok");
        assert_eq!(cfg.listen_addr, "127.0.0.1:9999");
    }

    #[test]
    fn env_var_overrides_log_level() {
        let _guard = EnvGuard::set(&["BEAVA_LISTEN_ADDR", "BEAVA_LOG_LEVEL"]);
        std::env::set_var("BEAVA_LOG_LEVEL", "trace");
        let f = write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\n");
        let cfg = load_config(f.path()).expect("load ok");
        assert_eq!(cfg.log_level, "trace");
    }

    #[test]
    fn invalid_listen_addr_fails_validation() {
        let _guard = EnvGuard::set(&["BEAVA_LISTEN_ADDR", "BEAVA_LOG_LEVEL"]);
        let f = write_yaml("listen_addr: \"not-a-socket-addr\"\nlog_level: info\n");
        let err = load_config(f.path()).unwrap_err();
        match err {
            ConfigError::Validation { field, .. } => assert_eq!(field, "listen_addr"),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn unknown_log_level_fails_validation() {
        let _guard = EnvGuard::set(&["BEAVA_LISTEN_ADDR", "BEAVA_LOG_LEVEL"]);
        let f = write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: shouty\n");
        let err = load_config(f.path()).unwrap_err();
        match err {
            ConfigError::Validation { field, .. } => assert_eq!(field, "log_level"),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    const TCP_ENV_KEYS: &[&str] = &[
        "BEAVA_LISTEN_ADDR",
        "BEAVA_LOG_LEVEL",
        "BEAVA_TCP_ENABLED",
        "BEAVA_TCP_HOST",
        "BEAVA_TCP_PORT",
        "BEAVA_TCP_MAX_FRAME_BYTES",
    ];

    #[test]
    fn tcp_config_default_matches_constants() {
        let t = TcpConfig::default();
        assert!(t.enabled);
        assert_eq!(t.host, "127.0.0.1");
        // Locked v0 wire surface (STATE.md surface-lock 2026-05-06):
        // HTTP on 8080, TCP on 8081. The pre-lock 7380 was a leftover
        // from the Phase 12 weather-station port convention.
        assert_eq!(t.port, 8081);
        assert_eq!(t.max_frame_bytes, 4 * 1024 * 1024);
    }

    #[test]
    fn config_default_includes_tcp_block() {
        let c = Config::default();
        assert_eq!(c.tcp, TcpConfig::default());
    }

    #[test]
    fn yaml_without_tcp_block_uses_defaults() {
        let _guard = EnvGuard::set(TCP_ENV_KEYS);
        let f = write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\n");
        let cfg = load_config(f.path()).expect("load ok");
        assert_eq!(cfg.tcp, TcpConfig::default());
    }

    #[test]
    fn yaml_with_partial_tcp_block_fills_defaults() {
        let _guard = EnvGuard::set(TCP_ENV_KEYS);
        let f =
            write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\ntcp:\n  port: 9999\n");
        let cfg = load_config(f.path()).expect("load ok");
        assert_eq!(cfg.tcp.port, 9999);
        assert!(cfg.tcp.enabled);
        assert_eq!(cfg.tcp.host, "127.0.0.1");
        assert_eq!(cfg.tcp.max_frame_bytes, 4 * 1024 * 1024);
    }

    #[test]
    fn yaml_with_full_tcp_block_round_trips() {
        let _guard = EnvGuard::set(TCP_ENV_KEYS);
        let f = write_yaml(
            "listen_addr: \"127.0.0.1:8080\"\n\
             log_level: info\n\
             tcp:\n  enabled: false\n  host: \"0.0.0.0\"\n  port: 1234\n  max_frame_bytes: 2048\n",
        );
        let cfg = load_config(f.path()).expect("load ok");
        assert!(!cfg.tcp.enabled);
        assert_eq!(cfg.tcp.host, "0.0.0.0");
        assert_eq!(cfg.tcp.port, 1234);
        assert_eq!(cfg.tcp.max_frame_bytes, 2048);
    }

    #[test]
    fn env_var_overrides_tcp_enabled_accepts_0_and_false() {
        let _guard = EnvGuard::set(TCP_ENV_KEYS);
        let f = write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\n");

        std::env::set_var("BEAVA_TCP_ENABLED", "0");
        assert!(!load_config(f.path()).unwrap().tcp.enabled);
        std::env::set_var("BEAVA_TCP_ENABLED", "false");
        assert!(!load_config(f.path()).unwrap().tcp.enabled);
        std::env::set_var("BEAVA_TCP_ENABLED", "FALSE");
        assert!(!load_config(f.path()).unwrap().tcp.enabled);
        std::env::set_var("BEAVA_TCP_ENABLED", "1");
        assert!(load_config(f.path()).unwrap().tcp.enabled);
        std::env::set_var("BEAVA_TCP_ENABLED", "true");
        assert!(load_config(f.path()).unwrap().tcp.enabled);
        std::env::set_var("BEAVA_TCP_ENABLED", "TRUE");
        assert!(load_config(f.path()).unwrap().tcp.enabled);
    }

    #[test]
    fn env_var_overrides_tcp_port() {
        let _guard = EnvGuard::set(TCP_ENV_KEYS);
        std::env::set_var("BEAVA_TCP_PORT", "9999");
        let f = write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\n");
        let cfg = load_config(f.path()).expect("load ok");
        assert_eq!(cfg.tcp.port, 9999);
    }

    #[test]
    fn env_var_overrides_tcp_host() {
        let _guard = EnvGuard::set(TCP_ENV_KEYS);
        std::env::set_var("BEAVA_TCP_HOST", "0.0.0.0");
        let f = write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\n");
        let cfg = load_config(f.path()).expect("load ok");
        assert_eq!(cfg.tcp.host, "0.0.0.0");
    }

    #[test]
    fn env_var_overrides_tcp_max_frame_bytes() {
        let _guard = EnvGuard::set(TCP_ENV_KEYS);
        std::env::set_var("BEAVA_TCP_MAX_FRAME_BYTES", "1048576");
        let f = write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\n");
        let cfg = load_config(f.path()).expect("load ok");
        assert_eq!(cfg.tcp.max_frame_bytes, 1_048_576);
    }

    #[test]
    fn env_var_invalid_tcp_port_returns_validation_error() {
        let _guard = EnvGuard::set(TCP_ENV_KEYS);
        std::env::set_var("BEAVA_TCP_PORT", "nope");
        let f = write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\n");
        let err = load_config(f.path()).unwrap_err();
        match err {
            ConfigError::Validation { field, .. } => assert_eq!(field, "tcp.port"),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn env_var_invalid_tcp_max_frame_bytes_returns_validation_error() {
        let _guard = EnvGuard::set(TCP_ENV_KEYS);
        std::env::set_var("BEAVA_TCP_MAX_FRAME_BYTES", "huge");
        let f = write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\n");
        let err = load_config(f.path()).unwrap_err();
        match err {
            ConfigError::Validation { field, .. } => assert_eq!(field, "tcp.max_frame_bytes"),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn deny_unknown_fields_in_tcp_block() {
        let _guard = EnvGuard::set(TCP_ENV_KEYS);
        let f =
            write_yaml("listen_addr: \"127.0.0.1:8080\"\nlog_level: info\ntcp:\n  typo_key: 1\n");
        let err = load_config(f.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }
}
