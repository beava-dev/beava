//! Periodic snapshot task: captures the highest applied WAL watermark, captures
//! live registry + state tables, encodes outside the apply lock, atomic-renames
//! into the snapshot dir, then prunes/reclaims WAL state covered by the
//! snapshot LSN and prunes old snapshots. A manual-trigger channel lets tests
//! force an immediate snapshot via `TestServer::force_snapshot_now`.

use crate::AppState;
use beava_core::snapshot_body::SnapshotBody;
use beava_persistence::{
    prune_old_snapshots, PersistError, SnapshotWriteStats, SnapshotWriter, WalSink,
};
use beava_runtime_core::wal_writer::WalReclaimHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Snapshot task configuration. Mirrors `DurabilityConfig`'s snapshot fields.
#[derive(Debug, Clone)]
pub struct SnapshotTaskConfig {
    pub interval: Duration,
    pub snapshot_dir: PathBuf,
    pub retain: usize,
    /// Minimum number of WAL events written since the previous successful
    /// snapshot that must have accumulated before the next interval tick
    /// fires a snapshot. `0` (default) preserves the legacy "always snapshot
    /// on every tick" behavior; any value > 0 enables Redis-style
    /// conditional snapshotting — idle minutes don't write a snapshot.
    ///
    /// Computed as `current_wal_lsn - last_snapshot_lsn`. The check is
    /// applied to interval ticks only; a manual `force_snapshot_now`
    /// trigger always runs regardless of threshold.
    ///
    /// Wired from env `BEAVA_SNAPSHOT_MIN_EVENTS`.
    pub min_events_per_snapshot: u64,
    /// Whether to use the fork+COW snapshot path (drops apply-thread
    /// lock-hold from seconds to microseconds). Resolved once at boot in
    /// `server.rs` from `BEAVA_SNAPSHOT_FORK=1`; tests construct
    /// `SnapshotTaskConfig` with this field set directly to avoid
    /// process-env pollution (per the Phase 13.5.3 architectural rule
    /// in `phase13_5_3_no_env_var_pokes_in_tests`).
    pub use_fork_snapshot: bool,
    /// Test-only synchronization hook: receives the LSN captured for an
    /// in-process snapshot immediately before `state_tables.lock()`.
    #[doc(hidden)]
    pub snapshot_lsn_capture_tx: Option<std::sync::mpsc::Sender<u64>>,
}

/// Read `BEAVA_SNAPSHOT_MIN_EVENTS` as a u64. Returns `0` if the env is
/// unset or unparseable (preserves legacy behavior).
pub fn min_events_from_env() -> u64 {
    std::env::var("BEAVA_SNAPSHOT_MIN_EVENTS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
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
    wal_reclaim: Option<WalReclaimHandle>,
    cancel: CancellationToken,
) -> (JoinHandle<()>, SnapshotTriggerTx) {
    let (trigger_tx, mut trigger_rx) = mpsc::channel::<oneshot::Sender<Result<(), String>>>(8);
    let join = tokio::spawn(async move {
        // Read last_snapshot_lsn BEFORE consuming the first interval tick.
        // The first tick is "immediate" (Tokio docs) but may still yield to
        // the runtime to set up the timer — yielding here would let
        // concurrent appends advance the LSN before we observe the
        // baseline, causing the first real tick to see `delta = 0` and
        // skip even though events DID accumulate.
        let mut last_snapshot_lsn: u64 = current_snapshot_lsn(&app_state, &wal_sink);

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
                    // Manual trigger always runs regardless of threshold.
                    // Tests + operators use this to force a snapshot.
                    let res = do_snapshot(&cfg, &app_state, &wal_sink, wal_reclaim.as_ref()).await;
                    let mapped = match res {
                        Ok(snapshot_lsn) => {
                            last_snapshot_lsn = snapshot_lsn;
                            Ok(())
                        }
                        Err(e) => Err(e.to_string()),
                    };
                    let _ = ack.send(mapped);
                }
                _ = iv.tick() => {
                    // Redis-style conditional skip. When
                    // `min_events_per_snapshot > 0`, an interval tick is a
                    // no-op if fewer than `min` WAL events have committed
                    // since the previous successful snapshot. This avoids
                    // the production write-amplification class where an
                    // idle beava still writes a multi-hundred-MB snapshot
                    // every 30-60 s. Default `0` preserves legacy behavior.
                    if cfg.min_events_per_snapshot > 0 {
                        let current_lsn = current_snapshot_lsn(&app_state, &wal_sink);
                        let delta = current_lsn.saturating_sub(last_snapshot_lsn);
                        if delta < cfg.min_events_per_snapshot {
                            tracing::debug!(
                                target: "beava.snapshot",
                                kind = "snapshot.skipped_below_threshold",
                                events_since_last = delta,
                                threshold = cfg.min_events_per_snapshot,
                                current_lsn,
                                last_snapshot_lsn,
                                "skipping snapshot — below event-count threshold"
                            );
                            continue;
                        }
                    }
                    match do_snapshot(&cfg, &app_state, &wal_sink, wal_reclaim.as_ref()).await {
                        Ok(snapshot_lsn) => {
                            last_snapshot_lsn = snapshot_lsn;
                        }
                        Err(e) => {
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
        }
    });
    (join, trigger_tx)
}

async fn do_snapshot(
    cfg: &SnapshotTaskConfig,
    app_state: &AppState,
    wal_sink: &WalSink,
    wal_reclaim: Option<&WalReclaimHandle>,
) -> Result<u64, SnapshotTaskError> {
    #[cfg(any(feature = "testing", test))]
    maybe_crash_at("before-snapshot");

    let snapshot_started = Instant::now();
    let legacy_snapshot_lsn = wal_sink.durable_lsn();

    // Dispatch on `BEAVA_SNAPSHOT_FORK=1` — the fork+COW path drops apply-
    // thread lock-hold from ~seconds to ~µs at the cost of a brief 2× memory
    // peak during the child's serialize+write window. See `snapshot_fork`
    // for the safety analysis. Default (env unset) is the legacy in-process
    // path below.
    if cfg.use_fork_snapshot {
        match crate::snapshot_fork::do_snapshot_via_fork(
            &cfg.snapshot_dir,
            legacy_snapshot_lsn,
            app_state,
        )
        .await
        {
            Ok(crate::snapshot_fork::ChildExit::Success {
                snapshot_lsn,
                write_stats,
            }) => {
                let mut legacy_segments_removed = 0;
                let mut handrolled_reclaim_requested = false;
                if snapshot_lsn > 0 {
                    legacy_segments_removed = wal_sink.truncate_up_to(snapshot_lsn).await?;
                    if let Some(reclaim) = wal_reclaim {
                        reclaim.request_reclaim_up_to(snapshot_lsn);
                        handrolled_reclaim_requested = true;
                    }
                }
                let removed = prune_old_snapshots(&cfg.snapshot_dir, cfg.retain)?;
                let registry_version = app_state.dev_agg.registry.version();
                let total_duration = snapshot_started.elapsed();
                let (snapshot_bytes, fsync_duration) =
                    snapshot_write_metrics(&cfg.snapshot_dir, snapshot_lsn, write_stats.as_ref());
                crate::snapshot_metrics::record_snapshot_success(
                    total_duration,
                    snapshot_bytes,
                    fsync_duration,
                );
                tracing::info!(
                    target: "beava.snapshot",
                    kind = "snapshot.written",
                    snapshot_lsn,
                    registry_version,
                    duration_ms = total_duration.as_secs_f64() * 1000.0,
                    bytes = snapshot_bytes,
                    fsync_ms = fsync_duration.as_secs_f64() * 1000.0,
                    retained = cfg.retain,
                    snapshots_removed = removed,
                    legacy_wal_segments_removed = legacy_segments_removed,
                    handrolled_wal_reclaim_requested = handrolled_reclaim_requested,
                    via = "fork",
                    "snapshot written via fork; covered WAL reclamation queued"
                );
                return Ok(snapshot_lsn);
            }
            Ok(crate::snapshot_fork::ChildExit::Failure { code, message }) => {
                return Err(SnapshotTaskError::Encode(format!(
                    "fork-snapshot child failed (code={code}): {message}"
                )));
            }
            Err(e) => {
                return Err(SnapshotTaskError::Encode(format!("fork-snapshot: {e}")));
            }
        }
    }

    // Legacy in-process path (default).
    let (snapshot_lsn, body) = {
        let captured_lsn =
            legacy_snapshot_lsn.max(app_state.dev_agg.next_event_id.load(Ordering::Acquire));
        if let Some(tx) = &cfg.snapshot_lsn_capture_tx {
            let _ = tx.send(captured_lsn);
        }
        let tables = app_state.dev_agg.state_tables.lock();
        let registry_snap = app_state.dev_agg.registry.snapshot();
        let next_event_id = app_state.dev_agg.next_event_id.load(Ordering::Acquire);
        let query_time_ms = app_state.dev_agg.query_time_ms.load(Ordering::Acquire) as i64;
        let snapshot_lsn = legacy_snapshot_lsn.max(next_event_id);
        let body = SnapshotBody::from_live(&registry_snap, &tables, next_event_id, query_time_ms);
        (snapshot_lsn, body)
    };
    let registry_version = body.registry.version;
    let encoded = body
        .encode()
        .map_err(|e| SnapshotTaskError::Encode(e.to_string()))?;

    #[cfg(any(feature = "testing", test))]
    maybe_crash_at("before-rename");

    let write_stats = SnapshotWriter::write_with_stats(
        &cfg.snapshot_dir,
        snapshot_lsn,
        registry_version,
        &encoded,
    )?;

    #[cfg(any(feature = "testing", test))]
    maybe_crash_at("after-rename-before-truncate");

    let mut legacy_segments_removed = 0;
    let mut handrolled_reclaim_requested = false;
    if snapshot_lsn > 0 {
        legacy_segments_removed = wal_sink.truncate_up_to(snapshot_lsn).await?;
        if let Some(reclaim) = wal_reclaim {
            reclaim.request_reclaim_up_to(snapshot_lsn);
            handrolled_reclaim_requested = true;
        }
    }

    let removed = prune_old_snapshots(&cfg.snapshot_dir, cfg.retain)?;
    let total_duration = snapshot_started.elapsed();
    let fsync_duration = write_stats.total_fsync_duration();
    crate::snapshot_metrics::record_snapshot_success(
        total_duration,
        write_stats.bytes,
        fsync_duration,
    );
    tracing::info!(
        target: "beava.snapshot",
        kind = "snapshot.written",
        snapshot_lsn,
        registry_version,
        duration_ms = total_duration.as_secs_f64() * 1000.0,
        bytes = write_stats.bytes,
        fsync_ms = fsync_duration.as_secs_f64() * 1000.0,
        retained = cfg.retain,
        snapshots_removed = removed,
        legacy_wal_segments_removed = legacy_segments_removed,
        handrolled_wal_reclaim_requested = handrolled_reclaim_requested,
        "snapshot written; covered WAL reclamation queued"
    );
    Ok(snapshot_lsn)
}

fn current_snapshot_lsn(app_state: &AppState, wal_sink: &WalSink) -> u64 {
    wal_sink
        .durable_lsn()
        .max(app_state.dev_agg.next_event_id.load(Ordering::Acquire))
}

fn snapshot_write_metrics(
    snapshot_dir: &Path,
    snapshot_lsn: u64,
    write_stats: Option<&SnapshotWriteStats>,
) -> (u64, Duration) {
    if let Some(stats) = write_stats {
        return (stats.bytes, stats.total_fsync_duration());
    }

    let bytes = snapshot_dir
        .join(format!(
            "snapshot-{snapshot_lsn:016x}.{}",
            beava_persistence::SNAPSHOT_EXT
        ))
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0);
    (bytes, Duration::ZERO)
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
