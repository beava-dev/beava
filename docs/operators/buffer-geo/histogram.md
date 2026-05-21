# bv.histogram

> Fixed-bucket count histogram of a numeric field. `buckets` is a required register-time kwarg per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## Signature

```python
bv.histogram(
    field: str,
    *,
    buckets: list[float],            # REQUIRED — register-time kwarg
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.histogram` returns a count per fixed numeric bucket of `field` across
events that match the optional `where=` predicate. `buckets` is a strictly
increasing list of split points; the cells are `(-inf, b[0])`, `[b[0], b[1])`,
…, `[b[n-1], +inf)`. For `buckets=[10, 20, 50]` you get four cells with
labels `"<10"`, `"10-20"`, `"20-50"`, `">=50"`. Use it for "transaction
amount distribution by tier" or "request size in p50 / p90 / p99 buckets" —
features where you want a coarse shape, not a quantile estimate.

`buckets` is a **required keyword argument** per
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md): the lifetime-aggregation
memory contract requires every unbounded-by-default operator to declare a
finite per-entity ceiling at register time. `bv.histogram`'s ceiling is
exactly `len(buckets) + 1` `u64` counters per entity. The register-time
JSON-prelude shim (`pre_check_unbounded_op_in_lifetime_mode`) rejects any
`histogram` payload missing `buckets` (or with an empty `buckets` array)
with the structured error code `unbounded_op_in_lifetime_mode`. There is
no fallback default — picking the bucket edges is a deliberate
capacity-planning + signal-design step.

`bv.histogram` belongs to the **bounded-buffer** family. Per-event update
is a linear scan over `buckets` (≤ ~20 in practice) plus a saturating
counter increment — Tier 1 floor (~10 ns / ~30 ns measured). The query
path materializes a `BTreeMap` of labelled counts and is therefore listed
under Tier 3 in [cost-class.md](../cost-class.md); the apply-path cost
remains Tier 1. There is no `window=` kwarg in v0 — `bv.histogram` is
**lifetime-only**. For "amount histogram for the last 24 h", compose with
`@bv.event(cold_after=...)` or use [`bv.quantile`](../sketch/quantile.md)
for a quantile sketch instead.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the numeric field to bucket (`f64` / `i64`). |
| `buckets` | `list[float]` | **Yes** | — | Strictly increasing list of split points. `n` values produce `n + 1` cells. Caps per-entity memory at `n + 1` `u64` counters per [V0-MEM-GOV-02 BoundedByRequiredKwarg("buckets")](../../../.planning/REQUIREMENTS.md). |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the counters. |

## Returns

A `dict[str, int]` keyed by bucket label (e.g. `"<10"`, `"10-20"`,
`">=50"`) with `i64` count values. Wire form is `Value::Map` with
`BTreeMap`-sorted iteration. When the entity has seen zero matching
events, the result is the dict with all counters at `0`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~10 ns floor / ~30 ns measured — linear scan over ≤ ~20 buckets + saturating add) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Query | **Tier 3** allocates a `BTreeMap` of `len(buckets) + 1` entries on each `app.get(...)` — see [cost-class.md](../cost-class.md#tier-3-algorithmic-floor-100-300-nscall--9-ops) — apply-thread cost is Tier 1; query-time cost is the asymmetry to flag when profiling |
| Memory per entity | **`BoundedByRequiredKwarg("buckets")`** — `(len(buckets) + 1) × 8` bytes per [Phase 12.8 V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.histogram` has no `window=` kwarg in v0; lifetime is the only mode |

## Examples

### Example 1: Transaction-amount histogram per user

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserAmountHistogram(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(amount_hist=bv.histogram("amount",
                                          buckets=[10.0, 50.0, 100.0, 500.0]))
    )

# Push events
for amt in [5.0, 12.0, 25.0, 80.0, 200.0, 750.0]:
    app.push("Txn", {"user_id": "alice", "amount": amt})

# Query
result = app.get("UserAmountHistogram", "alice")
# result == {"amount_hist": {"<10": 1, "10-50": 2, "50-100": 1, "100-500": 1, ">=500": 1}}
```

### Example 2: Successful-only request size histogram

```python
@bv.table(key="endpoint")
def EndpointReqSizeHist(reqs) -> bv.Table:
    return (
        reqs.group_by("endpoint")
            .agg(req_size_hist=bv.histogram("size_bytes",
                                              buckets=[1024, 65536, 1048576],
                                              where=bv.col("status") == 200))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserAmountHistogram",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "amount_hist": {
      "op": "histogram",
      "params": {
        "field": "amount",
        "buckets": [10.0, 50.0, 100.0, 500.0]
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **`buckets` missing or empty at register time:** rejected with structured error code `unbounded_op_in_lifetime_mode` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). The JSON-prelude shim catches this before any state is allocated.
- **`buckets` not strictly increasing:** rejected with `aggregation_invalid_param` at register time.
- **Empty stream / cold-start:** all counters are `0`; the result dict is the full label set with zero values.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. v0 has no windowed histogram; use `@bv.event(cold_after=...)` to scope the lifetime via cold-entity eviction, or [`bv.quantile`](../sketch/quantile.md) for a windowed shape estimate.
- **Non-numeric source field (`Value::Str`, `Value::Bool`):** event silently dropped (not bucketed). For categorical histograms use [`bv.event_type_mix`](./event_type_mix.md).
- **NaN inputs:** dropped — `numeric_from_row` returns `None` on non-`F64`/`I64` payloads. For cleaner semantics filter with `where=~bv.col(field).isnull()`.
- **Bucket-label format:** integer-valued edges render without trailing `.0` (`"<10"` not `"<10.0"`); fractional edges keep their decimal form. Stable across versions — Python SDKs can dict-key on the labels.
- **Counter overflow:** the per-bucket `u64` saturates at `2^64 - 1` (impossible in practice for a single entity).
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md) (post-2026-05-19: partially overturned by ADR-004 for v0.1 bucketing); buckets are populated in arrival order.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `(len(buckets) + 1) × 8` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) BoundedByRequiredKwarg("buckets").

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1 update / Tier 3 query)
- [bv.quantile](../sketch/quantile.md) — quantile-estimate companion (when you want p50/p90/p99 instead of fixed buckets)
- [bv.event_type_mix](./event_type_mix.md) — categorical-distribution companion (proportions per category, not counts per bucket)
- [bv.hour_of_day_histogram](./hour_of_day_histogram.md) — fixed 24-bin time-of-day histogram (no `buckets=` needed)
- [bv.dow_hour_histogram](./dow_hour_histogram.md) — fixed 168-bin day-of-week × hour histogram
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — BoundedByRequiredKwarg memory governance contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
