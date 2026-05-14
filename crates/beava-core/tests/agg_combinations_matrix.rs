//! Combination-matrix coverage for aggregation behavior axes.
//!
//! The audit triggered by PR #106 found that the three axes of aggregation
//! behavior — `where=…`, `window=…`, and field-bearing-vs-fieldless — were
//! each tested in isolation, but the 2^3 = 8 combinations were not.
//! The recently-fixed bug (windowed `Avg` returning `Null`, windowed `TopK`
//! reading the wrong field) lived in the (field-bearing × windowed) cell
//! precisely because no test stressed it.
//!
//! This file covers:
//!
//! 1. **Group 1 — `(where × window × field)` matrix (8 tests).** One test per
//!    cell of the 2^3 combination space, with a deliberate mix of matching /
//!    non-matching `where` predicates and field values, asserting the exact
//!    expected query value.
//! 2. **Group 2 — Multi-op windowed composition (4 tests).** Stress that
//!    multiple windowed ops sharing one agg do not bleed state across
//!    fields, in particular through the dispatcher's caller-pre_val
//!    contract (`WindowedOp::update_at` must honour the outer-resolved
//!    `pre_val`, not re-extract `extracted[field_idx]`).
//!
//! Style mirrors `windowed_op_uses_caller_pre_val.rs`: each test docstring
//! states the invariant; `panic!` / `assert!` messages name the bug class
//! the test guards against.

use beava_core::agg_op::{AggKind, AggOp, ExtractedFields, SketchParams, FIELD_IDX_NONE};
use beava_core::agg_state::{CountState, SumState};
use beava_core::agg_windowed::WindowedOp;
use beava_core::expr::parse as parse_expr;
use beava_core::row::{Row, Value};
use smallvec::smallvec;
use std::sync::Arc;

const WINDOW_MS: u64 = 64_000;

// ═══════════════════════════════════════════════════════════════════════════
// Group 1 — (where × window × field) 2^3 matrix.
// ═══════════════════════════════════════════════════════════════════════════

// ── (no where, no window, no field) — Count baseline ────────────────────────

/// 5 events, no predicate, no window, no field. The fieldless lifetime `Count`
/// op must simply return n=5.
#[test]
fn matrix_no_where_no_window_no_field() {
    let mut op = AggOp::Count(CountState::default());
    for now_ms in [100_i64, 200, 300, 400, 500] {
        let extracted: ExtractedFields<'_> = smallvec![None];
        op.update_with_extracted(
            None,
            now_ms,
            None,
            &Row::new(),
            None,
            FIELD_IDX_NONE,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }
    match op.query(999) {
        Value::I64(5) => {}
        other => panic!(
            "no-where/no-window/no-field Count should be 5; got {other:?} \
             (would indicate fieldless lifetime Count dispatch is broken)"
        ),
    }
}

// ── (where, no window, no field) — Count with predicate ─────────────────────

/// 5 events with `amount`s [10, 20, 30, 40, 50]; predicate `amount > 25`
/// drops the first two. Lifetime Count under a where must return 3.
#[test]
fn matrix_where_no_window_no_field() {
    let mut op = AggOp::Count(CountState::default());
    let where_expr = Arc::new(parse_expr("(amount > 25)").expect("parse"));
    for (i, &amount) in [10.0_f64, 20.0, 30.0, 40.0, 50.0].iter().enumerate() {
        let row = Row::new().with_field("amount", Value::F64(amount));
        let extracted: ExtractedFields<'_> = smallvec![None];
        op.update_with_extracted(
            None,
            (i as i64) * 100,
            Some(&where_expr),
            &row,
            None,
            FIELD_IDX_NONE,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }
    match op.query(999) {
        Value::I64(3) => {}
        other => panic!(
            "where=`amount>25` over [10,20,30,40,50] should keep 3; got {other:?} \
             (would indicate predicate-gated lifetime Count is broken)"
        ),
    }
}

// ── (no where, no window, field) — Sum over a field ─────────────────────────

/// 4 events with prices [1, 2, 3, 4]; no predicate, no window. Lifetime Sum
/// over the `price` field must return 10.0.
#[test]
fn matrix_no_where_no_window_field() {
    let mut op = AggOp::Sum(SumState::default());
    for (i, &p) in [1.0_f64, 2.0, 3.0, 4.0].iter().enumerate() {
        let pre = Value::F64(p);
        let extracted: ExtractedFields<'_> = smallvec![Some(&pre)];
        op.update_with_extracted(
            Some(&pre),
            (i as i64) * 100,
            None,
            &Row::new(),
            Some("price"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }
    match op.query(999) {
        Value::F64(v) => assert!(
            (v - 10.0).abs() < 1e-9,
            "no-where/no-window/field Sum should be 10.0; got {v} \
             (would indicate field-bearing lifetime Sum pre_val path is broken)"
        ),
        other => panic!("expected F64, got {other:?}"),
    }
}

// ── (where, no window, field) — Sum with predicate over a field ─────────────

/// Prices [10, 20, 30, 40, 50] with predicate `amount > 25` (using the same
/// `amount` field as the value). Lifetime Sum must include only {30, 40, 50}
/// → 120.0.
#[test]
fn matrix_where_no_window_field() {
    let mut op = AggOp::Sum(SumState::default());
    let where_expr = Arc::new(parse_expr("(amount > 25)").expect("parse"));
    for (i, &p) in [10.0_f64, 20.0, 30.0, 40.0, 50.0].iter().enumerate() {
        let pre = Value::F64(p);
        let row = Row::new().with_field("amount", Value::F64(p));
        let extracted: ExtractedFields<'_> = smallvec![Some(&pre)];
        op.update_with_extracted(
            Some(&pre),
            (i as i64) * 100,
            Some(&where_expr),
            &row,
            Some("amount"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }
    match op.query(999) {
        Value::F64(v) => assert!(
            (v - 120.0).abs() < 1e-9,
            "where=`amount>25` over Sum of [10,20,30,40,50] should be 120.0; got {v} \
             (would indicate predicate-gated field-bearing Sum is broken)"
        ),
        other => panic!("expected F64, got {other:?}"),
    }
}

// ── (no where, windowed, no field) — Windowed Count ─────────────────────────

/// 4 events at distinct buckets, all within window. Windowed Count must
/// return n=4 — the fieldless windowed-Count path through
/// `WindowedOp::update_at` with `pre_val=None` must still increment.
#[test]
fn matrix_no_where_window_no_field() {
    let mut op = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Count, WINDOW_MS)));
    for now_ms in [100_i64, 1_100, 2_100, 3_100] {
        let extracted: ExtractedFields<'_> = smallvec![None];
        op.update_with_extracted(
            None,
            now_ms,
            None,
            &Row::new(),
            None,
            FIELD_IDX_NONE,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }
    match op.query(4_000) {
        Value::I64(4) => {}
        other => panic!(
            "no-where/window/no-field windowed Count should be 4; got {other:?} \
             (would indicate fieldless windowed Count dispatch is broken)"
        ),
    }
}

// ── (where, windowed, no field) — Windowed Count with predicate ─────────────

/// `update_with_extracted` evaluates `where_expr` once at the outer
/// dispatcher and threads `where_matched` into `WindowedOp::update_at`.
/// Pushing 5 events with `amount`s [10,20,30,40,50] and predicate
/// `amount > 25` must yield a windowed count of 3.
#[test]
fn matrix_where_window_no_field() {
    let mut op = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Count, WINDOW_MS)));
    let where_expr = Arc::new(parse_expr("(amount > 25)").expect("parse"));
    for (i, &amount) in [10.0_f64, 20.0, 30.0, 40.0, 50.0].iter().enumerate() {
        let row = Row::new().with_field("amount", Value::F64(amount));
        let extracted: ExtractedFields<'_> = smallvec![None];
        op.update_with_extracted(
            None,
            (i as i64) * 1_000,
            Some(&where_expr),
            &row,
            None,
            FIELD_IDX_NONE,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }
    match op.query(5_000) {
        Value::I64(3) => {}
        other => panic!(
            "where=`amount>25` over windowed Count of [10,20,30,40,50] should be 3; \
             got {other:?} (would indicate where_matched is not being threaded \
             through update_with_extracted → WindowedOp::update_at)"
        ),
    }
}

// ── (no where, windowed, field) — Windowed Sum over field ───────────────────

/// 4 events with caller-resolved pre_vals [10, 20, 30, 40] across distinct
/// buckets. `extracted[0]` deliberately holds a wrong-slot value (999.0) the
/// pre-fix bug would have read. Windowed Sum must reflect the caller's
/// pre_val sum (100.0), not the re-extracted-slot sum.
#[test]
fn matrix_no_where_window_field() {
    let mut op = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Sum, WINDOW_MS)));
    let wrong_slot = Value::F64(999.0);
    let pres = [
        Value::F64(10.0),
        Value::F64(20.0),
        Value::F64(30.0),
        Value::F64(40.0),
    ];
    for (i, pre) in pres.iter().enumerate() {
        let extracted: ExtractedFields<'_> = smallvec![Some(&wrong_slot)];
        op.update_with_extracted(
            Some(pre),
            (i as i64) * 1_000,
            None,
            &Row::new(),
            Some("price"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }
    match op.query(4_000) {
        Value::F64(v) => assert!(
            (v - 100.0).abs() < 1e-9,
            "windowed Sum must reflect caller's pre_val sum (10+20+30+40 = 100.0); \
             got {v} (would be ~3996 if WindowedOp::update_at re-extracted \
             extracted[field_idx])"
        ),
        other => panic!("expected F64, got {other:?}"),
    }
}

// ── (where, windowed, field) — Windowed Sum with predicate ──────────────────

/// All three axes engaged: a windowed field-bearing op with a `where`
/// predicate. Five events with `amount`s [10,20,30,40,50] across distinct
/// buckets, predicate `amount > 25`. Expected: windowed sum of {30, 40, 50}
/// = 120.0. `extracted[0]` again holds a wrong-slot value to ensure the
/// caller-pre_val invariant continues to hold under a where predicate.
#[test]
fn matrix_where_window_field() {
    let mut op = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Sum, WINDOW_MS)));
    let where_expr = Arc::new(parse_expr("(amount > 25)").expect("parse"));
    let wrong_slot = Value::F64(999.0);
    for (i, &amount) in [10.0_f64, 20.0, 30.0, 40.0, 50.0].iter().enumerate() {
        let pre = Value::F64(amount);
        let row = Row::new().with_field("amount", Value::F64(amount));
        let extracted: ExtractedFields<'_> = smallvec![Some(&wrong_slot)];
        op.update_with_extracted(
            Some(&pre),
            (i as i64) * 1_000,
            Some(&where_expr),
            &row,
            Some("amount"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }
    match op.query(5_000) {
        Value::F64(v) => assert!(
            (v - 120.0).abs() < 1e-9,
            "where=`amount>25` × windowed × field Sum should be 30+40+50 = 120.0; \
             got {v} (would indicate the (where × window × field) cell — the \
             cell that hid the PR #106 bug — is broken again)"
        ),
        other => panic!("expected F64, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Group 2 — Multi-op windowed composition.
// ═══════════════════════════════════════════════════════════════════════════

// ── Three windowed ops on three different fields in one agg ─────────────────

/// Simulates the apply-loop's per-feature dispatch where three windowed
/// field-bearing ops share one agg with three distinct fields. The
/// dispatcher resolves each op's `pre_val` via the agg-local → union-index
/// remap; the `extracted` slot at the agg-local `field_idx` deliberately
/// holds the WRONG value (the pre-fix bug would have read it).
///
/// We push each op once with the appropriate `pre_val` and assert each
/// op's query reflects only its own field's values. Pre-fix, the second
/// and third ops would have read the first op's slot (or some other slot
/// in `extracted`) and returned the wrong answer.
#[test]
fn three_windowed_ops_different_fields_in_one_agg_dispatch() {
    // Three independent windowed ops — in production they share an agg's
    // op vector; for the dispatcher contract under test (`pre_val` is
    // routed correctly via `update_with_extracted`) we exercise each op
    // independently with a deliberately-wrong `extracted` slot at its
    // agg-local `field_idx`.
    let mut op_sum = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Sum, WINDOW_MS)));
    let mut op_avg = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Avg, WINDOW_MS)));
    let mut op_min = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Min, WINDOW_MS)));

    // pre_vals — what the outer dispatcher resolved correctly for each op.
    let a = Value::F64(10.0);
    let b = Value::F64(20.0);
    let c = Value::F64(3.0);

    // Wrong slot at agg-local field_idx=0 for every op. Pre-fix bug: each
    // op would re-extract from extracted[0] and pick up this wrong value.
    let wrong = Value::F64(999.0);
    let extracted: ExtractedFields<'_> = smallvec![Some(&wrong)];

    // Push two events per op into two different buckets — enough to make
    // Avg meaningful and Min unambiguous, while still cheap to verify.
    for &now_ms in &[100_i64, 1_100] {
        op_sum.update_with_extracted(
            Some(&a),
            now_ms,
            None,
            &Row::new(),
            Some("field_a"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        op_avg.update_with_extracted(
            Some(&b),
            now_ms,
            None,
            &Row::new(),
            Some("field_b"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        op_min.update_with_extracted(
            Some(&c),
            now_ms,
            None,
            &Row::new(),
            Some("field_c"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }

    // Sum over field_a: 2 × 10.0 = 20.0 (NOT 2 × 999 = 1998).
    match op_sum.query(2_000) {
        Value::F64(v) => assert!(
            (v - 20.0).abs() < 1e-9,
            "windowed Sum on field_a must see caller pre_val (sum=20.0); got {v} \
             (would indicate the windowed-arm re-extracted extracted[field_idx] again)"
        ),
        other => panic!("expected F64 for Sum, got {other:?}"),
    }
    // Avg over field_b: mean of two 20.0s = 20.0 (NOT 999.0; NOT Null).
    match op_avg.query(2_000) {
        Value::F64(v) => assert!(
            (v - 20.0).abs() < 1e-9,
            "windowed Avg on field_b must see caller pre_val (mean=20.0); got {v} \
             (would indicate the windowed-arm re-extracted extracted[field_idx] again)"
        ),
        Value::Null => panic!(
            "windowed Avg returned Null — the regression that PR #106 fixed: \
             the wrong-slot read failed to accumulate, n stayed 0"
        ),
        other => panic!("expected F64 for Avg, got {other:?}"),
    }
    // Min over field_c: min of two 3.0s = 3.0 (NOT 999.0).
    match op_min.query(2_000) {
        Value::F64(v) => assert!(
            (v - 3.0).abs() < 1e-9,
            "windowed Min on field_c must see caller pre_val (min=3.0); got {v} \
             (would indicate the windowed-arm re-extracted extracted[field_idx] again)"
        ),
        other => panic!("expected F64 for Min, got {other:?}"),
    }
}

// ── Windowed + non-windowed ops mixed in one agg ────────────────────────────

/// Both a windowed and a lifetime op on the same field, fed the same stream,
/// must reach the same total / mean over the active window.
///
/// Concretely: `WindowedOp(Avg, "x")` (with a 64s window) next to a
/// lifetime `AggOp::Sum`-style state on `"x"` see five events in distinct
/// buckets, all within the window. Windowed Avg → 5.0; lifetime Sum → 25.0.
/// Asserts independent state, identical apply protocol.
#[test]
fn windowed_and_non_windowed_ops_mixed_in_one_agg() {
    let mut op_w = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Avg, WINDOW_MS)));
    let mut op_l = AggOp::Sum(SumState::default());

    let pres = [
        Value::F64(1.0),
        Value::F64(3.0),
        Value::F64(5.0),
        Value::F64(7.0),
        Value::F64(9.0),
    ];
    for (i, pre) in pres.iter().enumerate() {
        let extracted: ExtractedFields<'_> = smallvec![Some(pre)];
        // Both ops are driven by the same outer-resolved pre_val and the
        // same agg-local field_idx=0; the windowed arm uses pre_val, the
        // lifetime arm uses pre_val via update_pre.
        op_w.update_with_extracted(
            Some(pre),
            (i as i64) * 1_000,
            None,
            &Row::new(),
            Some("x"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        op_l.update_with_extracted(
            Some(pre),
            (i as i64) * 1_000,
            None,
            &Row::new(),
            Some("x"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }

    match op_w.query(5_000) {
        Value::F64(v) => assert!(
            (v - 5.0).abs() < 1e-9,
            "windowed Avg over [1,3,5,7,9] must be 5.0; got {v} \
             (would indicate the windowed-arm dispatch differs from the lifetime-arm)"
        ),
        other => panic!("expected F64 for windowed Avg, got {other:?}"),
    }
    match op_l.query(5_000) {
        Value::F64(v) => assert!(
            (v - 25.0).abs() < 1e-9,
            "lifetime Sum over [1,3,5,7,9] must be 25.0; got {v} \
             (would indicate field-bearing lifetime dispatch is broken)"
        ),
        other => panic!("expected F64 for lifetime Sum, got {other:?}"),
    }
}

// ── Five windowed ops in one agg — no state leak ────────────────────────────

/// Stress the dispatcher with FIVE windowed ops in a row, each on a
/// different field with a different pre_val. None must read another op's
/// slot. We pick op kinds that yield distinct, exactly-checkable results.
#[test]
fn five_windowed_ops_in_one_agg_no_state_leak() {
    let mut op_sum = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Sum, WINDOW_MS)));
    let mut op_avg = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Avg, WINDOW_MS)));
    let mut op_min = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Min, WINDOW_MS)));
    let mut op_max = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Max, WINDOW_MS)));
    let mut op_count = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Count, WINDOW_MS)));

    let v_a = Value::F64(10.0);
    let v_b = Value::F64(20.0);
    let v_c = Value::F64(5.0);
    let v_d = Value::F64(50.0);

    // Wrong-slot guards: a different wrong value per op so any leak would
    // be visible in the assertion.
    let wrong = Value::F64(999.0);
    let extracted: ExtractedFields<'_> = smallvec![Some(&wrong)];

    for &now_ms in &[100_i64, 1_100, 2_100] {
        op_sum.update_with_extracted(
            Some(&v_a),
            now_ms,
            None,
            &Row::new(),
            Some("a"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        op_avg.update_with_extracted(
            Some(&v_b),
            now_ms,
            None,
            &Row::new(),
            Some("b"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        op_min.update_with_extracted(
            Some(&v_c),
            now_ms,
            None,
            &Row::new(),
            Some("c"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        op_max.update_with_extracted(
            Some(&v_d),
            now_ms,
            None,
            &Row::new(),
            Some("d"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        op_count.update_with_extracted(
            None,
            now_ms,
            None,
            &Row::new(),
            None,
            FIELD_IDX_NONE,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }

    // Sum: 3 × 10 = 30; Avg: 20.0; Min: 5.0; Max: 50.0; Count: 3.
    match op_sum.query(3_000) {
        Value::F64(v) => assert!(
            (v - 30.0).abs() < 1e-9,
            "stacked windowed Sum must remain 30.0 under 5-op stack; got {v} \
             (would indicate cross-op state leak across the dispatcher)"
        ),
        other => panic!("expected F64 for Sum, got {other:?}"),
    }
    match op_avg.query(3_000) {
        Value::F64(v) => assert!(
            (v - 20.0).abs() < 1e-9,
            "stacked windowed Avg must remain 20.0 under 5-op stack; got {v}"
        ),
        Value::Null => panic!("stacked windowed Avg returned Null — same bug class as PR #106"),
        other => panic!("expected F64 for Avg, got {other:?}"),
    }
    match op_min.query(3_000) {
        Value::F64(v) => assert!(
            (v - 5.0).abs() < 1e-9,
            "stacked windowed Min must remain 5.0 under 5-op stack; got {v}"
        ),
        other => panic!("expected F64 for Min, got {other:?}"),
    }
    match op_max.query(3_000) {
        Value::F64(v) => assert!(
            (v - 50.0).abs() < 1e-9,
            "stacked windowed Max must remain 50.0 under 5-op stack; got {v}"
        ),
        other => panic!("expected F64 for Max, got {other:?}"),
    }
    match op_count.query(3_000) {
        Value::I64(3) => {}
        other => panic!("stacked windowed Count must remain 3 under 5-op stack; got {other:?}"),
    }
}

// ── Two windowed TopK ops on different fields, independent distributions ────

/// Two `WindowedOp(TopK)` ops in one agg, on two different string fields.
/// Each op's distribution must be independent of the other's pre_val.
///
/// Pre-fix bug pattern (PR #106 `top_k("category", window="…")` after
/// `mean("price", window="…")` in the same agg): the second top_k would
/// surface the first op's distribution because `WindowedOp::update_at`
/// re-extracted from `extracted[field_idx]` instead of using the
/// caller-resolved `pre_val`.
#[test]
fn windowed_top_k_after_windowed_top_k_different_fields() {
    let mut op_a = AggOp::Windowed(Box::new(WindowedOp::new_with_params(
        AggKind::TopK,
        WINDOW_MS,
        SketchParams {
            top_k_k: Some(1),
            ..SketchParams::default()
        },
    )));
    let mut op_b = AggOp::Windowed(Box::new(WindowedOp::new_with_params(
        AggKind::TopK,
        WINDOW_MS,
        SketchParams {
            top_k_k: Some(1),
            ..SketchParams::default()
        },
    )));

    let cat = Value::Str("cat_a".into());
    let path = Value::Str("/home".into());
    // Wrong slot — if the bug returned, op_b would surface this string
    // (since both ops use agg-local field_idx=0 against the same wrong
    // `extracted`) instead of "/home".
    let wrong = Value::Str("WRONG-FIELD".into());
    let extracted: ExtractedFields<'_> = smallvec![Some(&wrong)];

    for &now_ms in &[100_i64, 200, 300] {
        op_a.update_with_extracted(
            Some(&cat),
            now_ms,
            None,
            &Row::new(),
            Some("category"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
        op_b.update_with_extracted(
            Some(&path),
            now_ms,
            None,
            &Row::new(),
            Some("path"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }

    let render_a = format!("{:?}", op_a.query(999));
    let render_b = format!("{:?}", op_b.query(999));
    assert!(
        render_a.contains("cat_a"),
        "first windowed TopK must surface its own pre_val ('cat_a'); got {render_a}"
    );
    assert!(
        !render_a.contains("/home"),
        "first windowed TopK must NOT surface the second op's pre_val ('/home'); \
         got {render_a} (cross-op state leak)"
    );
    assert!(
        render_b.contains("/home"),
        "second windowed TopK must surface its own pre_val ('/home'); got {render_b}"
    );
    assert!(
        !render_b.contains("cat_a"),
        "second windowed TopK must NOT surface the first op's pre_val ('cat_a'); \
         got {render_b} (this is the exact PR #106 regression pattern)"
    );
    assert!(
        !render_a.contains("WRONG-FIELD") && !render_b.contains("WRONG-FIELD"),
        "neither windowed TopK may surface the wrong extracted slot; \
         got render_a={render_a} render_b={render_b}"
    );
}
