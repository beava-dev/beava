# bv.reservoir_sample

> Uniform K-sample over the entity's full history via Vitter Algorithm R. `samples` is a required register-time kwarg per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## Signature

```python
bv.reservoir_sample(
    field: str,
    *,
    samples: int, # REQUIRED — register-time kwarg
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.reservoir_sample` returns a uniform random sample of `samples` values
from the entity's full history of matching events using Vitter Algorithm R
(Vitter, 1985). For the first `samples` events the reservoir fills directly;
each subsequent event is admitted with probability `samples/items_seen`,
overwriting a uniformly chosen existing slot. The result is statistically
indistinguishable from sampling `samples` of `items_seen` events without
replacement, in a single pass. Use it for "show me 100 representative
transactions across this user's lifetime" or "100 random failed-login
attempts to spot-check" — features that need a uniform sample of the entire
event history without storing every event.

`samples` is a **required keyword argument** per
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md): the lifetime-aggregation
memory contract requires every unbounded-by-default operator to declare a
finite per-entity ceiling at register time. `bv.reservoir_sample`'s ceiling
is exactly `samples × sizeof(Value)` bytes plus a `u64` for `items_seen`.
The register-time JSON-prelude shim
(`pre_check_unbounded_op_in_lifetime_mode`) rejects any `reservoir_sample`
payload missing `samples` with the structured error code
`unbounded_op_in_lifetime_mode`. There is no fallback default — picking
`samples` is a deliberate capacity-planning + statistical-power step.
`samples` is clamped to `≥ 1` at state construction.

`bv.reservoir_sample` belongs to the **bounded-buffer** family. Per-event
update is Tier 3 (~14 ns floor / ~35 ns measured per
[cost-class.md](../cost-class.md)) — one `Value::clone()`, one modulo, one
indexed write. The clone-path variance dominates (`Value::Bytes` of large
payloads can be expensive — see [`bv.most_recent_n`](./most_recent_n.md) for
the same caveat).

**Determinism: no `rand::` dependency.** The random index is driven by an
inline xorshift64 PRNG seeded from `items_seen` XOR'd with the
`0x9E37_79B9_7F4A_7C15` golden-ratio constant. The same event sequence
always produces the same reservoir — replay-safe. There is no `window=`
kwarg in v0 — `bv.reservoir_sample` is **lifetime-only** by design (the
algorithm samples from the entire history). For "uniform sample within the
last 30 days", compose with `@bv.event(cold_after="30d")` per
[V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the field whose values to sample. Any scalar `Value` type. |
| `samples` | `int` | **Yes** | — | Reservoir size — number of values to retain. Must be `≥ 1` per [V0-MEM-GOV-02 BoundedByRequiredKwarg("samples")](../../../.planning/REQUIREMENTS.md). Bounds the per-entity memory ceiling at register time. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events are considered for the reservoir. |

Note: the wire-form `params` field is named `samples`, not `k`, to match
the v0 SDK signature (`samples=`) and the
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) classifier
`BoundedByRequiredKwarg("samples")`.

## Returns

A `list` of up to `samples` values from the entity's full matching event
history, sampled uniformly. Wire form is `Value::List` — Python SDK readers
receive a native `list`. The order of the values within the list is the
arrival order in which they entered the reservoir (not their original
arrival rank), which is implementation-defined and should not be relied
upon. When fewer than `samples` events have been observed, the list is the
partial reservoir. Cold-start (no events) returns the empty list `[]` —
never `null`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 3** (~14 ns floor / ~35 ns measured — xorshift PRNG + modulo + one `Value::clone()`) — see [cost-class.md](../cost-class.md#tier-3-algorithmic-floor-100-300-nscall--9-ops). Clone-path variance: `Value::Str` is `Arc::clone` (cheap); `Value::Bytes` of large payloads can dominate |
| Memory per entity | **`BoundedByRequiredKwarg("samples")`** — `samples × sizeof(Value)` bytes + 1 `u64` (items_seen) per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.reservoir_sample` has no `window=` kwarg in v0; lifetime is the only mode (the algorithm samples from the entire history by design) |

## Examples

### Example 1: 100-sample of lifetime transaction amounts per user

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserAmountSample(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(amount_sample=bv.reservoir_sample("amount", samples=100))
    )

# After 50,000 transactions for "alice":
result = app.get("UserAmountSample", "alice")
# result == {"amount_sample": [12.5, 87.0, 240.0, ...]} # 100 values uniformly chosen
```

### Example 2: 50-sample of failed login IPs

```python
@bv.table(key="account_id")
def AccountFailedLoginIps(logins) -> bv.Table:
    return (
        logins.group_by("account_id")
              .agg(failed_ip_sample=bv.reservoir_sample("ip_address",
                                                          samples=50,
                                                          where=bv.col("status") == "failed"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserAmountSample",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "amount_sample": {
      "op": "reservoir_sample",
      "params": {
        "field": "amount",
        "samples": 100
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **`samples` missing at register time:** rejected with structured error code `unbounded_op_in_lifetime_mode` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). The JSON-prelude shim catches this before any state is allocated.
- **`samples=0` or negative `samples`:** clamped to `1` at state construction (`samples.max(1)`), but the SDK helper rejects pre-wire with `aggregation_invalid_param`.
- **Fewer than `samples` events seen:** returns the partial reservoir (e.g. `[v1, v2, v3]` after 3 events when `samples=100`).
- **Empty stream / cold-start:** returns `[]` (empty list) — never `null`.
- **Null source field (`Value::Null`):** events whose `field` is `null` are skipped and do **not** count toward `items_seen` (the reservoir's denominator).
- **Missing source field:** events without `field` are skipped — does not advance `items_seen`.
- **`where=` filter excludes everything:** returns `[]` until matching events arrive.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. The algorithm requires the entire history; for "uniform sample over the last N days" use `@bv.event(cold_after="...")` to bound the lifetime via per-entity TTL.
- **Determinism guarantee:** the xorshift PRNG is seeded from `items_seen` XOR'd with a golden-ratio constant — no calls to `rand::` or wall-clock — so a snapshot + WAL replay reconstructs the **same reservoir contents**. This makes `reservoir_sample` safe for replay-determinism contracts.
- **Sampling-quality guarantee:** Algorithm R (Vitter, 1985) is provably uniform — each of the `items_seen` events has equal probability `samples/items_seen` of appearing in the reservoir.
- **Large `Value::Bytes` cost:** the per-event admission clones the value into the reservoir; for high-throughput workloads with large payloads, sample a derived id (hash, summary) rather than the raw bytes.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); admission probability is governed by server arrival order via `items_seen`.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `samples × sizeof(Value)` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) BoundedByRequiredKwarg("samples").

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 3)
- [bv.most_recent_n](./most_recent_n.md) — recency sibling (last `n` events vs. uniform sample over all events; same `BoundedByRequiredKwarg` pattern, different kwarg name)
- [bv.first_n](../point-ordinal/first_n.md) — first-N companion (locks the first `n` matching values; never rotates)
- [bv.last_n](../point-ordinal/last_n.md) — last-N companion (point/ordinal family — chooses between by your traceability bucket)
- [bv.top_k](../sketch/top_k.md) — frequency-weighted-sample companion (top-K by count, not uniform random)
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — `BoundedByRequiredKwarg` memory governance contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
