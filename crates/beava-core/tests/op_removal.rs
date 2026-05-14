//! Integration tests for Plan 19.2-06 (D-05): removal of unique_cells and
//! geo_entropy from the operator catalogue + addition of quadkey builtin.
//!
//! RED commit: these tests fail BEFORE Task 1.b removes the variants and adds
//! the quadkey builtin. Specifically:
//!
//!   - Tests 1+2: unique_cells / geo_entropy registration should FAIL after
//!     removal; they currently SUCCEED → assertion inverted to prove RED.
//!   - Tests 3+4: lookup_builtin("quadkey") returns None today (builtin absent) → RED.
//!
//! GREEN commit: after Task 1.b, all 4 tests pass.

use beava_core::agg_compile::compile_aggregations_from_nodes;
use beava_core::expr_builtins::{lookup_builtin, Arity};
use beava_core::op_node::{AggSpec, OpNode};
use beava_core::registry::{DerivationDescriptor, EventDescriptor, OutputKind, RegistryInner};
use beava_core::registry_diff::PayloadNode;
use beava_core::schema::{DerivedSchema, EventSchema, FieldType};
use std::collections::BTreeMap;
use std::sync::Arc;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_event_schema_fields(fields: &[(&str, FieldType)]) -> EventSchema {
    EventSchema {
        fields: fields
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect::<BTreeMap<_, _>>(),
        optional_fields: vec![],
    }
}

fn make_event_descriptor(name: &str, fields: &[(&str, FieldType)]) -> EventDescriptor {
    EventDescriptor {
        name: name.to_string(),
        schema: make_event_schema_fields(fields),
        dedupe_key: None,
        dedupe_window_ms: None,
        keep_events_for_ms: None,
        cold_after_ms: None,
        registered_at_version: 0,
        name_arc: Arc::from(""),
        apply_field_names: vec![],
    }
}

/// Build a PayloadNode list: event + derivation with a single geo-family agg.
fn geo_agg_nodes(
    event_name: &str,
    deriv_name: &str,
    feat_name: &str,
    op: &str,
) -> Vec<PayloadNode> {
    let event = make_event_descriptor(
        event_name,
        &[
            ("user_id", FieldType::Str),
            ("lat", FieldType::F64),
            ("lon", FieldType::F64),
        ],
    );

    let mut agg_map = BTreeMap::new();
    agg_map.insert(
        feat_name.to_string(),
        AggSpec {
            op: op.to_string(),
            params: serde_json::json!({"lat": "lat", "lon": "lon"}),
        },
    );

    let deriv = DerivationDescriptor {
        name: deriv_name.to_string(),
        output_kind: OutputKind::Table,
        upstreams: vec![event_name.to_string()],
        ops: vec![OpNode::GroupBy {
            keys: vec!["user_id".to_string()],
            agg: agg_map,
        }],
        schema: DerivedSchema {
            fields: BTreeMap::new(),
            optional_fields: vec![],
        },
        table_primary_key: Some(vec!["user_id".to_string()]),
        registered_at_version: 0,
    };

    vec![PayloadNode::Event(event), PayloadNode::Derivation(deriv)]
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// After Plan 19.2-06 Task 1.b removes unique_cells from agg_compile.rs,
/// registering an aggregation with op="unique_cells" must fail with an
/// "unknown" operator error. Before removal it succeeds → this test is RED.
#[test]
fn test_unique_cells_register_rejected() {
    let nodes = geo_agg_nodes("Src", "Deriv", "cells", "unique_cells");
    let registry = RegistryInner::default();
    let (_compiled, errors) = compile_aggregations_from_nodes(&nodes, &registry, &[]);
    // Must have at least one error about the unknown op name.
    assert!(
        !errors.is_empty(),
        "expected registration of 'unique_cells' to be rejected after D-05 removal; got no errors"
    );
    let err_text = format!("{:?}", errors);
    assert!(
        err_text.contains("unique_cells")
            || err_text.to_lowercase().contains("unknown")
            || err_text.to_lowercase().contains("unsupported"),
        "error should mention 'unique_cells' or 'unknown'; got: {err_text}"
    );
}

/// After Plan 19.2-06 Task 1.b removes geo_entropy from agg_compile.rs,
/// registering an aggregation with op="geo_entropy" must fail. Before removal
/// it succeeds → this test is RED.
#[test]
fn test_geo_entropy_register_rejected() {
    let nodes = geo_agg_nodes("Src", "Deriv", "geo_h", "geo_entropy");
    let registry = RegistryInner::default();
    let (_compiled, errors) = compile_aggregations_from_nodes(&nodes, &registry, &[]);
    assert!(
        !errors.is_empty(),
        "expected registration of 'geo_entropy' to be rejected after D-05 removal; got no errors"
    );
    let err_text = format!("{:?}", errors);
    assert!(
        err_text.contains("geo_entropy")
            || err_text.to_lowercase().contains("unknown")
            || err_text.to_lowercase().contains("unsupported"),
        "error should mention 'geo_entropy' or 'unknown'; got: {err_text}"
    );
}

/// After Plan 19.2-06 Task 1.b adds the quadkey builtin in expr_builtins.rs,
/// lookup_builtin("quadkey") must return Some(_) with arity Fixed(3).
/// Before Task 1.b it returns None → RED.
#[test]
fn test_quadkey_builtin_exists() {
    let b = lookup_builtin("quadkey");
    assert!(
        b.is_some(),
        "quadkey builtin not found in BUILTINS table; Task 1.b must add it"
    );
    assert_eq!(
        b.unwrap().arity,
        Arity::Fixed(3),
        "quadkey must have arity Fixed(3) — args: (lat, lon, zoom)"
    );
}

/// After Task 1.b adds the quadkey builtin with the simplified-Mercator formula,
/// it must be deterministic and spatially-coherent (same call → same result;
/// nearby coords at same zoom → same tile; different zoom → different value).
/// Before Task 1.b the builtin doesn't exist → this panics at `.expect(...)` → RED.
#[test]
fn test_quadkey_returns_deterministic_cell_id() {
    use beava_core::row::Value;

    let b = lookup_builtin("quadkey").expect("quadkey builtin must exist after Task 1.b");

    // Determinism: same args → same output.
    let lat = Value::F64(40.0);
    let lon = Value::F64(-74.0);
    let zoom = Value::I64(7);
    let r1 = (b.eval)(&[lat.clone(), lon.clone(), zoom.clone()]);
    let r2 = (b.eval)(&[lat.clone(), lon.clone(), zoom.clone()]);
    assert_eq!(r1, r2, "quadkey must be deterministic");
    assert!(
        matches!(r1, Value::I64(_)),
        "quadkey must return Value::I64 cell id; got {r1:?}"
    );

    // Nearby coords share a tile at zoom=7 (~150 km cell).
    let lon_near = Value::F64(-74.001);
    let r_near = (b.eval)(&[lat.clone(), lon_near, zoom.clone()]);
    assert_eq!(
        r1, r_near,
        "lat=40.0, lon=-74.0 and lat=40.0, lon=-74.001 must map to the same zoom=7 tile"
    );

    // Different zoom → different granularity → different cell id for same coords.
    let zoom12 = Value::I64(12);
    let r_z12 = (b.eval)(&[lat.clone(), lon.clone(), zoom12]);
    assert_ne!(
        r1, r_z12,
        "zoom=7 and zoom=12 must produce different cell ids for the same lat/lon"
    );

    // Null lat → Null output.
    let r_null = (b.eval)(&[Value::Null, lon.clone(), zoom.clone()]);
    assert_eq!(r_null, Value::Null, "null lat must return Null");

    // Out-of-range zoom (0 or 25) → Null.
    let r_zoom0 = (b.eval)(&[lat.clone(), lon.clone(), Value::I64(0)]);
    assert_eq!(r_zoom0, Value::Null, "zoom=0 must return Null");
    let r_zoom25 = (b.eval)(&[lat, lon, Value::I64(25)]);
    assert_eq!(
        r_zoom25,
        Value::Null,
        "zoom=25 must return Null (max is 24)"
    );
}
