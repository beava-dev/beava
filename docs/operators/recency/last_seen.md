# bv.last_seen

> Server arrival timestamp of the most recent matching event. Server processing-time per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md) — **not** event-time.

## Signature

```python
bv.last_seen(
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.last_seen` returns the server-side arrival timestamp (`now_ms()` at the
apply path) of the most recent event that matched `where=` for this entity.
Each accepted event overwrites the previous timestamp. Read it as "when did
we last see this user log in?", "when was the last successful payment from
this card?", or "when did this device most recently authenticate?".

The timestamp is **server processing-time**, captured by reading the
hand-rolled apply loop's `now_ms()` clock when the event is applied. beava
does **not** consult any event-time field on the payload, per
[`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md)
(locked 2026-04-30). If two events arrive in (server-clock) order A → B,
then `last_seen` returns B's arrival even if A's payload claims to be
"later" by some `event_ts` field. This is intentional: the Redis-shaped
semantics eliminate event-time / watermark / late-arrival concerns from
the user's mental model.

`bv.last_seen` belongs to the **recency** family. Per-event update is one
`Option<i64>` write to `last_ms` (sharing the `SeenState` struct with
`first_seen`, `age`, `has_seen`, `time_since`). Memory per entity is `O(1)`
regardless of stream length. There is no `window=` kwarg — `bv.last_seen`
is **lifetime-only**. For "ms since last match" use [`bv.time_since`](./time_since.md);
for "is the most recent match within N ms?" use [`bv.first_seen_in_window`](./first_seen_in_window.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update `last_seen`. Without `where=`, every event refreshes the timestamp. |

## Returns

A single `Datetime` value (`int64` milliseconds since the Unix epoch). When
the entity has seen zero matching events, the result is `null` (Python
`None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single `Option<i64>` slot in the shared `SeenState` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.last_seen` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: Most recent activity timestamp per user

```python
import beava as bv

@bv.event
class Activity:
    user_id: str
    action: str

@bv.table(key="user_id")
def UserLastActive(activity) -> bv.Table:
    return (
        activity.group_by("user_id")
                .agg(last_active_ms=bv.last_seen())
    )

# Push events
app.push("Activity", {"user_id": "alice", "action": "view"}) # t=1700000000000
app.push("Activity", {"user_id": "alice", "action": "click"}) # t=1700000007500

# Query
result = app.get("UserLastActive", "alice")
# result == {"last_active_ms": 1700000007500}
```

### Example 2: Most recent failed-login timestamp (lockout heuristic)

```python
@bv.table(key="user_id")
def UserLastFailMs(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(last_fail_ms=bv.last_seen(where=bv.col("status") == "failed"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserLastActive",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "last_active_ms": {
      "op": "last_seen",
      "params": {}
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null` (Python `None`).
- **`where=` filter excludes everything:** result is `null` until a matching event arrives.
- **Server-time, NOT event-time:** the captured value is the server's `now_ms()` at apply, not any payload field. Per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md), beava intentionally has no event-time concept. Out-of-order arrivals are recorded in arrival order, not event-payload order.
- **Cold-entity eviction:** if [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md) evicts the entity, the next event after eviction starts a fresh `SeenState`; `last_seen` reflects only post-eviction arrivals.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. For windowed semantics use [`bv.first_seen_in_window`](./first_seen_in_window.md).
- **Lifetime mode:** **the only mode.** Footprint is `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.first_seen](./first_seen.md) — symmetric companion: first arrival timestamp
- [bv.time_since](./time_since.md) — milliseconds since `last_seen`, computed at read time
- [bv.has_seen](./has_seen.md) — boolean variant: ever-matched, no timestamp
- [bv.first_seen_in_window](./first_seen_in_window.md) — "is the most recent match within N ms?"
- [bv.last](../point-ordinal/last.md) — most recent **value of a field**, instead of arrival timestamp
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
