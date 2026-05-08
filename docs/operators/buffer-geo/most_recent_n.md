# bv.most_recent_n

> Circular buffer of the N most recent values. `n` is a required register-time kwarg per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## Signature

```python
bv.most_recent_n(
    field: str,
    *,
    n: int, # REQUIRED — register-time kwarg
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.most_recent_n` returns the most recent `n` non-null values of `field`,
in insertion order (oldest at index 0, newest at index `n − 1`). State is
a `Vec<Value>` of capacity `n` plus a head index — a fixed-size circular
buffer. Once `n` events have arrived, the buffer is `filled = true` and
each subsequent event overwrites the value at `head`, then advances `head`
modulo `n`. Use it for "the last 10 IPs this account logged in from",
"the last 5 device fingerprints", or "the last 20 transaction amounts on
this card" — features that need a rolling sample without summarization.

`n` is a **required keyword argument** per
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md): the lifetime-aggregation
memory contract requires every unbounded-by-default operator to declare a
finite per-entity ceiling at register time. `bv.most_recent_n`'s ceiling
is exactly `n × sizeof(Value)` bytes. The register-time JSON-prelude shim
(`pre_check_unbounded_op_in_lifetime_mode`) rejects any `most_recent_n`
payload missing `n` with the structured error code
`unbounded_op_in_lifetime_mode`. There is no fallback default — picking
`n` is a deliberate capacity-planning step. `n` is clamped to `≥ 1` at
state construction.

`bv.most_recent_n` belongs to the **bounded-buffer** family. Per-event
update is Tier 3 (~12 ns floor / ~32 ns measured per
[cost-class.md](../cost-class.md)) — one `Value::clone()` plus one indexed
write into the ring. The clone-path variance dominates: `Value::Str` clone
is `Arc::clone` (atomic bump, cheap); `Value::Bytes` clone can be expensive
for large payloads. There is no `window=` kwarg in v0 —
`bv.most_recent_n` is **lifetime-only**. For "last N matching values within
a window", compose with `@bv.event(cold_after="...")` per
[V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md), or use
[`bv.last_n`](../point-ordinal/last_n.md) — the point/ordinal sibling — if
you only need scalar values without the buffer-family insertion-order
guarantees.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the field whose last `n` values to track. Any scalar `Value` type — `i64`, `f64`, `str`, `bool`, `bytes`. |
| `n` | `int` | **Yes** | — | Number of values to retain. Must be `≥ 1` per [V0-MEM-GOV-02 BoundedByRequiredKwarg("n")](../../../.planning/REQUIREMENTS.md). Bounds the per-entity memory ceiling at register time. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the ring. |

## Returns

A `list` of up to `n` values in arrival order (oldest at index 0, newest at
index `n − 1`). Wire form is `Value::List` — Python SDK readers receive a
native `list`. When the buffer is not yet filled (`< n` events seen), the
list is the partial buffer in arrival order. Cold-start (no events)
returns the empty list `[]` — never `null`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 3** (~12 ns floor / ~32 ns measured — circular-buffer write + one `Value::clone()`) — see [cost-class.md](../cost-class.md#tier-3-algorithmic-floor-100-300-nscall--9-ops). Clone-path variance: `Value::Str` is `Arc::clone` (cheap); `Value::Bytes` of large payloads can dominate |
| Memory per entity | **`BoundedByRequiredKwarg("n")`** — `n × sizeof(Value)` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.most_recent_n` has no `window=` kwarg in v0; lifetime is the only mode |

## Examples

### Example 1: Last 10 IPs per user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    ip_address: str

@bv.table(key="user_id")
def UserRecentIps(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(recent_ips=bv.most_recent_n("ip_address", n=10))
    )

# After 12 logins from various IPs
result = app.get("UserRecentIps", "alice")
# result == {"recent_ips": ["10.0.0.3", "10.0.0.5", ..., "10.0.0.7"]}
# Length 10 — the 2 oldest IPs were rotated out.
```

### Example 2: Last 5 successful transaction amounts

```python
@bv.table(key="card_id")
def CardRecentSuccess(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(recent_amounts=bv.most_recent_n("amount",
                                                   n=5,
                                                   where=bv.col("status") == "captured"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserRecentIps",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "recent_ips": {
      "op": "most_recent_n",
      "params": {
        "field": "ip_address",
        "n": 10
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **`n` missing at register time:** rejected with structured error code `unbounded_op_in_lifetime_mode` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). The JSON-prelude shim catches this before any state is allocated.
- **`n=0` or negative `n`:** clamped to `1` at state construction (`n.max(1)`), but the SDK helper rejects pre-wire with `aggregation_invalid_param`.
- **Fewer than `n` events seen:** returns the partial list in arrival order (e.g. `["a", "b"]` after 2 events when `n=10`). The buffer is `filled = false`.
- **Empty stream / cold-start:** returns `[]` (empty list) — never `null`.
- **Null source field (`Value::Null`):** events whose `field` is `null` are skipped and do **not** consume buffer slots.
- **Missing source field:** events without `field` are skipped — no slot consumed.
- **`where=` filter excludes everything:** returns `[]` until matching events arrive.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. For a sliding-window analogue use `@bv.event(cold_after="...")` to bound the lifetime via per-entity TTL.
- **Large `Value::Bytes` cost:** the per-event clone copies the bytes; for high-throughput workloads with large payloads, consider tracking a hash or a derived id rather than the raw bytes.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); the buffer tracks server arrival order.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `n × sizeof(Value)` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) BoundedByRequiredKwarg("n").

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 3)
- [bv.last_n](../point-ordinal/last_n.md) — point/ordinal sibling (also `BoundedByRequiredKwarg("n")` — chooses between by your traceability bucket)
- [bv.first_n](../point-ordinal/first_n.md) — first-N companion (locks the first `n` matching values; never rotates)
- [bv.reservoir_sample](./reservoir_sample.md) — uniform-sample sibling (samples across the entire history rather than retaining the most recent `n`)
- [bv.lag](../point-ordinal/lag.md) — single-value `n`-events-ago companion (no buffer)
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — `BoundedByRequiredKwarg` memory governance contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
