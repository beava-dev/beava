//! Beava v2 server entry point.
//!
//! ServerV18 (mio) is the sole data-plane runtime per the mio-only invariant.
//! Admin endpoints mount via `BoundAdminServer` on `cfg.admin_addr` (default
//! 127.0.0.1:8090).

use anyhow::{Context, Result};
use beava_persistence::{Persistence, SyncMode};
use beava_server::server::ServerV18Config;
use beava_server::{
    banner,
    cli::{Cli, Command},
    logging, quickstart,
    shutdown::shutdown_signal,
    ServerV18,
};
use clap::Parser;
use std::path::Path;

/// Resolve the config from CLI + defaults.
///
/// Precedence (highest first):
///   1. CLI flags (`--http-addr`, `--tcp-addr`, `--data-dir`, `--memory-only`).
///   2. `--config <path>` — explicit YAML; fail if missing.
///   3. Built-in defaults + `BEAVA_*` env-var overrides.
///
/// There is no implicit `./beava.yaml` lookup: a YAML config is only loaded
/// when the operator points at one with `-c` / `--config`. This avoids the
/// footgun where running `beava` from a directory that happens to contain
/// an unrelated `beava.yaml` silently bound to whatever that file said.
///
/// Returns `(cfg, source_label)` where `source_label` describes where the
/// config came from for the boot banner.
fn resolve_config(cli: &Cli) -> Result<(beava_server::Config, String), beava_server::ConfigError> {
    use beava_server::config::{defaults_with_env_overrides, load_config};
    let (mut cfg, source_label) = if let Some(path) = cli.config.as_ref() {
        let cfg = load_config(path)?;
        (cfg, format!("--config {}", path.display()))
    } else {
        let cfg = defaults_with_env_overrides()?;
        (cfg, "built-in defaults + BEAVA_* env".to_string())
    };
    let mut overrides: Vec<&'static str> = Vec::new();
    if let Some(addr) = cli.http_addr.as_deref() {
        cfg.listen_addr = addr.to_string();
        overrides.push("--http-addr");
    }
    if let Some(addr) = cli.tcp_addr.as_deref() {
        let parsed: std::net::SocketAddr =
            addr.parse()
                .map_err(|_| beava_server::ConfigError::Validation {
                    field: "--tcp-addr",
                    reason: format!("expected `host:port`, got {addr:?}"),
                })?;
        cfg.tcp.host = parsed.ip().to_string();
        cfg.tcp.port = parsed.port();
        cfg.tcp.enabled = true;
        overrides.push("--tcp-addr");
    }
    if let Some(dir) = cli.data_dir.as_ref() {
        // --data-dir collapses both WAL and snapshot dirs under one root.
        // The two underlying paths still ship distinct subdirs so the
        // recovery code can scan WAL files without confusing them with
        // snapshot blobs.
        cfg.durability.wal_dir = dir.join("wal");
        cfg.durability.snapshot_dir = dir.join("snapshots");
        overrides.push("--data-dir");
    }
    let label = if overrides.is_empty() {
        source_label
    } else {
        format!("{} + CLI [{}]", source_label, overrides.join(", "))
    };
    Ok((cfg, label))
}

/// Build a fresh `Persistence` value for the apply path. `--memory-only`
/// short-circuits to `Persistence::Memory` (no WAL writer, no snapshot,
/// no recovery); otherwise we honour the resolved YAML/env durability
/// dirs that `resolve_config` produced.
fn build_persistence(memory_only: bool, wal_dir: &Path, snapshot_dir: &Path) -> Persistence {
    if memory_only {
        Persistence::Memory
    } else {
        Persistence::Disk {
            wal_dir: wal_dir.to_path_buf(),
            snapshot_dir: snapshot_dir.to_path_buf(),
            sync_mode: SyncMode::Periodic,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Subcommand dispatch — when present, runs to completion and exits;
    // we never fall through to the server-boot path. The default
    // (no subcommand) keeps booting the server, preserving back-compat
    // for every existing `beava` / `beava -c foo.yaml` invocation.
    if let Some(cmd) = cli.command {
        return match cmd {
            Command::Quickstart { no_file } => quickstart::run(no_file),
        };
    }

    let (cfg, source_label) = resolve_config(&cli).with_context(|| match cli.config.as_ref() {
        Some(p) => format!("loading config from {}", p.display()),
        None => "loading config (built-in defaults + BEAVA_* env)".to_string(),
    })?;

    logging::init(&cfg.log_level).context("init logging")?;

    tracing::info!(
        target: "beava.server",
        version = beava_server::VERSION,
        source = %source_label,
        "beava starting"
    );
    // Banner goes to stdout so operators see a non-JSON "started" line even
    // when log output is structured.
    println!("{}", banner());

    // Plain-text resolved-config block. Goes to stdout so operators see
    // exactly which ports the server bound and where state will be
    // written, regardless of log filtering. Mirrors the structured INFO
    // events further down but in a copy-paste-friendly shape.
    println!("config source : {source_label}");
    println!("HTTP listen   : {}", cfg.listen_addr);
    println!("Admin listen  : {}", cfg.admin_addr);
    println!(
        "TCP listen    : {}:{} (enabled={})",
        cfg.tcp.host, cfg.tcp.port, cfg.tcp.enabled
    );
    println!("WAL dir       : {}", cfg.durability.wal_dir.display());
    println!("Snapshot dir  : {}", cfg.durability.snapshot_dir.display());
    println!("Log level     : {}", cfg.log_level);

    // Single-thread tokio: admin endpoints + serve orchestration only. The
    // apply loop is a std::thread spawned inside `serve_with_dirs` and must
    // not touch tokio (mio-only invariant).
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    rt.block_on(async move {
        let http_addr: std::net::SocketAddr = cfg
            .listen_addr
            .parse()
            .with_context(|| format!("parse listen_addr {:?}", cfg.listen_addr))?;
        let tcp_addr: std::net::SocketAddr =
            format!("{}:{}", cfg.tcp.host, cfg.tcp.port)
                .parse()
                .with_context(|| format!("parse tcp addr {}:{}", cfg.tcp.host, cfg.tcp.port))?;
        let admin_addr: std::net::SocketAddr = cfg
            .admin_addr
            .parse()
            .with_context(|| format!("parse admin_addr {:?}", cfg.admin_addr))?;

        tracing::debug!(
            target: "beava.server",
            kind = "server.boot.v18",
            http_addr = %http_addr,
            tcp_addr = %tcp_addr,
            admin_addr = %admin_addr,
            "booting ServerV18"
        );

        // Read BEAVA_* env once at boot via `from_env()`; resolved values flow
        // through `ServerV18Config` so the hot path never re-reads env.
        let mut sv18_cfg = ServerV18Config::from_env();
        sv18_cfg.persistence = build_persistence(
            cli.memory_only,
            &cfg.durability.wal_dir,
            &cfg.durability.snapshot_dir,
        );
        sv18_cfg.tcp_max_frame_bytes = cfg.tcp.max_frame_bytes;
        // --test-mode CLI flag wins over BEAVA_TEST_MODE env (which
        // ServerV18Config::from_env already resolved). This matters when
        // an operator wants /reset on a single boot but not env-wide.
        if cli.test_mode {
            sv18_cfg.test_mode = true;
        }
        let server = ServerV18::bind_with_config(http_addr, Some(tcp_addr), admin_addr, sv18_cfg)
            .await
            .context("bind ServerV18 listeners")?;
        server
            .serve(shutdown_signal())
            .await
            .context("serve ServerV18")?;
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
