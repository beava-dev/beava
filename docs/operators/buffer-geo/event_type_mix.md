# bv.event_type_mix

> Proportion per category over the entity's lifetime, capped at `max_categories`.

## Signature

```python
bv.event_type_mix(
    field: str,
    *,
    categories: list[str] | None = None,
    max_categories: int = 256,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.event_type_mix` returns the proportion of matching events per categorical
value of `field`. State is a `BTreeMap<String, u64>` of per-category counts
plus a `total` counter; the query divides each count by `total` to produce
proportions in `[0, 1]`. Use it for "what fraction of this user's
transactions are 'p2p' vs 'card' vs 'crypto'?" or "what's the breakdown of
HTTP status codes for this endpoint?" — features where you want a
distribution shape, not just a top-K winner.

`max_categories` is **soft-defaulted to 256** as a `BoundedByConfig` cap per
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — the kwarg is OPTIONAL,
but the per-entity ceiling is always declared (the default applies if the
caller omits it). Once the cap is reached, **new categories are silently
dropped**; their events still increment `total`, matching SQL `OTHER`
collapse semantics. This means the proportions do not necessarily sum to
`1.0` once the cap is hit — they sum to `(1 − dropped/total)` for the cap.
v0 surfaces this via the per-source cap-hit metric introduced in
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md); the
metric increments whenever the cap-and-drop path runs.

You can also supply `categories=[...]` to **lock** the allowlist at register
time: events whose category falls outside the allowlist are dropped on the
counting side (they still increment `total`, like the cap-and-drop path).
A backs the allowlist with an `AHashSet` for O(1)
membership tests on the apply path; the serde-stable `Vec<String>` is kept
for snapshot back-compat. Allowlist-mode is the recommended pattern when you
know the category set in advance — it removes the cap-and-drop ambiguity.

`bv.event_type_mix` belongs to the **bounded-buffer** family. Per-event
update is Tier 3 (~70 ns floor / ~150 ns measured per
[cost-class.md](../cost-class.md)) — `BTreeMap` key insert is the
irreducible cost (string-key allocation on cap-miss, `AHashSet::contains`
when an allowlist is set). v0 boxed `EventTypeMixState` so the
`AggOp::EventTypeMix` variant fits the 80-byte enum cap (the state itself
lives on the heap behind a `Box`); see
`crates/beava-core/src/agg_op.rs` line 483 and
[SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md).
There is no `window=` kwarg in v0 — `bv.event_type_mix` is **lifetime-only**.
For a "category mix in the last 24 h", compose with
`@bv.event(cold_after="24h")` per
[V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Categorical field to bin (`str`, `i64`, `bool` — coerced to string for the BTreeMap key). |
| `categories` | `list[str]` \| `None` | No | `None` | Optional allowlist of categories to track. When set, events outside the list are dropped from counts but still increment `total`. Use it when the category set is known in advance. |
| `max_categories` | `int` | No | `256` | Soft cap on distinct categories retained. Once reached, new categories are silently dropped. Per-entity memory ceiling per [V0-MEM-GOV-02 BoundedByConfig("max_categories", 256)](../../../.planning/REQUIREMENTS.md). |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the counters. |

## Returns

A `dict[str, float]` mapping each retained category to its proportion in
`[0, 1]`. Cold-start (no events) returns the empty dict `{}` — never
`null`. When the cap-and-drop or allowlist-drop path has fired, proportions
**sum to `(1 − dropped/total)`**, not to `1.0` — track the cap-hit metric to
detect this case.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 3** (~70 ns floor / ~150 ns measured — `BTreeMap` key insert + cap-or-allowlist check) — see [cost-class.md](../cost-class.md#tier-3-algorithmic-floor-100-300-nscall--9-ops). String-key allocation is the irreducible cost on the accept path |
| Memory per entity | **`BoundedByConfig("max_categories", 256)`** per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — `BTreeMap` of size ≤ `max_categories` (or ≤ `len(categories)` when allowlisted). Boxed inside `AggOp` per v0 |
| Lifetime mode | **Required** — `bv.event_type_mix` has no `window=` kwarg in v0; lifetime is the only mode |

## Examples

### Example 1: Per-user transaction-type mix

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    txn_type: str # "card" | "p2p" | "crypto" | "ach" | ...

@bv.table(key="user_id")
def UserTxnTypeMix(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(type_mix=bv.event_type_mix("txn_type"))
    )

# After 100 transactions: 60 card, 30 p2p, 10 crypto
result = app.get("UserTxnTypeMix", "alice")
# result == {"type_mix": {"card": 0.6, "crypto": 0.1, "p2p": 0.3}}
```

### Example 2: Allowlisted HTTP status mix per endpoint

```python
@bv.table(key="endpoint")
def EndpointStatusMix(reqs) -> bv.Table:
    return (
        reqs.group_by("endpoint")
            .agg(status_mix=bv.event_type_mix("status",
                                                categories=["200", "400", "500"]))
    )

# Status 301 events still increment total but are dropped from counts —
# proportions for the 3 allowlisted statuses sum to (1 − dropped/total).
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserTxnTypeMix",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "type_mix": {
      "op": "event_type_mix",
      "params": {
        "field": "txn_type",
        "max_categories": 256
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `{}` (empty dict) — never `null`.
- **`max_categories` cap reached:** new categories are dropped silently from `counts` but still increment `total`. Proportions sum to `(1 − dropped/total)`, not to `1.0`. The v0 cap-hit metric increments per drop — wire this to your alerting if the cap-hit rate is non-zero in production.
- **`categories=[...]` allowlist set:** events outside the allowlist are dropped from `counts` but still increment `total`. Identical surfacing semantics to the cap-and-drop path; the allowlist removes the ambiguity by forcing a known schema.
- **Allowlist + cap interaction:** when both `categories=` and `max_categories=` are set, the allowlist takes precedence — `len(categories)` is the effective cap (the cap-and-drop path is unreachable).
- **Non-string source field:** `i64` and `bool` are coerced to their string form (`"42"`, `"true"`); other types (`f64`, `bytes`, `null`, `list`, `map`) are silently dropped (no `total` increment either, since `str_from_row` returns `None`).
- **`max_categories=0`:** rejected at register time with `aggregation_invalid_param`.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. For a windowed mix use `@bv.event(cold_after="...")` to bound the lifetime via per-entity TTL.
- **Snapshot reload:** `allowed_set: AHashSet` is `#[serde(skip)]` for snapshot stability; it is rebuilt lazily from `allowed: Vec<String>` on the first apply post-deserialization. Adds one-time cost per state per process lifetime after a snapshot load.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); categories are counted in arrival order.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `BoundedByConfig("max_categories", 256)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 3)
- [bv.entropy](../sketch/entropy.md) — distribution-shape companion (Shannon entropy of the same categorical field; same `BoundedByConfig` pattern)
- [bv.top_k](../sketch/top_k.md) — heavy-hitters companion (returns top-K with counts; this op returns proportions across all retained categories)
- [bv.histogram](./histogram.md) — numeric-bucket companion (counts per fixed numeric range, not per categorical value)
- [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md) — cold-entity eviction (`@bv.event(cold_after=...)`) for windowing the lifetime
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — `BoundedByConfig` lifetime-aggregation contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
