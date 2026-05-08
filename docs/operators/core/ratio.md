# bv.ratio

> Count matching predicate divided by total count.

## Signature

```python
bv.ratio(
    *,
    window: str | None = None,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.ratio` returns `count(where=p) / count(total)` — the proportion of events
in the window (or lifetime) that satisfy the optional `where=` predicate.
State is `(matching, total)` per bucket; the division happens at query time.
The result lies in `[0, 1]`. When the entity has seen zero total events the
result is `null`.

This is the canonical "match rate" operator: "fraction of failed logins in the
last 5 minutes", "click-through rate this hour", "fraction of payments that
settled today". It's also the easiest way to express boolean-rate features
without resorting to the boolean-sum two-stage pattern (see
[`bv.sum` Edge cases](./sum.md#edge-cases)) — `bv.ratio(where=bv.col("is_fraud"))`
gives the fraud rate directly.

`bv.ratio` belongs to the **core** family. `window` is **optional** — if
omitted, the operator runs in lifetime mode (cumulative ratio over the entity's
entire history). Unlike `bv.sum/mean/min/max/var/std`, no `field` argument is
needed.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `window` | `str` | No | `None` (lifetime) | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; the numerator filter. With `where=None`, the ratio is always `1.0` once any event arrives (matching == total). |

## Returns

A single `f64` in `[0, 1]`. When the entity has seen zero events (matching
or otherwise), the result is `null` (Python `None`) — no division-by-zero.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~5 ns algorithm floor / ~25 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `(matching, total)` per bucket (≤64 buckets) |
| Lifetime mode (`window=None`) | **Allowed** — `O(1)` footprint per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Lifetime failed-login rate per user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    status: str

@bv.table(key="user_id")
def UserFailRate(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(fail_rate_lifetime=bv.ratio(where=bv.col("status") == "failed"))
    )

# Push events
app.push("Login", {"user_id": "alice", "status": "ok"})
app.push("Login", {"user_id": "alice", "status": "failed"})
app.push("Login", {"user_id": "alice", "status": "failed"})

# Query
result = app.get("UserFailRate", "alice")
# result == {"fail_rate_lifetime": 0.6666666666666666} # 2 / 3
```

### Example 2: Click-through rate this hour

```python
@bv.table(key="user_id")
def CtrHourly(impressions) -> bv.Table:
    return (
        impressions.group_by("user_id")
                   .agg(ctr_1h=bv.ratio(window="1h",
                                          where=bv.col("event_type") == "click"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserFailRate",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "fail_rate_lifetime": {
      "op": "ratio",
      "params": {
        "where": "status == 'failed'"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null` — no events ⇒ no ratio defined.
- **`where=None` (no predicate):** the matching count equals the total count, so the ratio is always `1.0` once any event arrives. Useful only as a sanity check.
- **`where=` filter excludes everything:** result is `0.0`.
- **Lifetime mode (`window=None`):** explicitly **allowed** — `O(1)` per entity. Cumulative match-rate across the entity's entire history.
- **Malformed window string:** raises `ValueError` at SDK-helper-call time.
- **Numeric precision:** the division is `f64`; for ratios over very-long-lived entities the relative precision floor is ~`2^-53`. Not observed in practice.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.count](./count.md) — the numerator/denominator that `ratio` divides
- [bv.sum](./sum.md) — for amount-weighted ratios use `sum(amount, where=p) / sum(amount)` (compose two aggregations and divide at the application layer)
- [bv.mean](./mean.md) — for boolean-fraction with explicit predicate, the boolean-sum two-stage pattern via `bv.mean` is an alternative — but `bv.ratio` is the recommended primitive
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
