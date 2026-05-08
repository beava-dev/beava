# bv.trend

> Slope of an online linear regression of `(now_ms, field)` over the window — sign + magnitude of "is this entity going up or down?".

## Signature

```python
bv.trend(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.trend` fits an ordinary-least-squares (OLS) line to the
`(now_ms, value)` points of every matching event in the window and
returns the **slope** of that line. On every matching event the helper
folds `(x = now_ms, y = field)` into four running sums
(`Σx, Σy, Σx², Σxy`) plus an event count `n`; the query computes the
closed-form slope `(n·Σxy − Σx·Σy) / (n·Σx² − Σx²)`. A positive slope
means "this entity's `field` value is rising over the window"; negative
means "falling"; zero means "flat or noisy". The magnitude is the
slope of the best-fit line in **field-units per millisecond**.

This is the canonical "directional drift" primitive — useful for any
gauge-style signal where the trajectory matters more than the absolute
value (rising fraud-score over a session, declining account balance,
accelerating click-rate, drifting sensor reading). Compared to
[`bv.rate_of_change`](./rate_of_change.md), which uses only the two most
recent events and is **noisy** in choppy series, `bv.trend` smooths
across all matching events in the window — much more robust to single
outliers but slower to react to a genuine regime change. Pair it with
[`bv.trend_residual`](./trend_residual.md) when you also want to flag
"is this latest event consistent with the trend?".

`bv.trend` belongs to the **velocity** family. Per-event update is one
numeric extract plus four scalar adds (no `exp()`, no `sqrt()`); cost is
**Tier 1** (~12 ns floor / ~32 ns measured) and memory is `O(1)` per
entity (`n` plus four `f64` sums plus the `initialized` flag). The
`window=` kwarg is **required** by the Python SDK helper; the inner
`TrendState` is itself lifetime-bound `O(1)`.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Numeric field (`i64` or `f64`) to track. Non-numeric values are silently skipped. |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. See [shared.md window grammar](../../sdk-api/shared.md). |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the regression sums. |

## Returns

A single `f64` — the OLS slope of `(now_ms, field)`, in **field-units per millisecond**. Multiply by `1000.0` for units-per-second; by `60_000.0` for units-per-minute. Cold-start and one-event start (`n < 2`) both return `null` (Python `None`); a degenerate denominator `n·Σx² − (Σx)² == 0` (which only happens when every point shares the same `now_ms`) also returns `null`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~12 ns floor / ~32 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `TrendState` ≈ 48 B (`n: u64`, `sum_x: f64`, `sum_y: f64`, `sum_xx: f64`, `sum_xy: f64`, `initialized: bool`) |
| Lifetime mode (`window="forever"`) | **Allowed** — classified `O1` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Per-user transaction-amount trend over the last hour

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserAmtTrend(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(amt_slope_1h=bv.trend("amount", window="1h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "amount": 100.0})
app.push("Txn", {"user_id": "alice", "amount": 150.0})
app.push("Txn", {"user_id": "alice", "amount": 200.0})

# Query
result = app.get("UserAmtTrend", "alice")
# result == {"amt_slope_1h": <positive f64 — rising trend, units-per-ms>}
```

### Example 2: Filtered fraud-score trend per session

```python
@bv.table(key="session_id")
def SessionScoreTrend(events) -> bv.Table:
    return (
        events.group_by("session_id")
              .agg(risk_slope=bv.trend(
                       "fraud_score",
                       window="10m",
                       where=bv.col("event_type") == "scored"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserAmtTrend",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "amt_slope_1h": {
      "op": "trend",
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

- **Empty stream / cold-start (`n = 0`):** result is `null`.
- **Single-event entity (`n = 1`):** result is `null` — at least two matching events are required to define a slope.
- **All matching events at the same `now_ms` (degenerate `Σx²`):** denominator collapses to zero; the helper returns `null` rather than dividing by zero. This only happens when many matching events arrive within the same `now_ms()` granularity.
- **Constant signal (e.g. `field == 5.0` everywhere):** slope is mathematically `0.0`. The helper returns `0.0`, not `null`.
- **Missing or non-numeric `field`:** the event is silently skipped (no update); the regression state is unchanged. Matches the [`bv.sum`](../core/sum.md) / [`bv.mean`](../core/mean.md) behavior.
- **`where=` filter excludes the event:** no update; non-matching events do not contribute to `n`, `Σx`, etc.
- **Missing `window=`:** raises `ValueError` at SDK-helper-call time.
- **Malformed `window=`:** raises `ValueError` at SDK-helper-call time; if it somehow reaches the server, `register_validate.rs` returns structured error `aggregation_invalid_window`.
- **Numerical precision over very long lifetimes:** the four running sums grow with `n`; for `window="forever"` on a busy entity the sums can grow large enough to lose FP precision. For long-lifetime trend tracking prefer a fixed `window=` ≤ 1d, or use [`bv.ewma`](../decay/ewma.md) which has bounded magnitude by design.
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the underlying state per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md); the next post-eviction matching event reseeds an empty regression.

## See also

- [Velocity family index](./index.md) — overview of all 9 velocity-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.trend_residual](./trend_residual.md) — companion "is this event consistent with the trend?" primitive (shares state with `bv.trend`)
- [bv.rate_of_change](./rate_of_change.md) — two-event delta; noisier but reacts faster
- [bv.ewma](../decay/ewma.md) — exponentially-weighted mean for smoothing the underlying signal
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
