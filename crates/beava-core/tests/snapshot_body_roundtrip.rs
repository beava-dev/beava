//! Plan 07-02 red-then-green: bincode round-trip tests for SnapshotBody + AggOp serde.
//!
//! Contract: every AggOp variant, EntityKey, Value variant, and the top-level
//! SnapshotBody must survive an encode → decode round-trip byte-for-byte.
//!
//! Tests reference symbols (SnapshotBody, SnapshotBodyError, SNAPSHOT_BODY_FORMAT_VERSION)
//! that don't exist yet at red-commit time. Once Task 2b lands the serde derives
//! + snapshot_body.rs module, all tests pass.

use beava_core::agg_op::{AggKind, AggOp, AggOpDescriptor};
use beava_core::agg_state::{
    AvgState, CountState, MaxState, MinState, RatioState, SumState, VarianceState,
};
use beava_core::agg_state_table::{AggStateTable, EntityKey, StateTables};
use beava_core::agg_windowed::WindowedOp;
use beava_core::registry::{
    DerivationDescriptor, EventDescriptor, OutputKind, RegistryInner, TableDescriptor, TableMode,
};
use beava_core::row::{Row, Value};
use beava_core::schema::{DerivedSchema, EventSchema, FieldType, TableSchema};
use beava_core::snapshot_body::{
    RegistryDescriptorsOnly, SnapshotBody, SnapshotBodyError, SNAPSHOT_BODY_FORMAT_VERSION,
};
use std::collections::BTreeMap;

fn mk_count_op() -> AggOp {
    AggOp::new(&AggOpDescriptor {
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
        field_idx: beava_core::agg_op::FIELD_IDX_NONE,
        field_idx_into_event_extracted: Vec::new(),
    })
}

fn mk_sum_op() -> AggOp {
    AggOp::Sum(SumState::default())
}

fn row_amount(v: f64) -> Row {
    Row::new().with_field("amount", Value::F64(v))
}

// ─── Per-variant AggOp round-trips ─────────────────────────────────────────────

#[test]
fn aggop_count_serde_roundtrip() {
    let mut op = AggOp::Count(CountState::default());
    op.update(&Row::new(), 0, None, true);
    op.update(&Row::new(), 1, None, true);
    let bytes = bincode::serialize(&op).expect("encode count");
    let decoded: AggOp = bincode::deserialize(&bytes).expect("decode count");
    assert_eq!(decoded.query(0), Value::I64(2));
}

#[test]
fn aggop_sum_serde_roundtrip() {
    let mut op = AggOp::Sum(SumState::default());
    op.update(&row_amount(10.0), 0, Some("amount"), true);
    op.update(&row_amount(20.0), 1, Some("amount"), true);
    let bytes = bincode::serialize(&op).expect("encode sum");
    let decoded: AggOp = bincode::deserialize(&bytes).expect("decode sum");
    assert_eq!(decoded.query(0), Value::F64(30.0));
}

#[test]
fn aggop_avg_serde_roundtrip() {
    let mut op = AggOp::Avg(AvgState::default());
    op.update(&row_amount(4.0), 0, Some("amount"), true);
    op.update(&row_amount(6.0), 1, Some("amount"), true);
    let before = op.query(0);
    let bytes = bincode::serialize(&op).expect("encode avg");
    let decoded: AggOp = bincode::deserialize(&bytes).expect("decode avg");
    assert_eq!(decoded.query(0), before);
}

#[test]
fn aggop_min_serde_roundtrip() {
    let mut op = AggOp::Min(MinState::default());
    op.update(
        &Row::new().with_field("x", Value::I64(5)),
        0,
        Some("x"),
        true,
    );
    op.update(
        &Row::new().with_field("x", Value::I64(3)),
        1,
        Some("x"),
        true,
    );
    op.update(
        &Row::new().with_field("x", Value::I64(7)),
        2,
        Some("x"),
        true,
    );
    let bytes = bincode::serialize(&op).expect("encode min");
    let decoded: AggOp = bincode::deserialize(&bytes).expect("decode min");
    assert_eq!(decoded.query(0), Value::I64(3));
}

#[test]
fn aggop_max_serde_roundtrip() {
    let mut op = AggOp::Max(MaxState::default());
    for v in [5_i64, 3, 7] {
        op.update(
            &Row::new().with_field("x", Value::I64(v)),
            0,
            Some("x"),
            true,
        );
    }
    let bytes = bincode::serialize(&op).expect("encode max");
    let decoded: AggOp = bincode::deserialize(&bytes).expect("decode max");
    assert_eq!(decoded.query(0), Value::I64(7));
}

#[test]
fn aggop_variance_serde_roundtrip() {
    let mut op = AggOp::Variance(VarianceState::default());
    for v in [2.0_f64, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0] {
        op.update(&row_amount(v), 0, Some("amount"), true);
    }
    let before = match op.query(0) {
        Value::F64(v) => v.to_bits(),
        other => panic!("expected F64, got {:?}", other),
    };
    let bytes = bincode::serialize(&op).expect("encode variance");
    let decoded: AggOp = bincode::deserialize(&bytes).expect("decode variance");
    let after = match decoded.query(0) {
        Value::F64(v) => v.to_bits(),
        other => panic!("expected F64, got {:?}", other),
    };
    assert_eq!(before, after);
}

#[test]
fn aggop_stddev_serde_roundtrip() {
    let mut op = AggOp::StdDev(VarianceState::default());
    for v in [2.0_f64, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0] {
        op.update(&row_amount(v), 0, Some("amount"), true);
    }
    let before = match op.query(0) {
        Value::F64(v) => v.to_bits(),
        other => panic!("expected F64, got {:?}", other),
    };
    let bytes = bincode::serialize(&op).expect("encode stddev");
    let decoded: AggOp = bincode::deserialize(&bytes).expect("decode stddev");
    let after = match decoded.query(0) {
        Value::F64(v) => v.to_bits(),
        other => panic!("expected F64, got {:?}", other),
    };
    assert_eq!(before, after);
}

#[test]
fn aggop_ratio_serde_roundtrip() {
    let mut op = AggOp::Ratio(RatioState::default());
    for i in 0..10 {
        op.update(&Row::new(), i, None, i < 3);
    }
    let bytes = bincode::serialize(&op).expect("encode ratio");
    let decoded: AggOp = bincode::deserialize(&bytes).expect("decode ratio");
    match decoded.query(0) {
        Value::F64(v) => assert!((v - 0.3).abs() < 1e-10, "ratio 0.3 expected, got {v}"),
        other => panic!("expected F64, got {:?}", other),
    }
}

#[test]
fn aggop_windowed_sum_30s_roundtrip() {
    let mut op = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Sum, 30_000)));
    op.update(&row_amount(10.0), 0, Some("amount"), true);
    op.update(&row_amount(5.0), 20_000, Some("amount"), true);
    let before = op.query(29_999);
    let bytes = bincode::serialize(&op).expect("encode windowed");
    let decoded: AggOp = bincode::deserialize(&bytes).expect("decode windowed");
    let after = decoded.query(29_999);
    assert_eq!(before, after);
}

// ─── EntityKey + Value ────────────────────────────────────────────────────────

#[test]
fn entity_key_serde_roundtrip() {
    use compact_str::CompactString;
    use smallvec::SmallVec;
    let pairs: SmallVec<[(CompactString, Value); 2]> = SmallVec::from_buf([
        ("user_id".into(), Value::Str("alice".into())),
        ("merchant".into(), Value::Str("m1".into())),
    ]);
    let ek = EntityKey(pairs);
    let bytes = bincode::serialize(&ek).expect("encode");
    let decoded: EntityKey = bincode::deserialize(&bytes).expect("decode");
    assert_eq!(ek, decoded);
}

#[test]
fn value_serde_roundtrip_each_variant() {
    let variants = [
        Value::Null,
        Value::I64(42),
        Value::F64(2.5),
        Value::Bool(true),
        Value::Str("hello".into()),
        Value::Bytes(vec![0x01, 0x02, 0x03]),
        Value::Datetime(1_700_000_000_000),
        // The remaining three variants reach into `serde_json::Value` (Json)
        // or recursive Value containers (List, Map). Bincode does not support
        // `deserialize_any`, which `serde_json::Value::Deserialize` relies on
        // — historical coverage was limited to bincode-clean primitives, so
        // a Json-bearing snapshot decoded fine in tests but failed in prod.
        Value::Json(serde_json::json!([{"value": "a", "count": 5}])),
        Value::List(vec![Value::I64(1), Value::Str("two".into()), Value::Null]),
        Value::Map({
            let mut m = BTreeMap::new();
            m.insert("k1".to_string(), Value::I64(10));
            m.insert("k2".to_string(), Value::Bool(true));
            m
        }),
    ];
    for v in &variants {
        let bytes = bincode::serialize(v).expect("encode");
        let decoded: Value = bincode::deserialize(&bytes).expect("decode");
        assert_eq!(&decoded, v);
    }
}

/// Mirrors the live beava.dev SiteMetrics shape exactly: a derivation whose
/// `OpNode::GroupBy` carries an `AggSpec` with `params: serde_json::Value`.
/// The recovery log on prod (2026-05-08 04:04) showed:
///
/// ```text
/// "snapshot body decode failed; trying older snapshot"
/// "error":"bincode: Bincode does not support the
///  serde::Deserializer::deserialize_any method"
/// ```
///
/// This test isolates the same failure path: a SnapshotBody whose registry
/// contains a SiteMetrics-shaped derivation. Without the fix, bincode hits
/// `serde_json::Value`'s `deserialize_any` while decoding `AggSpec.params`
/// and aborts the whole snapshot decode.
#[test]
fn snapshot_body_aggspec_params_bincode_roundtrip() {
    use beava_core::op_node::{AggSpec, OpNode};
    use beava_core::registry::{DerivationDescriptor, OutputKind};

    let mut agg: BTreeMap<String, AggSpec> = BTreeMap::new();
    agg.insert(
        "median_dwell_1h".to_string(),
        AggSpec {
            op: "quantile".to_string(),
            params: serde_json::json!({
                "field": "dwell_ms",
                "window": "1h",
                "q": 0.5
            }),
        },
    );
    agg.insert(
        "page_views_today".to_string(),
        AggSpec {
            op: "count".to_string(),
            params: serde_json::json!({"window": "24h"}),
        },
    );
    agg.insert(
        "top_page_1h".to_string(),
        AggSpec {
            op: "top_k".to_string(),
            params: serde_json::json!({
                "field": "path",
                "window": "1h",
                "k": 1
            }),
        },
    );

    let derivation = DerivationDescriptor {
        name: "SiteMetrics".to_string(),
        output_kind: OutputKind::Table,
        upstreams: vec!["PageView".to_string()],
        ops: vec![OpNode::GroupBy { keys: vec![], agg }],
        schema: small_derived_schema(),
        table_primary_key: None,
        registered_at_version: 0,
    };

    let mut derivations = BTreeMap::new();
    derivations.insert("SiteMetrics".to_string(), derivation);
    let registry = RegistryDescriptorsOnly {
        version: 1,
        derivations,
        ..RegistryDescriptorsOnly::default()
    };

    let body = SnapshotBody {
        body_format_version: SNAPSHOT_BODY_FORMAT_VERSION,
        registry,
        state_tables: BTreeMap::new(),
        next_event_id: 0,
        query_time_ms: 0,
    };

    let bytes = body.encode().expect("encode");
    // The current bug: this decode fails with the bincode/`deserialize_any`
    // error, exactly matching the live recovery-log signature.
    let decoded = SnapshotBody::decode(&bytes).expect(
        "decode SiteMetrics-shape snapshot — prod failure path; \
         AggSpec.params (serde_json::Value) must survive bincode round-trip",
    );

    let d = decoded
        .registry
        .derivations
        .get("SiteMetrics")
        .expect("derivation present after decode");
    let group_by = match &d.ops[0] {
        beava_core::op_node::OpNode::GroupBy { agg, .. } => agg,
        other => panic!("expected GroupBy, got {other:?}"),
    };
    assert_eq!(group_by.len(), 3, "all three aggregations preserved");
    let q = group_by.get("median_dwell_1h").unwrap();
    assert_eq!(q.op, "quantile");
    assert_eq!(
        q.params["field"],
        serde_json::Value::String("dwell_ms".to_string())
    );
    assert_eq!(q.params["q"], serde_json::Value::from(0.5));
}

// ─── SnapshotBody round-trips ────────────────────────────────────────────────

#[test]
fn snapshot_body_empty_roundtrip() {
    let body = SnapshotBody {
        body_format_version: SNAPSHOT_BODY_FORMAT_VERSION,
        registry: RegistryDescriptorsOnly::default(),
        state_tables: BTreeMap::new(),
        next_event_id: 0,
        query_time_ms: 0,
    };
    let bytes = body.encode().expect("encode");
    let decoded = SnapshotBody::decode(&bytes).expect("decode");
    assert_eq!(decoded.body_format_version, SNAPSHOT_BODY_FORMAT_VERSION);
    assert_eq!(decoded.registry, RegistryDescriptorsOnly::default());
    assert!(decoded.state_tables.is_empty());
    assert_eq!(decoded.next_event_id, 0);
    assert_eq!(decoded.query_time_ms, 0);
    // Byte-equivalence confirms deterministic encoding.
    let reencoded = decoded.encode().expect("re-encode");
    assert_eq!(bytes, reencoded);
}

#[test]
fn snapshot_body_version_mismatch_rejected() {
    let body = SnapshotBody {
        body_format_version: 99,
        registry: RegistryDescriptorsOnly::default(),
        state_tables: BTreeMap::new(),
        next_event_id: 0,
        query_time_ms: 0,
    };
    let bytes = bincode::serialize(&body).expect("raw encode");
    match SnapshotBody::decode(&bytes) {
        Err(SnapshotBodyError::UnsupportedVersion(99)) => {}
        other => panic!("expected UnsupportedVersion(99), got {:?}", other),
    }
}

fn small_event_schema() -> EventSchema {
    let mut fields = BTreeMap::new();
    fields.insert("amount".to_string(), FieldType::F64);
    EventSchema {
        fields,
        optional_fields: vec![],
    }
}

fn small_table_schema() -> TableSchema {
    let mut fields = BTreeMap::new();
    fields.insert("user_id".to_string(), FieldType::Str);
    TableSchema {
        fields,
        optional_fields: vec![],
    }
}

fn small_derived_schema() -> DerivedSchema {
    DerivedSchema {
        fields: BTreeMap::new(),
        optional_fields: vec![],
    }
}

#[test]
fn snapshot_body_registry_descriptors_preserved() {
    let mut inner = RegistryInner {
        version: 3,
        ..RegistryInner::default()
    };
    use std::sync::Arc;
    inner.events.insert(
        "Txn".to_string(),
        Arc::new(EventDescriptor {
            name: "Txn".to_string(),
            schema: small_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 1,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        }),
    );
    inner.events.insert(
        "Login".to_string(),
        Arc::new(EventDescriptor {
            name: "Login".to_string(),
            schema: small_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 2,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        }),
    );
    inner.tables.insert(
        "UserStats".to_string(),
        TableDescriptor {
            name: "UserStats".to_string(),
            primary_key: vec!["user_id".to_string()],
            schema: small_table_schema(),
            ttl_ms: None,
            mode: TableMode::Upsert,
            registered_at_version: 3,
            temporal: false,
            retention_ms: None,
        },
    );
    inner.derivations.insert(
        "d1".to_string(),
        DerivationDescriptor {
            name: "d1".to_string(),
            output_kind: OutputKind::Table,
            upstreams: vec!["Txn".to_string()],
            ops: vec![],
            schema: small_derived_schema(),
            table_primary_key: None,
            registered_at_version: 3,
        },
    );

    let state_tables: StateTables = StateTables::default();
    let body = SnapshotBody::from_live(&inner, &state_tables, 0, 0);
    let bytes = body.encode().expect("encode");
    let decoded = SnapshotBody::decode(&bytes).expect("decode");

    assert_eq!(decoded.registry.version, inner.version);
    // Plan 18-11 D-6: inner.events holds Arc<EventDescriptor>; the snapshot
    // body holds plain EventDescriptor — compare element-wise.
    assert_eq!(decoded.registry.events.len(), inner.events.len());
    for (k, v) in &decoded.registry.events {
        let arc = inner.events.get(k).expect("key in inner");
        assert_eq!(v, arc.as_ref(), "event {} round-trip", k);
    }
    assert_eq!(decoded.registry.tables, inner.tables);
    assert_eq!(decoded.registry.derivations, inner.derivations);
}

#[test]
fn snapshot_body_state_tables_full_roundtrip() {
    use beava_core::agg_descriptor::AggregationDescriptor;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    // Plan 18-16 Task 16.2: state_tables is now Vec<AggStateTable> indexed by
    // agg_id, and SnapshotBody::from_live walks registry.compiled_aggregations
    // to assemble (name, entries) pairs. Build a RegistryInner with two
    // aggregations (agg_a → agg_id 0, agg_b → agg_id 1) so the snapshot
    // serializer finds them.
    let mut compiled_aggregations: BTreeMap<String, Arc<AggregationDescriptor>> = BTreeMap::new();
    let mut state_tables: StateTables = vec![AggStateTable::new(), AggStateTable::new()];
    for (idx, node) in ["agg_a", "agg_b"].iter().enumerate() {
        compiled_aggregations.insert(
            node.to_string(),
            Arc::new(AggregationDescriptor {
                node_name: node.to_string(),
                source_node_name: "src".to_string(),
                group_keys: vec!["user_id".to_string()],
                features: vec![],
                agg_id: idx as u32,
                field_names: vec![],
                cluster_id: 0,
            }),
        );
        let table = &mut state_tables[idx];
        for u in ["alice", "bob", "carol"] {
            use compact_str::CompactString;
            use smallvec::SmallVec;
            let pair: (CompactString, Value) = ("user_id".into(), Value::Str(u.into()));
            let key = EntityKey(SmallVec::from_buf_and_len(
                [pair, ("".into(), Value::Null)],
                1,
            ));
            let mut cnt = AggOp::Count(CountState::default());
            cnt.update(&Row::new(), 0, None, true);
            cnt.update(&Row::new(), 1, None, true);
            let mut s = mk_sum_op();
            s.update(&row_amount(10.0), 0, Some("amount"), true);
            table.insert_from_entity_key(key, vec![cnt, s]);
        }
    }
    let _unused = mk_count_op();
    let inner = RegistryInner {
        compiled_aggregations,
        next_agg_id: 2,
        ..Default::default()
    };
    let body = SnapshotBody::from_live(&inner, &state_tables, 12, 4567);
    let bytes = body.encode().expect("encode");
    let decoded = SnapshotBody::decode(&bytes).expect("decode");

    assert_eq!(decoded.next_event_id, 12);
    assert_eq!(decoded.query_time_ms, 4567);
    assert_eq!(decoded.state_tables.len(), 2);
    for node in ["agg_a", "agg_b"] {
        let entries = decoded.state_tables.get(node).expect("node present");
        assert_eq!(entries.len(), 3);
        for (_ek, ops) in entries {
            assert_eq!(ops.len(), 2);
            assert_eq!(ops[0].query(0), Value::I64(2)); // count
            assert_eq!(ops[1].query(0), Value::F64(10.0)); // sum
        }
    }
}
