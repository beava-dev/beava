# bv.lag

> Value of a field as of `n` events ago. `n` is a required register-time kwarg per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## Signature

```python
bv.lag(
    field: str,
    *,
    n: int, # REQUIRED — register-time kwarg
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.lag` returns the value of `field` as of exactly `n` events ago — i.e.,
the value from the event that arrived `n` matching events before the
current one. The most common shape is `bv.lag("amount", n=1)`: "what was
the previous transaction amount on this card?", ideal for delta and
rate-of-change calculations. `bv.lag(..., n=5)` walks back further.

Internally `lag` keeps a `VecDeque` of capacity `n + 1`: every accepted
event pushes onto the back; once the deque holds `n + 1` entries, the next
push pops the front. The query reads the front element — which is exactly
the value that was `n` events behind the most recent push. Until the deque
holds `n + 1` entries (i.e., until at least `n + 1` matching events have
been seen), the query returns `null`.

`n` is a **required keyword argument** per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md):
the lifetime-aggregation memory contract requires every unbounded-by-default
operator to declare a finite per-entity ceiling at register time. `bv.lag`'s
ceiling is `(n + 1) × sizeof(field)` bytes. The register-time JSON-prelude
shim (`pre_check_unbounded_op_in_lifetime_mode`) rejects any `lag` payload
without `n` with the structured error code `unbounded_op_in_lifetime_mode`.
Picking `n` is a deliberate capacity-planning step.

`bv.lag` belongs to the **point/ordinal** family. Per-event update is push_back
+ conditional pop_front (both O(1) on `VecDeque`). There is no `window=` kwarg —
`bv.lag` is **lifetime-only**. For a moving difference use `bv.lag(..., n=1)` and
subtract from the current event's value in a derivation; for a rate, use
[`bv.rate_of_change`](../velocity/rate_of_change.md) directly.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the field whose lagged value to read. Any scalar type. |
| `n` | `int` | **Yes** | — | How many events ago to look back. Must be `≥ 1` per [V0-MEM-GOV-02 BoundedByRequiredKwarg("n")](../../../.planning/REQUIREMENTS.md). Memory bound is `(n+1) × sizeof(field)`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events advance the lag-ring. |

## Returns

A single value of the source field's type. Until at least `n + 1` matching
events have been seen, the result is `null` (Python `None`). After that,
the result is the field value from exactly `n` matching events back.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~10 ns floor / ~32 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | **`BoundedByRequiredKwarg("n")`** — `(n+1) × sizeof(field)` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.lag` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: Previous transaction amount per card (delta calculation)

```python
import beava as bv

@bv.event
class Txn:
    card_id: str
    amount: float

@bv.table(key="card_id")
def CardPrevAmount(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(prev_amount=bv.lag("amount", n=1))
    )

# Push 3 events
app.push("Txn", {"card_id": "c1", "amount": 10.0})
app.push("Txn", {"card_id": "c1", "amount": 25.0})
app.push("Txn", {"card_id": "c1", "amount": 50.0})

# Query
result = app.get("CardPrevAmount", "c1")
# result == {"prev_amount": 25.0} # 1 event ago (the second one), not the most recent
```

### Example 2: Previous status code 5 events ago (failed-cluster detection)

```python
@bv.table(key="user_id")
def UserStatus5Ago(events) -> bv.Table:
    return (
        events.group_by("user_id")
              .agg(status_5_ago=bv.lag("status", n=5))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "CardPrevAmount",
  "output_kind": "table",
  "key": ["card_id"],
  "agg": {
    "prev_amount": {
      "op": "lag",
      "params": {
        "field": "amount",
        "n": 1
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **`n` missing at register time:** rejected with structured error code `unbounded_op_in_lifetime_mode` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). The register-time JSON-prelude shim (`pre_check_unbounded_op_in_lifetime_mode`) catches this before any state is allocated.
- **`n=0` or negative `n`:** rejected by the SDK helper's pre-validation; the wire-level shim catches it as a fallback.
- **Fewer than `n + 1` events seen:** returns `null`. `bv.lag` requires the ring to hold `n + 1` entries before it has a value to return — the front of a partially-full ring is **not** the lag value.
- **Empty stream / cold-start:** returns `null`.
- **Null source field:** events whose `field` is `null` are skipped — they do not advance the lag-ring at all. `lag` thus tracks the previous *non-null* value per its skip semantics.
- **`where=` filter excludes everything:** the lag-ring never advances; result stays `null` until matching events accumulate.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `(n+1) × sizeof(field)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) BoundedByRequiredKwarg("n").

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.delta_from_prev](../velocity/delta_from_prev.md) — `current - lag(field, n=1)` baked into one op (no extra derivation needed)
- [bv.rate_of_change](../velocity/rate_of_change.md) — delta divided by elapsed time
- [bv.last](./last.md) — current value (vs `lag`'s past value)
- [bv.last_n](./last_n.md) — the full window of recent values (vs `lag`'s single point in the past)
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — BoundedByRequiredKwarg memory governance contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
