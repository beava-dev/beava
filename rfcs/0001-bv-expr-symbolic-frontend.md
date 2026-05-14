# Beava RFC 0001 — `@bv.expr` Symbolic Python Frontend

Table of Contents:

<!-- TOC start (generate with https://bitdowntoc.derlin.ch) -->

<!-- TOC end -->

Status: Draft

Authors:

* [Khanh Doan](https://github.com/Khanathan)

References:

* Issue: [beava-dev/beava#56](https://github.com/beava-dev/beava/issues/56)
  — *feat: @bv.expr symbolic Python frontend (v0.1 capstone)*
* Existing operator-overload DSL: [`python/beava/_col.py`](../python/beava/_col.py)
* `@bv.event` decorator (function form): [`python/beava/_events.py`](../python/beava/_events.py) (lines 297–356)
* Aggregation descriptors: [`python/beava/_agg.py`](../python/beava/_agg.py)
* Wire-string grammar + parser: [`crates/beava-core/src/expr.rs`](../crates/beava-core/src/expr.rs)
  (grammar block at lines 5–19)
* Expression evaluator: [`crates/beava-core/src/eval.rs`](../crates/beava-core/src/eval.rs)
* Builtins table: [`crates/beava-core/src/expr_builtins.rs`](../crates/beava-core/src/expr_builtins.rs)
* 3VL helpers on `Value`: [`crates/beava-core/src/row.rs`](../crates/beava-core/src/row.rs)
  (lines 146 / 174 / 198)
* Locked invariants: [`SOURCE-OF-TRUTH.md`](../SOURCE-OF-TRUTH.md)
* Codebase orientation: [`CLAUDE.md`](../CLAUDE.md)

---

## Summary

This RFC adds `@bv.expr`: a new SDK decorator that lets users author
feature transformations and filters as **plain Python functions**
instead of as operator-overload chains on `bv.col(...)`. The decorator
captures the function body at decoration time via AST rewriting and
operator-overload tracing — supporting `if / else`, ternary
expressions, `and / or`, `is None`, chained
comparisons (`0 < x < 100`), `x in [...]` — and
lowers it into the same wire expression-string IR the server already
consumes. Function arguments are type checked against the upstream event
schema; return types are inferred from the IR.

On the server side, the expression engine
(`crates/beava-core/src/expr.rs`) gains four new `Expr` variants —
`IfElse`, `LetBinding`, `Compare`, `In` — together with their
matching grammar productions and evaluator arms. The new arms reuse
the existing 3VL helpers (`Value::and_three_valued` /
`or_three_valued` / `not_three_valued`, `row.rs:146+`) so null
propagation stays consistent with the current evaluator. A small
set of string / list builtins (`split`, `lower`, `len`, `index`,
`startswith`) lands alongside via the existing `BUILTINS` table
extension pattern in `expr_builtins.rs`.

The existing `bv.col` DSL (`python/beava/_col.py`) is preserved
verbatim — both authoring surfaces emit byte-identical wire
strings for the overlap, and the server cannot tell them apart. The
wire frame envelope, opcodes (`python/beava/_wire.py`), the
registration path (`python/beava/_app.py`), and the mio apply path
are unchanged. Unsupported Python constructs raise structured
`RegistrationError` at decoration time; there is no subprocess
fallback.

A companion `CONTRIBUTING-OPS.md` lands alongside the framework so
that subsequent per-op contributions follow a templated pattern
(~30–50 LOC across `expr_builtins.rs`, `eval.rs`, the Python
builtins-meta table, and one golden test).

---

## Motivation

The current `bv.col` DSL is dense and unfamiliar to data
practitioners, and it cannot express common feature patterns that
read naturally in Python:

* **Control flow.** `if email is None: return None / else: …` is the
  obvious shape for nullable derivations. The SDK has no `if` /
  `when` / `case` surface — the only conditional-shaped helper on
  `_Expr` is `.isnull()` (`_col.py:99`), and the wire grammar
  (`expr.rs:5-19`) has no branching production. Today users must
  factor branching out of the expression: a `with_columns` step that
  derives a boolean, a downstream `where=` filter on that boolean,
  and (when the two arms differ) two separate derivations recombined
  downstream.
* **Chained comparisons.** `0 < dwell_ms < 60_000` is *silently
  broken* under the current DSL. Python desugars `a < b < c` to `(a
  < b) and (b < c)` at the language level — where `and` is Python's,
  not the SDK's `&` overload (`_col.py:80`). Because `_Expr` does
  not define `__bool__`, both `_BinOp` instances are truthy by
  default, and Python's `and` returns its right operand whenever the
  left is truthy. Net: `0 < bv.col("x") < 60_000` evaluates to
  *just* `(x < 60_000)` — the `0 < x` half is silently discarded.
  The wire grammar caps comparisons at one per expression anyway
  (`expr.rs:11`: `CmpExpr := AddExpr (op AddExpr)?`).
* **Intermediate names.** `parts = email.split("@"); parts[1]` has
  no in-expression equivalent. Today users must emit two successive
  `with_columns` calls and re-reference `bv.col("derived")` from
  outside.
* **Function reuse.** Beava's only reuse unit today is `@bv.event def
  Foo(upstream)` (`_events.py:297`), which is a whole derivation —
  one function = one node in the registry graph. There is no shape
  for "compute `email_domain(x)` consistently across N derivations
  without copy-pasting the expression."

These four gaps are exactly what `@bv.expr` closes. The first three
need IR-level extensions in the wire grammar; the fourth needs a
function-shaped reuse unit on the SDK side. The decorator bundles
them.

### Canonical example (verbatim from #56)

```python
@bv.event
class Click:
    user_id: str
    email: str | None         # Optional schema field — nullable in wire format
    referrer: str
    dwell_ms: int
    ts: int

# Return type inferred from the IR tree — annotation optional
@bv.expr
def email_domain(email: str | None):
    if email is None:
        return None
    parts = email.split("@")
    return parts[1].lower() if len(parts) == 2 else None

@bv.expr
def host(url: str):
    if not url.startswith(("http://", "https://")):
        return ""
    parts = url.split("/")
    return parts[2].lower() if len(parts) >= 3 else ""

@bv.expr
def dwell_bucket(dwell_ms: int) -> int:
    if   dwell_ms < 1_000:   return 0
    elif dwell_ms < 10_000:  return 1
    elif dwell_ms < 60_000:  return 2
    else:                    return 3

def ClickFeatures(e: Click):
    e = e.with_columns(
        domain        = email_domain(e.email),       # str | None propagates
        referrer_host = host(e.referrer),
        dwell_bkt     = dwell_bucket(e.dwell_ms),
    )
    return e.group_by("user_id").agg(
        clicks_24h           = bv.count(window="24h"),
        # null-aware aggregations skip rows where the field resolves to None:
        distinct_domains_24h = bv.distinct_count("domain", window="24h"),
        unique_hosts_24h     = bv.n_unique("referrer_host", window="24h"),
        deep_count_1h        = bv.count(where=bv.col("dwell_bkt") == 3, window="1h"),
    )
```

The example demonstrates: type inference (`email_domain` and `host`
omit the return annotation), Optional schema fields (`email: str |
None`), null-aware control flow (`if x is None`), composition into
the existing `e.with_columns(...)` / `e.group_by(...).agg(...)`
chain shape (`_events.py:59`, `:162`), and `where=` predicates over
derived columns (`_agg.py:80 _serialize_where`).

---

## Goals

Mirroring the issue's Scope section:

### SDK (~1000 LOC Python) — `python/beava/`

* `@bv.expr` decorator (`_expr_decorator.py`) — mirrors the
  `@bv.event` function-form decorator pattern at `_events.py:297
  _make_event_derivation` (signature inspection, three-tier name
  resolution: globals → closure cells → caller-frame locals; marker
  attribute on the returned wrapper).
* AST rewriter (`_expr_ast.py`) — `ast.NodeTransformer` for
  `if/else`, ternary `IfExp`, `and / or`, `not`, comparison chains
  (`a < b < c`), `in` (`x in [1,2,3]`), `is None` / `is not None`,
  arithmetic and one-op comparisons. Reference: torch.fx
  `_symbolic_trace.py` for the proxy-tracing pattern.
* Operator-overload tracer (`_expr_tracer.py`) — `_SymbolicCol`
  proxy and `_Sym*` node types that mirror the `_col.py::_Expr`
  hierarchy (frozen dataclasses; explicit `__hash__` per subclass
  because `__eq__` returns an `_Expr`, per `_col.py:95-97`).
* Type checker at call sites against the event schema. **Parameter
  types required**; **return type inferred from the IR**. Reuses the
  schema map already attached to `EventSource`/`EventDerivation` by
  `_events.py:208 _make_event_source`.
* JSON IR emitter using the existing wire format — every `_Sym*`
  node implements `to_expr_string()` with the same contract as
  `_col.py::_Expr.to_expr_string` (line 109). The serialized string
  flows through the existing three serialization sites
  (`_events.py:46 / :65`, `_agg.py:80`); `python/beava/_app.py` is
  unchanged.

### Rust (~500 LOC) — `crates/beava-core/`

* Extend `expr.rs` with four new `Expr` variants — `IfElse`,
  `LetBinding`, `Compare`, `In`. Each carries `span: Span`,
  matching the existing pattern at `expr.rs:87` (`Field`, `Literal`,
  `BinOp`, `UnaryOp`, `Call`).
* Extend `Expr::span()` (line 113) and `collect_fields()` (line 134)
  with one match arm per new variant. **`LetBinding` shadows its
  bound name** in `collect_fields` — uses of the bound name inside
  `body` resolve to the binding, not to a field of the same name.
* Extend the parser (recursive-descent in `expr.rs`) with the
  matching productions for the four new forms. The existing
  `paren_depth > 0` gate (`expr.rs:628 parse_or` and downward) is
  preserved; the SDK is still required to fully parenthesize.
* Extend `eval.rs` with one match arm per new variant. **All
  3VL behavior delegates to the existing
  `Value::and_three_valued / or_three_valued / not_three_valued`
  helpers** (`row.rs:146 / 174 / 198`) — *do not duplicate the
  truth tables inside `eval.rs`* (CLAUDE.md "things to never do").
* Null-aware semantics for the new nodes match the existing strict-null
  semantics (`eval.rs:1-43` doc comment) and the
  `rewrite_null_eq` Pass B (`expr.rs:34`) — both are preserved
  unchanged.
* **One representative op per family lands as a template** for the
  cohort to copy when adding subsequent ops. Issue #56 names
  `math.log1p` as a good worked example.

### Contribution template (~50 LOC docs)

* `CONTRIBUTING-OPS.md` — walks one full op contribution end-to-end
  (Rust enum variant or `BUILTINS` row → eval arm → tracer table
  entry → golden test). The first per-op `good first issue` ticket
  merges through it as proof.

---

## Non-Goals

Mirroring the issue's "Out of Scope (Deferred)" section:

* **Nested types** (struct, list, vector). Tier 2; separate v0.1
  phase.
* **Subprocess Python fallback.** Explicitly rejected per #56.
* **Variadic args, recursion, `for` loops with dynamic bounds.**
  Register-time errors raised by the AST rewriter.
* **Cross-event joins, event-time, session windows.** Locked or
  tracked separately (event-time semantics design tracked in
  beava-dev/beava#51).
* **Changing the wire frame envelope** (`python/beava/_wire.py`,
  opcodes, content type). Per the locked invariant in
  `SOURCE-OF-TRUTH.md`, "wire format is locked; new wire features
  get new opcode numbers." This RFC adds *expression-string
  grammar productions*, not new wire opcodes.

---

## Design

The design has six layers, ordered the way the data flows: from the
Python source the user writes, through the SDK's AST rewrite and
symbolic tracer, across the wire as an expression string, into the
Rust parser, AST, and evaluator. Each layer extends an existing
pattern in the codebase; the ASCII map below names the layer →
existing-file correspondence.

```
                       @bv.expr function body
                                │
                                ▼
   Layer 1: decorator       (_expr_decorator.py — mirrors _events.py:297)
                                │
                                ▼
   Layer 2: AST rewriter    (_expr_ast.py     — new pattern; cf. torch.fx)
                                │
                                ▼
   Layer 3: tracer + nodes  (_expr_tracer.py  — mirrors _col.py:_Expr)
                                │
                                ▼  to_expr_string()
                                                    (mirrors _col.py:109)
                                ─── wire string ───
                                                    (carried by the same three
                                                     SDK→string sites:
                                                     _events.py:46/:65, _agg.py:80)
                                                    (no change to _app.py / _wire.py)
                                                    (no change to register frame
                                                     envelope or OP_REGISTER)
                                ▼
   Layer 4: Rust AST        (expr.rs — extends Expr enum at line 87)
                                │
                                ▼
   Layer 5: parser          (expr.rs — extends grammar at lines 5-19)
                                │
                                ▼
   Layer 6: evaluator       (eval.rs — extends eval_depth match;
                                       reuses Value::*_three_valued in row.rs)
```

Property the design preserves: both `bv.col(...) > 100` and a
`@bv.expr` body containing `e.amount > 100` produce the byte-identical
wire string `(amount > 100)` and the byte-identical `Expr::BinOp`
tree on the server. The server is indifferent to which authoring
surface emitted the string.

### Layer 1 — Python: `@bv.expr` decorator

New file: `python/beava/_expr_decorator.py`. Public surface added to
`python/beava/__init__.py::__all__`.

The decorator mirrors `_events.py:297 _make_event_derivation` step
for step:

1. **Source recovery.** `inspect.getsource(fn)` → `ast.parse(src)` →
   `ast.Module`.
2. **Schema-bound argument typing.** Three-tier resolution exactly
   as `_make_event_derivation` does: `fn.__globals__` → closure
   cells (mirror of `_collect_closure_cells_for_events` at
   `_events.py:248`) → caller-frame locals (mirror of
   `_collect_caller_frame_locals_for_events` at `_events.py:268`).
   `Optional[T]` annotations become nullable `_SymCol(name, type=T,
   nullable=True)`. **Parameter types are required** (raise
   `RegistrationError(code="missing_parameter_annotation")` if not
   present); the return type is inferred from the IR.
3. **AST rewrite.** Run Layer 2's transformer on the body. Any
   unsupported node raises `RegistrationError(code="unsupported_python_node")`
   — same structured-error shape as the existing `RegistrationError`
   (`_errors.py`).
4. **Symbolic execution.** Compile the rewritten module and `exec`
   it with the symbolic args (Layer 3). The return value is a
   `_Sym*` node tree.
5. **Type inference.** Bottom-up over the result tree:
   - literal → its Python type;
   - `_SymCol` → its schema type (with nullability);
   - `_SymIfElse` → join of branch types;
   - arithmetic → numeric widening (`I64 + F64 → F64`);
   - `_SymBinOp(cmp)` / `_SymCompare` → `Bool` (nullable iff any
     operand nullable);
   - `_SymCall` → table-driven from `expr_builtins.rs`'s arity /
     return spec, mirrored in Python in a small `_builtins_meta.py`.
6. **IR cache.** Stash the produced IR on `fn.__beava_expr_ir__` so
   repeat calls inside `App.register(...)` are O(1). This is the
   same kind of marker `_make_event_derivation` sets at line 354
   (`result._is_bv_event_function = True`).

The decorator returns a wrapper that, when called with `_Expr` /
`_Col` / `_SymbolicCol` arguments (e.g. inside another `@bv.expr`
or inside a `with_columns(...)` derivation), inlines its cached IR
with the formal parameters substituted. When called with concrete
Python values (e.g. inside a unit test), it falls through to the
original undecorated function — so users can pytest their `@bv.expr`
functions with plain Python.

### Layer 2 — Python: AST rewriter

New file: `python/beava/_expr_ast.py`.

`ast.NodeTransformer` that maps a strict, allow-listed Python subset
into symbolic-node constructors (Layer 3). Reference design:
torch.fx `_symbolic_trace.py`.

| Python AST node                           | Lowers to                                                                |
|-------------------------------------------|--------------------------------------------------------------------------|
| `ast.If` (and `elif` chains)              | `_SymIfElse(cond, then, else)` (nested for `elif`)                       |
| `ast.IfExp` (`a if c else b`)             | `_SymIfElse(c, a, b)`                                                    |
| `ast.BoolOp(And / Or, values)`            | left-folded `_SymBinOp("and"/"or", ...)`                                 |
| `ast.UnaryOp(Not, x)`                     | `_SymUnaryOp("not", x)`                                                  |
| `ast.Compare` with **≥ 2** ops            | `_SymCompare([ops], [operands])`                                         |
| `ast.Compare` with **1** op               | `_SymBinOp(op, l, r)` (matches `_col.py` 1-op cmps)                      |
| `x in [...]` (`ast.Compare` with `In`)    | `_SymIn(x, [literal-list])`                                              |
| `x is None` / `x is not None`             | `_SymCall("isnull", [x])` / `_SymUnaryOp("not", _SymCall("isnull",[x]))` |
| `ast.Assign` (intermediate locals)        | `_SymLetBinding(name, value, rest_of_body)` (CPS-style accumulation)     |
| `ast.Return`                              | terminates the surrounding `LetBinding` chain                            |
| arithmetic (`+ - * /`), 1-op cmps         | `_SymBinOp(op, l, r)`                                                    |
| `ast.Call(<allow-listed name>, ...)`      | `_SymCall(name, args)` (allow-list derived from `expr_builtins.rs`)      |

Rejected nodes (non-exhaustive): `for / while`, comprehensions,
generator expressions, lambdas, `try / except`, `with`, attribute
access on non-symbolic values, dynamic attribute access, calls to
functions that are neither in the builtins allow-list nor
`@bv.expr`-decorated. Each rejection raises
`RegistrationError(code="unsupported_python_node")` with a
source-line pointer resolved via `inspect.getsourcelines`.

### Layer 3 — Python: `_SymbolicCol` tracer

New file: `python/beava/_expr_tracer.py`.

A `_SymbolicCol` is the runtime stand-in passed to a `@bv.expr`
function in place of a real value. Its operator overloads mirror
`_col.py::_Expr` (lines 27–111) one-for-one — same `__add__` /
`__gt__` / `__eq__` / `__and__` / etc. dispatch, same `_coerce`
helper for literal lifting (`_col.py:184`), same explicit
`__hash__` per subclass to compensate for `__eq__` returning an
expression node.

Symbolic node types (frozen dataclasses, mirroring `_col.py`):

```python
_SymCol(name: str, type: SchemaType, nullable: bool)
_SymLit(value: Any)
_SymBinOp(op: str, left, right)
_SymUnaryOp(op: str, operand)
_SymCompare(ops: list[str], operands: list)
_SymIfElse(cond, then_branch, else_branch)
_SymLetBinding(name: str, value, body)
_SymIn(operand, choices: list)            # x in [...]
_SymCall(fn_name: str, args: list)
```

Each node implements `to_expr_string()`. The serialization is
**identical** to `_col.py` for the overlapping nodes (`_SymBinOp`,
`_SymUnaryOp`, `_SymLit`, `_SymCol`, `_SymCall`); the four new node
types serialize to the new productions Layer 5 will accept:

```
_SymIfElse        → "(if <cond> then <then> else <else>)"
_SymLetBinding    → "(let <name> = <value> in <body>)"
_SymCompare       → "(<a> < <b> < <c>)"      # n+1 operands, n ops
_SymIn            → "(<x> in [<lit1>, <lit2>, ...])"
```

Where node shapes overlap with `_col.py`, the tracer either reuses
`_col.py`'s nodes directly (preferred) or shares the serialization
helper. The tracer lives in a *new* private module rather than
extending `_col.py` because `_col.py` is the public-surface DSL and
the tracer is an implementation detail of the decorator.

### Layer 4 — Rust: new `Expr` variants

File: `crates/beava-core/src/expr.rs`. Extends the enum at line 87.

```rust
pub enum Expr {
    // existing variants (lines 87–111): Field, Literal, BinOp, UnaryOp, Call

    IfElse {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
        span: Span,
    },
    LetBinding {
        name: String,
        value: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    Compare {
        ops: Vec<String>,      // length n
        operands: Vec<Expr>,   // length n + 1
        span: Span,
    },
    In {
        operand: Box<Expr>,
        choices: Vec<Literal>, // literal-only RHS in v0
        span: Span,
    },
}
```

Each variant follows the existing `{...; span: Span}` shape so
error-message machinery and `Expr::span()` keep working uniformly.
For each new variant we also extend:

1. `Expr::span()` (line 113) — one match arm.
2. `collect_fields()` (line 134) — one match arm. **`LetBinding`
   shadows `name`**: the bound name is excluded from
   `referenced_fields`, and `Field` lookups inside `body` should
   resolve against the binding scope (this is what makes
   `parts = email.split("@"); parts[1]` correctly *not* declare a
   `parts` field on the schema).
3. `eval.rs` (Layer 6) — one match arm.

`Compare` is a dedicated node (not nested `BinOp`s) primarily so the
AST round-trips the user's intent — desugaring to `(a < b) and (b <
c)` is observationally equivalent under 3VL for pure operands, but
loses source-span fidelity and re-emits the middle operand. `In` is
dedicated for the same reason; v0 restricts the RHS to literals
(no column-list-membership) to keep the eval semantics trivial.

### Layer 5 — Rust: parser extensions

Still `crates/beava-core/src/expr.rs`. The grammar block (lines
5–19) gains the following productions:

```text
Expr     := IfExpr | LetExpr | OrExpr
IfExpr   := 'if' Expr 'then' Expr 'else' Expr            ; ternary, lowest precedence above 'or'
LetExpr  := 'let' Ident '=' Expr 'in' Expr               ; binds Ident in Expr
CmpExpr  := AddExpr ( ('>'|'>='|'<'|'<='|'=='|'!=') AddExpr )+
                                                         ; n+1 operands → Compare; n=1 stays BinOp
InExpr   := AddExpr 'in' '[' Literal (',' Literal)* ']'
```

The `paren_depth > 0` gate (`expr.rs:628`) is preserved unchanged;
the SDK is still required to fully parenthesize. The existing
post-parse Pass A (cast bare-ident normalization, line 22) and
Pass B (`rewrite_null_eq`, line 34) run unchanged. Adding `if /
then / else / let / in` as keyword tokens follows the existing
`not / and / or / true / false / null` pattern (`expr.rs:463`).

### Layer 6 — Rust: evaluator + 3VL

File: `crates/beava-core/src/eval.rs`. Extends `eval_depth` (line
60) with one match arm per new variant. The semantics:

| Construct                    | Result                                                                                |
|------------------------------|---------------------------------------------------------------------------------------|
| `IfElse(cond, then, else)`   | If `cond` evaluates to `Null`, return `Null`. Else pick the branch.                   |
| `LetBinding(name, v, body)`  | Evaluate `v` once; bind `name → v_value` in a small env (e.g. `SmallVec<(String, Value), 4>`); recurse `body` with the env. |
| `Compare(ops, operands)`     | Walk left-to-right pairwise; short-circuit on `Bool(false)`; propagate `Null` on any `Null` pair via `Value::and_three_valued`. |
| `In(operand, choices)`       | If `operand` is `Null`, return `Null`. Else `Bool(true)` if any literal matches; `Bool(false)` otherwise. |

All boolean reductions delegate to `Value::and_three_valued /
or_three_valued / not_three_valued` (`row.rs:146 / 174 / 198`). The
existing strict-null `BinOp("==")` guard is preserved verbatim, as
is the `MAX_EVAL_DEPTH = 512` bound (`eval.rs:47`); the new
variants count toward depth.

---

## Impact Analysis

Beava components this RFC touches.

### Python SDK (`python/beava/`)

- [x] Public surface (`__init__.py`'s `__all__`) — adds `bv.expr`.
- [x] Operator-overload DSL (`_col.py`) — read-only; tracer reuses
      `_Expr` types where they overlap.
- [x] Aggregation descriptors (`_agg.py`) — unchanged. `where=`
      still accepts an `_Expr`; a `@bv.expr`-returned IR satisfies
      that.
- [x] Errors (`_errors.py`) — adds `unsupported_python_node`,
      `missing_parameter_annotation`, `return_type_mismatch` codes
      under `RegistrationError`.
- [ ] Wire / transport (`_wire.py`, `_transport.py`) — unchanged.
      Frame envelope, opcodes, content type all unchanged.
- [ ] App layer (`_app.py`) — unchanged. Already opaque to expression
      shape (sees only `chain[*].exprs.*` strings,
      `chain[*].aggs.*.where` strings, `chain[*].expr` strings).
- [x] Demo (`_demo.py`) — add a `@bv.expr` example alongside the
      existing `bv.col` form.

### Rust core (`crates/beava-core/`)

- [x] Expression IR + parser (`expr.rs`) — four new `Expr` variants
      + four new grammar productions + extension of `Expr::span()` /
      `collect_fields()`. Existing Pass A / Pass B unchanged.
- [x] Evaluator (`eval.rs`) — four new match arms; 3VL preserved
      via existing `Value::*_three_valued`.
- [x] Builtins (`expr_builtins.rs`) — table grows by the builtins
      the canonical example exercises (`split`, `lower`, `len`,
      `index`, `startswith` for `email_domain` / `host`). Each
      addition is one row in `BUILTINS` per CLAUDE.md's preferred
      extension pattern.
- [x] Schema (`schema.rs`, `schema_propagate.rs`) — return-type
      inference clarification for `IfElse` branch-type join; no
      structural change.
- [ ] Sketches, wire format (`sketches/`, `wire.rs`) — unchanged.

### Runtime / server (`crates/beava-runtime-core/`, `crates/beava-server/`)

- [ ] mio data plane (`apply_shard.rs::dispatch_*_sync`) —
      unchanged. New `Expr` variants are evaluated through the same
      `eval.rs` entrypoint.
- [ ] axum admin sidecar (`http_admin.rs`) — unchanged.
- [ ] WAL / snapshots (`crates/beava-persistence/`) — unchanged.
      The expression string is opaque to the WAL.

### Wire / compatibility

- [ ] Opcode numbers — unchanged. Per the locked invariant, opcodes
      never change shape and new features get new numbers. This RFC
      adds expression-string grammar productions, not new wire
      opcodes.
- [x] Expression-string grammar — four new productions. Consumed
      only by the server's `expr::parse`.

### Docs / website

- [x] `CONTRIBUTING-OPS.md` — new top-level file (release-gate per
      issue Done-When).
- [x] `docs/python/` — new page documenting `@bv.expr` and the
      supported / rejected subset.
- [x] `CHANGELOG.md` — one entry under the next unreleased version.
- [x] `SOURCE-OF-TRUTH.md` — record `@bv.expr` as canonical under
      `python/beava/_expr_decorator.py`; add the four new `Expr`
      variants to the architectural-commitments expression-engine
      list.

---

## Operations

### Performance & cost

This RFC is **register-time and parse-time work**. The hot path on
the server is structurally unchanged: events still flow through
`apply_shard.rs::dispatch_push_sync` and the evaluator visits the
same `Expr` tree the parser produced.

- **Register-time (Python).** AST parsing + symbolic execution is
  one-shot per `@bv.expr` per process. Cached on
  `fn.__beava_expr_ir__`; repeat calls are dict lookups.
- **Apply path (Rust).** `IfElse` adds one branch per occurrence;
  `LetBinding` adds one map insert + one map lookup per use of the
  bound name; `Compare(n)` is a flat loop over n+1 operands;
  `In(k)` is a linear search over k literals (k is small in
  practice). Net: comparable to today.
- **Memory.** `LetBinding`'s per-evaluation env is a `SmallVec`-style
  buffer with inline capacity; no heap growth on typical
  expressions.
- **Wire bytes.** Marginally larger expression strings for the new
  forms. The wire frame envelope is unchanged.

### Observability

- **Configuration.** No new flags. `@bv.expr` is opt-in by usage.
- **Metrics.** None added in this RFC.
- **Logging.** Errors flow through the existing structured
  `RegistrationError(code, path, message, errors)` shape (see
  `_errors.py`). Server-side parse / eval errors keep their existing
  span-aware reporting (`expr.rs::ParseError`).

### Compatibility

- **Existing public APIs.** `bv.col` / `bv.lit` / `bv.event` /
  `bv.table` / `bv.App` unchanged. `@bv.expr` is net-additive.
  Aggregation helpers in `_agg.py` unchanged.
- **Existing wire format.** Round-trip property: every expression
  the *current* `_col.py` emits today parses to the *same* `Expr`
  AST after this RFC, modulo the parser-internal n=1-vs-n-ary
  `Compare` collapse (no wire change). All existing parser tests
  stay green.
- **Existing data on disk.** No persistence changes.
- **Mixed-version.** An old server cannot parse the four new
  productions and will return the existing structured parse error.
  An unmodified SDK against an old server is unaffected (it does
  not emit the new forms unless the user writes `@bv.expr`). We
  document that `@bv.expr` requires a matching-or-newer server.

---

## Testing

Per the issue's Done-When section. Acceptance suite is `python/tests/v0/`
(per `python/pyproject.toml::testpaths`).

- **`python/tests/v0/test_symbolic_frontend.py`** — the canonical
  `ClickFeatures` example end-to-end. Registers, pushes a sample
  event stream, asserts the IR JSON for each derived column,
  asserts window outputs, and confirms `email = None` rows are
  excluded from `distinct_domains_24h` / `unique_hosts_24h` (the
  null-aware-aggregation property). Exercises:
  - AST-rewritten `if / else`.
  - `is None` patterns.
  - Function composition (`@bv.expr` calling another `@bv.expr`).
  - Closure capture of module-level constants.
  - Return-type inference.
  - `where=` with derived predicates.
- **Targeted Python tests** under `python/tests/v0/`:
  - One ✓ test per supported Python construct, one ✗ test per
    rejected construct asserting error code + source line.
  - 3VL truth tables round-tripped through the server for every
    new node.
- **Rust unit tests** in `expr.rs` and `eval.rs`:
  - One success + one parse-error + one span test per new
    production (`IfElse`, `LetBinding`, `Compare`, `In`).
  - `LetBinding` shadow semantics in `referenced_fields()`.
  - `IfElse` with `Null` cond → `Null`.
  - `Compare` short-circuit on first `Bool(false)`; propagate
    `Null` on null middle operand.
  - `In` with `Null` operand → `Null`.

CI gate: `bash .github/scripts/check.sh` (the canonical script the
repo already ships) — unchanged in shape, expanded in body.

---

## Rollout

* **Single PR** per the issue's framing (v0.1 capstone). Commit
  order so the stack reads top-down for a reviewer:
  1. Rust: four new `Expr` variants + parser productions + eval arms
     + Rust unit tests. Green on its own against an unmodified SDK
     (the SDK doesn't emit the new forms yet).
  2. Python: tracer + AST rewriter + decorator + `_builtins_meta.py`
     + new error codes + `__init__.py` export.
  3. Acceptance test (`test_symbolic_frontend.py`) + targeted
     Python tests.
  4. `CONTRIBUTING-OPS.md` + `docs/python/expr.mdx` +
     `CHANGELOG.md` + `SOURCE-OF-TRUTH.md`.
* **No feature flag.** The decorator is net-additive; absent any
  `@bv.expr` use, no behavior changes.
* **Cohort positioning** (issue's Cohort Positioning section). The
  framework is the on-ramp; subsequent cohort PRs ship per-op
  contributions through `CONTRIBUTING-OPS.md`. After this RFC lands,
  a new scalar op = ~30–50 LOC across four files:
  - `crates/beava-core/src/expr_builtins.rs` — one `BUILTINS` row
    (or one `Expr` variant + `expr.rs` arms if it needs new syntax).
  - `crates/beava-core/src/eval.rs` — one match-arm extension (only
    if a new variant was added; pure builtins go straight through
    the existing `Call` arm).
  - `python/beava/_builtins_meta.py` — one row mirroring the Rust
    arity / return type.
  - `python/tests/v0/test_<op>.py` — one golden test.
* **Effort.** Per the issue: ~2 weeks human / ~1–2 days CC. Cohort
  Track-1 capstone.

---

## Alternatives

### Extend `_col.py` with helpers (`bv.if_`, `bv.let`, `bv.in_`) instead of `@bv.expr`

Would close the IR gap (Layers 4–6) without the SDK side (Layers
1–3). Rejected: gives no reuse unit (every feature would re-build
the helper chain by hand), no Python-as-source ergonomics, no
decoration-time type checking, no source-span fidelity for errors.
The issue's Cohort Positioning explicitly targets data scientists
writing plain Python — the helper-only path does not get there.

### Direct JSON IR emission from the SDK (skip the string grammar)

The decorator could emit JSON nodes that the server accepts
alongside the parsed string IR. Rejected: would create two IRs to
keep in sync; every config file, CLI input, and example that goes
through the string parser would drift from the SDK output. CLAUDE.md
treats the string grammar as canonical and the parser as the
enforcement point.

### Subprocess Python fallback for unsupported nodes

For nodes outside the allow-list, ship a sidecar Python interpreter
that the server shells out to per event. **Explicitly rejected by
the issue.** Defeats the apply-path latency contract and introduces
a security surface v0 cannot afford.

---

## Open Questions

* **Q1.** `match` statements (PEP 634) inside `@bv.expr`? Tentative
  answer: defer; `if / elif / else` covers the canonical example.
* **Q2.** Where does `_builtins_meta.py` live and how is it kept in
  sync with `expr_builtins.rs`? Tentative answer: hand-maintain with
  a CI cross-check; revisit if the table exceeds ~20 rows.
* **Q3.** Closure capture: freeze captured module-level values at
  decoration time, or resolve at call time? Tentative answer:
  freeze at decoration time (`_SymLit` the captured value once);
  error if a captured name does not resolve to a literal-equivalent.
  Mirrors how `_make_event_derivation` resolves names at decoration
  time.
* **Q4.** `dwell_bucket(7500) == 1` from a unit test — should the
  wrapper fall through to the original undecorated function for
  concrete-value inputs? Tentative answer: yes (Layer 1, step 6
  rationale).
* **Q5.** Source-span fidelity for errors raised inside lowered
  builtins (e.g. an `email_domain` that fails because `email`
  resolves null at row time). Tentative answer: span tracking on
  `_Sym*` nodes carries the `inspect.getsourcelines` line into the
  Rust span at parse time via a side-channel header in the wire
  string; defer the precise mechanism to a follow-up if needed.

---

## Sub-issues

Per the issue's tracker:

* **Infrastructure:** beava-dev/beava#58, #59, #57, #60, #67.
* **Representative op batches:** beava-dev/beava#66, #63, #61, #64.
* **Integration + onboarding:** beava-dev/beava#62, #65.

This RFC is the architectural umbrella; the sub-issues track the
per-area work items.

---

## Updates

* *2026-05-14* — Draft revised to mirror issue #56 closely
  (added `In` IR variant; cited torch.fx reference; added Cohort
  Positioning to Rollout; tightened references to current Beava
  files).