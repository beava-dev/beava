# bv.min

> Minimum value of a numeric field over a window.

## Signature

```python
bv.min(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.min` returns the smallest observed value of a numeric field across events
that match the optional `where=` predicate. The operator preserves the field's
type — `i64` in stays `i64` out; `f64` in stays `f64` out. Per-event update
is one comparison plus an optional Value clone (for string-keyed retention),
making it Tier 1.

Use `bv.min("temperature", window="5m")` for "lowest temperature reading in
the last 5 minutes", or `bv.min("amount", window="24h", where=bv.col("status")
== "settled")` for "smallest settled amount today". Both `field` and `window`
are required.

`bv.min` belongs to the **core** family alongside [`bv.max`](./max.md). The
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

### Example 1: Lowest temperature reading per sensor, last 5 minutes

```python
import beava as bv

@bv.event
class TempReading:
    sensor_id: str
    temperature: float

@bv.table(key="sensor_id")
def SensorStats(readings) -> bv.Table:
    return (
        readings.group_by("sensor_id")
                .agg(min_temp_5m=bv.min("temperature", window="5m"))
    )

# Push events
app.push("TempReading", {"sensor_id": "s1", "temperature": 21.5})
app.push("TempReading", {"sensor_id": "s1", "temperature": 19.8})
app.push("TempReading", {"sensor_id": "s1", "temperature": 23.0})

# Query
result = app.get("SensorStats", "s1")
# result == {"min_temp_5m": 19.8}
```

### Example 2: Smallest successful payment amount per user

```python
@bv.table(key="user_id")
def UserPaymentExtremes(payments) -> bv.Table:
    return (
        payments.group_by("user_id")
                .agg(min_amount_settled=bv.min("amount",
                                                 window="24h",
                                                 where=bv.col("status") == "settled"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "SensorStats",
  "output_kind": "table",
  "key": ["sensor_id"],
  "agg": {
    "min_temp_5m": {
      "op": "min",
      "params": {
        "field": "temperature",
        "window": "5m"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null` — no events ⇒ no min defined.
- **Non-numeric field:** rejected at register time with `schema_mismatch`.
- **Null / missing field on an event:** event is skipped (does not poison the running min).
- **NaN inputs:** propagate per IEEE-754 — comparison against NaN is false, so a NaN value is silently ignored when an existing min already holds a real number, but it can become the retained min if it arrives first. Filter with `where=~bv.col("amount").isnull()` if your source can emit NaN.
- **Missing `window=`:** `ValueError` at SDK-helper-call time. Use `window="forever"` for explicit lifetime min.
- **Lifetime mode (`window="forever"`):** explicitly allowed — `O(1)` per entity. Min is monotonically non-increasing across the entity's lifetime.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.max](./max.md) — symmetric companion
- [bv.first_n](../point-ordinal/first_n.md) / [bv.last_n](../point-ordinal/last_n.md) — retain N values, not just the extreme
- [bv.quantile](../sketch/quantile.md) — order-statistics across the full distribution
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
