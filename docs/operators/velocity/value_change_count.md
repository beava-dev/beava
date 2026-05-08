# bv.value_change_count

> Count of consecutive value flips of a numeric field — "how many times did this value change?".

## Signature

```python
bv.value_change_count(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.value_change_count` returns the number of times the `field` value
changed between consecutive matching events. On every matching event the
helper compares the new value to the cached `last_value`; if they differ,
it increments the counter and updates `last_value`. If they match, the
counter is unchanged. Read it as "how many times did this user's
shipping country change?", "how many distinct device fingerprints in a
row?", or "how many merchant-category flips on this card?". Note that
this is a **flip count**, not a distinct-value count — a sequence
`A, B, A, B` produces 3 flips, while it has only 2 distinct values.

This is the canonical "instability" or "value-churn" primitive — useful
for any signal where stability matters more than the absolute value
(geographic country flips suggesting account takeover, device-id flips
suggesting credential sharing, merchant-category churn suggesting card
testing). Compared to [`bv.n_unique`](../sketch/n_unique.md) (HLL
cardinality estimate of distinct values, also achievable on
non-numeric fields), `value_change_count` is exact and counts **adjacent
flips** rather than total distinct values — much cheaper and more
specific to the "is this entity bouncing around?" question.
[`bv.delta_from_prev`](./delta_from_prev.md) measures the **magnitude**
of the latest flip; this op counts **how many** flips happened.

`bv.value_change_count` belongs to the **velocity** family. The state is
two `f64` slots plus a counter; per-event update is one numeric extract
plus one float compare; cost is **Tier 1** (~8 ns floor / ~28 ns
measured) and memory is `O(1)` per entity. The `window=` kwarg is
**required** by the Python SDK helper; the inner state is itself
lifetime-bound `O(1)`.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Numeric field (`i64` or `f64`) to track. Non-numeric values are silently skipped (no flip counted, no `last_value` update). |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the flip counter. |

## Returns

A single `i64` — the cumulative flip count. Cold-start (no matching events seen) returns `0`, never `null`. The first matching event seeds `last_value` but does not count as a flip.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns floor / ~28 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `ValueChangeCountState` ≈ 24 B (`last_value: f64`, `changes: u64`, `initialized: bool`) |
| Lifetime mode (`window="forever"`) | **Allowed** — classified `O1` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Country flips per user, 24h window

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    country_code: int # ISO 3166 numeric code, e.g. 840 for US

@bv.table(key="user_id")
def UserCountryFlips(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(country_flips_24h=bv.value_change_count(
                       "country_code",
                       window="24h"))
    )

# Push events
app.push("Login", {"user_id": "alice", "country_code": 840}) # flips = 0 (first event)
app.push("Login", {"user_id": "alice", "country_code": 840}) # flips = 0 (same)
app.push("Login", {"user_id": "alice", "country_code": 124}) # flips = 1 (US → CA)
app.push("Login", {"user_id": "alice", "country_code": 826}) # flips = 2 (CA → UK)
app.push("Login", {"user_id": "alice", "country_code": 826}) # flips = 2 (same)

# Query
result = app.get("UserCountryFlips", "alice")
# result == {"country_flips_24h": 2}
```

### Example 2: Filtered merchant-category churn per card

```python
@bv.table(key="card_id")
def CardCategoryChurn(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(category_flips=bv.value_change_count(
                     "merchant_category_code",
                     window="1h",
                     where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserCountryFlips",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "country_flips_24h": {
      "op": "value_change_count",
      "params": {
        "field": "country_code",
        "window": "24h"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `0` (integer), not `null`. Counter starts at `0`.
- **Single-event entity:** result is `0`. The first matching event seeds `last_value` but is not itself a flip.
- **Sequence `A, B, A`:** counter is `2` (A→B is one flip; B→A is another). The op counts **adjacent** flips, not net distinct values.
- **Sequence `A, A, A, ...`:** counter stays at `0`. Repeated identical values do not flip.
- **Float comparison precision:** `last_value != x` is an **exact** float compare; values that look identical in print but differ at the ULP level (e.g. summed `0.1 + 0.2 == 0.3` returning false) will register as flips. For categorical signals, prefer integer encodings (e.g. ISO numeric country codes rather than string names — strings are not yet supported as the field type).
- **Non-numeric field:** the event is silently skipped (no flip counted, no `last_value` update). v0 supports only `i64` and `f64` fields; categorical strings are encoded as integers upstream by the producer.
- **`where=` filter excludes the event:** no update; the next matching event diffs against the previous **matching** event's value, not the previous event in arrival order overall.
- **Missing `window=`:** raises `ValueError` at SDK-helper-call time.
- **Malformed `window=`:** raises `ValueError` at SDK-helper-call time; if it somehow reaches the server, `register_validate.rs` returns structured error `aggregation_invalid_window`.
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the underlying state per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md); the next post-eviction matching event reseeds `last_value` and resets `changes` to 0.

## See also

- [Velocity family index](./index.md) — overview of all 9 velocity-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.delta_from_prev](./delta_from_prev.md) — magnitude of the latest flip rather than a count of flips
- [bv.n_unique](../sketch/n_unique.md) — HLL distinct-value count (counts unique values, not adjacent flips); pick when you care about cardinality rather than churn
- [bv.streak](../recency/streak.md) — consecutive **matches**; the inverse perspective on stability
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
