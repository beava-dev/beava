# bv.std

> Standard deviation — sqrt of sample variance.

## Signature

```python
bv.std(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

> Previously called `bv.stddev`. Renamed to `std` per [ADR-002](../../../.planning/decisions/ADR-002-polars-op-rename.md) for Polars-convention consistency. The old name remains as a deprecation alias in v0.0.x and is removed in v0.1.

## Description

`bv.std` returns the **sample standard deviation** of a numeric field — the
square root of the sample variance ([`bv.var`](./var.md)). State is shared
with `bv.var`: a single `(count, mean, M2)` Welford accumulator per bucket,
and the `sqrt` is deferred to query time. Per-event cost is identical to
`bv.var` because no extra work happens on apply.

Use `bv.std("amount", window="24h")` for "amount-stddev over the last day"
when you want the same scale as the underlying field (variance is in squared
units; stddev is in the field's native units). It is the natural normalization
input for any sigma-based threshold rule ("alert when value > mean + 3·std").

`bv.std` belongs to the **core** family. Tier 1 cost (~12 ns floor / ~32 ns
measured). Both `field` and `window` are required; the field must be `i64` or
`f64` (rejected at register time otherwise).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the numeric field (`i64` or `f64`). |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events contribute. |

## Returns

A single `f64`. When the entity has seen fewer than two matching events, the
result is `null` (Python `None`) — sample stddev is undefined for n<2.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~12 ns algorithm floor / ~32 ns measured — Welford apply, `sqrt` deferred to query) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `(count, mean, M2)` per bucket (≤64 buckets) |
| Lifetime mode (`window="forever"`) | **Allowed** — `O(1)` footprint per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Per-user transaction-amount stddev, hourly

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def TxnSpread(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(amount_std_1h=bv.std("amount", window="1h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "amount": 10.0})
app.push("Txn", {"user_id": "alice", "amount": 30.0})
app.push("Txn", {"user_id": "alice", "amount": 50.0})

# Query
result = app.get("TxnSpread", "alice")
# result == {"amount_std_1h": 20.0} # sqrt of sample variance 400.0
```

### Example 2: Latency-stddev for the success-bucket

```python
@bv.table(key="user_id")
def LatencyDispersion(payments) -> bv.Table:
    return (
        payments.group_by("user_id")
                .agg(latency_std_ok=bv.std("latency_ms",
                                             window="24h",
                                             where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "TxnSpread",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "amount_std_1h": {
      "op": "std",
      "params": {
        "field": "amount",
        "window": "1h"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null` — no events ⇒ no stddev.
- **Single matching event (n=1):** result is `null` — sample stddev requires ≥2 observations.
- **Non-numeric field:** rejected at register time with `schema_mismatch`.
- **NaN inputs:** propagate per IEEE-754 — a single NaN poisons the Welford state. Filter with `where=~bv.col("amount").isnull()`.
- **Missing `window=`:** `ValueError` at SDK-helper-call time. Use `window="forever"` for explicit lifetime stddev.
- **Lifetime mode (`window="forever"`):** explicitly allowed — `O(1)` per entity.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.var](./var.md) — sample variance (shared state; `std = sqrt(var)`)
- [bv.mean](./mean.md) — first-moment companion
- [bv.ew_zscore](../decay/ew_zscore.md) — z-score against exponentially-weighted baseline
- [bv.z_score](../velocity/z_score.md) — entity-level z-score using running mean / variance
- [bv.outlier_count](../velocity/outlier_count.md) — counts events beyond N·stddev
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
