---
title: Python SDK
description: The canonical authoring UX for beava — `@bv.event` / `@bv.table`, the `bv.col` expression DSL, 53 op helpers, and the `App` client.
sidebarTitle: Python
---

# Python SDK

beava's Python SDK is the **front door** for declaring pipelines and pushing
events. Authoring (decorators, the `bv.col` DSL, op helpers) is Python-only;
the same `App` client is also available in TypeScript and Go for
[communicate-only use](shared.md) (push events, query features, ship a
pre-compiled JSON pipeline).

```python
import beava as bv

@bv.event
class Click:
    user_id: str
    page: str

@bv.table(key="user_id")
def UserClicks(c: Click):
    return c.group_by("user_id").agg(visits=bv.count(window="1h"))

with bv.App() as app:                # spawn local beava binary
    app.register(Click, UserClicks)
    app.push("Click", {"user_id": "alice", "page": "/home"})
    app.push("Click", {"user_id": "alice", "page": "/pricing"})
    print(app.get("UserClicks", "alice"))   # {'visits': 2}
```

## Install

```bash
pip install tally
```

<Note>
The PyPI package is currently published as **`tally`** (the codename); the
import name is **`beava`**. The PyPI rename to `beava` lands at v0.0.0 GA.
</Note>

Python 3.10+ (PEP 604 union syntax used throughout the SDK).

## App

```python
class App:
    def __init__(
        self,
        url: str | None = None,
        *,
        timeout: float = 30.0,
        test_mode: bool = False,
    ) -> None: ...
```

The constructor selects the transport from the URL scheme:

| `url=` | Transport | Use case |
|---|---|---|
| `None` (default) | Embed — spawns local `beava` binary on ephemeral ports | Tests, local dev |
| `http://...` / `https://...` | HTTP/1.1 + JSON | curl-friendly, LB-friendly |
| `tcp://...` | Custom-framed TCP | Low-latency fast-path |

`timeout` is a transport-level I/O timeout in seconds. `test_mode=True` only
applies in embed mode — sets `BEAVA_TEST_MODE=1` in the spawned binary's
environment so test-only opcodes (`OP_RESET`) are accepted. Setting
`test_mode=True` against a network URL emits a `UserWarning` and is ignored.

<Warning>
**Embed mode requires the context manager.** Calling any wire method on
`bv.App()` outside a `with` block raises `RuntimeError`. Network-mode
`App` instances may use `with` or be closed manually via `app.close()`.
</Warning>

```python
with bv.App() as app:                       # embed
    app.register(Click)
    app.push("Click", {"user_id": "alice", "page": "/home"})

app = bv.App("http://localhost:8080")       # network — no `with` required
try:
    app.push("Click", {"user_id": "alice", "page": "/home"})
finally:
    app.close()
```

### Method summary

Each public method maps 1:1 to one wire opcode. See the
[HTTP API doc](../http-api.md) for the underlying request/response shapes.

| Method | Opcode | Purpose |
|---|---|---|
| `app.register(*descriptors, force=False, dry_run=False)` | `OP_REGISTER` | Declare event sources, derivations, tables. |
| `app.push(event_name, fields)` | `OP_PUSH` | Push one event. Fire-and-forget (acks=1). |
| `app.get(table, key=None, features=None)` | `OP_GET` | Single-row feature read. |
| `app.batch_get(requests)` | `OP_BATCH_GET` | Heterogeneous batched read. |
| `app.reset()` | `OP_RESET` | Wipe state + WAL. **Test-mode only.** |
| `app.ping()` | `OP_PING` | Liveness probe + version discovery. |
| `app.close()` | (lifecycle) | Close transport; terminate embed subprocess. |

### `app.register(*descriptors, force=False, dry_run=False)`

Validates the descriptor list locally (DAG / schema checks; zero network I/O),
topo-sorts upstreams before dependents, compiles the JSON payload, and
dispatches.

- `*descriptors` — descriptor objects produced by `@bv.event` or `@bv.table`.
- `force=True` — accept destructive schema changes (e.g., field type
  changes). Default `False` — destructive changes raise
  `RegistrationError(code="registration_conflict")`.
- `dry_run=True` — validate + return the diff without applying.
  `registry_version` is unchanged.

Returns the server response: `{status, registry_version, added, removed?, changed?, diff?}`.

Raises `RegistrationError` (with `.code`, `.path`, `.message`, `.errors`) on
validation failure. Common codes:

| Code | When |
|---|---|
| `registration_conflict` | Destructive change without `force=True`. |
| `schema_invalid` | Descriptor missing a required field or violates structural constraints. |
| `cycle` / `missing_upstream` | Descriptor list forms a cycle, or references an undeclared upstream. |
| `unknown_op` | `agg.<feature>.op` is not in the operator catalogue. Use [Polars-aligned names](#operator-catalog) (e.g. `mean` not `avg`). |
| `invalid_descriptor` | A raw chain (`EventDerivation`) was passed without being wrapped in `@bv.event def …`. |

The full structured-code list is in [docs/error-codes.md](../error-codes.md).

### `app.push(event_name, fields)`

Push one event. `event_name` matches a registered `@bv.event` class /
function name. `fields` is a flat dict matching the registered schema —
type coercion is permissive on the JSON boundary (string `"42"` for an
`i64` field is accepted).

Returns `{ack_lsn, registry_version}`. If the event is registered with a
`dedupe_key` + `dedupe_window`, duplicates within the window return the
prior `ack_lsn` plus `idempotent_replay: true`.

### `app.get(table, key=None, features=None)`

Single-row feature read. Returns the **row-shape** — a flat dict of
feature name → value.

- `table` — name of a registered table.
- `key` — string for single-key tables; `list[str | int | bool]` for
  composite-key tables. Pass `None` (the default) for global tables, which
  are keyless ([per-entity vs global](#global-aggregation)).
- `features` — optional `list[str]` filter; omit to return all features.

<Tip>
**Cold-start returns `{}`** — an empty dict, not an error, not a 404. A
key with no events is just a key with no data, per the Redis-shaped
contract. `unknown_table` IS an error (raises `RegistrationError`).
</Tip>

### `app.batch_get(requests)`

Heterogeneous batch lookup. Equivalent to N parallel `get(...)` calls in
one round-trip; the response list preserves request order.

```python
app.batch_get([
    ("UserClicks", "alice"),                              # 2-tuple form
    ("UserClicks", "bob", ["visits"]),                    # 3-tuple form — feature filter
    ("AccountByCard", ["acct123", "card_v1"]),            # composite key
])
```

Each entry is a `(table, key)` 2-tuple OR a `(table, key, features)`
3-tuple where `features` is the same `list[str] | None` filter as in
`app.get`. Mix forms freely.

<Warning>
**No partial success.** If any single entry fails validation (bad
table, bad key shape), the entire batch returns one error envelope with
the offending index in `path` (e.g. `requests[2].table`). Re-issue
without the bad entry. Cap is 10 000 entries — exceeding raises
`batch_too_large`.
</Warning>

### `app.reset()`, `app.ping()`, `app.close()`

- `reset()` — wipe in-memory state + truncate WAL. Synchronous; the next
  push observes the cleared state. Server must have `test_mode` enabled
  (the production default rejects with `reset_disabled_in_production`).
- `ping()` — returns `{server_version, registry_version}`. Use
  `registry_version` as a cache key for schema-dependent client state.
- `close()` — idempotent. For embed-mode `App` instances, also terminates
  the subprocess (SIGTERM, then SIGKILL after 5 seconds).

## `@bv.event`

Declare an event source (push-shaped) or a derivation (chain on top of an
event source). Two forms:

### Class form — event source

```python
@bv.event
class Txn:
    user_id: str
    amount: float
    merchant: str
    ip: bv.Optional[str]                  # nullable
```

The class body declares the schema via type annotations. The 6-element
field-type vocabulary is shared across SDKs:

| Wire | Python |
|---|---|
| `str` | `str` |
| `i64` | `int` |
| `f64` | `float` |
| `bool` | `bool` |
| `bytes` | `bytes` |
| `datetime` | `datetime.datetime` |

Use `bv.Optional[T]` (NOT `typing.Optional[T]`) for nullable fields.

**Per-source kwargs:**

```python
@bv.event(
    keep_events_for="30d",        # event retention; default None (unbounded)
    cold_after="1d",              # cold-entity TTL; default None
    dedupe_key="trace_id",        # field used for idempotent replay
    dedupe_window="5m",           # dedup TTL
)
class Login:
    user_id: str
    device_id: str
    trace_id: str
```

| Kwarg | Type | Default | Behaviour |
|---|---|---|---|
| `keep_events_for` | duration | `None` | Event-retention TTL. Windowed ops still bound state on their windows independently. |
| `cold_after` | duration | `None` | Per-source cold-entity eviction TTL. Range `[1s, 365d]`; `"forever"` is rejected — use `None`. |
| `dedupe_key` | field name | `None` | Field used for idempotent-replay matching. Must be in the schema. |
| `dedupe_window` | duration | `None` | Dedup TTL — re-pushes within this window with matching `dedupe_key` are idempotent. |

<Note>
**Event-time is not supported in v0.** beava stamps wall-clock processing
time on every push. Declaring an `event_time` field (or passing
`tolerate_delay` / `event_time_field` kwargs) raises `TypeError` at
decoration time.
</Note>

### Function form — derivation

```python
@bv.event
def BigTxn(txn: Txn):
    return txn.filter(bv.col("amount") > 100)
```

The function form takes annotated parameters referencing upstream
`@bv.event`-decorated descriptors and returns a chain expression. The
decorator extracts the chain and registers it as a derivation node with
`output_kind=event`.

## `@bv.table`

Aggregation-output decorator. v0 has no `app.upsert` / `app.delete` /
`app.retract` — tables are populated **only** by upstream aggregations.

### Per-entity table

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

`key=` accepts a string OR a list of strings (composite key). The function
body MUST return `events.group_by(...).agg(...)`.

### Global aggregation

Omit the `key=` kwarg (or use `@bv.table` bare, with no parens) for a
**single-row, no-entity-dimension** aggregation:

```python
@bv.table
def TotalClicks(clicks: Click):
    return clicks.agg(total=bv.count(window="forever"))

app.get("TotalClicks")          # → {"total": N}, no entity arg
```

Three equivalent forms compile to the same wire shape (`key: []` on
register, `key: ""` sentinel on get):

```python
clicks.agg(total=bv.count(...))                  # shortest
clicks.group_by().agg(total=bv.count(...))       # explicit empty group_by
@bv.table                                        # decorator with no key=
def Foo(c): return c.agg(total=bv.count(...))
```

All 53 operators work with both per-entity and global aggregation. Use
cases: dashboards (global throughput, p95), anomaly detection on global
rates, cross-entity aggregations (total spend across all users).

`app.get` arity:

| Table type | Call | Cold-start |
|---|---|---|
| Per-entity | `app.get(table, key)` | `{}` |
| Global | `app.get(table)` | `{}` |

Mismatched arity raises `KeyError`.

### Where can `@bv.event` / `@bv.table` be declared?

The decorators resolve parameter annotations back to upstream class
objects. Resolution order:

1. **Module-level** (canonical, mypy-friendly) — `fn.__globals__`.
2. **Closure cells** (factory pattern) — `fn.__closure__` + `co_freevars`.
3. **Caller-frame `f_locals`** (test-fixture pattern) — walks outward
   from the decoration site by file identity (any frame outside
   `python/beava/_table.py` and `_events.py`); first-seen wins; bounded
   to 32 frames.

Lambdas are not supported (the resolver needs a real `def`). Names
imported via `from x import *` after the decorator runs aren't found.

## Pipeline DSL chain methods

Polars-style chain on event descriptors and derivations:

| Method | Returns | Description |
|---|---|---|
| `events.filter(expr)` | derivation | Keep rows where `expr` is True. |
| `events.select(*cols)` | derivation | Keep only the named fields. |
| `events.drop(*cols)` | derivation | Remove the named fields. |
| `events.rename(**mapping)` | derivation | Rename fields. |
| `events.with_columns(**exprs)` | derivation | Add or overwrite derived fields. |
| `events.map(**exprs)` | derivation | Alias for `with_columns`. |
| `events.cast(**type_map)` | derivation | Change field types (`{"str", "int", "float", "bool"}`). |
| `events.fillna(**defaults)` | derivation | Replace nulls with defaults. |
| `events.group_by(*keys)` | groupby | Start an aggregation. Empty for global. |
| `groupby.agg(**named_features)` | derivation | Compile to an aggregation node. |

Full ambiguity matrix and FORBIDDEN patterns:
[pipeline-dsl/compilation-rules](../pipeline-dsl/compilation-rules.md).

## Expression DSL — `bv.col` / `bv.lit`

```python
bv.col("amount") > 100                                 # comparison
bv.col("user_id") == "alice"                           # equality
(bv.col("amount") > 100) & (bv.col("status") == "ok")  # AND  — use & not `and`
(bv.col("amount") > 100) | (bv.col("status") == "ok")  # OR   — use | not `or`
~(bv.col("flag"))                                      # NOT  — use ~ not `not`
bv.col("amount").isnull()                              # null check
bv.col("status").cast("int")                           # type cast
bv.col("a") + bv.col("b") * 2                          # arithmetic
bv.lit(42)                                             # explicit literal
```

Python's `and` / `or` / `not` keywords cannot be overloaded — use the
bitwise symbols `&` / `|` / `~`. Operator precedence usually requires
parenthesising each comparison.

`bv.lit(value)` accepts `int | float | str | bool | None` and is useful
for constant columns and for forcing explicit literal coercion:

```python
events.with_columns(source=bv.lit("web"))                       # constant column
events.with_columns(rate=bv.col("count") / bv.lit(60.0))        # force float division
events.filter(bv.col("amount") > bv.lit(100))                   # explicit literal
```

The implicit operator-overloading coercion (`bv.col("x") > 100`) still
works; `bv.lit` is for the cases where explicit construction matters
(constant columns, type-coercion, cross-language parity with TS/Go which
lack Python's flexible operator overloading).

Full grammar and edge cases:
[pipeline-dsl/expressions](../pipeline-dsl/expressions.md).

### `bv.sum(field: str, ...)` accepts string column names only

```python
def sum(field: str, *, window: str | None = None, where: bv.Col | None = None) -> AggDescriptor: ...
```

<Warning>
**Inline expressions as the field arg are forbidden.** This locks the
signature at parity with TS / Go (which are communicate-only and don't
have an expression layer at all).

```python
bv.sum(bv.col("is_fraud").cast(int), window="1h")     # raises RegistrationError
bv.sum(bv.col("amount") * 2, window="1h")             # same
```
</Warning>

The canonical pattern for conditional counts is two-stage —
`with_columns` to derive the typed column, then `sum` on the derived
column:

```python
@bv.table(key="user_id")
def UserFraudCounts(txn: Txn):
    return (
        txn.with_columns(flag_int=bv.col("is_fraud").cast(int))
           .group_by("user_id")
           .agg(c=bv.sum("flag_int", window="1h"))
    )
```

## Operator catalog

The `bv.*` namespace exposes 53 operator helpers. Each returns an
`AggDescriptor` consumed by `groupby.agg(...)` to name the resulting
feature column. Names follow Polars conventions —  `mean` / `var` /
`std` / `n_unique` / `quantile` (not `avg` / `variance` / etc.).

<CardGroup cols={2}>
  <Card title="Core (8)" icon="hash" href="/operators/core/">
    `count`, `sum`, `mean`, `min`, `max`, `var`, `std`, `ratio`
  </Card>
  <Card title="Sketch (5)" icon="chart-bar" href="/operators/sketch/">
    `n_unique`, `quantile`, `top_k`, `bloom_member`, `entropy`
  </Card>
  <Card title="Point/ordinal (5)" icon="list-ol" href="/operators/point-ordinal/">
    `first`, `last`, `first_n`, `last_n`, `lag`
  </Card>
  <Card title="Recency (10)" icon="clock" href="/operators/recency/">
    `first_seen`, `last_seen`, `age`, `has_seen`, `time_since`, `time_since_last_n`, `streak`, `max_streak`, `negative_streak`, `first_seen_in_window`
  </Card>
  <Card title="Decay (6)" icon="wave-square" href="/operators/decay/">
    `ewma` (alias `ema`), `ewvar`, `ew_zscore`, `decayed_sum`, `decayed_count`, `twa`
  </Card>
  <Card title="Velocity (9)" icon="gauge-high" href="/operators/velocity/">
    `rate_of_change`, `inter_arrival_stats`, `burst_count`, `delta_from_prev`, `trend`, `trend_residual`, `outlier_count`, `value_change_count`, `z_score`
  </Card>
  <Card title="Buffer (7)" icon="layer-group" href="/operators/buffer-geo/">
    `histogram`, `hour_of_day_histogram`, `dow_hour_histogram`, `seasonal_deviation`, `event_type_mix`, `most_recent_n`, `reservoir_sample`
  </Card>
  <Card title="Geo (4)" icon="map-pin" href="/operators/buffer-geo/">
    `geo_velocity`, `geo_distance`, `geo_spread`, `distance_from_home`
  </Card>
</CardGroup>

### Deprecation aliases

Five renamed ops ship deprecation aliases that emit `DeprecationWarning`
and will be removed in v0.1:

| Canonical | Deprecated alias |
|---|---|
| `bv.mean` | `bv.avg` |
| `bv.var` | `bv.variance` |
| `bv.std` | `bv.stddev` |
| `bv.n_unique` | `bv.count_distinct` |
| `bv.quantile` | `bv.percentile` |

## Bundled demos — `bv.demo`

```python
demo = bv.demo("fraud")          # also: "adtech", "ecommerce"
# → {"name": "fraud", "schema": [<descriptors>], "events": [<events>]}

with bv.App() as app:
    app.register(*demo["schema"])
    for ev in demo["events"]:
        app.push(ev["event_name"], ev["fields"])
```

`bv.demo(name)` loads a bundled dataset shipped at
`python/beava/demos/<name>/{schema.json, events.jsonl}`. Returns
`{name, schema, events}`. Raises `ValueError` on unknown name (lists the
valid choices), `RuntimeError` if the bundled files are missing from
this install.

Useful for end-to-end smoke tests, reproductions, and live demos.

## Test fixtures — `beava.test`

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
    assert_features_eq(app.get("Counts", "bob"),   {"c": 1})
```

`beava.test.fixture(reset_each=True)`:

- Yields an embed-mode `App` (binary spawned on ephemeral ports).
- If `reset_each=True` (default), calls `app.reset()` between tests.
- Cleans up the subprocess on session teardown.

`assert_features_eq(got, want)` — assertion helper with helpful diff
output and float near-equality (relative tolerance `1e-9`) for sketch ops
like `quantile` and `n_unique`.

## Errors

```python
class RegistrationError(Exception):
    code: str                         # one of the structured error codes
    path: str                         # JSON-pointer-style path to the offending field
    message: str                      # human-readable explanation
    errors: list[ValidationError]     # all errors when the server returns multiple

class BinaryNotFoundError(Exception):
    """Raised by embed mode when the beava binary is not on PATH."""

@dataclass(frozen=True)
class ValidationError:
    kind: str                         # one of 9 frozen kinds
    path: str
    message: str
```

The 9 `ValidationError.kind` values are documented in
[shared.md § ValidationError envelope](shared.md#validationerror-envelope).
The full alphabetised structured-code list (with HTTP status mapping) is in
[error-codes.md](../error-codes.md).

Error message text follows a forward-looking framing — messages say "X is
not supported in v0", not "X has been removed". This avoids implying a
previous-version reference for users who never saw older revisions.

## Cross-language parity

The Python SDK is the canonical authoring UX. The TypeScript and Go SDKs
ship the **communicate** surface only — no decorators, no expression DSL,
no op helpers. Authoring flow:

1. Author the pipeline in Python via the DSL.
2. Compile to JSON (via `app.register_json(...)` or by serialising
   descriptors).
3. Ship that JSON to a TypeScript / Go application — they pass it through
   to `app.register(...)` verbatim.

See [shared.md](shared.md) for the cross-language contract: wire transports,
window grammar, key shape, error envelope, and the per-language signature
table.
