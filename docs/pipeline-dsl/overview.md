# Pipeline DSL Overview

> **Status:** Authoritative for v0. Documents the **post-13.5 target** Python
> pipeline-DSL surface. The current `python/beava/` predates the v0 launch
> design session — Phase 13.5 implements the rewrite. This doc is the spec
> the rewrite targets.
> **Last reviewed:** 2026-05-03 (Phase 13.0).

## What pipelines are

A beava **pipeline** is a small Python program that:

1. Declares one or more **event sources** with `@bv.event`.
2. Declares one or more **aggregation outputs** with `@bv.table` (per
   [ADR-001](../../.planning/decisions/ADR-001-bv-table-partial-overturn.md)).
3. Optionally declares **derived events** (filter / select / with_columns /
   ... chains on existing event sources) — these are also `@bv.event`
   function-form decorators.
4. Hands the descriptors to `app.register(...)`. The SDK serialises them to
   JSON, the server validates the DAG, persists the registry, and bumps
   `registry_version`.

After register, the pipeline is **live** — every `app.push("EventName", {...})`
flows through the registered chain and updates per-entity state in memory.
`app.get("TableName", "key")` returns the row-shape (a flat dict of feature →
value) computed from those events.

The pipeline is **declarative** — you describe what features you want, not
how to compute them. The SDK compiles the chain to a JSON wire payload (see
[compilation-rules.md](compilation-rules.md)); the server's apply loop runs
each registered op atomically per event with no further user code involved.

## Hello world

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserTxnFeatures(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(
               tx_count_1h=bv.count(window="1h"),
               tx_sum_1h=bv.sum("amount", window="1h"),
               tx_mean_1h=bv.mean("amount", window="1h"),
           )
    )

with bv.App() as app:                    # embed mode — spawns the binary locally
    app.register(Txn, UserTxnFeatures)
    app.push("Txn", {"user_id": "alice", "amount": 12.50})
    app.push("Txn", {"user_id": "alice", "amount": 30.00})
    print(app.get("UserTxnFeatures", "alice"))
    # {'tx_count_1h': 2, 'tx_sum_1h': 42.5, 'tx_mean_1h': 21.25}
```

That is the entire surface for a real-world feature. The rest of this doc
walks the pieces in detail.

## `@bv.event` decorator

The `@bv.event` decorator declares an **event source** (an immutable
append-only stream of events with a typed schema) or a **derived event** (a
chain of stateless ops on top of an existing source).

### Class form (event source)

The class form declares a brand-new event source. Each annotated field
becomes a typed schema field; fields with `bv.Optional[T]` are nullable.

```python
@bv.event
class Txn:
    user_id: str
    card_id: str
    amount: float
    merchant: str
    ip: str
```

You may parameterise the decorator with retention / dedupe knobs:

```python
@bv.event(
    keep_events_for="30d",     # event-history retention for replay (optional)
    dedupe_key="txn_id",       # idempotent re-pushes within dedupe_window
    dedupe_window="24h",
    cold_after="7d",           # cold-entity TTL (Phase 12.8 D-01)
)
class Txn:
    txn_id: str
    user_id: str
    amount: float
```

Field types come from the [shared.md § Field types](../sdk-api/shared.md#field-types)
vocabulary: `str`, `i64` (Python `int`), `f64` (Python `float`), `bool`,
`bytes`, `datetime`. `event_time` fields are **rejected at decoration time**
per `project_redis_shaped_no_event_time_ever` — beava is processing-time only;
the server stamps wall-clock arrival time on every push.

### Function form (derived event)

The function form takes a single upstream event source as a parameter-annotated
argument and returns the result of a stateless op chain. The returned object IS
a new derived event you can push downstream:

```python
@bv.event
def BigTxn(txn: Txn) -> bv.Event:
    return txn.filter(bv.col("amount") > 100)
```

`BigTxn` is now a registered derivation — its schema mirrors `Txn`'s, and any
event pushed to `Txn` whose `amount > 100` flows to `BigTxn`'s downstream
consumers (other derivations, aggregation tables, etc.).

## `@bv.table` decorator (function form, per ADR-001)

`@bv.table` declares an **aggregation output** — a keyed materialisation of
features computed by `events.group_by(...).agg(...)`. Per
[ADR-001](../../.planning/decisions/ADR-001-bv-table-partial-overturn.md) the
decorator was revived for v0 strictly as the aggregation-output attachment
point. There is no `app.upsert / app.delete / app.retract` surface — those
remain killed by `project_v0_events_only_scope`.

```python
@bv.table(key="user_id")
def UserTxnFeatures(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(
               tx_count_1h=bv.count(window="1h"),
               tx_p99_amount=bv.quantile("amount", q=0.99, window="1h"),
               tx_unique_merchants_1h=bv.n_unique("merchant", window="1h"),
           )
    )
```

The `key=` kwarg names the entity-partition column. For composite keys, pass
a tuple: `@bv.table(key=("user_id", "card_id"))`. The function body MUST be
exactly an `events.group_by(...).agg(...)` chain — `@bv.table` is sugar over
the JSON wire derivation node with `output_kind=table`.

Wire-level: the decorator emits a `{"kind": "derivation", "name": "<Name>",
"output_kind": "table", "key": [...], ...}` payload, identical to what the
server would accept from a hand-written register JSON. SDK porters in 13.6
implement the same shape via builders (TS) or struct-returning functions (Go).

### Global aggregation — `@bv.table` no-`key=` form (per ADR-003)

Per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md), `@bv.table` may be declared **without** a `key=` kwarg → declares a **global table** (single output dict, no per-entity dimension). The wire-level signal is `key: []` (empty array).

```python
@bv.event
class Click:
    user_id: str
    page: str

# Per-entity (existing) — declares a state slot per user_id
@bv.table(key="user_id")
def UserClicks(c) -> bv.Table:
    return c.group_by("user_id").agg(total=bv.count(window="1h"))

# Global (per ADR-003) — declares a single state slot for the whole table
@bv.table   # no key= → global table
def TotalClicks(c) -> bv.Table:
    return c.agg(total=bv.count(window="1h"))   # no group_by

app.get("UserClicks", "alice")  # → {"total": 7}, requires entity arg
app.get("TotalClicks")          # → {"total": 1234}, no entity arg
```

Three equivalent forms compile to the same wire payload (all use `key: []`):

```python
clicks.agg(total=bv.count(...))                     # shortest — direct .agg() shorthand
clicks.group_by().agg(total=bv.count(...))          # explicit empty group_by
@bv.table                                           # decorator with no key=
def Foo(c): return c.agg(total=bv.count(...))
```

All 53 operators work with both per-entity and global aggregation — same op semantics, different state-keying dimension. See [`docs/concepts/global-aggregation.md`](../concepts/global-aggregation.md) for the full conceptual treatment (when to use global vs per-entity, performance characteristics, composition with `cold_after=`).

Implementation deferred to Phase 13.4 (engine sentinel routing) + Phase 13.5 (Python SDK no-`key=` form) + Phase 13.6 (TS + Go SDK overloads). Acceptance gate: `python/tests/v0/test_global.py` (Plan 13.0-16, 8 tests).

## Chain methods overview

Stateless op methods are available on every `EventSource` and
`EventDerivation` (and on the result of `.filter(...)` etc., enabling
fluent chaining). The full per-method semantics live in
[compilation-rules.md](compilation-rules.md).

| Method | Purpose |
|--------|---------|
| `.filter(expr)` | Keep only rows where `expr` evaluates True. |
| `.select(*cols)` | Narrow to the named columns. |
| `.drop(*cols)` | Remove the named columns. |
| `.rename(**mapping)` | Rename columns. |
| `.with_columns(**exprs)` | Add or overwrite derived columns. |
| `.map(**exprs)` | Alias for `.with_columns`. |
| `.cast(**type_map)` | Change field types. |
| `.fillna(**defaults)` | Replace nulls with defaults. |
| `.group_by(*keys)` | Returns `GroupBy` (intermediate; cannot push). |

The `GroupBy` intermediate exposes one method:

| Method | Purpose |
|--------|---------|
| `.agg(**named_features)` | Returns the table-shaped derivation. |

## `bv.col` expressions

Predicate / derivation expressions are built with `bv.col(...)` — see
[expressions.md](expressions.md) for the exhaustive operator list. Examples:

```python
bv.col("amount") > 100
(bv.col("amount") > 100) & (bv.col("merchant") != "amazon")
bv.col("amount").isnull()
bv.col("amount").cast("int")
```

Expressions are composed via Python operator overloading on AST nodes; the
SDK serialises them to a canonical parenthesised string at register time,
and the server's expression evaluator parses that string back into a
predicate.

## What's not supported

beava v0 is **events-only** + **processing-time only**. The following
surfaces are out of scope:

- **Multi-upstream event derivations** — `@bv.event def C(a: A, b: B)` is
  rejected at decoration time with `TypeError`. The engine only supports
  single-upstream chains in v0. Workaround: pick a single upstream and combine
  state at the table layer (e.g., `AStats` reads `A`, `BStats` reads `B`, and
  `CStats` reads the shared source `Txn` to act as a union).
- **Joins** (`event ↔ event`, `event ↔ table`, `table ↔ table`) — permanently
  killed per `project_redis_shaped_no_event_time_ever`. Compose via push/get
  patterns + entity-key sharding instead. Returns alongside tables in v0.1+
  if/when justified by demand.
- **`bv.union`** — deferred with joins.
- **Event-time / watermarks / `event_time_field` / `tolerate_delay`** —
  permanently killed per the same architectural lock. The server stamps
  wall-clock arrival time on every push; `agg_windowed` operators bucket on
  that.
- **Session windows** (`bv.session(gap_ms=..., inner=...)`) — out of v0 + v0.1
  per `.planning/ideas/session-windows-v0.1.md`.
- **Table mutation surface** (`app.upsert / app.delete / app.retract`) —
  killed in Phase 12.7. `@bv.table` is revived for **aggregation output
  only** per [ADR-001](../../.planning/decisions/ADR-001-bv-table-partial-overturn.md).
- **`bv.fork(...)` / `playground.beava.dev`** — dropped from the v0 ship.
- **CEP / sequence pattern detection / `on_timer` callbacks** — deferred
  post-v0; not part of the operator catalogue.

For each of those, the server raises a structured error code at register
time — see [docs/error-codes.md](../error-codes.md) for the full list.

## Cross-references

- [Pipeline DSL Expressions (`bv.col`)](expressions.md) — exhaustive operator
  reference for predicate / derivation expressions.
- [Pipeline DSL Compilation Rules](compilation-rules.md) — per-method H3
  worked examples (Python source → JSON wire → server semantics) plus the
  ambiguity matrix locking edge-case rulings.
- [Operator Catalog](../operators/index.md) — 53 per-op reference pages.
- [Wire spec](../wire-spec.md) — canonical JSON contract every SDK targets.
- [Schema evolution](../schema-evolution.md) — `force=True` / `dry_run=True`
  semantics for re-registering pipelines.
- [Error codes](../error-codes.md) — alphabetical structured-code list with
  HTTP status mapping.
- [ADR-001](../../.planning/decisions/ADR-001-bv-table-partial-overturn.md) —
  `@bv.table` aggregation-output revival.
- [ADR-002](../../.planning/decisions/ADR-002-polars-op-rename.md) — Polars
  op-rename rationale (`avg`→`mean`, `variance`→`var`, `stddev`→`std`,
  `count_distinct`→`n_unique`, `percentile`→`quantile`).
