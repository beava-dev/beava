# Memory Budget

beava runs entirely in memory. There is no SSD overflow, no tiered
storage, no page-out-to-cold-cache. Users size their box; if state
exceeds RAM, beava refuses new entities. The budget for v0 is
**~7 KB per entity for a rich 30-feature pack** — verified post-Phase
12.9 — which gives **~700 GB for 100 M entities**, fitting on a 1 TB
NVMe box with headroom.

This page walks the per-entity memory math, the verified Phase 12.9
numbers, what can blow the budget, and the three V0-MEM-GOV invariants
that keep it honest.

## Verified Phase 12.9 numbers (cite explicitly)

Phase 12.9 (closed 2026-05-03 PASS) boxed 7 fat AggOp variants
(`SeasonalDeviation`, `HourOfDayHistogram`, `EventTypeMix`,
`GeoVelocity`, `GeoSpread`, `GeoDistance`, `DistanceFromHome`) so they
store as `Box<State>` (8 B inline + heap state) instead of inline. The
result:

| Metric                                       | Pre-12.9 | Post-12.9 | Delta            |
| -------------------------------------------- | -------: | --------: | ---------------- |
| `size_of::<AggOp>()`                         | 600 B    | **80 B**  | **-87% (7.5×)**  |
| user_id entity inline cost (78 features)     | 46.8 KB  | 6.2 KB    | -86%             |
| card_fp entity inline cost (8 features)      | 4.8 KB   | 640 B     | -87%             |
| device_id entity inline cost (9 features)    | 5.4 KB   | 720 B     | -87%             |
| ip_address entity inline cost (12 features)  | 7.2 KB   | 960 B     | -87%             |
| merchant_id entity inline cost (4 features)  | 2.4 KB   | 320 B     | -87%             |

Per the Phase 12.9 SUMMARY, fraud-team weighted-average per-entity
dropped from **~22 KB → ~6 KB** post-boxing — clearing the 7 KB budget
with headroom.

## CI tripwire: `aggop_size_within_cap`

The 80-byte cap is enforced by a permanent CI test:

```rust
// crates/beava-core/tests/per_entity_size_dump.rs
const AGGOP_SIZE_CAP_BYTES: usize = 80;

#[test]
fn aggop_size_within_cap() {
    let actual = size_of::<AggOp>();
    assert!(actual <= AGGOP_SIZE_CAP_BYTES, ...);
}
```

Future operator additions that exceed the cap force a deliberate review
decision: either Box the new variant (preferred — preserves the cap) or
explicitly raise `AGGOP_SIZE_CAP_BYTES` with a documented rationale.
80 B accommodates `TrendResidualState` (72 B; the largest unboxed variant
post-12.9) plus discriminant + alignment headroom.

This is **not** an aspirational target — it's a CI gate. PRs that
add a new fat variant inline get caught before merge.

See [`crates/beava-core/tests/per_entity_size_dump.rs`](../../crates/beava-core/tests/per_entity_size_dump.rs)
and the Phase 12.9 SUMMARY at
[`.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md`](../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md).

## Per-entity memory math

For a representative 30-feature aggregation pack on a single entity:

| Component                                | Bytes (typical)                       |
| ---------------------------------------- | ------------------------------------- |
| 30 × `size_of::<AggOp>()` slots          | 30 × 80 B = **2.4 KB**                |
| Heap state for boxed variants            | 0.5-3 KB (depends on op mix)          |
| Heap state for unboxed bounded ops       | 0.2-0.8 KB (sketches, ring buffers)    |
| Entity-key overhead (`SmallVec<[u8;16]>`) | ~16 B inline (or heap if longer)     |
| HashMap bucket overhead (hashbrown)      | ~24 B per entry                       |
| **Total**                                | **~3-7 KB depending on op mix**       |

For the **fraud-team primary tuning shape** (14-node pipeline, 110
features distributed across multiple per-entity tables, weighted by
real fraud-shape distribution), the post-Phase-12.9 weighted-average
per-entity is **~6 KB**, comfortably under the 7 KB ceiling.

## Capacity calculation

```text
7 KB per entity × 100 M entities = ~700 GB
```

Fits on a 1 TB NVMe box with ~30% headroom. For larger entity counts,
shard horizontally — each beava instance owns a slice of the key
space (Redis-cluster pattern). See
[single-thread-apply.md](./single-thread-apply.md) for the
horizontal-scale story.

For workloads with significantly fewer features per entity (e.g. 10
features × 50 M entities = ~150 GB), beava fits comfortably on smaller
boxes. For workloads pushing the upper bound (e.g. 100 features ×
100 M entities ~= 1 TB), shard.

## What CAN blow the budget

Three failure modes the budget assumes you've handled:

1. **Lifetime aggregations without bounded kwargs.** A `bv.first_n()`
   without `n=` would grow per-entity without bound. **V0-MEM-GOV-02
   enforces this at register-time** — see
   [../concepts/lifetime-aggregation.md](../concepts/lifetime-aggregation.md).
2. **Cold entities accumulating without TTL.** If your stream has long
   entity-key tails (one-shot users, single-hit devices), entities
   accumulate forever. **V0-MEM-GOV-01** gives you opt-in
   `@bv.event(cold_after="<duration>")` for lazy TTL eviction.
3. **Sketch ops with very high cardinality.** `n_unique` (HLL) or
   `quantile` (DDSketch) state grows by a log factor with cardinality
   — bounded but real. A 1 M-distinct-value HLL is ~12 KB per entity.
   For very high cardinality you may want windowed sketches instead of
   lifetime.

## V0-MEM-GOV invariants

The three V0-MEM-GOV invariants (locked Phase 12.8) collectively bound
`entities × per-entity bytes` — the only memory dimension beava ships
with:

### V0-MEM-GOV-01 — Cold-entity TTL (opt-in)

`@bv.event(cold_after="<duration>")` is the canonical opt-in cold-entity
TTL surface (per-source decorator only — no env-var, no global
override). Default `cold_after=None` preserves no-expiry behavior. The
apply hot path lazily evicts entities (FRESH state on resurrect, Redis
TTL pattern) when `now_ms - last_seen_ms > cold_after_ms`.

Range: `[1s, 365d]` validated at decoration time. No background
thread; eviction is inline at the entity-state lookup (preserves
`project_no_sharded_apply` single-thread invariant).

CI: [`crates/beava-server/tests/phase12_8_cold_entity_eviction.rs`](../../crates/beava-server/tests/phase12_8_cold_entity_eviction.rs).
Metric: `beava_cold_entity_evictions_total`.

### V0-MEM-GOV-02 — Lifetime aggregation contract (register-time)

Every lifetime aggregation operator declares a finite per-entity
memory ceiling at register-time. Hard register-time rejection via
JSON-prelude shim `pre_check_unbounded_op_in_lifetime_mode` — error
code `unbounded_op_in_lifetime_mode`. Default-ON via
`BEAVA_MEMORY_GOV_ENFORCE` env-gate (escape hatch:
`BEAVA_MEMORY_GOV_ENFORCE=0`).

See [../concepts/lifetime-aggregation.md](../concepts/lifetime-aggregation.md)
for the per-op classification table (O1 / BoundedSketch /
BoundedByRequiredKwarg / BoundedByConfig).

CI: [`crates/beava-server/tests/phase12_8_lifetime_ops_have_bounds.rs`](../../crates/beava-server/tests/phase12_8_lifetime_ops_have_bounds.rs).
Metric: `beava_lifetime_op_cap_hit_total`.

### V0-MEM-GOV-03 — Per-event bucket reclaim (always-on)

Per-event bucket reclaim within active entities is the canonical
Tier-2 mechanism (always on, no opt-in). Existing `update_at(now_ms)`
per-windowed-op trims trailing buckets that have rolled past the
64-bucket cap; the `BucketReclaimCounter::inc()` site at
[`crates/beava-core/src/agg_windowed.rs`](../../crates/beava-core/src/agg_windowed.rs)
fires on every eviction so operators can observe the rate.

No new mechanism in Phase 12.8 — Tier 2 was already shipping; this
REQ-ID locks the contract: idle entities continue holding state until
either (a) Tier 1 cold-TTL evicts them or (b) the next event triggers
normal `update_at()` cleanup.

Metric: `beava_bucket_reclaim_total`.

## /metrics observable

Five Prometheus metric families ship from Phase 12.8 Plan 06 to make
the budget observable in production:

- `beava_cold_entity_evictions_total` (counter) — Plan 03 cold-TTL
  evictions fired.
- `beava_lifetime_op_cap_hit_total` (counter) — entropy-categories
  capped + future top_k / histogram cap-hit events.
- `beava_entity_count_resident` (gauge) — resident entity count
  snapshot (sampled).
- `beava_bucket_reclaim_total` (counter) —
  `WindowedOp::evict_oldest_bucket` firings.
- `beava_bytes_per_entity_p99` (gauge) — currently a static 7000
  placeholder per Phase 12.8 PLANNER-SURFACED CONCERN; dynamic
  sampling deferred to Phase 13.4 / v0.0.x. Phase 12.9's actual
  fraud-team weighted-avg is ~6 KB so the static value is no longer
  misleading, just not informative; replacement is ~30 LOC in
  `agg_state.rs::EntityCountResidentSnapshot`.

See [observability.md](./observability.md) for the full metric
endpoint shape + Prometheus exposition format.

## What's NOT in the budget

The 7 KB / entity number covers per-entity aggregation state. Other
memory consumers exist but don't scale per-entity:

- **Registry** — once per process (not per entity); typically <100 KB.
- **WAL ring buffer** — fixed 16 MiB × 3 buffers (Phase 18); 48 MiB
  total regardless of entity count.
- **HTTP listener / TCP listener** — per-connection buffers; small.
- **Snapshot working memory** — peaks during snapshot serialization
  (~size of state); transient, runs on dedicated thread.

For sizing a box, the dominant term is `entities × bytes/entity`. The
others are noise.

## Cross-references

- [`CLAUDE.md` § Constraints](../../CLAUDE.md) — the canonical 7 KB /
  entity number + the citation chain into Phase 12.9 SUMMARY +
  `aggop_size_within_cap` CI tripwire.
- [`CLAUDE.md` § Memory Governance Invariant](../../CLAUDE.md) — the
  three V0-MEM-GOV invariants in full.
- [`.planning/REQUIREMENTS.md`](../../.planning/REQUIREMENTS.md)
  V0-MEM-GOV-01 / V0-MEM-GOV-02 / V0-MEM-GOV-03 — the canonical
  requirement statements.
- [`.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md`](../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md)
  — the boxing change + measured numbers.
- [`crates/beava-core/tests/per_entity_size_dump.rs`](../../crates/beava-core/tests/per_entity_size_dump.rs)
  — `aggop_size_within_cap` CI tripwire.
- [`crates/beava-core/src/agg_op.rs`](../../crates/beava-core/src/agg_op.rs)
  — `AggOp` enum with the 7 boxed variants.
- [../concepts/lifetime-aggregation.md](../concepts/lifetime-aggregation.md)
  — V0-MEM-GOV-02 in user-facing terms.
- [single-thread-apply.md](./single-thread-apply.md) — single-thread
  apply + horizontal-scale story (Redis-cluster pattern).
- [observability.md](./observability.md) — Prometheus metrics for
  observing the budget in production.
- [`.planning/ideas/per-entity-memory-budget.md`](../../.planning/ideas/per-entity-memory-budget.md)
  — historical analysis that drove Phase 12.9 boxing.
