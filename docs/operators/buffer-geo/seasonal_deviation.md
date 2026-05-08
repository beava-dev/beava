# bv.seasonal_deviation

> Z-score of the most recent event's value against this entity's hour-of-day baseline.

## Signature

```python
bv.seasonal_deviation(
    field: str,
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.seasonal_deviation` returns the z-score of the most recent observation
of `field` against this entity's running per-hour baseline. The state
maintains 24 `(count, sum, sum_sq)` `HourBucket` triples — one per UTC
hour — and updates the bucket at index `hour_of_day(now_ms)` on every
matching event. The query computes
`(latest_value − bucket.mean) / bucket.stddev` for the bucket of the most
recent event. Use it for "is this transaction anomalously large for the
hour at which it landed?" — features that detect unusual-hour activity
where the same value would be normal in the entity's typical hour profile
but stands out in the current hour.

The 24 hourly buckets are a structural cap, so `bv.seasonal_deviation`
qualifies as `O(1)` per entity under the
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) lifetime-aggregation
contract — no required register-time kwarg, no fallback default. State per
entity is `[HourBucket; 24] + Option<(f64, usize)>` for the latest
observation: 24 × 24 bytes + 16 bytes ≈ 600 bytes. v0 boxed
`SeasonalDeviationState` so the `AggOp::SeasonalDeviation` variant fits the
80-byte enum cap (the state itself lives on the heap behind a `Box`); see
`crates/beava-core/src/agg_op.rs` line 482 and
[SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md).

`bv.seasonal_deviation` belongs to the **bounded-buffer** family — it
shares state-shape with [`bv.hour_of_day_histogram`](./hour_of_day_histogram.md)
but tracks variance, not just counts. Per-event update is Tier 1 (~10 ns
floor / ~30 ns measured per [cost-class.md](../cost-class.md)) — three FP
ops on the bucket plus the latest-observation memo. The query path
computes one mean, one variance via `E[X²] − E[X]²` with Bessel correction,
one sqrt, and one division. There is no `window=` kwarg in v0 —
seasonal_deviation is **lifetime-only**. For a "seasonal z-score over the
last 30 days" view, compose with `@bv.event(cold_after="30d")` per
[V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Numeric field whose hour-of-day baseline is tracked. `f64` or `i64`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the bucket and record the latest value. |

State invariants (informational; not user-tunable):

- 24 hourly buckets — one per UTC hour `0..23` (UTC, no `timezone=` kwarg in v0).
- Per-bucket Welford-incompatible textbook variance via running `(n, sum, sum_sq)` (single-pass, adequate for v0; subject to catastrophic cancellation only at extreme magnitudes).

## Returns

A `f64` z-score for the most recent matching event, or `null` (Python
`None`) when the entity has not yet accumulated enough data to compute one.
Specifically, the result is `null` when:

- No matching event has ever been observed (cold-start).
- The bucket for the most recent event has fewer than 2 observations
  (variance undefined).
- The bucket variance is exactly `0.0` (degenerate — every prior
  observation in this hour was identical).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~10 ns floor / ~30 ns measured — `n += 1; sum += v; sum_sq += v*v` on the indexed bucket) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | **`O(1)`** — `[HourBucket; 24] + Option<(f64, usize)>` ≈ 600 bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). Boxed inside `AggOp` per v0 to fit the 80-byte enum cap |
| Lifetime mode | **Required** — `bv.seasonal_deviation` has no `window=` kwarg in v0; lifetime is the only mode |

## Examples

### Example 1: Anomalous-amount detection per user

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserAmountSeasonality(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(amount_z_for_hour=bv.seasonal_deviation("amount"))
    )

# After many transactions building a per-hour baseline,
# a sudden $5000 charge at 03:00 UTC for a user whose 03:00 history is small
# returns a positive z-score:
result = app.get("UserAmountSeasonality", "alice")
# result == {"amount_z_for_hour": 4.2}
```

### Example 2: Successful-only request-size seasonality per endpoint

```python
@bv.table(key="endpoint")
def EndpointSizeSeasonality(reqs) -> bv.Table:
    return (
        reqs.group_by("endpoint")
            .agg(size_z=bv.seasonal_deviation("size_bytes",
                                                where=bv.col("status") == 200))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserAmountSeasonality",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "amount_z_for_hour": {
      "op": "seasonal_deviation",
      "params": {
        "field": "amount"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null` — there is no latest observation to score.
- **Single observation in the current bucket:** result is `null` — variance requires `n ≥ 2`.
- **Degenerate bucket (variance = 0):** result is `null` — every prior observation in this hour was identical, so no spread to z-score against.
- **Non-numeric source field (`Value::Str`, `Value::Bool`):** event silently dropped (the bucket is not updated, the latest-observation memo is not written).
- **NaN inputs:** dropped — `numeric_from_row` returns `None` on non-`F64`/`I64` payloads.
- **Pre-1970 events (`now_ms` < 0):** the hour index uses `rem_euclid`, so negative `now_ms` still maps to a valid hour (no panic, no wraparound).
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. For a "seasonal z-score over the last N days" use `@bv.event(cold_after="...")` to bound the lifetime via per-entity TTL.
- **Catastrophic cancellation at extreme magnitudes:** the textbook `E[X²] − E[X]²` formula can lose precision when `sum²/n` and `sum_sq` are nearly equal. v0 accepts this trade-off (single-pass + atomic-friendly) over a Welford rewrite. v0.1+ may switch to Welford if precision becomes a real-workload issue.
- **UTC-only:** the bucket index is computed against UTC. There is no `timezone=` kwarg in v0; if you need local-hour bucketing, derive a `local_hour` column and use [`bv.zscore`](../velocity/z_score.md) on the per-hour split via `where=bv.col("local_hour") == X`.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); the bucket is keyed on server arrival time `now_ms`.
- **Lifetime mode:** **the only mode.** Per-entity memory is fixed at ~600 bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.hour_of_day_histogram](./hour_of_day_histogram.md) — count-only sibling (same hour-of-day index, no value tracking)
- [bv.dow_hour_histogram](./dow_hour_histogram.md) — 168-bin weekly cousin (counts only)
- [bv.z_score](../velocity/z_score.md) — entity-level z-score against a single rolling baseline (no per-hour split)
- [bv.ew_zscore](../decay/ew_zscore.md) — drift-aware z-score with exponential decay (no per-hour split)
- [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md) — cold-entity eviction (`@bv.event(cold_after=...)`) for windowing the lifetime
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — `O(1)` lifetime-aggregation contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
