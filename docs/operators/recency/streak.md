# bv.streak

> Length of the entity's current consecutive matching streak. Resets to 0 on any non-match.

## Signature

```python
bv.streak(
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.streak` returns the number of consecutive events that have matched
`where=` ending at (and including) the most recent event. Each matching
event increments the counter; each non-matching event resets it to 0.
Read it as "how many failed-login attempts in a row?", "how many
consecutive successful payments?", or "how many declined transactions
before the most recent acceptance?".

Streak is event-order driven: it has no time dimension. The 6th
consecutive match is the 6th in arrival order, regardless of whether
those matches landed in 6 milliseconds or 6 days. The state is just two
`u64`s — `current` (the live streak) and `max_seen` (the all-time max,
read by [`bv.max_streak`](./max_streak.md) which shares the same
`StreakState` struct). On a match: `current += 1; max_seen = max(max_seen, current)`.
On a non-match: `current = 0`. Cold-start `current` is `0`.

`bv.streak` belongs to the **recency** family. Per-event update is two
`u64` writes; memory per entity is `O(1)` regardless of stream length.
There is no `window=` kwarg — `bv.streak` is **lifetime-only**. (Note:
because `current` resets on the first non-match, "lifetime" here just
means the state survives forever; the streak itself is short-lived
unless the entity is on a hot run.)

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields. Matching events extend the streak; non-matching events reset it to 0. Without `where=`, every event is a match (streak = total event count). |

## Returns

A single `i64` value: the current consecutive-matching count. Always returns
an integer; cold-start (no events seen) returns `0`, never `null`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~10 ns floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — two `u64` slots in `StreakState` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.streak` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: Consecutive failed-login count per user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    status: str

@bv.table(key="user_id")
def UserConsecutiveFails(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(fail_streak=bv.streak(where=bv.col("status") == "failed"))
    )

# Push events in arrival order
app.push("Login", {"user_id": "alice", "status": "failed"}) # streak = 1
app.push("Login", {"user_id": "alice", "status": "failed"}) # streak = 2
app.push("Login", {"user_id": "alice", "status": "failed"}) # streak = 3
app.push("Login", {"user_id": "alice", "status": "ok"}) # streak = 0 (reset)
app.push("Login", {"user_id": "alice", "status": "failed"}) # streak = 1

# Query
result = app.get("UserConsecutiveFails", "alice")
# result == {"fail_streak": 1}
```

### Example 2: Consecutive in-region transactions per card

```python
@bv.table(key="card_id")
def CardLocalRun(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(local_streak=bv.streak(where=bv.col("country") == "US"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserConsecutiveFails",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "fail_streak": {
      "op": "streak",
      "params": {
        "where": "status == 'failed'"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `0` (integer), not `null`. `bv.streak` always returns an integer.
- **`where=` filter excludes everything:** every event is a non-match, so `current` stays at `0` forever.
- **Without `where=`:** every event matches, so `current` equals the total number of events the entity has ever seen — equivalent to a [`bv.count()`](../core/count.md) without the `where=` filter.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); the streak follows server arrival order strictly.
- **Cold-entity eviction:** if [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md) evicts the entity, `StreakState` is dropped; the next event after eviction starts a fresh streak (current = 1 if it matches, 0 otherwise).
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. There is no windowed `streak` — windowed streaks would require a different state shape (a deque of match/no-match flags) and are out of v0 scope.
- **Lifetime mode:** **the only mode.** Footprint is `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.max_streak](./max_streak.md) — all-time longest streak (shares `StreakState`)
- [bv.negative_streak](./negative_streak.md) — symmetric companion: count of consecutive **non**-matches
- [bv.count](../core/count.md) — total matches (not consecutive); use `bv.count(where=...)` for "how many fails ever"
- [bv.has_seen](./has_seen.md) — boolean cumulative variant: "ever matched?", no streak count
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
