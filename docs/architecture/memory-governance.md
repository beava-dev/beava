# Memory governance

beava holds all state in memory. There is no SSD overflow, no tiered store, no page-out path. So the engine has to be honest, at register time, about exactly how much memory each feature can ever consume per entity. If an aggregation can't name its ceiling, beava refuses to register it.

That contract is what makes the **~7 KB per entity** budget defensible. This page is the architecture-side companion to the [memory budget](./memory-budget.md) — the budget is the number, governance is how the engine keeps the number honest.

## The shape of the problem

A windowed aggregation is naturally bounded. `bv.count(window="1h")` holds at most a few dozen buckets per entity, and old buckets evict as time advances. The math is finite by construction.

A **lifetime aggregation** — same operator with no `window=` — is what makes this hard. `bv.count()` over the whole history of an entity is fine: one `u64` per entity, constant. But `bv.first_n("event")` over the whole history is a memory leak waiting to happen — without an `n=`, the buffer grows with every event for as long as the entity exists.

beava's stance: every lifetime aggregation must declare a finite per-entity ceiling. If the operator can name a constant, it runs lifetime. If it can't, beava rejects the register payload before any user data touches the engine.

## What's bounded vs unbounded

Every operator in the catalogue lands in one of these classes:

| Class | Examples | Per-entity bound |
| --- | --- | --- |
| **Constant scalar** | `count`, `sum`, `mean`, `min`, `max`, `var`, `std`, `first`, `last`, `first_seen`, `last_seen`, `time_since`, `streak`, decay family, velocity / trend family | One scalar (or a small fixed pair) per entity. |
| **Bounded sketch** | `n_unique` (HLL), `quantile` (DDSketch), `bloom_member` | Sketch state — bounded by the sketch's own parameters, not by stream length. |
| **Bounded by required kwarg** | `first_n`, `last_n`, `lag`, `most_recent_n`, `reservoir_sample`, `histogram` | The caller has to supply the size: `n=`, `samples=`, or `buckets=`. No default. |
| **Bounded by config** | `top_k` (default `k=10`), `entropy` and `event_type_mix` (default `max_categories=256`), `distance_from_home` (default 100-point ring) | Configurable cap, sensible default, hard ceiling. |

Operators that don't fit any of these classes can't run lifetime. They have to declare a `window=`, or they get rejected.

## The three guardrails

The ~7 KB / entity number rests on three guardrails. Together they bound the only memory dimension beava ships with: `entities × per-entity bytes`.

**1. Cold-entity TTL** (opt-in, per source). `@bv.event(cold_after="30d")` lets the engine evict entities that haven't received an event for the configured duration. Eviction is lazy — it happens inline at the next entity-state lookup, no background thread. Range: `[1s, 365d]`. Default is no expiry, so the behavior is opt-in only.

**2. Lifetime ceiling** (always-on, register-time). Every lifetime aggregation declares a finite ceiling at register time. Operators that need a kwarg without one get rejected. The check runs before the engine deserializes the payload, so the rejection contract stays stable as the operator catalogue evolves.

**3. Per-event bucket reclaim** (always-on). Inside an active entity, windowed operators trim trailing buckets that have rolled past the cap on every event. No idle reclamation — buckets evict as new events arrive. Idle entities that aren't getting events are the cold-TTL guardrail's problem, not this one's.

## What rejection looks like

A user pushes `bv.first_n("amount")` without supplying `n=`. The register payload comes back with a structured error:

```json
{
  "code": "unbounded_op_in_lifetime_mode",
  "message": "Operator 'first_n' requires kwarg 'n' in lifetime mode (window= omitted). Provide n=<int> to bound per-entity memory.",
  "node": "lifetime_first_purchase"
}
```

The error names the operator and the missing kwarg. Fix it by adding `n=20`, or by supplying a `window=` and running it as a sliding window instead.

> **Heads up:** the same check applies to global aggregations (no `key=`). The single global slot is bounded by the same op-class contract — see [global aggregation](../concepts/global-aggregation.md).

For the full error envelope and other governance-related codes, see [error codes](../error-codes.md).

## Observable in production

The `/metrics` endpoint exposes counters that let operators watch the budget hold:

- `beava_cold_entity_evictions_total` — cold-TTL evictions fired.
- `beava_lifetime_op_cap_hit_total` — capped sketch / top-K / histogram entries.
- `beava_entity_count_resident` — resident entity count.
- `beava_bucket_reclaim_total` — windowed-bucket evictions.

See [observability](./observability.md) for the full Prometheus surface.

## What this buys you

A registered pipeline can be sized from its declarations alone. Per-entity bytes is computable at register time; entity count is bounded by ingest plus TTL; total memory is the product. There is no runtime surprise where a feature decides, six weeks into production, that it needs another 4 KB per user.

That's the trade. You give up the ability to register operators whose memory cost is a function of the data. You get a hard, predictable, sizable system in return.

## Cross-references

- [Memory budget](./memory-budget.md) — the per-entity numbers and the 7 KB / entity ceiling.
- [Single-thread apply](./single-thread-apply.md) — why eviction is inline rather than background.
- [Global aggregation](../concepts/global-aggregation.md) — how the same governance applies to global tables.
- [Error codes](../error-codes.md) — the full structured-error surface.
