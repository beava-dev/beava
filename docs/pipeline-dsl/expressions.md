# Pipeline DSL Expressions (`bv.col`)

> **Status:** Authoritative for v0. Documents the **post-13.5 target** Python
> expression DSL. The current `python/beava/_col.py` already implements every
> surface in this doc — Phase 13.5 polishes naming + cross-language parity.
> **Last reviewed:** 2026-05-03 (Phase 13.0).

## Overview

`bv.col(name)` constructs a **column-reference expression** — a leaf in an
operator-overloaded AST. Composing it with arithmetic, comparison, boolean,
and method calls yields more nodes; calling `.to_expr_string()` (the SDK
does this implicitly at register time) emits a canonical parenthesised
string the server's expression evaluator parses back into a predicate.

The grammar is **locked** — the server-side parser depends on the canonical
shape; SDK ports MUST produce the same string for the same Python source.

## Grammar (canonical)

```
expr      := field | literal | bin_op | unary_op | call
field     := identifier | identifier "." identifier         # e.g. x, Stream.x
literal   := number | "'" string "'" | "true" | "false" | "null"
bin_op    := "(" expr <space> op <space> expr ")"           # EVERY binary op is parenthesized
op        := "+" | "-" | "*" | "/" | ">" | ">=" | "<" | "<=" | "==" | "!=" | "and" | "or"
unary_op  := "(" "not" <space> expr ")"
call      := ident "(" expr ("," <space> expr)* ")"
```

String literal escaping: `\\` becomes `\\\\`; `'` becomes `\\'`. This is the
single mitigation point for predicate-string injection from user-supplied
strings (T-03-02-01).

## `bv.col(name)` — column reference

```python
import beava as bv

amount = bv.col("amount")            # Field('amount')
amount.to_expr_string()              # "amount"

# Qualified field reference (used when ops cross multiple upstream sources):
foreign = bv.col("Txn.amount")
foreign.to_expr_string()             # "Txn.amount"
```

Args:

- `name` — non-empty string. `TypeError` if absent / empty.

Returns: an `_ExprAST` leaf node, ready to compose with operators.

## Arithmetic: `+ - * /`

The four binary arithmetic operators are overloaded on `_ExprAST` and accept
either another expression or a Python scalar (auto-wrapped via `bv.lit(...)`):

```python
(bv.col("a") + bv.col("b")).to_expr_string()        # "(a + b)"
(bv.col("amount") * 2).to_expr_string()             # "(amount * 2)"
(bv.col("amount") / 100).to_expr_string()           # "(amount / 100)"
(5 - bv.col("balance")).to_expr_string()            # "(5 - balance)"
```

Both forms (left-operand or right-operand scalar) compile correctly because
`_ExprAST` implements both `__add__` and `__radd__` (and the same for `-`,
`*`, `/`).

**Type rules** (server-side, applied at register time during schema
propagation):

- Both operands must be numeric (`i64` or `f64`); `bool` is NOT numeric in
  arithmetic context. Cast first via `.cast("int")`.
- Division (`/`) always widens to `f64` to avoid integer-truncation surprises.
- Otherwise, `f64 + i64 → f64`; `i64 + i64 → i64`.

## Comparison: `> >= < <= == !=`

```python
(bv.col("amount") > 100).to_expr_string()           # "(amount > 100)"
(bv.col("amount") >= 100).to_expr_string()          # "(amount >= 100)"
(bv.col("status") == "ok").to_expr_string()         # "(status == 'ok')"
(bv.col("status") != "ok").to_expr_string()         # "(status != 'ok')"
```

All comparison ops return `bool` regardless of operand types. String
literals on the right-hand side are auto-quoted and backslash-escaped per
the grammar.

## Boolean combinators: `& | ~`

Python's keyword `and / or / not` cannot be operator-overloaded; beava uses
`& / | / ~` instead and emits the keywords in the canonical grammar:

```python
left = bv.col("amount") > 100
right = bv.col("merchant") == "amazon"

(left & right).to_expr_string()   # "((amount > 100) and (merchant == 'amazon'))"
(left | right).to_expr_string()   # "((amount > 100) or (merchant == 'amazon'))"
(~left).to_expr_string()          # "(not (amount > 100))"
```

**Both operands MUST be boolean** — the server-side type inference rejects
`bool & i64` etc. with `schema_mismatch`. Use `.cast("bool")` if you have a
0/1 column you want to combine; or use `(col != 0)` to coerce first.

**Operator precedence:** Python's `&` and `|` bind **tighter** than `>` /
`==`. To get the obvious "either of two predicates" reading, parenthesise:

```python
# WRONG — Python parses this as `bv.col("a") > (100 & bv.col("b")) > 0`
bv.col("a") > 100 & bv.col("b") > 0

# RIGHT
(bv.col("a") > 100) & (bv.col("b") > 0)
```

The SDK does NOT detect or rewrite the wrong form; it produces a
schema-mismatch error at register time. Always parenthesise your sub-predicates.

## `.isnull()` — null-check

```python
bv.col("amount").isnull().to_expr_string()        # "(amount == null)"
```

Shorthand for the `(col == null)` form. Emitted as a `_BinOp` so it composes
with boolean combinators:

```python
non_null_pos = (~bv.col("amount").isnull()) & (bv.col("amount") > 0)
non_null_pos.to_expr_string()
# "((not (amount == null)) and (amount > 0))"
```

Use `.isnull()` rather than `(col == None)` for clarity — both compile to
the same wire form, but `.isnull()` reads better in chains.

## `.cast(type_name)` — type coercion

```python
bv.col("flag_str").cast("bool").to_expr_string()  # "cast(flag_str, bool)"
bv.col("amount").cast("int").to_expr_string()     # "cast(amount, int)"
```

Args:

- `type_name` — one of `"str"`, `"int"`, `"float"`, `"bool"`. Other strings
  are rejected at decoration time with `ValueError`.

`.cast()` is a `_Call` node (not a `_BinOp`); the canonical form is
`cast(<expr>, <type>)`. The cast target name renders as a **bare
identifier** (NOT a quoted string), so the server's parser can dispatch on
the type without a string-strip.

**Use case 1 — within `with_columns(...)` to derive a new column:**

```python
@bv.event
def TxnWithFlagInt(txn: Txn) -> bv.Event:
    return txn.with_columns(is_fraud_int=bv.col("is_fraud").cast("int"))
```

This produces a new `is_fraud_int` column on the downstream derivation that
aggregations can then sum. **This is the recommended boolean-sum pattern** —
see [compilation-rules.md § Boolean-sum trick](compilation-rules.md#boolean-sum-trick-recommended-pattern-for-conditional-counts).

**Use case 2 — within `where=` predicates:**

```python
@bv.table(key="user_id")
def UserStats(txn) -> bv.Table:
    return (
        txn.group_by("user_id")
           .agg(big_count=bv.count(window="1h",
                                    where=bv.col("amount").cast("int") > 100))
    )
```

## `.alias(name)` — rename in derivation context

> **Status:** Implemented post-13.5; current code uses `**kwargs` naming on
> `with_columns(name=expr)` instead. SDK porters in 13.6 may add `.alias()`
> as a convenience method. Documented here for forward compatibility.

```python
expensive = (bv.col("amount") > 100).alias("is_expensive")
```

In the v0 API the recommended form is the kwarg-as-name shape:

```python
@bv.event
def TxnAnnotated(txn: Txn) -> bv.Event:
    return txn.with_columns(is_expensive=bv.col("amount") > 100)
```

The kwarg name (`is_expensive`) becomes the new column name on the wire;
`.alias(...)` exists in the spec for completeness with Polars conventions
but is not required to author v0 pipelines.

## `bv.lit(value)` — literal-value expression

> **Status:** Optional — most SDK code paths auto-wrap scalars via the
> internal `_wrap()` helper (e.g., `bv.col("amount") > 100` works without an
> explicit `bv.lit(100)`). `bv.lit(...)` is exposed for clarity in
> auto-generated / programmatic pipelines.

```python
import beava as bv

bv.lit(100).to_expr_string()       # "100"
bv.lit(3.14).to_expr_string()      # "3.14"
bv.lit(True).to_expr_string()      # "true"
bv.lit(None).to_expr_string()      # "null"
bv.lit("amazon").to_expr_string()  # "'amazon'"
```

Supported value types: `int`, `float`, `bool`, `str`, `None`. Other types
raise `TypeError` at decoration time. Strings are quoted with single quotes
and backslash-escape `\\` and `'` per the grammar.

You almost never need `bv.lit(...)` in user code — the operator overloading
auto-wraps Python scalars on either side of binary ops. Use it when you
need an explicit literal at a position the SDK cannot infer (e.g., as a
positional argument to a future call expression).

Per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md), `bv.lit` is the **canonical public surface** for literal construction across all three SDKs (Python `bv.lit(value)`, TypeScript `bv.lit(value)`, Go `beava.Lit(value)`). The implicit operator-overloading coercion path keeps working in Python; `bv.lit` is exposed for explicit cases (constant columns via `events.with_columns(source=bv.lit("web"))`, type-coercion patterns, and cross-language parity with TS/Go SDKs that lack Python's flexible operator overloading).

## Compilation: every node knows how to emit

Each `_ExprAST` subclass implements `to_expr_string()`. The SDK calls this
at register time when serialising:

- `EventDerivation.filter(expr)` → `{"op": "filter", "expr": expr.to_expr_string()}`
- `EventDerivation.with_columns(name=expr, ...)` → `{"op": "with_columns",
  "exprs": {name: expr.to_expr_string(), ...}}`
- `bv.<agg>(field, where=expr, ...)` (e.g. `bv.count(where=bv.col("status") == "ok")`)
  → `{"op": "<agg>", "params": {"where": expr.to_expr_string(), ...}}`

The expression string is the **canonical contract** between SDK and server.
SDK porters MUST produce the same string for semantically equivalent
expressions; round-trip tests on every fixture in
[`examples/wire/`](../../examples/wire/) verify this.

## Validation at register-time

When the server parses the register payload (per
[wire-spec OP_REGISTER](../wire-spec.md#op_register-0x0001)) it runs Phase 4
expression validation on every `expr` string:

- **Parse error** (malformed grammar, unbalanced parens) →
  `RegistrationError(code="invalid_expression")`.
- **Field reference unknown** (e.g., `bv.col("typo_amount")` referencing a
  field not in the upstream schema) → `RegistrationError(code="unknown_field_reference")`.
- **Type mismatch** (e.g., arithmetic on a `bool` field, boolean op on a
  numeric field) → `RegistrationError(code="schema_mismatch")`.
- **Cast target invalid** (e.g., `cast(x, "complex64")`) →
  `RegistrationError(code="invalid_cast_target")`.

The server emits all errors in a fail-soft batch — a single register call
returns the full list of validation failures, not just the first.

## Cross-references

- [Pipeline DSL Overview](overview.md) — how expressions fit in the larger
  pipeline-authoring story.
- [Pipeline DSL Compilation Rules](compilation-rules.md) — per-method H3
  worked examples; expressions are referenced from `.filter`, `.with_columns`,
  `.cast`, and aggregation `where=` kwargs.
- [Wire spec](../wire-spec.md) — canonical JSON contract.
- [Error codes](../error-codes.md) — `invalid_expression`,
  `unknown_field_reference`, `schema_mismatch`, `invalid_cast_target`.
