//! Builtin function registry for the expression evaluator.
//!
//! # Design
//!
//! Builtins are stored in a static `BUILTINS` table. Adding a new builtin
//! requires only appending one `BuiltinFn` entry — no grammar or evaluator
//! dispatch changes needed (SRV-APPLY-06 extension hook).
//!
//! Two builtins ship today:
//! - `cast(value, type_str)` — converts `Value` to target type; returns `Value::Null` on failure.
//! - `isnull(value)` — always returns `Bool(true/false)`, never `Null`.
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

// ─── Arity ────────────────────────────────────────────────────────────────────

/// Argument count constraint for a builtin function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arity {
    /// Exactly `n` arguments required.
    Fixed(usize),
    /// Any number of arguments (including zero).
    Variadic,
}

// ─── BuiltinFn ────────────────────────────────────────────────────────────────

/// A single entry in the builtin function table.
pub struct BuiltinFn {
    /// Function name as it appears in expressions (e.g. `"cast"`, `"isnull"`).
    pub name: &'static str,
    /// Expected argument count.
    pub arity: Arity,
    /// Evaluation function called by the evaluator after arguments have been
    /// evaluated to `Value`s.
    pub eval: fn(&[Value]) -> Value,
}

// ─── BUILTINS table ───────────────────────────────────────────────────────────

/// Static table of builtin functions.
///
/// Append a new `BuiltinFn` here to add one. No grammar or evaluator
/// dispatch surgery required — `lookup_builtin` performs a linear scan.
pub const BUILTINS: &[BuiltinFn] = &[
    BuiltinFn {
        name: "cast",
        arity: Arity::Fixed(2),
        eval: cast_eval,
    },
    BuiltinFn {
        name: "isnull",
        arity: Arity::Fixed(1),
        eval: isnull_eval,
    },
    BuiltinFn {
        name: "quadkey",
        arity: Arity::Fixed(3),
        eval: quadkey_eval,
    },
];

// ─── Lookup ───────────────────────────────────────────────────────────────────

/// Returns the `BuiltinFn` entry for `name`, or `None` if unknown.
///
/// Linear scan over the (currently 2-item) `BUILTINS` table. O(n) is fine
/// at the current scale; a `HashMap` would be premature.
pub fn lookup_builtin(name: &str) -> Option<&'static BuiltinFn> {
    BUILTINS.iter().find(|b| b.name == name)
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
    use crate::row::Value;

    // ── Lookup tests ──────────────────────────────────────────────────────────

    #[test]
    fn lookup_builtin_returns_cast() {
        let b = lookup_builtin("cast").expect("cast must be in BUILTINS");
        assert_eq!(b.name, "cast");
        assert_eq!(b.arity, Arity::Fixed(2));
    }

    #[test]
    fn lookup_builtin_returns_isnull() {
        let b = lookup_builtin("isnull").expect("isnull must be in BUILTINS");
        assert_eq!(b.name, "isnull");
        assert_eq!(b.arity, Arity::Fixed(1));
    }

    #[test]
    fn lookup_builtin_unknown_returns_none() {
        assert!(lookup_builtin("foo").is_none());
        assert!(lookup_builtin("").is_none());
        assert!(lookup_builtin("COUNT").is_none());
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
