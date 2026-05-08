# bv.hour_of_day_histogram

> 24-bin count histogram per entity, keyed on the UTC hour-of-day of arrival.

## Signature

```python
bv.hour_of_day_histogram(
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.hour_of_day_histogram` returns a 24-bin count histogram per entity, with
one bin for each hour `00..23` of the UTC day. Each event arriving at
processing-time `now_ms` increments the bin at index
`(now_ms / 3_600_000) mod 24`. Use it for "this user's activity heatmap by
hour of day" — features that surface diurnal patterns ("most logins occur
between 02:00 and 04:00 UTC for this account") that velocity / count ops
cannot express on their own.

The 24 bins are a structural cap — the operator carries no user-supplied
size kwarg. Memory per entity is `O(24 × 8) = 192` bytes regardless of
stream length, so `bv.hour_of_day_histogram` qualifies as `O(1)` under the
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) lifetime-aggregation
contract — no required register-time kwarg, no fallback default. The state
is `[u64; 24]`; per-event update is a single saturating array write.

`bv.hour_of_day_histogram` belongs to the **bounded-buffer** family. It is
the fastest v0 buffer op — Tier 1 floor (~4 ns / ~25 ns measured per
[cost-class.md](../cost-class.md)) — because the bin index is a direct
modular arithmetic, no field extraction, no string allocation. There is no
`window=` kwarg in v0 — the histogram is **lifetime-only**. For a
"hour-of-day mix in the last 7 days" view, compose with
`@bv.event(cold_after="7d")` so the per-entity state is reset after a week
of silence per the cold-entity eviction policy in
[V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md). The operator is the
underlying state for [`bv.seasonal_deviation`](./seasonal_deviation.md), the
z-score-against-this-baseline companion.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events increment a bin. |

No `field=` kwarg — the operator buckets on event arrival time, not on a
payload value. v0 boxed `HourOfDayHistogramState` so the
`AggOp::HourOfDayHistogram` variant fits within the 80-byte enum cap (the
state itself is 24 × 8 = 192 bytes, allocated on the heap behind a `Box`).
See `crates/beava-core/src/agg_op.rs` line 480 and
[SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md).

## Returns

A `dict[str, int]` keyed by zero-padded UTC hour string (`"00"`, `"01"`,
…, `"23"`) with `i64` count values. Wire form is `Value::Map` with
`BTreeMap`-sorted iteration, so dict iteration order is hour-ascending.
Cold-start (no events) returns the dict with all 24 keys at `0`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~4 ns floor / ~25 ns measured — direct `[u64; 24]` array write) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | **`O(1)`** — fixed `[u64; 24]` = 192 bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). Boxed inside `AggOp` per v0 to fit the 80-byte enum cap |
| Lifetime mode | **Required** — `bv.hour_of_day_histogram` has no `window=` kwarg in v0; lifetime is the only mode |

## Examples

### Example 1: Per-user UTC hour-of-day heatmap

```python
import beava as bv

@bv.event
class Login:
    user_id: str

@bv.table(key="user_id")
def UserHourHeatmap(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(hourly_logins=bv.hour_of_day_histogram())
    )

# After 1000 events arriving across the day for "alice"
result = app.get("UserHourHeatmap", "alice")
# result == {"hourly_logins": {"00": 12, "01": 8, ..., "23": 41}}
# (24 keys — one per UTC hour)
```

### Example 2: Successful-only diurnal pattern

```python
@bv.table(key="account_id")
def AccountSuccessHourly(reqs) -> bv.Table:
    return (
        reqs.group_by("account_id")
            .agg(success_by_hour=bv.hour_of_day_histogram(
                where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserHourHeatmap",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "hourly_logins": {
      "op": "hour_of_day_histogram",
      "params": {}
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** all 24 bins are `0`; the result dict is `{"00": 0, "01": 0, …, "23": 0}` — never `null`.
- **Hour boundary:** events whose `now_ms` is exactly on a multiple of `3_600_000` ms increment the bin for the hour they enter (the `mod 24` lower-edge bin).
- **Pre-1970 events (`now_ms` < 0):** the index uses `rem_euclid`, so negative `now_ms` still maps to a valid hour (no panic, no wraparound).
- **`field=` kwarg attempted:** raises `TypeError` at SDK-helper-call time — the operator is field-less by design.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. For a windowed diurnal pattern use `@bv.event(cold_after="...")` to bound the lifetime via per-entity TTL.
- **Counter overflow:** each `u64` bin saturates at `2^64 − 1` (impossible in practice for a single entity).
- **UTC-only:** the bin index is computed against UTC. There is no `timezone=` kwarg in v0; if you need local-time buckets, derive a `local_hour` column with `with_columns` and use [`bv.histogram`](./histogram.md) on it instead.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); the bin is keyed on server arrival time `now_ms`, not on a payload timestamp.
- **Lifetime mode:** **the only mode.** Per-entity memory is fixed at 192 bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1 — fastest v0 buffer op)
- [bv.dow_hour_histogram](./dow_hour_histogram.md) — 168-bin day-of-week × hour companion (weekly granularity)
- [bv.seasonal_deviation](./seasonal_deviation.md) — z-score against this hour-of-day baseline (consumes `HourBucket` state)
- [bv.histogram](./histogram.md) — value-bucket companion (configurable bucket edges on a numeric field)
- [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md) — cold-entity eviction (`@bv.event(cold_after=...)`) for windowing the lifetime
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — `O(1)` lifetime-aggregation contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
