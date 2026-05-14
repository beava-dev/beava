# Trace — current Beava (v0), end-to-end

Companion to [`0001-trace-example.md`](./0001-trace-example.md), which
traces the **proposed** `@bv.expr` flow. This document traces the
**current** v0 surface only: `bv.event` + `bv.col` + `bv.lit` + the
aggregation helpers. No new IR nodes, no decorator, no AST rewriting
— just what ships today.

Reading both back-to-back is the cleanest way to see what the RFC
adds and what it leaves alone.

---

## 0. The user's source

```python
import beava as bv

@bv.event
class Purchase:
    user_id: str
    amount: float
    item:    str | None
    ts:      int

@bv.event
def UserStats(e: Purchase):
    e = e.with_columns(is_big = bv.col("amount") > 100.0)
    return e.group_by("user_id").agg(
        big_count_1h = bv.count(where=bv.col("is_big"),       window="1h"),
        total_24h    = bv.sum("amount",                       window="24h"),
        named_24h    = bv.count(where=bv.col("item").isnull() == False,
                                                              window="24h"),
    )

app = bv.App(events=[Purchase, UserStats])
app.register()
```

The feature computes, per `user_id`:

- count of purchases in the last hour where `amount > 100`,
- sum of `amount` in the last 24 h,
- count of purchases in the last 24 h whose `item` is not null
  (written via the round-about `isnull() == False` so the trace
  exercises the server-side `rewrite_null_eq` pass).

Everything below uses only existing files — no proposed files.

---

## 1. Pipeline overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              PYTHON PROCESS                                 │
│                                                                             │
│   ┌──────────────┐                              ┌─────────────────────┐     │
│   │  @bv.event   │                              │     @bv.event       │     │
│   │  (Purchase)  │                              │ def UserStats(e)    │     │
│   └──────┬───────┘                              └──────────┬──────────┘     │
│          │                                                 │                │
│          ▼                                                 ▼                │
│   EventSource                          _make_event_derivation(fn)           │
│   _schema={user_id, amount, ...}        1. inspect.signature(fn)            │
│   _chain=[]                             2. get_type_hints → {e: Purchase}   │
│   _kind="event_source"                  3. proxy = ClickProxy-of(Purchase)  │
│                                         4. call fn(proxy)                   │
│                                                 │                           │
│                                                 ▼  attribute access on      │
│                                                    proxy returns _Col(...)  │
│                                                 │                           │
│                                                 ▼  operator overloads build │
│                                                    _BinOp / _UnaryOp trees  │
│                                                 │                           │
│                                                 ▼  .with_columns / .agg     │
│                                                    each append a dict step  │
│                                                    to ._chain               │
│                                                                             │
│   ──────────────────────────────────────────────────────────                │
│   App.register()  →  walk events  →  for each derivation, serialise its     │
│   chain (each `_Expr` rendered via to_expr_string)  →  encode_frame(...)    │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │  TCP frame: OP_REGISTER (0x0001), CT_JSON (0x01)
┌──────────────────────────────────────▼──────────────────────────────────────┐
│                              RUST SERVER                                    │
│                                                                             │
│   register_validate.rs   parse each "expr-string" via expr::parse(s)        │
│            │                  ↳ runs Pass A (cast bare-ident normalise)     │
│            │                  ↳ runs Pass B (rewrite (x == null) and        │
│            │                                  (x != null) to isnull-based  │
│            │                                  forms — CLAUDE.md ‘things    │
│            │                                  that look like bugs but      │
│            │                                  are not’)                    │
│            ▼                                                                │
│   schema_propagate.rs    referenced_fields() resolved against the schema    │
│            │             available at each chain step                       │
│            ▼                                                                │
│   compile to op_node     agg_compile.rs builds windowed buckets             │
│                                                                             │
│   per-event apply path (mio-only, apply_shard.rs::dispatch_push_sync)       │
│   ─► eval(&expr, &row) for each predicate / derived column                  │
└─────────────────────────────────────────────────────────────────────────────┘
```

Everything that crosses the wire boundary is a **string** plus a
small surrounding JSON envelope. The server's `expr::parse`
(`crates/beava-core/src/expr.rs`) is the single point of authority
for that string grammar — the same parser that this RFC proposes to
extend.

---

## 2. `@bv.event class Purchase`

File: `python/beava/_events.py:208 _make_event_source`.

The decorator:

1. `_validate_class_event` rejects `event_time` field names and
   the `tolerate_delay` / `event_time_field` kwargs (v0
   `project_redis_shaped_no_event_time_ever` invariant).
2. `get_type_hints(cls, include_extras=True)` resolves the
   annotations — `str | None` becomes a real `typing.Optional[str]`
   at this point, not a string.
3. Constructs an `EventSource`:

   ```python
   src = EventSource(name="Purchase",
                     schema={"user_id": str, "amount": float,
                             "item": str | None, "ts": int},
                     keep_events_for=None, cold_after=None,
                     dedupe_key=None, dedupe_window=None)
   ```
4. **Mirrors** `_name`, `_schema`, `_chain=[]`, `_kind="event_source"`,
   etc. onto the class object itself (`setattr(cls, attr, ...)`).
   The class object now carries server-bound state.
5. Binds the chain methods (`filter`, `select`, `with_columns`,
   `group_by`, `agg`, `named`, …) as `staticmethod`s on the class
   so user code can write `Purchase.with_columns(...)` without
   instantiating anything.

State after decoration:

```python
Purchase._name   = "Purchase"
Purchase._kind   = "event_source"
Purchase._schema = {"user_id": str, "amount": float,
                    "item": str | None, "ts": int}
Purchase._chain  = []
Purchase.with_columns is <bound method of EventSource src>
```

Nothing has crossed the wire yet.

---

## 3. `bv.col` / `bv.lit` — the operator-overload DSL

File: `python/beava/_col.py`.

The whole DSL is built around a single base class `_Expr` (lines
27–111) whose dunder methods return new `_Expr` subclasses. Each
overload calls `_coerce(other)` to lift plain Python values into
`_Literal` so `bv.col("x") > 100` and `bv.col("x") > bv.lit(100)`
are interchangeable.

```
   _Expr  (abstract base; defines __add__ / __gt__ / __eq__ / __and__ / ...)
     ├── _Col(name)            ──► to_expr_string() = self.name
     ├── _Literal(value)       ──► repr(value), with special cases for None/bool/str
     ├── _BinOp(op, l, r)      ──► f"({l} {op} {r})"   # fully parenthesised
     ├── _UnaryOp(op, operand) ──► "isnull" → f"({x} == null)"
     │                             "~"      → f"!({x})"      ← see §3.1 below
     └── _CastOp(operand, t)   ──► f"cast({x}, {t})"
```

Three SDK-side specifics matter for the trace:

- **`bv.col("x") & y` emits the keyword `and`** on the wire, not
  `&` (`_col.py:80`, comment 76–79). Same for `|` → `or`. Python
  forbids overloading `and` / `or`, so the SDK overloads `&` / `|`
  and serialises them as the grammar tokens the parser
  understands.
- **`.isnull()` emits `(x == null)`**, not `isnull(x)`. The server
  rewrites it to `Call("isnull", [x])` at parse time (Pass B in
  `expr.rs:34`). This is the paired design CLAUDE.md flags under
  "things that look like bugs but are not."
- **`__eq__` returns an `_Expr`, not a `bool`**, which makes
  instances unhashable by default — every concrete subclass
  restores `__hash__` explicitly (e.g. `_col.py:121, 139, 152,
  168, 180`). That is *why* AST nodes can be dict / set keys.

### 3.1 — A note on `~` (`__invert__`)

`_col.py:92 __invert__` emits a wire string of the form `!(x)`.
But the server's lexer in `crates/beava-core/src/expr.rs:361–373`
**rejects a bare `!`**: only `!=` is a valid token, and a `!` not
followed by `=` returns

```
ParseError { col, reason: "col N: unexpected character '!'" }
```

So `~bv.col("x")` is currently a latent footgun: it serializes,
but the server cannot parse it. The grammar accepts the keyword
`not` (`expr.rs:463`), but `_col.py` never emits that keyword.
None of the v0 acceptance tests appear to exercise `~`. This trace
avoids it. (Worth filing as a separate issue.)

---

## 4. The expression tree for `bv.col("amount") > 100.0`

```
bv.col("amount")               →  _Col(name="amount")
bv.col("amount") > 100.0       →  _BinOp(op=">",
                                          left=_Col("amount"),
                                          right=_Literal(100.0))
                                  # via _Expr.__gt__ at _col.py:58,
                                  # _coerce(100.0) lifts the float to _Literal

.to_expr_string()              →  "(amount > 100.0)"
```

For the other two predicates:

```
bv.col("is_big")               →  _Col("is_big")
.to_expr_string()              →  "is_big"
                                  # bare ident: _Col.to_expr_string at _col.py:118

bv.col("item").isnull()        →  _UnaryOp(op="isnull", operand=_Col("item"))
                                  # via _Expr.isnull at _col.py:99
.to_expr_string()              →  "(item == null)"
                                  # _UnaryOp.to_expr_string at _col.py:161

bv.col("item").isnull() == False
                               →  _BinOp(op="==",
                                          left=_UnaryOp("isnull", _Col("item")),
                                          right=_Literal(False))
                                  # _Expr.__eq__ at _col.py:70
.to_expr_string()              →  "((item == null) == false)"
                                  # ← note the doubled-equality, this is on purpose;
                                  # see §6.2 for what the server makes of it.
```

---

## 5. `@bv.event def UserStats` — function form

File: `python/beava/_events.py:297 _make_event_derivation`.

The function-form decorator is structurally different from the
class form. It does:

1. `inspect.signature(fn)` → the parameter list. Empty signatures
   raise `TypeError` immediately.
2. **Type-hint resolution** with three lookup sources, in this
   priority order (this exists so function-local `@bv.event class`
   inside a pytest body resolves correctly under `from __future__
   import annotations`):
   1. `fn.__globals__`
   2. closure cells (`_collect_closure_cells_for_events`)
   3. caller-frame locals (`_collect_caller_frame_locals_for_events`)
3. Builds a *proxy* for each parameter. The proxy carries
   `_schema` from the resolved upstream class. Attribute access on
   the proxy (e.g. `e.amount`) returns `_Col("amount")`.
4. **Calls the function** with the proxy bound. The body runs once
   — at decoration time, not at registration time, not per event.
   This is the only moment the user's Python code executes.
5. The returned `EventDerivation` is tagged
   `_is_bv_event_function = True` (a marker used by
   `App.register` to reject ad-hoc chains that try to skip the
   decorator).

For our example, when `fn(proxy)` runs:

```
e.amount               →  _Col("amount")                       (via proxy __getattr__)
bv.col("amount") > 100.0
                       →  _BinOp(">", _Col("amount"), _Literal(100.0))

e.with_columns(is_big=...)
                       →  _ChainMixin.with_columns at _events.py:59
                          appends step =
                            {"op": "with_columns",
                             "exprs": {"is_big": "(amount > 100.0)"}}
                          to a new EventDerivation's _chain
                          (the .to_expr_string() call lives in
                           _events.py:65, inside the dict
                           comprehension)

e.group_by("user_id")  →  GroupBy(parent=<EventDerivation>, keys=("user_id",))
.agg(big_count_1h=..., total_24h=..., named_24h=...)
                       →  _events.py:162 GroupBy.agg
                          appends step =
                            {"op": "agg", "keys": ["user_id"],
                             "aggs": { ... AggDescriptor.to_dict() ... }}
```

Final `UserStats._chain`:

```python
[
  {"op": "with_columns",
   "exprs": {"is_big": "(amount > 100.0)"}},

  {"op": "agg",
   "keys": ["user_id"],
   "aggs": {
       "big_count_1h": {"op": "count", "window": "1h",
                        "where": "is_big"},
       "total_24h":    {"op": "sum",   "field": "amount",
                        "window": "24h"},
       "named_24h":    {"op": "count", "window": "24h",
                        "where": "((item == null) == false)"},
   }},
]
```

### 5.1 — How `bv.count` / `bv.sum` got there

Both helpers live in `python/beava/_agg.py`:

```
bv.count(where=bv.col("is_big"), window="1h")
   │
   ▼ _agg.py:117 count(...)
   │
   ├── _validate_window("1h", "count", required=False)        ─► OK
   ├── _serialize_where(bv.col("is_big"))                     ─► "is_big"
   │     (_agg.py:76; calls .to_expr_string() on the _Expr)
   ▼
   AggDescriptor(op="count", window="1h", where="is_big")

bv.sum("amount", window="24h")
   │
   ▼ _agg.py:123 sum(...)
   │
   ├── _enforce_field_str("amount", "sum")                    ─► OK
   │     (would raise RegistrationError(code="schema_mismatch")
   │      if a _Expr were passed — _agg.py:51)
   ├── _validate_window("24h", "sum", required=False)         ─► OK
   ▼
   AggDescriptor(op="sum", field="amount", window="24h", where=None)
```

`AggDescriptor.to_dict()` (`_agg.py:103`) renders the descriptor,
omitting keys that are `None`. That is why `"big_count_1h"` has no
`"field"` key and `"total_24h"` has no `"where"` key.

---

## 6. Register payload

`App.register()` walks the registered objects. For each, it builds
a JSON body and sends one frame per object via
`encode_frame(OP_REGISTER, CT_JSON, payload)`
(`_wire.py:78`).

### 6.1 — Wire frame envelope

```
┌──────────────┬──────────┬──────────────┬───────────────────────────┐
│ length (u32, │  op      │ content-type │ JSON payload              │
│ big-endian)  │  0x0001  │ 0x01         │ (UTF-8 bytes)             │
│              │ (REGISTER│ (JSON)       │                           │
│              │  )       │              │                           │
└──────────────┴──────────┴──────────────┴───────────────────────────┘
   4 bytes        2 bytes      1 byte         length − 3 bytes
```

`length` covers `op + content-type + payload`; the minimum valid
length is 3 (empty payload). All multi-byte integers are
big-endian. See `_wire.py:1-26` docstring.

### 6.2 — `UserStats` payload

```json
{
  "kind": "derivation",
  "name": "UserStats",
  "output_kind": "event",
  "upstream": ["Purchase"],
  "chain": [
    {
      "op": "with_columns",
      "exprs": {
        "is_big": "(amount > 100.0)"
      }
    },
    {
      "op": "agg",
      "keys": ["user_id"],
      "aggs": {
        "big_count_1h": {"op": "count", "window": "1h",
                         "where": "is_big"},
        "total_24h":    {"op": "sum",   "field": "amount",
                         "window": "24h"},
        "named_24h":    {"op": "count", "window": "24h",
                         "where": "((item == null) == false)"}
      }
    }
  ]
}
```

The `Purchase` payload is simpler:

```json
{
  "kind": "event",
  "name": "Purchase",
  "schema": {"user_id": "str", "amount": "float",
             "item": "str?", "ts": "int"}
}
```

(The locked invariant `events-only-at-top-level` — see
`SOURCE-OF-TRUTH.md` — means `Purchase`'s `kind` is `"event"`, not
`"table"`. `UserStats` is `kind: "derivation", output_kind: "event"`.
`kind: "table"` at the top level is rejected with
`unsupported_node_kind`.)

---

## 7. Server-side parse

For each expression string, the server calls
`crates/beava-core/src/expr.rs::parse(s) -> Result<Expr, ParseError>`.
Three of our four strings exercise different parts of the parser.

### 7.1 — `"(amount > 100.0)"`

Straightforward parenthesised binop. The parser produces:

```rust
Expr::BinOp {
  op:    ">".into(),
  left:  Box::new(Expr::Field { name: "amount".into(),
                                 span: Span { start: 1, end: 7 } }),
  right: Box::new(Expr::Literal(Literal::Float(100.0),
                                 Span { start: 10, end: 15 })),
  span:  Span { start: 0, end: 16 },
}
```

The `paren_depth > 0` gate on infix operators (`expr.rs:628 parse_or`
and downward) is satisfied by the outer `(...)`. Same gate is why
the SDK is *required* to emit full parens.

### 7.2 — `"is_big"`

Bare ident. Parses to:

```rust
Expr::Field { name: "is_big".into(), span: Span { start: 0, end: 6 } }
```

This is the trivial path. `referenced_fields()` for this AST is
`{"is_big"}`; schema propagation (next section) verifies the name
exists at the `where=` step's schema.

### 7.3 — `"((item == null) == false)"` — the rewrite chain

This is where it gets interesting. The lexer / parser produces
the naïve nested-binop AST first:

```rust
Expr::BinOp {
  op: "==",
  left:  Box::new(Expr::BinOp {
    op: "==",
    left:  Box::new(Expr::Field { name: "item", .. }),
    right: Box::new(Expr::Literal(Literal::Null, ..)),
    span: ..,
  }),
  right: Box::new(Expr::Literal(Literal::Bool(false), ..)),
  span: ..,
}
```

Then **Pass B** runs bottom-up over the AST (`expr.rs:996+,
1041, 1053` — search for `rewrite_null_eq`). It looks for any
`BinOp("==", _, Literal::Null)` (in either argument order) and
rewrites it to `Call("isnull", [_])`. Walking our tree bottom-up:

1. Inner subtree `(item == null)` matches the pattern. Rewritten:
   ```
   BinOp("==", Field("item"), Literal::Null)
   ────►  Call("isnull", [Field("item")])
   ```
2. Outer subtree is `BinOp("==", Call("isnull", [...]), Literal::Bool(false))`.
   The RHS is `Bool(false)`, not `Null`, so Pass B does **not**
   rewrite it. (Pass B is null-equality-specific; it does not
   touch `== false`.)

Final AST:

```rust
Expr::BinOp {
  op: "==",
  left:  Box::new(Expr::Call {
    fn_name: "isnull".into(),
    args:    vec![ Expr::Field { name: "item", .. } ],
    span:    ..,
  }),
  right: Box::new(Expr::Literal(Literal::Bool(false), ..)),
  span:  ..,
}
```

**Why the rewrite exists** (CLAUDE.md "things to never do" #2 +
expr.rs:21–43 doc comment): the evaluator's `BinOp("==")` branch
is **strict-null** — if either operand is `Null`, it returns
`Null`. Without Pass B, `(item == null)` would itself return
`Null` whenever `item` is `null`, which silently swallows the
truth value the user wanted. The rewrite converts the user's
"is this null?" intent into a `Call("isnull", ...)`, which the
builtins table (`expr_builtins.rs`) guarantees returns
`Bool(true / false)` and never `Null`. The strict-null guard and
the Pass-B rewrite are a paired design — touching one without the
other breaks user code.

### 7.4 — Schema propagation

`schema_propagate.rs` walks the chain and tracks the schema at
each step:

```
Step 0 (Purchase base):
        schema = {user_id: str, amount: float, item: str?, ts: int}

Step 1 (with_columns):
        parse_expr("(amount > 100.0)")            → Expr::BinOp
        referenced_fields()                       → {"amount"}
        "amount" ∈ schema  ✓
        infer return type of (amount > 100.0)     → Bool
        schema' = schema ∪ {is_big: Bool}

Step 2 (agg):
        for each agg.where expression:
          parse, referenced_fields(), check ⊆ schema'

          "is_big"                       → {"is_big"}  ✓
          "((item == null) == false)"    → {"item"}    ✓
                                            (no extra fields after Pass B
                                             since Literal::Null and
                                             Literal::Bool(false) are excluded
                                             by collect_fields)
        for sum's field="amount":
          "amount" ∈ schema'  ✓
        keys=["user_id"]:    "user_id" ∈ schema'  ✓
```

Any failure here returns a structured `RegistrationError` over the
wire before any data flows — register-time errors are loud.

---

## 8. Apply path — evaluating against a row

`crates/beava-server/src/apply_shard.rs::dispatch_push_sync`
receives a `Purchase` event on the mio data plane. (`mio-only data
plane` is one of the locked invariants — axum is admin-only.)

For a sample event

```
Purchase { user_id="u1", amount=250.0, item="book", ts=1700000000 }
```

the row representation is

```rust
Row {
  user_id: Value::Str("u1".into()),
  amount:  Value::F64(250.0),
  item:    Value::Str("book".into()),
  ts:      Value::I64(1700000000),
}
```

### 8.1 — `with_columns` step

`eval(&expr_is_big, &row)` with `expr_is_big = BinOp(">", Field("amount"),
Literal::Float(100.0))`:

```
eval(BinOp(">", Field("amount"), Literal::Float(100.0)))
 └─ delegate to eval_binop                                     (eval.rs:88)
     ├─ left  = eval(Field("amount"))    = Value::F64(250.0)   (eval.rs:66)
     ├─ right = eval(Literal::Float)     = Value::F64(100.0)   (eval.rs:73)
     └─ op == ">" → cmp_op
        ├─ both F64; no promotion
        ├─ NaN check: neither operand is NaN  → no special-case
        └─ 250.0 > 100.0  → Value::Bool(true)
```

The row is extended with `is_big = Value::Bool(true)` and flows
into the agg step.

### 8.2 — `agg.where` predicates

Per the locked agg-where semantics in `agg_where.rs`, each
`where=` expression is evaluated per-event against the post-with_columns
row; the event is included in the agg's input iff the predicate
evaluates to `Value::Bool(true)`. Any other result (`false`, `null`,
non-bool) excludes the event — the agg path treats `null` as "not
included," matching SQL `WHERE` semantics.

For our row:

- **`big_count_1h.where = "is_big"`**
  ```
  eval(Field("is_big"))   = Value::Bool(true)    → included
  ```

- **`named_24h.where = "((item == null) == false)"`**
  Recall Pass B rewrote this at parse time to:
  ```
  BinOp("==", Call("isnull", [Field("item")]), Literal::Bool(false))
  ```
  Evaluating:
  ```
  eval(Call("isnull", [Field("item")]))                       (eval.rs:93–101)
    ├─ args = [eval(Field("item"))]      = [Value::Str("book")]
    └─ lookup_builtin("isnull").eval(&[Str(...)])
                                          = Value::Bool(false)
                                          (isnull is the only builtin
                                           that always returns Bool,
                                           never Null — expr_builtins.rs)

  eval(BinOp("==", Bool(false), Literal::Bool(false)))
    ├─ left  = Value::Bool(false)
    ├─ right = Value::Bool(false)
    └─ op == "==" → strict-null guard passes (no operand is Null)
                  → cmp_eq(Bool(false), Bool(false)) = Value::Bool(true)
                                                            → included
  ```

  For a counterfactual row with `item = None`:
  ```
  eval(Call("isnull", [Field("item")]))
    └─ args = [Value::Null]
    └─ isnull(Null) = Value::Bool(true)

  eval(BinOp("==", Bool(true), Literal::Bool(false)))
    └─ cmp_eq(true, false) = Value::Bool(false)              → excluded
  ```

  Symmetry preserved, no silent-Null swallow. The Pass-B rewrite
  paid off.

### 8.3 — `sum.field`

`bv.sum("amount", ...)` does not have a `where=`. The agg compiler
in `agg_compile.rs` builds a windowed bucket keyed on `user_id`;
`agg_apply.rs` reads `row.amount` directly (no expression eval —
the `field` form short-circuits) and adds it to the bucket's
running sum. For our row the bucket's `total_24h` for `u1` gains
`250.0`.

---

## 9. End-to-end summary diagram

```
              SOURCE                                          WIRE                       SERVER
              ──────                                          ────                       ──────

  @bv.event class Purchase ──────────► EventSource(schema)  ──► OP_REGISTER (kind:event)

  @bv.event def UserStats:
    body executes once at decoration time
        e.amount                 → _Col("amount")
        bv.col(...) > 100.0      → _BinOp(">", _Col, _Literal(100.0))
        bv.col("item").isnull()  → _UnaryOp("isnull", _Col("item"))
                                   .to_expr_string()
                                   ▼
                            "(item == null)"  ← still a string,
                                                rewritten on the server

  .with_columns / .agg appends chain dicts
        ─► UserStats._chain = [ {op: with_columns, exprs: {...}},
                                {op: agg,          aggs: {...}} ]

  bv.count / bv.sum                AggDescriptor.to_dict()
        ─► {op: ..., window: ..., where: <expr-string>, field: ...}
                                                                │
                                                                ▼
  App.register()  ──────────────────────────────────────► OP_REGISTER frame, CT_JSON
                                                                │
                                                                ▼
                                                       register_validate.rs
                                                          │ expr::parse  per string
                                                          │   ├─ Pass A: cast bare-ident
                                                          │   └─ Pass B: rewrite_null_eq
                                                          │              ((x == null) → isnull(x))
                                                          │              ((x != null) → not isnull(x))
                                                          ▼
                                                       Expr ASTs +
                                                       schema_propagate.rs ✓
                                                          ▼
                                                       agg_compile.rs builds windowed state

                              per-event PUSH (OP_PUSH 0x0010)
                              row = Purchase { amount=250.0, item="book", ... }
                                                                │
                                                                ▼
                                                  apply_shard.rs::dispatch_push_sync
                                                          │
                                                          ▼ for each derived col / where:
                                                       eval(&Expr, &Row)
                                                          ├─ Field("amount") → F64(250.0)
                                                          ├─ Literal::Float → F64(100.0)
                                                          ├─ ">" → Bool(true)
                                                          ├─ Call("isnull", [Str]) → Bool(false)
                                                          └─ "==" Bool(false) Bool(false) → Bool(true)
                                                          ▼
                                                       agg buckets updated:
                                                         u1:  big_count_1h += 1
                                                              total_24h    += 250.0
                                                              named_24h    += 1
```

---

## 10. Where today's surface hits a wall (motivating the RFC)

This trace covers every shape the v0 DSL can express. The
canonical-example features from issue #56 that this trace **cannot**
mechanically reproduce:

- **`if email is None: return None / else: ...`** — there is no
  `IfElse` IR node. The closest workaround is two `with_columns`
  steps + a `where=` filter, which moves the branch out of the
  expression.
- **`if dwell_ms < 1000: 0 elif < 10000: 1 elif < 60000: 2 else: 3`**
  — multi-way branching. No `IfElse`, no `match`, and chaining
  binop predicates does not give an integer bucket as a return
  value.
- **`0 < dwell_ms < 60_000`** — Python chained comparison. The
  current grammar's `CmpExpr := AddExpr (op AddExpr)?` accepts at
  most **one** relational operator, not a chain (`expr.rs:11`).
  Users today must write `(bv.col("dwell_ms") > 0) & (bv.col("dwell_ms")
  < 60000)`, which loses the 3VL guarantee on the middle operand.
- **Intermediate names inside one feature.** `parts = email.split("@")
  ... parts[1]` has no `let`-binding equivalent in the IR.
- **Calling `bv.col(...)` on a derived column from inside another
  function.** Today, every derivation chain step is dict-shaped and
  the only composition is left-to-right via `with_columns(...)`
  followed by another `with_columns(...)`.

These five gaps are exactly what RFC 0001 adds:

- `Expr::IfElse` → branches.
- `Expr::Compare` → chained comparisons.
- `Expr::LetBinding` → intermediate names.
- `@bv.expr` → composition / closure capture, with type
  inference at decoration time.
- 3VL truth tables consolidated and documented so the new nodes
  behave consistently with the existing ops.

The rest of the pipeline — wire frame, `App.register`, schema
propagation, `agg_compile`, `dispatch_push_sync`, the eval entry
point — is untouched. That is the value of doing the design as an
**expression-engine extension** rather than as a parallel
parser / IR.

---

## File references (current code, all real)

| Step                              | File                                                                |
|-----------------------------------|---------------------------------------------------------------------|
| `@bv.event` class form            | `python/beava/_events.py:208 _make_event_source`                    |
| `@bv.event` function form         | `python/beava/_events.py:297 _make_event_derivation`                |
| `_ChainMixin` (with_columns, agg) | `python/beava/_events.py:38`                                        |
| `GroupBy.agg`                     | `python/beava/_events.py:162`                                       |
| `bv.col` / `bv.lit`               | `python/beava/_col.py:189 col`, `:202 lit`                          |
| `_Expr` operator overloads        | `python/beava/_col.py:27–111`                                       |
| `_BinOp.to_expr_string`           | `python/beava/_col.py:149`                                          |
| `_UnaryOp.to_expr_string`         | `python/beava/_col.py:161`                                          |
| `AggDescriptor`                   | `python/beava/_agg.py:87`                                           |
| `bv.count` / `bv.sum`             | `python/beava/_agg.py:117`, `:123`                                  |
| `_enforce_field_str`              | `python/beava/_agg.py:51`                                           |
| `_serialize_where`                | `python/beava/_agg.py:76`                                           |
| Wire frame envelope               | `python/beava/_wire.py:1-26` (docstring), `:78 encode_frame`        |
| Expression grammar                | `crates/beava-core/src/expr.rs:1-43` (top-of-file)                  |
| Lexer (rejects bare `!`)          | `crates/beava-core/src/expr.rs:361`                                 |
| `rewrite_null_eq` (Pass B)        | `crates/beava-core/src/expr.rs:996, 1041, 1053`                     |
| Evaluator entry                   | `crates/beava-core/src/eval.rs:56 eval`                             |
| 3VL helpers                       | `Value::*_three_valued` (`row.rs`)                                  |
| `BUILTINS` table (`isnull`, …)    | `crates/beava-core/src/expr_builtins.rs`                            |
| Schema propagation                | `crates/beava-core/src/schema_propagate.rs`                         |
| Agg compile                       | `crates/beava-core/src/agg_compile.rs`                              |
| Agg apply / where                 | `crates/beava-core/src/agg_apply.rs`, `agg_where.rs`                |
| Apply path                        | `crates/beava-server/src/apply_shard.rs::dispatch_push_sync`        |
