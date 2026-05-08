# bv.top_k

> Top-K most-frequent values, backed by SpaceSaving + bounded heap.

## Signature

```python
bv.top_k(
    field: str,
    k: int,
    *,
    window: str | None = None,
    where: bv.Col | None = None,
    exact_threshold: int = 1024,
    hybrid_width: int = 2048,
    hybrid_depth: int = 4,
) -> AggDescriptor
```

## Description

`bv.top_k` returns the K most-frequent values of a field across events that
match the optional `where=` predicate. Backed by a hybrid: while the entity
has fewer than `exact_threshold` distinct values, the state holds them in an
exact frequency table; once the threshold is crossed, the state promotes to
a Count-Min Sketch (`hybrid_width × hybrid_depth`) coupled with a bounded
heavy-hitters heap of capacity K.

Use `bv.top_k("merchant", k=5, window="1h")` for "the 5 most-used merchants
in the last hour", or `bv.top_k("ip_address", k=10, window="forever")` for
"the 10 IP addresses this account has used most over its lifetime". The
parameter `k` is the **required width** of the result list — the value
governs both the result shape AND the per-entity memory ceiling under the
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) lifetime
contract (`BoundedByConfig("k", 10)`: defaults to 10 when omitted, but in
the public Python signature `k` is required).

`bv.top_k` belongs to the **sketch** family. Per-event update is the most
expensive of the sketch family because the heavy-hitters heap walk is
`O(log k)`; in exact mode it's a hashtable update.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the field to rank. Any hashable type (`str`, `i64`, `f64`). |
| `k` | `int` | Yes | — | Number of top values to retain (≥1). Caps per-entity memory under `BoundedByConfig("k", 10)`. |
| `window` | `str` | No | `None` (lifetime) | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events contribute. |
| `exact_threshold` | `int` | No | `1024` | Distinct-value count below which exact-frequency mode is used. |
| `hybrid_width` | `int` | No | `2048` | Count-Min Sketch width (number of buckets per row). |
| `hybrid_depth` | `int` | No | `4` | Count-Min Sketch depth (number of independent hash rows). |

## Returns

A list of `[value, count]` pairs, length ≤ `k`, sorted by descending count.
When the entity has seen zero matching events, the result is `[]` (the empty
list).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 2** (Exact mode, ~95 ns measured) — see [cost-class.md](../cost-class.md#tier-2-moderate-30-100-nscall--6-ops) |
| | **Tier 3** (Hybrid mode, ~250 ns floor / ~300 ns measured) — see [cost-class.md](../cost-class.md#tier-3-algorithmic-floor-100-300-nscall--9-ops) |
| Memory per entity | `BoundedByConfig("k", 10)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — exact frequency table up to `exact_threshold` entries, then CMS (`hybrid_width × hybrid_depth × i64`) plus heap of capacity `k` |
| Lifetime mode (`window=None`) | **Allowed** — `BoundedByConfig` declares the per-entity ceiling at register time |

## Examples

### Example 1: Top 5 merchants per user, hourly

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    merchant: str
    amount: float

@bv.table(key="user_id")
def UserTopMerchants(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(top_merchants_1h=bv.top_k("merchant", k=5, window="1h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "merchant": "amazon", "amount": 50.0})
app.push("Txn", {"user_id": "alice", "merchant": "amazon", "amount": 20.0})
app.push("Txn", {"user_id": "alice", "merchant": "starbucks", "amount": 5.0})

# Query
result = app.get("UserTopMerchants", "alice")
# result == {"top_merchants_1h": [["amazon", 2], ["starbucks", 1]]}
```

### Example 2: Top 10 lifetime IP addresses for successful logins

```python
@bv.table(key="user_id")
def UserIpFootprint(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(top_ips=bv.top_k("ip_address",
                                      k=10,
                                      where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserTopMerchants",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "top_merchants_1h": {
      "op": "top_k",
      "params": {
        "field": "merchant",
        "k": 5,
        "window": "1h",
        "exact_threshold": 1024,
        "hybrid_width": 2048,
        "hybrid_depth": 4
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `[]` (empty list), not `null`.
- **`k` < 1:** raises `RegistrationError(code="aggregation_invalid_param")` at register time.
- **Field type:** `str`, `i64`, `f64` supported. Non-hashable types fail at register time with `schema_mismatch`.
- **Ties in count:** the order among values with the same count is implementation-defined (heap-order); fraud rules should not depend on tie-breaking.
- **Hybrid promotion accuracy:** in CMS mode, count estimates are upper bounds with `1 - exp(-hybrid_depth)` confidence and `e / hybrid_width` overestimate. Default `(2048, 4)` ⇒ ~98.2% confidence, ~0.13% overestimate per item.
- **Lifetime mode (`window=None`):** explicitly allowed — `BoundedByConfig("k", 10)` declares the per-entity ceiling at register time per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). The heavy-hitters heap is bounded by `k`; the CMS is bounded by `hybrid_width × hybrid_depth`.
- **NaN inputs:** treated as a single distinct value (NaN equals itself in the hasher); for cleaner semantics filter with `where=~bv.col("field").isnull()`.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 2 exact / Tier 3 hybrid)
- [bv.n_unique](./n_unique.md) — cardinality companion
- [bv.entropy](./entropy.md) — distribution-shape companion (same `BoundedByConfig` pattern)
- [bv.quantile](./quantile.md) — order-statistics companion
- [bv.event_type_mix](../buffer-geo/event_type_mix.md) — proportion-per-category companion
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
