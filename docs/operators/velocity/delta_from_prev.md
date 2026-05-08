# bv.delta_from_prev

> Current numeric value minus the previous matching event's value — the "absolute jump" primitive.

## Signature

```python
bv.delta_from_prev(
    field: str,
    *,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.delta_from_prev` returns `current_value - previous_value` for a
numeric `field` across consecutive matching events. Each new matching
event computes `delta = x_curr - x_prev` and overwrites the stored
delta; subsequent reads return that scalar. Unlike
[`bv.rate_of_change`](./rate_of_change.md), `delta_from_prev` does **not**
divide by elapsed time — it is the raw absolute jump in the field value
between the two most recent matching arrivals. Read it as "how much did
the amount move on this latest transaction?", "what is this entity's
last-event swing?", or "what was the change since the previous reading?".

This is the canonical "sudden movement" primitive — useful for any
gauge-style signal where the absolute step matters more than the rate
(price changes, scoreboard updates, balance reconciliations). Pair it
with [`bv.value_change_count`](./value_change_count.md) when you want to
count distinct flips rather than measure their magnitude, or with
[`bv.rate_of_change`](./rate_of_change.md) when the time component
matters (e.g. acceleration anomalies on a smoothly-evolving signal).

`bv.delta_from_prev` belongs to the **velocity** family. It is the only
velocity op that takes **no `window=` kwarg** — the state is purely
"last value seen", with no time component beyond arrival order.
Per-event update is one numeric extract, one subtract, and two scalar
writes; cost is **Tier 1** (~8 ns floor / ~28 ns measured) and memory is
`O(1)` per entity (`last_value`, `current_delta`, two flags).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Numeric field (`i64` or `f64`) to track. Non-numeric values are silently skipped. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the delta. |

(No `window=` kwarg — `bv.delta_from_prev` is lifetime-only by design. Passing `window=` raises `TypeError` at SDK-helper-call time.)

## Returns

A single `f64` — the most recent jump in field value, in the same units as `field`. Cold-start (no matching event seen) and one-event start (no prior value to diff against) both return `null` (Python `None`). The first matching event seeds `last_value` but does **not** set a delta yet.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns floor / ~28 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `DeltaFromPrevState` ≈ 24 B (`last_value: f64`, `current_delta: f64`, `initialized: bool`, `has_delta: bool`) |
| Lifetime mode | **Required** — no `window=` kwarg; the only mode |

## Examples

### Example 1: Account-balance jump per user

```python
import beava as bv

@bv.event
class BalanceUpdate:
    user_id: str
    balance: float

@bv.table(key="user_id")
def UserBalanceJump(updates) -> bv.Table:
    return (
        updates.group_by("user_id")
               .agg(last_jump=bv.delta_from_prev("balance"))
    )

# Push events
app.push("BalanceUpdate", {"user_id": "alice", "balance": 1000.0}) # last_jump = null
app.push("BalanceUpdate", {"user_id": "alice", "balance": 1250.0}) # last_jump = 250
app.push("BalanceUpdate", {"user_id": "alice", "balance": 1200.0}) # last_jump = -50

# Query
result = app.get("UserBalanceJump", "alice")
# result == {"last_jump": -50.0}
```

### Example 2: Filtered transaction-amount swing

```python
@bv.table(key="user_id")
def UserOkAmtSwing(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(ok_amt_swing=bv.delta_from_prev(
                     "amount",
                     where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserBalanceJump",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "last_jump": {
      "op": "delta_from_prev",
      "params": {
        "field": "balance"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null`. The first matching event seeds `last_value` and flips `initialized` but does **not** set a delta yet.
- **Single-event entity:** result is `null` until a second matching event arrives.
- **Two events with identical values (`x_curr == x_prev`):** delta is `0.0` (not `null`) — the helper has computed a delta, it just happens to be zero.
- **Missing or non-numeric `field`:** the event is silently skipped (no update); the delta and `last_value` are unchanged. Matches the [`bv.sum`](../core/sum.md) / [`bv.mean`](../core/mean.md) behavior.
- **`where=` filter excludes the event:** no update; `last_value` is not refreshed by non-matching events. This means the "previous" value the next matching event diffs against is the previous **matching** event, not the previous event in arrival order overall.
- **`window=` argument passed:** raises `TypeError` at SDK-helper-call time — `delta_from_prev` is lifetime-only by design. Use [`bv.rate_of_change`](./rate_of_change.md) for a windowed alternative.
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the underlying state per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md); the next post-eviction matching event reseeds `last_value`.
- **Late or duplicate event:** processed in arrival order only — beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md). A late event is just "the next event" with whatever `now_ms` the server stamps.

## See also

- [Velocity family index](./index.md) — overview of all 9 velocity-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 1; cheapest velocity op)
- [bv.rate_of_change](./rate_of_change.md) — same primitive divided by elapsed `Δt`
- [bv.value_change_count](./value_change_count.md) — count of distinct flips, not their magnitude
- [bv.lag](../point-ordinal/lag.md) — value `n` events ago (more general historical lookup; `lag(field, n=1)` returns the `last_value` itself, not the delta)
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
