//! Locks down the agg-local vs union field-index invariant.
//!
//! The apply-loop (`agg_apply::apply_event_to_aggregations`) maintains two
//! index spaces over the event's field values:
//!
//!   * **agg-local** — position in `AggregationDescriptor.field_names`,
//!     scoped to a single aggregation. `AggOpDescriptor.field_idx` is in this
//!     space.
//!   * **union** — position in `EventDescriptor.apply_field_names`, the
//!     source-wide union of every field any agg on that source references.
//!     `ExtractedFields` is indexed by this space; `lat_idx` / `lon_idx`
//!     resolved by the geo resolver are in this space.
//!
//! The bridge is `field_idx_into_event_extracted[agg_local_idx] = union_idx`,
//! populated at register time. The outer dispatcher uses the bridge to resolve
//! `pre_val` once; inner arms must consume that `pre_val` and must NOT
//! re-extract from `extracted[field_idx]` (which is agg-local).
//!
//! The two index spaces only coincide when every agg happens to declare its
//! `field_names` in the same order as the source union. PR #106 (commits
//! `c0ba92a4` … `ed7192b6`) fixed two arms — `WindowedOp::update_at` and
//! `EventTypeMixState::update_at` — that were ignoring the resolved `pre_val`
//! and re-extracting from `extracted[field_idx]`. The bug stayed dormant
//! because the existing test corpus always happened to declare agg fields in
//! source-union order. These tests stress the case where the two orderings
//! deliberately disagree, locking the contract.
//!
//! Strategy mirrors `windowed_op_uses_caller_pre_val.rs`: call
//! `AggOp::update_with_extracted` directly with a caller-resolved `pre_val`
//! and an `extracted` array whose `field_idx`-th slot deliberately holds a
//! WRONG value. An op that honours the contract sees the caller's `pre_val`;
//! a regression that re-extracts from `extracted[field_idx]` sees the wrong
//! slot and produces a detectably wrong result.

use beava_core::agg_buffer::EventTypeMixState;
use beava_core::agg_geo::{GeoDistanceState, GeoVelocityState};
use beava_core::agg_op::{AggKind, AggOp, ExtractedFields, SketchParams, FIELD_IDX_NONE};
use beava_core::agg_state::{AvgState, SumState};
use beava_core::agg_windowed::WindowedOp;
use beava_core::row::{Row, Value};
use smallvec::smallvec;

const WINDOW_MS: u64 = 64_000;

// ── Test 1: two aggs on the same source declare overlapping fields in
//           opposite orders ─────────────────────────────────────────────────

/// Two aggregations sharing a source declare overlapping field subsets in
/// REVERSED order:
///
///   * Source union (`apply_field_names`): `["price", "category"]`
///   * Agg A `field_names`: `["price", "category"]`   (agg-local matches union)
///   * Agg B `field_names`: `["category", "price"]`   (agg-local is REVERSED)
///
/// For Agg B the agg-local index of `category` is 0, but its union index is 1
/// (and vice-versa for `price`). The outer apply-loop resolves `pre_val` via
/// the union remap; the inner arms must honour that resolution.
///
/// This test stresses the three arms that historically had the bug or the
/// shape of the bug: a windowed `Sum` (windowed wrapper), a windowed `Avg`
/// (mean — same bug-class symptom: silent `Null`), and an `EventTypeMix`.
/// All three must report the correct per-field result for both aggs.
#[test]
fn multi_agg_same_source_reversed_field_order() {
    // Union order: price @ 0, category @ 1.
    let price = Value::F64(10.0);
    let category = Value::Str("cat_a".into());
    let extracted: ExtractedFields<'_> = smallvec![Some(&price), Some(&category)];

    // ── Agg A: same order as union ──────────────────────────────────────────
    //   feature_0 = windowed Sum("price"), agg_local=0, union=0  → consistent
    //   feature_1 = EventTypeMix("category"), agg_local=1, union=1 → consistent
    let mut a_sum = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Sum, WINDOW_MS)));
    let mut a_etm = AggOp::EventTypeMix(Box::new(EventTypeMixState::new(10, None)));

    // ── Agg B: reversed ─────────────────────────────────────────────────────
    //   feature_0 = EventTypeMix("category"), agg_local=0, union=1 → mismatched
    //   feature_1 = windowed Avg("price"),    agg_local=1, union=0 → mismatched
    let mut b_etm = AggOp::EventTypeMix(Box::new(EventTypeMixState::new(10, None)));
    let mut b_avg = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Avg, WINDOW_MS)));

    // Outer apply-loop semantics: caller resolves pre_val from extracted via
    // the union remap, then threads the agg-local field_idx down. We
    // replicate that resolution explicitly here.
    for now_ms in [100_i64, 200] {
        // Agg A — agg-local matches union, so field_idx == union_idx.
        a_sum.update_with_extracted(
            Some(&price), // resolved via remap A[0] → union 0
            now_ms,
            None,
            &Row::new(),
            Some("price"),
            0, // agg-local
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        a_etm.update_with_extracted(
            Some(&category), // resolved via remap A[1] → union 1
            now_ms,
            None,
            &Row::new(),
            Some("category"),
            1, // agg-local
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );

        // Agg B — agg-local DISAGREES with union.
        // feature_0: EventTypeMix("category") at agg-local 0 → union 1.
        // A regression that re-extracts `extracted[field_idx=0]` reads
        // `price` (a numeric) and EventTypeMix's update_at would either
        // count "10" (I64 path) or no-op (F64 path); either way "cat_a"
        // would NEVER be counted.
        b_etm.update_with_extracted(
            Some(&category), // resolved via remap B[0] → union 1
            now_ms,
            None,
            &Row::new(),
            Some("category"),
            0, // agg-local — DELIBERATELY DISAGREES with union(=1)
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        // feature_1: windowed Avg("price") at agg-local 1 → union 0.
        // A regression that re-extracts `extracted[field_idx=1]` reads
        // `category` (a Str) which AvgState::update_pre rejects → n stays 0
        // → query returns Null. The classic mean-returns-null symptom.
        b_avg.update_with_extracted(
            Some(&price), // resolved via remap B[1] → union 0
            now_ms,
            None,
            &Row::new(),
            Some("price"),
            1, // agg-local — DELIBERATELY DISAGREES with union(=0)
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }

    // Agg A windowed Sum(price) must be 2 × 10.0 = 20.0.
    match a_sum.query(999) {
        Value::F64(v) => assert!(
            (v - 20.0).abs() < 1e-9,
            "Agg A windowed Sum(price) must be 20.0 (2 × 10.0); got {v}"
        ),
        other => panic!("Agg A Sum expected F64, got {other:?}"),
    }

    // Agg A EventTypeMix(category) must contain "cat_a".
    match a_etm.query(999) {
        Value::Map(m) => assert!(
            m.contains_key("cat_a"),
            "Agg A EventTypeMix(category) must count 'cat_a'; got {m:?}"
        ),
        other => panic!("Agg A EventTypeMix expected Map, got {other:?}"),
    }

    // Agg B EventTypeMix(category) — locks the bug shape: agg-local idx
    // disagrees with union idx. Must count "cat_a", NOT "10" or anything
    // numeric. A regression that re-extracted `extracted[0]` would see the
    // price (Value::F64(10.0)) and EventTypeMix's update_at would drop it on
    // the F64 arm (no-op), leaving the map empty.
    match b_etm.query(999) {
        Value::Map(m) => {
            assert!(
                m.contains_key("cat_a"),
                "Agg B EventTypeMix(category) (reversed order) must count 'cat_a' \
                 — regression would silently drop the value because it re-extracted \
                 the numeric `price` slot; got {m:?}"
            );
            assert!(
                !m.contains_key("10") && !m.contains_key("10.0"),
                "Agg B EventTypeMix must NOT count the wrong (price) slot; got {m:?}"
            );
        }
        other => panic!("Agg B EventTypeMix expected Map, got {other:?}"),
    }

    // Agg B windowed Avg(price) — must be 10.0. A regression that
    // re-extracted `extracted[1]` would read `category` (Str), AvgState would
    // reject, n stays 0, query returns Null. This is exactly the
    // `mean("price", window="…") returns Null` symptom called out in the
    // PR #106 bug report.
    match b_avg.query(999) {
        Value::F64(v) => assert!(
            (v - 10.0).abs() < 1e-9,
            "Agg B windowed Avg(price) (reversed order) must be 10.0; got {v}"
        ),
        Value::Null => panic!(
            "Agg B windowed Avg(price) returned Null — regression: re-extracted \
             a non-numeric slot, n stayed 0, query short-circuited to Null"
        ),
        other => panic!("Agg B Avg expected F64, got {other:?}"),
    }
}

// ── Test 2: three aggs declare disjoint 2-field subsets of a 5-field union ──

/// Three aggregations on the same source, each referencing a DISJOINT pair of
/// fields drawn from a 5-field source union. Each agg's `field_names` order
/// has zero relationship to its position in the union — agg-local index 0/1
/// resolves to different union indices in every agg. All three must produce
/// the correct per-field result on a non-windowed `update_pre` path
/// (`SumState`, `AvgState`) and via an `EventTypeMix` arm.
///
/// Union: `["a", "b", "c", "d", "e"]` (indices 0..=4)
///   * Agg X: fields=["c", "e"] (agg-local 0→union 2, agg-local 1→union 4)
///   * Agg Y: fields=["a", "d"] (agg-local 0→union 0, agg-local 1→union 3)
///   * Agg Z: fields=["b", "c"] (agg-local 0→union 1, agg-local 1→union 2)
#[test]
fn non_overlapping_field_subsets_across_aggs() {
    let va = Value::F64(1.0);
    let vb = Value::Str("BB".into());
    let vc = Value::F64(3.0);
    let vd = Value::F64(4.0);
    let ve = Value::Str("EE".into());
    let extracted: ExtractedFields<'_> = smallvec![
        Some(&va), // union 0
        Some(&vb), // union 1
        Some(&vc), // union 2
        Some(&vd), // union 3
        Some(&ve), // union 4
    ];

    // Agg X — Sum("c"), EventTypeMix("e").
    //   feature_0: Sum(c)            agg-local 0, union 2.
    //   feature_1: EventTypeMix(e)   agg-local 1, union 4.
    let mut x_sum = AggOp::Sum(SumState::default());
    let mut x_etm = AggOp::EventTypeMix(Box::new(EventTypeMixState::new(10, None)));

    // Agg Y — Avg("a"), Sum("d").
    let mut y_avg = AggOp::Avg(AvgState::default());
    let mut y_sum = AggOp::Sum(SumState::default());

    // Agg Z — EventTypeMix("b"), Sum("c").
    let mut z_etm = AggOp::EventTypeMix(Box::new(EventTypeMixState::new(10, None)));
    let mut z_sum = AggOp::Sum(SumState::default());

    // Outer-dispatcher resolution: caller provides pre_val from the union
    // index, threads down the (deliberately mismatched) agg-local field_idx.
    // Six calls modelled inline (one per (agg, feature)) — splicing mut refs
    // into an iterable would fight the borrow checker for no gain.
    let now_ms: i64 = 100;
    x_sum.update_with_extracted(
        Some(&vc),
        now_ms,
        None,
        &Row::new(),
        Some("c"),
        0,
        &extracted,
        FIELD_IDX_NONE,
        FIELD_IDX_NONE,
    );
    x_etm.update_with_extracted(
        Some(&ve),
        now_ms,
        None,
        &Row::new(),
        Some("e"),
        1,
        &extracted,
        FIELD_IDX_NONE,
        FIELD_IDX_NONE,
    );
    y_avg.update_with_extracted(
        Some(&va),
        now_ms,
        None,
        &Row::new(),
        Some("a"),
        0,
        &extracted,
        FIELD_IDX_NONE,
        FIELD_IDX_NONE,
    );
    y_sum.update_with_extracted(
        Some(&vd),
        now_ms,
        None,
        &Row::new(),
        Some("d"),
        1,
        &extracted,
        FIELD_IDX_NONE,
        FIELD_IDX_NONE,
    );
    z_etm.update_with_extracted(
        Some(&vb),
        now_ms,
        None,
        &Row::new(),
        Some("b"),
        0,
        &extracted,
        FIELD_IDX_NONE,
        FIELD_IDX_NONE,
    );
    z_sum.update_with_extracted(
        Some(&vc),
        now_ms,
        None,
        &Row::new(),
        Some("c"),
        1,
        &extracted,
        FIELD_IDX_NONE,
        FIELD_IDX_NONE,
    );

    // Assertions: each agg must report its OWN field's value, not a sibling's.
    assert_eq!(
        x_sum.query(999),
        Value::F64(3.0),
        "X.Sum(c) must be 3.0 (the 'c' value), not a sibling's slot"
    );
    match x_etm.query(999) {
        Value::Map(m) => assert!(
            m.contains_key("EE"),
            "X.EventTypeMix(e) must count 'EE'; got {m:?}"
        ),
        other => panic!("X.etm expected Map, got {other:?}"),
    }
    assert_eq!(
        y_avg.query(999),
        Value::F64(1.0),
        "Y.Avg(a) must be 1.0 (the 'a' value)"
    );
    assert_eq!(
        y_sum.query(999),
        Value::F64(4.0),
        "Y.Sum(d) must be 4.0 (the 'd' value)"
    );
    match z_etm.query(999) {
        Value::Map(m) => assert!(
            m.contains_key("BB"),
            "Z.EventTypeMix(b) must count 'BB'; got {m:?}"
        ),
        other => panic!("Z.etm expected Map, got {other:?}"),
    }
    assert_eq!(
        z_sum.query(999),
        Value::F64(3.0),
        "Z.Sum(c) must be 3.0 (the 'c' value)"
    );
}

// ── Test 3: windowed mean(field_a) followed by windowed top_k(field_b)
//           in the SAME agg — the marketplace_rerank.py bug shape ───────────

/// A single aggregation declares two windowed ops over DIFFERENT fields:
///
///   * feature_0 = `mean("price",    window=64s)` — agg-local 0
///   * feature_1 = `top_k("category", window=64s)` — agg-local 1
///
/// Source union: `["price", "category"]` (agg-local matches union here, but
/// the bug surfaces in the `top_k` arm regardless — it reads its OWN field_idx
/// without the union remap, and the windowed wrapper used to bypass the
/// caller-resolved `pre_val`). We arrange the bug-shape directly: extracted
/// slot 1 holds a deliberately wrong value (a float) and the caller supplies
/// the correct category Value. A `top_k` arm that re-extracts using its own
/// `field_idx` (agg-local 1) would read the wrong slot and surface the float
/// instead of `"cat_a"`.
///
/// Mirrors `windowed_top_k_captures_caller_pre_val_not_extracted_field_idx`
/// from `windowed_op_uses_caller_pre_val.rs` but through the multi-op-in-one-
/// agg lens, which is the exact shape of the marketplace_rerank.py regression
/// surfaced in PR #106.
#[test]
fn top_k_after_windowed_mean_in_same_agg() {
    let price = Value::F64(10.0);
    let category = Value::Str("cat_a".into());
    // Deliberately wrong value at slot 1 (the agg-local TopK field_idx). A
    // regression that re-extracted `extracted[1]` would read 999.0 instead
    // of the caller-resolved "cat_a".
    let wrong_at_slot_1 = Value::F64(999.0);
    let extracted: ExtractedFields<'_> = smallvec![Some(&price), Some(&wrong_at_slot_1)];

    let mut mean_op = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Avg, WINDOW_MS)));
    let top_k_params = SketchParams {
        top_k_k: Some(1),
        ..SketchParams::default()
    };
    let mut top_k_op = AggOp::Windowed(Box::new(WindowedOp::new_with_params(
        AggKind::TopK,
        WINDOW_MS,
        top_k_params,
    )));

    for now_ms in [100_i64, 200, 300] {
        // mean(price) — agg-local 0; pre_val resolved correctly to price.
        mean_op.update_with_extracted(
            Some(&price),
            now_ms,
            None,
            &Row::new(),
            Some("price"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        // top_k(category) — agg-local 1; caller resolves pre_val to category.
        // A regression in the windowed wrapper or in TopK's update_pre that
        // re-extracted `extracted[field_idx=1]` would read the wrong float.
        top_k_op.update_with_extracted(
            Some(&category),
            now_ms,
            None,
            &Row::new(),
            Some("category"),
            1,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }

    // mean must accumulate the 3 × 10.0 prices → 10.0.
    match mean_op.query(999) {
        Value::F64(v) => assert!(
            (v - 10.0).abs() < 1e-9,
            "windowed mean(price) must be 10.0; got {v}"
        ),
        other => panic!("windowed mean expected F64, got {other:?}"),
    }

    // top_k must surface "cat_a", NOT the wrong float at slot 1.
    let rendered = format!("{:?}", top_k_op.query(999));
    assert!(
        rendered.contains("cat_a"),
        "windowed top_k(category) must surface the caller's pre_val 'cat_a'; got {rendered}"
    );
    assert!(
        !rendered.contains("999"),
        "windowed top_k must NOT surface the wrong slot (999.0); got {rendered}"
    );
}

// ── Test 4: two geo aggs declare lat/lon in different positions ─────────────

/// Two geo aggregations share the same lat/lon fields on a source but declare
/// them in DIFFERENT positions in their respective `field_names` lists. The
/// geo fast-path takes `lat_idx` / `lon_idx` directly in the source-UNION
/// space — they're already remapped by the geo resolver at register time. As
/// long as the apply-loop threads the correct (union) indices down, both
/// aggs must produce identical `geo_velocity` / `geo_distance` results.
///
/// This test locks in the lat_idx/lon_idx union-remap discipline: the audit
/// flagged this path as POSSIBLE-BUG but architecturally safe (the indices
/// passed into `update_at` are union-indexed, not agg-local). Locking it
/// makes any future refactor that accidentally passes agg-local lat/lon
/// indices fail loudly.
///
/// Setup: union ordering is `["lat", "lon"]` → `lat_idx=0`, `lon_idx=1`.
/// Both aggs are fed the same lat/lon-bearing events with the same union
/// indices. Their `field_names` agg-local ordering differs (Agg P declares
/// `["lat", "lon"]`, Agg Q declares `["lon", "lat"]`), but the
/// dispatcher-supplied lat/lon idx must be union-indexed for both.
#[test]
fn geo_ops_with_reversed_lat_lon_position_in_field_list() {
    // Two events 1 hour apart, ~111 km along a meridian (1° latitude).
    let lat_1 = Value::F64(0.0);
    let lon_1 = Value::F64(0.0);
    let lat_2 = Value::F64(1.0);
    let lon_2 = Value::F64(0.0);

    // Agg P (agg-local order = union order: lat=0, lon=1).
    let mut p_vel = AggOp::GeoVelocity(Box::new(GeoVelocityState::with_fields(
        "lat".into(),
        "lon".into(),
    )));
    let mut p_dist = AggOp::GeoDistance(Box::new(GeoDistanceState::with_fields(
        "lat".into(),
        "lon".into(),
    )));

    // Agg Q (agg-local REVERSED: lat would be at agg-local 1, lon at 0).
    // The contract: register-time geo resolver populates `lat_idx`/`lon_idx`
    // in the UNION space, so the apply-loop threads lat_idx=0, lon_idx=1 for
    // BOTH aggs regardless of agg-local ordering. We verify the geo
    // update_at fast-path honours that — a regression that confused
    // agg-local with union would swap lat/lon for Agg Q.
    let mut q_vel = AggOp::GeoVelocity(Box::new(GeoVelocityState::with_fields(
        "lat".into(),
        "lon".into(),
    )));
    let mut q_dist = AggOp::GeoDistance(Box::new(GeoDistanceState::with_fields(
        "lat".into(),
        "lon".into(),
    )));

    // Two events 1h apart, both feeding into all four ops with identical
    // union-indexed lat_idx=0, lon_idx=1.
    let h0_ms: i64 = 0;
    let h1_ms: i64 = 3_600_000;
    let extracted_e1: ExtractedFields<'_> = smallvec![Some(&lat_1), Some(&lon_1)];
    let extracted_e2: ExtractedFields<'_> = smallvec![Some(&lat_2), Some(&lon_2)];

    for (op_name, op) in [
        ("p_vel", &mut p_vel),
        ("p_dist", &mut p_dist),
        ("q_vel", &mut q_vel),
        ("q_dist", &mut q_dist),
    ] {
        let _ = op_name;
        op.update_with_extracted(
            None,
            h0_ms,
            None,
            &Row::new(),
            None,
            FIELD_IDX_NONE,
            &extracted_e1,
            0, // lat_idx (union)
            1, // lon_idx (union)
        );
        op.update_with_extracted(
            None,
            h1_ms,
            None,
            &Row::new(),
            None,
            FIELD_IDX_NONE,
            &extracted_e2,
            0, // lat_idx (union)
            1, // lon_idx (union)
        );
    }

    // Both velocity ops must agree (and be ~111 km/h, the haversine for 1°
    // latitude over 1 hour).
    let v_p = p_vel.query(h1_ms);
    let v_q = q_vel.query(h1_ms);
    assert_eq!(
        v_p, v_q,
        "Agg P and Agg Q geo_velocity must produce identical results regardless \
         of agg-local field-name ordering; got P={v_p:?} Q={v_q:?}"
    );
    match v_p {
        Value::F64(kmh) => assert!(
            (kmh - 111.0).abs() < 1.0,
            "geo_velocity must be ~111 km/h for 1° latitude over 1 hour; got {kmh}"
        ),
        other => panic!("expected F64 for geo_velocity, got {other:?}"),
    }

    // Both distance ops must agree.
    let d_p = p_dist.query(h1_ms);
    let d_q = q_dist.query(h1_ms);
    assert_eq!(
        d_p, d_q,
        "Agg P and Agg Q geo_distance must produce identical results regardless \
         of agg-local field-name ordering; got P={d_p:?} Q={d_q:?}"
    );
    match d_p {
        Value::F64(km) => assert!(
            (km - 111.0).abs() < 1.0,
            "geo_distance must be ~111 km for 1° latitude; got {km}"
        ),
        other => panic!("expected F64 for geo_distance, got {other:?}"),
    }
}
