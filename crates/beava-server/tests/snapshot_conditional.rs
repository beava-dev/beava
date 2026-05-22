//! Redis-style conditional snapshot tests.
//!
//! With `BEAVA_SNAPSHOT_MIN_EVENTS=N` (or `SnapshotTaskConfig
//! { min_events_per_snapshot: N, .. }`), an interval tick skips
//! snapshotting unless at least N WAL events have committed since the
//! previous successful snapshot. Mirrors Redis's `save N M` directive.
//!
//! Tests:
//! - `default_zero_threshold_always_snapshots_on_tick`
//!   When threshold is 0 (legacy default), every interval tick produces a
//!   snapshot, even with zero WAL activity.
//! - `nonzero_threshold_skips_when_below`
//!   With threshold > 0 and no WAL events, no snapshot file is produced.
//! - `nonzero_threshold_fires_when_met`
//!   Once enough events have been appended, the next tick produces a
//!   snapshot.
//! - `manual_trigger_bypasses_threshold`
//!   `force_snapshot_now` always runs regardless of threshold (operators
//!   and tests need this escape hatch).

use beava_core::registry::Registry;
use beava_persistence::{list_snapshots, WalSink};
use beava_server::idem_cache::IdemCache;
use beava_server::registry_debug::DevAggState;
use beava_server::snapshot_task::{spawn_snapshot_task, SnapshotTaskConfig};
use beava_server::AppState;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

fn build_app_state() -> (AppState, WalSink, tokio::task::JoinHandle<()>) {
    let registry = Arc::new(Registry::new());
    let dev_agg = DevAggState::new(registry);
    let (wal_sink, wal_join) = WalSink::spawn_no_op();
    let idem_cache = Arc::new(IdemCache::new());
    let app_state = AppState::new(dev_agg, wal_sink.clone(), idem_cache);
    (app_state, wal_sink, wal_join)
}

fn snapshot_count(dir: &std::path::Path) -> usize {
    list_snapshots(dir).map(|v| v.len()).unwrap_or(0)
}

/// Snapshot interval used by these tests — short so the test completes
/// quickly while still letting us observe 2-3 ticks.
const TICK_MS: u64 = 100;

#[tokio::test(flavor = "current_thread")]
async fn default_zero_threshold_always_snapshots_on_tick() {
    let tmp = TempDir::new().unwrap();
    let (app_state, wal_sink, _wal_join) = build_app_state();

    let cfg = SnapshotTaskConfig {
        interval: Duration::from_millis(TICK_MS),
        snapshot_dir: tmp.path().to_path_buf(),
        retain: 10,
        min_events_per_snapshot: 0, // legacy behavior
        use_fork_snapshot: false,
    };
    let cancel = CancellationToken::new();
    let (snap_join, _trigger) =
        spawn_snapshot_task(cfg, Arc::new(app_state), wal_sink, cancel.clone());

    // Wait for ~3 ticks. With threshold=0, each tick produces a snapshot.
    // Note: with no WAL activity, all snapshots write to the same LSN-named
    // file (`snapshot-{lsn:016x}.bvs`), so multiple ticks overwrite the
    // same file. We only assert >=1 — the contract is "every tick fires",
    // not "every tick produces a unique file".
    tokio::time::sleep(Duration::from_millis(TICK_MS * 4)).await;
    cancel.cancel();
    let _ = snap_join.await;

    let n = snapshot_count(tmp.path());
    assert!(
        n >= 1,
        "with threshold=0, at least one snapshot must be written — got {n}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn nonzero_threshold_skips_when_below() {
    let tmp = TempDir::new().unwrap();
    let (app_state, wal_sink, _wal_join) = build_app_state();

    let cfg = SnapshotTaskConfig {
        interval: Duration::from_millis(TICK_MS),
        snapshot_dir: tmp.path().to_path_buf(),
        retain: 10,
        min_events_per_snapshot: 1000, // anything > 0 events appended
        use_fork_snapshot: false,
    };
    let cancel = CancellationToken::new();
    let (snap_join, _trigger) =
        spawn_snapshot_task(cfg, Arc::new(app_state), wal_sink, cancel.clone());

    // Same wait as the previous test — but with threshold > 0 and zero
    // WAL appends, every tick should be skipped.
    tokio::time::sleep(Duration::from_millis(TICK_MS * 4)).await;
    cancel.cancel();
    let _ = snap_join.await;

    let n = snapshot_count(tmp.path());
    assert_eq!(
        n, 0,
        "with threshold=1000 and zero appends, no snapshot should be written — got {n}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn nonzero_threshold_fires_when_met() {
    let tmp = TempDir::new().unwrap();
    let (app_state, wal_sink, _wal_join) = build_app_state();

    let cfg = SnapshotTaskConfig {
        interval: Duration::from_millis(TICK_MS),
        snapshot_dir: tmp.path().to_path_buf(),
        retain: 10,
        // Low threshold so a handful of appends clears it.
        min_events_per_snapshot: 3,
        use_fork_snapshot: false,
    };
    let cancel = CancellationToken::new();
    let app_state_arc = Arc::new(app_state);
    let (snap_join, _trigger) =
        spawn_snapshot_task(cfg, app_state_arc.clone(), wal_sink.clone(), cancel.clone());

    // Append 5 events — clears the threshold of 3.
    for _ in 0..5 {
        wal_sink
            .append_event(b"{}".to_vec())
            .await
            .expect("append_event");
    }

    // Wait for ~3 ticks. At least one should fire.
    tokio::time::sleep(Duration::from_millis(TICK_MS * 4)).await;
    cancel.cancel();
    let _ = snap_join.await;

    let n = snapshot_count(tmp.path());
    assert!(
        n >= 1,
        "threshold=3 met by 5 appends — at least 1 snapshot expected, got {n}"
    );
    // After a snapshot fires, last_snapshot_lsn updates so further ticks
    // with no new appends should NOT fire. We don't strictly assert the
    // exact count (timing-sensitive) but the test above proves the skip
    // path works.
}

#[tokio::test(flavor = "current_thread")]
async fn manual_trigger_bypasses_threshold() {
    let tmp = TempDir::new().unwrap();
    let (app_state, wal_sink, _wal_join) = build_app_state();

    let cfg = SnapshotTaskConfig {
        // Long interval so the periodic tick effectively never fires in
        // the test window — we only exercise the manual trigger path.
        interval: Duration::from_secs(3600),
        snapshot_dir: tmp.path().to_path_buf(),
        retain: 10,
        // High threshold — would skip any interval tick even if it fired.
        min_events_per_snapshot: u64::MAX,
        use_fork_snapshot: false,
    };
    let cancel = CancellationToken::new();
    let (snap_join, trigger) =
        spawn_snapshot_task(cfg, Arc::new(app_state), wal_sink, cancel.clone());

    // Fire a manual trigger — should always run regardless of threshold.
    let (ack_tx, ack_rx) = oneshot::channel();
    trigger.send(ack_tx).await.expect("trigger send");
    let result = ack_rx.await.expect("ack");
    assert!(result.is_ok(), "manual snapshot should succeed: {result:?}");

    cancel.cancel();
    let _ = snap_join.await;

    let n = snapshot_count(tmp.path());
    assert!(
        n >= 1,
        "manual trigger should always produce a snapshot — got {n}"
    );
}

// NOTE: env-parsing unit test was previously here but violated the Phase
// 13.5.3 architectural rule (`phase13_5_3_no_env_var_pokes_in_tests`).
// `BEAVA_SNAPSHOT_MIN_EVENTS` is read once at boot in `server.rs` via
// `snapshot_task::min_events_from_env()`; tests construct
// `SnapshotTaskConfig` with `min_events_per_snapshot` set directly (see
// the four tests above) rather than poking the global process env.
