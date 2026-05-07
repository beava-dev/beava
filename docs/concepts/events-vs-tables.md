# Events vs Tables

beava's registered DAG has two kinds of nodes: **events** (immutable
append-only streams that you push into) and **tables** (named, keyed
aggregation outputs that you query). v0 is events-only on the input side —
the only thing the SDK and HTTP API let you push is an event. Tables exist
only as the *output* of an aggregation chain, never as a user-mutable store.

This page explains what each one is, what the v0 line is between them, and
when to reach for which.

## Overview

| Node       | What it is                              | How it changes                                         | How you read it                |
| ---------- | --------------------------------------- | ------------------------------------------------------ | ------------------------------ |
| `@bv.event` | Immutable, append-only event stream     | `app.push(EventName, fields)` adds one event           | Cannot be queried directly      |
| `@bv.table` | Aggregation output, keyed by partition  | Updates implicitly when upstream events arrive          | `app.get(TableName, key)`      |

Events are facts that have happened; you push them. Tables are functions of
those facts; you read them. beava holds both in memory; the apply loop
updates table state every time a relevant event arrives.

## `@bv.event` — declares an event source

`@bv.event` decorates a class (or function) describing the shape of one
input stream. It is the only way new data enters beava.

**Class form:**

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float
    merchant: str
```

**Function form** (equivalent, used when you want validators / defaults):

```python
@bv.event
def Txn(user_id: str, amount: float, merchant: str):
    ...
```

Pushing an event:

```python
app.push(Txn, {"user_id": "u_123", "amount": 47.50, "merchant": "acme"})
```

That call writes to the WAL, increments any aggregations that index `Txn`,
and acks. It does not return data; events are write-only on the wire.

## `@bv.table` — declares an aggregation output

Per [ADR-001](../../.planning/decisions/ADR-001-bv-table-partial-overturn.md),
the `@bv.table` decorator survives in v0 — but only in **function form**, and
only as the attachment point for an aggregation chain. The decorator wraps a
`events.group_by(...).agg(...)` expression into a named, keyed derivation
node with `output_kind=table`.

```python
@bv.table(key="user_id")
def UserFeatures(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(
               tx_count_1h=bv.count(window="1h"),
               tx_sum_1h=bv.sum("amount", window="1h"),
               tx_lifetime=bv.count(),
           )
    )
```

After register, `UserFeatures` is queryable per key:

```python
features = app.get(UserFeatures, "u_123")
# {"tx_count_1h": 7, "tx_sum_1h": 412.50, "tx_lifetime": 1832}
```

The decorator is pure sugar over the same JSON wire shape Python emits for
any aggregation node — see
[../pipeline-dsl/compilation-rules.md](../pipeline-dsl/compilation-rules.md)
for the compiled wire form.

## What `@bv.table` is NOT in v0

The pre-12.7 `@bv.table` surface was much wider. ADR-001 deliberately
revives only the aggregation-output use case. Everything else stays gone:

- **NOT a mutable upserted store.** `app.upsert(table, key, fields)` does
  not exist. There is no SDK verb to write a row directly into a table.
- **NOT a tombstoned delete store.** `app.delete(table, key)` does not
  exist. Rows live for as long as their backing aggregations hold state.
- **NOT a temporal MVCC table.** `TemporalStore`, `MvccVersion`,
  `temporal_http`, and the WAL `RecordType::Table*` variants were stripped
  in Phase 12.7 and stay stripped. There are no time-travel queries.
- **NOT a retraction-aware aggregation.** Pushing a "retracting" event does
  not propagate undo through downstream aggregations. `app.retract(...)` is
  also gone.
- **NOT a class-form decorator.** `@bv.table` as a class decorator (the v1
  shape) is rejected at register-time with the structured error code
  `bv_table_class_form_not_supported` (see
  [../error-codes.md](../error-codes.md)).
- **NOT an aggregation source.** A `@bv.table` cannot be the input to
  another `@bv.table`'s `group_by(...).agg(...)`. Aggregating a table is
  rejected with `aggregation_on_table_not_supported`.

If you need any of these, you are in v0.1+ territory — see
[`.planning/ideas/v0.1-deferrals.md`](../../.planning/ideas/v0.1-deferrals.md)
for the deferred surface.

## When to use which

| You want to                                  | Reach for                                 |
| -------------------------------------------- | ----------------------------------------- |
| Record a fact that just happened             | `@bv.event` + `app.push(Event, fields)`   |
| Expose a per-entity feature for live scoring | `@bv.table` wrapping `group_by().agg()`   |
| Look up that feature                         | `app.get(Table, key)` / `batch_get(...)`  |
| "Insert a row" by hand                       | Not v0. Push an event; let aggregation update the table |
| "Delete a row" by hand                       | Not v0. Use `cold_after=` on the source event for TTL eviction |
| Compute a feature from another feature       | Not v0. Aggregating tables stays in v0.1+ |

## Push vs read semantics

- **Push semantics.** Events arrive over the wire as `OP_PUSH` frames (TCP)
  or `POST /push/{stream}` (HTTP). The server validates the event row
  against the registered schema, appends to the WAL, and applies to every
  derivation that indexes that source. The push acks once the WAL append is
  acknowledged (`acks=1` Kafka-style; `OP_PUSH_SYNC` is v0.1+).
- **Read semantics.** Tables are queried via `app.get(Table, key)` or the
  `OP_GET` / `OP_BATCH_GET` opcodes. The reply is a JSON object whose
  fields are the named aggregations declared in the `.agg(...)` call. Reads
  are O(1) per feature against in-memory state — see
  [../architecture/single-thread-apply.md](../architecture/single-thread-apply.md)
  for the apply-vs-query model.
- **Lifecycle.** Events live in the WAL until the next snapshot truncates
  them; aggregation state lives in RAM, snapshotted periodically and
  replayed on boot. See
  [../architecture/wal-snapshot.md](../architecture/wal-snapshot.md).

## Memory implications

A `@bv.event` source itself holds essentially no state beyond its schema —
it's a typed channel. Memory grows on the *table* side, where each
registered aggregation maintains per-entity state. See
[../architecture/memory-budget.md](../architecture/memory-budget.md) for the
verified ~6 KB/entity post-Phase-12.9 number on the fraud-team shape, and
[lifetime-aggregation.md](./lifetime-aggregation.md) for how lifetime ops
declare their per-entity ceilings at register-time per V0-MEM-GOV-02.

If a table is the output of an aggregation that omits `window=`, it runs in
**lifetime mode** — see [lifetime-aggregation.md](./lifetime-aggregation.md)
for the register-time memory contract.

## Cross-references

- [ADR-001: `@bv.table` aggregation-output revival](../../.planning/decisions/ADR-001-bv-table-partial-overturn.md)
  — the canonical record of the partial overturn that brought
  `@bv.table` back as an aggregation-output decorator only.
- [`CLAUDE.md` § Events-Only Invariant](../../CLAUDE.md) — the locked
  Phase 12.7 events-only commitment + ADR-001's amendment.
- [pipeline-dsl/overview.md](../pipeline-dsl/overview.md) — the
  `@bv.event` / `@bv.table` decorator surface in full.
- [pipeline-dsl/compilation-rules.md](../pipeline-dsl/compilation-rules.md)
  — Python source → JSON wire compilation for aggregation chains.
- `docs/concepts/global-aggregation.md` (forthcoming, owned by Plan
  13.0-15 closure per ADR-003) — global-only aggregation surface
  (no `key=`).
- [error-codes.md](../error-codes.md) — `bv_table_class_form_not_supported`,
  `aggregation_on_table_not_supported`, `unsupported_node_kind`.
- [`.planning/ideas/v0.1-deferrals.md`](../../.planning/ideas/v0.1-deferrals.md)
  — table mutation, joins, retraction propagation, session windows.
