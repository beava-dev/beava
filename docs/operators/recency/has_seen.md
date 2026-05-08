# bv.has_seen

> Boolean: has the entity ever matched the predicate? `O(1)` flag, no timestamp.

## Signature

```python
bv.has_seen(
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.has_seen` returns `true` once the entity has observed at least one
matching event, and `false` otherwise. It is the lightest possible member
of the recency family — internally it just queries whether the shared
`SeenState`'s `first_ms` slot is `Some(_)`. Use it as a fast "has this
user ever logged in?", "has this card ever been used for an international
transaction?", or "has this device ever shown a fraudulent payment?".

The boolean is sticky once flipped — beava never clears `has_seen` back
to false. The only way for it to revert is a [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md)
eviction, which deletes the entire entity state (Redis-TTL pattern); the
next event after eviction starts a fresh state with `has_seen = false`.

`bv.has_seen` belongs to the **recency** family. It shares `SeenState` with
`first_seen`, `last_seen`, `age`, and `time_since`, so registering several
of them on the same `where=` predicate costs roughly the same as
registering one (the four siblings read different methods off the same
struct). Per-event update is two `Option<i64>` writes; memory per entity
is `O(1)`. There is no `window=` kwarg — `bv.has_seen` is **lifetime-only**.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events flip `has_seen` to true. Without `where=`, the very first event flips the flag. |

## Returns

A single `bool`. Always returns `true` or `false`; never `null`. The
cold-start value (no events seen) is `false`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~4 ns floor / ~25 ns measured — fastest recency op) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single `Option<i64>` slot in the shared `SeenState` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.has_seen` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: Has this user ever made a successful payment?

```python
import beava as bv

@bv.event
class Payment:
    user_id: str
    status: str

@bv.table(key="user_id")
def UserHasPaid(payments) -> bv.Table:
    return (
        payments.group_by("user_id")
                .agg(has_paid=bv.has_seen(where=bv.col("status") == "ok"))
    )

# Push events
app.push("Payment", {"user_id": "alice", "status": "failed"})
app.push("Payment", {"user_id": "alice", "status": "ok"})
app.push("Payment", {"user_id": "alice", "status": "failed"})

# Query
result = app.get("UserHasPaid", "alice")
# result == {"has_paid": True} # the second event flipped it; subsequent failures don't matter
```

### Example 2: Has this card ever been used internationally?

```python
@bv.table(key="card_id")
def CardUsedInternationally(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(intl=bv.has_seen(where=bv.col("country") != "US"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserHasPaid",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "has_paid": {
      "op": "has_seen",
      "params": {
        "where": "status == 'ok'"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `false` (not `null` — `has_seen` always returns a bool).
- **`where=` filter excludes everything:** result is `false` until a matching event arrives, then sticks at `true`.
- **Once `true`, never reverts:** beava never decrements or clears `has_seen` based on events alone. The only reset path is full entity eviction via [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md), which deletes the underlying `SeenState`.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. For "ever matched in the last N ms?" semantics, see [`bv.first_seen_in_window`](./first_seen_in_window.md).
- **Lifetime mode:** **the only mode.** Footprint is `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1, fastest recency op)
- [bv.first_seen](./first_seen.md) — same `SeenState`; returns the timestamp instead of the bool
- [bv.first_seen_in_window](./first_seen_in_window.md) — "ever matched in the last N ms?" — windowed boolean
- [bv.streak](./streak.md) — count of *consecutive* matches, instead of the cumulative bool
- [bv.bloom_member](../sketch/bloom_member.md) — Bloom variant: "has this **value** ever been seen for this entity?"
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
