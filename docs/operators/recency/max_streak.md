# bv.max_streak

> All-time longest consecutive matching streak ever observed for this entity. Sticky once set; only rises.

## Signature

```python
bv.max_streak(
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.max_streak` returns the longest consecutive run of `where=`-matching
events the entity has ever produced. Whenever the live streak (see
[`bv.streak`](./streak.md)) grows past the previous high-water mark, that
new value is captured in `max_seen`. A non-matching event resets the live
streak to 0 but **does not** reset `max_seen`. Read it as "what's the
worst run of failed logins this user has ever shown?", "longest streak
of declined payments this card has ever produced?", or "what's the longest
in-region run for this device?".

`max_streak` shares its underlying `StreakState` struct with `streak`,
so registering both on the same `where=` predicate costs roughly the same
as registering one (the two siblings read different fields off the same
struct: `streak` returns `current`, `max_streak` returns `max_seen`).
Storage is two `u64` slots per entity. Per-event update on a match:
`current += 1; max_seen = max(max_seen, current)`. On a non-match:
`current = 0` (max_seen unchanged).

`bv.max_streak` belongs to the **recency** family. Per-event update is two
`u64` writes plus a comparison; memory per entity is `O(1)` regardless of
stream length. There is no `window=` kwarg — `bv.max_streak` is
**lifetime-only** by definition (it tracks an all-time maximum).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields. Matching events extend the live streak (and possibly the max); non-matching events reset only the live streak. Without `where=`, every event matches. |

## Returns

A single `i64` value: the longest consecutive-matching run ever observed.
Cold-start (no events seen) returns `0`, never `null`. Once set, the value
never decreases.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~12 ns floor / ~32 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — same two-`u64` `StreakState` shared with [`bv.streak`](./streak.md) per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.max_streak` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: Worst-ever failed-login run per user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    status: str

@bv.table(key="user_id")
def UserWorstFailRun(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(worst_fail_run=bv.max_streak(where=bv.col("status") == "failed"))
    )

# Push events in arrival order
for status in ["failed", "failed", "failed", "ok", "failed"]:
    app.push("Login", {"user_id": "alice", "status": status})

# Query — worst run was 3 in a row (events 1-3); the trailing single fail is 1
result = app.get("UserWorstFailRun", "alice")
# result == {"worst_fail_run": 3}
```

### Example 2: Pair `streak` + `max_streak` to surface both live and historical pressure

```python
@bv.table(key="card_id")
def CardDeclinePressure(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(
                live_decline_streak=bv.streak(where=bv.col("status") == "declined"),
                worst_decline_streak=bv.max_streak(where=bv.col("status") == "declined"),
            )
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserWorstFailRun",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "worst_fail_run": {
      "op": "max_streak",
      "params": {
        "where": "status == 'failed'"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `0` (integer), not `null`.
- **`where=` filter excludes everything:** `current` stays at `0`, so `max_seen` also stays at `0` forever.
- **Without `where=`:** every event matches, so the live streak grows monotonically and `max_streak` equals the entity's total event count.
- **Once set, never decreases:** `max_seen` is sticky for the entity's lifetime. The only way for `max_streak` to drop is full entity eviction via [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md), which discards the entire `StreakState`.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); streaks follow server arrival order.
- **Cold-entity eviction:** post-eviction, `max_streak` resets to `0` and rebuilds from the next match.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. A windowed maximum-streak would require a different state shape and is out of v0 scope.
- **Lifetime mode:** **the only mode.** Footprint is `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.streak](./streak.md) — live consecutive-match counter (shares the same `StreakState`)
- [bv.negative_streak](./negative_streak.md) — symmetric companion: live consecutive-non-match counter
- [bv.count](../core/count.md) — cumulative match count (no consecutiveness)
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
