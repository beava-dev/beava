# Global aggregation

> Per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md) — first-class global aggregation alongside per-entity aggregation.

By default, beava aggregations are **per-entity** — every feature is keyed by a partition column (e.g., `user_id`, `card_id`). Per-entity aggregations answer "what is feature X for this user?".

**Global aggregation** answers a different shape of question: "what is feature X across all entities?" — total throughput, current entity count, global p95 latency, top-10-globally features, anomaly detection on global rates. Global tables ship in v0 alongside the per-entity surface.

## When to use global vs per-entity

| Use case | Shape | Example |
|---|---|---|
| "Total events / sec across all sources" | Global | Operator dashboard throughput |
| "Current resident entity count" | Global | Memory governance monitor |
| "Global p95 latency" | Global | SLA tracking |
| "Top 10 hottest pages on the platform" | Global | Trending content rank |
| "Has the GLOBAL signup rate spiked?" | Global | Anomaly detection |
| "Total spend across all users in last hour" | Global | Platform health summary |
| "Count of events for THIS user" | Per-entity | Personalization feature |
| "p95 of THIS user's purchase amounts" | Per-entity | Risk scoring |
| "Velocity of THIS card in last 5 min" | Per-entity | Fraud rules |
| "Has THIS user seen page X" | Per-entity | A/B targeting |

The rule of thumb: if your downstream consumer queries "for entity Z, what is X?", use per-entity. If your consumer queries "for the platform as a whole, what is X?", use global. The two surfaces compose freely on the same source — you can have both `UserSpend` (per-entity) and `TotalSpend` (global) aggregations consuming the same `Purchase` events.

## Three equivalent forms

In Python (per [an internal plan + ADR-003](../sdk-api/python.md)), three forms compile to the same wire payload:

```python
# Form 1: shortest — direct .agg() shorthand on the source
clicks.agg(total=bv.count(window="forever"))

# Form 2: explicit empty group_by
clicks.group_by().agg(total=bv.count(window="forever"))

# Form 3: full decorator declaration
@bv.table # no key= → global
def TotalClicks(clicks) -> bv.Table:
    return clicks.agg(total=bv.count(window="forever"))
```

All three produce a register payload with `key: []` (empty array). The decorator form is the canonical "named, queryable" surface — Forms 1 and 2 are typically used inside a decorator body, not as standalone declarations.

## Querying a global table

```python
app.register(Click, TotalClicks)

# Push events for many users:
app.push("Click", {"user_id": "alice", "page": "/home"})
app.push("Click", {"user_id": "bob", "page": "/home"})
app.push("Click", {"user_id": "carol", "page": "/products"})

# Query the global feature dict — no entity arg:
app.get("TotalClicks")
# → {"total": 3}
```

Note the `App.get` arity:

- **Per-entity table:** `app.get(table_name, entity_id)` → 2 args required.
- **Global table:** `app.get(table_name)` → 1 arg required.
- **Mismatch raises `KeyError`** with a clear message indicating the table's expected arity.

## Sentinel mechanism (implementation detail)

On the wire, global state lives at the sentinel `key = ""` (empty string). The engine routes empty-string entity_id through the same per-entity hashmap machinery — no new code path, no new opcode, no new schema. The `&str` key path handles `""` natively.

This is intentionally implementation-leaking: alternatives like a dedicated `OP_GET_GLOBAL` opcode or a special header bit were rejected during ADR-003 discussion in favor of the simpler "global is just another entity with key = ''" model. The trade-off: empty-string sentinel composes cleanly with `OP_BATCH_GET` (heterogeneous batches can mix per-entity and global lookups by entity_id alone).

## Performance characteristics

Global tables hold a **single state slot** per registered table — bounded by the per-table state size, independent of entity count. By contrast, per-entity tables hold one state slot per active entity (bounded by entity count × per-entity state size, subject to memory governance per [V0-MEM-GOV-01..03](../architecture/memory-budget.md)).

Concrete sizing (post-Phase-12.9 boxing):

| Table type | State size per slot | Total state |
|---|---|---|
| Per-entity (10 ops × 100M users) | ~80 B × 10 = ~800 B | ~80 GB |
| Global (10 ops × 1 slot) | ~80 B × 10 = ~800 B | ~800 B |

Global tables are essentially free — a fraud-team-shape pipeline with 110 features per entity adds maybe a few KB of global-table state if every derivation is duplicated as a global counterpart. The memory cost is dominated by the per-entity dimension.

## Composition with `cold_after=`

`cold_after=` is the per-source TTL for entity eviction (see [V0-MEM-GOV-01](../architecture/memory-budget.md)). It applies to per-entity state — when an entity hasn't received an event for `cold_after_ms`, the engine reclaims its state.

**Global tables are NOT subject to `cold_after=` eviction.** The global state slot is always live (it's literally always one entity, with key = ""). Setting `cold_after=` on the source `@bv.event` decorator only affects per-entity tables consuming that source — global tables consuming the same source keep accumulating into their single slot indefinitely.

This is intentional: global tables are typically used for monitoring / dashboards / anomaly detection where you want the running aggregate to never reset. If you need a windowed global aggregate (e.g., "total clicks in the last hour"), use the windowed form: `clicks.agg(total=bv.count(window="1h"))` — bucket reclaim per [V0-MEM-GOV-03](../architecture/memory-budget.md) handles the eviction within the global slot.

If you need both — per-entity TTL eviction for personalization features AND global running aggregate — declare two derivations on the same source:

```python
@bv.event(cold_after="7d") # per-entity TTL applies to UserSpend below
class Purchase:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserSpend(p) -> bv.Table: # cold_after evicts inactive users
    return p.group_by("user_id").agg(spend=bv.sum("amount", window="1h"))

@bv.table # global — cold_after has no effect; state is always live
def TotalSpend(p) -> bv.Table:
    return p.agg(spend=bv.sum("amount", window="1h"))
```

## Memory governance for unbounded global aggregates

`bv.count(window="forever")` on a global table accumulates monotonically — the count never resets. This is fine for monitoring but unbounded for `BoundedSketch` ops (HLL, DDSketch, Bloom) and `BoundedByConfig` ops (top_k, event_type_mix, entropy):

- **`BoundedSketch` ops** have a fixed-size internal state regardless of input cardinality (HLL ~12 KB; DDSketch grows logarithmically). Safe.
- **`BoundedByConfig` ops** have explicit caps (top_k k=10 → 10 slots; event_type_mix max_categories=256 → 256 slots). Safe.
- **`BoundedByRequiredKwarg` ops** require user-supplied size (first_n requires n=, etc.). Safe at register-time.
- **`O1` ops** (count, sum, mean, etc.) have constant per-slot cost. Safe.

Per [V0-MEM-GOV-02](../architecture/memory-budget.md), every lifetime aggregation operator declares a finite per-entity memory ceiling at register-time. The same enforcement applies to global tables — the single global state slot is bounded by the same op-class memory contract.

## Forward link

For the full design rationale, alternatives considered, and implementation deferral plan, see [ADR-003: First-class global aggregation + public `bv.lit` export](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md).

The acceptance test suite for global aggregation lives at [`python/tests/v0/test_global.py`](../../python/tests/v0/test_global.py) (an internal plan, 8 tests gated by `_engine_available()` SKIP until v0 + v0 land the implementation).
