# Issue #56 — testing guide

Practical guide for writing the test suites that Steps 8–9 of
`issue-56-plan.md` call for. Each section grounds itself in the
existing test patterns in this repo, points at the exact files to
copy, and lists the specific cases that have to be covered before
the issue can close.

---

## The shape of the test pyramid

Five layers, smallest at the bottom. The acceptance criterion is
that the **canonical example** (top of the pyramid) passes end-to-end.
Every layer below exists so that when the canonical example fails,
the failure points at one specific bug instead of "something is
broken somewhere."

```
                  ┌──────────────────────────────┐
                  │   Canonical example (1 test) │   Step 8
                  ├──────────────────────────────┤
                  │   SDK ↔ server bridge tests  │   Roundtrip
                  ├──────────────────────────────┤
                  │   Python decorator tests     │   Step 9 (Py)
                  │   Python AST-rewriter tests  │
                  │   Python tracer / IR tests   │
                  ├──────────────────────────────┤
                  │   Rust evaluator tests       │   Step 9 (Rust)
                  │   Rust parser tests          │
                  ├──────────────────────────────┤
                  │   Rust AST unit tests        │
                  └──────────────────────────────┘
```

Run gates locally with `bash .github/scripts/check.sh`. The Python
gate uses `python/tests/v0` only (see `CLAUDE.md § Running checks`);
the Rust gate runs every `cargo test` in the workspace.

---

## Rust-side tests

All new Rust tests for this issue live **inline** in the file under
test, inside a `#[cfg(test)] mod tests { ... }` block. That is the
pattern `expr.rs`, `eval.rs`, and `expr_builtins.rs` already use —
do not invent a separate `tests/` directory for unit-scope work.

### Pattern to copy

Read `crates/beava-core/src/eval.rs` lines 286–525 first. The
pattern is:

1. **Builder helpers at the top of the test module.** `field_expr`,
   `lit_null`, `lit_bool`, `lit_int`, `lit_float`, `lit_str`,
   `lit_bare`, `binop`, `unaryop`, `call`, `row_with`. They keep
   tests readable: `binop("+", lit_int(1), lit_int(2))` reads
   directly as the expression it constructs.
2. **One `#[test]` per behavior.** Function name describes the
   behavior in `subject_verb_object` form
   (`eval_arith_div_by_zero_is_null`, `parse_binary_comparison`).
3. **A short comment above each test** stating the invariant being
   pinned, especially when the behavior is surprising. The
   `MAX_i64 + 1 → MAX_i64` saturation test (line 489) is a good
   model.
4. **Use `matches!`** for variant assertions, `assert_eq!` only
   when the entire `Value` should compare equal — `f64` NaN does
   not survive `assert_eq!`.

### Where each new test goes

| Step in plan | File | What to test |
|---|---|---|
| Step 2 — new `Expr` variants | `crates/beava-core/src/expr.rs` `mod tests` | construction, `span()`, `referenced_fields()` |
| Step 3 — parser extension | same | parse-success, parse-error-with-correct-`col`, round-trip span |
| Step 4 — evaluator extension | `crates/beava-core/src/eval.rs` `mod tests` | each new variant under happy path + 3VL |

### `expr.rs` parser tests — what to add

Mirror the existing test naming so the new tests sit cleanly next
to the originals. Names to use:

- `parse_ifelse_basic` — `(if (a > 0) then b else c)` produces an
  `Expr::IfElse` with the expected child shapes. Assert `span` covers
  the whole expression.
- `parse_ifelse_in_arithmetic` — `((if (x > 0) then 1 else 0) + 5)`
  composes inside `BinOp("+")`.
- `parse_let_basic` — `(let n = (x + 1) in (n * n))` produces an
  `Expr::LetBinding` with `name="n"`, child shapes correct,
  `referenced_fields()` returns `{"x"}` (NOT `{"n", "x"}` — let's
  bound name shadows).
- `parse_compare_chain` — `(a < b < c)` produces a single
  `Expr::Compare` with `ops=["<", "<"]` and `operands=[a, b, c]`.
  Specifically assert this is NOT `BinOp("<", BinOp("<", a, b), c)`.
- `parse_compare_single` — `(a < b)` still produces `BinOp("<", …)`
  (chained-comparison logic only kicks in for 2+ operators in a row).
- `parse_ifelse_error_missing_else` — input `(if a then b)` errors
  with `col` pointing at the `)`. Use the existing `parse_rejects_*`
  pattern (line 1405, 1419).
- `parse_let_error_missing_in` — same idea: `(let n = 1)` errors at
  the `)`.
- `parse_compare_normalisation_through_null_rewrite` — confirm the
  existing `rewrite_null_eq` pass still fires *inside* `IfElse`
  branches: `(if (x == null) then 1 else 0)` becomes
  `IfElse { cond: Call("isnull", [x]), …, … }`. This is the test
  that catches a missing recursion arm in the normalization passes.

### `eval.rs` evaluator tests — what to add

The existing 3VL behaviors are pinned by tests around lines 506–525.
Match that density for the new nodes:

- `eval_ifelse_true_branch` / `eval_ifelse_false_branch` — happy path.
- `eval_ifelse_null_condition_is_null` — pin the design decision
  from Step 1.3 of the plan: a `null` predicate makes the whole
  expression `null`. Without this test, a regression could silently
  flip the branch.
- `eval_ifelse_non_bool_condition_is_null` — `cond` evaluating to
  `Value::I64(5)` (or any non-bool) is `null`. Defensive.
- `eval_ifelse_short_circuits` — the unused branch must not be
  evaluated. Construct a branch that would panic if evaluated (a
  deep-recursion expression at depth 511, for example) and confirm
  the result is the *other* branch. Without this, "evaluate both
  then pick one" passes happy-path tests but breaks `MAX_EVAL_DEPTH`.
- `eval_let_value_evaluated_once` — write a `LetBinding` whose
  `body` references the bound name twice; assert via a side-channel
  (or by counting field lookups against a tracking `Row`) that
  `value` was only evaluated once. This is the core perf invariant.
- `eval_let_shadowing` — `(let x = 1 in (x + 1))` against a row that
  already has `x = 99` returns `2`, not `100`. Pins lexical scope.
- `eval_let_nested_shadowing` — `(let x = 1 in (let x = 2 in x))`
  returns `2`. Pins inner shadow.
- `eval_compare_chain_all_true` — `(1 < 2 < 3)` is `Bool(true)`.
- `eval_compare_chain_mid_false` — `(1 < 5 < 3)` is `Bool(false)`.
  Specifically: the third operand must NOT be evaluated. Pin
  short-circuit the same way as `eval_ifelse_short_circuits`.
- `eval_compare_chain_with_null` — `(1 < null < 3)` is `null`, NOT
  `false`. This is the most likely place to leak naive comparison
  semantics.
- `eval_compare_chain_arity_two` — `(1 < 2)` through the `Compare`
  path matches `Bool(true)` (assuming we route 2-operand chains
  through the same variant — if not, drop this test).

### `expr_builtins.rs` tests — only if you add builtins

The plan does not add builtins. If Step 1 decides to surface
`bv.coalesce` / `bv.is_null` aliases, follow the existing pattern:

- `lookup_builtin_returns_<name>` (lines 261–272 of
  `expr_builtins.rs`) — confirms the table entry exists.
- One `<name>_<input>_<expected>` test per matrix cell. The `cast`
  block at lines 307–425 is the template.

### Crate-level invariants

The grammar comment at the top of `expr.rs` (lines 6–19) is locked
documentation. **Update it as part of the same PR** that adds the
new parser rules. If it drifts, a future reader (and a future PR
reviewer) cannot tell the documented grammar from the implemented
one. Add a `parse_grammar_doc_matches_impl` test only if you find
yourself doing it manually a second time — premature otherwise.

---

## Python-side tests

All new Python tests go under `python/tests/v0/`. CI gates on
that directory only; placing tests anywhere else is "documented
drift" (`check.sh` mentions this explicitly). File-naming
convention: `test_<feature>.py`. One file per concern, not one
file per class under test.

### Pattern to copy

Read `python/tests/v0/conftest.py` and `python/tests/v0/_helpers.py`
first. The pattern is:

1. **Every test file declares an engine-availability skipif** at
   the module top, using the `_engine_available()` helper:
   ```python
   pytestmark = pytest.mark.skipif(
       not _engine_available(),
       reason="requires @bv.expr decorator (issue #56)",
   )
   ```
   Add `"expr"` to the `required_helpers` tuple in
   `conftest.py::_engine_available()` so the skip becomes a pass
   once the decorator ships. Without this, `pytest -q` from a
   half-built tree fails collection.
2. **`app` fixture per test for integration tests** (defined in
   `conftest.py`). Pure-IR tests do not need it.
3. **Module docstring states the contract.** `test_lit.py` (top of
   file) is the template — list the behaviors covered, not
   implementation details.
4. **Use `from __future__ import annotations`** at the top of
   every file (matches the rest of the SDK).
5. **No mocks for the server.** Integration tests spawn the real
   `beava` binary via the `app` fixture. The plan's Step 8 test
   *must* run end-to-end; mocking the server out is documented as
   a beava-specific don't (see `CLAUDE.md` and your memory).

### Files to create

| File | Step | Scope |
|---|---|---|
| `test_expr_ast_rewrite.py` | Step 6 | the rewriter alone — input function → IR tree |
| `test_expr_tracer.py` | Step 5 | `_SymbolicCol` ops produce the right IR nodes |
| `test_expr_null_logic.py` | Step 4 (Py-side observation of) | 3VL behavior visible at the SDK |
| `test_expr_return_types.py` | Step 7 | type-inference rule from Step 1.4 |
| `test_expr_composition.py` | Step 7 | `@bv.expr` calling another `@bv.expr` |
| `test_expr_clickfeatures.py` | Step 8 | the canonical-example acceptance test |
| `test_expr_wire_roundtrip.py` | bridge | Python IR → wire string → Rust parse |

### `test_expr_ast_rewrite.py` — what to cover

This file does **not** need the `app` fixture — it tests the
rewriter in isolation. For each supported and each rejected Python
construct, write one test. From the plan Step 1.3:

**Accepted constructs** (one positive test each — assert the IR
shape):

- `if cond: a else: b` → `_SymIfElse(cond, a, b)`
- `x and y` → `_SymBinOp("and", x, y)`
- `x or y` → `_SymBinOp("or", x, y)`
- `not x` → `_SymUnaryOp("not", x)`
- `a < b < c` → `_SymCompare([...], [...])` (not nested binops)
- `a == None` and `a is None` → `_SymCall("isnull", [a])` (mirror
  the `rewrite_null_eq` Rust pass at SDK level *or* let the wire
  string drive it — but pick one and pin which)
- Intermediate locals (`tmp = expr; return tmp * 2`) →
  `_SymLetBinding("tmp", expr, expr * 2)`

**Rejected constructs** (one negative test each — assert it raises
with a useful error message pointing at the original source line):

- `for x in ...`
- `[i for i in ...]` (list-comp)
- `lambda x: x + 1`
- `while ...`
- `def inner(): ...` (nested def)
- attribute access on a non-event value (`"hello".upper()`)
- recursive call: `@bv.expr def f(e): return f(e)`
- `try: / except:`
- `*args` / `**kwargs` in the decorated function signature
- `return` with no value
- multiple `return` paths (Step 1.3 may or may not allow these —
  decide once, test the decision)

For the negative tests, use `pytest.raises(SyntaxError, match=...)`
(or whatever exception type Step 6 settles on) and assert the line
number in the message points at the user's *original* source, not
the rewritten code. That is the user-experience promise of Step 7.4.

### `test_expr_tracer.py` — what to cover

This file tests `_SymbolicCol` against the same matrix that
`python/beava/_col.py` already covers for `_Col` / `_BinOp`, but on
the tracer's node types. The single most important guarantee:

```python
def test_tracer_emits_same_wire_string_as_col():
    """A traced expression must wire-serialize identically to bv.col(...)."""
    sym = _SymbolicCol("amount", type_=int)
    traced = (sym > 100) & (sym < 1000)
    direct = (bv.col("amount") > 100) & (bv.col("amount") < 1000)
    assert traced.to_expr_string() == direct.to_expr_string()
```

If that one assertion ever fails, the tracer has diverged from the
canonical SDK and the server will accept one form and reject the
other. Pin it.

Additional cases:

- arithmetic (`+`, `-`, `*`, `/`) including reversed operators
  (`1 + sym` exercising `__radd__`)
- chained boolean (`a & b & c` is left-associative? right? — test
  whichever the impl chooses and **comment why**)
- `~sym` → `_SymUnaryOp("not", …)` and serializes to `!(amount)`
- nullable column: `_SymbolicCol("opt", type_=Optional[int])`
  preserves the nullable flag through binops (this is what powers
  the return-type inference test below)

### `test_expr_null_logic.py` — what to cover

A 3VL truth-table file — one test per cell. Build the truth tables
in Step 1.2 of the plan; this file is just the codification. Suggested
structure:

```python
@pytest.mark.parametrize("a, b, expected", [
    (True,  True,  True),
    (True,  False, False),
    (True,  None,  None),
    (False, True,  False),
    (False, False, False),
    (False, None,  False),  # short-circuit
    (None,  True,  None),
    (None,  False, False),
    (None,  None,  None),
])
def test_and_truth_table(app, a, b, expected): ...
```

Three tables: `and` (9 cells), `or` (9 cells), `not` (3 cells).
Plus arithmetic-with-null (one test asserting `null + 5 == null`
through the SDK surface) and comparison-with-null (one test
asserting `null < 5 == null`, NOT `False`). The last is the
behavior the `rewrite_null_eq` pass exists to preserve — pin it
end-to-end.

### `test_expr_return_types.py` — what to cover

One test per rule from Step 1.4:

- literal `5` → output type `int`
- literal `"web"` → `str`
- `_SymbolicCol("x", int)` → `int`
- `_SymbolicCol("y", Optional[int])` → `Optional[int]`
- binary arithmetic `int + int → int`, `int + float → float`,
  `int + Optional[int] → Optional[int]`
- comparison `int > int → bool`
- `if cond: a else: b` → least-upper-bound of `a` and `b` types
- `is None` check → `bool`
- composition: passing the output of one `@bv.expr` to another
  preserves the type

### `test_expr_composition.py` — what to cover

The plan calls composition out explicitly. Tests:

- `@bv.expr f` returning a `_Sym*` node; another `@bv.expr g`
  consuming `f(event)` as a sub-expression; the resulting IR
  inlines `f`'s body (or references it by name — pin whichever
  Step 7 settles on).
- The composed IR's `referenced_fields()` (or the SDK equivalent)
  is the *union* of both bodies.
- Two `@bv.expr` functions that reference the same intermediate —
  ensure the IR cache from Step 7.5 returns the same object both
  times.

### `test_expr_clickfeatures.py` — the acceptance test

This is the **one** test the entire issue is gated on. From the
plan:

> If the example doesn't pass, every other test passing is
> irrelevant — this is the issue's done-bar.

Structure:

```python
def test_clickfeatures_end_to_end(app):
    """The issue's canonical example, end-to-end against a running server."""

    @bv.event
    class Click:
        user_id: str
        url: str | None
        ms_on_page: int

    @bv.expr
    def ClickFeatures(c: Click):
        # exact body from the issue's canonical example
        ...

    app.register(ClickFeatures)
    # push N events including some with url=None
    # query the resulting table for one user_id
    # assert: clicks_24h matches Python-computed ground truth
    # assert: distinct_urls excludes None (3VL contract)
    # assert: cold-start equivalent for unseen users
```

Use `compute_expected_per_entity` from
`python/tests/v0/_helpers.py` for ground-truth comparison
(template at the top of that file). Use the `app` fixture and
`cold_start_equivalent` for the cold-start assertion. **Do not
mock the server** — spawn the real binary via the fixture.

### `test_expr_wire_roundtrip.py` — the bridge

The most powerful test against silent IR drift between Python and
Rust:

```python
@pytest.mark.parametrize("python_expr", [
    "x > 0",
    "x is None",
    "1 < x < 10",
    "x if y > 0 else 0",
    # … one entry per construct from Step 1.3
])
def test_wire_roundtrip(python_expr):
    """SDK serialize → server parse must round-trip every supported form."""
    @bv.expr
    def F(e: SomeEvent):
        return eval(python_expr, {"x": ..., "y": ...})  # build the AST
    wire = F.to_wire_string()  # or whatever Step 7 exposes
    # call into the Rust parser via PyO3, or via a subprocess CLI helper, or
    # by sending the register payload and asserting no parse error.
    assert _server_parse_ok(wire)
```

The exact mechanism depends on Step 1.1's decision (string vs. JSON
IR). If string: the assertion is "the server accepts it." If JSON:
the assertion is "a JSON shape comparison against a recorded golden
file." Pick one and stick with it.

---

## Cross-cutting practices

Things to do everywhere — easy to forget mid-implementation.

- **Pin spans, not just shapes.** A parser test that checks
  `matches!(expr, Expr::IfElse { .. })` but ignores `span` will
  pass even when the span computation is wrong. The existing
  `parse_bare_field` test (`expr.rs` line 1170) asserts both — copy
  that idiom.
- **Pin error messages, not just error variants.** The existing
  `parse_rejects_*` tests (line 1405+) assert that the error reason
  contains `"col N:"` and a useful snippet. New error paths should
  do the same — users diagnose feature-config bugs from these
  strings.
- **Pin negative cases.** For every "this thing works" test, write
  the matching "this similar-but-invalid thing fails clearly" test.
  Half the existing test count in `expr.rs` is rejection tests; the
  new code should match that ratio.
- **Test against the wire format, not the Python AST.** If a test
  passes by inspecting `_Sym*` node shapes but the wire string is
  wrong, the server has no way to tell. The `tracer_emits_same_wire_string_as_col`
  guarantee covers this; every IR-shape test should have a sibling
  that checks `.to_expr_string()` too.
- **Use `# type: ignore[...]` sparingly.** Mypy is advisory in
  `check.sh` (lines around `mypy --strict`), but reviewers still
  read the noise. Prefer real type annotations.
- **Three-valued logic is the most-bug-prone surface.** Spend test
  budget here disproportionately. The Step 9 `test_expr_null_logic.py`
  file is small in LOC but earns its keep — a regression in
  `null < 5` returning `false` instead of `null` is exactly the
  kind of silent bug that ships and causes incidents.
- **One assertion per concept.** A test named
  `eval_ifelse_short_circuits` should assert short-circuit behavior
  and nothing else. If you also want to assert the result value,
  write a separate test. Multi-assertion tests are harder to
  diagnose when one assertion fails.

---

## What "done" looks like for testing

For Step 9 to be complete, all of this must be true at once:

1. `bash .github/scripts/check.sh` passes with zero failures.
2. `cargo test --workspace --features testing` passes (this is the
   Rust gate inside `check.sh`).
3. `cd python && python -m pytest tests/v0 -q` passes (this is the
   Python gate inside `check.sh`).
4. The canonical-example test (`test_expr_clickfeatures.py`) is
   present and green.
5. Every truth-table cell in the 3VL truth tables has a passing
   test row.
6. Every rejected-Python-construct from Step 1.3 has a failing-
   intentionally test (asserting it raises, not silently producing
   garbage IR).
7. Spans in new parser tests cover the full source range, not just
   `Span { start: 0, end: 0 }`.
8. The `_engine_available()` check in `conftest.py` includes
   `"expr"` so the v0 suite stops auto-skipping these tests.

The check.sh output, copy-pasted into the PR description's
Verification section, is the proof. The script formats it as a
ready-to-paste markdown block on its way out.
