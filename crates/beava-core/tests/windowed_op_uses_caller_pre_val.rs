//! Regression: `WindowedOp::update_at` must honour the `pre_val` resolved by
//! the outer caller, not re-extract it from `ExtractedFields` using
//! `field_idx` as the index.
//!
//! The outer apply-loop (`agg_apply::apply_event_to_aggregations`) computes
//! `pre_val` via the agg-local → union-index remap
//! (`feat.descriptor.field_idx_into_event_extracted[field_idx]`). The
//! `field_idx` it then threads down to `update_with_extracted` is the
//! agg-LOCAL position; `extracted` is indexed by the source-wide UNION. The
//! two index spaces only coincide when every agg's field_names list happens
//! to be in the same order as the source union.
//!
//! Pre-fix bug: `WindowedOp::update_at` re-extracts `extracted[field_idx]`,
//! reading the WRONG slot for any windowed field-bearing op whose agg-local
//! index differs from the union index. Symptoms seen end-to-end:
//!   - `mean("price", window="…")` returns `Null` because it reads a string
//!     slot, fails to accumulate, and `n` stays 0.
//!   - `top_k("category", window="…")` after `mean("price", window="…")` in
//!     the same agg returns the price distribution instead of the category
//!     distribution.
//!
//! These tests RED on current main and GREEN after the fix that routes the
//! caller's pre_val through the windowed arm.

use beava_core::agg_buffer::EventTypeMixState;
use beava_core::agg_op::{AggKind, AggOp, ExtractedFields, SketchParams, FIELD_IDX_NONE};
use beava_core::agg_windowed::WindowedOp;
use beava_core::row::{Row, Value};
use smallvec::smallvec;

const WINDOW_MS: u64 = 64_000;

// ── Test 1: windowed Sum honours caller's pre_val ───────────────────────────

/// Caller passes `pre_val = Some(&10.0)` (resolved via the agg-local → union
/// remap) and `field_idx = 0`. `extracted[0]` deliberately holds a DIFFERENT
/// value (999.0) — the simulated "wrong slot" the bug would read. After two
/// events, the windowed Sum must reflect the caller-resolved value (20.0),
/// not the re-extracted one (1998.0).
#[test]
fn windowed_sum_uses_caller_pre_val_not_extracted_field_idx() {
    let mut op = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Sum, WINDOW_MS)));

    let true_val = Value::F64(10.0);
    let wrong_slot = Value::F64(999.0);
    let extracted: ExtractedFields<'_> = smallvec![Some(&wrong_slot)];

    for now_ms in [100_i64, 200] {
        op.update_with_extracted(
            Some(&true_val),
            now_ms,
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
            (v - 20.0).abs() < 1e-9,
            "windowed Sum must reflect caller's pre_val (2 × 10.0 = 20.0); got {v} \
             (would be ~1998.0 if update_at re-extracted from extracted[field_idx])"
        ),
        other => panic!("expected F64, got {other:?}"),
    }
}

// ── Test 2: windowed Avg returns the resolved value, not Null ───────────────

/// Mirrors the `mean("price", window="1h")` regression. Pre-fix the windowed
/// Avg would read a non-numeric (or otherwise wrong) slot, fail to
/// accumulate, leave `n = 0`, and query would return `Null`. With the fix it
/// must return the mean of the caller-resolved pre_vals.
#[test]
fn windowed_avg_uses_caller_pre_val_and_does_not_return_null() {
    let mut op = AggOp::Windowed(Box::new(WindowedOp::new(AggKind::Avg, WINDOW_MS)));

    let v1 = Value::F64(10.0);
    let v2 = Value::F64(20.0);
    let wrong_slot = Value::Str("not-a-number".into());
    let extracted: ExtractedFields<'_> = smallvec![Some(&wrong_slot)];

    op.update_with_extracted(
        Some(&v1),
        100,
        None,
        &Row::new(),
        Some("price"),
        0,
        &extracted,
        FIELD_IDX_NONE,
        FIELD_IDX_NONE,
    );
    op.update_with_extracted(
        Some(&v2),
        200,
        None,
        &Row::new(),
        Some("price"),
        0,
        &extracted,
        FIELD_IDX_NONE,
        FIELD_IDX_NONE,
    );

    match op.query(999) {
        Value::F64(v) => assert!(
            (v - 15.0).abs() < 1e-9,
            "windowed Avg must reflect caller's pre_val mean (15.0); got {v}"
        ),
        Value::Null => panic!(
            "windowed Avg returned Null — the bug: update_at re-extracted a \
             non-numeric slot, n stayed 0, query short-circuited to Null"
        ),
        other => panic!("expected F64, got {other:?}"),
    }
}

// ── Test 3: windowed TopK captures the caller-resolved field ────────────────

/// Mirrors the `top_k("category", window="…")` regression seen when an
/// earlier windowed op in the same agg referenced a different field. The
/// caller's pre_val resolves to a string (`"cat_a"`); the extracted slot at
/// `field_idx` deliberately holds a different value (a float). With the fix,
/// TopK's most-frequent value must be the caller-resolved string, not the
/// re-extracted float.
#[test]
fn windowed_top_k_captures_caller_pre_val_not_extracted_field_idx() {
    let params = SketchParams {
        top_k_k: Some(1),
        ..SketchParams::default()
    };
    let mut op = AggOp::Windowed(Box::new(WindowedOp::new_with_params(
        AggKind::TopK,
        WINDOW_MS,
        params,
    )));

    let intended = Value::Str("cat_a".into());
    let wrong_slot = Value::F64(220.0);
    let extracted: ExtractedFields<'_> = smallvec![Some(&wrong_slot)];

    for now_ms in [100_i64, 200, 300] {
        op.update_with_extracted(
            Some(&intended),
            now_ms,
            None,
            &Row::new(),
            Some("category"),
            0,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
        );
    }

    let result = op.query(999);
    // Result shape per existing TopK contract: Value::Json(Array([{count, value}, ...]))
    // We assert against the rendered string for resilience to internal
    // representation tweaks: the resolved category must appear, the wrong
    // numeric slot must not.
    let rendered = format!("{result:?}");
    assert!(
        rendered.contains("cat_a"),
        "windowed TopK must surface the caller's pre_val ('cat_a'); got {rendered}"
    );
    assert!(
        !rendered.contains("220"),
        "windowed TopK must NOT surface the wrong extracted slot (220.0); got {rendered}"
    );
}

// ── Test 4: EventTypeMix honours caller's pre_val (same bug class) ──────────

/// `EventTypeMixState::update_at` was reading
/// `extracted[field_idx]` directly, with the same agg-local-vs-union
/// confusion as `WindowedOp::update_at`. The dispatcher at
/// `AggOp::update_with_extracted_no_where` for `AggOp::EventTypeMix` had
/// already computed the correct `pre_val`; the windowed and event-type-mix
/// arms were the only two that ignored it.
///
/// Same setup as the windowed tests: caller passes the correct `pre_val`
/// (`"CORRECT"`), but `extracted[field_idx]` holds a different value
/// (`"WRONG"`). After the fix, EventTypeMix must count `"CORRECT"`.
#[test]
fn event_type_mix_uses_caller_pre_val_not_extracted_field_idx() {
    let mut op = AggOp::EventTypeMix(Box::new(EventTypeMixState::new(10, None)));

    let correct = Value::Str("CORRECT".into());
    let wrong = Value::Str("WRONG".into());
    // Two-slot extracted; the wrong value is at the slot field_idx points to.
    let extracted: ExtractedFields<'_> = smallvec![Some(&wrong), Some(&correct)];

    op.update_with_extracted(
        Some(&correct),
        100,
        None,
        &Row::new(),
        Some("event_type"),
        0,
        &extracted,
        FIELD_IDX_NONE,
        FIELD_IDX_NONE,
    );

    match op.query(999) {
        Value::Map(m) => {
            assert!(
                m.contains_key("CORRECT"),
                "EventTypeMix must count caller's pre_val ('CORRECT'); got {m:?}"
            );
            assert!(
                !m.contains_key("WRONG"),
                "EventTypeMix must NOT count the wrong extracted slot ('WRONG'); got {m:?}"
            );
        }
        other => panic!("expected Value::Map, got {other:?}"),
    }
}
