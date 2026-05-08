# bv.mean

> Arithmetic mean of a numeric field over a window.

## Signature

```python
bv.mean(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

> Previously called `bv.avg`. Renamed to `mean` per [ADR-002](../../../.planning/decisions/ADR-002-polars-op-rename.md) for Polars-convention consistency. The old name remains as a deprecation alias in v0.0.x and is removed in v0.1.

## Description

`bv.mean` returns the arithmetic mean of a numeric field — the running sum
divided by the matching event count. State is `(running_sum, observation_count)`
maintained per-entity; the division happens at query time. This is the standard
Welford-friendly accumulator and is numerically stable for the typical fraud /
ad-tech magnitudes.

Use `bv.mean("amount", window="1h")` for "average transaction size in the last
hour", or `bv.mean("score", window="24h", where=bv.col("ok"))` for "average
ranking score among successful events today". Like `bv.sum`, both `field` and
`window` are required; the field must be `i64` or `f64` (rejected at register
time with `schema_mismatch` otherwise).

`bv.mean` belongs to the **core** family. Tier 1 cost (~8 ns floor / ~25 ns
measured) and `O(1)` per-entity memory.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the numeric field (`i64` or `f64`). |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events contribute. |

## Returns

A single `f64`. When the entity has seen zero matching events, the result is
`null` (returned as Python `None`) — no division-by-zero, just cold-start
behavior.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns algorithm floor / ~25 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `(sum, count)` plus bucket array (≤64 buckets) |
| Lifetime mode (`window="forever"`) | **Allowed** — `O(1)` footprint per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Average transaction amount per user, hourly

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserMean(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(avg_amount_1h=bv.mean("amount", window="1h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "amount": 10.00})
app.push("Txn", {"user_id": "alice", "amount": 30.00})

# Query
result = app.get("UserMean", "alice")
# result == {"avg_amount_1h": 20.0}
```

### Example 2: Average successful-payment latency over a day

```python
@bv.table(key="user_id")
def PaymentLatency(payments) -> bv.Table:
    return (
        payments.group_by("user_id")
                .agg(mean_latency_ok=bv.mean("latency_ms",
                                              window="24h",
                                              where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserMean",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "avg_amount_1h": {
      "op": "mean",
      "params": {
        "field": "amount",
        "window": "1h"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null` — no events ⇒ no mean to report (no division-by-zero error).
- **Non-numeric field:** rejected at register time with `schema_mismatch`.
- **Missing `window=`:** `ValueError` at SDK-helper-call time. Use `window="forever"` for explicit lifetime mean.
- **NaN inputs:** a single NaN poisons the running sum. Filter with `where=~bv.col("amount").isnull()` if your source can emit NaN.
- **Lifetime mode (`window="forever"`):** explicitly allowed; mean is `O(1)` per entity. Long-lived entities will eventually hit `f64` precision floor on the running sum (~`2^53` matching events for double-precision exactness — not observed in practice).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.sum](./sum.md) — running total (the numerator behind the mean)
- [bv.var](./var.md) / [bv.std](./std.md) — companion second-moment estimators (Welford)
- [bv.ewma](../decay/ewma.md) — exponentially-weighted moving average (decay-based, not window-based)
- [bv.twa](../decay/twa.md) — time-weighted average (gauge-style fields)
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
