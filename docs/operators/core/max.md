# bv.max

> Maximum value of a numeric field over a window.

## Signature

```python
bv.max(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.max` returns the largest observed value of a numeric field across events
that match the optional `where=` predicate. The operator preserves the field's
type — `i64` in stays `i64` out; `f64` in stays `f64` out. Per-event update
is one comparison plus an optional Value clone, making it Tier 1.

Use `bv.max("amount", window="1h")` for "highest transaction amount in the
last hour", or `bv.max("temperature", window="5m", where=bv.col("sensor_kind")
== "indoor")` for "warmest reading among indoor sensors recently". Both `field`
and `window` are required.

`bv.max` belongs to the **core** family alongside [`bv.min`](./min.md). The
two share the same per-entity state shape (single-field retention) but no
state at the operator level — they are two separate aggregations even when
you register both in the same `agg(...)` block.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the numeric field (`i64` or `f64`). |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events contribute. |

## Returns

A single value of the same type as `field` (`i64` or `f64`). When the entity
has seen zero matching events, the result is `null` (Python `None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~10 ns algorithm floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single retained value per bucket (≤64 buckets) |
| Lifetime mode (`window="forever"`) | **Allowed** — `O(1)` footprint per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Largest transaction amount per user, hourly

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserTxnExtremes(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(max_amount_1h=bv.max("amount", window="1h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "amount": 12.50})
app.push("Txn", {"user_id": "alice", "amount": 1500.00})

# Query
result = app.get("UserTxnExtremes", "alice")
# result == {"max_amount_1h": 1500.0}
```

### Example 2: Highest indoor temperature reading in the last 5 minutes

```python
@bv.table(key="sensor_id")
def IndoorPeaks(readings) -> bv.Table:
    return (
        readings.group_by("sensor_id")
                .agg(max_temp_indoor=bv.max("temperature",
                                              window="5m",
                                              where=bv.col("sensor_kind") == "indoor"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserTxnExtremes",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "max_amount_1h": {
      "op": "max",
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

- **Empty stream / cold-start:** result is `null` — no events ⇒ no max defined.
- **Non-numeric field:** rejected at register time with `schema_mismatch`.
- **Null / missing field on an event:** event is skipped (does not poison the running max).
- **NaN inputs:** propagate per IEEE-754 — a NaN value can become the retained max if it arrives first, but is silently ignored once a real number is established. Filter with `where=~bv.col("amount").isnull()` if your source can emit NaN.
- **Missing `window=`:** `ValueError` at SDK-helper-call time. Use `window="forever"` for explicit lifetime max.
- **Lifetime mode (`window="forever"`):** explicitly allowed — `O(1)` per entity. Max is monotonically non-decreasing across the entity's lifetime.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.min](./min.md) — symmetric companion
- [bv.first_n](../point-ordinal/first_n.md) / [bv.last_n](../point-ordinal/last_n.md) — retain N values, not just the extreme
- [bv.quantile](../sketch/quantile.md) — order-statistics across the full distribution
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
