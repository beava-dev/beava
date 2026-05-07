# Pipeline DSL Compilation Rules

> **Status:** Authoritative for v0. Documents the **post-13.5 target** Python
> → JSON wire compilation contract. SDK porters in 13.6 (TypeScript + Go)
> consume this doc as the canonical reference for what their compilers MUST
> emit. Where this doc and the current `python/beava/` source disagree, this
> doc wins — Phase 13.5 implements the target shape.
> **Last reviewed:** 2026-05-03 (Phase 13.0).

## How to read this doc

For each chain method (`events.filter()`, `events.group_by()`, etc.) we show:

1. **Python source** — the literal call as you'd write it in user code.
2. **JSON wire** — the descriptor body the SDK sends to the server in the
   `OP_REGISTER` payload (per [wire-spec § OP_REGISTER](../wire-spec.md#op_register-0x0001)).
3. **Server semantics** — what the apply loop does at push time.

After all methods, the [Boolean-sum trick](#boolean-sum-trick-recommended-pattern-for-conditional-counts)
section documents the v0-locked recommended pattern for conditional counts
(per [Q1 Path B](../../.planning/phases/13.0-design-contract-spec-docs/13.0-CONTEXT.md)).

The [Ambiguity Matrix](#ambiguity-matrix) at the bottom rules out 20+ edge
cases as ALLOWED / FORBIDDEN / UNDEFINED with a fixture link or structured
error code per row.

## Cross-language note

Every JSON-wire shape below is what **all 3 SDKs** (Python, TypeScript, Go)
MUST emit. The Python source is the reference syntax — TS uses
`event.filter(col("amount").gt(100))` and Go uses
`event.Filter(col("amount").Gt(100))`, but both compile to the same wire JSON
shown here. Cross-language semantic parity is locked in
[shared.md](../sdk-api/shared.md).

---

### events.filter(expr)

#### Python source

```python
@bv.event
class Txn:
    user_id: str
    amount: float

@bv.event
def BigTxn(txn: Txn) -> bv.Event:
    return txn.filter(bv.col("amount") > 100)
```

#### JSON wire

```json
{
  "kind": "derivation",
  "name": "BigTxn",
  "upstreams": ["Txn"],
  "ops": [
    {"op": "filter", "expr": "(amount > 100)"}
  ],
  "output_kind": "event"
}
```

#### Server semantics

Per-event predicate evaluation. The expression string is parsed by the
server's expression evaluator; events for which the predicate evaluates True
flow downstream, others are dropped. Schema is unchanged. Composes
left-to-right with subsequent ops.

**Chained filters compose by conjunction.** `txn.filter(a).filter(b)` is
equivalent to `txn.filter(a & b)` — both forms emit two ops or one op with a
conjunctive predicate; the server's evaluator collapses them at apply time.

---

### events.select(*cols)

#### Python source

```python
@bv.event
def TxnSlim(txn: Txn) -> bv.Event:
    return txn.select("user_id", "amount")
```

#### JSON wire

```json
{
  "kind": "derivation",
  "name": "TxnSlim",
  "upstreams": ["Txn"],
  "ops": [
    {"op": "select", "fields": ["user_id", "amount"]}
  ],
  "output_kind": "event"
}
```

#### Server semantics

Schema-narrowing: the output schema is exactly the listed fields, in order.
All other fields are dropped from the row before downstream ops see it.
Selecting a field not in the upstream schema is rejected at register time
with `unknown_field_reference`.

---

### events.drop(*cols)

#### Python source

```python
@bv.event
def TxnNoIp(txn: Txn) -> bv.Event:
    return txn.drop("ip", "card_id")
```

#### JSON wire

```json
{
  "kind": "derivation",
  "name": "TxnNoIp",
  "upstreams": ["Txn"],
  "ops": [
    {"op": "drop", "fields": ["ip", "card_id"]}
  ],
  "output_kind": "event"
}
```

#### Server semantics

Schema-narrowing inverse of `select`: the output schema is the upstream
schema **minus** the listed fields. Dropping a field not in the upstream is
a no-op (NOT an error) — for symmetry with the SQL `DROP COLUMN IF EXISTS`
convention. Repeated names are deduplicated.

---

### events.rename(**mapping)

#### Python source

```python
@bv.event
def TxnRenamed(txn: Txn) -> bv.Event:
    return txn.rename(amount="amount_usd", merchant="vendor")
```

#### JSON wire

```json
{
  "kind": "derivation",
  "name": "TxnRenamed",
  "upstreams": ["Txn"],
  "ops": [
    {"op": "rename", "mapping": {"amount": "amount_usd", "merchant": "vendor"}}
  ],
  "output_kind": "event"
}
```

#### Server semantics

In-place column rename. The output schema preserves field order; only the
names change. Renaming a field to one that already exists in the upstream
schema (collision) is rejected with `schema_mismatch`. Renaming a field not
in the upstream schema is rejected with `unknown_field_reference`.

---

### events.with_columns(**exprs)

> Alias: `.map(**exprs)` — same wire shape, different op string. Both forms
> are accepted by the server's apply loop.

#### Python source

```python
@bv.event
def TxnDecorated(txn: Txn) -> bv.Event:
    return txn.with_columns(
        amount_x_2=bv.col("amount") * 2,
        is_big=bv.col("amount") > 100,
    )
```

#### JSON wire

```json
{
  "kind": "derivation",
  "name": "TxnDecorated",
  "upstreams": ["Txn"],
  "ops": [
    {
      "op": "with_columns",
      "exprs": {
        "amount_x_2": "(amount * 2)",
        "is_big": "(amount > 100)"
      }
    }
  ],
  "output_kind": "event"
}
```

#### Server semantics

Adds (or overwrites) the named fields on each event. The expression strings
are parsed once at register time, compiled to expression-AST nodes, and
evaluated per-event. Output schema = upstream schema ∪ new fields, with
type-inferred FieldType per expression (per [expressions.md § Type
rules](expressions.md#arithmetic------)).

The `.map(...)` alias emits `{"op": "map", ...}` instead of `{"op":
"with_columns", ...}` — semantically identical; the op-string preserves
authorial intent on the wire.

---

### events.cast(**type_map)

#### Python source

```python
@bv.event
def TxnCast(txn: Txn) -> bv.Event:
    return txn.cast(amount="int", is_fraud="bool")
```

#### JSON wire

```json
{
  "kind": "derivation",
  "name": "TxnCast",
  "upstreams": ["Txn"],
  "ops": [
    {"op": "cast", "type_map": {"amount": "int", "is_fraud": "bool"}}
  ],
  "output_kind": "event"
}
```

#### Server semantics

In-place column type coercion. Target types are restricted to
`{"str", "int", "float", "bool"}` — the SDK validates at decoration time
and the server re-validates at register time with `invalid_cast_target`.

Coercion rules match the standard widening / narrowing semantics: `int →
float` is lossless; `float → int` truncates; `str → int / float` parses (or
errors at apply time per `schema_mismatch`); `bool → int` returns 0/1 (the
boolean-sum-trick foundation — see below). `bytes` cannot be cast.

---

### events.fillna(**defaults)

#### Python source

```python
@bv.event
def TxnFilled(txn: Txn) -> bv.Event:
    return txn.fillna(merchant="unknown", ip="0.0.0.0")
```

#### JSON wire

```json
{
  "kind": "derivation",
  "name": "TxnFilled",
  "upstreams": ["Txn"],
  "ops": [
    {"op": "fillna", "defaults": {"merchant": "unknown", "ip": "0.0.0.0"}}
  ],
  "output_kind": "event"
}
```

#### Server semantics

Per-event null replacement. For each named field, a null value at apply time
is substituted with the registered default. Default values must be concrete
scalars — `None` as a default is rejected at decoration time (filling-with-null
is a no-op). Defaults must be type-compatible with the field's schema type
(otherwise `schema_mismatch` at register time).

---

### events.group_by(*keys)

#### Python source

```python
@bv.event
class Txn:
    user_id: str
    amount: float

# .group_by(...) returns a GroupBy intermediate; .agg(...) is the next step.
groupby = Txn.group_by("user_id")
```

#### JSON wire

`group_by` is **not** emitted as a standalone op on the wire. It is fused
with the subsequent `.agg(...)` call into a single derivation node with
`output_kind=table`, `key=[<keys>]`, and `agg={...}`. See `groupby.agg(...)`
below for the combined wire form.

#### Server semantics

`GroupBy` is a Python-side intermediate object — it does not travel over the
wire. Its only method is `.agg(**named_features)`, which closes the chain by
returning a `Table`-shaped derivation. The keys are validated client-side at
decoration time (each must be a string field present in the upstream
schema); duplicates / missing keys raise `TypeError` / `ValueError`
immediately.

---

### groupby.agg(**named_features)

#### Python source

```python
@bv.table(key="user_id")
def UserTxnFeatures(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(
               tx_count_1h=bv.count(window="1h"),
               tx_sum_1h=bv.sum("amount", window="1h"),
               tx_p99_1h=bv.quantile("amount", q=0.99, window="1h"),
               tx_unique_merchants_1h=bv.n_unique("merchant", window="1h"),
           )
    )
```

#### JSON wire

```json
{
  "kind": "derivation",
  "name": "UserTxnFeatures",
  "upstreams": ["Txn"],
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "tx_count_1h": {"op": "count", "params": {"window": "1h"}},
    "tx_sum_1h": {"op": "sum", "params": {"field": "amount", "window": "1h"}},
    "tx_p99_1h": {"op": "quantile", "params": {"field": "amount", "q": 0.99, "window": "1h"}},
    "tx_unique_merchants_1h": {"op": "n_unique", "params": {"field": "merchant", "window": "1h"}}
  }
}
```

#### Server semantics

Each named feature is compiled to an `AggOp` instance; per-entity state for
that op is allocated lazily on the first event for each entity-key. Per-event
apply: extract the partition key, look up (or insert) the per-entity state,
call the op's `update(...)` with the event row, and update windowed bucket
state if the op carries a `window=` kwarg.

`output_kind: "table"` is the per-ADR-001 path: the derivation emits a keyed
row materialisation, queryable via `app.get(table_name, key)`. SDK porters
implement the same shape via `bv.table` (TS builder, Go function-returning
struct).

Op-strings inside `agg.<feature>.op` MUST come from the operator catalogue
(53 ops, post-ADR-002 Polars naming). Per [ADR-002](../../.planning/decisions/ADR-002-polars-op-rename.md):
`mean` (was `avg`), `var` (was `variance`), `std` (was `stddev`), `n_unique`
(was `count_distinct`), `quantile` (was `percentile`). Old SQL-prose names
remain as deprecation aliases in v0 Python only.

---

### bv.col(...) operator overloading

#### Python source

```python
predicate = (bv.col("amount") > 100) & (bv.col("merchant") != "amazon")

@bv.event
def TxnFiltered(txn: Txn) -> bv.Event:
    return txn.filter(predicate)
```

#### JSON wire

The expression compiles to a canonical parenthesised string via
`_ExprAST.to_expr_string()`. The wire form for the `filter` op above:

```json
{
  "ops": [
    {"op": "filter", "expr": "((amount > 100) and (merchant != 'amazon'))"}
  ]
}
```

The full operator-overloading list — arithmetic (`+ - * /`), comparison
(`> >= < <= == !=`), boolean (`& | ~`), `.isnull()`, `.cast(type)`, `.alias(name)`
— is documented in [expressions.md](expressions.md). Each operator emits a
specific grammar node:

| Python | Wire |
|--------|------|
| `bv.col("x") + 5` | `(x + 5)` |
| `bv.col("a") - bv.col("b")` | `(a - b)` |
| `bv.col("x") > 100` | `(x > 100)` |
| `bv.col("status") == "ok"` | `(status == 'ok')` |
| `pred1 & pred2` | `(<pred1> and <pred2>)` |
| `pred1 \| pred2` | `(<pred1> or <pred2>)` |
| `~pred1` | `(not <pred1>)` |
| `bv.col("x").isnull()` | `(x == null)` |
| `bv.col("x").cast("int")` | `cast(x, int)` |
| `bv.lit(None)` | `null` |
| `bv.lit(True)` | `true` |
| `bv.lit("hi")` | `'hi'` |

#### Server semantics

Expression strings are parsed once at register time into AST nodes; per-event
evaluation walks the AST. Type checking is enforced at register time per
[expressions.md § Validation at register-time](expressions.md#validation-at-register-time).

---

### window= kwarg semantics

> **Important:** the kwarg name is `window=`. All aggregation helpers in
> `bv.<op>(...)` use the `window` keyword per `python/beava/_agg.py`
> (verified RESEARCH §4 codebase verification). Do not append a `-d` suffix
> when porting to TS / Go — the keyword stays `window` across all 3 SDKs.

#### Python source

```python
# Sliding-window mode (5-minute rolling window):
sliding = bv.count(window="5m")

# Lifetime mode (window= omitted):
lifetime = bv.first_seen()

# Lifetime mode (explicit "forever"):
explicit_lifetime = bv.count(window="forever")
```

#### JSON wire

```json
{
  "agg": {
    "feature_sliding": {"op": "count", "params": {"window": "5m"}},
    "feature_lifetime": {"op": "first_seen", "params": {}},
    "feature_explicit_lifetime": {"op": "count", "params": {"window": "forever"}}
  }
}
```

When `window=` is omitted (or set to `"forever"`), the server allocates a
**lifetime** per-entity state slot — no buckets, no rolling-window eviction;
the op accumulates over all-time-since-cold-start. When `window=` is a
duration string (`5m`, `1h`, `100ms`, `7d`, ...), the server allocates
windowed state with up to 64 rolling buckets indexed by server-side
`now_ms()`.

#### Server semantics

The `window=` kwarg controls per-entity state shape:

- **Lifetime mode** (`window=None` or `window="forever"`): single state slot
  per entity. The op accumulates over all events for that entity. Memory
  bound MUST be declared at register time per Phase 12.8 V0-MEM-GOV-02 — for
  ops without an O(1) lifetime bound, the JSON-prelude shim
  `pre_check_unbounded_op_in_lifetime_mode` rejects with
  `unbounded_op_in_lifetime_mode`.
- **Windowed mode** (`window="<duration>"`): up to 64 rolling buckets
  bucketed by server-side `now_ms()`. Bucket reclaim is per-event during
  `update_at` (Phase 12.8 V0-MEM-GOV-03). Buckets older than `window` ms are
  dropped from the result.
- **Decay ops** (`ewma`, `ewvar`, `decayed_sum`, `decayed_count`,
  `ew_zscore`) take `half_life=` instead of `window=` and reject `forever`
  with `aggregation_invalid_half_life`.

The grammar for `window` strings is `\d+(ms|s|m|h|d)` or `forever` — leading
digit `1-9` (no `0ms`); see [shared.md § Window grammar](../sdk-api/shared.md#window-grammar).

---

### @bv.table decorator (function form, per ADR-001)

#### Python source

```python
@bv.event
class Txn:
    user_id: str
    amount: float
    merchant: str

@bv.table(key="user_id")
def UserTxnFeatures(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(
               tx_count_1h=bv.count(window="1h"),
               tx_sum_1h=bv.sum("amount", window="1h"),
           )
    )
```

#### JSON wire

The decorator wraps the function body — which MUST be exactly an
`events.group_by(...).agg(...)` chain — into a derivation node with
`output_kind: "table"` and the partition key materialised from the `key=`
kwarg:

```json
{
  "kind": "derivation",
  "name": "UserTxnFeatures",
  "upstreams": ["Txn"],
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "tx_count_1h": {"op": "count", "params": {"window": "1h"}},
    "tx_sum_1h": {"op": "sum", "params": {"field": "amount", "window": "1h"}}
  }
}
```

For composite keys: `@bv.table(key=("user_id", "card_id"))` yields
`"key": ["user_id", "card_id"]`.

#### Server semantics

Per [ADR-001](../../.planning/decisions/ADR-001-bv-table-partial-overturn.md),
`@bv.table` is the **aggregation-output decorator** — there is no
`app.upsert / app.delete / app.retract` SDK surface. The decorator is
**function form only** (no class form in v0). The body MUST be exactly
`events.group_by(...).agg(...)`; any other shape (e.g., a chain that returns
a non-aggregation derivation) is rejected with `bad_return_type` at register
time.

Server-side state allocation matches `groupby.agg(...)` above: per-entity
op state, lazy allocation on first event, queryable via
`app.get("UserTxnFeatures", "alice")` returning the row-shape.

The Phase 12.7 architectural test
`crates/beava-server/tests/phase12_7_no_table_surface.rs` is amended in
Phase 13.4 to permit `OpNode::Table*` ONLY when it appears as the
`output_kind` of a derivation (per-AST-context check) — top-level
`{"kind": "table", ...}` register payloads remain rejected with
`unsupported_node_kind`.

---

### @bv.table global form (no `key=`, per ADR-003)

Per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md), `@bv.table` may be declared **without** a `key=` kwarg → declares a **global table** (single output dict, no per-entity dimension). The function body uses `.agg(...)` directly (no `.group_by()`) or an explicit empty `group_by()`.

#### Python source

```python
@bv.event
class Click:
    user_id: str
    page: str

# Global form — no key=, no group_by:
@bv.table
def TotalClicks(clicks) -> bv.Table:
    return clicks.agg(total=bv.count(window="forever"))
```

#### JSON wire

The decorator emits the same derivation node as the per-entity form, with `key: []` (empty array) signalling the global shape:

```json
{
  "kind": "derivation",
  "name": "TotalClicks",
  "upstreams": ["Click"],
  "output_kind": "table",
  "key": [],
  "agg": {
    "total": {"op": "count", "params": {"window": "forever"}}
  }
}
```

#### Three equivalent forms compile to the same wire payload:

```python
clicks.agg(total=bv.count(window="forever"))                  # shortest
clicks.group_by().agg(total=bv.count(window="forever"))       # explicit empty group_by
@bv.table                                                     # decorator no key=
def Foo(c): return c.agg(total=bv.count(window="forever"))
```

All three produce `key: []` on the wire. Server-side state allocation: a single state slot at sentinel `entity_id = ""`, queryable via `app.get("TotalClicks")` (1-arg overload — returns the global feature dict).

#### Server semantics

Per ADR-003, the engine routes `entity_id = ""` (empty string) through the same per-entity hashmap machinery — no new code path. Register-time validation accepts `key: []` as a valid global-table declaration; `key` MUST be either non-empty (per-entity) or empty (global) — never null.

All 53 operators work in both per-entity and global modes — semantics identical, only the state-keying dimension differs. Standard memory governance applies: `cold_after=` doesn't affect global state (always-live); lifetime ops still subject to V0-MEM-GOV-02 lifetime-bound enforcement.

Implementation deferred to Phase 13.4 (engine, ~30 LOC) + Phase 13.5 (Python SDK, ~110 LOC) + Phase 13.6 (TS + Go SDK overloads, ~150 LOC). Acceptance gate: `python/tests/v0/test_global.py` (Plan 13.0-16, 8 tests).

See [`docs/concepts/global-aggregation.md`](../concepts/global-aggregation.md) for the full conceptual treatment.

---

## Boolean-sum trick (recommended pattern for conditional counts)

Per [Q1 Path B locked answer](../../.planning/phases/13.0-design-contract-spec-docs/13.0-CONTEXT.md),
v0 keeps `bv.sum(field: str)` only — the `field` arg accepts a **string
column name**, NOT an `_ExprAST`. To implement a "count where condition"
semantic, use the two-stage `with_columns` + `sum` pattern:

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    is_fraud: bool

@bv.table(key="user_id")
def UserFraud(txn) -> bv.Table:
    return (
        txn.with_columns(is_fraud_int=bv.col("is_fraud").cast("int"))
           .group_by("user_id")
           .agg(fraud_count_1h=bv.sum("is_fraud_int", window="1h"))
    )
```

The wire form is two ops on the derivation:

```json
{
  "kind": "derivation",
  "name": "UserFraud",
  "upstreams": ["Txn"],
  "output_kind": "table",
  "key": ["user_id"],
  "ops": [
    {"op": "with_columns", "exprs": {"is_fraud_int": "cast(is_fraud, int)"}}
  ],
  "agg": {
    "fraud_count_1h": {"op": "sum", "params": {"field": "is_fraud_int", "window": "1h"}}
  }
}
```

This pattern is verified to work because:

1. `with_columns(name=expr)` accepts an `_ExprAST` (per `_events.py::with_columns`)
   and produces a new typed column that flows downstream.
2. `bv.col("is_fraud").cast("int")` coerces `bool → i64` per
   [expressions.md § `.cast()`](expressions.md#cast-type_name--type-coercion);
   `True → 1`, `False → 0`.
3. `bv.sum("is_fraud_int", window="1h")` sums the new integer column over
   the rolling 1-hour window — the count-where-condition semantic.

**Inline boolean-sum like `bv.sum(bv.col("is_fraud").cast("int"))` is FORBIDDEN
in v0.** The SDK raises `RegistrationError(code="schema_mismatch")` at
register time when `field` is not a string. Lifting `bv.sum` to accept an
`_ExprAST` argument is captured in `.planning/ideas/v0.1-deferrals.md` for
v0.1+.

---

## Ambiguity Matrix

Explicit ALLOWED / FORBIDDEN / UNDEFINED rulings on edge cases. SDK porters
in 13.6 grep this matrix during their compiler work; each row links to a
fixture (ALLOWED) or a structured error code (FORBIDDEN).

| Pattern | Verdict | Rationale | Test fixture / Error code |
|---------|---------|-----------|----------------------------|
| `e.filter(a).filter(b)` | ALLOWED, equivalent to `e.filter(a & b)` | Filter ops compose by conjunction at apply time. | (no fixture; both shapes round-trip identically) |
| `e.select("user_id", "amount").group_by("user_id")` | ALLOWED | `select` trims columns; the `group_by` key remains present. | (no fixture; standard chain) |
| `e.with_columns(big=bv.col("amount") > 100).group_by("user_id").agg(c=bv.sum("big_int", window="1h"))` | ALLOWED — recommended boolean-sum pattern | Two-stage: derive a `bool→int` column with `with_columns`, then `sum` it. | See [Boolean-sum trick](#boolean-sum-trick-recommended-pattern-for-conditional-counts) section above |
| `bv.sum(bv.col("amount") * 2)` | FORBIDDEN — `bv.sum` field arg is `str`, not `_ExprAST` | Field arg is a column name string; arithmetic-on-field is v0.1+. Use `with_columns(amount_x_2=bv.col("amount") * 2)` then `bv.sum("amount_x_2", ...)`. | `RegistrationError(code="schema_mismatch")` |
| Inline `bv.sum(bv.col("flag").cast("int"))` | FORBIDDEN — inline boolean-sum (per Q1 Path B) | Same as above; the field arg is `str`, not `_ExprAST`. Use the two-stage `with_columns` + `sum` pattern. | `RegistrationError(code="schema_mismatch")` |
| `e.with_columns(...) AFTER e.group_by(...)` | FORBIDDEN | `group_by` returns `GroupBy`; `with_columns` is not on the `GroupBy` interface. | `AttributeError` (Python); compile-time `TypeError` (TS); compile error (Go) |
| `e.group_by("a").group_by("b")` | FORBIDDEN | `GroupBy` does not expose `.group_by()`; nested grouping is unsupported. | `AttributeError` (Python); compile-time `TypeError` (TS) |
| `e.group_by("a").filter(...)` | FORBIDDEN | `GroupBy` does not expose stateless ops. Filter BEFORE the `group_by`. | `AttributeError` (Python); compile-time `TypeError` (TS) |
| Cross-event aggregation (`bv.sum(other_event.col("x"))` etc.) | FORBIDDEN per `project_redis_shaped_no_event_time_ever` | beava is Redis-shaped, processing-time only — no cross-stream joins ever. | `RegistrationError(code="joins_not_supported")` |
| `event_time` field on `@bv.event` | FORBIDDEN per `project_redis_shaped_no_event_time_ever` | Server-side `now_ms()` is the only time source; client-supplied event time is killed permanently. | `TypeError` at decoration time; `RegistrationError(code="event_time_not_supported_in_v0")` if it reaches the wire |
| `event_time_field=` / `tolerate_delay=` kwargs on `@bv.event` | FORBIDDEN per same lock | Same as above. | `TypeError` at decoration time |
| `bv.col("x") + 5` arithmetic in `where=` predicates | ALLOWED | Compiles to expr-string via `_BinOp.to_expr_string()`. | (no fixture; standard expression) |
| `bv.col("x").isnull()` | ALLOWED | Compiles to `(x == null)` per `_col.py::isnull()`. | (no fixture; standard expression) |
| `bv.col("x").cast("int")` in `with_columns(int_col=...)` | ALLOWED | `with_columns` accepts `_ExprAST`; `.cast()` returns one. | (no fixture; standard expression) |
| `bv.col("x").cast("int")` AS `field` arg to `bv.sum(...)` | FORBIDDEN | Field arg is `str`, not expression — same Q1 Path B locked rule. | `RegistrationError(code="schema_mismatch")` |
| `@bv.table(key="user_id")` function form | ALLOWED per ADR-001 | Wraps `events.group_by(...).agg(...)` into a derivation node with `output_kind=table`. | [`examples/wire/register-fraud-team.request.json`](../../examples/wire/register-fraud-team.request.json) |
| `@bv.table` (no `key=` kwarg) → global table | ALLOWED + RECOMMENDED for global use cases per ADR-003 | Declares a global table — single output dict, wire-level signal `key: []`. Use for monitoring / dashboards / anomaly detection / top-K-globally features. | [`examples/wire/register-global-counter.request.json`](../../examples/wire/register-global-counter.request.json) |
| `events.agg(**aggs)` direct (no `group_by`) | ALLOWED per ADR-003 — equivalent to `events.group_by().agg(...)` | Polars-aligned shorthand for global aggregation. Compiles to the same wire payload as the explicit empty `group_by`. | (no fixture; same wire payload as global `@bv.table` row above) |
| `app.get("GlobalTable")` (1-arg) | ALLOWED per ADR-003 — Python+TS arity overload | Returns the global feature dict. Equivalent to the wire request `{"table": "...", "key": ""}`. Go SDK uses `app.GetGlobal(ctx, "...")` (separate method per Go convention). | [`examples/wire/get-global.request.json`](../../examples/wire/get-global.request.json) + [`examples/wire/get-global.response.json`](../../examples/wire/get-global.response.json) |
| `bv.lit(value)` in expression chains | ALLOWED per ADR-003 — public literal factory | Promotes the existing internal `_Literal` AST node to public namespace. Use cases: constant columns, type-coercion patterns, cross-language parity. | (no fixture; existing literal grammar) |
| `@bv.table` aggregating ANOTHER table | FORBIDDEN — table-to-table aggregation deferred | Only events feed aggregations in v0; aggregation on a `Table` upstream is rejected. | `RegistrationError(code="aggregation_on_table_not_supported")` |
| `@bv.table` class form | FORBIDDEN — class form deferred to v0.1+ | v0 ships function form only per ADR-001. The class-form decorator is captured in `.planning/ideas/v0.1-deferrals.md`. | `RegistrationError(code="bv_table_class_form_not_supported")` |
| `app.upsert(table, key, ...)` | FORBIDDEN — table mutation gone per ADR-001 | Aggregation output is the only `@bv.table` use case in v0. | `AttributeError` on `App` class (no method exists) |
| `app.delete(table, key)` | FORBIDDEN — table mutation gone | Same as above. | `AttributeError` on `App` class |
| `app.retract(...)` | FORBIDDEN — retraction gone | Retraction propagation deferred with table mutation. | `AttributeError` on `App` class |
| `bv.session(gap_ms=..., inner=...)` (session windows) | FORBIDDEN — session windows v0.1+ | Per `.planning/ideas/session-windows-v0.1.md`. | `RegistrationError(code="session_windows_not_supported_in_v0")` |
| `bv.fork(...)` | FORBIDDEN — `bv.fork` dropped from v0 + v0.1 | Per ROADMAP §13 deferral list. | `AttributeError` on `bv` namespace |
| `bv.union(*events)` | FORBIDDEN — deferred with joins | Multiplex client-side for v0; first-class union returns alongside joins in a future minor. | `RegistrationError(code="unions_not_supported_in_v0")` |
| `dry_run=True` flag on `app.register(...)` | ALLOWED | Returns the diff envelope without applying; per [shared.md § Schema evolution](../sdk-api/shared.md#schema-evolution) and [schema-evolution.md](../schema-evolution.md). | [`examples/wire/register-dry-run.request.json`](../../examples/wire/register-dry-run.request.json) |
| `force=True` flag on `app.register(...)` | ALLOWED | Permits destructive register (field type change / removal). Affected aggregations are zeroed. | [`examples/wire/register-force.request.json`](../../examples/wire/register-force.request.json) |

## Cross-references

- [Pipeline DSL Overview](overview.md) — primer on decorators + chained methods.
- [Pipeline DSL Expressions (`bv.col`)](expressions.md) — operator-overloading
  reference for predicate / derivation expressions.
- [Wire spec](../wire-spec.md) — canonical JSON contract.
- [Schema evolution](../schema-evolution.md) — `force=True` / `dry_run=True`
  semantics referenced in the ambiguity matrix.
- [Error codes](../error-codes.md) — alphabetical structured-code list with
  HTTP status mapping for every FORBIDDEN row above.
- [ADR-001](../../.planning/decisions/ADR-001-bv-table-partial-overturn.md) —
  `@bv.table` aggregation-output revival narrative.
- [ADR-002](../../.planning/decisions/ADR-002-polars-op-rename.md) — Polars
  op-rename narrative.
- [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md) —
  first-class global aggregation (`@bv.table` no `key=` / `events.agg(...)` no
  `group_by`) + public `bv.lit` export. See also
  [`docs/concepts/global-aggregation.md`](../concepts/global-aggregation.md).
