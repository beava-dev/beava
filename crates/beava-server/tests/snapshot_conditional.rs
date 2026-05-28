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
//! - `nonzero_threshold_uses_applied_data_plane_lsn`
//!   Production push traffic advances the applied data-plane watermark, not
//!   the legacy `WalSink` watermark.

use beava_core::agg_descriptor::{AggregationDescriptor, NamedAggOp};
use beava_core::agg_op::{AggKind, AggOp, AggOpDescriptor, FIELD_IDX_NONE};
use beava_core::agg_state::CountState;
use beava_core::agg_state_table::{ensure_capacity_for, AggStateTable, EntityKey};
use beava_core::op_node::{AggSpec, OpNode};
use beava_core::registry::{DerivationDescriptor, EventDescriptor, OutputKind, Registry};
use beava_core::registry_diff::PayloadNode;
use beava_core::row::Value;
use beava_core::schema::{DerivedSchema, EventSchema, FieldType};
use beava_persistence::{list_snapshots, WalSink};
use beava_server::idem_cache::IdemCache;
use beava_server::registry_debug::DevAggState;
use beava_server::snapshot_task::{spawn_snapshot_task, SnapshotTaskConfig};
use beava_server::AppState;
use compact_str::CompactString;
use smallvec::smallvec;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
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

fn count_desc() -> AggOpDescriptor {
    AggOpDescriptor {
        kind: AggKind::Count,
        field: None,
        window_ms: None,
        where_expr: None,
        n: None,
        half_life_ms: None,
        sub_window_ms: None,
        sigma: None,
        sketch_params: None,
        ext: Default::default(),
        field_idx: FIELD_IDX_NONE,
        field_idx_into_event_extracted: Vec::new(),
    }
}

fn build_registered_app_state(
    n_entities: usize,
) -> (AppState, WalSink, tokio::task::JoinHandle<()>) {
    let registry = Arc::new(Registry::new());

    let mut event_fields = BTreeMap::new();
    event_fields.insert("user_id".to_string(), FieldType::Str);
    let event = EventDescriptor {
        name: "Txn".to_string(),
        schema: EventSchema {
            fields: event_fields,
            optional_fields: vec![],
        },
        dedupe_key: None,
        dedupe_window_ms: None,
        keep_events_for_ms: None,
        cold_after_ms: None,
        registered_at_version: 0,
        name_arc: Arc::from(""),
        apply_field_names: vec![],
    };

    let mut group_by = BTreeMap::new();
    group_by.insert(
        "cnt".to_string(),
        AggSpec {
            op: "count".to_string(),
            params: serde_json::Value::Object(Default::default()),
        },
    );
    let mut derived_fields = BTreeMap::new();
    derived_fields.insert("user_id".to_string(), FieldType::Str);
    derived_fields.insert("cnt".to_string(), FieldType::I64);
    let deriv = DerivationDescriptor {
        name: "UserCounts".to_string(),
        output_kind: OutputKind::Table,
        upstreams: vec!["Txn".to_string()],
        ops: vec![OpNode::GroupBy {
            keys: vec!["user_id".to_string()],
            agg: group_by,
        }],
        schema: DerivedSchema {
            fields: derived_fields,
            optional_fields: vec![],
        },
        table_primary_key: Some(vec!["user_id".to_string()]),
        registered_at_version: 0,
    };
    let agg = AggregationDescriptor {
        node_name: "UserCounts".to_string(),
        source_node_name: "Txn".to_string(),
        group_keys: vec!["user_id".to_string()],
        features: vec![NamedAggOp {
            feature_name: "cnt".to_string(),
            descriptor: count_desc(),
        }],
        agg_id: 0,
        field_names: vec![],
        cluster_id: 0,
    };

    registry.apply_registration(
        vec![PayloadNode::Event(event), PayloadNode::Derivation(deriv)],
        vec![],
        vec![],
        vec![("UserCounts".to_string(), Arc::new(agg))],
    );

    let dev_agg = DevAggState::new(registry);
    {
        let mut tables = dev_agg.state_tables.lock();
        ensure_capacity_for(&mut tables, 1);
        let mut table = AggStateTable::new();
        for ent in 0..n_entities {
            let key_str = format!("user_{ent:09}");
            let entity_key = EntityKey(smallvec![(
                CompactString::from("user_id"),
                Value::Str(CompactString::from(key_str.as_str())),
            )]);
            table.insert_from_entity_key(
                entity_key,
                vec![AggOp::Count(CountState { n: ent as u64 })],
            );
        }
        tables[0] = table;
    }

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
        snapshot_lsn_capture_tx: None,
    };
    let cancel = CancellationToken::new();
    let (snap_join, _trigger) =
        spawn_snapshot_task(cfg, Arc::new(app_state), wal_sink, None, cancel.clone());

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
        snapshot_lsn_capture_tx: None,
    };
    let cancel = CancellationToken::new();
    let (snap_join, _trigger) =
        spawn_snapshot_task(cfg, Arc::new(app_state), wal_sink, None, cancel.clone());

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
        snapshot_lsn_capture_tx: None,
    };
    let cancel = CancellationToken::new();
    let app_state_arc = Arc::new(app_state);
    let (snap_join, _trigger) = spawn_snapshot_task(
        cfg,
        app_state_arc.clone(),
        wal_sink.clone(),
        None,
        cancel.clone(),
    );

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
async fn nonzero_threshold_uses_applied_data_plane_lsn() {
    let tmp = TempDir::new().unwrap();
    let (app_state, wal_sink, _wal_join) = build_registered_app_state(10);

    let cfg = SnapshotTaskConfig {
        interval: Duration::from_millis(TICK_MS),
        snapshot_dir: tmp.path().to_path_buf(),
        retain: 10,
        min_events_per_snapshot: 3,
        use_fork_snapshot: false,
        snapshot_lsn_capture_tx: None,
    };
    let cancel = CancellationToken::new();
    let app_state = Arc::new(app_state);
    let (snap_join, _trigger) =
        spawn_snapshot_task(cfg, Arc::clone(&app_state), wal_sink, None, cancel.clone());

    tokio::time::sleep(Duration::from_millis(TICK_MS / 2)).await;
    app_state.dev_agg.next_event_id.store(5, Ordering::Release);
    tokio::time::sleep(Duration::from_millis(TICK_MS * 4)).await;
    cancel.cancel();
    let _ = snap_join.await;

    let lsns: Vec<u64> = list_snapshots(tmp.path())
        .expect("list snapshots")
        .into_iter()
        .map(|(lsn, _)| lsn)
        .collect();
    assert!(
        lsns.contains(&5),
        "data-plane applied watermark should trigger snapshot at LSN 5; got {lsns:?}"
    );
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
        snapshot_lsn_capture_tx: None,
    };
    let cancel = CancellationToken::new();
    let (snap_join, trigger) =
        spawn_snapshot_task(cfg, Arc::new(app_state), wal_sink, None, cancel.clone());

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_baseline_stays_at_captured_lsn_when_wal_advances_during_write() {
    let tmp = TempDir::new().unwrap();
    let (app_state, wal_sink, _wal_join) = build_registered_app_state(10);

    for _ in 0..5 {
        wal_sink
            .append_event(b"before".to_vec())
            .await
            .expect("append before snapshot");
    }

    let (capture_tx, capture_rx) = std::sync::mpsc::channel();
    let cfg = SnapshotTaskConfig {
        interval: Duration::from_millis(TICK_MS),
        snapshot_dir: tmp.path().to_path_buf(),
        retain: 10,
        min_events_per_snapshot: 5,
        use_fork_snapshot: false,
        snapshot_lsn_capture_tx: Some(capture_tx),
    };
    let cancel = CancellationToken::new();
    let app_state = Arc::new(app_state);
    let (snap_join, trigger) = spawn_snapshot_task(
        cfg,
        Arc::clone(&app_state),
        wal_sink.clone(),
        None,
        cancel.clone(),
    );

    let tables = Arc::clone(&app_state.dev_agg.state_tables);
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let lock_thread = std::thread::spawn(move || {
        let _guard = tables.lock();
        let _ = locked_tx.send(());
        let _ = release_rx.recv();
    });
    locked_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("test lock holder acquired state_tables");

    let (ack_tx, ack_rx) = oneshot::channel();
    trigger.send(ack_tx).await.expect("trigger send");

    // Wait until the snapshot task has captured snapshot_lsn=5 and is about
    // to block on `state_tables.lock()`. It cannot serialize until this test
    // releases the lock.
    assert_eq!(
        capture_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("snapshot task did not capture LSN before lock"),
        5
    );

    for _ in 0..5 {
        wal_sink
            .append_event(b"during".to_vec())
            .await
            .expect("append during snapshot");
    }
    release_tx.send(()).expect("release test lock holder");

    let result = tokio::time::timeout(Duration::from_secs(5), ack_rx)
        .await
        .expect("manual snapshot ack timed out")
        .expect("ack");
    assert!(result.is_ok(), "manual snapshot should succeed: {result:?}");
    lock_thread.join().expect("lock holder thread");

    tokio::time::sleep(Duration::from_millis(TICK_MS * 3)).await;
    cancel.cancel();
    let _ = snap_join.await;

    let lsns: Vec<u64> = list_snapshots(tmp.path())
        .expect("list snapshots")
        .into_iter()
        .map(|(lsn, _)| lsn)
        .collect();
    assert!(
        lsns.contains(&5) && lsns.contains(&10),
        "expected snapshots at captured LSN 5 and later LSN 10; got {lsns:?}"
    );
}

// NOTE: env-parsing unit test was previously here but violated the Phase
// 13.5.3 architectural rule (`phase13_5_3_no_env_var_pokes_in_tests`).
// `BEAVA_SNAPSHOT_MIN_EVENTS` is read once at boot in `server.rs` via
// `snapshot_task::min_events_from_env()`; tests construct
// `SnapshotTaskConfig` with `min_events_per_snapshot` set directly (see
// the four tests above) rather than poking the global process env.
