# bv.var

> Sample variance via Welford's online algorithm.

## Signature

```python
bv.var(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

> Previously called `bv.variance`. Renamed to `var` per [ADR-002](../../../.planning/decisions/ADR-002-polars-op-rename.md) for Polars-convention consistency. The old name remains as a deprecation alias in v0.0.x and is removed in v0.1.

## Description

`bv.var` returns the **sample variance** (Bessel-corrected, divisor `n-1`) of
a numeric field across events that match the optional `where=` predicate.
State is updated via Welford's online algorithm — numerically stable across
many orders of magnitude and across long-running streams. Per-entity state is
`(count, mean, M2)` (three `f64` slots) regardless of stream length.

Use `bv.var("amount", window="24h")` for "amount-variance over the last day"
or pair with [`bv.std`](./std.md) for the standard-deviation form. Variance is
the bedrock for outlier detection (`bv.outlier_count` uses sigma = sqrt(var))
and for entity-specific z-scores (`bv.z_score`).

`bv.var` belongs to the **core** family. Tier 1 cost (~12 ns floor / ~32 ns
measured — five FP ops per update). Both `field` and `window` are required;
the field must be `i64` or `f64` (rejected at register time otherwise).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the numeric field (`i64` or `f64`). |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events contribute. |

## Returns

A single `f64`. When the entity has seen fewer than two matching events, the
result is `null` (Python `None`) — sample variance is undefined for n<2.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~12 ns algorithm floor / ~32 ns measured — Welford 5-FP-op step) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `(count, mean, M2)` per bucket (≤64 buckets) |
| Lifetime mode (`window="forever"`) | **Allowed** — `O(1)` footprint per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Per-user transaction-amount variance, hourly

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
            .agg(amount_var_1h=bv.var("amount", window="1h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "amount": 10.0})
app.push("Txn", {"user_id": "alice", "amount": 30.0})
app.push("Txn", {"user_id": "alice", "amount": 50.0})

# Query
result = app.get("TxnSpread", "alice")
# result == {"amount_var_1h": 400.0} # sample variance: ((10-30)^2 + (30-30)^2 + (50-30)^2) / 2
```

### Example 2: Latency-variance for successful payments only

```python
@bv.table(key="user_id")
def LatencyDispersion(payments) -> bv.Table:
    return (
        payments.group_by("user_id")
                .agg(latency_var_ok=bv.var("latency_ms",
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
    "amount_var_1h": {
      "op": "var",
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

- **Empty stream / cold-start:** result is `null` — no matching events ⇒ no variance.
- **Single matching event (n=1):** result is `null` — sample variance requires at least two observations (Bessel correction divides by `n-1`).
- **Non-numeric field:** rejected at register time with `schema_mismatch`.
- **NaN inputs:** propagate per IEEE-754 — a single NaN poisons the Welford `M2` term. Filter with `where=~bv.col("amount").isnull()` if your source can emit NaN.
- **Missing `window=`:** `ValueError` at SDK-helper-call time. Use `window="forever"` for explicit lifetime variance.
- **Lifetime mode (`window="forever"`):** explicitly allowed — `O(1)` per entity. Welford is numerically stable across millions of events.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.std](./std.md) — `sqrt(var)` companion (same state)
- [bv.mean](./mean.md) — first-moment companion
- [bv.ewvar](../decay/ewvar.md) — exponentially-weighted variance (decay-based, not window-based)
- [bv.outlier_count](../velocity/outlier_count.md) — counts events beyond N-sigma using the running variance
- [bv.z_score](../velocity/z_score.md) — entity-level z-score using running mean / variance
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
