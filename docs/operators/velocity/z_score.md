# bv.z_score

> Current event's value standardised against the entity's running mean and stddev — `(x − mean) / stddev`. The "how unusual is this?" primitive.

## Signature

```python
bv.z_score(
    field: str,
    *,
    baseline_window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.z_score` returns the **current matching event's** value expressed in
units of standard deviation away from the entity's own running mean.
On every matching event the helper folds the value into a Welford
accumulator `(n, mean, m2)` and caches the latest value; the query
computes `stddev = sqrt(m2 / (n−1))` and returns `(last_value − mean) / stddev`.
A z-score near `0` means "this event is typical for this entity"; a
positive z-score means "above this entity's average"; a negative one
means "below"; `|z| > 3` is the classic three-sigma anomaly threshold.

This is the canonical "anomaly score against this entity's own history"
primitive — the magnitude analogue of [`bv.outlier_count`](./outlier_count.md)
which counts how many anomalies have occurred. Read it as "how unusual
is this transaction amount given everything I have seen from this user?",
"is this response time abnormally slow for this IP?", "is this sensor
reading unusual for this device?". Compared to [`bv.ew_zscore`](../decay/ew_zscore.md),
which uses an **exponentially-weighted** baseline that adapts to drift,
`bv.z_score` uses an **unweighted cumulative** mean and stddev — much
better at "is this the entity's all-time peak?" but slow to react to
legitimate behavioural shifts.

`bv.z_score` belongs to the **velocity** family (it lives here per
RESEARCH §3 directory layout — entity-level statistics that pair
naturally with the velocity / trend / outlier ops). Per-event update is
one numeric extract plus four scalar FP ops (Welford); the query path
includes one `sqrt()`. Cost is **Tier 1** (~18 ns floor / ~38 ns
measured) and memory is `O(1)` per entity. Per the SDK helper, the
required kwarg is named `baseline_window` (not `window`) to make the
"baseline-against-which-the-current-event-is-scored" intent explicit.
The wire-form `params` field is still `"window"`.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Numeric field (`i64` or `f64`) to track. Non-numeric values are silently skipped. |
| `baseline_window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. Defines the look-back baseline against which the current event is z-scored. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the baseline (and the cached `last_value`). |

## Returns

A single `f64` — the z-score of the most recent matching event in standard-deviation units. Cold-start, one-event start (`n < 2`), and degenerate baseline (`stddev = 0`) all return `null` (Python `None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~18 ns floor / ~38 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `ZScoreState` ≈ 40 B (`n: u64`, `mean: f64`, `m2: f64`, `last_value: f64`, `initialized: bool`) |
| Lifetime mode (`baseline_window="forever"`) | **Allowed** — classified `O1` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Per-user transaction-amount z-score (24h baseline)

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserAmtZScore(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(amt_z_24h=bv.z_score("amount", baseline_window="24h"))
    )

# Push events: a steady stream around $100 then a $5,000 spike
for amt in [100.0, 95.0, 110.0, 102.0, 98.0]:
    app.push("Txn", {"user_id": "alice", "amount": amt})
app.push("Txn", {"user_id": "alice", "amount": 5000.0})

# Query
result = app.get("UserAmtZScore", "alice")
# result == {"amt_z_24h": <large positive f64 — many sigmas above baseline>}
```

### Example 2: Filtered response-time z-score per IP (10m baseline)

```python
@bv.table(key="ip")
def IpRespZScore(reqs) -> bv.Table:
    return (
        reqs.group_by("ip")
            .agg(resp_z=bv.z_score(
                     "response_ms",
                     baseline_window="10m",
                     where=bv.col("status_code") < 400))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserAmtZScore",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "amt_z_24h": {
      "op": "z_score",
      "params": {
        "field": "amount",
        "window": "24h"
      }
    }
  }
}
```

Note that the wire `params.window` field is the SDK helper's
`baseline_window=` argument — the rename is purely an SDK ergonomics
choice. See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json)
for a full payload example.

## Edge cases

- **Empty stream / cold-start (`n = 0`):** result is `null`.
- **Single-event entity (`n = 1`):** result is `null` — at least two matching events are required for a non-zero stddev.
- **Constant signal (`stddev = 0`):** result is `null` — no spread to normalise against. As soon as the signal varies (`m2 > 0`), the next matching event has a defined z-score.
- **Latest event exactly at the mean:** z-score is `0.0`, not `null`.
- **Missing or non-numeric `field`:** the event is silently skipped (no update); the baseline and `last_value` are unchanged.
- **`where=` filter excludes the event:** no update; non-matching events do not contribute to the baseline.
- **Missing `baseline_window=`:** raises `ValueError` at SDK-helper-call time.
- **Malformed `baseline_window=`:** raises `ValueError` at SDK-helper-call time; if it somehow reaches the server, `register_validate.rs` returns structured error `aggregation_invalid_window`.
- **Numerical precision over very long lifetimes:** Welford's `m2` accumulator grows with `n`; for `baseline_window="forever"` on a busy entity the absolute value can grow large enough to lose FP precision. Prefer a fixed `baseline_window=` on long-lived high-volume entities, or use [`bv.ew_zscore`](../decay/ew_zscore.md) which has a bounded magnitude by design.
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the underlying state per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md); the next post-eviction matching event reseeds the Welford accumulator.

## See also

- [Velocity family index](./index.md) — overview of all 9 velocity-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.outlier_count](./outlier_count.md) — count of anomalies; the "how many?" sibling to this op's "how unusual?"
- [bv.ew_zscore](../decay/ew_zscore.md) — same primitive against an exponentially-weighted baseline (adapts to drift; bounded magnitude)
- [bv.trend_residual](./trend_residual.md) — deviation against a trend line rather than an unweighted mean (pick when there is legitimate drift)
- [bv.var](../core/var.md) / [bv.std](../core/std.md) — the underlying baseline statistics
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
