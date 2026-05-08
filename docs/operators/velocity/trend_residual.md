# bv.trend_residual

> Most recent value minus the value predicted by the OLS trend line — "is this event consistent with the trend?".

## Signature

```python
bv.trend_residual(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.trend_residual` answers "given the linear trend of `field` over the
window, does the latest value sit on the line, above it, or below it?".
On every matching event the helper folds `(now_ms, field)` into the
same OLS regression as [`bv.trend`](./trend.md) (sums of `x`, `y`, `x²`,
`xy`, plus event count `n`) and additionally caches the most recent
`(value, now_ms)`. The query computes the regression slope and intercept,
predicts the value at the cached `now_ms`, and returns
`current_value − predicted = current_value − (slope · now_ms + intercept)`.
A residual near zero means "this event is on the trend"; a large positive
or negative residual means "this event broke from the trend".

This is the canonical "anomaly versus its own trend" primitive — useful
for detecting events that break a previously-established direction
without having to commit to a static threshold. A 5% rise on a flat
series is suspicious; a 5% rise on an already-rising series is on-trend.
Compared to [`bv.z_score`](./z_score.md), which compares against an
**unweighted** mean and stddev (no notion of trajectory), `trend_residual`
implicitly subtracts off the linear drift first — much better for signals
that legitimately rise or fall over time. Pair it with
[`bv.outlier_count`](./outlier_count.md) when you want a bounded count of
breaks rather than the magnitude of the latest one.

`bv.trend_residual` belongs to the **velocity** family. The state shape
embeds a full `TrendState` (the four running sums + `n`) plus the
last-event cache; per-event update is one numeric extract, four scalar
adds, and two scalar writes; cost is **Tier 1** (~16 ns floor / ~36 ns
measured) and memory is `O(1)` per entity. The query path is a constant
amount of arithmetic — no iteration over history. The `window=` kwarg is
**required** by the Python SDK helper.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Numeric field (`i64` or `f64`) to track. Non-numeric values are silently skipped. |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the regression and the last-event cache. |

## Returns

A single `f64` — the residual of the most recent matching event relative to its trend-line prediction, in the same units as `field`. Cold-start, one-event start (`n < 2`), or a degenerate denominator (every point at the same `now_ms`) all return `null` (Python `None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~16 ns floor / ~36 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `TrendResidualState` ≈ 72 B (embeds `TrendState` ~48 B plus `last_value: f64`, `last_t: i64`, `initialized: bool`) |
| Lifetime mode (`window="forever"`) | **Allowed** — classified `O1` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Detect off-trend transaction amount per user

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserAmtResidual(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(amt_residual_1h=bv.trend_residual("amount", window="1h"))
    )

# Push events on a steady upward trend
app.push("Txn", {"user_id": "alice", "amount": 100.0})
app.push("Txn", {"user_id": "alice", "amount": 110.0})
app.push("Txn", {"user_id": "alice", "amount": 120.0})
# Now an off-trend spike:
app.push("Txn", {"user_id": "alice", "amount": 500.0})

# Query
result = app.get("UserAmtResidual", "alice")
# result == {"amt_residual_1h": <large positive f64 — last event broke the trend>}
```

### Example 2: Filtered fraud-score residual per session

```python
@bv.table(key="session_id")
def SessionScoreResidual(events) -> bv.Table:
    return (
        events.group_by("session_id")
              .agg(risk_residual=bv.trend_residual(
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
  "name": "UserAmtResidual",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "amt_residual_1h": {
      "op": "trend_residual",
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
- **Slope undefined** (degenerate denominator: every matching event at the same `now_ms`): result is `null`.
- **Latest event exactly on the trend line:** residual is `0.0`, not `null`. The helper has computed a residual; it just happens to be zero.
- **Constant signal:** slope is `0.0`, intercept is the constant; residual is `0.0` for every event.
- **Missing or non-numeric `field`:** the event is silently skipped (no update); the trend state and last-event cache are unchanged.
- **`where=` filter excludes the event:** no update — neither the regression nor the last-event cache is refreshed by non-matching events. The residual continues to reflect the previous matching event's deviation.
- **Missing `window=`:** raises `ValueError` at SDK-helper-call time.
- **Malformed `window=`:** raises `ValueError` at SDK-helper-call time; if it somehow reaches the server, `register_validate.rs` returns structured error `aggregation_invalid_window`.
- **Numerical precision over very long lifetimes:** same caveat as [`bv.trend`](./trend.md) — the four running sums grow with `n`; for `window="forever"` on a busy entity the sums can grow large enough to lose FP precision. Prefer a fixed `window=` ≤ 1d on long-lived entities.
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the underlying state per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md); the next post-eviction matching event reseeds.

## See also

- [Velocity family index](./index.md) — overview of all 9 velocity-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.trend](./trend.md) — companion "what is the slope?" primitive (shares regression state)
- [bv.z_score](./z_score.md) — unweighted (mean / stddev)-based deviation; pick when there is no expected trajectory
- [bv.outlier_count](./outlier_count.md) — bounded count of deviation events rather than magnitude of the latest
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
