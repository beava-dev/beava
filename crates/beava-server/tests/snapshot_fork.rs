//! Integration tests for the fork()+COW snapshot path.
//!
//! These tests verify the contract documented in
//! `crates/beava-server/src/snapshot_fork.rs`:
//!
//! 1. `do_snapshot_via_fork` produces a snapshot file decodable by the
//!    existing `SnapshotReader` (byte-identical schema to the in-process
//!    path).
//! 2. The child path does not corrupt parent state (parent can continue
//!    using `app_state` after the fork without crashing).
//! 3. The fork path serializes real registered aggregation state.
//!
//! NOTE on lock-hold timing: a microbenchmark proving "lock held < 10ms"
//! is intentionally NOT included here — it's timing-sensitive and would
//! flake in CI. The qualitative claim is locked in by inspection of the
//! `snapshot_fork.rs` source (the lock guard scope wraps only the fork
//! syscall) and the parent-state-after-fork test below (which would fail
//! if the parent were blocked on a long lock-hold).

use beava_core::agg_descriptor::{AggregationDescriptor, NamedAggOp};
use beava_core::agg_op::{AggKind, AggOp, AggOpDescriptor, FIELD_IDX_NONE};
use beava_core::agg_state::CountState;
use beava_core::agg_state_table::{ensure_capacity_for, AggStateTable, EntityKey};
use beava_core::op_node::{AggSpec, OpNode};
use beava_core::registry::{DerivationDescriptor, EventDescriptor, OutputKind, Registry};
use beava_core::registry_diff::PayloadNode;
use beava_core::row::Value;
use beava_core::schema::{DerivedSchema, EventSchema, FieldType};
use beava_core::snapshot_body::SnapshotBody;
use beava_persistence::SnapshotReader;
use beava_server::registry_debug::DevAggState;
use beava_server::snapshot_fork::{do_snapshot_via_fork, ChildExit};
use beava_server::AppState;
use compact_str::CompactString;
use smallvec::smallvec;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tempfile::TempDir;

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

/// Build a minimal `AppState` populated with N entities × 1 Count aggregation.
fn build_app_state(n_entities: usize) -> AppState {
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

    // Build a no-op WalSink for this test — the snapshot path doesn't need
    // WAL durability.
    let (wal_sink, _wal_join) = beava_persistence::WalSink::spawn_no_op();

    let idem_cache = Arc::new(beava_server::idem_cache::IdemCache::new());
    AppState::new(dev_agg, wal_sink, idem_cache)
}

// NOTE: env-gate test was previously here but violated the Phase 13.5.3
// architectural rule (`phase13_5_3_no_env_var_pokes_in_tests`) — process-env
// pokes pollute parallel test runs. Production reads `BEAVA_SNAPSHOT_FORK`
// once at boot in `server.rs` via `snapshot_fork::fork_enabled()`; tests
// drive the fork path by calling `do_snapshot_via_fork` directly (below)
// or by setting `SnapshotTaskConfig.use_fork_snapshot = true` (see
// `tests/snapshot_conditional.rs`).

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn fork_snapshot_writes_decodable_file() {
    let tmp = TempDir::new().unwrap();
    let app_state = build_app_state(100);

    let exit = do_snapshot_via_fork(tmp.path(), 42, &app_state)
        .await
        .expect("fork-snapshot must not error");

    match exit {
        ChildExit::Success { .. } => {}
        ChildExit::Failure { code, message } => {
            panic!("child failed: code={code} message={message}");
        }
    }

    // File should exist with the expected name.
    let path = tmp.path().join(format!("snapshot-{:016x}.bvs", 42u64));
    assert!(path.exists(), "snapshot file must exist at {path:?}");

    // And decode cleanly.
    let (header, body) = SnapshotReader::open(&path).expect("snapshot must decode");
    assert_eq!(header.snapshot_lsn, 42);
    // body_len must match the actual body bytes count.
    assert_eq!(header.body_len as usize, body.len());
    // SnapshotBody must decode and contain the registered aggregation state.
    let decoded = SnapshotBody::decode(&body).expect("body must decode");
    let entries = decoded
        .state_tables
        .get("UserCounts")
        .expect("registered aggregation state must be serialized");
    assert_eq!(entries.len(), 100);
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn fork_snapshot_parent_state_intact() {
    let tmp = TempDir::new().unwrap();
    let app_state = build_app_state(50);
    let pre_event_id = app_state.dev_agg.next_event_id.load(Ordering::Relaxed);

    let _ = do_snapshot_via_fork(tmp.path(), 1, &app_state)
        .await
        .expect("fork-snapshot must not error");

    // Parent must still be able to use app_state after fork.
    let post_event_id = app_state.dev_agg.next_event_id.load(Ordering::Relaxed);
    assert_eq!(pre_event_id, post_event_id);

    // The state_tables Mutex must still be lockable in the parent — the fork
    // only briefly held it across the syscall and dropped immediately.
    let _guard = app_state.dev_agg.state_tables.lock();
    // If we got the lock without deadlock, the parent is healthy.
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn fork_snapshot_with_zero_state() {
    // Edge case: snapshot an empty state. Must still produce a decodable file.
    let tmp = TempDir::new().unwrap();
    let app_state = build_app_state(0);

    let exit = do_snapshot_via_fork(tmp.path(), 7, &app_state)
        .await
        .unwrap();
    assert!(matches!(exit, ChildExit::Success { .. }));

    let path = tmp.path().join(format!("snapshot-{:016x}.bvs", 7u64));
    let (header, body) = SnapshotReader::open(&path).expect("zero-state snapshot must decode");
    assert_eq!(header.snapshot_lsn, 7);
    let decoded = SnapshotBody::decode(&body).expect("zero-state body must decode");
    assert_eq!(
        decoded.state_tables["UserCounts"].len(),
        0,
        "registered aggregation with zero entities should serialize as an empty table"
    );
}

// Suppress unused-import warning in non-unix builds.
#[cfg(not(unix))]
fn _force_uses() {
    let _: Option<&AppState> = None;
}
