# Recency Aggregation Operators

The 10 recency ops cover **time-since semantics** (how long since first / most-recent match), **windowed-recency booleans** (was the last match within window N?), and **streak counters** (consecutive matches and non-matches). All recency ops use **server processing-time** (`now_ms()` at the apply path) per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md) — beava intentionally has no event-time concept (post-2026-05-19: partially overturned by ADR-004 for v0.1 bucketing).

| Op | Returns | Time source | Notes |
|----|---------|-------------|-------|
| [`bv.first_seen`](./first_seen.md) | `Datetime` (i64 ms) | server `now_ms()` at apply | First match's server arrival timestamp |
| [`bv.last_seen`](./last_seen.md) | `Datetime` (i64 ms) | server `now_ms()` at apply | Most recent match's server arrival timestamp |
| [`bv.age`](./age.md) | `i64` ms | computed at **read time** | `now_ms() - first_seen` |
| [`bv.has_seen`](./has_seen.md) | `bool` | n/a | Cumulative ever-matched flag |
| [`bv.time_since`](./time_since.md) | `i64` ms or `null` | computed at **read time** | `now_ms() - last_seen` |
| [`bv.time_since_last_n`](./time_since_last_n.md) | `i64` ms or `null` | computed at **read time**; `n` required | Generalization: ms since the kth most recent match |
| [`bv.streak`](./streak.md) | `i64` | event arrival order | Live consecutive-match counter |
| [`bv.max_streak`](./max_streak.md) | `i64` | event arrival order | All-time max of `streak`; never decreases |
| [`bv.negative_streak`](./negative_streak.md) | `i64` | event arrival order | Live consecutive-non-match counter (mirror of `streak`) |
| [`bv.first_seen_in_window`](./first_seen_in_window.md) | `bool` | `now_ms() - last_ms < window` at read time; `window=` required | "Was the last match within the last N ms?" |

## Key invariants

- **Server processing-time only.** Per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md) (locked 2026-04-30) (post-2026-05-19: partially overturned by ADR-004 for v0.1 bucketing), beava records server `now_ms()` at apply for `first_seen` / `last_seen`, and computes elapsed-ms using server `now_ms()` at read for `age` / `time_since` / `time_since_last_n` / `first_seen_in_window`. Producers cannot influence captured timestamps via payload fields for recency operators, though v0.1 supports event-time bucketing via a reserved `_ts` field. The recency operators themselves continue to use server processing-time for their timestamp semantics.
- **Read-time computation.** `age`, `time_since`, `time_since_last_n`, and `first_seen_in_window` change between reads **without any new events** — the right-hand side of the elapsed calculation is captured at query time, not apply time. This makes them useful staleness/recency features.
- **`bv.time_since_last_n` requires `n`.** Per [V0-MEM-GOV-02 BoundedByRequiredKwarg("n")](../../../.planning/REQUIREMENTS.md), the deque of timestamps must have a register-time ceiling. Missing `n` is rejected by the JSON-prelude shim with code `unbounded_op_in_lifetime_mode`.
- **`bv.first_seen_in_window` requires `window`.** The windowed-recency boolean is meaningless without a horizon length. The `window` parameter is enforced at register time; `"forever"` is rejected (use [`bv.has_seen`](./has_seen.md) for that semantic).
- **Cold-start behavior is per-op.** Streaks return `0`. Booleans (`has_seen`, `first_seen_in_window`) return `false`. Datetime/duration ops (`first_seen`, `last_seen`, `age`, `time_since`, `time_since_last_n`) return `null`.
- **Cold-entity eviction (`@bv.event(cold_after=...)`)** drops the underlying state per the Redis-TTL pattern (V0-MEM-GOV-01); recency state rebuilds from the next post-eviction event.
- **9 of 10 ops share `SeenState`** (`first_seen`, `last_seen`, `age`, `has_seen`, `time_since`) or related `StreakState` (`streak`, `max_streak`) / `NegativeStreakState` / `FirstSeenInWindowState` — registering several siblings on the same `where=` predicate costs roughly the same as registering one.

## See also

- [Operator catalog index](../index.md) — full 53-op catalogue
- [cost-class.md](../cost-class.md) — per-op CPU tier metadata (all recency ops are Tier 1)
- [Point/ordinal family](../point-ordinal/) — value-based first/last/N counterparts to the timestamp ops here
- Per-operator memory governance: [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — every lifetime aggregation operator declares a finite per-entity memory ceiling at register-time
- [Pipeline DSL compilation rules](../../pipeline-dsl/compilation-rules.md) — how `bv.<op>(...)` calls compile to JSON wire form
