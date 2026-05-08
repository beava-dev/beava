# bv.dow_hour_histogram

> 168-bin (day-of-week × hour) count histogram per entity. Mon-00 through Sun-23.

## Signature

```python
bv.dow_hour_histogram(
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.dow_hour_histogram` returns a 168-bin count histogram per entity, with
one bin for each `(day_of_week, hour)` cell of the UTC week. Each event
arriving at processing-time `now_ms` increments the bin at index
`day_of_week * 24 + hour_of_day`, where Mon=0 and Sun=6 (Unix epoch was a
Thursday, so the index uses a +3 offset). Use it for "this user's weekly
activity heatmap" — features that surface weekend-vs-weekday or
specific-day-and-hour patterns ("most failed-login attempts on this account
land Friday-19:00 to Sat-02:00 UTC") that the [24-bin
`hour_of_day_histogram`](./hour_of_day_histogram.md) can't separate.

The 168 bins are a structural cap — the operator carries no user-supplied
size kwarg. Memory per entity is `O(168 × 8) = 1,344` bytes regardless of
stream length, so `bv.dow_hour_histogram` qualifies as `O(1)` under the
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) lifetime-aggregation
contract — no required register-time kwarg, no fallback default. The state
is `Vec<u64>` of length 168; per-event update is a single saturating
indexed write.

`bv.dow_hour_histogram` belongs to the **bounded-buffer** family. Per-event
update is Tier 1 (~4 ns / ~25 ns measured per
[cost-class.md](../cost-class.md)) — pure arithmetic on `now_ms`, no field
extraction, no string allocation. There is no `window=` kwarg in v0 —
the histogram is **lifetime-only**. For a "weekly mix in the last 30 days"
view, compose with `@bv.event(cold_after="30d")` so the per-entity state is
reset after a month of silence per the cold-entity eviction policy in
[V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events increment a bin. |

No `field=` kwarg — the operator buckets on event arrival time, not on a
payload value. State size is 168 × 8 = 1,344 bytes; the variant is unboxed
in `AggOp` because `DowHourHistogramState` is `Vec<u64>` whose stack
footprint is just the `Vec` header (24 bytes) — see
`crates/beava-core/src/agg_op.rs` line 481.

## Returns

A `dict[str, int]` keyed by `"<Day>-<HH>"` strings, e.g. `"Mon-00"`,
`"Mon-01"`, …, `"Sun-23"` (168 keys total) with `i64` count values. Wire
form is `Value::Map` with `BTreeMap`-sorted iteration, so dict iteration
order is alphabetical-by-day then ascending-by-hour. Cold-start (no events)
returns the dict with all 168 keys at `0`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~4 ns floor / ~25 ns measured — direct `Vec[168]` index write) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | **`O(1)`** — fixed `Vec<u64>` of length 168 = 1,344 bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.dow_hour_histogram` has no `window=` kwarg in v0; lifetime is the only mode |

## Examples

### Example 1: Per-user weekly activity heatmap

```python
import beava as bv

@bv.event
class Login:
    user_id: str

@bv.table(key="user_id")
def UserWeeklyHeatmap(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(weekly_logins=bv.dow_hour_histogram())
    )

# After many events spread across a few weeks
result = app.get("UserWeeklyHeatmap", "alice")
# result == {"weekly_logins": {"Mon-00": 3, "Mon-01": 0, ..., "Sun-23": 7}}
# (168 keys — Mon-00 through Sun-23)
```

### Example 2: Successful payments — Mon 9am vs Fri 7pm pattern

```python
@bv.table(key="merchant_id")
def MerchantWeeklyOk(payments) -> bv.Table:
    return (
        payments.group_by("merchant_id")
                .agg(success_weekly=bv.dow_hour_histogram(
                    where=bv.col("status") == "captured"))
    )

# Lookups can compare cells:
result = app.get("MerchantWeeklyOk", "m_42")
mon_9 = result["success_weekly"]["Mon-09"]
fri_19 = result["success_weekly"]["Fri-19"]
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserWeeklyHeatmap",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "weekly_logins": {
      "op": "dow_hour_histogram",
      "params": {}
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** all 168 bins are `0`; the result dict is `{"Mon-00": 0, …, "Sun-23": 0}` — never `null`.
- **Day-of-week boundary:** events whose `now_ms` straddles a midnight increment the bin for the day they enter — the index is computed at apply time per event.
- **Pre-1970 events (`now_ms` < 0):** the index uses `rem_euclid` for both the day and hour components, so negative `now_ms` still maps to a valid `(dow, hour)` pair (no panic, no wraparound).
- **`field=` kwarg attempted:** raises `TypeError` at SDK-helper-call time — the operator is field-less by design.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. For a windowed weekly pattern use `@bv.event(cold_after="...")` to bound the lifetime via per-entity TTL.
- **Counter overflow:** each `u64` bin saturates at `2^64 − 1` (impossible in practice for a single entity).
- **UTC-only:** the day and hour are computed against UTC. There is no `timezone=` kwarg in v0; if you need local-time cells, derive `local_dow` / `local_hour` columns with `with_columns` and use [`bv.histogram`](./histogram.md) on them instead.
- **Day labels:** `Mon`/`Tue`/`Wed`/`Thu`/`Fri`/`Sat`/`Sun` (English, fixed). The label format is stable across versions — Python SDKs can dict-key on the labels.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); the bin is keyed on server arrival time `now_ms`, not on a payload timestamp.
- **Lifetime mode:** **the only mode.** Per-entity memory is fixed at 1,344 bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.hour_of_day_histogram](./hour_of_day_histogram.md) — 24-bin diurnal companion (no day-of-week axis)
- [bv.seasonal_deviation](./seasonal_deviation.md) — z-score against the hour-of-day baseline (uses 24-bin state, not 168-bin)
- [bv.histogram](./histogram.md) — value-bucket companion (configurable numeric edges)
- [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md) — cold-entity eviction (`@bv.event(cold_after=...)`) for windowing the lifetime
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — `O(1)` lifetime-aggregation contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
