//! Builtin function registry for the expression evaluator.
//!
//! # Design
//!
//! Builtins are a closed Rust enum `BuiltinFn` — one variant per builtin.
//! Adding a new builtin is one variant + arms in the five `match self`
//! methods (`name`, `from_name`, `arity`, `eval`, `infer`). The compiler
//! enforces exhaustiveness — no `_ =>` fallback arm, so a missing
//! handler is a hard compile error.
//!
//! Three builtins ship in PR 1:
//! - `cast(value, type)` — type-conversion operator. **TRANSITIONAL**:
//!   removed from this enum in PR 1 Step 8 when cast is promoted to
//!   `Expr::Cast` (its own AST variant).
//! - `isnull(value)` — always returns `Bool(true/false)`, never `Null`.
//! - `quadkey(lat, lon, zoom)` — geo cell ID.
//!
//! # Cast policy decisions (CONTEXT.md §D-05)
//!
//! - **Arity check**: wrong argument count → `Null` (register-time catches; runtime is defensive).
//! - **Null input**: `cast(null, any)` → `Null` (all targets).
//! - **Unknown target type**: → `Null`.
//! - **f64 → i64**: `as i64` (truncate toward zero, Rust default). Documents the truncation choice.
//! - **str → int/float**: `str.parse::<i64/f64>()` — fails → `Null`, not panic.
//! - **Bytes**: no implicit bytes-to-str without an encoding spec → `Null`.

pub(crate) mod _inference;

use crate::row::Value;
use crate::schema::FieldType;
use crate::schema_propagate::InferredType;
use _inference::{any_to_bool, require_arg_class, InferError, TypeClass};

// ─── Arity ────────────────────────────────────────────────────────────────────

/// Argument count constraint for a builtin function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// Exactly `n` arguments required.
    Fixed(usize),
    /// Any number of arguments (including zero).
    Variadic,
}

// ─── BuiltinFn ────────────────────────────────────────────────────────────────

/// Closed enum of builtin functions.
///
/// Each variant has arms in the five `match self` methods below; the
/// compiler enforces exhaustiveness. To add a builtin: add a variant +
/// one arm in each of `name`, `from_name`, `arity`, `eval`, `infer`.
///
/// `Copy + Hash + Eq` so callers can use `BuiltinFn` values freely
/// (hash-map keys, equality checks, copy across boundaries) without
/// borrow ceremony.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinFn {
    /// `cast(value, type_name)` — type-conversion operator.
    ///
    /// **TRANSITIONAL**: this variant exists only while the codebase
    /// still routes cast through `Call("cast", …)`. PR 1 Step 8
    /// promotes cast to a dedicated `Expr::Cast` AST variant and
    /// removes `Cast` from this enum. Cast is not value-shaped (its
    /// second "argument" is a type, not a value), so its `infer` arm
    /// returns a placeholder error and is unreachable in normal
    /// operation — cast inference is handled by the `fn_name == "cast"`
    /// arm in `schema_propagate.rs::infer_call_type` until Step 9
    /// collapses that block (and by then Cast is gone from here).
    Cast,
    /// `isnull(value)` — always returns `Bool(true/false)`, never `Null`.
    IsNull,
    /// `quadkey(lat, lon, zoom)` — geo cell ID. `lat`/`lon` numeric;
    /// `zoom` typed as numeric at register time (runtime requires
    /// strict `I64` in `1..=24`, otherwise `Null`).
    Quadkey,
}

impl BuiltinFn {
    /// Wire-format name. `const fn` because it's a pure lookup.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cast => "cast",
            Self::IsNull => "isnull",
            Self::Quadkey => "quadkey",
        }
    }

    /// Parse a wire-format name. `None` for unknown names.
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "cast" => Some(Self::Cast),
            "isnull" => Some(Self::IsNull),
            "quadkey" => Some(Self::Quadkey),
            _ => None,
        }
    }

    /// Argument count constraint.
    pub const fn arity(self) -> Arity {
        match self {
            Self::IsNull => Arity::Fixed(1),
            Self::Cast => Arity::Fixed(2),
            Self::Quadkey => Arity::Fixed(3),
        }
    }

    /// Per-event evaluator dispatch. Called after each arg has been
    /// evaluated to a `Value`.
    pub fn eval(self, args: &[Value]) -> Value {
        match self {
            Self::Cast => cast_eval(args),
            Self::IsNull => isnull_eval(args),
            Self::Quadkey => quadkey_eval(args),
        }
    }

    /// Register-time type inference.
    ///
    /// Single-arg signature: no `&[Expr]` parameter, because no
    /// value-shaped builtin needs AST access. The `Cast` arm is a
    /// transitional placeholder; see the variant doc.
    pub fn infer(self, arg_types: &[InferredType]) -> Result<InferredType, InferError> {
        match self {
            Self::IsNull => any_to_bool(arg_types),
            Self::Quadkey => quadkey_infer(arg_types),
            // Unreachable until Step 9 collapses the per-name match in
            // schema_propagate.rs (and by then Step 8 has removed Cast).
            Self::Cast => Err(InferError::Custom {
                reason: "cast inference handled by schema_propagate's \
                         per-name match; this arm is transitional and \
                         removed when cast moves to Expr::Cast (Step 8)"
                    .to_string(),
            }),
        }
    }

    /// All variants. Used by the name↔variant round-trip test and any
    /// future iteration need (testing, doc generation).
    #[cfg(test)]
    pub const fn all() -> &'static [BuiltinFn] {
        &[Self::Cast, Self::IsNull, Self::Quadkey]
    }
}

impl std::fmt::Display for BuiltinFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

// ─── quadkey_infer ────────────────────────────────────────────────────────────
//
// Quadkey's signature (Numeric, Numeric, Numeric) → I64 is unique enough that
// a shared helper wouldn't pull weight. Lives here next to quadkey_eval.

/// Register-time inference for `quadkey(lat, lon, zoom)`.
///
/// All three args typed as `Numeric` (I64 or F64); `NullLiteral` accepted
/// per the wildcard rule. Returns `I64`. Note: zoom is lenient at register
/// time — runtime requires strict `I64` in `1..=24` and returns `Null`
/// otherwise (matches existing `quadkey_eval` behavior).
fn quadkey_infer(arg_types: &[InferredType]) -> Result<InferredType, InferError> {
    require_arg_class(
        arg_types,
        &[TypeClass::Numeric, TypeClass::Numeric, TypeClass::Numeric],
    )?;
    Ok(InferredType::Known(FieldType::I64))
}

// ─── cast ─────────────────────────────────────────────────────────────────────

/// Evaluate `cast(value, type_str)`.
///
/// `args[0]` is the value to cast; `args[1]` is `Value::Str(type_str)` (the
/// evaluator converts `Literal::BareIdent` → `Value::Str` before dispatch, so
/// `cast(x, float)` arrives as `[eval(x), Value::Str("float")]`).
///
/// Returns `Value::Null` on any error: wrong arity, unknown type, null input,
/// or parse failure.
///
/// # Cast conversion matrix (CONTEXT.md §D-05)
///
/// | Source     | "str"          | "int"             | "float"           | "bool"               |
/// |------------|----------------|-------------------|-------------------|----------------------|
/// | Null       | Null           | Null              | Null              | Null                 |
/// | Str        | unchanged      | parse or Null     | parse or Null     | "true"→T,"false"→F   |
/// | I64        | fmt            | unchanged         | as f64            | ≠0 → true            |
/// | F64        | fmt            | as i64 (trunc)    | unchanged         | ≠0.0&&!NaN→true      |
/// | Bool       | "true"/"false" | 1 / 0             | 1.0 / 0.0         | unchanged            |
/// | Bytes      | Null           | Null              | Null              | Null                 |
/// | Datetime   | i64.to_string  | I64(ms)           | F64(ms as f64)    | ms≠0→true            |
fn cast_eval(args: &[Value]) -> Value {
    // Arity guard: must be exactly 2 args.
    if args.len() != 2 {
        return Value::Null;
    }

    // Target type comes as Value::Str (evaluator converts BareIdent → Str).
    let target = match &args[1] {
        Value::Str(s) => s.as_str(),
        _ => return Value::Null,
    };

    // Null input → always Null regardless of target.
    if matches!(args[0], Value::Null) {
        return Value::Null;
    }

    match target {
        "str" => cast_to_str(&args[0]),
        "int" => cast_to_int(&args[0]),
        "float" => cast_to_float(&args[0]),
        "bool" => cast_to_bool(&args[0]),
        _ => Value::Null,
    }
}

fn cast_to_str(v: &Value) -> Value {
    match v {
        Value::Null => Value::Null,
        Value::Str(s) => Value::Str(s.clone()),
        Value::I64(n) => Value::Str(n.to_string().into()),
        Value::F64(f) => Value::Str(f.to_string().into()),
        Value::Bool(b) => Value::Str((if *b { "true" } else { "false" }).into()),
        Value::Bytes(_) => Value::Null, // no implicit bytes→str without encoding spec
        Value::Datetime(ms) => Value::Str(ms.to_string().into()),
        Value::Json(_) => Value::Null,
        Value::List(_) | Value::Map(_) => Value::Null,
    }
}

fn cast_to_int(v: &Value) -> Value {
    match v {
        Value::Null => Value::Null,
        Value::I64(n) => Value::I64(*n),
        Value::F64(f) => Value::I64(*f as i64), // truncate toward zero
        Value::Bool(b) => Value::I64(if *b { 1 } else { 0 }),
        Value::Str(s) => s.parse::<i64>().map(Value::I64).unwrap_or(Value::Null),
        Value::Bytes(_) => Value::Null,
        Value::Datetime(ms) => Value::I64(*ms),
        Value::Json(_) => Value::Null,
        Value::List(_) | Value::Map(_) => Value::Null,
    }
}

fn cast_to_float(v: &Value) -> Value {
    match v {
        Value::Null => Value::Null,
        Value::I64(n) => Value::F64(*n as f64),
        Value::F64(f) => Value::F64(*f),
        Value::Bool(b) => Value::F64(if *b { 1.0 } else { 0.0 }),
        Value::Str(s) => s.parse::<f64>().map(Value::F64).unwrap_or(Value::Null),
        Value::Bytes(_) => Value::Null,
        Value::Datetime(ms) => Value::F64(*ms as f64),
        Value::Json(_) => Value::Null,
        Value::List(_) | Value::Map(_) => Value::Null,
    }
}

fn cast_to_bool(v: &Value) -> Value {
    match v {
        Value::Null => Value::Null,
        Value::Bool(b) => Value::Bool(*b),
        Value::I64(n) => Value::Bool(*n != 0),
        Value::F64(f) => Value::Bool(*f != 0.0 && !f.is_nan()),
        Value::Str(s) => match s.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::Null,
        },
        Value::Bytes(_) => Value::Null,
        Value::Datetime(ms) => Value::Bool(*ms != 0),
        Value::Json(_) => Value::Null,
        Value::List(_) | Value::Map(_) => Value::Null,
    }
}

// ─── quadkey ─────────────────────────────────────────────────────────────────

/// Evaluate `quadkey(lat, lon, zoom)`.
///
/// Returns a deterministic `Value::I64` cell-id using a simplified-Mercator
/// formula (NOT RFC slippy-tile — no external tile dependency required).
///
/// # Formula
///
/// ```text
/// n   = 1 << zoom                         (tiles per axis)
/// row = floor((sin(lat_clamped_rad) + 1) / 2 * n)
/// col = floor((lon + 180) / 360 * n)
/// cell_id = col * n + row.clamp(0, n-1)
/// ```
///
/// # Null / range rules
/// - Any `Null` argument → `Null`.
/// - `zoom` outside `1..=24` → `Null`.
/// - `lat` is clamped to `[-85.05112878, 85.05112878]` (Web-Mercator bounds).
fn quadkey_eval(args: &[Value]) -> Value {
    if args.len() != 3 {
        return Value::Null;
    }
    let lat = match &args[0] {
        Value::F64(v) => *v,
        Value::I64(v) => *v as f64,
        _ => return Value::Null,
    };
    let lon = match &args[1] {
        Value::F64(v) => *v,
        Value::I64(v) => *v as f64,
        _ => return Value::Null,
    };
    let zoom = match &args[2] {
        Value::I64(v) if (1..=24).contains(v) => *v,
        _ => return Value::Null,
    };
    let n = 1i64 << zoom;
    let lat_clamped = lat.clamp(-85.051_128_78, 85.051_128_78);
    let row = ((lat_clamped.to_radians().sin() + 1.0) / 2.0 * (n as f64)).floor() as i64;
    let col = ((lon + 180.0) / 360.0 * (n as f64)).floor() as i64;
    Value::I64(
        col.saturating_mul(n)
            .saturating_add(row.clamp(0, n.saturating_sub(1))),
    )
}

// ─── isnull ───────────────────────────────────────────────────────────────────

/// Evaluate `isnull(value)`.
///
/// Always returns `Bool(true/false)` — never `Null`.
fn isnull_eval(args: &[Value]) -> Value {
    // Arity guard: must be exactly 1 arg (defensive; register-time catches).
    if args.len() != 1 {
        return Value::Null;
    }
    Value::Bool(matches!(args[0], Value::Null))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::_inference::InferError;
    use crate::row::Value;
    use crate::schema::FieldType;
    use crate::schema_propagate::InferredType;

    // ── Lookup tests ──────────────────────────────────────────────────────────

    #[test]
    fn from_name_returns_cast() {
        let b = BuiltinFn::from_name("cast").expect("cast must be a BuiltinFn variant");
        assert_eq!(b, BuiltinFn::Cast);
        assert_eq!(b.name(), "cast");
        assert_eq!(b.arity(), Arity::Fixed(2));
    }

    #[test]
    fn from_name_returns_isnull() {
        let b = BuiltinFn::from_name("isnull").expect("isnull must be a BuiltinFn variant");
        assert_eq!(b, BuiltinFn::IsNull);
        assert_eq!(b.name(), "isnull");
        assert_eq!(b.arity(), Arity::Fixed(1));
    }

    #[test]
    fn from_name_returns_quadkey() {
        let b = BuiltinFn::from_name("quadkey").expect("quadkey must be a BuiltinFn variant");
        assert_eq!(b, BuiltinFn::Quadkey);
        assert_eq!(b.name(), "quadkey");
        assert_eq!(b.arity(), Arity::Fixed(3));
    }

    #[test]
    fn from_name_unknown_returns_none() {
        assert!(BuiltinFn::from_name("foo").is_none());
        assert!(BuiltinFn::from_name("").is_none());
        assert!(BuiltinFn::from_name("COUNT").is_none());
    }

    // ── Name ↔ variant round-trip (permanent guard against mirror drift) ─────

    #[test]
    fn name_from_name_roundtrip() {
        for &b in BuiltinFn::all() {
            assert_eq!(
                BuiltinFn::from_name(b.name()),
                Some(b),
                "round-trip failed for {b:?}: name() = {:?}, from_name returned different variant",
                b.name()
            );
        }
    }

    // ── Display ───────────────────────────────────────────────────────────────

    #[test]
    fn display_emits_wire_name() {
        assert_eq!(format!("{}", BuiltinFn::Cast), "cast");
        assert_eq!(format!("{}", BuiltinFn::IsNull), "isnull");
        assert_eq!(format!("{}", BuiltinFn::Quadkey), "quadkey");
    }

    // ── BuiltinFn::eval dispatch ──────────────────────────────────────────────

    #[test]
    fn enum_eval_dispatches_to_underlying_fns() {
        // Spot-check that match-arm dispatch in BuiltinFn::eval reaches the
        // same fn that the per-fn tests below verify in detail.
        assert_eq!(BuiltinFn::IsNull.eval(&[Value::Null]), Value::Bool(true));
        assert_eq!(
            BuiltinFn::Cast.eval(&[Value::I64(7), Value::Str("float".into())]),
            Value::F64(7.0)
        );
        // Quadkey: just confirm it returns I64 for valid input (full eval
        // tested in tests/op_removal.rs).
        let r = BuiltinFn::Quadkey.eval(&[Value::F64(40.0), Value::F64(-74.0), Value::I64(7)]);
        assert!(matches!(r, Value::I64(_)));
    }

    // ── BuiltinFn::infer dispatch ─────────────────────────────────────────────

    #[test]
    fn enum_infer_dispatches_to_helpers() {
        // IsNull → any_to_bool → Bool
        assert_eq!(
            BuiltinFn::IsNull.infer(&[InferredType::Known(FieldType::I64)]),
            Ok(InferredType::Known(FieldType::Bool))
        );
        // Quadkey → quadkey_infer → I64
        let quadkey_args = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::I64),
        ];
        assert_eq!(
            BuiltinFn::Quadkey.infer(&quadkey_args),
            Ok(InferredType::Known(FieldType::I64))
        );
        // Cast → transitional placeholder → Custom error.
        assert!(matches!(
            BuiltinFn::Cast.infer(&[]),
            Err(InferError::Custom { .. })
        ));
    }

    // ── isnull tests ──────────────────────────────────────────────────────────

    #[test]
    fn isnull_of_null_is_bool_true() {
        assert_eq!(isnull_eval(&[Value::Null]), Value::Bool(true));
    }

    #[test]
    fn isnull_of_i64_is_bool_false() {
        assert_eq!(isnull_eval(&[Value::I64(42)]), Value::Bool(false));
        assert_eq!(isnull_eval(&[Value::I64(0)]), Value::Bool(false));
        assert_eq!(isnull_eval(&[Value::I64(-1)]), Value::Bool(false));
    }

    #[test]
    fn isnull_of_str_is_bool_false() {
        assert_eq!(
            isnull_eval(&[Value::Str("hello".into())]),
            Value::Bool(false)
        );
        assert_eq!(
            isnull_eval(&[Value::Str(compact_str::CompactString::new(""))]),
            Value::Bool(false)
        );
    }

    // ── cast tests ────────────────────────────────────────────────────────────

    // cast(I64(7), Str("float")) → F64(7.0)
    #[test]
    fn cast_int_to_float_ok() {
        assert_eq!(
            cast_eval(&[Value::I64(7), Value::Str("float".into())]),
            Value::F64(7.0)
        );
    }

    // cast(Str("42"), Str("int")) → I64(42)
    #[test]
    fn cast_str_to_int_parses_numeric() {
        assert_eq!(
            cast_eval(&[Value::Str("42".into()), Value::Str("int".into())]),
            Value::I64(42)
        );
    }

    // cast(Str("abc"), Str("int")) → Null (parse failure)
    #[test]
    fn cast_str_to_int_nonnumeric_is_null() {
        assert_eq!(
            cast_eval(&[Value::Str("abc".into()), Value::Str("int".into())]),
            Value::Null
        );
    }

    // cast(Null, Str("int")) → Null
    #[test]
    fn cast_null_any_is_null() {
        assert_eq!(
            cast_eval(&[Value::Null, Value::Str("int".into())]),
            Value::Null
        );
        assert_eq!(
            cast_eval(&[Value::Null, Value::Str("str".into())]),
            Value::Null
        );
        assert_eq!(
            cast_eval(&[Value::Null, Value::Str("float".into())]),
            Value::Null
        );
        assert_eq!(
            cast_eval(&[Value::Null, Value::Str("bool".into())]),
            Value::Null
        );
    }

    // cast(I64(1), Str("blob")) → Null (unknown target type)
    #[test]
    fn cast_unknown_type_is_null() {
        assert_eq!(
            cast_eval(&[Value::I64(1), Value::Str("blob".into())]),
            Value::Null
        );
        assert_eq!(
            cast_eval(&[Value::I64(1), Value::Str("bytes".into())]),
            Value::Null
        );
    }

    // cast(F64(3.9), Str("int")) → I64(3)  (truncate toward zero, Rust `as i64`)
    #[test]
    fn cast_float_to_int_truncates() {
        // Truncation toward zero: 3.9 → 3, -3.9 → -3
        assert_eq!(
            cast_eval(&[Value::F64(3.9), Value::Str("int".into())]),
            Value::I64(3)
        );
        assert_eq!(
            cast_eval(&[Value::F64(-3.9), Value::Str("int".into())]),
            Value::I64(-3)
        );
    }

    // cast(Bool(true), Str("int")) → I64(1); cast(Bool(false), Str("int")) → I64(0)
    #[test]
    fn cast_bool_to_int() {
        assert_eq!(
            cast_eval(&[Value::Bool(true), Value::Str("int".into())]),
            Value::I64(1)
        );
        assert_eq!(
            cast_eval(&[Value::Bool(false), Value::Str("int".into())]),
            Value::I64(0)
        );
    }

    // cast(I64(42), Str("str")) → Str("42")
    #[test]
    fn cast_int_to_str() {
        assert_eq!(
            cast_eval(&[Value::I64(42), Value::Str("str".into())]),
            Value::Str("42".into())
        );
    }

    // cast(I64(1)) [wrong arity] → Null  (defensive; register-time catches)
    #[test]
    fn cast_arity_wrong_is_null() {
        assert_eq!(cast_eval(&[Value::I64(1)]), Value::Null);
        assert_eq!(cast_eval(&[]), Value::Null);
        assert_eq!(
            cast_eval(&[
                Value::I64(1),
                Value::Str("int".into()),
                Value::Str("extra".into())
            ]),
            Value::Null
        );
    }
}
