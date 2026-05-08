# bv.time_since

> Milliseconds since `last_seen`, computed at read time. Server processing-time per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md) — **not** event-time.

## Signature

```python
bv.time_since(
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.time_since` returns the elapsed milliseconds between the entity's
[`last_seen`](./last_seen.md) timestamp and the **query time** (`now_ms()`
at the moment `app.get(...)` resolves). Read it as "how long since this
user last logged in?", "how many ms since the last successful payment from
this card?", or "how stale is this device?".

Like [`bv.age`](./age.md), `time_since` changes between reads **without
any new events** — because the right-hand side of `now_ms() - last_ms` is
captured at query time, not at apply time. The apply-side state is just
the most recent `last_ms` (overwritten on every match); the subtraction
happens server-side when the read fans out. This makes `time_since` a
useful staleness/recency feature: it grows with wall-clock seconds even on
quiescent entities, and resets to a small number whenever a new matching
event arrives.

Both timestamps are **server processing-time**: `last_ms` is server `now_ms()`
at the most recent arrival; the read-side `now_ms()` is the server clock
when `app.get(...)` reaches the entity. beava intentionally does not consult
any event-time field — see [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md)
(locked 2026-04-30).

`bv.time_since` belongs to the **recency** family. Per-event update is one
`Option<i64>` write (the same `SeenState` shared with `first_seen`,
`last_seen`, `age`, `has_seen`). Memory per entity is `O(1)`. There is no
`window=` kwarg — `bv.time_since` is **lifetime-only** by definition.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events advance `last_seen`. Without `where=`, every event refreshes the timestamp (and thus zeroes `time_since`). |

## Returns

A single `i64` value: the number of milliseconds between `last_seen` and
the query-time `now_ms()`. When the entity has seen zero matching events,
the result is `null` (Python `None`) — there is no `last_seen` to subtract
from. The value is clamped to a non-negative range — clock skew that would
produce a negative `time_since` returns 0 instead.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single `Option<i64>` slot in the shared `SeenState` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.time_since` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: Time since the last successful login per user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    status: str

@bv.table(key="user_id")
def UserSinceLastSuccess(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(since_ok_ms=bv.time_since(where=bv.col("status") == "ok"))
    )

# Push the most recent successful login at t=1700000000000
app.push("Login", {"user_id": "alice", "status": "ok"})

# Query at server time t=1700000300000 (5 minutes later)
result = app.get("UserSinceLastSuccess", "alice")
# result == {"since_ok_ms": 300000}

# Query later at t=1700003600000 — same state, larger time_since
# result == {"since_ok_ms": 3600000}
```

### Example 2: Staleness check on session activity

```python
@bv.table(key="session_id")
def SessionStaleness(events) -> bv.Table:
    return (
        events.group_by("session_id")
              .agg(stale_ms=bv.time_since())
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserSinceLastSuccess",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "since_ok_ms": {
      "op": "time_since",
      "params": {
        "where": "status == 'ok'"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null` (Python `None`). `time_since` requires at least one matching event before it has a `last_seen` to measure from.
- **`where=` filter excludes everything:** result is `null` until a matching event arrives.
- **Reads grow without new events:** the subtraction `now_ms() - last_ms` happens at query time, so the returned value increases between reads even if no events arrived. This is intentional — it is what makes `time_since` a useful staleness feature.
- **Reads shrink on a new match:** every accepted event advances `last_ms` to the current server `now_ms()`, so the next read returns a small value (close to "ms since that event arrived").
- **Clock-skew safety:** `time_since` is clamped to `>= 0`. If `query_time_ms < last_ms` (only possible under clock-skew or replay), the result is `0`.
- **Server-time, NOT event-time:** both endpoints are server-side per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md). Producers cannot influence `last_ms` or the read-time `now_ms()` via payload fields.
- **Cold-entity eviction:** if [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md) evicts the entity, `time_since` resets to `null` until a new post-eviction event arrives (Redis-TTL pattern, V0-MEM-GOV-01).
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. For "is the most recent match within N ms?" semantics, see [`bv.first_seen_in_window`](./first_seen_in_window.md).
- **Lifetime mode:** **the only mode.** Footprint is `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.last_seen](./last_seen.md) — the absolute timestamp `time_since` is measured from
- [bv.age](./age.md) — sibling: ms since `first_seen` (entity's earliest match), not `last_seen`
- [bv.time_since_last_n](./time_since_last_n.md) — generalization: ms since the kth most recent match (`n` required)
- [bv.first_seen_in_window](./first_seen_in_window.md) — boolean variant: "is the most recent match within N ms?"
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
