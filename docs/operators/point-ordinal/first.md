# bv.first

> First observed value of a field across the entity's lifetime.

## Signature

```python
bv.first(
    field: str,
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.first` returns the very first non-null value of `field` that the entity
has observed since registration (or since the most recent
[`cold_after=`](../../../.planning/REQUIREMENTS.md) eviction, if configured).
Once captured, the value is sticky — every subsequent matching event is a
no-op for this op's state. Read it as "what was the first device this user
ever logged in from", "what was the first IP we saw on this card", "what
was the first ad creative this session served".

The operator preserves the source field's type. If `field` is `Str`, you
get a `Str` back; if `field` is an `i64` or `f64`, you get a number back.
Null values from the source field are skipped — `first` will keep waiting
for a real value to arrive. The optional `where=` predicate gates which
events are considered candidates.

`bv.first` belongs to the **point/ordinal** family. Per-event update is a
single `Option::is_some()` early-exit branch plus (on the cold path) one
`Value::clone()`; memory per entity is `O(1)` regardless of stream length.
There is no `window=` kwarg — `first` is **lifetime-only** by definition.
For a windowed alternative, compose with [`bv.last_n(n=1, window="...")`](../buffer-geo/most_recent_n.md)
or use [`bv.first_seen`](../recency/first_seen.md) if you only need the
arrival timestamp.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the field whose first value to capture. Any scalar type (`str`, `i64`, `f64`, `bool`). |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events are candidates for "first". |

## Returns

A single value of the source field's type. When the entity has seen zero
matching events with a non-null `field`, the result is `null` (Python `None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~5 ns floor / ~25 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single `Option<Value>` slot per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.first` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: First device-id ever seen for a user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    device_id: str

@bv.table(key="user_id")
def UserFirstDevice(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(first_device=bv.first("device_id"))
    )

# Push events
app.push("Login", {"user_id": "alice", "device_id": "iphone-12"})
app.push("Login", {"user_id": "alice", "device_id": "macbook-pro"})

# Query
result = app.get("UserFirstDevice", "alice")
# result == {"first_device": "iphone-12"} # the second event is a no-op
```

### Example 2: First successful payment amount per user

```python
@bv.table(key="user_id")
def UserFirstPayment(payments) -> bv.Table:
    return (
        payments.group_by("user_id")
                .agg(first_amount=bv.first("amount",
                                            where=bv.col("status") == "completed"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserFirstDevice",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "first_device": {
      "op": "first",
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
- **Null source field:** events whose `field` is `null` are skipped — `first` keeps waiting until a non-null arrives. This means "first" is "first non-null" by construction.
- **`where=` filter excludes everything:** result is `null`; once a matching event eventually arrives, the value is captured and sticks for the entity's lifetime.
- **Field missing from event:** treated identically to null — skipped.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. There is no windowed `first`; compose with [`bv.last_n(n=1, window="...")`](../buffer-geo/most_recent_n.md) for a windowed "earliest in this window" approximation, or use [`bv.first_seen`](../recency/first_seen.md) if only the arrival time matters.
- **Lifetime mode:** **the only mode.** Footprint is `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — a single `Option<Value>` slot.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.last](./last.md) — symmetric companion: most recent value
- [bv.first_n](./first_n.md) — first **N** values (bounded by required `n` kwarg)
- [bv.first_seen](../recency/first_seen.md) — first arrival **timestamp** instead of value
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
