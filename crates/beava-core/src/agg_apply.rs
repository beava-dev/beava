//! Apply-loop hook: routes a push event to every matching aggregation.
//!
//! # SDK-AGG-02, AGG-CORE-09
//!
//! `apply_event_to_aggregations` is the single-writer entry point for stateful
//! feature updates. It is called:
//! - in Phase 5, by the dev endpoint (`POST /dev/apply_events`);
//! - in Phase 6, by the production push handler (WAL group-commit path).
//!
//! ## D-06 determinism invariants
//!
//! This function is a **pure function** of `(source_name, row, now_ms,
//! registry state, prior agg state)`.  No wall-clock reads.  No random sources.
//! Safe for WAL replay (SC4).
//!
//! ## Why `event_id: u64` is in the signature
//!
//! WAL replay passes the stable event identifier from the WAL record. The
//! parameter is threaded through so the WAL replay path doesn't need to
//! change the signature of every caller. Currently ignored on the live
//! apply path (prefixed `_event_id`); dev-endpoint callers pass a
//! monotonic counter (0, 1, 2, …).

use crate::agg_op::{AggKind, ExtractedFields, FIELD_IDX_NONE};
use crate::agg_state_table::{EntityKeyShape, StateTables};
use crate::registry::Registry;
use crate::row::Row;

// ─── Plan 19.4-04 (D-02) Task 4.3 — ExtractedFields build instrumentation ───
//
// `EXTRACTED_BUILD_COUNT` counts the number of times `ExtractedFields` is
// populated for the apply path. Pre-Task-4.3.b: incremented INSIDE the
// per-descriptor loop, so the count is D × event_count (one rebuild per
// descriptor on each event). Post-Task-4.3.b (the hoist): incremented ONCE
// per event ABOVE the per-descriptor loop, so the count == event_count.
//
// Test-only: `#[cfg(test)]` gates the static so production builds incur
// zero overhead and the apply path stays branch-free in release builds.
//
// Per-thread counter (Cell<u64>) rather than process-wide AtomicU64: cargo's
// default test runner runs tests in parallel across worker threads; a global
// AtomicU64 would let concurrent apply calls in unrelated tests pollute this
// test's delta. The hot apply path is single-threaded per the project's
// `project_no_sharded_apply` invariant, so per-thread storage is correct
// (each test thread has its own counter; the production data plane only ever
// uses one thread). The companion test-only accessors below
// (extracted_build_count_load / _store) exist so the assertion site can
// read/write its own thread-local counter.
#[cfg(test)]
thread_local! {
    pub(crate) static EXTRACTED_BUILD_COUNT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[inline(always)]
pub(crate) fn extracted_build_count_increment() {
    EXTRACTED_BUILD_COUNT.with(|c| c.set(c.get() + 1));
}

// reason: test-only counter accessor; kept on the `tests` cfg even when no
// in-tree test currently invokes it so external integration tests / debug
// builds can flip it.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn extracted_build_count_load() -> u64 {
    EXTRACTED_BUILD_COUNT.with(|c| c.get())
}

// reason: see `extracted_build_count_load` above.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn extracted_build_count_store(v: u64) {
    EXTRACTED_BUILD_COUNT.with(|c| c.set(v));
}

// ─── Plan 19.2-07 (D-07): per-kind snapshot for /debug/op-cost ──────────────

/// Process-static snapshot of the latest TRACE_AGG per-kind output.
///
/// Updated by `apply_event_to_aggregations` whenever `BEAVA_TRACE_AGG_TIMING=1`
/// is active; read by the optional `GET /debug/op-cost` HTTP route.
///
/// Empty (data vec is empty, captured_at_ms == 0) when tracing has never been
/// enabled in this process.
pub struct PerKindSnapshot {
    /// Wall-clock ms since UNIX_EPOCH at the time of the last snapshot write.
    pub captured_at_ms: std::sync::atomic::AtomicU64,
    /// Per-AggKind (kind, total_duration, call_count) from the most recent
    /// TRACE_AGG measurement window.  Protected by a `parking_lot::Mutex`
    /// because writes happen at apply-thread frequency (once per traced event)
    /// and reads happen at HTTP scrape rate (~1 Hz from /debug/op-cost).
    pub data: parking_lot::Mutex<Vec<(AggKind, std::time::Duration, u32)>>,
}

static PER_KIND_LATEST: std::sync::OnceLock<PerKindSnapshot> = std::sync::OnceLock::new();

/// Return a reference to the process-static per-kind snapshot.
///
/// Initialises the singleton on first call (zero-filled, data is empty).
/// Called by `apply_event_to_aggregations` (write path) and by the
/// `/debug/op-cost` HTTP handler (read path).
pub fn per_kind_latest() -> &'static PerKindSnapshot {
    PER_KIND_LATEST.get_or_init(|| PerKindSnapshot {
        captured_at_ms: std::sync::atomic::AtomicU64::new(0),
        data: parking_lot::Mutex::new(Vec::new()),
    })
}

/// Test-only legacy oracle implementing a structurally DIFFERENT path
/// from production for the bit-identity regression test.
///
/// Three INDEPENDENT codepath differences from `apply_event_to_aggregations`:
///   1. Allocates a fresh `ExtractedFields` (`SmallVec::new()`) PER DESCRIPTOR
///      every event — no thread_local reuse, no event-level hoist.
///   2. Populates `extracted` from `desc.field_names` (the per-agg distinct
///      field list) NOT from `apply_field_names` (the per-source union).
///   3. Computes `pre_val` via `feat.descriptor.field_idx` directly into the
///      per-desc rebuild — NOT via the
///      `feat.descriptor.field_idx_into_event_extracted` union remap.
///
/// These differences mean the f64::to_bits() comparison exercises an
/// INDEPENDENT codepath, not the same code with different inputs — so a
/// passing test verifies the hoist preserves state semantics, not a
/// tautology.
///
/// Reuses the same state surface as production (StateTables, EntityKeyShape,
/// `update_with_extracted`) so both paths can update the same registry's
/// state — required for the bit-identity comparison to read from one
/// snapshot at the end. The test calls this against a SEPARATE registry +
/// state-tables tuple to keep the two paths' state cleanly separated.
#[cfg(test)]
pub(crate) fn legacy_apply_event_to_aggregations(
    source_name: &str,
    row: &Row,
    now_ms: i64,
    _event_id: u64,
    registry: &Registry,
    state_tables: &mut StateTables,
) {
    let descs = registry.compiled_aggregations_for_source(source_name);

    // Same EntityKey-shape cluster cache as production, since the legacy
    // codepath difference is in the EXTRACTED build, not the entity-key
    // dispatch.
    let mut shape_cache: Vec<Option<Option<EntityKeyShape>>> = Vec::new();

    for desc in descs {
        // Build EntityKeyShape once per cluster_id.
        let cluster_idx = desc.cluster_id as usize;
        if shape_cache.len() <= cluster_idx {
            shape_cache.resize_with(cluster_idx + 1, || None);
        }
        let shape_opt: &Option<EntityKeyShape> = match &shape_cache[cluster_idx] {
            Some(cached) => cached,
            None => {
                let computed = EntityKeyShape::from_row(&desc.group_keys, row);
                shape_cache[cluster_idx] = Some(computed);
                shape_cache[cluster_idx].as_ref().unwrap()
            }
        };
        let shape = match shape_opt {
            Some(s) => s,
            None => continue,
        };

        let agg_idx = desc.agg_id as usize;
        if state_tables.len() <= agg_idx {
            state_tables.resize_with(agg_idx + 1, crate::agg_state_table::AggStateTable::new);
        }
        let table = &mut state_tables[agg_idx];
        let entity_row = table.get_or_init_by_shape(shape, &desc);

        // Independent codepath difference #1 + #2: fresh per-descriptor
        // SmallVec allocation populated from desc.field_names (per-agg
        // distinct list), NOT from apply_field_names (per-source union).
        let extracted: ExtractedFields = desc
            .field_names
            .iter()
            .map(|f| row.get(f.as_str()))
            .collect();

        for (i, feat) in desc.features.iter().enumerate() {
            // Independent codepath difference #3: use feat.descriptor.field_idx
            // DIRECTLY into the per-desc rebuild — NOT via
            // field_idx_into_event_extracted.
            let pre_val: Option<&crate::row::Value> = if feat.descriptor.field_idx != FIELD_IDX_NONE
            {
                extracted
                    .get(feat.descriptor.field_idx as usize)
                    .copied()
                    .flatten()
            } else {
                None
            };
            // The legacy path passes the per-agg field_idx into
            // update_with_extracted (matching the pre-19.4-04 dispatch
            // protocol). Geo lat_idx/lon_idx in production now refer to the
            // per-source union; for the legacy oracle, the test pipeline
            // uses no geo features so lat_idx/lon_idx stay at FIELD_IDX_NONE
            // and the geo dispatch arm falls through to its slow row.get
            // path — yielding the same result either way.
            entity_row[i].update_with_extracted(
                pre_val,
                now_ms,
                feat.descriptor.where_expr.as_ref(),
                row,
                feat.descriptor.field.as_deref(),
                feat.descriptor.field_idx,
                &extracted,
                feat.descriptor.ext.lat_idx,
                feat.descriptor.ext.lon_idx,
            );
        }
    }
}

/// Apply a single event to every aggregation whose `source_node_name` matches
/// `source_name`.
///
/// # Semantics
///
/// 1. Look up all aggregations for `source_name` via
///    `Registry::compiled_aggregations_for_source`.
/// 2. For each aggregation:
///    - Derive `EntityKey` from `row` + `descriptor.group_keys`.
///      If any group-key field is null/missing → drop the event for this
///      aggregation (continue to the next).
///    - Look up or initialise the entity row in the aggregation's
///      `AggStateTable`.
///    - For each feature: call `AggOp::update_with_row(row, now_ms,
///      field, where_expr)`.
///
/// # `event_id` parameter
///
/// `_event_id` is deliberately prefixed with `_` to silence the
/// `unused_variables` lint while preserving the exact parameter name in the
/// signature for Phase 6.  **Do NOT remove this parameter.**  Phase 6 WAL will
/// populate it with the stable WAL event identifier (D-08); callers must not
/// break their signatures.
///
/// # No wall-clock reads
///
/// `now_ms` is the only time source.  Wall-clock reads are forbidden
/// in this function (D-06).
///
/// # `cold_after_ms` parameter (Plan 12.8-03)
///
/// Per CONTEXT D-01 (per-source `@bv.event(cold_after=...)`) + D-04 (FRESH
/// state on resurrect, Redis TTL pattern, locked permanent), callers pass the
/// source's `EventDescriptor.cold_after_ms`. When `Some(N)`, each touched
/// entity is checked against its sidecar `last_seen_ms`; if older than
/// `now_ms - N`, the entity's prior `Vec<AggOp>` is dropped and a fresh row
/// is allocated by the subsequent `get_or_init_by_shape` call.
///
/// When `cold_after_ms = None` (the common case, sources that don't opt in),
/// the eviction check is a single `Option::is_some` branch — zero per-event
/// cost over the pre-12.8-03 path.
///
/// Recovery (`replay_handrolled_wal_dir` / `replay_wal_from_lsn`) also passes
/// the source's `cold_after_ms`. This is correct: replay re-builds live state
/// in time order, so cold-TTL eviction during replay matches what the
/// running server would have done.
pub fn apply_event_to_aggregations(
    source_name: &str,
    row: &Row,
    now_ms: i64,
    event_id: u64, // Phase 5: unused. Phase 6 WAL populates via D-08.
    registry: &Registry,
    state_tables: &mut StateTables,
    cold_after_ms: Option<u64>,
) {
    let descs = registry.compiled_aggregations_for_source(source_name);
    apply_event_to_aggregations_with_descs(
        source_name,
        row,
        now_ms,
        event_id,
        registry,
        state_tables,
        cold_after_ms,
        descs,
    );
}

/// Replay-only variant: route the event ONLY to aggregations whose owning
/// `DerivationDescriptor.registered_at_version <= event_rv`. Live path
/// uses `apply_event_to_aggregations` (no version filter). See
/// `Registry::compiled_aggregations_for_source_at_rv` for the rationale.
#[allow(clippy::too_many_arguments)] // mirrors the live entry-point signature
pub fn apply_event_to_aggregations_replay(
    source_name: &str,
    row: &Row,
    now_ms: i64,
    event_id: u64,
    event_rv: u64,
    registry: &Registry,
    state_tables: &mut StateTables,
    cold_after_ms: Option<u64>,
) {
    let descs = registry.compiled_aggregations_for_source_at_rv(source_name, event_rv);
    apply_event_to_aggregations_with_descs(
        source_name,
        row,
        now_ms,
        event_id,
        registry,
        state_tables,
        cold_after_ms,
        descs,
    );
}

/// Shared body for `apply_event_to_aggregations` (live path) and
/// `apply_event_to_aggregations_replay` (recovery). Takes pre-resolved
/// descs so the caller can choose whether to apply a version filter.
#[allow(clippy::too_many_arguments)] // mirrors the live entry-point signature
fn apply_event_to_aggregations_with_descs(
    source_name: &str,
    row: &Row,
    now_ms: i64,
    _event_id: u64,
    registry: &Registry,
    state_tables: &mut StateTables,
    cold_after_ms: Option<u64>,
    descs: Vec<std::sync::Arc<crate::agg_descriptor::AggregationDescriptor>>,
) {
    // SPIKE: per-substage timing of the agg hot path.
    // Gated on its OWN env var (not BEAVA_TRACE_APPLY_TIMING) so that the
    // outer dispatch_push_sync trace can run without the inner eprintln
    // contaminating its agg-stage measurement.
    // OnceLock cache: env::var lookup happens once per process, not per push.
    fn trace_agg_enabled() -> bool {
        use std::sync::OnceLock;
        static FLAG: OnceLock<bool> = OnceLock::new();
        *FLAG.get_or_init(|| std::env::var("BEAVA_TRACE_AGG_TIMING").ok().as_deref() == Some("1"))
    }
    let trace = trace_agg_enabled();
    let t0 = if trace {
        Some(std::time::Instant::now())
    } else {
        None
    };

    let t_registry = t0.map(|t| t.elapsed());

    // Hoist ExtractedFields build above the descriptor loop. The per-event
    // field-union (apply_field_names) is populated at register-time by
    // apply_registration; the apply loop pre-extracts ONCE per event
    // (instead of D times for D descriptors).
    //
    // The `'a` lifetime of `ExtractedFields<'a>` is bound to `&row`'s lifetime,
    // which lives for the duration of this function — so a stack-allocated
    // SmallVec works without thread_local or unsafe re-borrow tricks. When
    // there are no aggs on this source (descs is empty) OR when no agg declares
    // a field (apply_field_names is empty), the hoisted slice is empty and
    // each feature's pre_val branch falls through to None.
    //
    // SmallVec inline cap = 16 covers fraud-team's ~12-field union without
    // spilling on the warm path.
    let event_extracted: ExtractedFields = if descs.is_empty() {
        // Fast path: no aggs on this source. Source EventDescriptor lookup
        // would be wasted; skip the build (the descriptor loop body never
        // runs).
        ExtractedFields::new()
    } else {
        match registry.get_event_descriptor(source_name) {
            Some(src_event) => src_event
                .apply_field_names
                .iter()
                .map(|f| row.get(f.as_str()))
                .collect(),
            None => {
                // Source isn't registered as an event — shouldn't happen on
                // the hot path (descs is non-empty so an agg targets this
                // source, and apply_registration enforces source registration
                // ordering). Defensive: fall back to empty so per-feature
                // pre_val resolves to None and op falls through to the slow
                // row.get-by-name path inside update_with_extracted.
                ExtractedFields::new()
            }
        }
    };
    // Instrumentation site: increment counts once per event (not D times),
    // verifying the hoist eliminates per-desc rebuild scaffolding.
    #[cfg(test)]
    extracted_build_count_increment();

    let mut t_entity_key_total = std::time::Duration::ZERO;
    let mut t_table_lookup_total = std::time::Duration::ZERO;
    let mut t_entity_row_total = std::time::Duration::ZERO;
    let mut t_features_total = std::time::Duration::ZERO;
    let mut feat_updates: u32 = 0;
    let mut desc_count: u32 = 0;

    // Per-op-kind timing accumulator (only populated when trace is on).
    // Breaks the `features` loop into per-AggKind buckets and dumps with
    // the trace line so callers can see which operator family dominates
    // apply time on a given pipeline.
    let mut per_kind: Vec<(crate::agg_op::AggKind, std::time::Duration, u32)> = Vec::new();

    // Cluster dispatch cache.
    //
    // Aggregations that share the same group_keys signature share a cluster_id
    // (assigned at register-time). We build EntityKeyShape ONCE per cluster_id
    // per event call, not once per aggregation, eliminating redundant SmallVec
    // builds and CompactString allocations on the hot path.
    //
    // The cache is a small inline Vec<Option<EntityKeyShape>> indexed by
    // cluster_id (u32). It is allocated lazily only for the first aggregation
    // that references each cluster_id. For the common single-cluster case
    // (all aggs on one event type have the same group_keys) the Vec has length 1
    // and the branch is predicted.
    //
    // `None` in the slot means "not yet computed for this call"; the slot is
    // never cleared within a single apply_event_to_aggregations call because
    // all aggs in the same cluster share group_keys and thus share the result.
    //
    // Special sentinel `Option<Option<EntityKeyShape>>`:
    //   - outer None → not computed yet
    //   - outer Some(None) → computed, but the row was missing/NaN → skip slot
    //   - outer Some(Some(shape)) → ready to use
    let mut shape_cache: Vec<Option<Option<EntityKeyShape>>> = Vec::new();

    for desc in descs {
        desc_count += 1;
        let t_a = t0.map(|t| t.elapsed());

        // Apply this derivation's chain ops (with_columns / select / drop /
        // rename / cast / fillna) to the source row before evaluating the
        // aggregation. Falls back to the raw source row (zero overhead)
        // when no chain is registered — the common case for direct
        // event→table aggs. Required for `bv.lit('web')` constant-column
        // flows where the where predicate or feature field references a
        // column added upstream by an `@bv.event def`-decorated derivation.
        // Chain `Filter` ops may drop a row → skip this desc when that fires.
        let chain_opt = registry.compiled_chain(&desc.node_name);
        let chain_row_storage: Option<crate::row::Row> = match chain_opt.as_ref() {
            Some(c) => match c.apply(row.clone()) {
                Some(transformed) => Some(transformed),
                None => continue,
            },
            None => None,
        };
        let agg_row: &crate::row::Row = chain_row_storage.as_ref().unwrap_or(row);

        // Build EntityKeyShape once per cluster_id.
        let cluster_idx = desc.cluster_id as usize;
        if shape_cache.len() <= cluster_idx {
            shape_cache.resize_with(cluster_idx + 1, || None);
        }
        let shape_opt: &Option<EntityKeyShape> = match &shape_cache[cluster_idx] {
            Some(cached) => cached,
            None => {
                // First agg in this cluster for this event: compute and cache.
                let computed = EntityKeyShape::from_row(&desc.group_keys, row);
                shape_cache[cluster_idx] = Some(computed);
                shape_cache[cluster_idx].as_ref().unwrap()
            }
        };
        let shape = match shape_opt {
            Some(s) => s,
            None => continue, // missing/null/NaN group-key — skip this agg
        };

        let t_b = t0.map(|t| t.elapsed());

        // O(1) array index by `agg_id` (assigned at register-time). Server-side
        // register handler resizes `state_tables` after each registration,
        // but tests/admin paths sometimes call apply_event_to_aggregations
        // directly without going through that handler — so guard with a
        // lazy resize here. Branch is cheap (len compare; predicted
        // not-taken in steady state).
        let agg_idx = desc.agg_id as usize;
        if state_tables.len() <= agg_idx {
            state_tables.resize_with(agg_idx + 1, crate::agg_state_table::AggStateTable::new);
        }
        let table = &mut state_tables[agg_idx];
        let t_c = t0.map(|t| t.elapsed());

        // Phase 12.8 memory-governance: cold-TTL eviction check (FRESH
        // state on resurrect). Skipped when source has no cold_after_ms —
        // single Option::is_some branch, ~1 ns. When set, costs 1 HashMap
        // lookup (~10-15 ns warm-path) + comparison; on cold-eviction,
        // drops the entity's Vec<AggOp> + sidecar entry so the next
        // `get_or_init_by_shape` call below allocates a fresh row.
        if let Some(ttl_ms) = cold_after_ms {
            let evicted = table.evict_entity_by_shape_if_cold(shape, now_ms as u64, ttl_ms);
            // Increment process-static cold-entity-eviction counter on
            // eviction firing. Read by the admin sidecar's /metrics handler
            // for `beava_cold_entity_evictions_total` (UNLABELED v0
            // — per-source labels deferred).
            if evicted {
                crate::agg_state::ColdEntityEvictionCounter::inc();
            }
        }

        let entity_row = table.get_or_init_by_shape(shape, &desc);
        let t_d = t0.map(|t| t.elapsed());

        // ExtractedFields is hoisted above this loop. We still need to remap
        // each feature's `field_idx` (which indexes into `agg.field_names`,
        // the per-agg list) into the per-event `event_extracted` slice
        // (indexed by the per-source `apply_field_names` union). The remap
        // mapping is `feat.descriptor.field_idx_into_event_extracted` —
        // populated at register-time by `resolve_field_indices_for_agg_mut*`.
        for (i, feat) in desc.features.iter().enumerate() {
            let op_t0 = if trace {
                Some(std::time::Instant::now())
            } else {
                None
            };
            // Look up the pre-extracted value for this feature's field via
            // the union remap. FIELD_IDX_NONE on field_idx means the op is
            // fieldless (Count, Ratio, etc.) and pre_val resolves to None —
            // the op ignores it.
            let pre_val: Option<&crate::row::Value> = if feat.descriptor.field_idx != FIELD_IDX_NONE
            {
                let agg_local_idx = feat.descriptor.field_idx as usize;
                match feat
                    .descriptor
                    .field_idx_into_event_extracted
                    .get(agg_local_idx)
                {
                    Some(&union_idx) if union_idx != FIELD_IDX_NONE => {
                        event_extracted.get(union_idx as usize).copied().flatten()
                    }
                    _ => {
                        // Mapping absent or sentinel — fall back to per-row
                        // lookup by name (slow path; reachable when
                        // apply_field_names was empty at resolver time, OR
                        // when the field was added by chain ops and isn't
                        // in the source-event apply_field_names union).
                        // Use `agg_row` so chain-added fields are visible.
                        feat.descriptor
                            .field
                            .as_deref()
                            .and_then(|n| agg_row.get(n))
                    }
                }
            } else {
                // When field_idx is FIELD_IDX_NONE but the agg does declare
                // a `field` name, the field is likely added by a chain op
                // (with_columns/cast/rename/fillna) — not in the source
                // event schema, so the resolver couldn't assign a
                // field_idx. Fall back to per-row lookup against agg_row so
                // chain-added fields like `rate` (`with_columns(rate=...)`)
                // are visible to the agg's value-bearing ops (mean / sum /
                // first / quantile / etc.).
                feat.descriptor
                    .field
                    .as_deref()
                    .and_then(|n| agg_row.get(n))
            };
            entity_row[i].update_with_extracted(
                pre_val,
                now_ms,
                feat.descriptor.where_expr.as_ref(),
                agg_row,
                feat.descriptor.field.as_deref(),
                feat.descriptor.field_idx,
                &event_extracted,
                feat.descriptor.ext.lat_idx,
                feat.descriptor.ext.lon_idx,
            );
            if let Some(t) = op_t0 {
                let dur = t.elapsed();
                let kind = feat.descriptor.kind;
                // Linear scan; per-pipeline kind count is small (<30 typical).
                if let Some(slot) = per_kind.iter_mut().find(|(k, _, _)| *k == kind) {
                    slot.1 += dur;
                    slot.2 += 1;
                } else {
                    per_kind.push((kind, dur, 1));
                }
            }
            feat_updates += 1;
        }

        // Record arrival time AFTER applying the event so the sidecar
        // reflects the most-recent successful update. Only paid when the
        // source opts in via cold_after_ms. The post-eviction
        // `get_or_init_by_shape` call above either reused the warm row or
        // allocated a fresh one — either way, this entity is now warm at
        // `now_ms`, so the sidecar must be (re)stamped here.
        if cold_after_ms.is_some() {
            table.record_last_seen_by_shape(shape, now_ms as u64);
        }

        let t_e = t0.map(|t| t.elapsed());

        if let (Some(a), Some(b), Some(c), Some(d), Some(e)) = (t_a, t_b, t_c, t_d, t_e) {
            t_entity_key_total += b - a;
            t_table_lookup_total += c - b;
            t_entity_row_total += d - c;
            t_features_total += e - d;
        }
    }

    if let (Some(t0_inst), Some(reg)) = (t0, t_registry) {
        let total = t0_inst.elapsed();
        // Format per-kind as "Count=42@1,Sum=120@1,..." (ns@count_per_event).
        let mut per_kind_str = String::new();
        for (kind, dur, cnt) in &per_kind {
            if !per_kind_str.is_empty() {
                per_kind_str.push(',');
            }
            per_kind_str.push_str(&format!("{:?}={}@{}", kind, dur.as_nanos(), cnt));
        }
        eprintln!(
            "TRACE_AGG ns: descs={} feat_updates={} registry_call={} entity_key={} table_lookup={} entity_row_init={} features={} TOTAL={} per_kind={}",
            desc_count,
            feat_updates,
            reg.as_nanos(),
            t_entity_key_total.as_nanos(),
            t_table_lookup_total.as_nanos(),
            t_entity_row_total.as_nanos(),
            t_features_total.as_nanos(),
            total.as_nanos(),
            per_kind_str,
        );

        // Snapshot per_kind data for /debug/op-cost. Written once per
        // traced event under the mutex. Mutex contention is negligible:
        // writes at most once per event (trace-gated), reads at HTTP
        // scrape rate (~1 Hz from /debug/op-cost).
        //
        // D-06 compliance: we use `now_ms` (the event's logical time,
        // already passed in as the canonical time source) instead of a
        // wall-clock read. For live workloads now_ms ≈ wall-clock ms;
        // it is deterministic for WAL replay. Note: the trace path itself
        // uses `Instant::now()` for duration measurement, but Instant is a
        // monotonic clock that does not affect replay determinism (it is
        // not stored in the WAL or entity state).
        let snap = per_kind_latest();
        if now_ms > 0 {
            snap.captured_at_ms
                .store(now_ms as u64, std::sync::atomic::Ordering::Relaxed);
        }
        {
            let mut data = snap.data.lock();
            *data = per_kind.clone();
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
    use crate::agg_op::{AggKind, AggOpDescriptor};
    use crate::registry::{DerivationDescriptor, EventDescriptor, OutputKind, Registry};
    use crate::registry_diff::PayloadNode;
    use crate::row::{Row, Value};
    use crate::schema::{DerivedSchema, EventSchema, FieldType};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn simple_event_schema() -> EventSchema {
        let mut fields = BTreeMap::new();
        fields.insert("user_id".to_string(), FieldType::Str);
        fields.insert("amount".to_string(), FieldType::F64);
        fields.insert("status".to_string(), FieldType::Str);
        EventSchema {
            fields,
            optional_fields: vec![],
        }
    }

    fn make_event(name: &str) -> EventDescriptor {
        EventDescriptor {
            name: name.to_string(),
            schema: simple_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        }
    }

    fn make_agg_desc(
        node_name: &str,
        source: &str,
        keys: &[&str],
        features: &[(&str, AggOpDescriptor)],
    ) -> AggregationDescriptor {
        AggregationDescriptor {
            node_name: node_name.to_string(),
            source_node_name: source.to_string(),
            group_keys: keys.iter().map(|k| k.to_string()).collect(),
            features: features
                .iter()
                .map(|(name, d)| NamedAggOp {
                    feature_name: name.to_string(),
                    descriptor: d.clone(),
                })
                .collect(),
            agg_id: 0,
            field_names: vec![],
            cluster_id: 0,
        }
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
            field_idx: crate::agg_op::FIELD_IDX_NONE,
            field_idx_into_event_extracted: Vec::new(),
        }
    }

    fn sum_desc(field: &str) -> AggOpDescriptor {
        AggOpDescriptor {
            kind: AggKind::Sum,
            field: Some(field.to_string()),
            window_ms: None,
            where_expr: None,
            n: None,
            half_life_ms: None,
            sub_window_ms: None,
            sigma: None,
            sketch_params: None,
            ext: Default::default(),
            field_idx: crate::agg_op::FIELD_IDX_NONE,
            field_idx_into_event_extracted: Vec::new(),
        }
    }

    fn make_registry_with_agg(event_name: &str, agg: AggregationDescriptor) -> Arc<Registry> {
        let registry = Arc::new(Registry::new());
        let deriv_name = agg.node_name.clone();

        let deriv = DerivationDescriptor {
            name: deriv_name.clone(),
            output_kind: OutputKind::Table,
            upstreams: vec![event_name.to_string()],
            ops: vec![],
            schema: DerivedSchema {
                fields: BTreeMap::new(),
                optional_fields: vec![],
            },
            table_primary_key: None,
            registered_at_version: 0,
        };

        registry.apply_registration(
            vec![
                PayloadNode::Event(make_event(event_name)),
                PayloadNode::Derivation(deriv),
            ],
            vec![],
            vec![],
            vec![(deriv_name, Arc::new(agg))],
        );

        registry
    }

    // ── apply_event_to_aggregations tests ─────────────────────────────────────

    /// A01: Event routes to matching source only — not to aggregations with a
    /// different source.
    #[test]
    fn apply_routes_event_to_matching_source_only() {
        // Register AggA (source=Transaction) and AggB (source=Login).
        let registry = Arc::new(Registry::new());

        let agg_a = make_agg_desc(
            "AggA",
            "Transaction",
            &["user_id"],
            &[("cnt", count_desc())],
        );
        let agg_b = make_agg_desc("AggB", "Login", &["user_id"], &[("cnt", count_desc())]);

        let deriv_a = DerivationDescriptor {
            name: "AggA".to_string(),
            output_kind: OutputKind::Table,
            upstreams: vec!["Transaction".to_string()],
            ops: vec![],
            schema: DerivedSchema {
                fields: BTreeMap::new(),
                optional_fields: vec![],
            },
            table_primary_key: None,
            registered_at_version: 0,
        };
        let deriv_b = DerivationDescriptor {
            name: "AggB".to_string(),
            output_kind: OutputKind::Table,
            upstreams: vec!["Login".to_string()],
            ops: vec![],
            schema: DerivedSchema {
                fields: BTreeMap::new(),
                optional_fields: vec![],
            },
            table_primary_key: None,
            registered_at_version: 0,
        };

        registry.apply_registration(
            vec![
                PayloadNode::Event(make_event("Transaction")),
                PayloadNode::Event(make_event("Login")),
                PayloadNode::Derivation(deriv_a),
                PayloadNode::Derivation(deriv_b),
            ],
            vec![],
            vec![],
            vec![
                ("AggA".to_string(), Arc::new(agg_a)),
                ("AggB".to_string(), Arc::new(agg_b)),
            ],
        );

        let mut state_tables: StateTables = crate::agg_state_table::new_state_tables_for(&registry);
        let row = Row::new().with_field("user_id", Value::Str("alice".into()));

        apply_event_to_aggregations(
            "Transaction",
            &row,
            1000,
            0,
            &registry,
            &mut state_tables,
            None,
        );

        // AggA's table should be populated; AggB's table should NOT.
        assert!(
            crate::agg_state_table::has_entries_for_name(&state_tables, &registry, "AggA"),
            "AggA must be populated for Transaction events"
        );
        assert!(
            !crate::agg_state_table::has_entries_for_name(&state_tables, &registry, "AggB"),
            "AggB must NOT be populated for Transaction events"
        );
    }

    /// A02: Count aggregation, 10 events → count == I64(10).
    #[test]
    fn apply_increments_count_feature() {
        let agg = make_agg_desc(
            "UserCount",
            "Transaction",
            &["user_id"],
            &[("cnt", count_desc())],
        );
        let registry = make_registry_with_agg("Transaction", agg);

        let mut state_tables: StateTables = crate::agg_state_table::new_state_tables_for(&registry);
        let row = Row::new().with_field("user_id", Value::Str("alice".into()));

        for i in 0..10 {
            apply_event_to_aggregations(
                "Transaction",
                &row,
                1000 + i,
                i as u64,
                &registry,
                &mut state_tables,
                None,
            );
        }

        let table =
            crate::agg_state_table::lookup_table_by_name(&state_tables, &registry, "UserCount")
                .expect("UserCount table must exist");
        let key = crate::agg_state_table::EntityKey({
            let mut sv: smallvec::SmallVec<[(compact_str::CompactString, Value); 2]> =
                smallvec::SmallVec::new();
            sv.push(("user_id".into(), Value::Str("alice".into())));
            sv
        });
        let val = table
            .query_feature(&key, 0, 10_000)
            .expect("must have value");
        assert_eq!(val, Value::I64(10), "count must be 10 after 10 events");
    }

    /// A03: Event with null group-key is dropped — no state_table entry created.
    #[test]
    fn apply_drops_events_with_null_group_key() {
        let agg = make_agg_desc(
            "UserCount",
            "Transaction",
            &["user_id"],
            &[("cnt", count_desc())],
        );
        let registry = make_registry_with_agg("Transaction", agg);

        let mut state_tables: StateTables = crate::agg_state_table::new_state_tables_for(&registry);
        // Row with user_id = Null → should be dropped.
        let row = Row::new().with_field("user_id", Value::Null);

        apply_event_to_aggregations(
            "Transaction",
            &row,
            1000,
            0,
            &registry,
            &mut state_tables,
            None,
        );

        // No state should exist at all.
        let is_empty =
            crate::agg_state_table::lookup_table_by_name(&state_tables, &registry, "UserCount")
                .map(|t| t.entity_count() == 0)
                .unwrap_or(true);
        assert!(
            is_empty,
            "null group-key event must not create any entity state"
        );
    }

    /// A04: where predicate = "(amount > 100)"; amount=50 event → entity row
    /// created but count feature stays at I64(0).
    ///
    /// Semantics (D-03): `AggOp::update_with_row` gates the update per feature.
    /// The entity row IS created (get_or_init is called before evaluating the
    /// predicate), but the per-feature update is skipped when where=false.
    ///
    /// NOTE: Revised semantics — entity row is NOT created if we guard before
    /// get_or_init. Either is acceptable; DOCUMENT which is chosen. This test
    /// accepts EITHER: entity row absent OR entity row present with count=0.
    #[test]
    fn apply_with_where_false_skips_update() {
        let where_expr =
            std::sync::Arc::new(crate::expr::parse("(amount > 100)").expect("parse where expr"));
        let agg = make_agg_desc(
            "UserCount",
            "Transaction",
            &["user_id"],
            &[(
                "cnt",
                AggOpDescriptor {
                    kind: AggKind::Count,
                    field: None,
                    window_ms: None,
                    where_expr: Some(where_expr),
                    n: None,
                    half_life_ms: None,
                    sub_window_ms: None,
                    sigma: None,
                    sketch_params: None,
                    ext: Default::default(),
                    field_idx: crate::agg_op::FIELD_IDX_NONE,
                    field_idx_into_event_extracted: Vec::new(),
                },
            )],
        );
        let registry = make_registry_with_agg("Transaction", agg);

        let mut state_tables: StateTables = crate::agg_state_table::new_state_tables_for(&registry);
        let row = Row::new()
            .with_field("user_id", Value::Str("alice".into()))
            .with_field("amount", Value::F64(50.0)); // below threshold

        apply_event_to_aggregations(
            "Transaction",
            &row,
            1000,
            0,
            &registry,
            &mut state_tables,
            None,
        );

        // Either: no entry for alice, OR alice's count == 0.
        let count =
            crate::agg_state_table::lookup_table_by_name(&state_tables, &registry, "UserCount")
                .and_then(|t| {
                    let key = crate::agg_state_table::EntityKey({
                        let mut sv: smallvec::SmallVec<[(compact_str::CompactString, Value); 2]> =
                            smallvec::SmallVec::new();
                        sv.push(("user_id".into(), Value::Str("alice".into())));
                        sv
                    });
                    t.query_feature(&key, 0, 10_000)
                });

        match count {
            None => {}                // Acceptable: no entity row created
            Some(Value::I64(0)) => {} // Acceptable: entity row exists but count=0
            Some(other) => panic!("where=false must not increment count; got {:?}", other),
        }
    }

    /// A05: Replay determinism — apply same 5-event stream twice; Debug repr
    /// of state_table must be byte-identical.
    #[test]
    fn apply_replay_determinism() {
        let agg = make_agg_desc(
            "UserCount",
            "Transaction",
            &["user_id"],
            &[("cnt", count_desc())],
        );
        let registry = make_registry_with_agg("Transaction", agg);

        let events: Vec<(Row, i64)> = (0..5)
            .map(|i| {
                let row =
                    Row::new().with_field("user_id", Value::Str(format!("user_{}", i % 2).into()));
                (row, 1000 + i)
            })
            .collect();

        let apply_all = |tables: &mut StateTables| {
            for (i, (row, t)) in events.iter().enumerate() {
                apply_event_to_aggregations(
                    "Transaction",
                    row,
                    *t,
                    i as u64,
                    &registry,
                    tables,
                    None,
                );
            }
        };

        let mut tables1: StateTables = crate::agg_state_table::new_state_tables_for(&registry);
        let mut tables2: StateTables = crate::agg_state_table::new_state_tables_for(&registry);
        apply_all(&mut tables1);
        apply_all(&mut tables2);

        // Compare via iter_sorted (BTreeMap-ordered) for deterministic Debug output.
        let snapshot1 =
            crate::agg_state_table::lookup_table_by_name(&tables1, &registry, "UserCount")
                .map(|t| t.iter_sorted().collect::<Vec<_>>());
        let snapshot2 =
            crate::agg_state_table::lookup_table_by_name(&tables2, &registry, "UserCount")
                .map(|t| t.iter_sorted().collect::<Vec<_>>());
        assert_eq!(
            format!("{snapshot1:?}"),
            format!("{snapshot2:?}"),
            "apply_event_to_aggregations must be deterministic (D-06)"
        );
    }

    /// A06: Multi-feature aggregation [count, sum(amount)] updated correctly.
    #[test]
    fn apply_multi_feature_aggregation_updates_all() {
        let agg = make_agg_desc(
            "UserStats",
            "Transaction",
            &["user_id"],
            &[("cnt", count_desc()), ("total", sum_desc("amount"))],
        );
        let registry = make_registry_with_agg("Transaction", agg);

        let mut state_tables: StateTables = crate::agg_state_table::new_state_tables_for(&registry);
        let amounts = [10.0_f64, 20.0, 30.0, 40.0, 50.0];
        for (i, &amt) in amounts.iter().enumerate() {
            let row = Row::new()
                .with_field("user_id", Value::Str("alice".into()))
                .with_field("amount", Value::F64(amt));
            apply_event_to_aggregations(
                "Transaction",
                &row,
                1000 + i as i64,
                i as u64,
                &registry,
                &mut state_tables,
                None,
            );
        }

        let table =
            crate::agg_state_table::lookup_table_by_name(&state_tables, &registry, "UserStats")
                .expect("UserStats must exist");
        let key = crate::agg_state_table::EntityKey({
            let mut sv: smallvec::SmallVec<[(compact_str::CompactString, Value); 2]> =
                smallvec::SmallVec::new();
            sv.push(("user_id".into(), Value::Str("alice".into())));
            sv
        });

        let cnt = table
            .query_feature(&key, 0, 10_000)
            .expect("cnt must exist");
        assert_eq!(cnt, Value::I64(5), "count must be 5");

        let total = table
            .query_feature(&key, 1, 10_000)
            .expect("total must exist");
        match total {
            Value::F64(v) => assert!((v - 150.0).abs() < 1e-10, "total must be 150.0, got {v}"),
            other => panic!("expected F64 for total, got {:?}", other),
        }
    }

    /// A07: event_id has no observable effect in Phase 5.
    ///
    /// Apply the SAME (row, now_ms) twice — once with event_id=0 and
    /// once with event_id=99 — into two independent state_table instances.
    /// The resulting state must be identical.
    #[test]
    fn apply_accepts_event_id_and_ignores_it_in_phase_5() {
        let agg = make_agg_desc(
            "UserCount",
            "Transaction",
            &["user_id"],
            &[("cnt", count_desc())],
        );
        let registry = make_registry_with_agg("Transaction", agg);

        let row = Row::new().with_field("user_id", Value::Str("alice".into()));
        let t = 1000_i64;

        // Apply with event_id=0.
        let mut tables_0: StateTables = crate::agg_state_table::new_state_tables_for(&registry);
        apply_event_to_aggregations("Transaction", &row, t, 0, &registry, &mut tables_0, None);

        // Apply with event_id=99.
        let mut tables_99: StateTables = crate::agg_state_table::new_state_tables_for(&registry);
        apply_event_to_aggregations("Transaction", &row, t, 99, &registry, &mut tables_99, None);

        // State must be identical regardless of event_id.
        let snap_0 =
            crate::agg_state_table::lookup_table_by_name(&tables_0, &registry, "UserCount")
                .map(|t| t.iter_sorted().collect::<Vec<_>>());
        let snap_99 =
            crate::agg_state_table::lookup_table_by_name(&tables_99, &registry, "UserCount")
                .map(|t| t.iter_sorted().collect::<Vec<_>>());
        assert_eq!(
            format!("{snap_0:?}"),
            format!("{snap_99:?}"),
            "event_id must have no observable effect in Phase 5"
        );
    }

    /// A08: No wall-clock reads or rand in agg_apply.rs (D-06 grep guard).
    #[test]
    fn no_systemtime_now_in_agg_apply() {
        let src = include_str!("agg_apply.rs");
        let forbidden_clock = ["SystemTime", "::", "now"].concat();
        let forbidden_rand = ["rand", "::"].concat();
        assert!(
            !src.contains(forbidden_clock.as_str()),
            "agg_apply.rs must not use wall-clock reads (D-06)"
        );
        assert!(
            !src.contains(forbidden_rand.as_str()),
            "agg_apply.rs must not use rand crate (D-06)"
        );
    }
}

// ─── Registry extension tests ─────────────────────────────────────────────────

#[cfg(test)]
mod registry_source_tests {
    use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
    use crate::agg_op::{AggKind, AggOpDescriptor};
    use crate::agg_state_table::StateTables;
    use crate::registry::{DerivationDescriptor, EventDescriptor, OutputKind, Registry};
    use crate::registry_diff::PayloadNode;
    use crate::row::{Row, Value};
    use crate::schema::{DerivedSchema, EventSchema, FieldType};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn simple_event_schema() -> EventSchema {
        let mut fields = BTreeMap::new();
        fields.insert("user_id".to_string(), FieldType::Str);
        fields.insert("amount".to_string(), FieldType::F64);
        fields.insert("status".to_string(), FieldType::Str);
        EventSchema {
            fields,
            optional_fields: vec![],
        }
    }

    fn make_event(name: &str) -> EventDescriptor {
        let mut fields = BTreeMap::new();
        fields.insert("user_id".to_string(), FieldType::Str);
        EventDescriptor {
            name: name.to_string(),
            schema: EventSchema {
                fields,
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        }
    }

    fn make_agg(node_name: &str, source: &str) -> AggregationDescriptor {
        AggregationDescriptor {
            node_name: node_name.to_string(),
            source_node_name: source.to_string(),
            group_keys: vec!["user_id".to_string()],
            features: vec![NamedAggOp {
                feature_name: "cnt".to_string(),
                descriptor: AggOpDescriptor {
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
                    field_idx: crate::agg_op::FIELD_IDX_NONE,
                    field_idx_into_event_extracted: Vec::new(),
                },
            }],
            agg_id: 0,
            field_names: vec![],
            cluster_id: 0,
        }
    }

    /// R01: Two aggregations with source=Transaction; lookup returns both.
    #[test]
    fn compiled_aggregations_for_source_returns_matching() {
        let registry = Arc::new(Registry::new());

        let agg1 = make_agg("Agg1", "Transaction");
        let agg2 = make_agg("Agg2", "Transaction");
        let agg3 = make_agg("Agg3", "Login");

        for (name, event_name, agg) in [
            ("Agg1", "Transaction", agg1),
            ("Agg2", "Transaction", agg2),
            ("Agg3", "Login", agg3),
        ] {
            let deriv = DerivationDescriptor {
                name: name.to_string(),
                output_kind: OutputKind::Table,
                upstreams: vec![event_name.to_string()],
                ops: vec![],
                schema: DerivedSchema {
                    fields: BTreeMap::new(),
                    optional_fields: vec![],
                },
                table_primary_key: None,
                registered_at_version: 0,
            };
            registry.apply_registration(
                vec![
                    PayloadNode::Event(make_event(event_name)),
                    PayloadNode::Derivation(deriv),
                ],
                vec![],
                vec![],
                vec![(name.to_string(), Arc::new(agg))],
            );
        }

        let txn_aggs = registry.compiled_aggregations_for_source("Transaction");
        assert_eq!(
            txn_aggs.len(),
            2,
            "two aggregations should match source=Transaction"
        );
        let names: Vec<&str> = txn_aggs.iter().map(|a| a.node_name.as_str()).collect();
        assert!(names.contains(&"Agg1"), "Agg1 must be in results");
        assert!(names.contains(&"Agg2"), "Agg2 must be in results");
    }

    /// R02: Lookup for unknown source → empty Vec.
    #[test]
    fn compiled_aggregations_for_source_empty_for_unknown() {
        let registry = Arc::new(Registry::new());
        let result = registry.compiled_aggregations_for_source("Nonexistent");
        assert!(
            result.is_empty(),
            "unknown source must return empty Vec, got {} entries",
            result.len()
        );
    }

    // ── ExtractedFields per-event-build test ──

    /// ExtractedFields must be built once per event, not D-times per
    /// descriptor. With D=2 derivations on the Txn source, the pre-hoist
    /// count is 2*N (per-desc rebuild fires twice per event); the
    /// post-hoist count is N (single per-event hoist).
    ///
    /// To force a meaningful test (not a tautology with D=1), this test
    /// registers TWO derivations on the same Txn source. Pre-hoist count
    /// would be 2N; post-hoist count is N.
    #[test]
    fn extracted_fields_built_once_per_event_not_per_desc() {
        use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
        use crate::agg_op::{AggKind, AggOpDescriptor, FIELD_IDX_NONE};

        let registry = Arc::new(Registry::new());
        // Build the Txn event + 2 derivations + 2 compiled aggs (D=2 descriptors on Txn).
        let event_txn = EventDescriptor {
            name: "Txn".to_string(),
            schema: simple_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };
        let mk_agg = |node: &str, feat: &str| {
            Arc::new(AggregationDescriptor {
                node_name: node.to_string(),
                source_node_name: "Txn".to_string(),
                group_keys: vec!["user_id".to_string()],
                features: vec![NamedAggOp {
                    feature_name: feat.to_string(),
                    descriptor: AggOpDescriptor {
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
                    },
                }],
                agg_id: 0,
                field_names: vec![],
                cluster_id: 0,
            })
        };
        let mk_deriv = |node: &str, feat: &str| DerivationDescriptor {
            name: node.to_string(),
            output_kind: OutputKind::Table,
            upstreams: vec!["Txn".to_string()],
            ops: vec![],
            schema: DerivedSchema {
                fields: {
                    let mut m = BTreeMap::new();
                    m.insert("user_id".to_string(), FieldType::Str);
                    m.insert(feat.to_string(), FieldType::I64);
                    m
                },
                optional_fields: vec![],
            },
            table_primary_key: None,
            registered_at_version: 0,
        };
        registry.apply_registration(
            vec![
                PayloadNode::Event(event_txn),
                PayloadNode::Derivation(mk_deriv("AggA", "ca")),
                PayloadNode::Derivation(mk_deriv("AggB", "cb")),
            ],
            vec![],
            vec![],
            vec![
                ("AggA".to_string(), mk_agg("AggA", "ca")),
                ("AggB".to_string(), mk_agg("AggB", "cb")),
            ],
        );

        let mut state_tables: StateTables = crate::agg_state_table::new_state_tables_for(&registry);

        // Drive N events through the apply path. The instrumentation
        // counter is thread_local (per the EXTRACTED_BUILD_COUNT declaration
        // above), so other parallel cargo-test threads don't pollute this
        // test's count. Reset to 0 at the start of THIS thread's run.
        let n_events: u64 = 50;
        super::extracted_build_count_store(0);
        for i in 0..n_events {
            let row = Row::new()
                .with_field("user_id", Value::Str(format!("u{}", i % 5).into()))
                .with_field("amount", Value::F64(10.0 + i as f64))
                .with_field("status", Value::Str("ok".into()));
            super::apply_event_to_aggregations(
                "Txn",
                &row,
                i as i64 * 1000,
                i,
                &registry,
                &mut state_tables,
                None,
            );
        }
        let count = super::extracted_build_count_load();
        assert_eq!(
            count, n_events,
            "EXTRACTED_BUILD_COUNT should equal n_events ({}) — once per event. Got {} (D=2 derivs * N=50 = {} means per-desc rebuild still happens — Task 4.3.b hoist not yet landed).",
            n_events,
            count,
            n_events * 2
        );
    }

    // ── Bit-identity regression test ─────────────────────────────────────────

    /// Bit-identical state cross-check between (a) the production hoisted
    /// apply path and (b) the legacy per-desc-rebuild oracle on a
    /// deterministic 14-feature event sequence.
    ///
    /// The event-level hoist must produce bit-identical state vs a
    /// structurally-different legacy path so the f64::to_bits() strict
    /// equality is meaningful (not a tautology that rubber-stamps
    /// whatever code ships).
    ///
    /// The legacy oracle MUST exhibit at least three INDEPENDENT
    /// codepath differences from production:
    ///   1. Allocates a fresh `ExtractedFields::new()` per descriptor (no
    ///      reuse, no thread_local — different allocation pattern).
    ///   2. Populates from `desc.field_names` (per-agg list), NOT
    ///      `apply_field_names` (per-source union) — different field-list
    ///      scope.
    ///   3. Uses the per-agg `feat.descriptor.field_idx` directly into the
    ///      per-desc rebuild, NOT the union remap via
    ///      `field_idx_into_event_extracted` — different mapping path.
    ///
    /// RED state today: `legacy_apply_event_to_aggregations` does not exist
    /// as a symbol; cargo build errors with E0425 ("cannot find function").
    /// Task 4.4.b adds the test-only oracle and flips this to GREEN.
    #[test]
    fn bit_identical_state_legacy_per_desc_rebuild_vs_event_level_hoist() {
        use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
        use crate::agg_op::{AggKind, AggOpDescriptor, FIELD_IDX_NONE};

        // Build TWO independent registries with the same 14-feature pipeline.
        let build_registry = || -> Arc<Registry> {
            let r = Arc::new(Registry::new());
            let event_txn = EventDescriptor {
                name: "Txn".to_string(),
                schema: simple_event_schema(),
                dedupe_key: None,
                dedupe_window_ms: None,
                keep_events_for_ms: None,
                cold_after_ms: None,
                registered_at_version: 0,
                name_arc: Arc::from(""),
                apply_field_names: vec![],
            };
            // 14 mixed features: alternating Count and Sum(amount).
            let mut features = Vec::new();
            for i in 0..14u32 {
                features.push(NamedAggOp {
                    feature_name: format!("f{}", i),
                    descriptor: AggOpDescriptor {
                        kind: if i % 2 == 0 {
                            AggKind::Count
                        } else {
                            AggKind::Sum
                        },
                        field: if i % 2 == 0 {
                            None
                        } else {
                            Some("amount".to_string())
                        },
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
                    },
                });
            }
            let agg_arc = Arc::new(AggregationDescriptor {
                node_name: "AggBundle".to_string(),
                source_node_name: "Txn".to_string(),
                group_keys: vec!["user_id".to_string()],
                features,
                agg_id: 0,
                field_names: vec![],
                cluster_id: 0,
            });
            let mut deriv_fields = BTreeMap::new();
            deriv_fields.insert("user_id".to_string(), FieldType::Str);
            for i in 0..14u32 {
                deriv_fields.insert(format!("f{}", i), FieldType::F64);
            }
            let deriv = DerivationDescriptor {
                name: "AggBundle".to_string(),
                output_kind: OutputKind::Table,
                upstreams: vec!["Txn".to_string()],
                ops: vec![],
                schema: DerivedSchema {
                    fields: deriv_fields,
                    optional_fields: vec![],
                },
                table_primary_key: Some(vec!["user_id".to_string()]),
                registered_at_version: 0,
            };
            r.apply_registration(
                vec![
                    PayloadNode::Event(event_txn),
                    PayloadNode::Derivation(deriv),
                ],
                vec![],
                vec![],
                vec![("AggBundle".to_string(), agg_arc)],
            );
            r
        };

        let r_new = build_registry();
        let r_legacy = build_registry();

        let mut state_new: StateTables = crate::agg_state_table::new_state_tables_for(&r_new);
        let mut state_legacy: StateTables = crate::agg_state_table::new_state_tables_for(&r_legacy);

        // Deterministic 1000-event sequence using a seeded xorshift64 PRNG.
        let seed: u64 = 0xCAFEBABE_DEADBEEF;
        let mut state = seed;
        let mut next_u64 = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        for i in 0..1000_u64 {
            let card = format!("u{}", next_u64() % 100);
            // Cap amount magnitude so floats stay in-range; use raw u64 → f64
            // conversion bounded to a reasonable scale.
            let amount = (next_u64() % 1_000_000) as f64 / 100.0;
            let event_time = (next_u64() % 86_400_000) as i64;
            let row = Row::new()
                .with_field("user_id", Value::Str(card.into()))
                .with_field("amount", Value::F64(amount))
                .with_field("status", Value::Str("ok".into()));

            super::apply_event_to_aggregations(
                "Txn",
                &row,
                event_time,
                i,
                &r_new,
                &mut state_new,
                None,
            );
            super::legacy_apply_event_to_aggregations(
                "Txn",
                &row,
                event_time,
                i,
                &r_legacy,
                &mut state_legacy,
            );
        }

        // Bit-identical state cross-check via f64::to_bits() across all
        // entities × all features. AggBundle has agg_id=0; both state-table
        // arrays index there.
        let table_new = &state_new[0];
        let table_legacy = &state_legacy[0];
        let entries_new: Vec<_> = table_new.iter_sorted().collect();
        let entries_legacy: Vec<_> = table_legacy.iter_sorted().collect();
        assert_eq!(
            entries_new.len(),
            entries_legacy.len(),
            "same entity count expected"
        );
        for ((k_new, ops_new), (k_legacy, ops_legacy)) in
            entries_new.iter().zip(entries_legacy.iter())
        {
            assert_eq!(k_new, k_legacy, "entity-keys must match in sorted order");
            for (i, (op_new, op_legacy)) in ops_new.iter().zip(ops_legacy.iter()).enumerate() {
                let v_new = op_new.query(86_400_001);
                let v_legacy = op_legacy.query(86_400_001);
                match (&v_new, &v_legacy) {
                    (Value::F64(a), Value::F64(b)) => assert_eq!(
                        a.to_bits(),
                        b.to_bits(),
                        "feature {} f64 state divergence at key {:?}: new={} legacy={}",
                        i,
                        k_new,
                        a,
                        b
                    ),
                    (Value::I64(a), Value::I64(b)) => assert_eq!(
                        a, b,
                        "feature {} i64 state divergence at key {:?}: new={} legacy={}",
                        i, k_new, a, b
                    ),
                    _ => assert_eq!(
                        v_new, v_legacy,
                        "feature {} value divergence at key {:?}: new={:?} legacy={:?}",
                        i, k_new, v_new, v_legacy
                    ),
                }
            }
        }
    }
}
