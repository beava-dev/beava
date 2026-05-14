//! Windowed<Op> wrapper: 64-bucket event-time tumbling ring buffer.
//!
//! # Requirements traceability
//! - AGG-CORE-09: Windowed<Op> with 64-bucket event-time tumbling
//!
//! Bucket index = `floor(t / bucket_ms) mod 64` via `div_euclid` — no
//! wall-clock reads, no rand — pure event-time determinism.
//!
//! # Lazy bucket allocation
//!
//! The old layout pre-allocated `[Option<Box<AggOp>>; 64]` + `[i64; 64]` for
//! every WindowedOp instance — ~1024 bytes of zero-init memory per instance.
//! With 4-14 windowed ops per entity in fraud-team-shape pipelines, this was
//! ~60% of the cold-key entity init cost (~1500 ns / 2576 ns mean).
//!
//! The current layout is `SmallVec<[(i64, Box<AggOp>); 4]>` + lazy
//! allocation. Most entities only see 1-2 active buckets at any given
//! moment; the 4-slot inline SmallVec covers the typical case without heap
//! allocation. The 64-bucket cap from AGG-CORE-09 is enforced by
//! oldest-epoch eviction on each new-epoch insert once
//! `buckets.len() >= max_buckets`.
//!
//! Bucket lookup is a linear scan of the SmallVec by epoch (typical
//! 1-2 active = effectively O(1); worst-case 64 entries × ~0.5 ns scan ≈
//! 32 ns — still cheap vs the saved ~1500 ns cold-init).
//!
//! Snapshot format: serde representation of `SmallVec<[(i64, Box<AggOp>); 4]>`
//! is incompatible with the OLD `[Option<Box<AggOp>>; 64]` + `[i64; 64]`
//! representation. Recovery falls back to WAL replay if snapshot
//! deserialization fails. Operators with existing snapshots: delete the
//! snapshot file before restart; the WAL will replay the missing state.

use crate::agg_op::{AggKind, AggOp, SketchParams};
use crate::row::Row;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// 64-bucket event-time tumbling ring buffer wrapping any core AggOp.
///
/// AGG-CORE-09: Windowed<Op> with 64 tumbling event-time buckets.
/// `bucket_ms = ceil(window_ms / 64)`. On update: route to the bucket whose
/// epoch matches the event time, lazily creating it if needed; evict the
/// oldest-epoch entry once `buckets.len() >= max_buckets`. On query: fold
/// active buckets (those with epoch ∈ [query_time - window_ms, query_time])
/// using op-specific combine logic (Welford pairwise for variance/stddev).
///
/// Lazy `SmallVec<[(epoch_ms, Box<AggOp>); 4]>` replaces the original
/// `[Option<Box<AggOp>>; 64]` + `[i64; 64]` arrays for ~60% reduction in
/// cold WindowedOp::new cost on complex pipelines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowedOp {
    pub inner_kind: AggKind,
    pub bucket_ms: u64,
    pub window_ms: u64,
    /// Lazy-allocated bucket entries: `(epoch_start_ms, op_state)`.
    /// Inline cap 4 covers the typical fraud workload (1-2 active buckets
    /// per entity at any time); spills to heap above 4. Cap of 64 active
    /// buckets (AGG-CORE-09) enforced by oldest-epoch eviction in `update`.
    pub buckets: SmallVec<[(i64, Box<AggOp>); 4]>,
    /// AGG-CORE-09: cap = 64 active buckets. Beyond cap, the oldest-epoch
    /// entry is evicted on each new-epoch insert.
    #[serde(default = "default_max_buckets")]
    pub max_buckets: usize,
    /// Sketch construction params propagated to per-bucket fresh_op() calls.
    /// Default for non-sketch kinds; threaded for sketch kinds.
    #[serde(default)]
    pub sketch_params: SketchParams,
}

fn default_max_buckets() -> usize {
    64
}

impl WindowedOp {
    /// Create a new WindowedOp.
    ///
    /// `bucket_ms = ceil(window_ms / 64)` — ensures at least 1ms per bucket.
    ///
    /// Cold construction is allocation-free (`SmallVec::new` is a no-op).
    /// Buckets are pushed lazily on the first event.
    pub fn new(kind: AggKind, window_ms: u64) -> Self {
        Self::new_with_params(kind, window_ms, SketchParams::default())
    }

    /// Construct with explicit sketch params (k, q, fpr, etc.). Sketch
    /// params are persisted on the WindowedOp so each bucket re-init honors
    /// user-supplied configuration.
    ///
    /// Panics for `AggKind::BloomMember` — bloom_member is windowless-only
    /// (rejected at register time with kind=window_not_supported).
    pub fn new_with_params(kind: AggKind, window_ms: u64, sketch_params: SketchParams) -> Self {
        assert!(
            !matches!(kind, AggKind::BloomMember),
            "bloom_member is windowless-only — cannot be wrapped in WindowedOp"
        );
        let bucket_ms = window_ms.div_ceil(64);
        WindowedOp {
            inner_kind: kind,
            bucket_ms,
            window_ms,
            // Lazy allocation: SmallVec::new is allocation-free. Buckets push
            // on first event into a new epoch.
            buckets: SmallVec::new(),
            max_buckets: 64,
            sketch_params,
        }
    }

    /// Compute the bucket index (slot 0..64) for an event time.
    ///
    /// Uses `div_euclid` so negative now_ms yields a non-negative index.
    ///
    /// This is a pure mathematical function returning
    /// `floor(t / bucket_ms) mod 64`. It is not used to address physical
    /// storage slots (the SmallVec is keyed by epoch_ms, not slot index),
    /// but is kept as a public API for tests and external callers that
    /// reason about bucket-collision behavior at the 64-slot abstraction level.
    pub fn bucket_index(&self, now_ms: i64) -> usize {
        ((now_ms.div_euclid(self.bucket_ms as i64)) as usize) % 64
    }

    /// Compute the bucket epoch (start time in ms, inclusive) for an event.
    ///
    /// The bucket identifier in the SmallVec layout. Two events at times
    /// `t1` and `t2` share a bucket iff
    /// `bucket_epoch(t1) == bucket_epoch(t2)`.
    #[inline]
    pub fn bucket_epoch(&self, now_ms: i64) -> i64 {
        now_ms.div_euclid(self.bucket_ms as i64) * self.bucket_ms as i64
    }

    /// Find the position in `self.buckets` for a given epoch, if any.
    ///
    /// Linear scan is fastest for n ≤ 4 (typical case is 1-2 active buckets;
    /// worst case 64 still has small constant factor — ~32 ns at scan-of-64).
    #[inline]
    fn position_for_epoch(&self, epoch: i64) -> Option<usize> {
        self.buckets.iter().position(|(e, _)| *e == epoch)
    }

    /// Evict the oldest-epoch bucket. Called when `len >= max_buckets` and a
    /// new-epoch entry is about to be pushed. AGG-CORE-09: 64-bucket cap.
    fn evict_oldest_bucket(&mut self) {
        // Phase 12.8 memory-governance: bump the process-static
        // bucket-reclaim counter for the /metrics endpoint
        // (`beava_bucket_reclaim_total`). Inline atomic fetch_add —
        // Relaxed ordering, no allocation.
        crate::agg_state::BucketReclaimCounter::inc();
        if let Some(min_pos) = self
            .buckets
            .iter()
            .enumerate()
            .min_by_key(|(_, (e, _))| *e)
            .map(|(i, _)| i)
        {
            self.buckets.swap_remove(min_pos);
        }
    }

    /// Update the windowed state with one event row.
    pub fn update(&mut self, row: &Row, now_ms: i64, field: Option<&str>, where_matched: bool) {
        let epoch = self.bucket_epoch(now_ms);
        if let Some(pos) = self.position_for_epoch(epoch) {
            self.buckets[pos]
                .1
                .update(row, now_ms, field, where_matched);
            return;
        }
        // New epoch — evict-then-push if at cap.
        if self.buckets.len() >= self.max_buckets {
            self.evict_oldest_bucket();
        }
        let mut new_op = Box::new(fresh_op(self.inner_kind, &self.sketch_params));
        new_op.update(row, now_ms, field, where_matched);
        self.buckets.push((epoch, new_op));
    }

    /// Update the windowed state with one event row, evaluating `where_expr`
    /// (if any) before forwarding to the inner bucket's AggOp.
    ///
    /// Same bucket routing + lazy-allocation logic as `update`; the predicate is
    /// threaded into the per-bucket `AggOp::update_with_row` call.
    ///
    /// # SDK-AGG-04
    pub fn update_with_row(
        &mut self,
        row: &Row,
        now_ms: i64,
        field: Option<&str>,
        where_expr: Option<&std::sync::Arc<crate::expr::Expr>>,
    ) {
        let epoch = self.bucket_epoch(now_ms);
        if let Some(pos) = self.position_for_epoch(epoch) {
            self.buckets[pos]
                .1
                .update_with_row(row, now_ms, field, where_expr);
            return;
        }
        if self.buckets.len() >= self.max_buckets {
            self.evict_oldest_bucket();
        }
        let mut new_op = Box::new(fresh_op(self.inner_kind, &self.sketch_params));
        new_op.update_with_row(row, now_ms, field, where_expr);
        self.buckets.push((epoch, new_op));
    }

    /// Pre-extracted fast-path mirroring
    /// `AggOp::update_with_extracted_no_where` across the WindowedOp wrapper.
    ///
    /// Per-bucket inner op dispatches via
    /// `AggOp::update_with_extracted_no_where` (NOT `update_with_row`),
    /// preserving the pre-extraction protocol across the wrapper boundary.
    /// Eliminates inner `row.get(fname)` linear scan + per-bucket
    /// `evaluate_where_predicate` re-evaluation.
    ///
    /// `where_matched` is computed once by the outer dispatcher
    /// (`AggOp::update_with_extracted`) and threaded down — no per-bucket
    /// re-evaluation.
    ///
    /// `field_idx`/`lat_idx`/`lon_idx` use `FIELD_IDX_NONE` (u8::MAX) as the
    /// "no field" sentinel, matching the existing protocol.
    ///
    /// Inner ops are restricted to `AggKind::supports_windowed_wrap()` kinds:
    /// Count, Sum, Avg, Min, Max, Variance, StdDev, Ratio, CountDistinct,
    /// Percentile, TopK, Entropy. None of these consult `&Row` content
    /// directly on the apply hot path; they either use `pre_val` (Sum/Avg/
    /// Min/Max/Variance/StdDev/CountDistinct/Percentile/TopK/Entropy via
    /// `update_pre`) or are fieldless (Count/Ratio). Passing an empty `Row`
    /// + `None` field down through the bucket dispatch is therefore safe.
    // reason: per-bucket hot-path dispatch; see AggOp::update_with_extracted
    // for the rationale on threading independent parameters rather than a
    // struct-bag.
    #[allow(clippy::too_many_arguments)]
    pub fn update_at(
        &mut self,
        pre_val: Option<&crate::row::Value>,
        extracted: &crate::agg_op::ExtractedFields<'_>,
        field_idx: u8,
        lat_idx: u8,
        lon_idx: u8,
        now_ms: i64,
        where_matched: bool,
    ) {
        // `pre_val` is supplied by the outer dispatcher
        // (`AggOp::update_with_extracted_no_where`) which resolves it via the
        // agg-local → union-index remap
        // (`feat.descriptor.field_idx_into_event_extracted`). The `field_idx`
        // we still receive is agg-local; `extracted` is indexed by the
        // source-wide union — the two index spaces only coincide by
        // accident. Re-extracting `extracted[field_idx]` here would silently
        // read the wrong slot for any windowed field-bearing op whose
        // agg-local index differs from its union index.
        // Synthetic empty row for arms that take `&Row` for type-signature
        // reasons but ignore content for windowable kinds (Count/Ratio etc.).
        let empty_row = crate::row::Row::new();

        let epoch = self.bucket_epoch(now_ms);
        if let Some(pos) = self.position_for_epoch(epoch) {
            self.buckets[pos].1.update_with_extracted_no_where(
                pre_val,
                now_ms,
                &empty_row,
                None,
                field_idx,
                extracted,
                lat_idx,
                lon_idx,
                where_matched,
            );
            return;
        }
        // New epoch — evict-then-push if at cap.
        if self.buckets.len() >= self.max_buckets {
            self.evict_oldest_bucket();
        }
        let mut new_op = Box::new(fresh_op(self.inner_kind, &self.sketch_params));
        new_op.update_with_extracted_no_where(
            pre_val,
            now_ms,
            &empty_row,
            None,
            field_idx,
            extracted,
            lat_idx,
            lon_idx,
            where_matched,
        );
        self.buckets.push((epoch, new_op));
    }

    /// Query the windowed aggregation value at `query_time_ms`.
    ///
    /// Active buckets: those where `query_time_ms - bucket_epoch >= 0`
    /// AND `query_time_ms - bucket_epoch < window_ms`.
    pub fn query(&self, query_time_ms: i64) -> crate::row::Value {
        use crate::agg_op::AggOp;
        use crate::agg_state::value_lt;
        use crate::agg_state::{AvgState, CountState, MaxState, MinState, RatioState, SumState};
        use crate::row::Value;

        let window_ms = self.window_ms as i64;

        // Helper closure: is a bucket epoch active at the query time?
        // Inlined here so each match arm can use it without re-borrowing self.
        let active = |epoch: i64| -> bool {
            let age = query_time_ms - epoch;
            age >= 0 && age < window_ms
        };

        match self.inner_kind {
            AggKind::Count => {
                let mut total: u64 = 0;
                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    if let AggOp::Count(CountState { n }) = op.as_ref() {
                        total += n;
                    }
                }
                Value::I64(total as i64)
            }
            AggKind::Sum => {
                let mut total = 0.0_f64;
                let mut seen = false;
                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    if let AggOp::Sum(SumState { total: t, n }) = op.as_ref() {
                        if *n > 0 {
                            total += t;
                            seen = true;
                        }
                    }
                }
                if seen {
                    Value::F64(total)
                } else {
                    Value::Null
                }
            }
            AggKind::Avg => {
                let mut sum = 0.0_f64;
                let mut n: u64 = 0;
                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    if let AggOp::Avg(AvgState { sum: s, n: bn }) = op.as_ref() {
                        sum += s;
                        n += bn;
                    }
                }
                if n == 0 {
                    Value::Null
                } else {
                    Value::F64(sum / n as f64)
                }
            }
            AggKind::Min => {
                let mut current: Option<Value> = None;
                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    if let AggOp::Min(MinState { current: Some(bv) }) = op.as_ref() {
                        match &current {
                            None => current = Some(bv.clone()),
                            Some(cur) => {
                                if value_lt(bv, cur) {
                                    current = Some(bv.clone());
                                }
                            }
                        }
                    }
                }
                current.unwrap_or(Value::Null)
            }
            AggKind::Max => {
                let mut current: Option<Value> = None;
                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    if let AggOp::Max(MaxState { current: Some(bv) }) = op.as_ref() {
                        match &current {
                            None => current = Some(bv.clone()),
                            Some(cur) => {
                                if value_lt(cur, bv) {
                                    current = Some(bv.clone());
                                }
                            }
                        }
                    }
                }
                current.unwrap_or(Value::Null)
            }
            AggKind::Variance | AggKind::StdDev => {
                // Welford pairwise merge across active buckets.
                let mut combined_n: u64 = 0;
                let mut combined_mean: f64 = 0.0;
                let mut combined_m2: f64 = 0.0;

                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    let bstate = match op.as_ref() {
                        AggOp::Variance(s) | AggOp::StdDev(s) => s,
                        _ => continue,
                    };
                    if bstate.n == 0 {
                        continue;
                    }

                    // Welford pairwise combine:
                    // delta = b_mean - a_mean
                    // new_n = a_n + b_n
                    // new_mean = a_mean + delta * b_n / new_n
                    // new_m2 = a_m2 + b_m2 + delta^2 * a_n * b_n / new_n
                    let delta = bstate.mean - combined_mean;
                    let new_n = combined_n + bstate.n;
                    let new_mean = combined_mean + delta * bstate.n as f64 / new_n as f64;
                    let new_m2 = combined_m2
                        + bstate.m2
                        + delta * delta * combined_n as f64 * bstate.n as f64 / new_n as f64;
                    combined_n = new_n;
                    combined_mean = new_mean;
                    combined_m2 = new_m2;
                }

                if combined_n < 2 {
                    return Value::Null;
                }
                let variance = combined_m2 / (combined_n - 1) as f64;
                if matches!(self.inner_kind, AggKind::StdDev) {
                    Value::F64(variance.sqrt())
                } else {
                    Value::F64(variance)
                }
            }
            AggKind::CountDistinct => {
                // Merge CountDistinctState across active buckets so the
                // distinct count reflects the full window. The legacy
                // "pick latest bucket" pattern caused the displayed value
                // to drop on every bucket rollover (~ window_ms / 64
                // cadence).
                let mut combined: Option<crate::sketches::count_distinct::CountDistinctState> =
                    None;
                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    if let AggOp::CountDistinct(s) = op.as_ref() {
                        match &mut combined {
                            None => combined = Some(s.inner.clone()),
                            Some(c) => c.merge(&s.inner),
                        }
                    }
                }
                match combined {
                    Some(c) => Value::I64(c.estimate() as i64),
                    None => Value::I64(0),
                }
            }
            AggKind::Percentile => {
                // Merge PercentileState across active buckets so the
                // quantile reflects the full window, not just the latest
                // bucket (root cause of beava.dev's `median_dwell_1h`
                // bouncing every ~56s on a 1h window).
                let mut combined: Option<crate::sketches::percentile::PercentileState> = None;
                let mut q: f64 = 0.5;
                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    if let AggOp::Percentile(s) = op.as_ref() {
                        q = s.q;
                        match &mut combined {
                            None => combined = Some(s.inner.clone()),
                            Some(c) => c.merge(&s.inner),
                        }
                    }
                }
                match combined {
                    Some(c) => match c.quantile(q) {
                        Some(v) => Value::F64(v),
                        None => Value::Null,
                    },
                    None => Value::Null,
                }
            }
            AggKind::TopK => {
                // Merge top_k state across active buckets so the result
                // reflects the full window. Without this, the displayed
                // top-1 reset every bucket rollover (~window_ms / 64),
                // visible as the prod beava.dev `top_page_1h` count
                // dropping every ~56s on a 1h window.
                let mut combined: Option<crate::sketches::top_k::TopKState> = None;
                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    if let AggOp::TopK(s) = op.as_ref() {
                        match &mut combined {
                            None => combined = Some(s.inner.clone()),
                            Some(c) => c.merge(&s.inner),
                        }
                    }
                }
                match combined {
                    Some(c) => {
                        let entries: Vec<serde_json::Value> = c
                            .top()
                            .into_iter()
                            .map(|(v, count)| {
                                serde_json::json!({"value": v.to_json(), "count": count})
                            })
                            .collect();
                        Value::Json(serde_json::Value::Array(entries))
                    }
                    None => Value::Json(serde_json::Value::Array(vec![])),
                }
            }
            AggKind::Entropy => {
                // Merge histograms across active buckets via EntropyHistogram::merge.
                use crate::sketches::entropy::EntropyHistogram;
                let mut combined: Option<EntropyHistogram> = None;
                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    if let AggOp::Entropy(s) = op.as_ref() {
                        match &mut combined {
                            None => combined = Some(s.inner.clone()),
                            Some(c) => c.merge(&s.inner),
                        }
                    }
                }
                match combined {
                    Some(c) => Value::F64(c.entropy_bits()),
                    None => Value::F64(0.0),
                }
            }
            AggKind::BloomMember => {
                // Unreachable: BloomMember rejected by new_with_params; defensive.
                Value::Bool(false)
            }
            AggKind::Ratio => {
                let mut matching: u64 = 0;
                let mut total: u64 = 0;
                for (epoch, op) in self.buckets.iter() {
                    if !active(*epoch) {
                        continue;
                    }
                    if let AggOp::Ratio(RatioState {
                        matching: m,
                        total: t,
                    }) = op.as_ref()
                    {
                        matching += m;
                        total += t;
                    }
                }
                if total == 0 {
                    Value::Null
                } else {
                    Value::F64(matching as f64 / total as f64)
                }
            }
            // Lifetime-only ops (point/recency/streak/decay/velocity/buffer/geo)
            // are never wrapped in WindowedOp — compile-time invariant.
            // Catch-all returns Null defensively.
            _ => Value::Null,
        }
    }
}

/// Create a fresh lifetime AggOp for a given kind (used to initialise buckets).
///
/// `WindowedOp` supports Phase 5 core ops + Phase 10 sketch ops (except
/// `BloomMember` which is windowless-only). Phase 8 + 9 + 11 ops are
/// lifetime-only and `agg_compile` validation rejects `window=` for them.
/// `new_lifetime` handles sketches via `sketch_params`; for non-windowable
/// kinds it delegates to `new_lifetime_full` with a default descriptor.
fn fresh_op(kind: AggKind, sketch_params: &SketchParams) -> AggOp {
    AggOp::new_lifetime(kind, Some(sketch_params))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::{Row, Value};

    fn row_f64(field: &str, v: f64) -> Row {
        Row::new().with_field(field, Value::F64(v))
    }

    fn empty_row() -> Row {
        Row::new()
    }

    // ── Bucket configuration ─────────────────────────────────────────────

    #[test]
    fn windowed_count_bucket_ms_is_ceil_window_div_64() {
        // 64_000ms / 64 = 1000ms exactly
        let op = WindowedOp::new(AggKind::Count, 64_000);
        assert_eq!(
            op.bucket_ms, 1_000,
            "64s window / 64 buckets = 1000ms bucket"
        );
    }

    #[test]
    fn windowed_count_1s_window_rounds_up_bucket_ms_to_at_least_1() {
        // 10ms / 64 = 0.15 → ceil = 1
        let op = WindowedOp::new(AggKind::Count, 10);
        assert_eq!(op.bucket_ms, 1, "10ms/64 rounds up to 1ms minimum bucket");
    }

    // ── Bucket index ─────────────────────────────────────────────────────

    #[test]
    fn windowed_count_bucket_index_is_pure_function_of_event_time() {
        let op = WindowedOp::new(AggKind::Count, 64_000); // bucket_ms=1000
                                                          // Same t always returns same index
        let idx_a = op.bucket_index(0);
        let idx_b = op.bucket_index(0);
        assert_eq!(
            idx_a, idx_b,
            "bucket_index must be pure function of event_time"
        );

        // Two events in the same bucket share an index
        let idx_1 = op.bucket_index(500);
        let idx_2 = op.bucket_index(999);
        assert_eq!(
            idx_1, idx_2,
            "500ms and 999ms should share bucket 0 (bucket_ms=1000)"
        );

        // Event at boundary belongs to next bucket
        let idx_3 = op.bucket_index(1_000);
        assert_ne!(idx_1, idx_3, "1000ms should be in next bucket");

        // Indices are mod 64
        let idx_wrap = op.bucket_index(64_000); // epoch 64, mod 64 = 0
        assert_eq!(idx_wrap, 0, "index must wrap via mod 64");
    }

    // ── Count windowing ───────────────────────────────────────────────────

    #[test]
    fn windowed_count_100_events_in_5min_window_returns_100() {
        let window_ms: u64 = 5 * 60 * 1_000; // 300_000ms
        let mut op = WindowedOp::new(AggKind::Count, window_ms);
        let r = empty_row();
        // Push 100 events spread across [0, window_ms)
        for i in 0..100_i64 {
            let t = i * (window_ms as i64 / 100);
            op.update(&r, t, None, true);
        }
        // Query at query_time_ms = window_ms - 1 (all events still active)
        // Use query_time that keeps all buckets alive: epoch of bucket 0 is 0,
        // age = (window_ms - 1) - 0 = window_ms - 1 < window_ms ✓
        let result = op.query(window_ms as i64 - 1);
        assert_eq!(result, Value::I64(100), "all 100 events should be counted");
    }

    #[test]
    fn windowed_count_events_outside_window_excluded() {
        let window_ms: u64 = 64_000; // 64s, bucket_ms = 1000
        let mut op = WindowedOp::new(AggKind::Count, window_ms);
        let r = empty_row();
        // Push 50 events in [0, window_ms)
        for i in 0..50_i64 {
            op.update(&r, i * 1_000, None, true);
        }
        // Query at t = 2 * window_ms: all original buckets have age >= window_ms → excluded
        let result = op.query(2 * window_ms as i64);
        assert_eq!(
            result,
            Value::I64(0),
            "events older than window should be excluded"
        );
    }

    #[test]
    fn windowed_count_bucket_rollover_deterministic() {
        let window_ms: u64 = 64_000; // bucket_ms = 1000
        let mut op = WindowedOp::new(AggKind::Count, window_ms);
        let r = empty_row();

        // Push event at t=0: epoch 0
        op.update(&r, 0, None, true);
        // Query at t=0: age of epoch 0 = 0 < 64_000 ✓
        let r1 = op.query(0);
        assert_eq!(r1, Value::I64(1));

        // Push event at t=window_ms+1: a new epoch beyond the original window
        // (epoch for t=window_ms+1 with bucket_ms=1000: floor(64001/1000)*1000 = 64000)
        op.update(&r, window_ms as i64 + 1, None, true);
        // Query at t=window_ms+1: epoch 0 has age=window_ms+1 >= window_ms → excluded
        // epoch 64000 has age=1 < window_ms → included
        let r2 = op.query(window_ms as i64 + 1);
        assert_eq!(
            r2,
            Value::I64(1),
            "only new event should be counted after rollover"
        );
    }

    // ── Sum windowing ─────────────────────────────────────────────────────

    #[test]
    fn windowed_sum_folds_across_buckets() {
        // 5 rows with amount=10.0 in 5 different buckets within window
        let window_ms: u64 = 64_000; // bucket_ms = 1000
        let mut op = WindowedOp::new(AggKind::Sum, window_ms);
        for i in 0..5_i64 {
            let r = row_f64("amount", 10.0);
            op.update(&r, i * 1_000, Some("amount"), true);
        }
        let result = op.query(4_999); // all 5 events within window
        match result {
            Value::F64(v) => assert!((v - 50.0).abs() < 1e-10, "sum should be 50.0, got {v}"),
            other => panic!("expected F64, got {:?}", other),
        }
    }

    // ── Avg windowing ─────────────────────────────────────────────────────

    #[test]
    fn windowed_avg_weighted_by_bucket_n() {
        // Two buckets: bucket 0 has 1 event (value=10), bucket 1 has 9 events (value=1)
        // Weighted avg = (10 + 9*1) / 10 = 1.9, NOT (10+1)/2 = 5.5
        let window_ms: u64 = 64_000;
        let mut op = WindowedOp::new(AggKind::Avg, window_ms);

        op.update(&row_f64("x", 10.0), 0, Some("x"), true);
        for _ in 0..9 {
            op.update(&row_f64("x", 1.0), 1_000, Some("x"), true);
        }
        let result = op.query(1_999);
        match result {
            Value::F64(v) => assert!(
                (v - 1.9).abs() < 1e-10,
                "weighted avg should be 1.9, got {v}"
            ),
            other => panic!("expected F64, got {:?}", other),
        }
    }

    // ── Min/Max windowing ─────────────────────────────────────────────────

    #[test]
    fn windowed_min_is_min_across_bucket_mins() {
        let window_ms: u64 = 64_000;
        let mut op = WindowedOp::new(AggKind::Min, window_ms);
        // Spread values across buckets
        for (i, v) in [
            (0_i64, 5.0_f64),
            (1_000, 2.0),
            (2_000, 8.0),
            (3_000, 1.0),
            (4_000, 7.0),
        ] {
            op.update(&row_f64("x", v), i, Some("x"), true);
        }
        let result = op.query(4_999);
        assert_eq!(result, Value::F64(1.0), "min across buckets should be 1.0");
    }

    #[test]
    fn windowed_max_is_max_across_bucket_maxes() {
        let window_ms: u64 = 64_000;
        let mut op = WindowedOp::new(AggKind::Max, window_ms);
        for (i, v) in [
            (0_i64, 5.0_f64),
            (1_000, 2.0),
            (2_000, 8.0),
            (3_000, 1.0),
            (4_000, 7.0),
        ] {
            op.update(&row_f64("x", v), i, Some("x"), true);
        }
        let result = op.query(4_999);
        assert_eq!(result, Value::F64(8.0), "max across buckets should be 8.0");
    }

    // ── Variance windowing ────────────────────────────────────────────────

    #[test]
    fn windowed_variance_combines_via_welford_pairwise_merge() {
        // [2, 4, 4, 4, 5, 5, 7, 9] split across two buckets:
        //   bucket 0 (t=0):    [2, 4, 4, 4]  — n=4, mean=3.5, m2=3.0
        //   bucket 1 (t=1000): [5, 5, 7, 9]  — n=4, mean=6.5, m2=8.0
        //
        // Pairwise Welford merge gives the same result as computing on the full stream.
        // Full stream: n=8, mean=5.0, SS=32.0
        // Sample variance (n-1 denominator) = 32/7 ≈ 4.571428...
        //
        // Note: the plan referenced "4.0" which is the population variance.
        // Beava uses sample variance (Bessel-corrected, n-1) consistently.
        // (Deviation: plan had incorrect expected value; correct sample variance is 32/7.)
        let window_ms: u64 = 64_000; // bucket_ms = 1000
        let mut op = WindowedOp::new(AggKind::Variance, window_ms);

        for (i, v) in [(0_i64, 2.0_f64), (0, 4.0), (0, 4.0), (0, 4.0)] {
            op.update(&row_f64("x", v), i, Some("x"), true);
        }
        // Put last 4 in bucket 1 (t=1000..1999)
        for (i, v) in [
            (1_000_i64, 5.0_f64),
            (1_000, 5.0),
            (1_000, 7.0),
            (1_000, 9.0),
        ] {
            op.update(&row_f64("x", v), i, Some("x"), true);
        }

        let result = op.query(1_999);
        let expected = 32.0_f64 / 7.0; // sample variance (n-1 denominator) = 4.571428...
        match result {
            Value::F64(v) => assert!(
                (v - expected).abs() < 1e-10,
                "pairwise Welford combined variance should be {expected:.6}, got {v}"
            ),
            other => panic!("expected F64, got {:?}", other),
        }
    }

    // ── Ratio windowing ───────────────────────────────────────────────────

    #[test]
    fn windowed_ratio_is_sum_matching_over_sum_total() {
        // 3 matching out of 5 total across 3 buckets
        let window_ms: u64 = 64_000;
        let mut op = WindowedOp::new(AggKind::Ratio, window_ms);
        let r = empty_row();
        // bucket 0: 2 events, 2 matching
        op.update(&r, 0, None, true);
        op.update(&r, 0, None, true);
        // bucket 1: 2 events, 1 matching
        op.update(&r, 1_000, None, true);
        op.update(&r, 1_000, None, false);
        // bucket 2: 1 event, 0 matching
        op.update(&r, 2_000, None, false);

        let result = op.query(2_999);
        match result {
            Value::F64(v) => assert!((v - 0.6).abs() < 1e-10, "ratio should be 3/5=0.6, got {v}"),
            other => panic!("expected F64, got {:?}", other),
        }
    }

    // ── update_with_row (Plan 05-02) ─────────────────────────────────────

    /// Windowed count with predicate "amount > 25": only matching rows counted
    /// in buckets. 5 rows [10, 20, 30, 40, 50] → 3 match (30, 40, 50) → I64(3).
    #[test]
    fn windowed_count_with_where_predicate_drops_non_matching() {
        let window_ms: u64 = 64_000; // bucket_ms = 1000
        let mut op = WindowedOp::new(AggKind::Count, window_ms);
        let where_expr =
            std::sync::Arc::new(crate::expr::parse("(amount > 25)").expect("should parse"));
        for (i, &amount) in [10.0_f64, 20.0, 30.0, 40.0, 50.0].iter().enumerate() {
            let row = Row::new().with_field("amount", Value::F64(amount));
            // spread across different buckets to exercise bucket routing
            op.update_with_row(&row, (i as i64) * 1_000, None, Some(&where_expr));
        }
        // query at t=4999: all 5 buckets in window; only 3 had matching rows
        let result = op.query(4_999);
        assert_eq!(
            result,
            Value::I64(3),
            "only rows with amount > 25 should be counted (30, 40, 50)"
        );
    }

    // ── Multi-bucket merge for sketch ops (regression) ────────────────────
    //
    // Bug surfaced from prod (beava.dev homepage `top_page_1h`): for a 1h
    // windowed top_k the displayed count "resets" every ~56s. Root cause:
    // `query()` for AggKind::TopK / Percentile / CountDistinct was picking
    // only the highest-epoch active bucket and returning that single
    // bucket's result, instead of merging across all active buckets the way
    // Count / Sum / Avg / Min / Max / Variance / StdDev / Entropy do. With
    // bucket_ms = ceil(window_ms / 64) ≈ 56s for a 1h window, every new
    // bucket made the visible result drop to whatever was observed in the
    // last sub-bucket only.
    //
    // These tests pin the merged-across-active-buckets contract.

    #[test]
    fn windowed_top_k_merges_across_active_buckets() {
        // window=64ms → bucket_ms=1ms. Push the same path "/home" 3× into
        // bucket 0 and 1× into bucket 10; query at t=20 with all buckets
        // still active. Merged top-1 must report "/home" with count 4, not
        // count 1 (which is what the latest bucket alone would say).
        let mut op = WindowedOp::new_with_params(
            AggKind::TopK,
            64,
            SketchParams {
                top_k_k: Some(2),
                ..Default::default()
            },
        );
        for t in [0_i64, 0, 0, 10] {
            let row = Row::new().with_field("path", Value::Str("/home".into()));
            op.update(&row, t, Some("path"), true);
        }
        // sanity: also insert a different path in yet another bucket to
        // ensure we don't accidentally merge unrelated values.
        let row_about = Row::new().with_field("path", Value::Str("/about".into()));
        op.update(&row_about, 15, Some("path"), true);

        let result = op.query(20);
        let arr = match result {
            Value::Json(serde_json::Value::Array(arr)) => arr,
            other => panic!("expected Json(Array), got {:?}", other),
        };
        // Top-1 must be /home with merged count 4, not just the latest
        // bucket's 0 (bucket 10 only saw /home once).
        assert!(
            !arr.is_empty(),
            "merged top_k must not be empty when buckets have data"
        );
        let top0 = &arr[0];
        assert_eq!(
            top0.get("value").and_then(|v| v.as_str()),
            Some("/home"),
            "/home should be the top value (4 occurrences across buckets); got {:?}",
            top0
        );
        assert_eq!(
            top0.get("count").and_then(|v| v.as_u64()),
            Some(4),
            "/home count must be merged across active buckets (3 in bucket 0 + 1 in bucket 10 = 4); got {:?}",
            top0
        );
    }

    #[test]
    fn windowed_percentile_merges_across_active_buckets() {
        // Same shape as the top_k test: spread known values across buckets
        // and assert the median reflects the full window, not the latest
        // bucket alone. Bucket 0 holds [1, 2, 3]; bucket 10 holds [100,
        // 100, 100]. Whole-window median is 50; latest-bucket-only median
        // is 100. Tolerance accounts for UDDSketch quantization.
        let mut op = WindowedOp::new_with_params(
            AggKind::Percentile,
            64,
            SketchParams {
                percentile_q: Some(0.5),
                ..Default::default()
            },
        );
        for (t, v) in [
            (0_i64, 1.0_f64),
            (0, 2.0),
            (0, 3.0),
            (10, 100.0),
            (10, 100.0),
            (10, 100.0),
        ] {
            let row = Row::new().with_field("dwell_ms", Value::F64(v));
            op.update(&row, t, Some("dwell_ms"), true);
        }
        let result = op.query(20);
        let median = match result {
            Value::F64(v) => v,
            other => panic!("expected F64 median, got {:?}", other),
        };
        // Full-window median of [1, 2, 3, 100, 100, 100] is 50 (between 3
        // and 100). Latest-bucket-only median is 100. The merged result
        // must be much closer to 50 than to 100.
        assert!(
            median < 80.0,
            "merged percentile must reflect both buckets; latest-only would give 100, got {}",
            median
        );
    }

    #[test]
    fn windowed_count_distinct_merges_across_active_buckets() {
        // Same shape as the top_k + percentile regressions: insert
        // distinct values across two buckets, query at a time both are
        // active, expect the merged distinct count rather than just the
        // latest bucket's count.
        let mut op = WindowedOp::new(AggKind::CountDistinct, 64);
        for (t, name) in [(0_i64, "a"), (0, "b"), (0, "c"), (10, "d"), (10, "e")] {
            let row = Row::new().with_field("user_id", Value::Str(name.into()));
            op.update(&row, t, Some("user_id"), true);
        }
        let result = op.query(20);
        assert_eq!(
            result,
            Value::I64(5),
            "merged distinct count must reflect both buckets (3 + 2 distinct = 5)"
        );
    }

    #[test]
    fn windowed_top_k_merges_across_active_buckets_in_hybrid_mode() {
        // Force Hybrid mode by registering many distinct values so the
        // exact-mode threshold is exceeded; verify the merge still picks
        // the dominant value as top-1. Without this test, a regression
        // that breaks only the Hybrid-merge path could ship green.
        let mut op = WindowedOp::new_with_params(
            AggKind::TopK,
            64,
            SketchParams {
                top_k_k: Some(3),
                ..Default::default()
            },
        );
        // Bucket 0: dominant value "winner" (1500x) plus many low-count
        // distinct fillers to push past the exact-mode threshold (1024).
        for _ in 0..1500 {
            let row = Row::new().with_field("path", Value::Str("winner".into()));
            op.update(&row, 0, Some("path"), true);
        }
        for i in 0..1500 {
            let row = Row::new().with_field("path", Value::Str(format!("filler-{i}").into()));
            op.update(&row, 0, Some("path"), true);
        }
        // Bucket 10: more "winner" hits.
        for _ in 0..500 {
            let row = Row::new().with_field("path", Value::Str("winner".into()));
            op.update(&row, 10, Some("path"), true);
        }
        let result = op.query(20);
        let arr = match result {
            Value::Json(serde_json::Value::Array(arr)) => arr,
            other => panic!("expected Json(Array), got {other:?}"),
        };
        let top0 = &arr[0];
        assert_eq!(
            top0.get("value").and_then(|v| v.as_str()),
            Some("winner"),
            "winner must remain top-1 after Hybrid merge; got {top0:?}"
        );
        // CMS-estimated count is approximate but must reflect both
        // buckets; a regression that picks one bucket only would yield
        // ≤1500.
        let count = top0.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        assert!(
            count >= 1900,
            "winner count must reflect the merged 1500+500; got {count}"
        );
    }

    // ── Cross-bucket merge tripwire (regression gate) ─────────────────────
    //
    // Catches the next time someone adds a windowable AggKind without a
    // proper cross-bucket merge: applies the same event stream two ways
    // — every event in one bucket vs each event in its own bucket — and
    // asserts the windowed query returns the same result. The buggy
    // "best-epoch active bucket" pattern fails this immediately, since
    // distributing events across buckets makes the latest-bucket-only
    // result diverge from the all-in-one-bucket result.
    //
    // Currently exercises Count, Sum, Min, Max, Variance, CountDistinct
    // (kinds with `Value::Eq` results that compare cleanly). Top_k +
    // Percentile have dedicated tests because their result shapes
    // (Json(Array) and approximate F64) don't compare with raw `==`.

    #[test]
    fn windowed_query_invariant_under_bucket_distribution() {
        let kinds = [
            AggKind::Count,
            AggKind::Sum,
            AggKind::Min,
            AggKind::Max,
            AggKind::Variance,
            AggKind::CountDistinct,
        ];
        let values: [f64; 8] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let window_ms: u64 = 64; // bucket_ms = 1ms

        for kind in kinds {
            let needs_field = !matches!(kind, AggKind::Count);

            let mut op_a = WindowedOp::new(kind, window_ms);
            for &v in values.iter() {
                let row = if needs_field {
                    Row::new().with_field("amount", Value::F64(v))
                } else {
                    Row::new()
                };
                let field = if needs_field { Some("amount") } else { None };
                op_a.update(&row, 0, field, true);
            }
            let result_a = op_a.query(10);

            let mut op_b = WindowedOp::new(kind, window_ms);
            for (i, &v) in values.iter().enumerate() {
                let row = if needs_field {
                    Row::new().with_field("amount", Value::F64(v))
                } else {
                    Row::new()
                };
                let field = if needs_field { Some("amount") } else { None };
                op_b.update(&row, i as i64, field, true);
            }
            // Query at t=15: all 8 buckets still active (window=64ms).
            let result_b = op_b.query(15);

            assert_eq!(
                result_a, result_b,
                "windowed {kind:?}: bucket distribution must not change \
                 the query result. single-bucket={result_a:?} \
                 multi-bucket={result_b:?}"
            );
        }
    }

    // ── Replay determinism ────────────────────────────────────────────────

    #[test]
    fn windowed_replay_determinism() {
        // Apply same 1000-event stream twice; internal state debug representations
        // must be byte-identical (SC4 internal-state gate per D-06).
        let window_ms: u64 = 64_000;

        let mut op1 = WindowedOp::new(AggKind::Count, window_ms);
        let mut op2 = WindowedOp::new(AggKind::Count, window_ms);
        let r = empty_row();

        // Deterministic pseudo-event stream: now_ms = i * 97 (prime step, mod window)
        for i in 0..1000_i64 {
            let t = (i * 97) % (window_ms as i64 * 2);
            op1.update(&r, t, None, true);
            op2.update(&r, t, None, true);
        }

        // Snapshot state as debug representation — must be byte-identical.
        // The SmallVec entry order can depend on push order, but with
        // deterministic event streams it's deterministic across runs.
        let snap1 = format!("{:?}", op1);
        let snap2 = format!("{:?}", op2);
        assert_eq!(
            snap1, snap2,
            "applying the same event stream twice must yield identical state (D-06 SC4)"
        );
    }

    // ── Lazy bucket allocation ──────────────────────

    /// Cold WindowedOp must not preallocate 64 bucket slots.
    ///
    /// The lazy SmallVec layout replaced earlier
    /// `[Option<Box<AggOp>>; 64]` + `[i64; 64]` preallocation. A
    /// freshly-constructed op has zero active buckets; pre-fix,
    /// `op.buckets.len()` returned 64 (array length), which was the red signal.
    #[test]
    fn test_cold_windowed_op_has_no_allocated_buckets() {
        let op = WindowedOp::new(AggKind::Count, 60_000);
        assert_eq!(
            op.buckets.len(),
            0,
            "cold WindowedOp must have ZERO active buckets (lazy alloc); got {}",
            op.buckets.len()
        );
    }

    /// First update must lazily allocate exactly one bucket entry.
    ///
    /// With the SmallVec layout, `update` on a cold op grows `buckets`
    /// from 0 → 1 (one push for the current epoch). Pre-fix,
    /// `op.buckets.len()` was 64 regardless, which was the red signal.
    #[test]
    fn test_windowed_op_lazy_allocates_one_bucket_on_first_update() {
        let mut op = WindowedOp::new(AggKind::Count, 60_000);
        let row = empty_row();
        op.update(&row, 1_000, None, true);
        assert_eq!(
            op.buckets.len(),
            1,
            "single update must lazy-allocate exactly one bucket; got {}",
            op.buckets.len()
        );
    }

    /// SmallVec inline cap is 4: pushing into 4 distinct epochs stays inline
    /// (no heap promotion); pushing a 5th promotes to heap but stays correct.
    ///
    /// Inline cap=4 covers the typical fraud case (1-2 active buckets per
    /// entity at any time).
    #[test]
    fn test_windowed_op_smallvec_inline_cap_4_then_spills_to_heap() {
        let mut op = WindowedOp::new(AggKind::Count, 64_000); // bucket_ms = 1000
        let r = empty_row();

        // Push into 4 distinct buckets; should stay inline.
        for i in 0..4_i64 {
            op.update(&r, i * 1_000, None, true);
        }
        assert_eq!(op.buckets.len(), 4, "4 buckets after 4 distinct epochs");
        assert!(
            !op.buckets.spilled(),
            "4 entries should fit inline (SmallVec cap=4)"
        );

        // 5th distinct bucket spills to heap but stays correct.
        op.update(&r, 4_000, None, true);
        assert_eq!(op.buckets.len(), 5, "5 buckets after 5 distinct epochs");
        assert!(
            op.buckets.spilled(),
            "5th entry must spill to heap (graceful promotion)"
        );
    }

    /// AGG-CORE-09 cap: 64 active buckets max — beyond cap, oldest is evicted.
    ///
    /// Pushes into 65 distinct epochs and verifies that exactly 64 entries
    /// remain (the oldest was swap_remove'd) and that querying across all
    /// active windows still folds correctly.
    #[test]
    fn test_windowed_op_evicts_oldest_at_max_buckets_cap() {
        // Use a very small bucket_ms (1ms) so we can pack 65 distinct epochs
        // into a meaningful test: window_ms = 64 * 65 = 4160ms
        let window_ms: u64 = 64; // bucket_ms = 1ms
        let mut op = WindowedOp::new(AggKind::Count, window_ms);
        let r = empty_row();

        // Push 65 events into 65 distinct epochs (event_time = i ms; bucket_ms=1).
        for i in 0..65_i64 {
            op.update(&r, i, None, true);
        }
        assert_eq!(
            op.buckets.len(),
            64,
            "AGG-CORE-09 cap: at most 64 active buckets"
        );
        // Oldest (epoch 0) should be evicted; buckets 1..65 remain.
        let oldest_present = op.buckets.iter().any(|(e, _)| *e == 0);
        assert!(!oldest_present, "epoch=0 should have been evicted");
        let newest_present = op.buckets.iter().any(|(e, _)| *e == 64);
        assert!(newest_present, "epoch=64 should still be present");
    }

    // ── WindowedOp::update_at fast-path ──────────────────────────────────

    /// Pre-extraction routes events to the right buckets via `update_at`.
    ///
    /// Constructing a WindowedOp(Count) and calling `update_at` 5 times across
    /// 3 distinct epochs (within `max_buckets`) must produce 3 buckets with
    /// the correct counts: epoch0=2, epoch1=2, epoch2=1.
    #[test]
    fn update_at_routes_to_buckets() {
        use crate::agg_op::FIELD_IDX_NONE;
        let window_ms: u64 = 64_000; // bucket_ms = 1000
        let mut op = WindowedOp::new(AggKind::Count, window_ms);
        let extracted: crate::agg_op::ExtractedFields<'_> = smallvec::smallvec![None];
        // 2 events in epoch 0
        op.update_at(
            None,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            100,
            true,
        );
        op.update_at(
            None,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            200,
            true,
        );
        // 2 events in epoch 1
        op.update_at(
            None,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            1_100,
            true,
        );
        op.update_at(
            None,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            1_900,
            true,
        );
        // 1 event in epoch 2
        op.update_at(
            None,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            2_500,
            true,
        );
        assert_eq!(op.buckets.len(), 3, "expected 3 distinct buckets");
        // Query at t=2999 — all three buckets active in 64s window.
        let result = op.query(2_999);
        assert_eq!(result, Value::I64(5), "count of all 5 events should be 5");
    }

    /// Pre-extracted Value is the source of truth. The row's field at the
    /// same name has a DIFFERENT value; `update_at` must use the
    /// caller-provided pre_val, not consult the row.
    #[test]
    fn windowed_update_at_bypasses_row_get() {
        let window_ms: u64 = 64_000;
        let mut op = WindowedOp::new(AggKind::Sum, window_ms);
        // pre-extracted carries 42.0 at index 0
        let pre = Value::F64(42.0);
        let extracted: crate::agg_op::ExtractedFields<'_> = smallvec::smallvec![Some(&pre)];
        op.update_at(
            Some(&pre),
            &extracted,
            0_u8,
            crate::agg_op::FIELD_IDX_NONE,
            crate::agg_op::FIELD_IDX_NONE,
            100,
            true,
        );
        let result = op.query(999);
        match result {
            Value::F64(v) => assert!(
                (v - 42.0).abs() < 1e-10,
                "sum should equal pre-extracted 42.0, got {v} (would be 0.0 if row.get was consulted)"
            ),
            other => panic!("expected F64, got {:?}", other),
        }
    }

    /// `where_matched=false` skips bucket update. No state should be
    /// mutated when the predicate already evaluated false at the outer
    /// dispatcher.
    #[test]
    fn windowed_update_at_skips_when_where_matched_false() {
        use crate::agg_op::FIELD_IDX_NONE;
        let window_ms: u64 = 64_000;
        let mut op = WindowedOp::new(AggKind::Count, window_ms);
        let extracted: crate::agg_op::ExtractedFields<'_> = smallvec::smallvec![None];
        // where_matched = false: bucket may be created but count must remain 0.
        op.update_at(
            None,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            100,
            false,
        );
        op.update_at(
            None,
            &extracted,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            FIELD_IDX_NONE,
            200,
            false,
        );
        let result = op.query(999);
        assert_eq!(
            result,
            Value::I64(0),
            "where_matched=false must not increment count"
        );
    }

    // ── Determinism guard ─────────────────────────────────────────────────

    #[test]
    fn no_wall_clock_or_rand_in_windowed_module() {
        // Split forbidden patterns so this file does not itself trigger the check.
        let forbidden_clock = ["SystemTime", "::", "now"].concat();
        let forbidden_rand = ["rand", "::"].concat();
        let src = include_str!("agg_windowed.rs");
        assert!(
            !src.contains(forbidden_clock.as_str()),
            "agg_windowed.rs must not use wall-clock reads (D-06 determinism invariant)"
        );
        assert!(
            !src.contains(forbidden_rand.as_str()),
            "agg_windowed.rs must not use rand crate (D-06 determinism invariant)"
        );
    }

    // ── Plan 12.6-05 Path X: time-source rename guard ─────────────────────
    //
    // Per `project_redis_shaped_no_event_time_ever` and CONTEXT D-03, the
    // windowed-op surface must read a server-clock parameter on every fold
    // (the post-Path-X name is `now_ms`, not the body event-time name).
    // This audit asserts the public API uses the post-Path-X parameter name
    // and never the pre-Path-X one.
    //
    // Comments and doc-comments are stripped so historical references (in
    // module docs / inline notes) do not falsely trigger.
    //
    // Forbidden tokens are reconstructed from chunked components at runtime
    // so this test source does not contain the literal it forbids — i.e.
    // the test does not match itself.
    #[test]
    fn windowed_op_signatures_use_now_ms_param_name() {
        let src = include_str!("agg_windowed.rs");
        let stripped: String = src
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !(trimmed.starts_with("//") || trimmed.starts_with("///"))
            })
            .collect::<Vec<_>>()
            .join("\n");
        // Reconstruct the forbidden token via chunk concat so this file
        // does not contain the literal pre-Path-X parameter.
        let forbidden = ["event", "_time_ms: ", "i64"].concat();
        let forbidden_us = ["_event", "_time_ms: ", "i64"].concat();
        let required = ["now", "_ms: ", "i64"].concat();
        assert!(
            stripped.contains(required.as_str()),
            "Path X invariant: agg_windowed.rs must declare `<{}>` somewhere",
            required
        );
        assert!(
            !stripped.contains(forbidden.as_str()),
            "Path X invariant: agg_windowed.rs must not contain pre-Path-X param `<{}>`",
            forbidden
        );
        assert!(
            !stripped.contains(forbidden_us.as_str()),
            "Path X invariant: agg_windowed.rs must not contain pre-Path-X param `<{}>`",
            forbidden_us
        );
    }
}
