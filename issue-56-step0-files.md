# Issue #56 — Step 0 reading: walkthrough of the existing code

This is a file-by-file walkthrough of the six files Step 0 of
`issue-56-plan.md` told you to read before writing a single line. For
each file: a one-line **simple-terms** summary first, then a deeper
**how it works** breakdown, then the **specific hooks** the `@bv.expr`
work will plug into.

Read top-down. The order matches the Step 0 list and goes from "things
the user types" → "things the server runs".

---

## 1. `python/beava/_col.py` (221 lines)

### Simple terms

This is the file that turns `bv.col("amount") > 100` from Python code
into a tiny tree of objects, and then renders that tree into a string
like `(amount > 100)` that the server can parse. Nothing executes
here — it just builds a description of *what the user meant*.

### How it works

The file defines a class hierarchy that lives entirely in memory and
serializes to a string at the very end:

- `_Expr` (line 27) — the abstract base. **All it really does** is
  overload Python operators (`+`, `-`, `*`, `/`, `>`, `>=`, `<`, `<=`,
  `==`, `!=`, `&`, `|`, `~`) so that `bv.col("a") > 100` does not
  actually compute a boolean — instead, Python's operator dispatch
  invokes `_Expr.__gt__`, which returns a new `_BinOp(">", self, _coerce(other))`.
  Same trick for every other operator.
- `_Col(name)` (line 114) — a leaf node for a column reference. Its
  `to_expr_string()` is just `self.name`.
- `_Literal(value)` (line 125) — a leaf node for a Python constant.
  Its `to_expr_string()` is `repr(value)` with two special cases:
  `None` → `"null"`, and `bool` → `"true"`/`"false"` (notice that the
  `bool` branch comes *before* `int`, because in Python `isinstance(True, int)`
  is true).
- `_BinOp(op, left, right)` (line 143) — an internal node holding a
  two-letter operator and two child `_Expr`s. Its `to_expr_string()`
  is the fully-parenthesized `(left op right)` — *every* binop emits
  parentheses, which matches the Rust grammar (more on that below).
- `_UnaryOp(op, operand)` (line 156) — for `~x` (rendered as `!(x)`)
  and `.isnull()` (rendered as `(x == null)`, **not** `isnull(x)` —
  the Rust parser does the final rewrite).
- `_CastOp(operand, target)` (line 172) — for `x.cast("float")`,
  rendered as `cast(x, float)`.

Three subtle things worth flagging:

1. **`&` and `|` are repurposed.** Python forbids overloading `and` /
   `or`, so the SDK overloads `__and__` / `__or__` and serializes them
   as the keyword tokens `and` / `or` (not the bitwise `&` / `|` they
   would normally be). See the comment on lines 76–79.
2. **`__eq__` is broken on purpose.** Overriding `__eq__` to return an
   `_Expr` instead of a `bool` means instances become unhashable.
   Each subclass restores `__hash__` explicitly (line 121, 139, 152,
   168, 180) so AST nodes can live in `set`/`dict` keys. Comment on
   lines 95–97.
3. **`.cast()` validates target eagerly.** Only `"str"`, `"int"`,
   `"float"`, `"bool"` are allowed (line 24). Anything else raises
   `ValueError` at *construction time* — not at server eval time.

### Hooks for `@bv.expr`

- **Reuse the serialization format.** When the new `_SymbolicCol`
  tracer in Step 5 emits its IR, every leaf and binop should produce
  the same `to_expr_string()` shape that `_col.py` already produces,
  so the Rust parser does not see two dialects.
- **Reuse `_coerce(other)`** (line 184) — the "wrap any Python
  literal as `_Literal`" helper. The tracer will need an identical
  helper for its symbolic nodes.
- **Mirror the `&` / `|` repurposing.** The AST rewriter (Step 6)
  converts source-level `and` / `or` straight into the same `"and"` /
  `"or"` op strings. The tracer's `_SymBinOp` should keep that string
  the same way `_BinOp` does, so the wire output is identical.

---

## 2. `python/beava/__init__.py` (155 lines)

### Simple terms

This is the package's front door: it decides which names users can
import as `import beava as bv` followed by `bv.<something>`. Right
now `bv.col`, `bv.lit`, `bv.event`, `bv.table`, `bv.App`, and roughly
60 aggregation helpers (`bv.count`, `bv.sum`, …) are exported. The
new `bv.expr` decorator must show up here.

### How it works

The file is mostly a flat list of `from beava._foo import bar` lines
followed by `__all__`. Two observations:

- **The module docstring (lines 1–8) pins scope explicitly.** It calls
  out `bv.event` / `bv.table` / `bv.col` / `bv.lit` as the *public
  surface* and references ADR-001. Any new top-level export is a real
  surface change.
- **`__all__` is the canonical list.** Star-imports use it, and
  type-checkers/IDEs hint from it. The aggregation block (lines
  95–155) is dense but uneventful: each line just re-exports a helper
  from `_agg.py`.

### Hooks for `@bv.expr`

- **Step 11 work lives here.** Add `from beava._expr_decorator import expr`
  (or whatever the module ends up being called) below the `_col`
  import, then add `"expr"` to `__all__`. Place it next to `col` /
  `lit` since it occupies the same conceptual slot — "things that
  describe a column or a derived value."
- **Update the module docstring.** The Phase-3 comment ("Public
  surface: bv.event / bv.table / bv.col / bv.lit / bv.App plus the
  operator catalogue") will become misleading once `bv.expr` ships —
  add `bv.expr` to the listed front door.

---

## 3. `crates/beava-core/src/expr.rs` (1852 lines)

### Simple terms

This is the Rust file that **reads the expression strings the Python
SDK builds** and turns them into a tree the server can evaluate. It is
a tokenizer + recursive-descent parser + two normalization passes.
About a third of the file is the parser; the rest is tests.

### How it works

**Public types** (lines 47–112):

- `Span { start, end }` — byte offsets into the source string. Used
  for error messages and (eventually) for "underline this part."
- `ParseError { col, reason }` — the only error type; `col` is
  1-indexed for human-readable messages.
- `Literal` enum — `Null`, `Bool`, `Int(i64)`, `Float(f64)`,
  `Str(String)`, and a special `BareIdent(String)` (covered below).
- `Expr` enum — five variants:
  - `Field { name, span }` — a column reference like `amount` or
    `Stream.x` (note the one level of dot qualification).
  - `Literal(Literal, span)`.
  - `BinOp { op, left, right, span }` — operator stored as a `String`
    (`"+"`, `"and"`, `">"`, etc.). Not an enum — this keeps the parser
    simple and matches the Python side.
  - `UnaryOp { op, operand, span }` — at present **only `"not"`** is
    used.
  - `Call { fn_name, args, span }` — every builtin call (`cast`,
    `isnull`, `quadkey`).

**Public methods** on `Expr` (lines 113–132):

- `span()` — pulls the span out regardless of variant.
- `referenced_fields()` — walks the tree, returns a `BTreeSet<String>`
  of every `Field` name. Critically, `BareIdent` is *not* a field
  reference — `cast(amount, float)` references `amount`, not `float`.

**Grammar** (the comment at the top of the file, lines 6–19, is the
spec — keep it in mind):

```text
Expr   := OrExpr
OrExpr  := AndExpr ('or' AndExpr)*
AndExpr := NotExpr ('and' NotExpr)*
NotExpr := 'not' NotExpr | CmpExpr
CmpExpr := AddExpr (('>'|'>='|'<'|'<='|'=='|'!=') AddExpr)?
AddExpr := MulExpr (('+'|'-') MulExpr)*
MulExpr := Atom (('*'|'/') Atom)*
Atom    := '(' Expr ')' | Call | Ident | Literal
```

Precedence climbs from low (`or`) to high (`*`/`/`), exactly as in
SQL/Python. **One big quirk:** every binary operator below the atom
level is *only* parsed when `paren_depth > 0` (lines 606, 629, 659,
688, 713). The reason: the Python SDK always emits fully-parenthesized
binops (`(a > b)`, `((a > b) and (c < d))`), so the parser refuses
unparenthesized binops at the top level to keep the wire format
unambiguous. `parse_bare_field` (line 1170) shows the consequence:
`amount` alone parses fine, but `amount > 100` would not — you must
write `(amount > 100)`.

**Tokenizer** (lines 273–537): hand-rolled byte-level lexer. Handles:

- Single-character punctuation: `(`, `)`, `,`, `+`, `-`, `*`, `/`.
- Two-character compounds: `>=`, `<=`, `==`, `!=`.
- Disallowed: bare `=` (line 351) and bare `!` (line 367) both error
  with helpful messages.
- Single-quoted strings with `\\` and `\'` escapes (lines 375–417).
- Numbers: integer or float; float requires `digit.digit` (lines
  482–537). Optional exponent.
- Identifiers: ASCII alpha + underscore, plus one optional `.segment`
  (lines 427–471). Keywords (`and`, `or`, `not`, `true`, `false`,
  `null`) are recognized in the same loop.

**Parser** (lines 247–940): straightforward recursive descent. One
lookahead token (`self.peeked`). The `paren_depth` field gates every
infix rule (above). `parse_atom` (line 736) handles every Atom case
including the negative-literal trick: a leading `-` followed
**immediately** by a number atom is folded into a single
`Literal::Int`/`Float` (line 876) — anything else is a parse error
("`-` must be followed by a number literal", line 902). This means
the SDK has to emit `(0 - x)` for subtraction; `-x` is reserved for
negative literals.

**Post-parse normalization** (lines 942–1084):

- **Pass A — `normalize_cast`** (line 946): `cast(x, float)` parses
  with `float` as a `Field`, which is wrong. The pass rewrites every
  `Call("cast", _)`'s second argument from `Field { name }` to
  `Literal(BareIdent(name))`. That is the *only* way `BareIdent` is
  produced.
- **Pass B — `rewrite_null_eq`** (line 1002): `(x == null)` and
  `(null == x)` are both rewritten to `Call("isnull", [x])`.
  Symmetric: `(x != null)` / `(null != x)` become
  `UnaryOp("not", Call("isnull", [x]))`. **Why:** the evaluator
  treats `==` strictly under null propagation (`null == anything = null`),
  so without this rewrite `(x != null)` would *silently drop every
  row* (always Null). The comment on lines 39–43 explains it.

### Hooks for `@bv.expr`

This is the file Step 2 and Step 3 of the plan extend. Concretely:

- **Add three `Expr` variants** at line 87, next to the existing five.
  Keep them in the same idiom: `{ ..., span: Span }`.
- **Add a `span()` arm** at line 113, an arm in `collect_fields`
  at line 134 (with `LetBinding` shadowing its bound name), and a
  parser rule in the appropriate `parse_*` (most likely a new
  `parse_let` and a new `parse_ifelse` at a low precedence level,
  plus a folding step in `parse_cmp` for chained comparisons).
- **Mirror the parentheses convention.** Since the SDK emits
  parens around every binop, you have two choices: emit
  `if … then … else …` *outside* parens (like the existing top-level
  Expr) or *inside*. Pick one and document it in the grammar comment
  at line 6.
- **Mind the normalization passes.** If `IfElse` or `LetBinding`
  bodies can contain `cast(x, float)` or `(x == null)`, both passes
  need an arm for the new variants. They are bottom-up rewrites,
  so the recursion has to walk in.

---

## 4. `crates/beava-core/src/expr_builtins.rs` (420 lines)

### Simple terms

A tiny lookup table of named functions the expression language can
call. Today: `cast`, `isnull`, and `quadkey`. New functions can be
added by appending one entry — no parser or evaluator surgery needed.

### How it works

- `Arity` (line 28): `Fixed(n)` or `Variadic`. Used by the evaluator
  and (more importantly) by register-time validation.
- `BuiltinFn { name, arity, eval }` (line 38): the entry. `eval` is a
  plain function pointer `fn(&[Value]) -> Value`. Arguments come in
  already evaluated — the dispatch is in `eval.rs`.
- `BUILTINS` (line 54): a `&[BuiltinFn]` static array. Currently three
  entries (`cast`, `isnull`, `quadkey`).
- `lookup_builtin(name)` (line 78): linear scan. Fine at this scale;
  a `HashMap` would be premature.

**`cast_eval`** (line 104) implements the conversion matrix in the
doc comment (line 95). Notable choices:

- Wrong arity → `Null` (defensive; register-time should catch).
- `Null` input → `Null` always.
- Unknown target type → `Null`.
- `f64 → i64` truncates toward zero (Rust default `as i64`).
- `str → int/float` uses `str::parse`; failure → `Null`, never panic.
- `bytes → anything` is `Null` (no implicit encoding).

**`isnull_eval`** (line 243): always returns `Bool(true/false)`,
**never** `Null`. That guarantee is what lets the `(x == null)`
rewrite in `expr.rs` produce a clean boolean.

**`quadkey_eval`** (line 210): tile-id from lat/lon/zoom using a
simplified Mercator. Bounds-clamped, range-validated, null-safe.

### Hooks for `@bv.expr`

- The plan does not (in this issue) add new builtins, but if you
  decide to surface a few user helpers (`bv.coalesce`, `bv.in_set`,
  …) this is exactly where they go: one row in `BUILTINS`, one
  function below.
- **Three-valued-logic helpers might also belong here.** Right now,
  3VL lives as methods on `Value` (`and_three_valued`,
  `or_three_valued`, `not_three_valued`) — see `eval.rs` line 84,
  117, 126. If you decide to expose `bv.is_null` as a friendly
  alias (or `bv.nvl` etc.), wire them through this table.

---

## 5. `crates/beava-core/src/eval.rs` (932 lines)

### Simple terms

The function that takes an `Expr` tree and a row of values, and
computes the answer. Pure, deterministic, depth-bounded. This is
where null-aware semantics actually happen.

### How it works

Entry point: `pub fn eval(expr: &Expr, row: &Row) -> Value` (line 56).
Calls `eval_depth(expr, row, 0)`. The depth counter is bounded by
`MAX_EVAL_DEPTH = 512` (line 47); deeper expressions return `Null`
instead of overflowing the stack — defends against crafted inputs.

`eval_depth` (line 60) is a `match` on the five `Expr` variants:

1. **`Field`** (line 66): look up the row column, default to `Null`
   if absent.
2. **`Literal`** (line 69): map each `Literal` variant to its
   `Value`. **`BareIdent` becomes `Value::Str`** — that is how
   `cast(x, float)` arrives at `cast_eval` as `[..., Value::Str("float")]`.
3. **`UnaryOp`** (line 81): only `"not"` exists today. Delegates to
   `Value::not_three_valued()`.
4. **`BinOp`** (line 88): forwards to `eval_binop`.
5. **`Call`** (line 93): evaluate every arg, then `lookup_builtin` →
   call. Unknown function name → `Null`.

**`eval_binop`** (line 107) is the workhorse:

- `and`: short-circuit on `false`, otherwise `and_three_valued`.
- `or`: short-circuit on `true`, otherwise `or_three_valued`.
- Everything else: evaluate both operands. **If either is `Null`,
  return `Null` immediately** (line 134) — this is the "strict null"
  policy that the `(x == null)` rewrite in `expr.rs` *depends on*.
  Then dispatch on op: `+`, `-`, `*`, `/`, `>`, `>=`, `<`, `<=`, `==`,
  `!=`.

**Arithmetic** (lines 164–210):

- `I64 op I64` uses `saturating_*` (no panic on overflow).
- `I64 / I64` truncates toward zero; division by zero → `Null`.
- `F64 / 0.0` is IEEE-754 — `Inf`, not `Null`.
- Mixed `I64 / F64` promotes the `I64` to `F64`.

**Comparisons** (lines 212–284):

- `try_compare` returns `Option<Ordering>`: `None` for cross-type or
  NaN.
- Ordered (`>`, `>=`, `<`, `<=`) and equality (`==`, `!=`) each
  distinguish "NaN" (→ `Bool(false)`) from "cross-type" (→ `Null`).
  Note that `NaN != NaN` is `Bool(false)` here — that is IEEE-754,
  not a bug.

The remaining lines (≈ 287 onward) are unit tests with helpers
mirroring the AST shape — useful templates for the new node tests.

### Hooks for `@bv.expr`

Step 4 of the plan extends this file:

- **Add a `match` arm per new `Expr` variant** at line 64.
  - `IfElse { cond, then, else }` — evaluate `cond` once; if `Null`,
    return `Null` (the plan's choice); if `Bool(true)`, return
    `eval(then)`; if `Bool(false)`, return `eval(else)`. Any other
    `Value` type → `Null`.
  - `LetBinding { name, value, body }` — evaluate `value` once,
    push it into a scope, evaluate `body` against that scope, pop.
    **Pitfall:** the existing `Row` does not have a "push scope"
    concept; you will either thread a small `HashMap` through
    `eval_depth` or do a substitution pre-pass. The former is
    cleaner and matches the issue's "let-bindings" goal.
  - `Compare { ops, operands }` — short-circuit left-to-right; if
    any pair compares to `Null`, the whole chain is `Null`; if any
    pair compares to `false`, the whole chain is `false`; only if
    every pair is `true` is the chain `true`.
- **3VL stays where it is.** Methods on `Value` already exist;
  reuse them — do not duplicate truth tables in `eval.rs`.
- **Depth-bound the new nodes.** Every recursive call needs
  `depth + 1`.

---

## 6. `python/beava/_agg.py` (793 lines)

### Simple terms

This is the file users call when they want to compute things like
`bv.count(window="24h")` or `bv.sum("amount", where=bv.col("x") > 0)`.
Every helper builds an `AggDescriptor` — a small dict-like object
that serializes to JSON the server understands. The canonical
example in the issue uses these.

### How it works

Module-level helpers (lines 32–84) do all the validation:

- `_validate_window` (line 32): the window pattern is
  `\d+(ms|s|m|h|d)` or `forever`. Used by every windowed op.
- `_validate_half_life` (line 46): same pattern, different argument
  name (for decay ops).
- `_enforce_field_str` (line 51): an early guardrail. If a user
  passes `bv.col("x") * 2` as the `field` arg of a sum, *raise* with
  a structured `RegistrationError(code="schema_mismatch")` and a
  hint to use a two-stage chain. The hint mentions the exact API:
  `events.with_columns(...).group_by(...).agg(...)`.
- `_serialize_where` (line 76): the `where=` kwarg accepts an
  `_Expr` (from `_col.py`). If given, calls `to_expr_string()` —
  that is the *only* place the expression DSL crosses into
  aggregation land.

`AggDescriptor` (line 86): a frozen dataclass with `op`, `field`,
`window`, `half_life`, `where`, and a flexible `extras: dict[str, Any]`
for op-specific kwargs (`q` for quantile, `k` for top_k, etc.).
`to_dict()` (line 103) renders the wire JSON, skipping any field that
is `None` and folding `extras` in last.

**Helper functions** (lines 117 onward) follow one of three patterns:

1. **No-arg ops** (`bv.count`, `bv.first_seen`, `bv.age`, …) —
   `return AggDescriptor(op="...")` with optional `window` /
   `where`.
2. **Field-bearing ops** (`bv.sum`, `bv.mean`, `bv.min`, `bv.max`,
   `bv.var`, `bv.std`, `bv.n_unique`, `bv.entropy`, …) —
   `_enforce_field_str(field, ...)`, validate window, return
   descriptor. Note `bv.sum` uses `# noqa: A001` (line 123) to
   intentionally shadow the Python builtin `sum`.
3. **Field + extras** (`bv.quantile(field, q=...)`,
   `bv.top_k(field, k=...)`, `bv.first_n(field, n=...)`,
   …) — same shape plus an `extras={"q": ...}` etc.

The Polars-vs-SQL aliasing (`avg` is a deprecation alias for
`mean`, `variance` → `var`, `stddev` → `std`, `count_distinct` →
`n_unique`, `percentile` → `quantile`) lives at the bottom of the
file (not shown in detail above).

### Hooks for `@bv.expr`

- **The canonical example uses these as call-sites.** The issue's
  example is roughly:
  ```python
  @bv.expr
  def ClickFeatures(e: ClickEvent):
      return {
          "clicks_24h": bv.count(window="24h"),
          "distinct_urls": bv.distinct_count("url", window="24h"),
          ...
      }
  ```
  i.e. `@bv.expr` returns a dict of `AggDescriptor`s. The tracer
  does not need to *understand* aggregations — it just needs to let
  them flow through as plain Python objects (not symbolic columns).
- **The `where=` kwarg is the only direct coupling.** Today, users
  pass `where=bv.col("x") > 0`. Under `@bv.expr` the body inside
  could be a Python `if` — the AST rewriter converts that to an
  `IfElse` IR node, and if it ends up routed into a `where=` slot,
  `_serialize_where` (line 76) needs to call `to_expr_string()` on
  the new tracer nodes. Make sure the tracer's nodes implement
  `_Expr.to_expr_string()` *exactly the same way* so this path
  works without an `isinstance` change.
- **Field-name strings still flow through unchanged.** The
  `_enforce_field_str` guardrail (line 51) is a *non-goal* under
  `@bv.expr` — that decorator is *for* the case where you would
  otherwise need a two-stage chain. The error message even points
  users at "use a chain" — `@bv.expr` is the alternative.

---

## Cross-file picture (one paragraph)

A user writes Python. `_col.py` and `_agg.py` build small in-memory
descriptors. Those descriptors serialize to strings/JSON and flow
over the wire. The Rust side reads them in `expr.rs` (parser →
`Expr` tree) and `expr_builtins.rs` (function table). At evaluation
time, `eval.rs` walks the `Expr` and computes a `Value` per row,
deferring three-valued logic to methods on `Value` itself. The
`@bv.expr` work *extends every layer at once*: a new public name in
`__init__.py`, two new Python modules for the tracer and rewriter,
three new `Expr` variants in Rust, matching parser rules, matching
evaluator arms. The hook points called out above are the **exact**
spots Steps 2–11 of `issue-56-plan.md` touch.
