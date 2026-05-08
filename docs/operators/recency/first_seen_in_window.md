# bv.first_seen_in_window

> Bool: was the entity's most recent matching event within the last `window` ms? `O(1)` state; computed at read time. Server processing-time per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md).

## Signature

```python
bv.first_seen_in_window(
    *,
    window: str, # REQUIRED — register-time kwarg
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.first_seen_in_window` returns `true` if the entity's most recent matching
event arrived within the last `window` milliseconds (measured at query time),
and `false` otherwise. Read it as "has this user been active in the last
hour?", "did this card see a transaction in the last 5 minutes?", or "is
this device's auth fresh in the last 30 seconds?".

Despite its name, the operator is **lifetime-state**: it stores only the
most recent matching arrival's `now_ms()` (one `Option<i64>` slot, plus
the parameter `window_ms` baked in at register time) and computes the
`age = now_ms() - last_ms; age < window_ms` decision at query time. The
"window" is therefore a **read-time horizon**, not a tumbling-bucket
window — there is no per-bucket state, no expiry path, and the operator
is **not** wrapped in `WindowedOp`. The `window=` kwarg is required by
the operator's semantics (the window length defines the question being
asked); it is enforced at register time.

The query returns `false` when the entity has never matched (cold start)
**and** when the most recent match is older than `window` ms. This means
the cold-start return value differs from the sibling `Datetime`-typed ops
([`bv.first_seen`](./first_seen.md), [`bv.last_seen`](./last_seen.md))
which return `null` on cold-start: `bv.first_seen_in_window` always returns
a `bool`, never `null`.

All timestamps are **server processing-time** per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md)
(locked 2026-04-30) — beava intentionally has no event-time concept.

`bv.first_seen_in_window` belongs to the **recency** family. Per-event
update is one `Option<i64>` write. Memory per entity is `O(1)` regardless
of stream length or `window` size. The bigger the window, the more
events qualify as "in the window" — but the state cost is constant.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `window` | `str` | **Yes** | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` (e.g., `"30s"`, `"5m"`, `"1h"`). The read-time horizon for "in the window?". `"forever"` is rejected — for "ever matched" use [`bv.has_seen`](./has_seen.md). |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the timestamp. |

## Returns

A single `bool`. Returns `true` iff the entity has at least one matching
event whose arrival was less than `window` ms before query time. Cold-start
returns `false` (never `null`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single `Option<i64>` slot in `FirstSeenInWindowState` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) (the `window_ms` parameter is shared with the descriptor, not per-entity state) |
| Lifetime mode | **Special** — windowed semantics, but `O(1)` lifetime state. The `window=` kwarg is **required** at register time. |

## Examples

### Example 1: Was this user active in the last hour?

```python
import beava as bv

@bv.event
class Activity:
    user_id: str

@bv.table(key="user_id")
def UserActiveLastHour(activity) -> bv.Table:
    return (
        activity.group_by("user_id")
                .agg(active_1h=bv.first_seen_in_window(window="1h"))
    )

# Push at server time t=1700000000000
app.push("Activity", {"user_id": "alice"})

# Query at t=1700000600000 (10 minutes later) — well within 1h
result = app.get("UserActiveLastHour", "alice")
# result == {"active_1h": True}

# Query at t=1700004000000 (over 1h later) — outside the window
# result == {"active_1h": False}
```

### Example 2: Did this card see a successful payment in the last 5 minutes?

```python
@bv.table(key="card_id")
def CardRecentSuccess(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(success_5m=bv.first_seen_in_window(
                                window="5m",
                                where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserActiveLastHour",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "active_1h": {
      "op": "first_seen_in_window",
      "params": {
        "window": "1h"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `false` (not `null` — `first_seen_in_window` always returns a bool).
- **Most recent match older than `window`:** result is `false`. The slot is not cleared on staleness — only the read-time comparison flips. The next matching event refreshes the timestamp.
- **`where=` filter excludes everything:** result is `false` until a matching event arrives.
- **`window` missing at register time:** raises `ValueError` at SDK-helper-call time (the `window=` kwarg is required by the function signature). The wire-level register validator catches it as a fallback.
- **`window="forever"`:** rejected. For "has the entity ever matched?", use [`bv.has_seen`](./has_seen.md), which is the lifetime-boolean variant.
- **Reads flip false → true and back without new events:** the slot stays the same, but query-time `now_ms() - last_ms` grows; once it crosses the threshold, the read returns false. This is intentional — windowed-recency is the question being asked.
- **Server-time, NOT event-time:** the captured value is server `now_ms()` at apply per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md). Producers cannot influence the captured timestamp via the payload.
- **Cold-entity eviction:** if [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md) evicts the entity, the slot is dropped; the next event after eviction starts a fresh `last_ms` (Redis-TTL pattern, V0-MEM-GOV-01).
- **Lifetime state, windowed semantics:** unlike v0/10 windowed ops (which carry up to 64 buckets), `first_seen_in_window` is `O(1)` because it only needs the last arrival timestamp + a constant window threshold to decide.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.has_seen](./has_seen.md) — lifetime-boolean variant: "ever matched?", no window
- [bv.last_seen](./last_seen.md) — returns the absolute timestamp instead of the in-window boolean
- [bv.time_since](./time_since.md) — returns ms-since-last-match instead of the in-window boolean
- [bv.bloom_member](../sketch/bloom_member.md) — Bloom variant: "has this **value** been seen?", uses a probabilistic sketch instead of a single timestamp
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
