# bv.last_n

> Last N observed values of a field, in arrival order (oldest first). `n` is a required register-time kwarg per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## Signature

```python
bv.last_n(
    field: str,
    *,
    n: int, # REQUIRED — register-time kwarg
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.last_n` returns the most recent `n` non-null values of `field`, in
arrival order (the oldest of the surviving `n` is at index 0; the newest
is at index `n-1`). Internally it's a `VecDeque` of capacity `n`: each
accepted event pushes onto the back; once the deque is full the next
push pops from the front. Use it for "the last 5 device fingerprints
this user logged in with" or "the last 10 transaction amounts on this
card" — features that need a rolling sample without summarization.

`n` is a **required keyword argument** per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md):
the lifetime-aggregation memory contract requires every unbounded-by-default
operator to declare a finite per-entity ceiling at register time.
`bv.last_n`'s ceiling is exactly `n × sizeof(field)` bytes. The
register-time JSON-prelude shim (`pre_check_unbounded_op_in_lifetime_mode`)
rejects any `last_n` payload without `n` with the structured error code
`unbounded_op_in_lifetime_mode`. There is no fallback — picking `n` is a
deliberate capacity-planning step.

`bv.last_n` belongs to the **point/ordinal** family. Per-event update is
push_back + a conditional pop_front when the deque is full; both are O(1)
on `VecDeque`. Memory per entity is `O(n × sizeof(field))` bounded by the
register-time `n`. There is no `window=` kwarg in v0 — `bv.last_n` is
**lifetime-only**. For a true sliding-window "values in the last 5 minutes"
view, see [`bv.most_recent_n`](../buffer-geo/most_recent_n.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the field whose last `n` values to track. Any scalar type. |
| `n` | `int` | **Yes** | — | Number of values to retain. Must be `≥ 1` per [V0-MEM-GOV-02 BoundedByRequiredKwarg("n")](../../../.planning/REQUIREMENTS.md). Bounds the per-entity memory ceiling at register time. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the deque. |

## Returns

A JSON-array string holding up to `n` values in arrival order (oldest at
index 0, newest at index `n-1`). When the entity has seen zero matching
events, the result is the empty list `"[]"`. Wire format is a
`Value::Str(serde_json::to_string(...))` because the v0 `Value` enum has
no `List` variant — Python SDK readers parse it back transparently.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~10 ns floor / ~32 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | **`BoundedByRequiredKwarg("n")`** — `n × sizeof(field)` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.last_n` has no `window=` kwarg in v0; lifetime is the only mode |

## Examples

### Example 1: Last 5 device fingerprints used by a user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    device_id: str

@bv.table(key="user_id")
def UserRecentDevices(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(recent_devices=bv.last_n("device_id", n=5))
    )

# Push 7 logins
for d in ["d1", "d2", "d3", "d4", "d5", "d6", "d7"]:
    app.push("Login", {"user_id": "alice", "device_id": d})

# Query
result = app.get("UserRecentDevices", "alice")
# result == {"recent_devices": ["d3", "d4", "d5", "d6", "d7"]}
# d1 and d2 were popped to make room
```

### Example 2: Last 3 transaction amounts on a card, success-only

```python
@bv.table(key="card_id")
def CardRecentSuccessAmounts(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(recent_amounts=bv.last_n("amount",
                                            n=3,
                                            where=bv.col("status") == "completed"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserRecentDevices",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "recent_devices": {
      "op": "last_n",
      "params": {
        "field": "device_id",
        "n": 5
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **`n` missing at register time:** rejected with structured error code `unbounded_op_in_lifetime_mode` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). The JSON-prelude shim catches this before any state is allocated.
- **`n=0` or negative `n`:** rejected by the SDK helper's pre-validation; the wire-level shim catches it as a fallback.
- **Fewer than `n` events seen:** returns the partial list in arrival order (e.g., `["a", "b"]` after 2 events when `n=5`).
- **Empty stream / cold-start:** returns the empty list `"[]"`.
- **Null source field:** events whose `field` is `null` are skipped and do **not** consume deque slots.
- **`where=` filter excludes everything:** returns `"[]"` until matching events arrive.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. For a sliding-window variant use [`bv.most_recent_n`](../buffer-geo/most_recent_n.md) (also `BoundedByRequiredKwarg("n")`, but its semantics are "N most recent within window").
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); the deque tracks server arrival order.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `n × sizeof(field)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) BoundedByRequiredKwarg("n").

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.first_n](./first_n.md) — symmetric companion: first N values (also `BoundedByRequiredKwarg("n")`)
- [bv.last](./last.md) — degenerate `n=1` case (lighter — no `VecDeque` allocation)
- [bv.most_recent_n](../buffer-geo/most_recent_n.md) — N most recent values within a window (v0 buffer family)
- [bv.lag](./lag.md) — value `n` events ago (single value, not the rolling window)
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — BoundedByRequiredKwarg memory governance contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
