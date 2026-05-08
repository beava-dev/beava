# Point / Ordinal Aggregation Operators

The 5 point/ordinal ops return specific events from the entity's event stream — first, last, or N-most-recent — without aggregation or summarization. They preserve the source field's type and use processing-time arrival order (not event-time) per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md).

| Op | Memory | CPU | Notes |
|----|--------|-----|-------|
| [`bv.first`](./first.md) | `O(1)` | Tier 1 | First non-null value of the field; sticky |
| [`bv.last`](./last.md) | `O(1)` | Tier 1 | Most recent non-null value of the field |
| [`bv.first_n`](./first_n.md) | `BoundedByRequiredKwarg("n")` | Tier 1 | First N values; `n` required at register time |
| [`bv.last_n`](./last_n.md) | `BoundedByRequiredKwarg("n")` | Tier 1 | Last N values (deque); `n` required |
| [`bv.lag`](./lag.md) | `BoundedByRequiredKwarg("n")` | Tier 1 | Value `n` events ago; `n` required |

Three of five (`first_n`, `last_n`, `lag`) require an explicit `n` kwarg per [V0-MEM-GOV-02 BoundedByRequiredKwarg("n")](../../../.planning/REQUIREMENTS.md) — the lifetime-aggregation memory contract. Without `n` the register-time JSON-prelude shim (`pre_check_unbounded_op_in_lifetime_mode`) rejects the payload with the structured code `unbounded_op_in_lifetime_mode`.

All 5 ops are **lifetime-only** in v0 (no `window=` kwarg). For sliding-window "values in the last N ms" semantics, see [`bv.most_recent_n`](../buffer-geo/most_recent_n.md) (v0 buffer family). For arrival-timestamp variants instead of values, see the [recency family](../recency/).

## See also

- [Operator catalog index](../index.md) — full 53-op catalogue
- [cost-class.md](../cost-class.md) — per-op CPU tier metadata (Tier 1 / 2 / 3)
- [Recency family](../recency/) — timestamp- and streak-based recency ops
- Per-operator memory governance: [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — every lifetime aggregation operator declares a finite per-entity memory ceiling at register-time
- [Pipeline DSL compilation rules](../../pipeline-dsl/compilation-rules.md) — how `bv.<op>(...)` calls compile to JSON wire form
