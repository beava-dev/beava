# RFC-001: Expression DSL Extension

| Status | Author | Date | Target | Supersedes |
|---|---|---|---|---|
| Draft | KD | 2026-05-16 | v0.1 | Issue #56 (narrowed) |

---

## 1. Summary

This RFC lets users define reusable expressions with plain Python. A new `@bv.expr` decorator accepts a subset of plain-Python
and translates it into the existing expression DSL. The RFC proposes a catalogue of builtin functions that should be supported 
including five categories (math, string, time, hashing, conditional / null) and implements at least one builtin per category.
Existing AST stays the same, everything reuses existing extension points.

## 2. Motivation

Today's DSL only supports basic comparison and arithmetic operators, and three builtins. Users need more builtin functions for real use cases, 
and we need to make it easy to support more builtins in the future.

## 3. Goals

- Ship one function per category of builtins in 5.1 below.
- Make future builtins a bounded mechanical change: one Rust-side `BuiltinFn` row (name + arity + eval + `infer` fn pointer) + one Python sugar method. 
- Make no change to wire format and AST shape.
- Support method chaining and dot-access on events.
- Support plain-Python `if/elif`/ternary/`and`/`or`/`not` inside expression defs.
- Add `CONTRIBUTING-OPS.md` walking one full op contribution.

## 4. Non-goals

- Full `@bv.expr` per #56 (type-annotation schema checks, loops, recursion, subprocess fallback).
- Runtime-mutable sets — different problem class.
- New `Expr` / `Literal` variants or grammar growth. `is_in` uses the existing variadic call form.
- List builtins (`split`, `at`), `[i]` indexing, `Value::Map`, nested types. Deferred together — they couple several type-system decisions.
- Python-side register-time type checking.

---

## 5. Detailed design

### 5.1 Builtin catalogue

Full proposed catalogue (rest become good-first-issues citing `CONTRIBUTING-OPS.md`):

| Category | Functions |
|---|---|
| Math | `abs`, `sign`, `log`, `log1p`, `log10`, `exp`, `sqrt`, `pow`, `floor`, `ceil`, `round`, `mod`, `clip` |
| String | `lower`, `upper`, `length`, `contains`, `starts_with`, `ends_with`, `substr`, `replace`, `concat` |
| Conditional / null | `if_else`, `coalesce`, `fill_null`, `is_in` |
| Predicates | `any_of`, `all_of` |
| Time | `hour_of_day`, `day_of_week`, `month`, `day_of_month` |
| Hashing | `hash`, `hash_mod` |

**v0 ships these 11**:

- Math: `log1p`, `clip`
- String: `lower`, `contains`, `starts_with`, `ends_with`, `replace`
- Conditional / null: `if_else`, `is_in`
- Time: `hour_of_day`
- Hashing: `hash_mod`

**Predicates defer entirely** — `&` / `|` cover v0; flat variadics earn keep at 3+ args.

**Null rules** (each builtin pins one in its doc):
- **strict-propagating** — math, time
- **null-eating** — `fill_null`, `coalesce`
- **null-aware predicate** — `contains`, `is_in`
- **short-circuit** — `if_else`

Non-bool inputs follow §D-04: return `Null`, never panic.

#### Generalized per-builtin typechecking

Typechecking for builtins are currently hardcoded. This RFC generalizes typechecking by having functions with the same type signatures share the same typechecking function.
Adds a field to the existing BuiltinFn struct to hold a function pointer for typechecking.

```rust
pub struct BuiltinFn {
    pub name:  &'static str,
    pub arity: Arity,
    pub eval:  fn(&[Value]) -> Value,
    pub infer: fn(arg_types: &[InferredType], args: &[Expr])
                 -> Result<InferredType, InferError>,
}
```

`args: &[Expr]` exists for `cast` only (used by the current typechecking implementation). Every other builtin ignores it.

**Shared helpers** in `builtins/_inference.rs` (each representing a type signature, example: all builtins of type `num -> float` uses `binary_numeric_to_f64`):

| Helper | Used by |
|---|---|
| `unary_str_to_str` | `lower`, `upper` |
| `unary_str_to_i64` | `length` |
| `unary_numeric_same` | `abs`, `sign`, `floor`, `ceil`, `round` |
| `unary_numeric_to_f64` | `log`, `log1p`, `log10`, `exp`, `sqrt` |
| `binary_numeric_to_f64` | `pow`, `mod` |
| `string_search_to_bool` | `contains`, `starts_with`, `ends_with` |
| `polymorphic_var0_unify` | `coalesce`, `fill_null` |
| `any_to_bool` | `isnull` |

**Inline primitives** for new signatures with no existing helper, provide primitives for building typechecking function: 
`require_arg_types`, `require_arg_class`, `unify_var0_strict`, `unify_var0_with_class`, `read_literal_type_name`.

```rust
fn day_diff_infer(arg_types: &[InferredType], _: &[Expr])
    -> Result<InferredType, InferError>
{
    require_arg_types(arg_types, &[FieldType::Datetime, FieldType::Datetime])?;
    Ok(InferredType::Known(FieldType::I64))
}
```

**Unification** (`if_else`, `coalesce`, `fill_null`) is **strict equality** — no numeric promotion. `if_else(c, 1, 2.0)` register-fails with `TypeMismatch`; wrap in `cast(...)`. `NullLiteral` is the permitted hole; all-null → `FieldType::Str`. Relaxing later is additive.

**Coverage is type-system-enforced**: no default on the `infer` field → missing impl = compile error. The catch-all match arm (which also misclassifies `quadkey`) is deleted.

### 5.2 BUILTINS organization

Each category of builtins get its own file, `expr_builtins.rs` performs the lookup.

### 5.3 The `@bv.expr` decorator

Narrow Python→`_Expr` translator. Rewrites five constructs; rejects everything else.

#### Decoration semantics

- **Rewrite at decoration time.** Reads source via `inspect.getsource`, parses with `ast.parse`, applies the five rewrites, validates (structured `RegistrationError` at the user's `def` site), `compile()`s, rebinds the name. One-time cost; errors land at the def, not deep in registration.
- **Returns a callable, not a static `_Expr`.** Each invocation runs the rewritten body and returns a fresh `_Expr`. Same `@bv.expr` reusable with different args at different call sites (`email_token(e.email)` vs `email_token(e.alt_email)`). No tree cache.
- **Args: `_Expr` pass-through; literals coerced lazily.** Non-`_Expr` args (`int`/`float`/`str`/`bool`/`None`) wrapped in `_Literal` before the body runs. Anything else (lists, dataclasses) reaches the body unwrapped and hits normal `_Expr` operator errors.
- **Type annotations are decorative** — docs and IDE hints only, not enforced.
- **Nested `@bv.expr`** is plain function composition; inner returns an `_Expr`, outer uses it like any subexpr. Wire tree identical to hand-written nesting.

#### Accepted rewrites

**1. `if`/`elif`/`else` chains.** Each branch body = zero or more local assigns then `return <expr>` or fall-through. Lowers to nested `bv.if_else(...)`.

```python
@bv.expr
def dwell_bucket(dwell_ms):
    if   dwell_ms < 1000:  return 0
    elif dwell_ms < 5000:  return 1
    elif dwell_ms < 30000: return 2
    else:                  return 3
```

**2. Ternary `a if cond else b`** → `_Call("if_else", (cond, a, b))`. Bare ternary outside `@bv.expr` still raises via `__bool__` (§5.6). Asymmetry is intentional: loud error or correct code, never silent miscompile.

**3. Local assignments** — top level OR inside branches. **Inline-substituted** into the lowered tree (no `Expr::Let`, no eval-time env). Translator keeps a **stack of binding dicts** (one per scope: push on branch entry, pop+merge on exit); each `ast.Name` resolves to the innermost subtree at rewrite time.

```python
@bv.expr
def risk(c):
    country_risk = bv.if_else(bv.is_in(c.country, "NG", "RO"), 5, 0)
    amount_risk  = bv.log1p(c.amount) * 2
    return country_risk + amount_risk
```

**Per-branch convergence rules (loose carry-over):**

| Pattern | Lowering |
|---|---|
| Name assigned in *every* branch | `y = bv.if_else(c₁, t₁, bv.if_else(c₂, t₂, …))` — merged subtree replaces post-if refs |
| Name assigned in *some* branches **AND** bound outer | Unmodified branches carry the outer binding into the merge |
| Name assigned in *some* branches, **no outer** (non-converging) | Reject: `expr_branch_local_binding` |
| Branch ends in `return` | Terminal; post-if continuation wraps into the not-taken arm |

A missing `else:` (AST `orelse=[]`) is treated as an empty branch holding the outer bindings — `if c: y = a` with outer `y = 0` merges to `y = bv.if_else(c, a, 0)`; same shape without outer `y` is non-converging.

```python
# all branches assign → simple merge
if c: y = a
else: y = b
return y + 1                # → y = bv.if_else(c, a, b); return y + 1

# outer y + asymmetric branch → loose carry-over
y = 0
if c: y = a
return y + 1                # → y = bv.if_else(c, a, 0); return y + 1

# early return → post-if wraps into not-taken arm
if c: return early_value
y = x * 2
return y + 1                # → bv.if_else(c, early_value, (x * 2) + 1)

# non-converging — rejected
if c: y = a
return y + 1                # ← y unbound on else path
```

**Sequential reassignment** (`y = a; y = y + 1`) mutates the active dict in place; later RHS sees the prior binding. Works at top level and inside branches.

**Augmented assigns** (`+=`, `-=`, `*=`, `/=`, `%=`) desugar to `x = x <op> rhs` in a pre-rewrite pass, then flow through normal assign analysis.

**Rejected assign forms:**
- Tuple/list unpacking (`a, b = …`) → `expr_bad_assign_target`
- Attribute/subscript targets (`c.x = …`, `c[0] = …`) → `expr_bad_assign_target` ("row mutation not allowed")
- Walrus (`:=`) → `expr_unsupported_python_op`
- Reference before assignment → `expr_unknown_name`
- Non-converging branch assigns → `expr_branch_local_binding`. Error names the assigning branches, the post-if read site, and the missing branch / "no outer binding" hint. Fix: add `else: y = …`, or bind `y` before the if.

**Trade-off vs `Expr::Let`**: inline substitution duplicates the bound subtree per reference. A merged `y = bv.if_else(...)` referenced 8 times after the join produces 8 copies on the wire. Bounded at registration; eval cost dwarfed by deserialize / per-event allocation. `Expr::Let` is §11 future work, triggered by >100KB register payloads from duplicated subexprs.

**4. `is None` / `is not None`** — four accepted shapes, identified positionally:

| Source | Rewritten to |
|---|---|
| `x is None`, `None is x` | `_Call("isnull", (x,))` |
| `x is not None`, `None is not x` | `_UnaryOp("not", _Call("isnull", (x,)))` |

Reason: `is` is object-identity; on `_Expr` instances `email is None` is unconditionally `False` — silent miscompile if unrewritten. All other `is`/`is not` shapes (`x is y`, `x is True`, …) rejected at decoration time.

**5. `and` / `or` / `not`** → `&` / `|` / `~` at the AST-rewrite layer (before Python executes the body, so `__bool__` never fires). Full pipeline for `a and b`:

```
Python source:       a and b
Rewritten AST:       a & b                          (BoolOp → BitAnd)
Operator overload:   _Expr.__and__(a, b)
SDK node:            _BinOp("and", a, b)
Wire string:         (a and b)                       (keyword form)
Rust AST:            BinOp { op: "and", … }
```

`&` is purely a Python-side bridge — never visible on the wire. Same for `|` → `or`, `~x` → `(not x)`. Outside `@bv.expr`, bare `and`/`or`/`not` still raises via `__bool__`.

#### Rejected at decoration time

`for`, `while`, nested defs/classes/lambdas, `try`/`except`/`with`/`raise`, unpacking, attribute/subscript assign targets, walrus, `import` inside body, `is`/`is not` on non-`None`, and **direct self-recursion** (any rewritten `Call` whose `func` is an `ast.Name` matching the def name → `expr_recursive_call`, pointing at the offending line).

Why a static check for direct recursion? `if`/`elif` lowers eagerly at tree-build time (short-circuit is a *Rust-eval* optimization, §5.3), so a recursive body otherwise builds both arms forever and surfaces as `RecursionError` deep in decorator internals.

#### Indirect / mutual recursion

`f → g → f` and longer cycles are caught during **tree building** via a thread-local stack — standard DFS cycle detection.

The stack tracks **which `@bv.expr` are currently mid-tree-building** (not the interpreter call stack). 

Trace of `a → b → c → a`:

| step | enter | stack before | in? | action |
|---|---|---|---|---|
| 1 | `a` | `[]` | no | push → `[a]` |
| 2 | `b` | `[a]` | no | push → `[a, b]` |
| 3 | `c` | `[a, b]` | no | push → `[a, b, c]` |
| 4 | `a` | `[a, b, c]` | **yes** | raise `expr_recursive_call: a → b → c → a` |

Catches every cycle that re-enters any `@bv.expr` regardless of what's between (plain helpers, attribute access, etc). Thread-local so concurrent registration paths don't race. Cost: one push/pop/membership check per tree-build call; zero impact on wire/server.

#### Error format (both checks)
Error messages will include offending line number, source file name and the sequence of calls that produced the error.

Sources: file name via `inspect.getsourcefile(fn)`; line number via `ast.increment_lineno` (static) and `frame.f_lineno` (runtime); source text via `linecache.getline`; per-hop call site via `sys._getframe(1)` captured on each `_Frame`.

**Direct (decoration-time):**

```
RegistrationError [expr_recursive_call]

  @bv.expr 'f' calls itself directly.

    File "helpers.py", line 42, in 'f':
        return f(x - 1) + 1
               ^

  '@bv.expr' does not support recursive calls. See RFC §5.7.
```

(Caret = `node.col_offset` of the offending `ast.Call`.)

**Mutual (runtime, `a → b → c → a`):**

```
RegistrationError [expr_recursive_call]

  @bv.expr call cycle: a → b → c → a

    File "helpers.py", line 12, in 'a':
        return b(x) + 1
               ^                  →  calls 'b'

    File "helpers.py", line 18, in 'b':
        return c(x) + 1
               ^                  →  calls 'c'

    File "helpers.py", line 25, in 'c':
        return a(x) + 1
               ^                  →  calls 'a'   (cycle closes)

  '@bv.expr' does not support recursive or mutually-recursive
  composition. See RFC §5.7.
```

Each "in `<X>`" block prints the line where `<X>` calls the next function (from the *next* `_Frame`'s `called_from_{file,line}`). Cross-file cycles render each block with the participating function's file path. Cycles >5 hops elide the middle as `... (k more) ...`; head and tail always render so the closing edge is visible.

---

## 6. Testing plan

Gated by `check.sh` at every phase boundary.

### 6.1 Per-builtin

Four tests per builtin in per-category test module:

- **Arity** — wrong arg count → documented structured error (`aggregation_invalid_arity` or `TypeMismatch` for zero-arg under `AtLeast(n)`).
- **Eval truth-table** — 3–5 inputs covering normal, boundary (e.g. `clip` at exact bounds), and edge (`I64::MAX`, `F64::NAN`, empty string).
- **Null-rule conformance** — verifies documented behavior per §5.1.
- **SDK round-trip** — Python `_Expr` → wire → Rust `parse` → identical AST. Pins the wire form documented in the sugar's docstring.

### 6.2 `@bv.expr` 

Two modules under `python/tests/v0/test_expr_translator/`:

- **Accepted**: one test per rewrite; plus integration cases combining rules (§8 reused as fixture). Per-branch assign fixtures covering the convergence table: (a) all-branches-assign simple merge; (b) outer-binding + asymmetric via loose carry; (c) early-return wrapping post-if into not-taken arm; (d) sequential reassignment in a branch; (e) `+=`/etc. desugar producing same tree as explicit form.
- **Rejected**: one test per error code (`expr_unsupported_python_op`, `expr_bad_assign_target`, `expr_branch_local_binding`, `expr_recursive_call`, …). Each asserts code + source line. `expr_branch_local_binding` covers the non-converging case; assert the message names the assigning branches, post-if read site, and missing branch / "no outer binding". `expr_recursive_call` covers (a) **direct** — unconditional self-call AND self-call inside `if`/`elif`/ternary (caught at decoration after `if_else` lowering); (b) **mutual / indirect** — `a → b → a` and longer chains incl. via helpers, aliases, attribute access, dispatch tables (caught at call time via thread-local stack). Each test asserts the rendered message contains: file, line, source text, and (for cycles) full call order + per-hop block naming the call line in each function. Cross-file cycles have a dedicated test asserting per-hop file paths differ. >5-hop cycles assert `... (k more) ...` elision keeps head + tail.

Plus: decoration-time validation runs once per `@bv.expr`; call-time literal coercion (`int`/`str`/`bool`/`None` → `_Literal`); sequential reassignment threads the dict; nested `@bv.expr` composition.

### 6.3 Type system

- **Helper tests** in `builtins/_inference.rs`: each shared helper gets ≥1 positive (well-typed → expected `InferredType`) and ≥1 negative (mistyped → `TypeMismatch`).
- **Unification corner cases**: `polymorphic_var0_unify` rejects `I64`/`F64` mixing, accepts `NullLiteral`, falls back to `FieldType::Str` when all bindings null.
- **Name uniqueness across category tables** — compile-time test.
- `infer` field coverage is type-system-enforced (no default; missing = compile error) → no runtime test needed.

### 6.4 Backwards compatibility

- Existing builtins (`cast`, `isnull`, `quadkey`) — typechecks correctly under fn-pointer dispatch where they previously fell through the deleted catch-all.
- Wire round-trip stability: every expression in current `python/tests/v0` corpus continues to parse and eval identically.

### 6.5 End-to-end

Canonical example below registers against a v0 server, exercises every accepted `@bv.expr` rewrite + every v0 builtin, produces documented aggregations against a synthetic Click stream.

---

## 7. End-to-end use case

```python
import beava as bv

@bv.event
class Click:
    user_id: str
    email: str | None
    country: str
    referrer: str
    amount_usd: float
    dwell_ms: int
    ts: int

# (is None) + hash_mod + string.lower
@bv.expr
def email_bucket(email: str | None):
    if email is None:
        return 0
    return bv.hash_mod(email.lower(), 1024)

# (ternary) + (and/not) + string predicates
@bv.expr
def is_external_secure(url: str):
    return 1 if url.starts_with("https://") and not url.contains("internal.") else 0

# (if/elif/else) + (ternary) + (local assigns)
# + (and/or) + log1p + clip + is_in
@bv.expr
def risk_score(amount_usd: float, dwell_ms: int, country: str):
    log_amount  = bv.log1p(amount_usd)
    short_dwell = bv.clip(dwell_ms, 0, 1_000)
    geo_bonus   = 3.0 if bv.is_in(country.lower(), "ru", "kp", "ir") else 0.0
    if   log_amount > 6.0 and short_dwell < 200: geo_bonus = geo_bonus + 5.0
    elif log_amount > 4.0 or  short_dwell < 500: geo_bonus += 2.0

    return geo_bonus

# (local assign) + (ternary) + (or)
# + method chains + ends_with + contains
@bv.expr
def is_suspicious_referrer(referrer: str):
    clean = referrer.lower().replace("https://", "").replace("http://", "")
    return 1 if clean.ends_with(".xyz") or clean.ends_with(".tk") or clean.contains("bit.ly") else 0

# event dot-access (e.email rather than bv.col("email"))
def ClickFeatures(e: Click):
    e = e.with_columns(
        email_bkt      = email_bucket(e.email),
        secure_ext     = is_external_secure(e.referrer),
        risk           = risk_score(e.amount_usd, e.dwell_ms, e.country),
        suspicious_ref = is_suspicious_referrer(e.referrer),
    )
    return e.group_by("user_id").agg(
        clicks_24h            = bv.count(window="24h"),
        distinct_emails_24h   = bv.n_unique("email_bkt", window="24h"),
        risky_clicks_1h       = bv.count(where=bv.col("risk") >= 5.0, window="1h"),
        avg_amount_24h        = bv.mean("amount_usd", window="24h"),
        suspicious_refs_24h   = bv.sum("suspicious_ref", window="24h"),
        secure_clicks_24h     = bv.sum("secure_ext", window="24h"),
    )
```

---

# 8. RFC-001 — PR breakdown

Split the implementation into 6 PRs.


| PR | What it does | LOC Estimate |
|---|---|---|
| 1 | Rust: add typechecking rules to each builtin row | ~600 |
| 2 | Python: small SDK pieces every later PR needs | ~150 |
| 3 | Add 10 of the 11 v0 builtins | ~900 |
| 4 | Add `if_else` with its short-circuit rule | ~350 |
| 5 | Build the `@bv.expr` decorator | ~1500 |
| 6 | Write contributor docs, update the website | ~600 |
| **Total** | | **~4100** |