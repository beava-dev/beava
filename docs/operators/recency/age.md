# bv.age

> Milliseconds since `first_seen`, computed at read time. Server processing-time per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md).

## Signature

```python
bv.age(
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.age` returns the elapsed milliseconds between the entity's
[`first_seen`](./first_seen.md) timestamp and the **query time** (`now_ms()`
at the moment `app.get(...)` resolves). Read it as "how long has this
account existed in our system?", "how many ms since this card's first
transaction?", or "what's the lifetime age of this device?".

The interesting property of `bv.age` is that it changes between reads
**without any new events** — because the right-hand side of
`now_ms() - first_ms` is captured at query time, not at apply time. State
on the apply path is identical to `first_seen` (the first arrival ms is
recorded once and never overwritten); the subtraction happens server-side
when the read fans out from the registry to the entity. This makes `age`
a "time-travel" feature: it grows with wall-clock seconds even on
quiescent entities.

Both timestamps are **server processing-time**: `first_ms` is server `now_ms()`
at the original arrival; the read-side `now_ms()` is the server clock when
`app.get(...)` reaches the entity. beava intentionally does not consult
any event-time field — see [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md)
(locked 2026-04-30).

`bv.age` belongs to the **recency** family. Per-event update is two
`Option<i64>` writes (the same `SeenState` shared with `first_seen`,
`last_seen`, `has_seen`, `time_since`). Memory per entity is `O(1)`.
There is no `window=` kwarg — `bv.age` is **lifetime-only** by definition.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events count toward `first_seen`. Without `where=`, every event is a candidate. |

## Returns

A single `i64` value: the number of milliseconds between `first_seen` and
the query-time `now_ms()`. When the entity has seen zero matching events,
the result is `null` (Python `None`). The value is clamped to a non-negative
range — clock skew that would produce a negative age returns 0 instead.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single `Option<i64>` slot in the shared `SeenState` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.age` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: Account age in milliseconds per user

```python
import beava as bv

@bv.event
class Login:
    user_id: str

@bv.table(key="user_id")
def UserAccountAge(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(account_age_ms=bv.age())
    )

# Push first event at server time t=1700000000000
app.push("Login", {"user_id": "alice"})

# Query at server time t=1700000060000 (1 minute later)
result = app.get("UserAccountAge", "alice")
# result == {"account_age_ms": 60000}

# Query again at t=1700003600000 (1 hour later) — same state, different age
result = app.get("UserAccountAge", "alice")
# result == {"account_age_ms": 3600000}
```

### Example 2: Age computed only against successful logins

```python
@bv.table(key="user_id")
def UserSuccessAge(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(success_age_ms=bv.age(where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserAccountAge",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "account_age_ms": {
      "op": "age",
      "params": {}
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null`. `bv.age` requires at least one matching event to anchor `first_seen`.
- **`where=` filter excludes everything:** result is `null` until a matching event arrives.
- **Reads grow without new events:** the subtraction `now_ms() - first_ms` happens at query time, so the returned value increases between reads even if no events arrived. This is intentional — it is what makes `age` a useful "time-since-creation" feature.
- **Clock-skew safety:** age is clamped to `>= 0`. If `query_time_ms < first_ms` (only possible under clock-skew or replay scenarios), the result is `0`, not a negative number.
- **Server-time, NOT event-time:** both endpoints are server-side per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md). Producers cannot influence the captured `first_ms` or the read-time `now_ms()` via payload fields.
- **Cold-entity eviction:** if [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md) evicts the entity, `age` resets to "ms since the next post-eviction arrival" — the entity is treated as fresh per the Redis-TTL pattern (V0-MEM-GOV-01).
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. `age` is "since first observation", which is inherently lifetime; for windowed-recency see [`bv.first_seen_in_window`](./first_seen_in_window.md).
- **Lifetime mode:** **the only mode.** Footprint is `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.first_seen](./first_seen.md) — the absolute timestamp `age` is measured from
- [bv.time_since](./time_since.md) — sibling: ms since `last_seen` (the most recent match), not `first_seen` (the earliest)
- [bv.has_seen](./has_seen.md) — boolean variant: ever-matched, no duration
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
