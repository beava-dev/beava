# bv.first_seen

> Server arrival timestamp of the first matching event. Server processing-time per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md) — **not** event-time.

## Signature

```python
bv.first_seen(
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.first_seen` returns the server-side arrival timestamp (`now_ms()` at
the apply path) of the very first event that matched `where=` for this
entity. The value is sticky — once captured, every subsequent event is a
no-op for this op's state. Read it as "when did we first see this user?",
"when did this card show its first transaction?", or "when did this device
first authenticate?".

The timestamp is **server processing-time**, captured by reading the
hand-rolled apply loop's `now_ms()` clock when the event is applied. beava
does **not** consult any event-time field on the payload, per
[`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md)
(locked 2026-04-30). If your producer adds a `event_ts` field, beava ignores
it for this op — `first_seen` is "first **arrival**", not "earliest
event-time". This is intentional: the Redis-shaped semantics eliminate
event-time / watermark / late-arrival concerns from the user's mental model.

`bv.first_seen` belongs to the **recency** family. Per-event update is two
`Option<i64>` writes (`first_ms` and `last_ms` are kept in the shared
`SeenState` struct that powers `first_seen`, `last_seen`, `age`, `has_seen`,
and `time_since`). Memory per entity is `O(1)` regardless of stream length.
There is no `window=` kwarg — `bv.first_seen` is **lifetime-only**.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events count toward "first seen". Without `where=`, every event is a candidate. |

## Returns

A single `Datetime` value (`int64` milliseconds since the Unix epoch). When
the entity has seen zero matching events, the result is `null` (Python
`None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single `Option<i64>` slot in the shared `SeenState` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.first_seen` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: First-seen timestamp per user (account-creation proxy)

```python
import beava as bv

@bv.event
class Login:
    user_id: str

@bv.table(key="user_id")
def UserCreatedAt(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(first_login_ms=bv.first_seen())
    )

# Push events
app.push("Login", {"user_id": "alice"}) # arrives at server time t=1700000000000
app.push("Login", {"user_id": "alice"}) # arrives at server time t=1700000005000

# Query
result = app.get("UserCreatedAt", "alice")
# result == {"first_login_ms": 1700000000000} # the second login is a no-op
```

### Example 2: First successful payment timestamp per card

```python
@bv.table(key="card_id")
def CardFirstSuccessAt(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(first_ok_ms=bv.first_seen(where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserCreatedAt",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "first_login_ms": {
      "op": "first_seen",
      "params": {}
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null` (Python `None`).
- **`where=` filter excludes everything:** result is `null`; once a matching event arrives, the timestamp is captured and stays for the entity's lifetime.
- **Server-time, NOT event-time:** the captured value is the server's `now_ms()` at apply, not any payload field. Per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md), beava intentionally has no event-time concept. Producers cannot influence the captured timestamp via the payload.
- **Cold-entity eviction:** if [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md) is configured and the entity is evicted, the next event after eviction is treated as a fresh entity — `first_seen` resets to that re-arrival's `now_ms()`. This is the documented Redis-TTL pattern (V0-MEM-GOV-01).
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. For "is this value new in the last N ms?" semantics, see [`bv.first_seen_in_window`](./first_seen_in_window.md).
- **Lifetime mode:** **the only mode.** Footprint is `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.last_seen](./last_seen.md) — symmetric companion: most recent arrival timestamp
- [bv.age](./age.md) — milliseconds since `first_seen`, computed at read time
- [bv.has_seen](./has_seen.md) — boolean variant: ever-matched, no timestamp
- [bv.first_seen_in_window](./first_seen_in_window.md) — windowed variant: "in the last N ms?"
- [bv.first](../point-ordinal/first.md) — first **value of a field**, instead of arrival timestamp
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
