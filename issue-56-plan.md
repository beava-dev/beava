# Issue #56 — `@bv.expr` symbolic Python frontend: step-by-step plan

**Issue:** [beava-dev/beava#56](https://github.com/beava-dev/beava/issues/56) — *feat: @bv.expr symbolic Python frontend (v0.1 capstone)*

This document breaks the issue into concrete, ordered steps. Each step
states what to build, where it lives in the repo, and (in plain terms)
why it matters.

---

## What we are building, in one paragraph

Today a user writes feature logic with `bv.col("amount") > 100` — an
operator-overloaded mini-DSL (see `python/beava/_col.py`). The issue
asks us to add a second, friendlier front door: a `@bv.expr` decorator
that lets the user write **plain Python** inside a function, and the
SDK quietly turns that Python into the same JSON expression IR the
server already consumes. The Rust expression engine
(`crates/beava-core/src/expr.rs`) gets a few new node types so it can
represent `if/else`, `let`-bindings, and chained comparisons that
plain-Python users naturally write but the current grammar cannot.

---

## Step 0 — Read the existing pieces first (orientation, ~30 min)

Before writing anything, read these files end-to-end. Most of the work
is *extending* them, not replacing them.

1. `python/beava/_col.py` (221 lines) — the current operator-overload
   AST (`_Col`, `_Literal`, `_BinOp`, `_UnaryOp`, `_CastOp`). The new
   tracer's symbolic objects will look very similar.
2. `python/beava/__init__.py` — the public `bv.*` surface; this is
   where `bv.expr` must be exported.
3. `crates/beava-core/src/expr.rs` (1852 lines) — the Rust AST
   (`Expr::Field | Literal | BinOp | UnaryOp | Call`) and the parser.
   We will add three new variants.
4. `crates/beava-core/src/expr_builtins.rs` — the table of callable
   functions (`cast`, `isnull`, …). Three-valued-logic helpers go here.
5. `crates/beava-core/src/eval.rs` — the evaluator. New nodes need
   `match` arms here.
6. `python/beava/_agg.py` — to understand how aggregations like
   `bv.count` / `bv.distinct_count` are currently expressed, since the
   canonical example wires them in.

**Why first:** the issue says ~500 LOC of Rust and ~1000 LOC of Python.
That is small *only* if we reuse what is there. Skipping orientation is
the fastest way to write a parallel system that duplicates `_col.py`.

---

## Step 1 — Decide the IR shape (design, ~half a day, no code)

Open a short design note (can be inline in the PR description). Lock
down four things:

1. **JSON shape of the new nodes.** The Rust parser already speaks an
   expression-string grammar. The decorator does *not* need to emit a
   string — it can emit JSON directly. Pick one of:
   - *(a)* Extend the string grammar with `if … then … else …` and
     `let x = … in …`. Pro: one IR for everyone. Con: parser churn.
   - *(b)* Add a parallel JSON IR variant that the server accepts
     alongside the string form. Pro: zero parser changes. Con: two
     IRs to keep in sync.
   - **Recommended: (a)** — the issue explicitly extends `expr.rs`, so
     the canonical IR is the Rust enum and the string grammar that
     feeds it.
2. **Null semantics.** Three-valued logic = `true`, `false`, `null`.
   Write down the truth table for `and`, `or`, `not`, comparisons,
   `==`, `!=`. This decides 80% of the test cases later.
3. **Allowed Python subset.** From the issue: `if/else`, `and/or`,
   comparisons, `is None`. Explicitly *not* allowed: loops,
   comprehensions, lambdas, attribute access on non-event objects,
   recursion. Write this list down — it is what the AST rewriter
   *rejects*.
4. **Return-type inference rule.** Bottom-up: literal → its Python
   type; `_SymbolicCol` → its schema type; `if a else b` → join of
   branches; arithmetic → numeric widening; comparisons → bool. Pin
   the rule before coding.

**Why:** all four are choices that look small but pin down dozens of
downstream test cases. Make them once, write them down, stop
re-litigating.

---

## Step 2 — Rust: add the new AST variants (Rust, ~150 LOC)

File: `crates/beava-core/src/expr.rs`.

Add to `pub enum Expr` (around line 87):

```rust
IfElse { cond: Box<Expr>, then_branch: Box<Expr>, else_branch: Box<Expr>, span: Span },
LetBinding { name: String, value: Box<Expr>, body: Box<Expr>, span: Span },
Compare { ops: Vec<String>, operands: Vec<Expr>, span: Span },
```

For each new variant:

1. Add a match arm in `Expr::span()` (line 113).
2. Add a match arm in `collect_fields()` (line 134) — `LetBinding`
   must *shadow* `name`: do not surface the bound name as a referenced
   field.
3. Add a match arm in `eval.rs` (find every `match expr` over the enum
   — `rustc` will list them all once the enum grows).

**Compare vs nested BinOp:** Python's `a < b < c` is a *chained*
comparison, not `(a < b) < c`. The `Compare` node holds parallel
`ops` and `operands` (lengths `n` and `n+1`). The evaluator
short-circuits left-to-right and applies 3VL between each pair.

**Why a new node and not desugaring to `BinOp`:** desugaring loses
short-circuit semantics on null (`a < null < c` should be `null`,
not `false`).

---

## Step 3 — Rust: extend the parser (Rust, ~150 LOC)

Still `crates/beava-core/src/expr.rs`. Grow the recursive-descent
`Parser` to recognise:

- `if <expr> then <expr> else <expr>` — ternary, lowest precedence
  above `or`.
- `let <ident> = <expr> in <expr>` — bind a sub-expression.
- Chained comparisons: when `parse_compare` sees a second relational
  operator at the same precedence level, fold into a `Compare` node.

Add tests beside the existing parser tests (search for `#[test]` in
the file). One test per new construct: success, parse-error message,
span correctness.

**Why parser too, not just enum:** the SDK could emit JSON directly,
but every other consumer (config files, the CLI, examples) goes
through the string parser. Two IRs would drift.

---

## Step 4 — Rust: three-valued logic in the evaluator (Rust, ~100 LOC)

Files: `crates/beava-core/src/eval.rs`,
`crates/beava-core/src/expr_builtins.rs`.

Implement the truth tables from Step 1.3. Concretely:

- `null and false` = `false`, `null and true` = `null`,
  `null and null` = `null`, mirror for `or`.
- `not null` = `null`.
- Any comparison with a `null` operand = `null` (not `false`).
- `IfElse` with a `null` condition = `null`.
- `LetBinding` evaluates `value` once and substitutes into `body` —
  do **not** re-evaluate on each reference.

Add a `nullable` helper or a `Value::Null` arm — whichever the existing
evaluator already uses (read first, do not invent).

**Why:** the issue calls out "null-aware semantics" as a goal. The
canonical example `bv.count()` over an Optional field is meaningless
unless `null` rows are filtered correctly by `if x is not None else …`.

---

## Step 5 — Python: the `_SymbolicCol` tracer (Python, ~250 LOC)

New file: `python/beava/_expr_tracer.py`.

A `_SymbolicCol` is the runtime stand-in passed to a `@bv.expr`
function in place of a real value. Every operator on it returns a new
symbolic node, exactly the pattern in `_col.py`. The bulk of this file
is the symbolic node hierarchy:

- `_SymCol(name, type)` — references an event field.
- `_SymLit(value)` — Python literal lifted into the IR.
- `_SymBinOp`, `_SymCompare`, `_SymIfElse`, `_SymLetBinding`,
  `_SymCall` — IR-node mirrors.
- `.to_expr_string()` on each, identical contract to `_col.py`.

**Why a new file rather than extending `_col.py`:** `_col.py` is the
*public* expression DSL. The tracer is the *implementation detail* of
the decorator. Keeping them separate stops the public surface from
growing accidentally.

**Reuse:** the tracer's nodes should serialise to the same wire JSON
as `_col.py`'s nodes so the server cannot tell them apart. Lift the
serialisation into a shared helper if it starts to drift.

---

## Step 6 — Python: the AST rewriter (Python, ~400 LOC)

New file: `python/beava/_expr_ast.py`.

Use the stdlib `ast` module. A `@bv.expr` decorator:

1. Reads the function's source via `inspect.getsource`.
2. Parses it to an `ast.Module`.
3. Walks the body with an `ast.NodeTransformer` that converts:
   - `if cond: a else: b`  →  `_SymIfElse(cond, a, b)`
   - `x and y` / `x or y`  →  `_SymBinOp("and"/"or", x, y)`
   - `a < b < c`           →  `_SymCompare([...], [...])`
   - `x is None`           →  `_SymCall("isnull", [x])`
   - intermediate locals   →  `_SymLetBinding(name, value, rest)`
4. Compiles the rewritten module and exec's it. The function's body
   now traces against `_SymbolicCol` arguments instead of running
   normal Python.
5. Any unsupported node (loop, comprehension, lambda, attribute on a
   non-symbolic value) raises a `SyntaxError`-style error pointing at
   the original source line.

**Why an AST rewriter at all (vs. operator overloads alone):** Python
forbids overloading `if`, `and`, `or`, and `is`. Operator overloads
alone get you arithmetic and `&`/`|` (which `_col.py` already does).
The rewriter is the *only* way to capture control flow.

---

## Step 7 — Python: the `@bv.expr` decorator (Python, ~150 LOC)

New file: `python/beava/_expr_decorator.py`. Export from
`python/beava/__init__.py` as `bv.expr`.

The decorator does five things in order:

1. Look up the *event schema* the function takes as its argument
   (type annotation or explicit `bv.event(...)` reference).
2. Build a `_SymbolicCol` for each parameter, typed against the
   schema. `Optional[T]` parameters become nullable.
3. Call the AST-rewriter (Step 6) on the function, then execute the
   rewritten body with the symbolic args. The return value is a
   `_Sym*` node.
4. Run **type inference** bottom-up (Step 1.4) over the resulting
   node to derive the feature's output type. Surface errors with the
   *original* source line — never with the rewritten code.
5. Cache the produced IR on the decorator wrapper so repeat calls
   inside `App.register(...)` are O(1).

**Why a cache:** registration paths can re-evaluate the same feature
many times; users will notice the slowdown otherwise.

---

## Step 8 — Wire the canonical example end-to-end (~100 LOC of tests)

The issue gives a `ClickFeatures` canonical example. Turn it into one
integration test under `python/tests/` (e.g.
`test_expr_clickfeatures.py`). Assert:

- The IR JSON for each derived field is exactly what we expect.
- Running the feature against a sample event stream produces the
  expected numeric output.
- An `Optional[str]` field with a `None` value flows through
  `bv.count()` and `bv.distinct_count()` without crashing and with
  the *right* count (nulls excluded).

If the example doesn't pass, *every other test passing is irrelevant*
— this is the issue's done-bar.

---

## Step 9 — Targeted test suites (parallel to Steps 5–7)

Under `python/tests/`:

- `test_expr_ast_rewrite.py` — every supported `ast` node and every
  rejected one. One test per construct from Step 1.3.
- `test_expr_null_logic.py` — every cell of the 3VL truth tables.
- `test_expr_composition.py` — `@bv.expr` calling another `@bv.expr`,
  to confirm IR composition (the issue calls this out).
- `test_expr_return_types.py` — type inference for every combination
  from Step 1.4.

Under `crates/beava-core/`:

- New unit tests in `expr.rs` for the three new nodes (parse, span,
  `referenced_fields`).
- New unit tests in `eval.rs` for 3VL evaluation.

**Why split SDK and server tests:** the issue lists both as
acceptance criteria. The Python tests prove the rewriter works; the
Rust tests prove the IR semantics are right *independent of the SDK*.

---

## Step 10 — Write `CONTRIBUTING-OPS.md` (~200 lines of prose)

New top-level file. The issue calls this out specifically and ties the
release to it. Cover:

1. What an "operator" / "expression node" is in beava — one diagram,
   one paragraph.
2. How a Python user writes `@bv.expr` (link to a worked example,
   probably copy the `ClickFeatures` test in).
3. How a contributor adds a *new* expression node end-to-end:
   - Rust enum variant + span + `collect_fields` arm.
   - Parser rule + parser test.
   - Evaluator arm + evaluator test.
   - Python tracer node + serialisation.
   - AST rewriter mapping (if the new node is reachable from plain
     Python).
4. The 3VL truth tables, verbatim from Step 1.2.
5. The supported-vs-rejected Python subset, verbatim from Step 1.3.

**Why it matters this much:** the issue explicitly bars merging
without this file. Treat it as a deliverable on par with the code.

---

## Step 11 — Public-surface and changelog

1. `python/beava/__init__.py`: add `expr` to the imports and to
   `__all__`.
2. `CHANGELOG.md`: one entry under the next unreleased version
   describing the new decorator and the three new IR nodes.
3. `docs/python/` (or wherever the SDK reference lives): one new page,
   `expr.mdx`, with the canonical example. Link from the existing
   `bv.col` page so users find it.

---

## Step 12 — PR shape

One PR. The issue calls this the *v0.1 capstone*; splitting it across
PRs hides the integration. Order the commits so a reviewer can read
the stack top-down:

1. Rust enum + parser + evaluator (Steps 2–4) — green on its own.
2. Python tracer + AST rewriter + decorator (Steps 5–7).
3. Tests (Steps 8–9).
4. Docs (Step 10 + Step 11).

Run `bash .github/scripts/check.sh` locally before pushing — it is
what CI runs.

---

## Explicitly *not* doing (from the issue)

Do not even start on any of these inside this PR — they each deserve
their own follow-up:

- Nested types (lists, structs, maps inside an event).
- A Python subprocess fallback for unsupported nodes.
- Variadic arguments (`*args` / `**kwargs`) inside `@bv.expr`.
- Recursive `@bv.expr` calls.
- Cross-event joins.

If a reviewer asks for any of these, push back with a link to the
issue's "Out of Scope" section.

---

## Suggested timeline (calendar, not effort)

| Day | Work |
| --- | --- |
| 1   | Steps 0–1 (orientation + design note) |
| 2–3 | Steps 2–4 (Rust enum, parser, evaluator) — green CI |
| 4–6 | Steps 5–7 (Python tracer, rewriter, decorator) |
| 7   | Step 8 (canonical example end-to-end) |
| 8   | Step 9 (test coverage to the done-bar) |
| 9   | Step 10 (`CONTRIBUTING-OPS.md`) |
| 10  | Step 11 + Step 12 (public surface, changelog, PR) |

Two weeks of calendar with buffer; one focused week is plausible if
Step 1's design choices land cleanly.
