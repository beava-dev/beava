# bv.last

> Most recent observed value of a field, by arrival order (processing-time, not event-time).

## Signature

```python
bv.last(
    field: str,
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.last` returns the most-recently-observed non-null value of `field` for
the entity, where "most recent" is defined by **server arrival order** —
the order events flowed through beava's apply loop. This is processing-time
semantics per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md):
beava does not consult any event-time field. If two events arrive at the
server in (server-clock) order A → B, then `last(field)` returns B's value
even if a downstream `event_time_ms` field on event A is "later".

The operator preserves the source field's type. Each accepted event
overwrites the previous value with one `Value::clone()`. Null values from
the source field are skipped — `last` keeps the previously-captured value
rather than overwriting with null. The optional `where=` predicate gates
which events update the slot.

`bv.last` belongs to the **point/ordinal** family. Per-event update is a
single field lookup plus one `Value::clone()`; memory per entity is `O(1)`
regardless of stream length. There is no `window=` kwarg — `last` is
**lifetime-only** by definition. For a windowed "most recent in window"
view use [`bv.last_n(field, n=1, window="...")`](./last_n.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the field whose most-recent value to track. Any scalar type. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the slot. |

## Returns

A single value of the source field's type. When the entity has seen zero
matching events with a non-null `field`, the result is `null` (Python `None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single `Option<Value>` slot per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.last` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: Most recent device-id seen for a user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    device_id: str

@bv.table(key="user_id")
def UserLastDevice(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(last_device=bv.last("device_id"))
    )

# Push events
app.push("Login", {"user_id": "alice", "device_id": "iphone-12"})
app.push("Login", {"user_id": "alice", "device_id": "macbook-pro"})

# Query
result = app.get("UserLastDevice", "alice")
# result == {"last_device": "macbook-pro"}
```

### Example 2: Most recent successful transaction status per card

```python
@bv.table(key="card_id")
def CardLastSuccess(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(last_ok_amount=bv.last("amount",
                                          where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserLastDevice",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "last_device": {
      "op": "last",
      "params": {
        "field": "device_id"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null` (Python `None`).
- **Null source field:** events whose `field` is `null` are skipped — the previously-captured value is preserved.
- **`where=` filter excludes everything:** result is `null` until a matching event arrives.
- **Field missing from event:** treated identically to null — skipped.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); `last` always reflects the **server arrival order**, never the event payload's timestamp field.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. Use [`bv.last_n(n=1, window="...")`](./last_n.md) for a windowed alternative.
- **Lifetime mode:** **the only mode.** Footprint is `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.first](./first.md) — symmetric companion: first observed value
- [bv.last_n](./last_n.md) — last **N** values (bounded by required `n` kwarg; supports `window=`)
- [bv.last_seen](../recency/last_seen.md) — most recent arrival **timestamp** instead of value
- [bv.lag](./lag.md) — value `n` events ago (instead of the most recent)
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
