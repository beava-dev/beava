//! Beava v2 server entry point.
//!
//! ServerV18 (mio) is the sole data-plane runtime per the mio-only invariant.
//! Admin endpoints mount via `BoundAdminServer` on `cfg.admin_addr` (default
//! 127.0.0.1:8090).

use anyhow::{Context, Result};
use beava_persistence::{Persistence, SyncMode};
use beava_server::server::ServerV18Config;
use beava_server::{banner, cli::Cli, logging, shutdown::shutdown_signal, ServerV18};
use clap::Parser;
use std::path::PathBuf;

/// Resolve the config from CLI + filesystem + defaults.
///
/// Precedence (highest first):
///   1. `--config <path>` — explicit; fail if missing.
///   2. `./beava.yaml` exists in cwd — load implicitly.
///   3. Built-in defaults + `BEAVA_*` env-var overrides.
///
/// Returns `(cfg, source_label)` where `source_label` describes where the
/// config came from for the boot banner.
fn resolve_config(
    explicit: Option<&PathBuf>,
) -> Result<(beava_server::Config, String), beava_server::ConfigError> {
    use beava_server::config::{defaults_with_env_overrides, load_config};
    let implicit = PathBuf::from("./beava.yaml");
    if let Some(path) = explicit {
        let cfg = load_config(path)?;
        Ok((cfg, format!("--config {}", path.display())))
    } else if implicit.exists() {
        let cfg = load_config(&implicit)?;
        Ok((cfg, "./beava.yaml".to_string()))
    } else {
        let cfg = defaults_with_env_overrides()?;
        Ok((cfg, "built-in defaults + BEAVA_* env".to_string()))
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let (cfg, source_label) =
        resolve_config(cli.config.as_ref()).with_context(|| match cli.config.as_ref() {
            Some(p) => format!("loading config from {}", p.display()),
            None => "loading config (built-in defaults + ./beava.yaml + BEAVA_* env)".to_string(),
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
        sv18_cfg.persistence = Persistence::Disk {
            wal_dir: cfg.durability.wal_dir.clone(),
            snapshot_dir: cfg.durability.snapshot_dir.clone(),
            sync_mode: SyncMode::Periodic,
        };
        sv18_cfg.tcp_max_frame_bytes = cfg.tcp.max_frame_bytes;
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
