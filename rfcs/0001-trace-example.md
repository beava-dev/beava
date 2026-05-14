# RFC 0001 — End-to-end trace: `@bv.expr` → wire IR → evaluator

Companion to [`0001-bv-expr-symbolic-frontend.md`](./0001-bv-expr-symbolic-frontend.md).

Concrete walk-through of one feature definition, from the user's
Python source through every layer the RFC describes, to a server-side
evaluator call. Every code reference points at a real file in the
repo, and every wire string in this document is what the existing
parser would actually parse — including the new productions the RFC
adds.

The example uses `dwell_bucket` from the issue's canonical sample —
small enough to fit on a page, exercises the three new IR nodes
(`IfElse`, `LetBinding` indirectly, `Compare` indirectly through n=1
`BinOp`), and feeds into a `with_columns` + `agg(where=...)` chain
that already works in v0.

---

## 0. The user's source

```python
import beava as bv

@bv.event
class Click:
    user_id: str
    email: str | None
    dwell_ms: int
    ts: int

@bv.expr
def dwell_bucket(dwell_ms: int) -> int:
    if   dwell_ms < 1_000:   return 0
    elif dwell_ms < 10_000:  return 1
    elif dwell_ms < 60_000:  return 2
    else:                    return 3

@bv.event
def ClickFeatures(e: Click):
    e = e.with_columns(dwell_bkt=dwell_bucket(e.dwell_ms))
    return e.group_by("user_id").agg(
        clicks_24h    = bv.count(window="24h"),
        deep_count_1h = bv.count(where=bv.col("dwell_bkt") == 3, window="1h"),
    )

app = bv.App(events=[Click, ClickFeatures])
app.register()
```

The trace below follows what happens when this script runs.

---

## 1. Pipeline overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              PYTHON PROCESS                                 │
│                                                                             │
│   ┌──────────────┐    ┌──────────────┐    ┌──────────────┐                  │
│   │  @bv.event   │    │   @bv.expr   │    │  ClickFeats  │                  │
│   │   (Click)    │    │ dwell_bucket │    │  (function)  │                  │
│   └──────┬───────┘    └──────┬───────┘    └──────┬───────┘                  │
│          │                   │                   │                          │
│          ▼                   ▼                   ▼                          │
│   EventSource          decorator wrapper    EventDerivation                 │
│   _schema={...}        + cached IR          _chain=[                        │
│   _chain=[]            (__beava_expr_ir__)    {op:"with_columns",           │
│                                                exprs:{"dwell_bkt":"…"}},   │
│                                               {op:"agg",                    │
│                                                aggs:{...}} ]                │
│                                                                             │
│   ──────────────────────────────────────────────────────────                │
│   App.register()  →  build register payload  →  encode_frame(OP_REGISTER)   │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │  TCP wire (length-prefixed frame)
┌──────────────────────────────────────▼──────────────────────────────────────┐
│                              RUST SERVER                                    │
│                                                                             │
│   register_validate.rs  →  registry.rs (canonicalize)  →  schema_propagate  │
│                                       │                                     │
│                                       ▼                                     │
│                              expr::parse(string)  ──►  Expr AST             │
│                                       │                                     │
│                                       ▼                                     │
│                              eval::eval(&expr, &row)  ──►  Value            │
└─────────────────────────────────────────────────────────────────────────────┘
```

The wire boundary carries **strings**, not pre-parsed JSON ASTs. The
register payload embeds `"dwell_bkt": "<expression-string>"`; the
server's `expr::parse` (`crates/beava-core/src/expr.rs`) is the
single point of authority for understanding those strings.

---

## 2. `@bv.event class Click` — what exists today

File: `python/beava/_events.py:208 _make_event_source`.

The decorator:

1. Reads type hints via `get_type_hints(cls)`.
2. Rejects `event_time` field names (`_FORBIDDEN_FIELD_NAMES`).
3. Constructs `EventSource(name="Click", schema={...}, chain=[])`.
4. Mirrors `_name`, `_schema`, `_chain`, `_kind="event_source"`
   onto the class so `Click.with_columns(...)` works statically.
5. Binds the chain methods as `staticmethod` on the class.

After decoration, `Click` carries:

```python
Click._name   = "Click"
Click._kind   = "event_source"
Click._schema = {"user_id": str, "email": str | None,
                 "dwell_ms": int, "ts": int}
Click._chain  = []
```

Nothing has been sent to the server yet — `@bv.event` is a pure
local construction.

---

## 3. `@bv.expr def dwell_bucket` — Layers 1–3 of the RFC

The decorator is invoked with `fn = dwell_bucket`. Per RFC §Design
Layer 1, six things happen in order.

### Step 3.1 — Source recovery (Layer 1, step 1)

```python
src = inspect.getsource(fn)
mod = ast.parse(src)
```

`mod` is a standard `ast.Module` whose body is one `ast.FunctionDef`:

```
FunctionDef(
  name='dwell_bucket',
  args=arguments(args=[arg(arg='dwell_ms', annotation=Name(id='int'))]),
  body=[
    If(test=Compare(Name('dwell_ms'), [Lt()], [Constant(1000)]),
       body=[Return(Constant(0))],
       orelse=[
         If(test=Compare(Name('dwell_ms'), [Lt()], [Constant(10000)]),
            body=[Return(Constant(1))],
            orelse=[
              If(test=Compare(Name('dwell_ms'), [Lt()], [Constant(60000)]),
                 body=[Return(Constant(2))],
                 orelse=[Return(Constant(3))]),
            ]),
       ]),
  ],
  returns=Name('int'))
```

### Step 3.2 — Schema-bound argument typing (Layer 1, step 2)

For the parameter `dwell_ms: int`, the decorator binds:

```python
sym_args = {
    "dwell_ms": _SymCol(name="dwell_ms", type=I64, nullable=False)
}
```

(The annotation is `int`, not `int | None`, so `nullable=False`.)

### Step 3.3 — AST rewrite (Layer 2)

The `ast.NodeTransformer` in `_expr_ast.py` walks the body. Each
`ast.If`/`ast.Return` pair lowers to `_SymIfElse`; the `Compare`
(n=1) lowers to `_SymBinOp` (per the n=1 collapse rule in §Layer 5
of the RFC). The rewriter produces this expression tree:

```
_SymIfElse(
    cond=_SymBinOp("<", _SymCol("dwell_ms"), _SymLit(1000)),
    then_branch=_SymLit(0),
    else_branch=_SymIfElse(
        cond=_SymBinOp("<", _SymCol("dwell_ms"), _SymLit(10000)),
        then_branch=_SymLit(1),
        else_branch=_SymIfElse(
            cond=_SymBinOp("<", _SymCol("dwell_ms"), _SymLit(60000)),
            then_branch=_SymLit(2),
            else_branch=_SymLit(3))))
```

### Step 3.4 — Symbolic execution (Layer 1, step 4 → Layer 3)

The rewriter compiled the module; `exec`ing it with `dwell_ms`
bound to the `_SymCol` produced above runs the operator overloads
on `_SymbolicCol` (Layer 3, `python/beava/_expr_tracer.py`). Every
`<` becomes `_SymBinOp("<", ...)`, every `if/elif/else` becomes a
nested `_SymIfElse`. The return value is the tree above.

### Step 3.5 — Type inference (Layer 1, step 5)

Bottom-up:

```
_SymLit(0..3)                     → I64
_SymBinOp("<", I64, I64)          → Bool (non-nullable, both sides non-null)
_SymIfElse(Bool, I64, I64)        → I64 (join of branches)
_SymIfElse(Bool, I64, I64)        → I64
_SymIfElse(Bool, I64, I64)        → I64
```

Result type: `I64`. Matches the `-> int` annotation. If the
annotation disagreed, the decorator raises `RegistrationError(code=
"return_type_mismatch")`.

### Step 3.6 — `to_expr_string()` (Layer 3)

Walking the tree:

```
_SymLit(1000).to_expr_string()
  → "1000"

_SymBinOp("<", _SymCol("dwell_ms"), _SymLit(1000)).to_expr_string()
  → "(dwell_ms < 1000)"
  (matches _col.py:_BinOp.to_expr_string — full parens, required by
   the parser's paren_depth > 0 gate)

_SymIfElse(c, t, e).to_expr_string()
  → f"(if {c.to_expr_string()} then {t.to_expr_string()} else {e.to_expr_string()})"
```

Final wire string (re-flowed for readability — emitted as one line):

```text
(if (dwell_ms < 1000)
   then 0
   else (if (dwell_ms < 10000)
            then 1
            else (if (dwell_ms < 60000)
                     then 2
                     else 3)))
```

### Step 3.7 — IR cache (Layer 1, step 6)

```python
dwell_bucket.__beava_expr_ir__ = {
    "tree":       <_SymIfElse ...>,
    "wire":       "(if (dwell_ms < 1000) then 0 else (if (dwell_ms < ...",
    "return_type": "I64",
    "params":     ["dwell_ms"],
}
```

Done with the decorator. `dwell_bucket` is now a callable wrapper.

---

## 4. `ClickFeatures` — calling the decorated function

```python
@bv.event
def ClickFeatures(e: Click):
    e = e.with_columns(dwell_bkt=dwell_bucket(e.dwell_ms))
    ...
```

`@bv.event` (function form) is in `python/beava/_events.py:297
_make_event_derivation`. The flow:

```
       ┌─────────────────────────────────────────────────────────────┐
       │   _make_event_derivation(ClickFeatures)                     │
       │                                                             │
       │   1. inspect.signature(fn)         → e: Click               │
       │   2. resolve_type_hints            → e ↦ Click              │
       │   3. build proxy = ClickProxy(...)  (carries _schema)       │
       │   4. call fn(proxy)                                         │
       └────────────────────────┬────────────────────────────────────┘
                                ▼
                fn body runs with `e` = proxy whose
                attribute access returns a `_Col`
                                │
                                ▼
       e.dwell_ms     →  _Col("dwell_ms")            (a regular _Expr)
                                │
                                ▼ wrapped at call site
       dwell_bucket(e.dwell_ms)  ──► wrapper sees _SymbolicCol-shaped arg
                                    inlines cached IR with
                                    dwell_ms ↦ _Col("dwell_ms")
                                │
                                ▼
       returns:   _SymIfElse(cond=_BinOp("<", _Col("dwell_ms"), _Literal(1000)),
                             then_branch=_Literal(0),
                             else_branch=_SymIfElse(...))
                                │
                                ▼
       e.with_columns(dwell_bkt=...) — _Events.py:59:
         step = {"op": "with_columns",
                 "exprs": {"dwell_bkt": <ir>.to_expr_string()}}
```

The key detail: when the decorated `dwell_bucket` wrapper receives
`_Col("dwell_ms")` (a normal `_col.py` node — `_Col` *is* a
`_SymbolicCol` for the purposes of operator overloads), it inlines
its cached IR with the parameter substituted. The substitution
walks `__beava_expr_ir__["tree"]` and replaces every
`_SymCol("dwell_ms")` with the caller-supplied `_Col("dwell_ms")`.
Since they serialize identically, the resulting wire string is the
same as Step 3.6 above.

After `with_columns`:

```python
ClickFeatures._chain = [
    {"op": "with_columns",
     "exprs": {"dwell_bkt": "(if (dwell_ms < 1000) then 0 else (if ...))"}}
]
```

Then `.group_by("user_id").agg(...)` adds a second chain step.

### 4.1 — The `where=` slice on `bv.count`

```python
bv.count(where=bv.col("dwell_bkt") == 3, window="1h")
```

This path is entirely `_col.py` + `_agg.py` — no `@bv.expr`
involvement. Concretely:

```
bv.col("dwell_bkt")          → _Col("dwell_bkt")
_Col("dwell_bkt") == 3       → _BinOp("==", _Col("dwell_bkt"), _Literal(3))
                               (_Expr.__eq__ at _col.py:70 + _coerce)
.to_expr_string()            → "(dwell_bkt == 3)"
bv.count(where=..., window="1h")
  → AggDescriptor(op="count", window="1h", where="(dwell_bkt == 3)")
  → to_dict() emits {"op": "count", "window": "1h",
                     "where": "(dwell_bkt == 3)"}
```

`_agg.py:76 _serialize_where` is the single hop: it calls
`to_expr_string()` on the `_Expr` and stores the string. The
server is now back to parsing strings.

---

## 5. The register payload

`App.register()` traverses the registered objects and emits a
single `OP_REGISTER` frame (`_wire.py:37 OP_REGISTER = 0x0001`,
`CT_JSON = 0x01`). Frame body for `ClickFeatures` is JSON:

```json
{
  "kind": "derivation",
  "name": "ClickFeatures",
  "output_kind": "event",
  "upstream": ["Click"],
  "chain": [
    {
      "op": "with_columns",
      "exprs": {
        "dwell_bkt": "(if (dwell_ms < 1000) then 0 else (if (dwell_ms < 10000) then 1 else (if (dwell_ms < 60000) then 2 else 3)))"
      }
    },
    {
      "op": "agg",
      "keys": ["user_id"],
      "aggs": {
        "clicks_24h":    {"op": "count", "window": "24h"},
        "deep_count_1h": {"op": "count", "window": "1h",
                          "where": "(dwell_bkt == 3)"}
      }
    }
  ]
}
```

`Click` is registered as `{"kind": "event", "name": "Click",
"schema": {...}}` (the v0 events-only-at-top-level rule from
SOURCE-OF-TRUTH applies — derivations carry `output_kind` instead).

---

## 6. Server side — parser

The server's `register_validate.rs` reads the chain. For each
`with_columns.exprs.<name>` value, it calls
`expr::parse(s) -> Result<Expr, ParseError>` from
`crates/beava-core/src/expr.rs`.

For the `dwell_bkt` string, the parser produces (with this RFC's
new `IfElse` variant):

```rust
Expr::IfElse {
  cond: Box::new(Expr::BinOp {
    op: "<".into(),
    left:  Box::new(Expr::Field { name: "dwell_ms".into(), span: 4..12 }),
    right: Box::new(Expr::Literal(Literal::Int(1000), 15..19)),
    span:  4..19,
  }),
  then_branch: Box::new(Expr::Literal(Literal::Int(0), 26..27)),
  else_branch: Box::new(Expr::IfElse {
    cond: Box::new(Expr::BinOp { op: "<".into(),
      left:  /* Field("dwell_ms") */ ...,
      right: Box::new(Expr::Literal(Literal::Int(10000), ...)),
      span:  ... }),
    then_branch: Box::new(Expr::Literal(Literal::Int(1), ...)),
    else_branch: Box::new(Expr::IfElse {
      cond: /* (dwell_ms < 60000) */ ...,
      then_branch: Box::new(Expr::Literal(Literal::Int(2), ...)),
      else_branch: Box::new(Expr::Literal(Literal::Int(3), ...)),
      span: ...,
    }),
    span: ...,
  }),
  span: 0..N,
}
```

For the `where` string `(dwell_bkt == 3)`, the parser produces a
plain `Expr::BinOp` — and then the post-parse `rewrite_null_eq`
pass (`expr.rs:34 Pass B`) inspects it: since the RHS is
`Literal::Int(3)` (not `Literal::Null`), no rewrite fires. The
final AST is:

```rust
Expr::BinOp {
  op: "==",
  left:  Expr::Field { name: "dwell_bkt", span: ... },
  right: Expr::Literal(Literal::Int(3), ...),
  span: ...,
}
```

`Expr::referenced_fields()` on the `dwell_bkt` derivation returns
`{"dwell_ms"}`; on the `where` it returns `{"dwell_bkt"}`. Schema
propagation (`schema_propagate.rs`) verifies each name resolves
against the schema available at that chain step:

```
   step 0  (Click base):     schema = {user_id, email, dwell_ms, ts}
                              dwell_ms ∈ schema ✓
   step 1  (with_columns):    schema = base ∪ {dwell_bkt: I64}
   step 2  (agg.where):       schema = step-1 schema
                              dwell_bkt ∈ schema ✓
```

---

## 7. Apply path — evaluating against a row

For a sample event `Click(user_id="u1", email=None, dwell_ms=42000,
ts=...)`, `apply_shard.rs::dispatch_push_sync` calls into the
expression evaluator (`crates/beava-core/src/eval.rs:56 eval`)
for each `with_columns` derivation. The Row is:

```rust
Row {
  user_id: Value::Str("u1"),
  email:   Value::Null,
  dwell_ms: Value::I64(42000),
  ts:       Value::I64(...),
}
```

Tracing `eval(&dwell_bkt_expr, &row)` with the new `IfElse` arm:

```
eval(IfElse { cond, then, else })
 └─ cv = eval(cond = BinOp("<", Field("dwell_ms"), Literal::Int(1000)))
     ├─ left  = eval(Field("dwell_ms"))   = Value::I64(42000)   (eval.rs:66)
     ├─ right = eval(Literal::Int(1000))  = Value::I64(1000)    (eval.rs:72)
     └─ "<"  → cmp_op(Lt, 42000, 1000)    = Value::Bool(false)
 └─ cv = Bool(false) → recurse into else_branch

eval(IfElse { cond=(dwell_ms < 10000), then=Lit(1), else=... })
 └─ cv = Bool(false) again (42000 < 10000 is false)
 └─ recurse into else_branch

eval(IfElse { cond=(dwell_ms < 60000), then=Lit(2), else=Lit(3) })
 └─ cv = Bool(true)   (42000 < 60000)
 └─ return eval(Lit(2)) = Value::I64(2)
```

The derived field `dwell_bkt` is stored on the row as `Value::I64(2)`,
and the row continues into the `agg` step. There, `bv.count(where=
(dwell_bkt == 3), window="1h")` evaluates the predicate:

```
eval(BinOp("==", Field("dwell_bkt"), Literal::Int(3)))
 └─ left  = Value::I64(2)
 └─ right = Value::I64(3)
 └─ "==" → cmp_eq(2, 3) = Value::Bool(false)
```

`Value::Bool(false)` → this event is excluded from `deep_count_1h`,
included in `clicks_24h` (no `where` clause). Done.

### 7.1 — Null path (the done-bar's `Optional[str]` claim)

To prove the 3VL claim for `email: str | None`, consider a
hypothetical second `with_columns` step `email_is_set = email is
not None`. The decorator emits the wire string:

```text
(not isnull(email))
```

`expr::parse` consumes this as `Expr::UnaryOp { op: "not", operand:
Call("isnull", [Field("email")]) }`. For `email = Value::Null`:

```
eval(Call("isnull", [Field("email")]))
 └─ args  = [Value::Null]                                       (eval.rs:95)
 └─ lookup_builtin("isnull").eval(&[Null]) = Value::Bool(true)  (expr_builtins.rs)

eval(UnaryOp("not", _))
 └─ v.not_three_valued()                                        (eval.rs:84)
 └─ Bool(true).not_three_valued() = Bool(false)
```

For `email = Value::Str("a@b.com")`:

```
isnull(Str(...))         = Bool(false)
not Bool(false)          = Bool(true)
```

For a hypothetical comparison `email == "hi"` against `Null`:

```
eval(BinOp("==", Field("email"), Literal::Str("hi")))
 └─ left  = Value::Null
 └─ right = Value::Str("hi")
 └─ strict-null guard in eval_binop returns Value::Null
                                                — NOT Bool(false). 3VL preserved.
```

This is why the RFC keeps the `rewrite_null_eq` post-parse pass
(`expr.rs:34 Pass B`) and the strict-null guard *paired*: a user
who writes `email == None` in `@bv.expr` is rewritten to
`isnull(email)` and gets a deterministic `Bool`; a user who writes
`email == other_col` and `other_col` happens to be null gets `Null`
back, so `where=` correctly excludes the row.

---

## 8. Summary diagram

```
              SOURCE                                          WIRE                       SERVER
              ──────                                          ────                       ──────

  @bv.event class Click ─────────────► EventSource(schema={...})  ──► OP_REGISTER (kind:event)

  @bv.expr def dwell_bucket ──────────► wrapper + cached IR
                                         │ (tree + wire string)
                                         │
  e.with_columns(                        │
      dwell_bkt = dwell_bucket(          │
                    e.dwell_ms))         │
            │                            │
            ▼  inline IR at call site,   │
              substitute _SymCol         │
              with caller's _Col         │
            │                            │
            ▼                            │
  to_expr_string()  ─────────────────────┴───────────────►  "(if (dwell_ms < 1000) ...)"
                                                                │
  bv.count(where=bv.col("dwell_bkt")==3) ──────────►  AggDescriptor.to_dict()
                                                                │
                                                                ▼
                                                       OP_REGISTER frame
                                                       payload = JSON above
                                                                │
                                                                ▼
                                                       expr::parse                ──► Expr::IfElse
                                                       schema_propagate           ──► OK
                                                       compile to op_node              │
                                                                                       │
                                          per-event PUSH                                ▼
                                          row = Click {dwell_ms=42000} ─────► eval(&Expr, &Row)
                                                                                       │
                                                                                       ▼
                                                                                 Value::I64(2)
                                                                                       │
                                                                                       ▼
                                                                       agg state update,
                                                                       window bucketing
```

---

## File references (clickable in the repo)

| Step                         | File                                                                |
|------------------------------|---------------------------------------------------------------------|
| `@bv.event` class form       | `python/beava/_events.py:208 _make_event_source`                    |
| `@bv.event` function form    | `python/beava/_events.py:297 _make_event_derivation`                |
| Chain methods                | `python/beava/_events.py:38 _ChainMixin`                            |
| `bv.col` / `bv.lit`          | `python/beava/_col.py:189 col`, `:202 lit`                          |
| `_BinOp.to_expr_string`      | `python/beava/_col.py:149`                                          |
| Aggregation descriptors      | `python/beava/_agg.py`                                              |
| `where=` serialization       | `python/beava/_agg.py:76 _serialize_where`                          |
| Wire frame encoding          | `python/beava/_wire.py:78 encode_frame`                             |
| `@bv.expr` decorator (new)   | `python/beava/_expr_decorator.py` *(this RFC)*                      |
| AST rewriter (new)           | `python/beava/_expr_ast.py` *(this RFC)*                            |
| Symbolic tracer (new)        | `python/beava/_expr_tracer.py` *(this RFC)*                         |
| Expression grammar + parser  | `crates/beava-core/src/expr.rs`                                     |
| `Expr::IfElse` (new)         | `crates/beava-core/src/expr.rs` *(this RFC)*                        |
| `rewrite_null_eq` post-pass  | `crates/beava-core/src/expr.rs:34 Pass B`                           |
| Evaluator entry              | `crates/beava-core/src/eval.rs:56 eval`                             |
| 3VL truth tables             | `Value::*_three_valued` (`row.rs`)                                  |
| Builtins table               | `crates/beava-core/src/expr_builtins.rs`                            |
| Schema propagation           | `crates/beava-core/src/schema_propagate.rs`                         |
| Apply path                   | `crates/beava-server/src/apply_shard.rs::dispatch_push_sync`        |
