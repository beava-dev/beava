# bv.count

> Event count over a window or lifetime.

## Signature

```python
bv.count(
    *,
    window: str | None = None,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.count` returns the integer number of events that match the optional `where=`
predicate within the rolling `window`. When `window=None` (the default) the
operator runs in **lifetime mode** — the count never reclaims old events and
grows monotonically across the entity's history.

It is the simplest aggregation in the catalogue and the workhorse of fraud /
ad-tech velocity rules: "how many login attempts in the last 5 minutes?",
"how many ad impressions today?", "how many failed payments this hour?". Use
`bv.count(window="5m", where=bv.col("status") == "failed")` to express the
last example.

`bv.count` belongs to the **core** family. Per-event update is a single integer
increment; memory per entity is `O(1)` regardless of stream length. There is
no `field` argument — if you want a sum of a numeric field instead of a row
count, reach for [`bv.sum`](./sum.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `window` | `str` | No | `None` (lifetime) | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. Examples: `"5m"`, `"1h"`, `"30s"`, `"100ms"`, `"7d"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events are counted. |

## Returns

A single `i64`. When the entity has seen zero matching events, the result is
`0` (not `null` — `count` returns the integer zero on cold-start).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~5 ns algorithm floor / ~25 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — single counter (plus bucket array if `window` is set, capped at 64 buckets) |
| Lifetime mode (`window=None`) | **Allowed** — `O(1)` footprint per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Lifetime event count per user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    status: str

@bv.table(key="user_id")
def UserLoginStats(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(total_logins=bv.count())
    )

# Push events
app.push("Login", {"user_id": "alice", "status": "ok"})
app.push("Login", {"user_id": "alice", "status": "ok"})
app.push("Login", {"user_id": "alice", "status": "failed"})

# Query
result = app.get("UserLoginStats", "alice")
# result == {"total_logins": 3}
```

### Example 2: Failed-login velocity in the last 5 minutes

```python
@bv.table(key="user_id")
def UserLoginVelocity(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(failed_5m=bv.count(window="5m",
                                       where=bv.col("status") == "failed"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserLoginStats",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "total_logins": {
      "op": "count",
      "params": {}
    },
    "failed_5m": {
      "op": "count",
      "params": {
        "window": "5m",
        "where": "status == 'failed'"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `0` (integer), not `null`. `bv.count` is the only core op that returns 0 on cold-start; numeric ops (`sum`/`mean`/`min`/`max`/`var`/`std`) return `null`.
- **`where=` filter excludes everything:** result is `0`.
- **Lifetime mode (`window=None`):** explicitly **allowed** — `count` is `O(1)` per entity and carries no per-event allocation.
- **Malformed window string:** raises `ValueError` at SDK-helper-call time (must match `\d+(ms|s|m|h|d)` or `"forever"`; leading zero rejected).
- **Counts wrap on i64 overflow:** entities pushing more than `2^63 - 1` matching events will overflow. Not observed in practice.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.sum](./sum.md) — sum a numeric field instead of counting rows
- [bv.ratio](./ratio.md) — count(where=p) / count(total) — the matching-rate companion
- [bv.histogram](../buffer-geo/histogram.md) — count per fixed bucket
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
