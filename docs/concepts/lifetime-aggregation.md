# Lifetime Aggregation

When an aggregation operator omits the `window=` kwarg, it accumulates over
the entity's entire history with no rollover, no bucket eviction, one slot
per entity. This is **lifetime mode**. It's the right answer for
"how many transactions has this user ever made," "first time we saw this
device," "lifetime average dwell time" — features whose semantics are the
whole-history reduction, not a sliding window.

Lifetime mode is the default when `window=` is absent. beava enforces a
hard register-time contract on which operators are allowed to run lifetime:
every lifetime op must declare a finite per-entity memory ceiling. The
contract is V0-MEM-GOV-02; the enforcement is structural and runs before
any user payload is even fully deserialized.

## What lifetime mode means

Same operator catalogue, two execution shapes:

```python
# Sliding window — 1-hour rolling sum
bv.sum("amount", window="1h")

# Lifetime — running sum over all events ever seen for this entity
bv.sum("amount")
```

In sliding-window mode, the windowed-op data structure (`WindowedOp`) holds
up to 64 buckets of state, each indexed by server-side `now_ms()` modulo
the window step. Buckets evict as time advances; per-event memory cost is
bounded by `64 × size_of::<inner state>`.

In lifetime mode, there is one slot of state per entity. The slot holds
whatever the operator's reducer accumulates — for `sum`, a single `f64`;
for `count`, a single `u64`; for `n_unique`, an HLL sketch.

## When to use lifetime mode

- **Recency markers** — `first_seen`, `last_seen`, `age`, `time_since`,
  `has_seen`. These are inherently lifetime concepts.
- **Lifetime totals** — `count`, `sum`, `mean`, `min`, `max` over the
  whole history of an entity. Useful for cohort analytics + churn
  detection + "ever vs never" features.
- **First / last accessors** — `first`, `last`, `first_n`, `last_n`,
  `lag`. Read the earliest or latest N events.
- **Streaks** — `streak`, `max_streak`, `negative_streak`. State is
  fixed-size regardless of stream length.
- **Decay accumulators** — `ewma`, `ewvar`, `ew_zscore`, `decayed_sum`,
  `decayed_count`, `twa`. Half-life-weighted reduction over the entire
  history; per-entity state is one or two scalars.
- **Bounded sketches** — `n_unique` (HLL), `quantile` (DDSketch),
  `bloom_member`. State is bounded by sketch parameters, not by stream
  length.

## V0-MEM-GOV-02 contract

Every operator that legally runs lifetime declares a finite per-entity
memory ceiling at register-time. The contract is V0-MEM-GOV-02 in
[`.planning/REQUIREMENTS.md`](../../.planning/REQUIREMENTS.md):

> Every lifetime aggregation operator (windowless mode) declares a finite
> per-entity memory ceiling at register-time.

The classification lives in
[`crates/beava-core/src/register_validate.rs`](../../crates/beava-core/src/register_validate.rs)
under `lifetime_bound_for_op_str`. It returns one of four `OpLifetimeBound`
variants per operator string.

### Memory bound classifications

| Class                                   | Examples                                                                                              | Per-entity bound                                  |
| --------------------------------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| `O1`                                    | `count`, `sum`, `mean`, `min`, `max`, `var`, `std`, `ratio`, `first`, `last`, `has_seen`, `first_seen`, `last_seen`, `age`, `time_since`, `streak`, `max_streak`, `negative_streak`, decay family, velocity / trend family | One scalar (or fixed pair) per entity.            |
| `BoundedSketch`                         | `n_unique` (HLL), `quantile` (DDSketch), `bloom_member`                                               | Sketch state — bounded by sketch parameters.      |
| `BoundedByRequiredKwarg("n")`           | `first_n`, `last_n`, `lag`, `time_since_last_n`, `most_recent_n`                                      | `n × size_of::<element>` — caller specifies.       |
| `BoundedByRequiredKwarg("samples")`     | `reservoir_sample`                                                                                    | `samples × size_of::<element>` — caller specifies. |
| `BoundedByRequiredKwarg("buckets")`     | `histogram`                                                                                           | `buckets.len() × size_of::<bucket counter>`.       |
| `BoundedByConfig("max_categories", 256)` | `entropy`, `event_type_mix`                                                                           | Up to 256 distinct categories tracked per entity.  |
| `BoundedByConfig("k", 10)`              | `top_k`                                                                                               | Top-K SpaceSaving sketch — `k` slots default 10.   |
| `BoundedByConfig("samples", 100)`       | `distance_from_home`                                                                                  | Ring buffer of 100 recent geo points.              |

Operators not classifiable as bounded are forbidden in lifetime mode at
register-time.

## Register-time enforcement

A JSON-prelude shim at
[`crates/beava-core/src/register_validate.rs`](../../crates/beava-core/src/register_validate.rs)
called `pre_check_unbounded_op_in_lifetime_mode` walks each register
payload's `nodes[]` array. For every operator declared without a
`window=`, it looks up `lifetime_bound_for_op_str` and rejects with
structured error code `unbounded_op_in_lifetime_mode` if the operator
needs a kwarg the caller didn't provide. The error message names the op
and suggests the missing kwarg.

```text
{
  "code": "unbounded_op_in_lifetime_mode",
  "message": "Operator 'first_n' requires kwarg 'n' in lifetime mode (window= omitted). Provide n=<int> to bound per-entity memory.",
  "node": "lifetime_first_purchase"
}
```

The shim runs **before** strict serde deserialization, so the rejection is
stable even as the underlying `OpNode` enum evolves. This is the same
JSON-prelude shim pattern used by Phases 12.6 / 12.7 / 12.8 to keep
structured error codes durable across code refactors — see
[../schema-evolution.md](../schema-evolution.md) for the full layered
validation pipeline.

The shim is gated by an env var, `BEAVA_MEMORY_GOV_ENFORCE`, default-ON.
Setting `BEAVA_MEMORY_GOV_ENFORCE=0` disables the shim — escape hatch for
ops shop emergency, NOT for production. The escape hatch is read on every
register call (per-call read, not `OnceLock`-cached) so per-test env flips
behave correctly.

## What this catches

The shim catches three classes of register-time mistake:

1. **Required-kwarg omission.** `bv.first_n("amount")` without `n=` — the
   sketch would grow unbounded with the stream.
2. **Forgot `window=`.** Some operators (e.g. `n_unique` over
   high-cardinality fields) are best as windowed aggregations; running
   them lifetime might be fine, might not, but at least the sketch is
   self-bounded so the shim allows it.
3. **Operator entirely unsuitable for lifetime.** Some ops have no
   meaningful lifetime semantic — they're rejected here rather than
   silently misbehaving.

A real CI tripwire (`crates/beava-server/tests/phase12_8_lifetime_ops_have_bounds.rs`)
walks `parse_agg_kind` across every `AggKind` variant and asserts each one
has a classification entry. Adding a new operator that lands in
`agg_compile.rs` but not in `register_validate.rs::lifetime_bound_for_op_str`
fails CI immediately.

## Memory budget connection

V0-MEM-GOV-02 is one of three V0-MEM-GOV invariants (see
[../architecture/memory-budget.md](../architecture/memory-budget.md) for
the full story). Together they make the ~7 KB / entity number defensible:

- **V0-MEM-GOV-01** — opt-in `cold_after=` on `@bv.event` for cold-entity
  TTL eviction. Keeps the entity count from growing without bound.
- **V0-MEM-GOV-02** — this page. Per-op lifetime ceiling at
  register-time. Keeps each entity's state from growing without bound.
- **V0-MEM-GOV-03** — per-event bucket reclaim within active entities
  (always-on Tier 2, no opt-in). Trims trailing buckets as time advances.

Together they bound `entities × per-entity bytes`, which is the only
memory dimension beava ships with.

## Cross-references

- [`CLAUDE.md` § Memory Governance Invariant](../../CLAUDE.md) — locked
  Phase 12.8 contract; cite for the architectural commitment.
- [`.planning/REQUIREMENTS.md`](../../.planning/REQUIREMENTS.md)
  V0-MEM-GOV-02 — the canonical requirement statement.
- [`crates/beava-core/src/register_validate.rs`](../../crates/beava-core/src/register_validate.rs)
  — `pre_check_unbounded_op_in_lifetime_mode` + `lifetime_bound_for_op_str`.
- [../architecture/memory-budget.md](../architecture/memory-budget.md) —
  per-entity memory math; the 7 KB / entity number.
- [../operators/index.md](../operators/index.md) — per-op pages with
  Memory + Lifetime sections per op.
- [../pipeline-dsl/compilation-rules.md](../pipeline-dsl/compilation-rules.md)
  — `window=` kwarg semantics in the chain compiler.
- [../error-codes.md](../error-codes.md) —
  `unbounded_op_in_lifetime_mode` envelope + recovery.
