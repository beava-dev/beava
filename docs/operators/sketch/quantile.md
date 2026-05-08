# bv.quantile

> Approximate quantile of a numeric field over a window or lifetime, backed by DDSketch.

## Signature

```python
bv.quantile(
    field: str,
    q: float,
    *,
    window: str | None = None,
    where: bv.Col | None = None,
    exact_threshold: int = 256,
    hybrid_alpha: float = 0.01,
) -> AggDescriptor
```

> Previously called `bv.percentile`. Renamed to `quantile` per [ADR-002](../../../.planning/decisions/ADR-002-polars-op-rename.md) for Polars-convention consistency. The old name remains as a deprecation alias in v0.0.x and is removed in v0.1.

## Description

`bv.quantile` estimates the q-th quantile (0 ≤ q ≤ 1) of a numeric field
across events that match the optional `where=` predicate. Backed by a hybrid
exact-then-DDSketch state: while the entity has fewer than `exact_threshold`
distinct values, the state holds them exactly and returns the precise q-th
order statistic. Once the threshold is crossed, the state promotes to a
DDSketch with relative accuracy `hybrid_alpha` (default 0.01 = 1% relative
error).

Returns a single `f64` per entity. Use `bv.quantile(field="amount", q=0.99,
window="1h")` for "the 99th-percentile transaction amount over the last hour".
Pair with [`bv.mean`](../core/mean.md) and [`bv.std`](../core/std.md) when
fraud rules need both central-tendency and tail behavior.

The hybrid mode is transparent at the API: callers always read a single
float. Promotion happens server-side without observable behavior change
beyond a small accuracy tradeoff. `bv.quantile` belongs to the **sketch**
family and is `BoundedSketch` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — fixed structural cap regardless of stream length.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the numeric field (`f64` or `i64`) to take the quantile of. |
| `q` | `float` | Yes | — | The quantile to compute, in `[0, 1]`. `0.5` is the median; `0.99` is the 99th percentile. |
| `window` | `str` | No | `None` (lifetime) | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events contribute. |
| `exact_threshold` | `int` | No | `256` | Distinct-value count below which exact mode is used. |
| `hybrid_alpha` | `float` | No | `0.01` | DDSketch relative-error parameter once promoted (0.01 = 1% relative error). |

## Returns

A single `f64`. When the entity has seen zero matching events, the result is
`null` (returned as Python `None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 2** (Exact mode, ~8 ns floor / ~35 ns measured) — see [cost-class.md](../cost-class.md#tier-2-moderate-30-100-nscall--6-ops) |
| | **Tier 3** (DDSketch mode, post-promotion, ~130 ns floor / ~180 ns measured) — see [cost-class.md](../cost-class.md#tier-3-algorithmic-floor-100-300-nscall--9-ops) |
| Memory per entity | `BoundedSketch` — exact array up to `exact_threshold` entries, then DDSketch buckets (~few KB max) regardless of stream length |
| Lifetime mode (`window=None`) | **Allowed** — `BoundedSketch` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: 99th-percentile transaction amount per user, hourly

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserAmountStats(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(p99_amount_1h=bv.quantile("amount", q=0.99, window="1h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "amount": 12.50})
app.push("Txn", {"user_id": "alice", "amount": 1500.00})

# Query
result = app.get("UserAmountStats", "alice")
# result == {"p99_amount_1h": 1500.0}
```

### Example 2: Median latency for successful events only

```python
@bv.table(key="user_id")
def LatencyP50(reqs) -> bv.Table:
    return (
        reqs.group_by("user_id")
            .agg(median_lat_ok=bv.quantile("latency_ms",
                                            q=0.5,
                                            window="5m",
                                            where=bv.col("status") == 200))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserAmountStats",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "p99_amount_1h": {
      "op": "quantile",
      "params": {
        "field": "amount",
        "q": 0.99,
        "window": "1h",
        "exact_threshold": 256,
        "hybrid_alpha": 0.01
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full pipeline example (uses `quantile` for `tx_p99_1h`).

## Edge cases

- **No matching events / cold-start:** result is `null`. (Cold-start returns `{}` for the row, not an error.)
- **`q` out of range:** values outside `[0, 1]` raise `RegistrationError(code="aggregation_invalid_param")` at register time.
- **Non-numeric field:** schema validation rejects at register time with structured error code `schema_mismatch`.
- **NaN inputs:** poisons the sketch state in DDSketch mode (NaN routes to a non-finite bucket); filter with `where=~bv.col("amount").isnull()` if your source can emit NaN.
- **Lifetime mode (`window=None`):** explicitly allowed — DDSketch is `BoundedSketch` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).
- **Hybrid promotion:** transparent. Callers cannot observe whether the entity is in exact or sketch mode (intentional). Promotion happens once the entity has seen `exact_threshold` distinct values.
- **`exact_threshold` lowered to 0:** equivalent to "always sketch mode"; useful only for explicit memory tuning.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 2 exact / Tier 3 sketch)
- [bv.entropy](./entropy.md) — also a `BoundedSketch` for distributions
- [bv.top_k](./top_k.md) — heavy-hitters companion
- [bv.n_unique](./n_unique.md) — cardinality companion
- [bv.min](../core/min.md) / [bv.max](../core/max.md) — extreme-value companions (Tier 1)
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
