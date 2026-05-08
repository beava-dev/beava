# bv.n_unique

> Approximate distinct-value count, backed by HyperLogLog.

## Signature

```python
bv.n_unique(
    field: str,
    *,
    window: str | None = None,
    where: bv.Col | None = None,
    exact_threshold: int = 1024,
    hybrid_precision: int = 14,
) -> AggDescriptor
```

> Previously called `bv.count_distinct`. Renamed to `n_unique` per [ADR-002](../../../.planning/decisions/ADR-002-polars-op-rename.md) for Polars-convention consistency. The old name remains as a deprecation alias in v0.0.x and is removed in v0.1.

## Description

`bv.n_unique` estimates the number of distinct values of a field across events
that match the optional `where=` predicate. Backed by a hybrid exact-then-HLL
state: while the entity has fewer than `exact_threshold` distinct values, the
state holds them in a hash set and returns the precise cardinality. Once the
threshold is crossed, the state promotes to a HyperLogLog sketch with
precision `hybrid_precision` (default 14 ⇒ ~16 KB per entity, ~1.6%
relative-error floor at HLL threshold).

Use `bv.n_unique("merchant", window="24h")` for "how many distinct merchants
did this user interact with today?" or
`bv.n_unique("device_id", window="forever", where=bv.col("status") == "ok")`
for "how many devices has this account ever logged in from successfully?".
The hybrid mode is transparent at the API: callers always read a single
integer.

`bv.n_unique` belongs to the **sketch** family and is `BoundedSketch` per
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — fixed
structural cap regardless of stream length.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the field whose distinct-value count to estimate. Any hashable type (`str`, `i64`, `f64`). |
| `window` | `str` | No | `None` (lifetime) | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events contribute. |
| `exact_threshold` | `int` | No | `1024` | Distinct-value count below which exact (hashset) mode is used. |
| `hybrid_precision` | `int` | No | `14` | HLL precision parameter once promoted; bytes per entity ≈ `2^precision` × 1 byte (~16 KB at 14). |

## Returns

A single `i64`. When the entity has seen zero matching events, the result is
`0` (not `null` — distinct-count of an empty set is the integer zero).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 2** (Exact mode, ~18 ns floor / ~80 ns post-wrapping-fix) — see [cost-class.md](../cost-class.md#tier-2-moderate-30-100-nscall--6-ops) |
| | **Tier 3** (HLL mode, post-promotion) |
| Memory per entity | `BoundedSketch` — exact hashset up to `exact_threshold` entries, then HLL fixed at `2^precision` registers (~16 KB at precision=14) |
| Lifetime mode (`window=None`) | **Allowed** — `BoundedSketch` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Distinct merchants per user, daily

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    merchant: str
    amount: float

@bv.table(key="user_id")
def UserMerchantStats(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(unique_merchants_24h=bv.n_unique("merchant", window="24h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "merchant": "amazon", "amount": 50.0})
app.push("Txn", {"user_id": "alice", "merchant": "starbucks", "amount": 5.0})
app.push("Txn", {"user_id": "alice", "merchant": "amazon", "amount": 30.0})

# Query
result = app.get("UserMerchantStats", "alice")
# result == {"unique_merchants_24h": 2}
```

### Example 2: Distinct successful login devices over the entity's lifetime

```python
@bv.table(key="user_id")
def DeviceFootprint(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(unique_devices=bv.n_unique("device_id",
                                                where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserMerchantStats",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "unique_merchants_24h": {
      "op": "n_unique",
      "params": {
        "field": "merchant",
        "window": "24h",
        "exact_threshold": 1024,
        "hybrid_precision": 14
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full pipeline example (uses `n_unique` for `tx_unique_merchants_1h`).

## Edge cases

- **Empty stream / cold-start:** result is `0` (integer), not `null`.
- **`exact_threshold` set to 0:** forces always-HLL mode; useful for explicit memory tuning when you know the cardinality will be high. Tier 3 floor applies from the first event.
- **Field type:** `str`, `i64`, `f64` are all supported (hashable). Non-hashable types fail at register time with `schema_mismatch`.
- **NaN inputs:** treated as a single distinct value (NaN equals itself in the HLL hasher); for cleaner semantics filter with `where=~bv.col("field").isnull()`.
- **Lifetime mode (`window=None`):** explicitly allowed — HLL is `BoundedSketch` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).
- **Hybrid promotion:** transparent — caller only sees the integer estimate. Promotion happens once the entity has seen `exact_threshold` distinct values; in exact mode the result is precise, in HLL mode the standard error is ~1.6% at precision=14.
- **Combining with quadkey for geo:** the recommended replacement for the deleted `bv.unique_cells` op is `bv.n_unique(quadkey(lat, lon, zoom))`. The `quadkey(...)` expression at apply time produces a deterministic integer cell id for `n_unique` to count.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 2 exact / Tier 3 HLL)
- [bv.quantile](./quantile.md) — quantile sibling (also hybrid exact-then-sketch)
- [bv.bloom_member](./bloom_member.md) — set-membership companion (BoundedSketch)
- [bv.top_k](./top_k.md) — heavy-hitters companion
- [bv.entropy](./entropy.md) — distribution-shape companion
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
