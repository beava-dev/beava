# bv.rate_of_change

> Rate of change of a numeric field across consecutive matching events — `(value_curr - value_prev) / Δt_ms`.

## Signature

```python
bv.rate_of_change(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.rate_of_change` returns the per-millisecond rate of change of a
numeric `field` between the two most recent matching events seen in the
window. On each new matching event the helper computes
`current_rate = (x_curr - x_prev) / (now_ms_curr - now_ms_prev)` and
overwrites the stored rate; subsequent reads return that scalar. `Δt`
uses **server processing-time** (`now_ms()` between consecutive matching
arrivals) per
[`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md) —
beava intentionally has no event-time concept, so older events here means
"older by arrival order, with elapsed wall-time between arrivals as the
denominator".

This is the canonical "is this entity accelerating?" primitive — useful
when you want to flag a sudden spike in a smoothly-evolving signal
(transaction amount, click rate, sensor reading) instead of looking at
the absolute value alone. Combine it with [`bv.outlier_count`](./outlier_count.md)
to count how many recent events broke a threshold, or with
[`bv.trend`](./trend.md) for a window-wide regression slope rather than a
two-event delta. Pair it with [`bv.delta_from_prev`](./delta_from_prev.md)
when you want the absolute jump rather than the per-millisecond rate.

`bv.rate_of_change` belongs to the **velocity** family. Per-event update
is two scalar reads, one subtraction, and one division (no `exp()`,
no `sqrt()`); cost is **Tier 1** (~10 ns algorithm floor / ~30 ns
measured) and memory is `O(1)` per entity (`last_value`, `last_t`,
`current_rate`, two flags). The `window=` kwarg is **required** by the
Python SDK helper; the inner `RateOfChangeState` is itself lifetime-bound
`O(1)`, but windowed dispatch is the v0 contract for this op.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Numeric field (`i64` or `f64`) to track. Non-numeric values are silently skipped. |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. See [shared.md window grammar](../../sdk-api/shared.md). |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the rate. |

## Returns

A single `f64` — the rate of change in **units-per-millisecond**. Cold-start (no matching event seen) and one-event start (`Δt = 0`, no prior rate computed yet) both return `null` (Python `None`). Multiply by `1000.0` for units-per-second; by `60_000.0` for units-per-minute.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~10 ns floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `RateOfChangeState` ≈ 32 B (`last_value: f64`, `last_t: i64`, `current_rate: f64`, `initialized: bool`, `has_rate: bool`) |
| Lifetime mode (`window="forever"`) | **Allowed** — classified `O1` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Per-second transaction-amount rate of change

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserAmtRate(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(amt_rate_1h=bv.rate_of_change("amount", window="1h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "amount": 100.0}) # rate = null (single event)
app.push("Txn", {"user_id": "alice", "amount": 250.0}) # rate = (250-100)/Δt_ms

# Query
result = app.get("UserAmtRate", "alice")
# result == {"amt_rate_1h": <float, units per ms>}
# Multiply by 1000 for units per second.
```

### Example 2: Filtered rate of change of approved-payment amounts

```python
@bv.table(key="user_id")
def UserOkAmtRate(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(ok_amt_rate=bv.rate_of_change(
                     "amount",
                     window="30m",
                     where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserAmtRate",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "amt_rate_1h": {
      "op": "rate_of_change",
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

- **Empty stream / cold-start:** result is `null`. The first matching event seeds `(last_value, last_t)` and flips `initialized` but does **not** set a rate yet.
- **Single-event entity:** result is `null` until a second matching event arrives.
- **Two events at the same `now_ms` (`Δt = 0`):** the helper skips the rate update (no division by zero) and refreshes `(last_value, last_t)`. The previously-computed rate is preserved.
- **Late or duplicate event (`Δt < 0`):** treated identically to `Δt = 0` — no rate update, but `(last_value, last_t)` are refreshed. Time never moves backward.
- **Missing or non-numeric `field`:** the event is silently skipped (no update); the rate is unchanged. Matches the [`bv.sum`](../core/sum.md) / [`bv.mean`](../core/mean.md) behavior.
- **`where=` filter excludes the event:** no update; non-matching events do not advance `last_t` either.
- **Missing `window=`:** raises `ValueError` at SDK-helper-call time.
- **Malformed `window=`:** raises `ValueError` at SDK-helper-call time; if it somehow reaches the server, `register_validate.rs` returns structured error `aggregation_invalid_window`.
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the underlying state per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md); the next post-eviction matching event reseeds `(last_value, last_t)`.

## See also

- [Velocity family index](./index.md) — overview of all 9 velocity-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.delta_from_prev](./delta_from_prev.md) — absolute jump (no `Δt` denominator)
- [bv.trend](./trend.md) — slope of an OLS regression over the whole window (smoother than a two-event delta)
- [bv.ewma](../decay/ewma.md) — exponentially-weighted mean for smoothing the underlying signal before differencing
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
