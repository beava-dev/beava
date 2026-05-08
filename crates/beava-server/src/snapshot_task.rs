//! Periodic snapshot task: captures `wal_sink.durable_lsn()`, encodes the
//! live registry + state tables outside the lock, atomic-renames into the
//! snapshot dir, then truncates the WAL up to the snapshot LSN and prunes
//! old snapshots. A manual-trigger channel lets tests force an immediate
//! snapshot via `TestServer::force_snapshot_now`.

use crate::AppState;
use beava_core::snapshot_body::SnapshotBody;
use beava_persistence::{prune_old_snapshots, PersistError, SnapshotWriter, WalSink};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Snapshot task configuration. Mirrors `DurabilityConfig`'s snapshot fields.
#[derive(Debug, Clone)]
pub struct SnapshotTaskConfig {
    pub interval: Duration,
    pub snapshot_dir: PathBuf,
    pub retain: usize,
}

/// Trigger channel sender for `force_snapshot_now`.
pub type SnapshotTriggerTx = mpsc::Sender<oneshot::Sender<Result<(), String>>>;

#[derive(Debug, thiserror::Error)]
pub enum SnapshotTaskError {
    #[error("encode: {0}")]
    Encode(String),
    #[error("persist: {0}")]
    Persist(#[from] PersistError),
}

/// Spawn the periodic snapshot task. Returns the JoinHandle + the
/// manual-trigger sender (gated for test usage; production callers can ignore).
pub fn spawn_snapshot_task(
    cfg: SnapshotTaskConfig,
    app_state: Arc<AppState>,
    wal_sink: WalSink,
    cancel: CancellationToken,
) -> (JoinHandle<()>, SnapshotTriggerTx) {
    let (trigger_tx, mut trigger_rx) = mpsc::channel::<oneshot::Sender<Result<(), String>>>(8);
    let join = tokio::spawn(async move {
        let mut iv = tokio::time::interval(cfg.interval);
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // First interval tick fires immediately; skip it so boot doesn't
        // race a snapshot before the WAL has any records.
        iv.tick().await;
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    tracing::debug!(
                        target: "beava.snapshot",
                        kind = "snapshot.task_exit",
                        "snapshot task cancelled"
                    );
                    return;
                }
                Some(ack) = trigger_rx.recv() => {
                    let res = do_snapshot(&cfg, &app_state, &wal_sink).await;
                    let mapped = res.map_err(|e| e.to_string());
                    let _ = ack.send(mapped);
                }
                _ = iv.tick() => {
                    if let Err(e) = do_snapshot(&cfg, &app_state, &wal_sink).await {
                        tracing::warn!(
                            target: "beava.snapshot",
                            kind = "snapshot.tick_failed",
                            error = %e,
                            "scheduled snapshot failed"
                        );
                    }
                }
            }
        }
    });
    (join, trigger_tx)
}

async fn do_snapshot(
    cfg: &SnapshotTaskConfig,
    app_state: &AppState,
    wal_sink: &WalSink,
) -> Result<(), SnapshotTaskError> {
    #[cfg(any(feature = "testing", test))]
    maybe_crash_at("before-snapshot");

    // Capture `durable_lsn` FIRST so `snapshot_lsn ≤ actual covered state`.
    // Any WAL record past this LSN is safely re-applied on restart — Event
    // records are idempotent through `apply_event_to_aggregations`,
    // RegistryBump records are additive.
    let snapshot_lsn = wal_sink.durable_lsn();
    let next_event_id = app_state.dev_agg.next_event_id.load(Ordering::Relaxed);
    let query_time_ms = app_state.dev_agg.query_time_ms.load(Ordering::Relaxed) as i64;

    let body = {
        let registry_snap = app_state.dev_agg.registry.snapshot();
        let tables = app_state.dev_agg.state_tables.lock();
        SnapshotBody::from_live(&registry_snap, &tables, next_event_id, query_time_ms)
    };
    let registry_version = body.registry.version;
    let encoded = body
        .encode()
        .map_err(|e| SnapshotTaskError::Encode(e.to_string()))?;

    #[cfg(any(feature = "testing", test))]
    maybe_crash_at("before-rename");

    SnapshotWriter::write(&cfg.snapshot_dir, snapshot_lsn, registry_version, &encoded)?;

    #[cfg(any(feature = "testing", test))]
    maybe_crash_at("after-rename-before-truncate");

    if snapshot_lsn > 0 {
        wal_sink.truncate_up_to(snapshot_lsn).await?;
    }

    let removed = prune_old_snapshots(&cfg.snapshot_dir, cfg.retain)?;
    tracing::info!(
        target: "beava.snapshot",
        kind = "snapshot.written",
        snapshot_lsn,
        registry_version,
        retained = cfg.retain,
        removed,
        "snapshot written + WAL truncated + old snapshots pruned"
    );
    Ok(())
}

#[cfg(any(feature = "testing", test))]
fn maybe_crash_at(point: &str) {
    if let Ok(env) = std::env::var("BEAVA_CRASH_AT") {
        if env == point {
            tracing::error!(
                target: "beava.snapshot",
                kind = "snapshot.crash_inject",
                at = point,
                "BEAVA_CRASH_AT triggered — aborting process"
            );
            std::process::abort();
        }
    }
}
