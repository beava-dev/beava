//! Expression evaluator: `eval(expr: &Expr, row: &Row) -> Value`.
//!
//! # Semantics (CONTEXT.md §D-04, §D-05)
//!
//! - **Three-valued null propagation**: arithmetic, comparison, and boolean ops
//!   on `Value::Null` return `Value::Null`, except SQL short-circuit cases
//!   (`false AND null = false`, `true OR null = true`). Boolean ops delegate to
//!   `Value::and_three_valued` / `or_three_valued` / `not_three_valued` from
//!   Plan 04-01 — no truth-table logic is duplicated here.
//!
//! - **i64 arithmetic**: saturating add/sub/mul (`i64::saturating_*`); integer
//!   division truncates toward zero; division by zero returns `Null`.
//!
//! - **f64 arithmetic**: IEEE-754 — NaN/Inf propagate naturally; `f64 / 0.0`
//!   yields `Inf` (not `Null`). Comparisons involving NaN return `Bool(false)`.
//!
//! - **Type promotion**: `I64 op F64` (or reverse) promotes the `I64` to `F64`
//!   before the operation. Two `I64`s stay `I64` (except division, see below).
//!
//! - **Integer division (v1 decision)**: `I64 / I64` → `I64` (truncating). If
//!   either operand is `F64`, the result is `F64`. See test 4 and doc comment.
//!
//! - **`(x == null)` rewrite**: Plan 04-02's parser post-pass rewrites
//!   `BinOp("==", _, Literal::Null)` and `BinOp("==", Literal::Null, _)` to
//!   `Call("isnull", [_])` before the AST reaches this evaluator. Therefore:
//!   - The `BinOp("==")` branch here is strict-null: if either operand evaluates
//!     to `Value::Null`, it returns `Value::Null` (no special-casing).
//!   - Users who write `(x == null)` in source get `isnull(x)` at eval time,
//!     which correctly returns `Bool(true/false)`.
//!
//! - **Builtins**: `Call` nodes dispatch through the `BUILTINS` table in
//!   `builtins/mod.rs`. Unknown function names return `Null` (register-time
//!   rejects these; runtime is defensive).
//!
//! - **`Literal::BareIdent`**: converted to `Value::Str` so that `cast`'s
//!   second argument (`cast(x, float)`) arrives at `cast_eval` as
//!   `Value::Str("float")` — matching the builtin contract.

use crate::expr::{Expr, Literal};
use crate::builtins::lookup_builtin;
use crate::row::{Row, Value};

/// Maximum recursion depth for expression evaluation.
/// Expressions deeper than this return `Value::Null` rather than overflowing
/// the stack. 512 levels is far beyond any legitimate SDK-generated expression;
/// a depth-1000+ expression indicates a bug or a crafted DoS input.
const MAX_EVAL_DEPTH: usize = 512;

/// Evaluate `expr` against `row`, returning the resulting `Value`.
///
/// This function is pure and deterministic: the same `(expr, row)` always
/// produces the same `Value`.
///
/// Depth is bounded to `MAX_EVAL_DEPTH`; expressions exceeding that limit
/// return `Value::Null` rather than overflowing the call stack.
pub fn eval(expr: &Expr, row: &Row) -> Value {
    eval_depth(expr, row, 0)
}

fn eval_depth(expr: &Expr, row: &Row, depth: usize) -> Value {
    if depth > MAX_EVAL_DEPTH {
        return Value::Null;
    }
    match expr {
        // ── Field reference ───────────────────────────────────────────────────
        Expr::Field { name, .. } => row.get(name).cloned().unwrap_or(Value::Null),

        // ── Scalar literals ───────────────────────────────────────────────────
        Expr::Literal(lit, _) => match lit {
            Literal::Null => Value::Null,
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Int(n) => Value::I64(*n),
            Literal::Float(f) => Value::F64(*f),
            Literal::Str(s) => Value::Str(s.into()),
            // BareIdent is the type-arg to cast(x, float): evaluator converts to
            // Str so cast_eval receives Value::Str("float") matching its contract.
            Literal::BareIdent(s) => Value::Str(s.into()),
        },

        // ── Unary NOT ─────────────────────────────────────────────────────────
        Expr::UnaryOp { operand, .. } => {
            // Only "not" exists in Phase 4; future ops would branch on `op`.
            let v = eval_depth(operand, row, depth + 1);
            v.not_three_valued()
        }

        // ── Binary ops ────────────────────────────────────────────────────────
        Expr::BinOp {
            op, left, right, ..
        } => eval_binop(op, left, right, row, depth),

        // ── Call (builtins) ───────────────────────────────────────────────────
        Expr::Call { fn_name, args, .. } => {
            // Evaluate all args to Values first.
            let arg_vals: Vec<Value> = args.iter().map(|a| eval_depth(a, row, depth + 1)).collect();
            match lookup_builtin(fn_name) {
                Some(builtin) => (builtin.eval)(&arg_vals),
                // Unknown function → Null (register-time catches; runtime defensive).
                None => Value::Null,
            }
        }
    }
}

// ─── Binary operator dispatch ─────────────────────────────────────────────────

fn eval_binop(op: &str, left: &Expr, right: &Expr, row: &Row, depth: usize) -> Value {
    match op {
        // Boolean operators: short-circuit evaluation delegated to three-valued helpers.
        "and" => {
            let lv = eval_depth(left, row, depth + 1);
            // Short-circuit: false AND _ = false (skip right).
            if lv == Value::Bool(false) {
                return Value::Bool(false);
            }
            let rv = eval_depth(right, row, depth + 1);
            lv.and_three_valued(&rv)
        }
        "or" => {
            let lv = eval_depth(left, row, depth + 1);
            // Short-circuit: true OR _ = true (skip right).
            if lv == Value::Bool(true) {
                return Value::Bool(true);
            }
            let rv = eval_depth(right, row, depth + 1);
            lv.or_three_valued(&rv)
        }

        // Arithmetic and comparison: evaluate both operands, then dispatch.
        _ => {
            let lv = eval_depth(left, row, depth + 1);
            let rv = eval_depth(right, row, depth + 1);
            // Null propagates for arithmetic and comparison (D-04).
            if matches!(lv, Value::Null) || matches!(rv, Value::Null) {
                return Value::Null;
            }
            match op {
                "+" => arith_add(lv, rv),
                "-" => arith_sub(lv, rv),
                "*" => arith_mul(lv, rv),
                "/" => arith_div(lv, rv),
                ">" => cmp_op(lv, rv, |o| matches!(o, std::cmp::Ordering::Greater)),
                ">=" => cmp_op(lv, rv, |o| {
                    matches!(o, std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                }),
                "<" => cmp_op(lv, rv, |o| matches!(o, std::cmp::Ordering::Less)),
                "<=" => cmp_op(lv, rv, |o| {
                    matches!(o, std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                }),
                "==" => cmp_eq(lv, rv),
                "!=" => cmp_ne(lv, rv),
                // Unknown operator → Null (defensive).
                _ => Value::Null,
            }
        }
    }
}

// ─── Arithmetic helpers ───────────────────────────────────────────────────────
// Each arithmetic function handles I64+I64, F64+F64, and mixed I64/F64 cases
// inline via a two-level match. This avoids a shared helper with a confusing
// return type and keeps each function's logic self-contained.

fn arith_add(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::I64(x), Value::I64(y)) => Value::I64(x.saturating_add(y)),
        (Value::F64(x), Value::F64(y)) => Value::F64(x + y),
        (Value::I64(x), Value::F64(y)) => Value::F64(x as f64 + y),
        (Value::F64(x), Value::I64(y)) => Value::F64(x + y as f64),
        _ => Value::Null, // non-numeric types
    }
}

fn arith_sub(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::I64(x), Value::I64(y)) => Value::I64(x.saturating_sub(y)),
        (Value::F64(x), Value::F64(y)) => Value::F64(x - y),
        (Value::I64(x), Value::F64(y)) => Value::F64(x as f64 - y),
        (Value::F64(x), Value::I64(y)) => Value::F64(x - y as f64),
        _ => Value::Null,
    }
}

fn arith_mul(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::I64(x), Value::I64(y)) => Value::I64(x.saturating_mul(y)),
        (Value::F64(x), Value::F64(y)) => Value::F64(x * y),
        (Value::I64(x), Value::F64(y)) => Value::F64(x as f64 * y),
        (Value::F64(x), Value::I64(y)) => Value::F64(x * y as f64),
        _ => Value::Null,
    }
}

fn arith_div(a: Value, b: Value) -> Value {
    match (a, b) {
        // Integer division: divide-by-zero → Null; otherwise truncating.
        (Value::I64(x), Value::I64(y)) => {
            if y == 0 {
                Value::Null
            } else {
                Value::I64(x / y)
            }
        }
        // Float division: IEEE-754 (div by 0.0 → ±Inf, NaN propagates).
        (Value::F64(x), Value::F64(y)) => Value::F64(x / y),
        (Value::I64(x), Value::F64(y)) => Value::F64(x as f64 / y),
        (Value::F64(x), Value::I64(y)) => Value::F64(x / y as f64),
        _ => Value::Null,
    }
}

// ─── Comparison helpers ───────────────────────────────────────────────────────

/// Try to compare two same-type values. Returns `None` for cross-type or
/// NaN-containing comparisons (which must resolve to Null or false depending on
/// the context — callers handle this).
fn try_compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::I64(x), Value::I64(y)) => x.partial_cmp(y),
        (Value::F64(x), Value::F64(y)) => x.partial_cmp(y), // returns None for NaN
        (Value::I64(x), Value::F64(y)) => (*x as f64).partial_cmp(y),
        (Value::F64(x), Value::I64(y)) => x.partial_cmp(&(*y as f64)),
        (Value::Str(x), Value::Str(y)) => x.partial_cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.partial_cmp(y),
        (Value::Datetime(x), Value::Datetime(y)) => x.partial_cmp(y),
        (Value::Bytes(x), Value::Bytes(y)) => x.partial_cmp(y),
        // Cross-type → None (will become Null)
        _ => None,
    }
}

/// Ordered comparison (`>`, `>=`, `<`, `<=`).
/// Returns `Null` for cross-type or NaN; returns `Bool` otherwise.
fn cmp_op(a: Value, b: Value, pred: impl Fn(std::cmp::Ordering) -> bool) -> Value {
    match try_compare(&a, &b) {
        Some(ord) => Value::Bool(pred(ord)),
        // NaN partial_cmp returns None → Bool(false) per IEEE-754.
        // Cross-type → Null per D-05.
        None => {
            // Distinguish NaN (both are F64) from cross-type.
            match (&a, &b) {
                (Value::F64(_), Value::F64(_))
                | (Value::F64(_), Value::I64(_))
                | (Value::I64(_), Value::F64(_)) => Value::Bool(false), // NaN
                _ => Value::Null, // cross-type
            }
        }
    }
}

/// Equality comparison (`==`). Null-strict: either Null → Null.
/// NaN == NaN → Bool(false) (IEEE-754). Cross-type → Null.
fn cmp_eq(a: Value, b: Value) -> Value {
    // Null already filtered out by caller (null propagation check in eval_binop).
    match try_compare(&a, &b) {
        Some(ord) => Value::Bool(matches!(ord, std::cmp::Ordering::Equal)),
        None => {
            // NaN or cross-type.
            match (&a, &b) {
                (Value::F64(_), Value::F64(_))
                | (Value::F64(_), Value::I64(_))
                | (Value::I64(_), Value::F64(_)) => Value::Bool(false), // NaN
                _ => Value::Null,
            }
        }
    }
}

/// Inequality comparison (`!=`). Null-strict: either Null → Null.
/// NaN != NaN → Bool(false) (IEEE-754 — not-equal for NaN is also false).
/// Cross-type → Null.
fn cmp_ne(a: Value, b: Value) -> Value {
    match try_compare(&a, &b) {
        Some(ord) => Value::Bool(!matches!(ord, std::cmp::Ordering::Equal)),
        None => {
            match (&a, &b) {
                (Value::F64(_), Value::F64(_))
                | (Value::F64(_), Value::I64(_))
                | (Value::I64(_), Value::F64(_)) => Value::Bool(false), // NaN
                _ => Value::Null,
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{Literal, Span};
    use crate::row::{Row, Value};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn span() -> Span {
        Span { start: 0, end: 0 }
    }

    fn field_expr(name: &str) -> Expr {
        Expr::Field {
            name: name.to_string(),
            span: span(),
        }
    }

    fn lit_null() -> Expr {
        Expr::Literal(Literal::Null, span())
    }

    fn lit_bool(b: bool) -> Expr {
        Expr::Literal(Literal::Bool(b), span())
    }

    fn lit_int(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n), span())
    }

    fn lit_float(f: f64) -> Expr {
        Expr::Literal(Literal::Float(f), span())
    }

    fn lit_str(s: &str) -> Expr {
        Expr::Literal(Literal::Str(s.to_string()), span())
    }

    fn lit_bare(s: &str) -> Expr {
        Expr::Literal(Literal::BareIdent(s.to_string()), span())
    }

    fn binop(op: &str, left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: op.to_string(),
            left: Box::new(left),
            right: Box::new(right),
            span: span(),
        }
    }

    fn unaryop(op: &str, operand: Expr) -> Expr {
        Expr::UnaryOp {
            op: op.to_string(),
            operand: Box::new(operand),
            span: span(),
        }
    }

    fn call(fn_name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            fn_name: fn_name.to_string(),
            args,
            span: span(),
        }
    }

    fn row_with(pairs: &[(&str, Value)]) -> Row {
        let mut r = Row::new();
        for (k, v) in pairs {
            r = r.with_field(k, v.clone());
        }
        r
    }

    // ── Test 1: Field hit ─────────────────────────────────────────────────────

    #[test]
    fn eval_field_hit() {
        let row = row_with(&[("amount", Value::I64(100))]);
        assert_eq!(eval(&field_expr("amount"), &row), Value::I64(100));
    }

    // ── Test 2: Field miss → Null ─────────────────────────────────────────────

    #[test]
    fn eval_field_miss_is_null() {
        let row = Row::new();
        assert_eq!(eval(&field_expr("missing"), &row), Value::Null);
    }

    // ── Test 3: Literals ──────────────────────────────────────────────────────

    #[test]
    fn eval_literal_null() {
        assert_eq!(eval(&lit_null(), &Row::new()), Value::Null);
    }

    #[test]
    fn eval_literal_bool() {
        assert_eq!(eval(&lit_bool(true), &Row::new()), Value::Bool(true));
        assert_eq!(eval(&lit_bool(false), &Row::new()), Value::Bool(false));
    }

    #[test]
    fn eval_literal_int() {
        assert_eq!(eval(&lit_int(42), &Row::new()), Value::I64(42));
        assert_eq!(eval(&lit_int(-7), &Row::new()), Value::I64(-7));
    }

    #[test]
    fn eval_literal_float() {
        assert_eq!(eval(&lit_float(2.5), &Row::new()), Value::F64(2.5));
    }

    #[test]
    fn eval_literal_str() {
        assert_eq!(eval(&lit_str("hi"), &Row::new()), Value::Str("hi".into()));
    }

    /// `BareIdent` is the type argument to `cast(x, float)`. The evaluator
    /// converts it to `Value::Str` so the builtin receives `Value::Str("float")`.
    #[test]
    fn eval_literal_bare_ident_becomes_str() {
        assert_eq!(
            eval(&lit_bare("float"), &Row::new()),
            Value::Str("float".into())
        );
    }

    // ── Test 4: i64 arithmetic ────────────────────────────────────────────────
    //
    // v1 decision: I64 / I64 → I64 (truncating integer division). Type promotes
    // to F64 only when at least one operand is F64. This matches Python's `//`
    // semantics for the integer path and avoids silent widening.

    #[test]
    fn eval_arith_i64_add_sub_mul_div() {
        let empty = Row::new();
        // add
        assert_eq!(
            eval(&binop("+", lit_int(1), lit_int(2)), &empty),
            Value::I64(3)
        );
        // sub
        assert_eq!(
            eval(&binop("-", lit_int(5), lit_int(2)), &empty),
            Value::I64(3)
        );
        // mul
        assert_eq!(
            eval(&binop("*", lit_int(3), lit_int(4)), &empty),
            Value::I64(12)
        );
        // div — truncating
        assert_eq!(
            eval(&binop("/", lit_int(10), lit_int(3)), &empty),
            Value::I64(3)
        );
    }

    // ── Test 5: f64 promotion ─────────────────────────────────────────────────

    #[test]
    fn eval_arith_f64_promotion() {
        let empty = Row::new();
        // I64(1) + F64(2.5) → F64(3.5)
        assert_eq!(
            eval(&binop("+", lit_int(1), lit_float(2.5)), &empty),
            Value::F64(3.5)
        );
        // F64(4.0) - I64(1) → F64(3.0)
        assert_eq!(
            eval(&binop("-", lit_float(4.0), lit_int(1)), &empty),
            Value::F64(3.0)
        );
    }

    // ── Test 6: division by zero ──────────────────────────────────────────────
    //
    // I64 / I64(0) → Null (integer division by zero is undefined).
    // F64 / F64(0.0) → F64(Inf) per IEEE-754 (positive infinity).

    #[test]
    fn eval_arith_div_by_zero_is_null() {
        let empty = Row::new();
        assert_eq!(
            eval(&binop("/", lit_int(1), lit_int(0)), &empty),
            Value::Null
        );
        // f64 / 0.0 → Inf (IEEE-754; not Null)
        let result = eval(&binop("/", lit_float(1.0), lit_float(0.0)), &empty);
        assert!(
            matches!(result, Value::F64(f) if f.is_infinite() && f > 0.0),
            "expected F64(+Inf), got {result:?}"
        );
    }

    // ── Test 7: i64 overflow saturates ───────────────────────────────────────

    #[test]
    fn eval_arith_i64_overflow_saturates() {
        let empty = Row::new();
        // MAX + 1 → MAX (saturates)
        assert_eq!(
            eval(&binop("+", lit_int(i64::MAX), lit_int(1)), &empty),
            Value::I64(i64::MAX)
        );
        // MIN - 1 → MIN (saturates)
        assert_eq!(
            eval(&binop("-", lit_int(i64::MIN), lit_int(1)), &empty),
            Value::I64(i64::MIN)
        );
    }

    // ── Test 8: null propagation in arithmetic ────────────────────────────────

    #[test]
    fn eval_arith_null_propagation() {
        let empty = Row::new();
        // null + I64(1) → Null
        assert_eq!(
            eval(&binop("+", lit_null(), lit_int(1)), &empty),
            Value::Null
        );
        // I64(1) * null → Null
        assert_eq!(
            eval(&binop("*", lit_int(1), lit_null()), &empty),
            Value::Null
        );
        // null / I64(0) → Null (null-first; not div-zero-first)
        assert_eq!(
            eval(&binop("/", lit_null(), lit_int(0)), &empty),
            Value::Null
        );
    }

    // ── Test 9: comparison ops ────────────────────────────────────────────────

    #[test]
    fn eval_comparison_ops() {
        let empty = Row::new();
        assert_eq!(
            eval(&binop(">", lit_int(5), lit_int(3)), &empty),
            Value::Bool(true)
        );
        assert_eq!(
            eval(&binop(">", lit_int(3), lit_int(5)), &empty),
            Value::Bool(false)
        );
        assert_eq!(
            eval(&binop("==", lit_int(2), lit_int(2)), &empty),
            Value::Bool(true)
        );
        assert_eq!(
            eval(&binop("!=", lit_int(2), lit_int(3)), &empty),
            Value::Bool(true)
        );
        assert_eq!(
            eval(&binop("<", lit_str("a"), lit_str("b")), &empty),
            Value::Bool(true)
        );
        assert_eq!(
            eval(&binop(">=", lit_float(3.0), lit_float(3.0)), &empty),
            Value::Bool(true)
        );
        assert_eq!(
            eval(&binop("<=", lit_float(2.0), lit_float(3.0)), &empty),
            Value::Bool(true)
        );
        // Datetime-style: two I64 ms values compared as integers
        assert_eq!(
            eval(&binop("<", lit_int(1_000_000), lit_int(2_000_000)), &empty),
            Value::Bool(true)
        );
    }

    // ── Test 10: null comparison is Null ──────────────────────────────────────
    //
    // CONTEXT.md §D-04: comparisons with null return Null (strict SQL).
    // Note: source strings `(x == null)` are rewritten by Plan 04-02's parser
    // to `Call("isnull", [x])` before reaching this evaluator; only
    // hand-constructed ASTs with BinOp("==", _, Null) reach this path.

    #[test]
    fn eval_comparison_null_is_null() {
        let empty = Row::new();
        // I64(1) > Null → Null
        assert_eq!(
            eval(&binop(">", lit_int(1), lit_null()), &empty),
            Value::Null
        );
        // Null == Null (hand-constructed AST, strict SQL) → Null
        assert_eq!(
            eval(&binop("==", lit_null(), lit_null()), &empty),
            Value::Null
        );
        // Null != I64(1) → Null
        assert_eq!(
            eval(&binop("!=", lit_null(), lit_int(1)), &empty),
            Value::Null
        );
    }

    // ── Test 11: NaN comparisons return Bool(false) ───────────────────────────
    //
    // IEEE-754: any comparison involving NaN returns false.

    #[test]
    fn eval_comparison_nan_is_bool_false() {
        let empty = Row::new();
        let nan = lit_float(f64::NAN);
        let one = lit_float(1.0);

        assert_eq!(
            eval(&binop(">", nan.clone(), one.clone()), &empty),
            Value::Bool(false)
        );
        assert_eq!(
            eval(&binop("==", nan.clone(), nan.clone()), &empty),
            Value::Bool(false)
        );
        assert_eq!(
            eval(&binop("<", nan.clone(), one.clone()), &empty),
            Value::Bool(false)
        );
        assert_eq!(
            eval(&binop(">=", nan.clone(), nan.clone()), &empty),
            Value::Bool(false)
        );
    }

    // ── Test 12: cross-type comparison is Null ────────────────────────────────

    #[test]
    fn eval_comparison_cross_type_is_null() {
        let empty = Row::new();
        assert_eq!(
            eval(&binop(">", lit_int(1), lit_str("x")), &empty),
            Value::Null
        );
        assert_eq!(
            eval(&binop("==", lit_bool(true), lit_int(1)), &empty),
            Value::Null
        );
    }

    // ── Test 13: boolean AND ──────────────────────────────────────────────────

    #[test]
    fn eval_bool_and() {
        let empty = Row::new();
        // true AND false → false
        assert_eq!(
            eval(&binop("and", lit_bool(true), lit_bool(false)), &empty),
            Value::Bool(false)
        );
        // true AND Null → Null
        assert_eq!(
            eval(&binop("and", lit_bool(true), lit_null()), &empty),
            Value::Null
        );
        // false AND Null → false (short-circuit)
        assert_eq!(
            eval(&binop("and", lit_bool(false), lit_null()), &empty),
            Value::Bool(false)
        );
        // Null AND false → false (short-circuit)
        assert_eq!(
            eval(&binop("and", lit_null(), lit_bool(false)), &empty),
            Value::Bool(false)
        );
    }

    // ── Test 14: boolean OR ───────────────────────────────────────────────────

    #[test]
    fn eval_bool_or() {
        let empty = Row::new();
        // false OR Null → Null
        assert_eq!(
            eval(&binop("or", lit_bool(false), lit_null()), &empty),
            Value::Null
        );
        // true OR Null → true (short-circuit)
        assert_eq!(
            eval(&binop("or", lit_bool(true), lit_null()), &empty),
            Value::Bool(true)
        );
        // Null OR Null → Null
        assert_eq!(
            eval(&binop("or", lit_null(), lit_null()), &empty),
            Value::Null
        );
    }

    // ── Test 15: unary NOT ────────────────────────────────────────────────────

    #[test]
    fn eval_unary_not() {
        let empty = Row::new();
        assert_eq!(
            eval(&unaryop("not", lit_bool(true)), &empty),
            Value::Bool(false)
        );
        assert_eq!(
            eval(&unaryop("not", lit_bool(false)), &empty),
            Value::Bool(true)
        );
        assert_eq!(eval(&unaryop("not", lit_null()), &empty), Value::Null);
    }

    // ── Test 16: call cast via parse ──────────────────────────────────────────

    #[test]
    fn eval_call_cast_int_to_float() {
        use crate::expr::parse;
        let row = row_with(&[("amount", Value::I64(5))]);
        let expr = parse("cast(amount, float)").expect("should parse cast");
        assert_eq!(eval(&expr, &row), Value::F64(5.0));
    }

    // ── Test 17: isnull for missing field → true ──────────────────────────────

    #[test]
    fn eval_call_isnull_missing_field_is_true() {
        use crate::expr::parse;
        let row = Row::new();
        let expr = parse("isnull(missing)").expect("should parse isnull");
        assert_eq!(eval(&expr, &row), Value::Bool(true));
    }

    // ── Test 18: isnull for present field → false ─────────────────────────────

    #[test]
    fn eval_call_isnull_present_field_is_false() {
        use crate::expr::parse;
        let row = row_with(&[("amount", Value::I64(0))]);
        let expr = parse("isnull(amount)").expect("should parse isnull");
        assert_eq!(eval(&expr, &row), Value::Bool(false));
    }

    // ── Test 19: unknown function → Null ─────────────────────────────────────

    #[test]
    fn eval_call_unknown_fn_is_null() {
        use crate::expr::parse;
        let row = row_with(&[("amount", Value::I64(1))]);
        let expr = parse("foobar(amount)").expect("should parse foobar call");
        assert_eq!(eval(&expr, &row), Value::Null);
    }

    // ── Test 20: (x == null) via parser rewrite ───────────────────────────────
    //
    // CONTRACT TEST between the parser and the evaluator.
    //
    // The parser's post-parse Pass B rewrites `BinOp("==", e, Literal::Null)` to
    // `Call("isnull", [e])` before the AST reaches eval.rs. This test verifies
    // that contract end-to-end:
    //
    //   parse("(amount == null)") → Call("isnull", [Field("amount")])
    //   eval(Call("isnull", [Field("amount")]), row{amount=Null}) → Bool(true)
    //   eval(Call("isnull", [Field("amount")]), row{amount=I64(5)}) → Bool(false)
    //
    // If the parser rewrite regresses (returns BinOp("==") instead of
    // Call("isnull")), this test fails with "got Null, expected Bool(true)"
    // because strict-null BinOp("==") returns Null when either operand is Null.
    #[test]
    fn eval_equals_null_literal_via_parser_rewrite() {
        use crate::expr::parse;

        let expr = parse("(amount == null)").expect("should parse (amount == null)");
        // The parse result MUST be Call("isnull", ...) — not BinOp("==") — due
        // to Plan 04-02's Pass B.
        assert!(
            matches!(&expr, crate::expr::Expr::Call { fn_name, .. } if fn_name == "isnull"),
            "parser must have rewritten (amount == null) to Call('isnull', ...), got {expr:?}"
        );

        // Row where amount IS null → Bool(true)
        let row_null = row_with(&[("amount", Value::Null)]);
        assert_eq!(
            eval(&expr, &row_null),
            Value::Bool(true),
            "(amount == null) with amount=Null must eval to Bool(true) via isnull rewrite"
        );

        // Row where amount is NOT null → Bool(false)
        let row_val = row_with(&[("amount", Value::I64(5))]);
        assert_eq!(
            eval(&expr, &row_val),
            Value::Bool(false),
            "(amount == null) with amount=I64(5) must eval to Bool(false) via isnull rewrite"
        );
    }

    // ── Test 21: nested binop ─────────────────────────────────────────────────

    #[test]
    fn eval_nested_binop() {
        use crate::expr::parse;

        let expr = parse("((amount > 100) and (merchant_id == 'M123'))").expect("should parse");
        let row_match = row_with(&[
            ("amount", Value::I64(150)),
            ("merchant_id", Value::Str("M123".into())),
        ]);
        assert_eq!(eval(&expr, &row_match), Value::Bool(true));

        let row_no_match = row_with(&[
            ("amount", Value::I64(150)),
            ("merchant_id", Value::Str("OTHER".into())),
        ]);
        assert_eq!(eval(&expr, &row_no_match), Value::Bool(false));
    }

    // ── Test 22: deep nesting does not stack overflow ─────────────────────────
    //
    // Construct a BinOp chain of depth 200 and verify eval completes.
    // 200 levels is within MAX_EVAL_DEPTH (512) so returns I64, not Null.

    #[test]
    fn eval_deep_nesting_does_not_stack_overflow() {
        // Build: ((...(1 + 1) + 1)...) of depth 200
        let mut expr = lit_int(1);
        for _ in 0..200 {
            expr = binop("+", expr, lit_int(1));
        }
        // 200 < MAX_EVAL_DEPTH(512), so should complete and return I64.
        let result = eval(&expr, &Row::new());
        assert!(
            matches!(result, Value::I64(_)),
            "deep nesting (depth 200) must return I64 (within limit), got {result:?}"
        );
    }

    // ── Test 22b: expression exceeding MAX_EVAL_DEPTH returns Null ────────────
    //
    // A BinOp chain of depth 600 (> MAX_EVAL_DEPTH=512) must return Null
    // rather than overflowing the stack. This is the DoS guard for CR-01.

    #[test]
    fn eval_exceeds_max_depth_returns_null() {
        // Build: ((...(1 + 1) + 1)...) of depth 600 — exceeds MAX_EVAL_DEPTH.
        let mut expr = lit_int(1);
        for _ in 0..600 {
            expr = binop("+", expr, lit_int(1));
        }
        // Must return Null (depth guard) without panicking.
        let result = eval(&expr, &Row::new());
        assert_eq!(
            result,
            Value::Null,
            "expression exceeding MAX_EVAL_DEPTH must return Null, got {result:?}"
        );
    }

    // ── Test 23 (proptest): evaluator is deterministic ────────────────────────
    //
    // For any (Expr, Row) pair, eval returns the same Value when called twice
    // with the same inputs (including on a clone of the Row).
    // Depth ≤ 3, ≥ 256 cases.

    use proptest::prelude::*;

    fn arb_value() -> impl Strategy<Value = Value> {
        prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            any::<i32>().prop_map(|n| Value::I64(n as i64)),
            (-1000.0f64..1000.0f64).prop_map(Value::F64),
            "[a-zA-Z0-9_]*".prop_map(|s: String| Value::Str(s.into())),
        ]
    }

    fn arb_row() -> impl Strategy<Value = Row> {
        let field_names = prop_oneof![Just("a"), Just("b"), Just("amount"), Just("x"),];
        prop::collection::btree_map(field_names, arb_value(), 0..=4).prop_map(|map| {
            let mut r = Row::new();
            for (k, v) in map {
                r = r.with_field(k, v);
            }
            r
        })
    }

    fn arb_expr(depth: u32) -> impl Strategy<Value = Expr> {
        let leaf = prop_oneof![
            Just(lit_null()),
            any::<bool>().prop_map(lit_bool),
            any::<i32>().prop_map(|n| lit_int(n as i64)),
            (-100.0f64..100.0f64).prop_map(lit_float),
            prop_oneof![
                Just(field_expr("a")),
                Just(field_expr("b")),
                Just(field_expr("amount")),
            ],
        ];
        leaf.prop_recursive(depth, 32, 3, |inner| {
            let ops = vec![
                "+", "-", "*", "/", ">", ">=", "<", "<=", "==", "!=", "and", "or",
            ];
            prop_oneof![
                (0usize..ops.len(), inner.clone(), inner.clone())
                    .prop_map(move |(idx, l, r)| { binop(ops[idx], l, r) }),
                inner.clone().prop_map(|e| unaryop("not", e)),
                inner.clone().prop_map(|e| call("isnull", vec![e])),
            ]
        })
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 256,
            ..Default::default()
        })]

        #[test]
        fn proptest_determinism(
            expr in arb_expr(3),
            row in arb_row(),
        ) {
            // Same (expr, row) → same Value on two calls
            let result1 = eval(&expr, &row);
            let result2 = eval(&expr, &row);
            // Use Debug repr equality as a proxy for F64 NaN cases
            // (Value doesn't impl Eq, NaN != NaN):
            prop_assert_eq!(
                format!("{result1:?}"),
                format!("{result2:?}"),
                "eval must be deterministic: same inputs must produce same output"
            );

            // Clone of row → same result
            let row2 = row.clone();
            let result3 = eval(&expr, &row2);
            prop_assert_eq!(
                format!("{result1:?}"),
                format!("{result3:?}"),
                "eval must be deterministic: clone of row must produce same output"
            );
        }
    }
}
