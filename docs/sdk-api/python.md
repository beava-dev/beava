# beava Python SDK

> **Status:** Authoritative for v0. Documents the **post-13.5 target** Python
> SDK shape — Phase 13.5 implements the rewrite. Cross-language semantics
> live in [shared.md](shared.md); wire-level body shapes live in
> [docs/wire-spec.md](../wire-spec.md).
> **Last reviewed:** 2026-05-03 (Phase 13.0).

## Overview

Python is the **canonical authoring UX** for beava (per project memory
`project_v2_devex_first` + `project_beava_product`). Feature engineers reach
for Python first; the TypeScript and Go SDKs are ports of the Python surface
into idiomatic JS / Go. Wire semantics are identical across languages
(per [shared.md](shared.md)); per-language idioms differ where the language
demands them.

The v0 Python public surface is:

- The `bv.App` client class (the 7 lifecycle methods plus context-manager support).
- Two decorators: `@bv.event` (event source / derivation) and `@bv.table` (aggregation-output, per [ADR-001](../../.planning/decisions/ADR-001-bv-table-partial-overturn.md)).
- The `bv.col(...)` expression DSL (operator-overloaded AST).
- 53 op helper functions in the `bv.*` namespace, named per Polars conventions per [ADR-002](../../.planning/decisions/ADR-002-polars-op-rename.md) — `bv.mean` / `bv.var` / `bv.std` / `bv.n_unique` / `bv.quantile` (NOT `bv.avg` / `bv.variance` / etc.).
- `beava.test` fixtures for pytest integration.

This doc describes the **v0-target** shape that Phase 13.5 will land. The
current `python/beava/_agg.py` is incomplete (`@bv.table` is stubbed out
per Plan 12.7-06; only 32 of the 53 op helpers are present; `app.batch_get`
and `app.reset` are not yet wired). The forward-looking shape documented
here is what the SDK rewrite ships.

> **Module name:** install via `pip install "git+https://github.com/beava-dev/beava.git#subdirectory=python"` until v0.0.0 GA (when `beava` publishes to PyPI). Import as `import beava as bv`.

## Module structure

```
beava/                     # core flat namespace
├── __init__.py            # public exports: App, event, table, col, lit, count, sum, mean, ...
├── _app.py                # App class + transport dispatch
├── _events.py             # @bv.event decorator (class + function form)
├── _table.py              # @bv.table decorator (function form, per ADR-001)
├── _agg.py                # 53 op helpers (count / sum / mean / ... / z_score)
├── _col.py                # bv.col(...) + bv.lit(...) + operator overloading
├── _errors.py             # exceptions (RegistrationError, BinaryNotFoundError)
├── _types.py              # bv.Optional[T], bv.Field(...), type vocab
├── _wire.py               # frame codec, opcodes
├── _transport.py          # HTTP / TCP / Embed transports + URL-scheme dispatch
└── _embed.py              # binary discovery + spawn

beava/test/                # test fixtures (separate submodule)
├── fixture                # pytest fixture factory
├── replay                 # replay events for deterministic tests
└── assert_features_eq     # assertion helper

beava/cli/                 # CLI subcommands (Redis-style: beava bench, beava demo, etc.)
```

## App class

```python
import beava as bv

class App:
    def __init__(
        self,
        url: str | None = None,
        *,
        timeout: float = 30.0,
    ) -> None: ...

    # Context manager
    def __enter__(self) -> "App": ...
    def __exit__(self, *exc: object) -> None: ...

    # Lifecycle
    def close(self) -> None: ...

    # Public API (7 wire-mapped methods)
    def register(
        self,
        *descriptors: object,
        force: bool = False,
        dry_run: bool = False,
    ) -> dict[str, object]: ...
    def push(self, event_name: str, fields: dict[str, object]) -> dict[str, object]: ...
    def get(self, table: str, key: str | list[str | int | bool]) -> dict[str, object]: ...
    def batch_get(
        self,
        requests: list[tuple[str, str | list[str | int | bool]]],
    ) -> list[dict[str, object]]: ...
    def reset(self) -> None: ...
    def ping(self) -> dict[str, object]: ...
```

Each public method maps 1:1 to a wire opcode:

| Method | Wire opcode | Wire spec section |
|--------|-------------|-------------------|
| `app.register(...)` | `OP_REGISTER` (`0x0001`) | [wire-spec § OP_REGISTER](../wire-spec.md#op_register-0x0001) |
| `app.push(...)` | `OP_PUSH` (`0x0010`) | [wire-spec § OP_PUSH](../wire-spec.md#op_push-0x0010) |
| `app.get(...)` | `OP_GET` (`0x0020`) | [wire-spec § OP_GET](../wire-spec.md#op_get-0x0020) |
| `app.batch_get(...)` | `OP_BATCH_GET` (`0x0024`) | [wire-spec § OP_BATCH_GET](../wire-spec.md#op_batch_get-0x0024) |
| `app.reset()` | `OP_RESET` (`0x0040`) | [wire-spec § OP_RESET](../wire-spec.md#op_reset-0x0040) |
| `app.ping()` | `OP_PING` (`0x0000`) | [wire-spec § OP_PING](../wire-spec.md#op_ping-0x0000) |
| `app.close()` | (lifecycle) | n/a — closes transport + terminates embed subprocess. |

### Constructor

`bv.App(url=None, *, timeout=30.0)` — the URL controls transport selection
per [shared.md § Wire transports](shared.md#wire-transports):

- `http://...` / `https://...` → HTTP/JSON transport.
- `tcp://...` → custom-framed TCP transport.
- `None` (default) → embed mode; spawns local `beava` binary on ephemeral ports.

`timeout` is a transport-level I/O timeout in seconds (default 30.0).

**Embed mode requires the context manager:**

```python
with bv.App() as app:                    # spawns the binary, binds to ephemeral ports
    app.register(Txn, UserFeatures)
    app.push("Txn", {"user_id": "alice", "amount": 42.50})
    print(app.get("UserFeatures", "alice"))
# subprocess terminated on exit
```

Calling `register(...)` on an embed-mode `App` outside a `with` block raises
`RuntimeError`. Explicit-URL `App` instances may use `with` or be closed
manually via `app.close()`; both are idempotent.

**Embed-mode disk lifecycle (`test_mode=True` and the default
embed-spawn path):** Each embed spawn allocates a unique tmpdir under
`$TMPDIR/beava-embed-<pid>-<unix-ms>-<hex>/` holding the binary's
`./wal` and `./snapshots` sub-dirs. The path is registered with
`atexit.register(shutil.rmtree, ..., ignore_errors=True)`, so the dir
is reaped at Python interpreter shutdown — long-running test processes
(e.g. a pytest session that spawns 100s of embed apps) do not leak
disk. SIGKILL'd Python processes leave the dir for the OS tmpfs reaper
to handle (typical `$TMPDIR` aging is days). Users of `App(test_mode=True)`
do not need to register their own cleanup; the SDK handles it.

### `app.register(*descriptors, force=False, dry_run=False)`

**Wire opcode:** `OP_REGISTER` (`0x0001`).

Validates the descriptor list locally (DAG / schema checks; zero network
I/O), topo-sorts upstreams before dependents, compiles the `OP_REGISTER`
JSON payload, and dispatches.

**Args:**

- `*descriptors`: one or more descriptor objects returned by `@bv.event`,
  `@bv.table`, or fluent op chains (e.g.,
  `Txn.filter(bv.col("amount") > 100).named("BigTxn")`).
- `force` (kwarg): if `True`, accept destructive schema changes (e.g.,
  field type changes). Default `False` — destructive changes raise
  `RegistrationError(code="registration_conflict")` (HTTP `409`).
- `dry_run` (kwarg): if `True`, return the diff without applying. Response
  carries `{added, removed, changed, diff}`; `registry_version` is unchanged.

**Returns:** server response dict, e.g.
`{"status": "ok", "registry_version": 1, "added": ["Txn", "UserFeatures"]}`.

**Raises:**

- `RegistrationError` — local validation failed OR server returned 4xx / 5xx.
  `.code` carries the structured error code per [shared.md § ValidationError envelope](shared.md#validationerror-envelope);
  `.errors` lists all `ValidationError` entries when the server returns
  multiple problems.
- `RuntimeError` — App is closed, or embed-mode used without context manager.

### `app.push(event_name, fields)`

**Wire opcode:** `OP_PUSH` (`0x0010`).

Push a single event into a registered event source. `event_name` matches
the source's class name (or function name, for derivation-form sources).

**Args:**

- `event_name`: string matching a registered event source.
- `fields`: dict mapping schema field names to values. Field types must
  match the registered schema (string-to-int and similar coercions are
  accepted in v0 per the wire spec).

**Returns:** dict carrying `ack_lsn` (server-assigned monotonic Log
Sequence Number) and `registry_version`. Idempotent re-pushes (matching
`dedupe_key` within `dedupe_window`) return the prior `ack_lsn` plus
`idempotent_replay: true`.

**Raises:**

- `ValidationError` (push variant) — `schema_mismatch`, `missing_field`,
  `unknown_event`. See [docs/error-codes.md](../error-codes.md) (forward-ref
  Plan 13.0-12) for the full list.
- `RuntimeError` — App is closed, or embed-mode used without context manager.

### `app.get(table, key)`

**Wire opcode:** `OP_GET` (`0x0020`).

Single-row feature read. Returns the **row-shape** — a flat dict of feature
name → value — for the requested `(table, key)` pair.

**Args:**

- `table`: name of a registered table (declared via `@bv.table`).
- `key`: either a string (single-key tables) or a list of `[str | int | bool]`
  for composite-key tables.

**Returns:** `dict[str, Any]` mapping feature name to value. **Cold-start**
(no events ever pushed for this key) returns `{}` — this is **not** an
error per [shared.md § FeatureResult shape](shared.md#featureresult-shape).

**Raises:**

- `ValidationError` — `unknown_table`, `feature_not_in_table`, `key_shape_mismatch`.
- `RuntimeError` — App is closed, or embed-mode used without context manager.

### `app.batch_get(requests)`

**Wire opcode:** `OP_BATCH_GET` (`0x0024`).

Heterogeneous batch lookup. Equivalent to N parallel `app.get(...)` calls in
a single round-trip; the server processes them in order, and the response
list preserves request order.

**Args:**

- `requests`: list of `(table, key)` tuples. Different `table` values may
  appear in the same batch.

**Returns:** `list[dict[str, Any]]` matching request order. Per-entry
cold-start is `{}`.

**Raises:**

- `ValidationError` — same set as `app.get(...)` plus `batch_too_large`
  (when more than `max_batch_size` entries; default 10000). Per
  [shared.md § Error semantics](shared.md#error-semantics), v0 has **no
  partial success** — any single bad entry fails the whole batch.

### `app.reset()`

**Wire opcode:** `OP_RESET` (`0x0040`).

Wipe all in-memory state and truncate the WAL. **Destructive — only call
on a beava instance bound to test data.** Used by `beava.test.fixture` to
clear state between tests.

**Returns:** `None`.

**Raises:**

- `RuntimeError` — server config has `enable_reset_op=false` (production
  operators set this to forbid resets).

### `app.ping()`

**Wire opcode:** `OP_PING` (`0x0000`).

Health probe + version discovery.

**Returns:** dict carrying `server_version` (semver string, e.g. `"0.0.0"`)
and `registry_version` (monotonic counter; clients use as a cache key
when caching feature schemas).

### `app.close()`

Close the underlying transport (idempotent). For embed-mode `App`
instances, this also terminates the subprocess (SIGTERM, then SIGKILL
after 5 seconds).

`__exit__` calls `close()` automatically; manually-managed `App` instances
should call `app.close()` in a `finally` block.

## Decorators

### @bv.event

The `@bv.event` decorator declares an **event source** (push-shaped) or a
**derivation** (chain of stateless ops on top of an event source).

#### Class form — event source

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float
    merchant: str
    ip: bv.Optional[str]              # nullable per shared.md § Field types
```

The class body declares the event's **schema** via Python type annotations.
Supported field types are the 6-element vocabulary from
[shared.md § Field types](shared.md#field-types). Use `bv.Optional[T]` (NOT
`typing.Optional[T]`) for nullable fields.

**Per-source kwargs:**

```python
@bv.event(
    keep_events_for="30d",        # event retention; default None (unbounded)
    cold_after="1d",              # cold-entity TTL per V0-MEM-GOV-01; default None
    dedupe_key="trace_id",        # field name for idempotent replay
    dedupe_window="5m",           # dedup TTL
)
class Login:
    user_id: str
    device_id: str
    trace_id: str
```

| Kwarg | Type | Default | Behavior |
|-------|------|---------|----------|
| `keep_events_for` | duration string | `None` | Event-retention TTL. `None` = unbounded (windowed ops still bound state on their windows). |
| `cold_after` | duration string | `None` | Per-source cold-entity TTL per V0-MEM-GOV-01 (Phase 12.8). Range: `[1s, 365d]`; `"forever"` is REJECTED — use `cold_after=None` for unbounded retention. |
| `dedupe_key` | field name | `None` | Field used for idempotent-replay matching. Must be in schema. |
| `dedupe_window` | duration string | `None` | Dedup TTL — re-pushes within this window with matching `dedupe_key` are treated as idempotent replays. |

**`event_time` is NOT supported in v0** per
`project_redis_shaped_no_event_time_ever`. The server stamps wall-clock
processing time on every push; declaring an `event_time` field on the
class raises `TypeError`. Same for the `tolerate_delay` and
`event_time_field` kwargs — they raise `TypeError` at decoration time.

#### Function form — event derivation

```python
@bv.event
def BigTxn(txn: Txn):
    return txn.filter(bv.col("amount") > 100)
```

The function form takes one or more **annotated parameters** referencing
upstream `@bv.event`-decorated descriptors and returns a chain expression.
The decorator extracts the chain, names it (function name → derivation
name), and registers it as a derivation node with `output_kind=event`.

The chain methods supported on event descriptors are documented under
[Pipeline DSL](#pipeline-dsl-chained-methods-on-eventtable) below.

The function-form parameter annotations are resolved against the same
declaration-site contract documented under
[Supported `@bv.event` declaration sites](#supported-bvevent-declaration-sites)
below — module-level + enclosing closure + caller-frame locals.

### @bv.table (function form, per ADR-001)

```python
@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserTxnFeatures(txn: Txn):
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

The `@bv.table(key=...)` decorator wraps a function whose body returns
`events.group_by(...).agg(...)` into a **named, keyed derivation** with
`output_kind=table`. This is the **partial overturn** of the events-only
commitment per [ADR-001](../../.planning/decisions/ADR-001-bv-table-partial-overturn.md):

- `@bv.table(key=...)` is REVIVED as the aggregation-output decorator.
- `app.upsert(...)`, `app.delete(...)`, `app.retract(...)` REMAIN absent.
- Tables are populated **only** by upstream aggregation derivations — they
  are NOT user-mutable.
- MVCC, `TemporalStore`, retraction propagation, and session windows
  REMAIN killed.

**Args:**

- `key`: string OR list of strings (composite key). The list form declares
  a composite-key table; entries on the wire follow the same order as
  the list.

**Function body:** the body MUST return `events.group_by(...).agg(...)`.
The decorator captures the chain, names the result (function name →
table name), and emits a derivation node with `output_kind=table`.

**Class form is v0.1+** — only function form is supported in v0.

#### Supported `@bv.event` declaration sites

`@bv.event` and `@bv.table` need to resolve their parameter annotations
back to the actual upstream class objects. This works under PEP 563
(`from __future__ import annotations`) by following a documented
resolution order. Any name found by one of the sites below resolves
correctly; names that don't appear in any of these scopes raise a
`NameError`-style failure at decoration time.

| # | Site | Mechanism | Example |
|---|---|---|---|
| 1 | Module-level (canonical) | `fn.__globals__` | `@bv.event class Click: ...` at module top |
| 2 | Enclosing closure cells | `fn.__closure__` + `fn.__code__.co_freevars` | Inner-class captured by the decorated fn body |
| 3 | Caller-frame `f_locals` (user-code) | Walked outward from the decoration site by FILE IDENTITY (any frame outside `python/beava/_table.py` and `python/beava/_events.py`); first-seen wins; depth-bounded to 32 frames | Function-local class declared inside a pytest test fn or class method |

**Module-level (priority 1, canonical):**

```python
@bv.event
class Click:
    user_id: str
    page: str

@bv.table(key="user_id")
def UserClicks(c: Click):
    return c.group_by("user_id").agg(n=bv.count(window="forever"))
```

This is the mypy-friendly default; prefer it whenever possible.

**Function-local (priority 3, pytest-fixture pattern):**

```python
def test_user_clicks():
    @bv.event
    class Click:                 # local to this fn — never reaches module scope
        user_id: str

    @bv.table(key="user_id")
    def UserClicks(c: Click):
        return c.group_by("user_id").agg(n=bv.count(window="forever"))

    # ... use UserClicks in the test ...
```

The resolver finds `Click` by walking outward through the call stack
(skipping its own internal frames) until it lands on the test fn's frame,
where `Click` is in `f_locals`.

**Inner-class via closure (priority 2, factory pattern):**

```python
@bv.event
class Click:
    user_id: str

def make_user_clicks_table():
    @bv.table(key="user_id")
    def UserClicks(c: Click):    # Click captured as a free variable
        return c.group_by("user_id").agg(n=bv.count(window="forever"))

    return UserClicks
```

The decorated fn `UserClicks` references `Click` from the enclosing
scope. Python compiles `Click` into the fn's `__closure__` cells; the
resolver inspects the cells directly. This pattern is also robust to
`@functools.lru_cache`-wrapped factories — the cache wraps the OUTER
factory, not the inner decorated fn, so the closure cells survive.

**Class-method (priority 3, unittest pattern):**

```python
class TestSuite:
    def make_table(self):
        @bv.event
        class Click:
            user_id: str

        @bv.table(key="user_id")
        def UserClicks(c: Click):
            return c.group_by("user_id").agg(n=bv.count(window="forever"))

        return UserClicks
```

Same priority-3 mechanism as the function-local pattern; the resolver
walks back to the method body's frame.

**Not supported:**

- Lambdas: `@bv.table` requires a real `def` body — the upstream-proxy
  call needs a fn object, and lambdas are limited to a single
  expression.
- Names imported via `from x import *` *after* the decorator runs: the
  resolver runs at decoration time; if the name doesn't exist yet, it
  can't be found.

## bv.sum signature (Q1 Path B locked)

```python
def sum(field: str, *, window: str | None = None, where: bv.Col | None = None) -> AggDescriptor: ...
```

> **Locked per Q1 Path B** ([13.0-CONTEXT.md](../../.planning/phases/13.0-design-contract-spec-docs/13.0-CONTEXT.md)).
> The Python `bv.sum(field: str, ...)` signature accepts a string column name
> **only**. Inline expressions are **FORBIDDEN**.

### What is FORBIDDEN

```python
# FORBIDDEN — inline boolean-cast expression as the field arg.
bv.sum(bv.col("is_fraud").cast(int), window="1h")     # ✗ raises RegistrationError
bv.sum(bv.col("amount") * 2, window="1h")             # ✗ same
```

Why: v0 keeps the `bv.sum(field: str)` shape stable across all 3 SDKs (TS
`bv.sum("field", { window: "1h" })`, Go `beava.Sum("field", beava.Window("1h"))`).
Allowing arbitrary `_ExprAST` field args in Python only would split the
contract — TS and Go would either need feature-parity (extra wire surface)
or stay narrower (unequal SDKs). Locking the signature to `field: str`
keeps every SDK at parity and keeps the wire contract minimal.

The SDK raises `RegistrationError(code="schema_mismatch")` at register-time
when the field arg is not a string.

### Recommended pattern: two-stage `with_columns` + `sum`

The canonical pattern for **conditional counts** (e.g., "count of fraud
events per user per hour") uses a two-stage chain — derive a typed column
with `with_columns(...)` first, then sum the typed column:

```python
@bv.table(key="user_id")
def UserFraudCounts(txn: Txn):
    return (
        txn.with_columns(flag_int=bv.col("is_fraud").cast(int))   # stage 1: derive int column
           .group_by("user_id")
           .agg(c=bv.sum("flag_int", window="1h"))                # stage 2: sum the int column
    )
```

The `with_columns` call writes a derived field (`flag_int` here) into the
event row before the `group_by(...)` keys it. The aggregation then sums a
plain `i64` field, exactly as the wire shape expects.

> **See:** [`docs/pipeline-dsl/compilation-rules.md`](../pipeline-dsl/compilation-rules.md)
> § Boolean-sum recipe (Plan 13.0-12 — forward reference) for the canonical
> worked example, the corresponding wire JSON, and the ambiguity-matrix
> FORBIDDEN row that locks this rule across all 3 SDKs.

This narrowing applies **symmetrically** across the
[TypeScript SDK](typescript.md) and the [Go SDK](go.md). All three express
the same rule with idiomatic syntax:

| Language | Forbidden | Recommended |
|----------|-----------|-------------|
| Python | `bv.sum(bv.col("flag").cast(int), window="1h")` | `events.with_columns(flag_int=bv.col("flag").cast(int)).group_by(...).agg(c=bv.sum("flag_int", window="1h"))` |
| TypeScript | `bv.sum(bv.col("flag").cast("int"), { window: "1h" })` | `events.withColumns({ flag_int: bv.col("flag").cast("int") }).groupBy(...).agg({ c: bv.sum("flag_int", { window: "1h" }) })` |
| Go | `beava.Sum(beava.Col("flag").Cast("int"), beava.Window("1h"))` | `events.WithColumns(map[string]beava.Expr{ "flag_int": beava.Col("flag").Cast("int") }).GroupBy(...).Agg(...)` |

## Pipeline DSL (chained methods on Event/Table)

Per [docs/pipeline-dsl/overview.md](../pipeline-dsl/overview.md) (Plan
13.0-12 — forward reference), the v0-supported chain methods on event
descriptors and event derivations are Polars-style:

| Method | Returns | Description |
|--------|---------|-------------|
| `events.filter(expr)` | `EventDerivation` | Keep only rows where `expr` is True. |
| `events.select(*cols)` | `EventDerivation` | Keep only the named fields. |
| `events.drop(*cols)` | `EventDerivation` | Remove the named fields. |
| `events.rename(**mapping)` | `EventDerivation` | Rename fields per mapping. |
| `events.with_columns(**exprs)` | `EventDerivation` | Add or overwrite derived fields. |
| `events.map(**exprs)` | `EventDerivation` | Alias for `with_columns` (legacy). |
| `events.cast(**type_map)` | `EventDerivation` | Change field types; targets in `{"str", "int", "float", "bool"}`. |
| `events.fillna(**defaults)` | `EventDerivation` | Replace null values. |
| `events.group_by(*keys)` | `GroupBy` | Start an aggregation pipeline. |
| `groupby.agg(**named_features)` | derivation | Compile to an aggregation derivation node. |

The full ambiguity matrix (chained filters, select-then-group-by,
multi-agg, FORBIDDEN patterns like `with_columns AFTER group_by`) lives at
[`docs/pipeline-dsl/compilation-rules.md`](../pipeline-dsl/compilation-rules.md).

## Expression DSL (bv.col)

```python
bv.col("amount") > 100                               # comparison: amount > 100
bv.col("user_id") == "alice"                         # equality: user_id == 'alice'
(bv.col("amount") > 100) & (bv.col("status") == "ok")  # conjunction (use & for and)
(bv.col("amount") > 100) | (bv.col("status") == "ok")  # disjunction (use | for or)
~(bv.col("flag"))                                    # negation (use ~ for not)
bv.col("amount").isnull()                            # null check
bv.col("status").cast("int")                         # type cast
bv.col("a") + bv.col("b") * 2                        # arithmetic
bv.lit(42)                                           # literal value
```

Operator overloading details:

- `+`, `-`, `*`, `/` — arithmetic.
- `>`, `>=`, `<`, `<=`, `==`, `!=` — comparison (always returns `bool`).
- `&`, `|`, `~` — boolean combinators (`and`, `or`, `not`). Python's
  `and` / `or` / `not` keywords cannot be overloaded; use the bitwise
  symbols.
- `.isnull()` — equivalent to `(x == null)`.
- `.cast("type")` — emits `cast(x, type)` where the target name renders
  as a bare identifier (validated against `{"str", "int", "float", "bool"}`).

Cross-link: [`docs/pipeline-dsl/expressions.md`](../pipeline-dsl/expressions.md)
(Plan 13.0-12) for the full grammar and edge cases.

`bv.lit(value)` wraps a Python value as a literal AST node, useful for
explicit literal coercion or for the rare case where Python operator
precedence requires it.

## Public expression literals (`bv.lit`) — per ADR-003

Per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md), `bv.lit(value)` is exposed as a public factory function in the `bv` namespace. The signature accepts `int | float | str | bool | None`:

```python
def lit(value: int | float | str | bool | None) -> Expr: ...
```

Use cases:

```python
# Constant column — add a fixed-value column to an event derivation
events.with_columns(source=bv.lit("web"))

# Force float division — both operands could be ints, but bv.lit makes float explicit
events.with_columns(rate=bv.col("count") / bv.lit(60.0))

# Explicit literal in filter (equivalent to implicit operator-overloading coercion)
events.filter(bv.col("amount") > bv.lit(100))
```

The implicit operator-overloading coercion (`bv.col("x") > 100`) still works — `bv.lit` is for cases where explicit construction matters (constant columns, type-coercion patterns, cross-language parity with TS/Go SDKs that lack Python's flexible operator overloading).

`bv.lit` lands in Phase 13.5 (`python/beava/__init__.py`, ~5 LOC). Acceptance gate: `python/tests/v0/test_lit.py` (Plan 13.0-16, 5 tests).

## Global aggregation — per ADR-003

Per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md), beava ships first-class **global aggregation** alongside the per-entity surface. Declare a global table by omitting the `key=` kwarg on `@bv.table`:

```python
@bv.event
class Click:
    user_id: str
    page: str

@bv.table   # no key= → global table
def TotalClicks(clicks) -> bv.Table:
    return clicks.agg(total=bv.count(window="forever"))

app = bv.App()
app.register(Click, TotalClicks)
app.push("Click", {"user_id": "alice", "page": "/home"})
app.push("Click", {"user_id": "bob",   "page": "/home"})

app.get("TotalClicks")  # → {"total": 2}, no entity arg
```

**Three equivalent forms** compile to the same wire payload (all use `key: []`):

```python
clicks.agg(total=bv.count(...))                 # shortest — direct .agg() shorthand
clicks.group_by().agg(total=bv.count(...))      # explicit empty group_by
@bv.table                                       # decorator with no key=
def Foo(c): return c.agg(total=bv.count(...))
```

All 53 operators work with both per-entity and global aggregation — same op semantics, different state-keying dimension. See [`docs/concepts/global-aggregation.md`](../concepts/global-aggregation.md) for the full conceptual treatment (when to use global vs per-entity, performance characteristics, composition with `cold_after=`).

**`App.get` arity contract:**

| Table type | Call shape | Cold-start return |
|---|---|---|
| Per-entity table | `app.get(table_name, entity_id)` (2 args required) | `{}` for unknown entity |
| Global table | `app.get(table_name)` (1 arg required) | `{}` until first event |

Mismatched arity raises `KeyError` with a clear message indicating the table's expected arity. The Python SDK enforces this at call-site (no silent wrong-shape behavior).

**Use cases for global aggregation:**

- Operator dashboards (total throughput, current entity count, global p95)
- Anomaly detection on global rates ("is the GLOBAL signup rate spiking?")
- Top-K-globally features ("top 10 hottest pages on the platform")
- Cross-entity aggregations ("total spend across all users in last hour")

**Implementation deferred** to Phase 13.5 (~110 LOC: `bv.lit` export + `events.group_by()` empty allowance + `events.agg(**aggs)` shorthand + `@bv.table` no-`key=` form + `App.get(table_name)` 1-arg overload). The wire-level signal is `key: []` (empty array) on the register payload + sentinel `key: ""` (empty string) on the GET request — see [`docs/wire-spec.md`](../wire-spec.md) § Global tables. Acceptance gate: `python/tests/v0/test_global.py` (Plan 13.0-16, 8 tests).

## Operator catalog

The `bv.*` namespace exposes 53 op helper functions, organised into 8
families. Each helper returns an `AggDescriptor`; `groupby.agg(...)`
consumes them by keyword to name the resulting feature column. Ops are
named per [ADR-002](../../.planning/decisions/ADR-002-polars-op-rename.md)
(Polars conventions).

| Family | Ops | Doc |
|--------|-----|-----|
| Core (8) | count, sum, mean, min, max, var, std, ratio | [docs/operators/core/](../operators/core/) |
| Sketch (5) | n_unique, quantile, top_k, bloom_member, entropy | [docs/operators/sketch/](../operators/sketch/) |
| Point/ordinal (5) | first, last, first_n, last_n, lag | [docs/operators/point-ordinal/](../operators/point-ordinal/) |
| Recency (10) | first_seen, last_seen, age, has_seen, time_since, time_since_last_n, streak, max_streak, negative_streak, first_seen_in_window | [docs/operators/recency/](../operators/recency/) |
| Decay (6) | ewma (alias ema), ewvar, ew_zscore, decayed_sum, decayed_count, twa | [docs/operators/decay/](../operators/decay/) |
| Velocity (9) | rate_of_change, inter_arrival_stats, burst_count, delta_from_prev, trend, trend_residual, outlier_count, value_change_count, z_score | [docs/operators/velocity/](../operators/velocity/) |
| Bounded-buffer (7) | histogram, hour_of_day_histogram, dow_hour_histogram, seasonal_deviation, event_type_mix, most_recent_n, reservoir_sample | [docs/operators/buffer-geo/](../operators/buffer-geo/) |
| Geo (4) | geo_velocity, geo_distance, geo_spread, distance_from_home | [docs/operators/buffer-geo/](../operators/buffer-geo/) |

Total: 8+5+5+10+6+9+7+4 = **54** entries. The `ema` row is an alias for
`ewma` (same server-side op), so the 53 unique server-side ops plus the
`ema` alias produce the 54-row catalogue table.

Per ADR-002, the renamed ops have **deprecation aliases** in v0:

| New (v0 canonical) | Old (deprecated, raises `DeprecationWarning`) |
|---------------------|----------------------------------------------|
| `bv.mean(...)` | `bv.avg(...)` |
| `bv.var(...)` | `bv.variance(...)` |
| `bv.std(...)` | `bv.stddev(...)` |
| `bv.n_unique(...)` | `bv.count_distinct(...)` |
| `bv.quantile(...)` | `bv.percentile(...)` |

Old names ship as deprecation aliases in v0.0.x and are **removed in v0.1**.

Per-op signatures, semantics, and worked examples live on each per-op
page under [`docs/operators/<family>/<op>.md`](../operators/) (Plans
13.0-05 through 13.0-11 — forward references).

## Exceptions

The public exception hierarchy (from `python/beava/_errors.py`):

```python
class RegistrationError(Exception):
    code: str                          # structured error code (one of 9 ValidationError kinds)
    path: str                          # JSON-pointer-style path to offending field
    message: str                       # human-readable message
    errors: list[ValidationError]      # all errors when server returns multiple

class BinaryNotFoundError(Exception):
    pass                               # raised by embed mode when binary not on PATH

@dataclass(frozen=True)
class ValidationError:
    kind: str                          # one of VALIDATION_ERROR_KINDS
    path: str
    message: str
```

The 9 valid `ValidationError.kind` values are documented in
[shared.md § ValidationError envelope](shared.md#validationerror-envelope).
The full alphabetised structured-code list with HTTP status mapping lives
at [`docs/error-codes.md`](../error-codes.md) (Plan 13.0-12 — forward
reference).

## bv.test fixtures

```python
import pytest
import beava as bv
from beava.test import fixture, assert_features_eq

@pytest.fixture
def app():
    yield from fixture(reset_each=True)

def test_count_per_user(app):
    @bv.event
    class Txn:
        user_id: str

    @bv.table(key="user_id")
    def Counts(txn: Txn):
        return txn.group_by("user_id").agg(c=bv.count(window="1h"))

    app.register(Txn, Counts)
    app.push("Txn", {"user_id": "alice"})
    app.push("Txn", {"user_id": "alice"})
    app.push("Txn", {"user_id": "bob"})

    assert_features_eq(app.get("Counts", "alice"), {"c": 2})
    assert_features_eq(app.get("Counts", "bob"), {"c": 1})
```

`beava.test.fixture(reset_each=True)`:

- Yields an embed-mode `App` instance (binary spawned on ephemeral ports).
- If `reset_each=True` (default), calls `app.reset()` between tests via
  `OP_RESET` to clear in-memory state and truncate the WAL.
- Cleans up the subprocess on test session teardown.

`beava.test.assert_features_eq(got, want)` — assertion helper that
compares feature dicts with helpful diff output. Tolerant of float
near-equality (relative tolerance `1e-9`) for sketch-based ops like
`quantile` and `n_unique`.

## Versioning + compatibility

- **Python versions:** Python 3.10+ (PEP 604 union syntax used throughout).
- **Wire compatibility:** v0 SDKs talk to v0 servers. Cross-version
  compatibility (newer SDK ↔ older server, etc.) is reserved for v0.1+.
- **API stability:** the public surface in this doc is **frozen for v0**.
  Adding new optional kwargs is non-breaking; removing or renaming
  surface is breaking.
- **Deprecation policy:** ADR-002-renamed op aliases (`bv.avg` etc.) ship
  in v0.0.x and are removed in v0.1. The `DeprecationWarning` includes
  the new name.

## Plan-level traceability

This document is authored by Plan 13.0-04 (Wave 1). Downstream consumers:

- [`docs/sdk-api/typescript.md`](typescript.md) — TS SDK port mirrors this surface.
- [`docs/sdk-api/go.md`](go.md) — Go SDK port mirrors this surface.
- **Phase 13.5** — Python SDK rewrite reads this doc as the canonical
  surface; lands the v0-target shape (full `_agg.py` 53 helpers, full
  `_app.py` with `batch_get` / `reset`, `@bv.table` revival).
- **Phase 13.6** — TS + Go SDK ports use this doc as the parity reference.

For the full Phase 13.0 plan tree, see
[`.planning/phases/13.0-design-contract-spec-docs/13.0-PLAN.md`](../../.planning/phases/13.0-design-contract-spec-docs/13.0-PLAN.md).
