# bv.sum

> Sum of a numeric field over a window.

## Signature

```python
bv.sum(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.sum` accumulates the running total of a numeric field across events that
match the optional `where=` predicate. Both `field` and `window` are
**required** — `bv.sum` is a windowed aggregation by design (use `bv.count`
for unbounded counting or `window="forever"` for explicit lifetime sum).

The most common use case is monetary or quantity totals: "purchase amount in
the last 24h", "ad spend this hour", "tokens consumed this minute". Works on
any field declared as `i64` or `f64` in the upstream `@bv.event` schema —
non-numeric fields are rejected at register time with `schema_mismatch`.

`bv.sum` belongs to the **core** family. Per-event update is two arithmetic
ops (running total and observation count), and memory per entity is `O(1)`
plus the window's bucket array (capped at 64 buckets).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the numeric field (`i64` or `f64`) declared on the upstream `@bv.event` schema. |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. Examples: `"1h"`, `"30s"`, `"7d"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events contribute to the sum. |

## Returns

A single `f64` (or `i64` if the field is integer-typed and no overflow occurs).
When the entity has seen zero matching events, the result is `null`
(returned as Python `None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns algorithm floor / ~25 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — running total + count + bucket array (≤64 buckets) |
| Lifetime mode (`window="forever"`) | **Allowed** — `O(1)` footprint per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Total purchase amount per user, hourly

```python
import beava as bv

@bv.event
class Purchase:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserSpend(purchases) -> bv.Table:
    return (
        purchases.group_by("user_id")
                 .agg(spend_1h=bv.sum("amount", window="1h"))
    )

# Push events
app.push("Purchase", {"user_id": "alice", "amount": 42.50})
app.push("Purchase", {"user_id": "alice", "amount": 17.00})

# Query
result = app.get("UserSpend", "alice")
# result == {"spend_1h": 59.5}
```

### Example 2: Refund-amount sum filtered to successful refunds

```python
@bv.table(key="user_id")
def RefundTotals(refunds) -> bv.Table:
    return (
        refunds.group_by("user_id")
               .agg(refunded_24h=bv.sum("amount",
                                          window="24h",
                                          where=bv.col("status") == "completed"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserSpend",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "spend_1h": {
      "op": "sum",
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

- **Empty stream / cold-start:** result is `null` (Python `None`).
- **Non-numeric field:** rejected at register time with structured error code `schema_mismatch`.
- **Missing `window=`:** `ValueError` at SDK-helper-call time. Use `window="forever"` for an explicit unbounded sum.
- **NaN inputs:** propagate per IEEE-754 — a single NaN value pollutes the running total. Filter with `where=~bv.col("amount").isnull()` if your source can emit NaN.
- **Boolean-sum trick FORBIDDEN inline:** `bv.sum(bv.col("is_fraud").cast(int))` is **not supported in v0** — `bv.sum` takes a literal field name (`str`), not an expression. Compile the boolean → integer cast as a stateless `with_columns` step first, then sum the resulting field. (Documented in [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) when that page lands.)
- **Lifetime mode (`window="forever"`):** explicitly allowed; sum is `O(1)` per entity with no observable memory growth.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.mean](./mean.md) — same state, returns the average instead of the total
- [bv.count](./count.md) — count of matching events instead of summed value
- [bv.decayed_sum](../decay/decayed_sum.md) — exponentially-decayed forward-decay sum (Cormode)
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — boolean-sum two-stage pattern
