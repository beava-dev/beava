# bv.negative_streak

> Length of the entity's current consecutive **non**-matching streak. Symmetric to [`bv.streak`](./streak.md).

## Signature

```python
bv.negative_streak(
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.negative_streak` is the mirror of [`bv.streak`](./streak.md): it counts
consecutive events that **fail** the `where=` predicate, ending at (and
including) the most recent event. Each non-matching event increments the
counter; each matching event resets it to 0. Read it as "how many events
in a row did NOT match the success criterion?", "how many consecutive
non-payment events?", or "how long has this user been silent on the
high-value SKU shelf?".

The semantics are intentionally just `streak` with the predicate inverted
at the apply path: on a `where_matched = false` event, `current += 1`;
on a `where_matched = true` event, `current = 0`. The state is a single
`u64` (`NegativeStreakState` does not track a `max_seen` — there is no
`bv.max_negative_streak` op in v0). Cold-start `current` is `0`. If you
need both the positive and the negative streak symmetrically, register
`bv.streak(where=p)` alongside `bv.negative_streak(where=p)` — they will
each maintain independent state.

`bv.negative_streak` belongs to the **recency** family. Per-event update
is one `u64` write; memory per entity is `O(1)`. There is no `window=`
kwarg — `bv.negative_streak` is **lifetime-only**.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields. Non-matching events extend the streak; matching events reset it to 0. Without `where=`, every event is a match (so `negative_streak` stays at 0 — generally not useful without a predicate). |

## Returns

A single `i64` value: the current consecutive-non-matching count. Always
returns an integer; cold-start (no events seen) returns `0`, never `null`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~10 ns floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single `u64` slot in `NegativeStreakState` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.negative_streak` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: Consecutive non-success-payment count per user

```python
import beava as bv

@bv.event
class Payment:
    user_id: str
    status: str

@bv.table(key="user_id")
def UserConsecutiveFailures(payments) -> bv.Table:
    return (
        payments.group_by("user_id")
                .agg(non_success_streak=bv.negative_streak(
                                            where=bv.col("status") == "ok"))
    )

# Push events
for status in ["ok", "failed", "failed", "declined", "ok", "failed"]:
    app.push("Payment", {"user_id": "alice", "status": status})

# Query — the trailing run of non-"ok" is just 1 ("failed")
# But before that single trailing failure was an "ok" (reset), so streak=1
result = app.get("UserConsecutiveFailures", "alice")
# result == {"non_success_streak": 1}
```

### Example 2: Silent-period detector — count consecutive non-purchase events

```python
@bv.table(key="user_id")
def UserSilentPeriod(events) -> bv.Table:
    return (
        events.group_by("user_id")
              .agg(non_purchase_run=bv.negative_streak(
                                        where=bv.col("event_type") == "purchase"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserConsecutiveFailures",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "non_success_streak": {
      "op": "negative_streak",
      "params": {
        "where": "status == 'ok'"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `0` (integer), not `null`.
- **`where=` filter excludes everything:** every event is a non-match, so `current` grows with the entity's total event count (equivalent to a cumulative non-match `bv.count`).
- **Without `where=`:** every event is a match, so `current` stays at `0` forever — `bv.negative_streak()` without a predicate is a constant-zero feature and rarely useful.
- **No "max_negative_streak" op in v0:** unlike [`bv.streak`](./streak.md) / [`bv.max_streak`](./max_streak.md), `negative_streak` has no all-time-max sibling. If you need it, compose two ops manually or open an issue for v0.1+.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); the streak follows server arrival order.
- **Cold-entity eviction:** if [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md) evicts the entity, `NegativeStreakState` is dropped; the next event after eviction restarts the count.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. A windowed negative-streak would require a different state shape and is out of v0 scope.
- **Lifetime mode:** **the only mode.** Footprint is `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.streak](./streak.md) — symmetric companion: consecutive-match counter
- [bv.max_streak](./max_streak.md) — all-time-max companion to `streak` (no analog for `negative_streak` in v0)
- [bv.has_seen](./has_seen.md) — cumulative boolean for "ever matched"; pair with `negative_streak` to detect cold-start vs sustained silence
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
