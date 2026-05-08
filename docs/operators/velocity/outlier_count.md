# bv.outlier_count

> Count of events whose value deviates from the running mean by more than `sigma · stddev` — the bounded "how many anomalies?" primitive.

## Signature

```python
bv.outlier_count(
    field: str,
    *,
    window: str,
    sigma: float = 3.0,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.outlier_count` returns the number of matching events seen so far
whose `field` value is more than `sigma` standard deviations away from
the running mean. On every matching event the helper compares
`|x − mean|` against `sigma · stddev`, increments the outlier counter
if it exceeds the threshold, and **then** folds the value into a
Welford accumulator `(n, mean, m2)`. The check uses pre-update statistics
so the event being tested does not bias its own threshold. The op
**warms up** for `MIN_BASELINE_N = 5` matching events before the outlier
test fires — events seen during warm-up always contribute to the
baseline but never count as outliers, which keeps the early-stream
counter from spuriously incrementing during the cold-start regime.

This is the canonical "how many anomalies has this entity produced?"
primitive — useful for any signal where you care about the **count** of
breaks rather than the magnitude of the latest one (failed-payment
amount outliers per card, abnormally large response sizes per IP, freak
sensor readings per device). Compared to [`bv.z_score`](./z_score.md)
which returns the **current** event's deviation, `outlier_count` is a
bounded scalar that grows with anomaly accumulation — easier to use as
a rule input ("flag if the user has produced 3+ amount outliers in the
last hour"). Pair it with [`bv.trend_residual`](./trend_residual.md)
when the underlying signal has a legitimate drift you don't want
counted as anomalies.

`bv.outlier_count` belongs to the **velocity** family. It is the only
**Tier 2** velocity op — the per-event update includes one `sqrt()` on
the variance to derive the threshold (no path eliminates this floor).
Cost is **Tier 2** (~22 ns floor / ~42 ns measured) and memory is
`O(1)` per entity (`n`, `mean`, `m2`, `outliers`). The `window=` kwarg
is **required** by the Python SDK helper; the inner `OutlierCountState`
is itself lifetime-bound `O(1)`.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Numeric field (`i64` or `f64`) to test for deviation. Non-numeric values are silently skipped. |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. See [shared.md window grammar](../../sdk-api/shared.md). |
| `sigma` | `float` | No | `3.0` | Threshold in **standard deviations** away from the running mean. Must be `> 0`. The classic three-sigma default catches values in roughly the 0.3% tail under a normal distribution. Lower (e.g. `2.0`) is more sensitive; higher (`4.0`+) is stricter. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the baseline and the outlier counter. |

## Returns

A single `i64` — the lifetime count of outlier events seen so far. Cold-start (no matching events) returns `0`, never `null`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 2** (~22 ns floor / ~42 ns measured — one `sqrt()`) — see [cost-class.md](../cost-class.md#tier-2-moderate-30-100-nscall--6-ops) |
| Memory per entity | `O(1)` — `OutlierCountState` ≈ 32 B (`n: u64`, `mean: f64`, `m2: f64`, `outliers: u64`) |
| Lifetime mode (`window="forever"`) | **Allowed** — classified `O1` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Three-sigma transaction-amount outliers per user

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserAmtOutliers(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(amt_outliers_24h=bv.outlier_count(
                     "amount",
                     window="24h",
                     sigma=3.0))
    )

# Push events: a steady stream around $100 with one $5,000 spike
for amt in [100.0, 95.0, 110.0, 102.0, 98.0, 5000.0]:
    app.push("Txn", {"user_id": "alice", "amount": amt})

# Query
result = app.get("UserAmtOutliers", "alice")
# result == {"amt_outliers_24h": 1} # the $5,000 event broke the 3-sigma threshold
```

### Example 2: Two-sigma response-time outliers per IP, hourly window

```python
@bv.table(key="ip")
def IpRespOutliers(reqs) -> bv.Table:
    return (
        reqs.group_by("ip")
            .agg(slow_responses=bv.outlier_count(
                     "response_ms",
                     window="1h",
                     sigma=2.0,
                     where=bv.col("status_code") < 400))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserAmtOutliers",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "amt_outliers_24h": {
      "op": "outlier_count",
      "params": {
        "field": "amount",
        "window": "24h",
        "sigma": 3.0
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `0` (integer), not `null`. Counter starts at `0`.
- **Warm-up phase (`n < 5`):** events count toward the baseline (`mean` / `m2`) but the outlier check **does not fire**. This avoids spurious early-stream increments where the running stddev is unstable. The first event that can possibly be flagged as an outlier is the **6th** matching event.
- **Constant signal (`stddev = 0`):** the outlier check is **skipped** (a zero-stddev baseline cannot meaningfully distinguish outliers from baseline). The counter does not increment regardless of the event value. Once the signal eventually varies, the next non-conforming event can be flagged.
- **`sigma <= 0`:** raises `ValueError` at SDK-helper-call time.
- **Default `sigma=3.0`:** the [classic three-sigma rule](https://en.wikipedia.org/wiki/68%E2%80%9395%E2%80%9399.7_rule) — catches roughly the 0.3% tail under a normal distribution. Tighten to `sigma=2.0` for ~5% tail; loosen to `sigma=4.0` for ~0.006% tail. Higher `sigma` reduces noise from non-normally-distributed signals but also misses milder anomalies.
- **Missing or non-numeric `field`:** the event is silently skipped (no update). Matches the [`bv.sum`](../core/sum.md) / [`bv.mean`](../core/mean.md) behavior.
- **`where=` filter excludes the event:** no update; non-matching events do not contribute to the baseline either.
- **Missing `window=`:** raises `ValueError` at SDK-helper-call time.
- **Malformed `window=`:** raises `ValueError` at SDK-helper-call time; if it somehow reaches the server, `register_validate.rs` returns structured error `aggregation_invalid_window`.
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the entire state including the outlier counter; the next post-eviction matching event reseeds at `n = 0` and re-enters the warm-up regime.

## See also

- [Velocity family index](./index.md) — overview of all 9 velocity-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 2; the `sqrt()` is the floor)
- [bv.z_score](./z_score.md) — current event's deviation in stddev units (single scalar; pick when you want magnitude rather than count)
- [bv.trend_residual](./trend_residual.md) — deviation against a trend line rather than an unweighted mean
- [bv.var](../core/var.md) / [bv.std](../core/std.md) — the underlying baseline statistics
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
