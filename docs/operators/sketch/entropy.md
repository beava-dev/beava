# bv.entropy

> Shannon entropy over a categorical-distribution field.

## Signature

```python
bv.entropy(
    field: str,
    *,
    window: str | None = None,
    where: bv.Col | None = None,
    max_categories: int = 256,
) -> AggDescriptor
```

## Description

`bv.entropy` returns the Shannon entropy (log₂ base) of the categorical
distribution of a field across events that match the optional `where=`
predicate. State is a per-category frequency table capped at `max_categories`
distinct keys; once full, the cap-and-drop policy keeps the most-frequent
categories and discards the tail (v0-06 D-05a).

Entropy ranges from `0.0` (degenerate distribution — every event has the
same value) to `log₂(K)` for K equally-likely categories. It quantifies how
diverse / unpredictable the field's values are for this entity. Use
`bv.entropy("merchant", window="24h")` for "how diverse is this user's
merchant mix today?" or `bv.entropy("user_agent")` for "how varied are the
client UA strings ever seen for this account?".

`bv.entropy` belongs to the **sketch** family. `BoundedByConfig("max_categories", 256)`
per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — the
per-entity memory ceiling is declared at register time via the
`max_categories` kwarg. Per-event update is a BTreeMap key insert; Tier 3
cost (~60 ns floor / ~160 ns measured) — string-key allocation is the
irreducible per-event cost.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the categorical field to compute entropy over. Any hashable type (`str`, `i64`, `f64`). |
| `window` | `str` | No | `None` (lifetime) | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events contribute. |
| `max_categories` | `int` | No | `256` | Cap on distinct categories retained per entity. Memory bounded by `O(max_categories)` BTreeMap entries. v0 `BoundedByConfig` ceiling. |

## Returns

A single `f64` in `[0, log₂(max_categories)]`. When the entity has seen zero
matching events, the result is `null` (Python `None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 3** (~60 ns algorithm floor / ~160 ns measured — BTreeMap key insert + cap-and-drop) — see [cost-class.md](../cost-class.md#tier-3-algorithmic-floor-100-300-nscall--9-ops) |
| Memory per entity | `BoundedByConfig("max_categories", 256)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — BTreeMap of size ≤ `max_categories` |
| Lifetime mode (`window=None`) | **Allowed** — `BoundedByConfig` declares the per-entity ceiling at register time |

## Examples

### Example 1: Per-user merchant diversity, daily

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    merchant: str
    amount: float

@bv.table(key="user_id")
def UserMerchantDiversity(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(merchant_entropy_24h=bv.entropy("merchant", window="24h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "merchant": "amazon", "amount": 50.0})
app.push("Txn", {"user_id": "alice", "merchant": "amazon", "amount": 20.0})
app.push("Txn", {"user_id": "alice", "merchant": "starbucks", "amount": 5.0})
app.push("Txn", {"user_id": "alice", "merchant": "uber", "amount": 12.0})

# Query
result = app.get("UserMerchantDiversity", "alice")
# result == {"merchant_entropy_24h": ~1.5} # 2/4 amazon, 1/4 starbucks, 1/4 uber
```

### Example 2: Lifetime user-agent entropy with a tighter cap

```python
@bv.table(key="account_id")
def UaDiversity(reqs) -> bv.Table:
    return (
        reqs.group_by("account_id")
            .agg(ua_entropy=bv.entropy("user_agent", max_categories=64))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserMerchantDiversity",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "merchant_entropy_24h": {
      "op": "entropy",
      "params": {
        "field": "merchant",
        "window": "24h",
        "max_categories": 256
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null` — no events ⇒ no entropy defined.
- **Single category (degenerate distribution):** result is `0.0` — perfectly predictable.
- **`max_categories` exceeded:** cap-and-drop policy keeps the most-frequent categories; the entropy estimate is biased low (concentrates probability mass into the retained categories). For high-cardinality fields, raise `max_categories` cautiously — memory grows linearly.
- **Field type:** `str`, `i64`, `f64` all hashable. Non-hashable types fail at register time with `schema_mismatch`.
- **NaN inputs:** treated as a single distinct category (NaN equals itself in the BTreeMap key); for cleaner semantics filter with `where=~bv.col("field").isnull()`.
- **`max_categories` set to 0:** rejected at register time with `aggregation_invalid_param`.
- **Lifetime mode (`window=None`):** explicitly allowed — `BoundedByConfig("max_categories", 256)` declares the per-entity ceiling at register time per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).
- **Quadkey-for-geo recipe:** the recommended replacement for the deleted `bv.geo_entropy` op is `bv.entropy(quadkey(lat, lon, zoom), max_categories=1024)` — the `quadkey(...)` expression at apply time produces a deterministic integer cell id for `entropy` to bin.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 3)
- [bv.top_k](./top_k.md) — heavy-hitters companion (same `BoundedByConfig` pattern)
- [bv.n_unique](./n_unique.md) — cardinality companion
- [bv.event_type_mix](../buffer-geo/event_type_mix.md) — proportion-per-category companion (also `BoundedByConfig`)
- [bv.quantile](./quantile.md) — order-statistics companion
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
