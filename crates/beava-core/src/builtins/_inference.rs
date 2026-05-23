//! Shared type-inference helpers and primitives for builtins.
//!
//! Each variant in `BuiltinFn` (in `builtins/mod.rs`) dispatches its
//! register-time inference through a `match self` arm; this module is
//! the single source of:
//!
//! - **Primitives** (`require_arg_types`, `require_arg_class`,
//!   `unify_var0_strict`, `unify_var0_with_class`) — small building
//!   blocks for one-off inference fns.
//! - **Shared helpers** (`any_to_bool`, `unary_str_to_str`,
//!   `unary_numeric_to_f64`, …) — one per common signature shape, so
//!   each builtin row in `BuiltinFn::infer`'s match block is a one-liner.
//!
//! Step 3 lands `InferError` and `TypeClass` here; Step 4 lands the
//! primitives and helpers themselves.

use crate::schema::FieldType;
use crate::schema_propagate::InferredType;

// ─── InferError ──────────────────────────────────────────────────────────────

/// Errors a builtin's `infer` fn can return.
///
/// Smaller and more focused than `PropagationError` — the dispatcher in
/// `schema_propagate.rs::infer_call_type` converts these into
/// `PropagationError::TypeMismatch` with a rendered reason string at the
/// call site. Builtin authors only deal with this enum, never the
/// schema-walker's error vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferError {
    /// Argument count mismatch caught inside an infer fn.
    ///
    /// Rare in practice — `BuiltinFn::arity()` is checked by the dispatcher
    /// before `infer` runs. Variadic helpers (`polymorphic_var0_unify`)
    /// can still produce this if a per-helper minimum-arity rule fails.
    Arity { expected: usize, got: usize },

    /// An argument's `InferredType` does not satisfy the helper's
    /// expected `FieldType` or `TypeClass` at position `arg_idx`.
    ///
    /// `expected` is a static human-readable label (e.g. `"Str"`,
    /// `"Numeric"`) used directly in the rendered error message.
    /// `got` is the actual inferred type, included so the message can
    /// quote it back to the user.
    TypeMismatch {
        arg_idx: usize,
        expected: &'static str,
        got: InferredType,
    },

    /// Polymorphic unification disagreed (e.g. `coalesce(int, float)`
    /// where both args are concrete but different `FieldType`s).
    ///
    /// `reason` names the conflict ("I64 vs F64") and is used directly
    /// in the rendered error message.
    Unify { reason: &'static str },

    /// Builtin-specific validation failure that doesn't fit the
    /// `Arity` / `TypeMismatch` / `Unify` shapes.
    ///
    /// Reserved for future builtins with one-off semantics. Owned
    /// `String` because the message often interpolates runtime data.
    /// No PR 1 builtin uses this variant.
    Custom { reason: String },
}

// ─── TypeClass ───────────────────────────────────────────────────────────────

/// Coarse type group used by polymorphic builtins.
///
/// Lets helpers say "this arg must be numeric" without enumerating
/// every `FieldType`. The wildcard rule (RFC-001 §5.1) is handled by
/// the callers (`require_arg_class` etc.), not here — `accepts` only
/// answers the strict question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeClass {
    /// `I64` or `F64`. Used by all math builtins.
    Numeric,
    /// Any concrete `FieldType`. Used by `isnull`, which doesn't care
    /// what it's being asked about.
    Any,
}

impl TypeClass {
    /// True if `ft` belongs to this class.
    ///
    /// Note: `InferredType::NullLiteral` is not a `FieldType` and is
    /// handled separately at the helper boundary (always accepted as
    /// the wildcard rule).
    pub fn accepts(self, ft: FieldType) -> bool {
        match self {
            TypeClass::Numeric => matches!(ft, FieldType::I64 | FieldType::F64),
            TypeClass::Any => true,
        }
    }
}

// ─── Static name helpers ─────────────────────────────────────────────────────
//
// `InferError::TypeMismatch::expected` and `InferError::Unify::reason` are
// `&'static str`. These const helpers pick the static label for each
// FieldType / TypeClass / pair so error messages can quote a stable name.

const fn field_type_name(ft: FieldType) -> &'static str {
    match ft {
        FieldType::Str => "Str",
        FieldType::I64 => "I64",
        FieldType::F64 => "F64",
        FieldType::Bool => "Bool",
        FieldType::Bytes => "Bytes",
        FieldType::Datetime => "Datetime",
        FieldType::Json => "Json",
    }
}

const fn type_class_name(tc: TypeClass) -> &'static str {
    match tc {
        TypeClass::Numeric => "Numeric",
        TypeClass::Any => "Any",
    }
}

/// Stable `&'static str` for the unify-mismatch reason. Covers the common
/// scalar pairs explicitly; other combinations fall back to a generic label.
/// The dispatcher prefixes this with the function name when rendering.
const fn unify_reason(a: FieldType, b: FieldType) -> &'static str {
    use FieldType::*;
    match (a, b) {
        (I64, F64) | (F64, I64) => "I64 vs F64",
        (I64, Str) | (Str, I64) => "I64 vs Str",
        (I64, Bool) | (Bool, I64) => "I64 vs Bool",
        (F64, Str) | (Str, F64) => "F64 vs Str",
        (F64, Bool) | (Bool, F64) => "F64 vs Bool",
        (Str, Bool) | (Bool, Str) => "Str vs Bool",
        _ => "type mismatch",
    }
}

// ─── Primitives ──────────────────────────────────────────────────────────────

/// Require each arg's inferred type to exactly match the expected `FieldType`
/// at the same position.
///
/// `NullLiteral` is accepted at any position regardless of the expected type
/// (the wildcard rule — mirrors null-propagating runtime, RFC-001 §5.1).
///
/// Returns `Err(InferError::Arity)` if lengths differ, or
/// `Err(InferError::TypeMismatch)` naming the first bad position.
pub fn require_arg_types(
    arg_types: &[InferredType],
    expected: &[FieldType],
) -> Result<(), InferError> {
    if arg_types.len() != expected.len() {
        return Err(InferError::Arity {
            expected: expected.len(),
            got: arg_types.len(),
        });
    }
    for (i, (got, &exp)) in arg_types.iter().zip(expected.iter()).enumerate() {
        match got {
            InferredType::NullLiteral => continue,
            InferredType::Known(ft) if *ft == exp => continue,
            _ => {
                return Err(InferError::TypeMismatch {
                    arg_idx: i,
                    expected: field_type_name(exp),
                    got: got.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Require each arg to satisfy the expected `TypeClass` at the same position.
///
/// `NullLiteral` is accepted at any position (wildcard rule).
///
/// Returns `Err(InferError::Arity)` if lengths differ, or
/// `Err(InferError::TypeMismatch)` naming the first bad position.
pub fn require_arg_class(
    arg_types: &[InferredType],
    expected: &[TypeClass],
) -> Result<(), InferError> {
    if arg_types.len() != expected.len() {
        return Err(InferError::Arity {
            expected: expected.len(),
            got: arg_types.len(),
        });
    }
    for (i, (got, &class)) in arg_types.iter().zip(expected.iter()).enumerate() {
        match got {
            InferredType::NullLiteral => continue,
            InferredType::Known(ft) if class.accepts(*ft) => continue,
            _ => {
                return Err(InferError::TypeMismatch {
                    arg_idx: i,
                    expected: type_class_name(class),
                    got: got.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Strict-equality unification across the `arg_types` positions listed in
/// `indices`.
///
/// - `NullLiteral` acts as a hole — doesn't pin the type, doesn't conflict
///   with anything. The first concrete `Known(...)` arg determines the bound
///   type; later args must equal it or fail with `InferError::Unify`.
/// - If every listed position is `NullLiteral`, falls back to
///   `Known(FieldType::Str)`. Arbitrary-but-documented default (RFC-001 §5.1).
///
/// Returns the unified `InferredType` — always `Known(...)` because the
/// all-null branch resolves to `Known(Str)`.
pub fn unify_var0_strict(
    arg_types: &[InferredType],
    indices: &[usize],
) -> Result<InferredType, InferError> {
    let mut bound: Option<FieldType> = None;
    for &idx in indices {
        match &arg_types[idx] {
            InferredType::NullLiteral => continue,
            InferredType::Known(ft) => match bound {
                None => bound = Some(*ft),
                Some(b) if b == *ft => continue,
                Some(b) => {
                    return Err(InferError::Unify {
                        reason: unify_reason(b, *ft),
                    });
                }
            },
        }
    }
    Ok(InferredType::Known(bound.unwrap_or(FieldType::Str)))
}

/// As `unify_var0_strict`, but each binding must also satisfy `class`.
///
/// Order of checks: per-arg class first (so `coalesce(Str, Str)` under
/// `Numeric` reports the first non-numeric arg with `arg_idx = 0`), then
/// strict equality (so `coalesce(I64, F64)` under `Numeric` reports the
/// mix as `Unify` since both fit the class).
pub fn unify_var0_with_class(
    arg_types: &[InferredType],
    indices: &[usize],
    class: TypeClass,
) -> Result<InferredType, InferError> {
    let mut bound: Option<FieldType> = None;
    for &idx in indices {
        match &arg_types[idx] {
            InferredType::NullLiteral => continue,
            InferredType::Known(ft) if class.accepts(*ft) => match bound {
                None => bound = Some(*ft),
                Some(b) if b == *ft => continue,
                Some(b) => {
                    return Err(InferError::Unify {
                        reason: unify_reason(b, *ft),
                    });
                }
            },
            InferredType::Known(_) => {
                return Err(InferError::TypeMismatch {
                    arg_idx: idx,
                    expected: type_class_name(class),
                    got: arg_types[idx].clone(),
                });
            }
        }
    }
    Ok(InferredType::Known(bound.unwrap_or(FieldType::Str)))
}

// ─── Shared helpers ──────────────────────────────────────────────────────────
//
// One per common signature shape. Each is a 1-2 line wrapper over the
// primitives. `BuiltinFn::infer` arms point at these so each builtin row is
// a one-liner.

/// `isnull` — accepts one arg of any type; returns `Bool`.
pub fn any_to_bool(arg_types: &[InferredType]) -> Result<InferredType, InferError> {
    require_arg_class(arg_types, &[TypeClass::Any])?;
    Ok(InferredType::Known(FieldType::Bool))
}

/// `lower`, `upper` — one `Str` arg, returns `Str`.
pub fn unary_str_to_str(arg_types: &[InferredType]) -> Result<InferredType, InferError> {
    require_arg_types(arg_types, &[FieldType::Str])?;
    Ok(InferredType::Known(FieldType::Str))
}

/// `length` — one `Str` arg, returns `I64`.
pub fn unary_str_to_i64(arg_types: &[InferredType]) -> Result<InferredType, InferError> {
    require_arg_types(arg_types, &[FieldType::Str])?;
    Ok(InferredType::Known(FieldType::I64))
}

/// `abs`, `sign`, `floor`, `ceil`, `round` — one numeric arg; returns the
/// same numeric type. Identity on the input — preserves `I64` vs `F64`, and
/// propagates `NullLiteral` (the only helper that does so).
pub fn unary_numeric_same(arg_types: &[InferredType]) -> Result<InferredType, InferError> {
    require_arg_class(arg_types, &[TypeClass::Numeric])?;
    Ok(arg_types[0].clone())
}

/// `log`, `log1p`, `log10`, `exp`, `sqrt` — one numeric arg; returns `F64`.
pub fn unary_numeric_to_f64(arg_types: &[InferredType]) -> Result<InferredType, InferError> {
    require_arg_class(arg_types, &[TypeClass::Numeric])?;
    Ok(InferredType::Known(FieldType::F64))
}

/// `pow`, `mod` — two numeric args; returns `F64`.
pub fn binary_numeric_to_f64(arg_types: &[InferredType]) -> Result<InferredType, InferError> {
    require_arg_class(arg_types, &[TypeClass::Numeric, TypeClass::Numeric])?;
    Ok(InferredType::Known(FieldType::F64))
}

/// `contains`, `starts_with`, `ends_with` — `(Str, Str) -> Bool`.
pub fn string_search_to_bool(arg_types: &[InferredType]) -> Result<InferredType, InferError> {
    require_arg_types(arg_types, &[FieldType::Str, FieldType::Str])?;
    Ok(InferredType::Known(FieldType::Bool))
}

/// `coalesce`, `fill_null` — variadic; all args unify under strict equality
/// with `NullLiteral` as the hole. All-null (including zero args) falls back
/// to `Known(Str)` per the documented default.
pub fn polymorphic_var0_unify(
    arg_types: &[InferredType],
) -> Result<InferredType, InferError> {
    let indices: Vec<usize> = (0..arg_types.len()).collect();
    unify_var0_strict(arg_types, &indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::FieldType;
    use crate::schema_propagate::InferredType;

    // ── Tests 1–13: Primitives ────────────────────────────────────────────────
    //
    // Note: `read_literal_type_name` is NOT a PR 1 primitive — it existed only
    // for `cast_infer` reading the type-name literal from Call args. With cast
    // promoted to Expr::Cast, the type-name lives on Expr::Cast.target
    // and is read by a dedicated arm in schema_propagate.rs::infer_expr_type_inner.
    // Defer this primitive until a future builtin actually needs AST access.

    // ── Test 1: require_arg_types accepts exact match ─────────────────────────
    // Why: types line up exactly → ok. Simplest happy path.

    #[test]
    fn require_arg_types_accepts_exact_match() {
        let args = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::Str),
        ];
        let r = require_arg_types(&args, &[FieldType::I64, FieldType::Str]);
        assert!(r.is_ok());
    }

    // ── Test 2: require_arg_types accepts NullLiteral anywhere ────────────────
    // Why: null is always allowed in any slot, so users can write `log(maybe_null)` without wrapping in a cast.

    #[test]
    fn require_arg_types_accepts_null_literal_anywhere() {
        // NullLiteral at position 0
        let args0 = [
            InferredType::NullLiteral,
            InferredType::Known(FieldType::Str),
        ];
        let r0 = require_arg_types(&args0, &[FieldType::I64, FieldType::Str]);
        assert!(r0.is_ok(), "NullLiteral should be accepted at position 0");

        // NullLiteral at position 1
        let args1 = [
            InferredType::Known(FieldType::I64),
            InferredType::NullLiteral,
        ];
        let r1 = require_arg_types(&args1, &[FieldType::I64, FieldType::Str]);
        assert!(r1.is_ok(), "NullLiteral should be accepted at position 1");
    }

    // ── Test 3: require_arg_types rejects arity mismatch ──────────────────────
    // Why: wrong number of args → loud error, not silent truncation or padding.

    #[test]
    fn require_arg_types_rejects_arity_mismatch() {
        // Too few: expected 2, got 1
        let args_short = [InferredType::Known(FieldType::I64)];
        let r_short = require_arg_types(&args_short, &[FieldType::I64, FieldType::Str]);
        assert!(matches!(
            r_short,
            Err(InferError::Arity {
                expected: 2,
                got: 1
            })
        ));

        // Too many: expected 2, got 3
        let args_long = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::I64),
        ];
        let r_long = require_arg_types(&args_long, &[FieldType::I64, FieldType::Str]);
        assert!(matches!(
            r_long,
            Err(InferError::Arity {
                expected: 2,
                got: 3
            })
        ));
    }

    // ── Test 4: require_arg_types rejects wrong type at idx ───────────────────
    // Why: wrong type → the error message says which arg is bad so users can fix it quickly.

    #[test]
    fn require_arg_types_rejects_wrong_type_at_idx() {
        // Wrong type at idx 0
        let args0 = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
        ];
        let r0 = require_arg_types(&args0, &[FieldType::I64, FieldType::Str]);
        assert!(matches!(
            r0,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));

        // Wrong type at idx 1
        let args1 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
        ];
        let r1 = require_arg_types(&args1, &[FieldType::I64, FieldType::Str]);
        assert!(matches!(
            r1,
            Err(InferError::TypeMismatch { arg_idx: 1, .. })
        ));
    }

    // ── Test 5: require_arg_class Numeric accepts I64 and F64 ─────────────────
    // Why: "Numeric" should mean either int or float in any mix, so `pow(2, 3.0)` works.

    #[test]
    fn require_arg_class_numeric_accepts_i64_and_f64() {
        // I64 + F64
        let args0 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::F64),
        ];
        assert!(require_arg_class(&args0, &[TypeClass::Numeric, TypeClass::Numeric]).is_ok());

        // F64 + I64 (order swapped)
        let args1 = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::I64),
        ];
        assert!(require_arg_class(&args1, &[TypeClass::Numeric, TypeClass::Numeric]).is_ok());

        // NullLiteral anywhere
        let args2 = [
            InferredType::NullLiteral,
            InferredType::Known(FieldType::I64),
        ];
        assert!(require_arg_class(&args2, &[TypeClass::Numeric, TypeClass::Numeric]).is_ok());
    }

    // ── Test 6: require_arg_class Any accepts every type ──────────────────────
    // Why: "Any" should literally accept anything — used by isnull, which doesn't care about input type.

    #[test]
    fn require_arg_class_any_accepts_every_type() {
        for ft in [
            FieldType::Str,
            FieldType::I64,
            FieldType::F64,
            FieldType::Bool,
        ] {
            let args = [InferredType::Known(ft)];
            assert!(
                require_arg_class(&args, &[TypeClass::Any]).is_ok(),
                "Any should accept {ft:?}"
            );
        }
        let args_null = [InferredType::NullLiteral];
        assert!(require_arg_class(&args_null, &[TypeClass::Any]).is_ok());
    }

    // ── Test 7: require_arg_class Numeric rejects Str ─────────────────────────
    // Why: strings aren't numbers. Error names the bad arg so users know which one to fix.

    #[test]
    fn require_arg_class_numeric_rejects_str() {
        // Str at idx 0
        let args0 = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::I64),
        ];
        let r0 = require_arg_class(&args0, &[TypeClass::Numeric, TypeClass::Numeric]);
        assert!(matches!(
            r0,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));

        // Str at idx 1
        let args1 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::Str),
        ];
        let r1 = require_arg_class(&args1, &[TypeClass::Numeric, TypeClass::Numeric]);
        assert!(matches!(
            r1,
            Err(InferError::TypeMismatch { arg_idx: 1, .. })
        ));
    }

    // ── Test 8: unify_var0_strict same type returns it ────────────────────────
    // Why: when all args have the same type, that's the answer. No silent upgrades.

    #[test]
    fn unify_var0_strict_same_type_returns_that_type() {
        // I64 + I64 → I64
        let args_i64 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
        ];
        assert_eq!(
            unify_var0_strict(&args_i64, &[0, 1]).unwrap(),
            InferredType::Known(FieldType::I64)
        );

        // F64 + F64 → F64 (verify result type isn't hard-coded to I64)
        let args_f64 = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::F64),
        ];
        assert_eq!(
            unify_var0_strict(&args_f64, &[0, 1]).unwrap(),
            InferredType::Known(FieldType::F64)
        );

        // Str + Str → Str
        let args_str = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
        ];
        assert_eq!(
            unify_var0_strict(&args_str, &[0, 1]).unwrap(),
            InferredType::Known(FieldType::Str)
        );
    }

    // ── Test 9: unify_var0_strict NullLiteral as hole ─────────────────────────
    // Why: a `null` literal doesn't pick the type. The real-typed arg wins, so `if_else(c, null, 5.0)` is float.

    #[test]
    fn unify_var0_strict_null_literal_is_hole() {
        // NullLiteral at position 0
        let args0 = [
            InferredType::NullLiteral,
            InferredType::Known(FieldType::F64),
        ];
        assert_eq!(
            unify_var0_strict(&args0, &[0, 1]).unwrap(),
            InferredType::Known(FieldType::F64)
        );

        // NullLiteral at position 1
        let args1 = [
            InferredType::Known(FieldType::F64),
            InferredType::NullLiteral,
        ];
        assert_eq!(
            unify_var0_strict(&args1, &[0, 1]).unwrap(),
            InferredType::Known(FieldType::F64)
        );
    }

    // ── Test 10: unify_var0_strict all-null falls back to Str ─────────────────
    // Why: if everything is null, pick a default. String was chosen — arbitrary but documented, so users can rely on it.

    #[test]
    fn unify_var0_strict_all_null_falls_back_to_str() {
        let args = [InferredType::NullLiteral, InferredType::NullLiteral];
        let r = unify_var0_strict(&args, &[0, 1]);
        assert_eq!(
            r.unwrap(),
            InferredType::Known(FieldType::Str),
            "all-null binding should fall back to Str"
        );
    }

    // ── Test 11: unify_var0_strict rejects I64 vs F64 ─────────────────────────
    // Why: int and float are different. Mixing them must fail loudly here, not silently upgrade like `+` does.

    #[test]
    fn unify_var0_strict_rejects_i64_vs_f64() {
        // I64 then F64
        let args0 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::F64),
        ];
        assert!(matches!(
            unify_var0_strict(&args0, &[0, 1]),
            Err(InferError::Unify { .. })
        ));

        // F64 then I64 (verify order-independent)
        let args1 = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::I64),
        ];
        assert!(matches!(
            unify_var0_strict(&args1, &[0, 1]),
            Err(InferError::Unify { .. })
        ));
    }

    // ── Test 12: unify_var0_with_class strict and class OK ────────────────────
    // Why: combining the two checks should work — if args are the same type AND fit the class, pass.

    #[test]
    fn unify_var0_with_class_combines_strict_and_class_ok() {
        // I64 + I64 under Numeric → I64
        let args_i64 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
        ];
        assert_eq!(
            unify_var0_with_class(&args_i64, &[0, 1], TypeClass::Numeric).unwrap(),
            InferredType::Known(FieldType::I64)
        );

        // F64 + F64 under Numeric → F64
        let args_f64 = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::F64),
        ];
        assert_eq!(
            unify_var0_with_class(&args_f64, &[0, 1], TypeClass::Numeric).unwrap(),
            InferredType::Known(FieldType::F64)
        );
    }

    // ── Test 13: unify_var0_with_class rejects class violation ────────────────
    // Why: even if args are the same type, wrong class still fails (e.g. `Str + Str` is consistent but not numeric).

    #[test]
    fn unify_var0_with_class_rejects_class_violation() {
        // Both args violate Numeric (Str + Str) — first idx surfaces
        let args0 = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
        ];
        assert!(matches!(
            unify_var0_with_class(&args0, &[0, 1], TypeClass::Numeric),
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));

        // Only idx 1 violates (I64 + Str)
        let args1 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::Str),
        ];
        assert!(matches!(
            unify_var0_with_class(&args1, &[0, 1], TypeClass::Numeric),
            Err(InferError::TypeMismatch { arg_idx: 1, .. })
        ));
    }

    // ── Tests 14, 18–38: Helpers (numbering preserved from pre-cleanup; ──────
    // ── tests 15–17 covered `read_literal_type_name`, now deferred) ──────────

    // ── Test 14: any_to_bool returns Bool for any input ───────────────────────
    // Why: isnull always answers true or false, regardless of what's being checked.

    #[test]
    fn any_to_bool_returns_bool_for_any_input() {
        let args_i64 = [InferredType::Known(FieldType::I64)];
        assert_eq!(
            any_to_bool(&args_i64).unwrap(),
            InferredType::Known(FieldType::Bool)
        );

        let args_str = [InferredType::Known(FieldType::Str)];
        assert_eq!(
            any_to_bool(&args_str).unwrap(),
            InferredType::Known(FieldType::Bool)
        );

        let args_null = [InferredType::NullLiteral];
        assert_eq!(
            any_to_bool(&args_null).unwrap(),
            InferredType::Known(FieldType::Bool)
        );
    }

    // ── Test 18: unary_str_to_str accepts Str (and NullLiteral) ───────────────
    // Why: string in → string out. The shape used by `lower()` and `upper()`.

    #[test]
    fn unary_str_to_str_accepts_str() {
        let args_str = [InferredType::Known(FieldType::Str)];
        assert_eq!(
            unary_str_to_str(&args_str).unwrap(),
            InferredType::Known(FieldType::Str)
        );

        // NullLiteral input — primitive allows it, helper should too
        let args_null = [InferredType::NullLiteral];
        assert_eq!(
            unary_str_to_str(&args_null).unwrap(),
            InferredType::Known(FieldType::Str)
        );
    }

    // ── Test 19: unary_str_to_str rejects I64 ─────────────────────────────────
    // Why: can't lowercase a number — error points at the bad arg so users fix the right place.

    #[test]
    fn unary_str_to_str_rejects_i64() {
        let args = [InferredType::Known(FieldType::I64)];
        let r = unary_str_to_str(&args);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));
    }

    // ── Test 20: unary_str_to_i64 accepts Str (and NullLiteral) ───────────────
    // Why: string in → integer out. The shape used by `length()`.

    #[test]
    fn unary_str_to_i64_accepts_str() {
        let args_str = [InferredType::Known(FieldType::Str)];
        assert_eq!(
            unary_str_to_i64(&args_str).unwrap(),
            InferredType::Known(FieldType::I64)
        );

        let args_null = [InferredType::NullLiteral];
        assert_eq!(
            unary_str_to_i64(&args_null).unwrap(),
            InferredType::Known(FieldType::I64)
        );
    }

    // ── Test 21: unary_str_to_i64 rejects I64 ─────────────────────────────────
    // Why: `length(42)` makes no sense. Error tells users they passed the wrong type at arg 0.

    #[test]
    fn unary_str_to_i64_rejects_i64() {
        let args = [InferredType::Known(FieldType::I64)];
        let r = unary_str_to_i64(&args);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));
    }

    // ── Test 22: unary_numeric_same I64 returns I64 ───────────────────────────
    // Why: `abs(int)` should stay int. Don't sneak in a float upgrade.

    #[test]
    fn unary_numeric_same_i64_returns_i64() {
        let args = [InferredType::Known(FieldType::I64)];
        let r = unary_numeric_same(&args);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::I64));
    }

    // ── Test 23: unary_numeric_same F64 returns F64 ───────────────────────────
    // Why: `abs(float)` stays float. Confirms the result tracks the input, not hardcoded to int.

    #[test]
    fn unary_numeric_same_f64_returns_f64() {
        let args = [InferredType::Known(FieldType::F64)];
        let r = unary_numeric_same(&args);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::F64));
    }

    // ── Test 24: unary_numeric_same rejects Str ───────────────────────────────
    // Why: `abs("hello")` is nonsense. Error points at arg 0.

    #[test]
    fn unary_numeric_same_rejects_str() {
        let args = [InferredType::Known(FieldType::Str)];
        let r = unary_numeric_same(&args);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));
    }

    // ── Test 25: unary_numeric_to_f64 I64 returns F64 ─────────────────────────
    // Why: `log(int)` returns float — log always produces a real number, even from an integer.

    #[test]
    fn unary_numeric_to_f64_i64_returns_f64() {
        let args = [InferredType::Known(FieldType::I64)];
        let r = unary_numeric_to_f64(&args);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::F64));
    }

    // ── Test 26: unary_numeric_to_f64 F64 returns F64 ─────────────────────────
    // Why: float input also returns float. The output type is fixed, not input-dependent.

    #[test]
    fn unary_numeric_to_f64_f64_returns_f64() {
        let args = [InferredType::Known(FieldType::F64)];
        let r = unary_numeric_to_f64(&args);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::F64));
    }

    // ── Test 27: unary_numeric_to_f64 rejects Str ─────────────────────────────
    // Why: `log("hi")` is nonsense. Error at arg 0.

    #[test]
    fn unary_numeric_to_f64_rejects_str() {
        let args = [InferredType::Known(FieldType::Str)];
        let r = unary_numeric_to_f64(&args);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));
    }

    // ── Test 28: binary_numeric_to_f64 I64/I64 returns F64 ────────────────────
    // Why: `pow(2, 3)` returns float even though both inputs are int — pow always gives back float.

    #[test]
    fn binary_numeric_to_f64_i64_i64_returns_f64() {
        let args = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
        ];
        let r = binary_numeric_to_f64(&args);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::F64));
    }

    // ── Test 29: binary_numeric_to_f64 F64/I64 returns F64 ────────────────────
    // Why: mixed int/float input → float output. Same as two ints.

    #[test]
    fn binary_numeric_to_f64_f64_i64_returns_f64() {
        let args = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::I64),
        ];
        let r = binary_numeric_to_f64(&args);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::F64));
    }

    // ── Test 30: binary_numeric_to_f64 rejects Str at either arg ──────────────
    // Why: string in either slot fails, and the error names the exact slot (arg 0 or arg 1).

    #[test]
    fn binary_numeric_to_f64_rejects_str() {
        // Str at arg 0
        let args0 = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::I64),
        ];
        assert!(matches!(
            binary_numeric_to_f64(&args0),
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));

        // Str at arg 1
        let args1 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::Str),
        ];
        assert!(matches!(
            binary_numeric_to_f64(&args1),
            Err(InferError::TypeMismatch { arg_idx: 1, .. })
        ));
    }

    // ── Test 31: binary_numeric_to_f64 rejects wrong arity ────────────────────
    // Why: not exactly 2 args → arity error. Confirms the helper checks count, not just types.

    #[test]
    fn binary_numeric_to_f64_rejects_wrong_arity() {
        // Too few: expected 2, got 1
        let args_short = [InferredType::Known(FieldType::I64)];
        assert!(matches!(
            binary_numeric_to_f64(&args_short),
            Err(InferError::Arity {
                expected: 2,
                got: 1
            })
        ));

        // Too many: expected 2, got 3
        let args_long = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
        ];
        assert!(matches!(
            binary_numeric_to_f64(&args_long),
            Err(InferError::Arity {
                expected: 2,
                got: 3
            })
        ));
    }

    // ── Test 32: string_search_to_bool Str/Str returns Bool ───────────────────
    // Why: two strings in → true or false out. The shape used by `contains`, `starts_with`, `ends_with`.

    #[test]
    fn string_search_to_bool_str_str_returns_bool() {
        let args = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
        ];
        let r = string_search_to_bool(&args);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::Bool));
    }

    // ── Test 33: string_search_to_bool rejects I64 at arg0 ────────────────────
    // Why: searching inside a number is nonsense. Error pinpoints arg 0.

    #[test]
    fn string_search_to_bool_rejects_i64_at_arg0() {
        let args = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::Str),
        ];
        let r = string_search_to_bool(&args);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));
    }

    // ── Test 34: string_search_to_bool rejects I64 at arg1 ────────────────────
    // Why: searching for a number inside a string is nonsense. Error pinpoints arg 1.

    #[test]
    fn string_search_to_bool_rejects_i64_at_arg1() {
        let args = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::I64),
        ];
        let r = string_search_to_bool(&args);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 1, .. })
        ));
    }

    // ── Test 35: polymorphic_var0_unify same type ─────────────────────────────
    // Why: `coalesce(int, int, int)` returns int. Tested with several types so result isn't hardcoded.

    #[test]
    fn polymorphic_var0_unify_same_type() {
        // I64
        let args_i64 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
        ];
        assert_eq!(
            polymorphic_var0_unify(&args_i64).unwrap(),
            InferredType::Known(FieldType::I64)
        );

        // F64 (verify identity not hard-coded to I64)
        let args_f64 = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::F64),
        ];
        assert_eq!(
            polymorphic_var0_unify(&args_f64).unwrap(),
            InferredType::Known(FieldType::F64)
        );

        // Str
        let args_str = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
        ];
        assert_eq!(
            polymorphic_var0_unify(&args_str).unwrap(),
            InferredType::Known(FieldType::Str)
        );
    }

    // ── Test 36: polymorphic_var0_unify NullLiteral as hole ───────────────────
    // Why: a `null` in `coalesce` doesn't pick the type — the real-typed arg does.

    #[test]
    fn polymorphic_var0_unify_null_is_hole() {
        // NullLiteral at position 0
        let args0 = [
            InferredType::NullLiteral,
            InferredType::Known(FieldType::F64),
        ];
        assert_eq!(
            polymorphic_var0_unify(&args0).unwrap(),
            InferredType::Known(FieldType::F64)
        );

        // NullLiteral at position 1
        let args1 = [
            InferredType::Known(FieldType::F64),
            InferredType::NullLiteral,
        ];
        assert_eq!(
            polymorphic_var0_unify(&args1).unwrap(),
            InferredType::Known(FieldType::F64)
        );
    }

    // ── Test 37: polymorphic_var0_unify all-null falls back to Str ────────────
    // Why: `coalesce(null, null)` has nothing to pin a type to — default to string.

    #[test]
    fn polymorphic_var0_unify_all_null_falls_back_to_str() {
        let args = [InferredType::NullLiteral, InferredType::NullLiteral];
        let r = polymorphic_var0_unify(&args);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::Str));
    }

    // ── Test 38: polymorphic_var0_unify rejects I64 vs F64 ────────────────────
    // Why: `coalesce(int, float)` mixes types — fail loud. No silent upgrade like `+` does.

    #[test]
    fn polymorphic_var0_unify_rejects_i64_vs_f64() {
        // I64 then F64
        let args0 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::F64),
        ];
        assert!(matches!(
            polymorphic_var0_unify(&args0),
            Err(InferError::Unify { .. })
        ));

        // F64 then I64 (order-independent)
        let args1 = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::I64),
        ];
        assert!(matches!(
            polymorphic_var0_unify(&args1),
            Err(InferError::Unify { .. })
        ));
    }

    // ── Tests 39–43: Additional coverage ──────────────────────────────────────

    // ── Test 39: TypeClass::accepts ───────────────────────────────────────────
    // Why: tests the class predicate directly, so a bug in `accepts` is caught even if no helper is failing.

    #[test]
    fn type_class_accepts_method() {
        // Numeric
        assert!(TypeClass::Numeric.accepts(FieldType::I64));
        assert!(TypeClass::Numeric.accepts(FieldType::F64));
        assert!(!TypeClass::Numeric.accepts(FieldType::Str));
        assert!(!TypeClass::Numeric.accepts(FieldType::Bool));

        // Any
        for ft in [
            FieldType::Str,
            FieldType::I64,
            FieldType::F64,
            FieldType::Bool,
            FieldType::Bytes,
            FieldType::Datetime,
            FieldType::Json,
        ] {
            assert!(TypeClass::Any.accepts(ft), "Any should accept {ft:?}");
        }
    }

    // ── Test 40: require_arg_class rejects arity mismatch ─────────────────────
    // Why: wrong number of args fails here too — arity check happens before class check.

    #[test]
    fn require_arg_class_rejects_arity_mismatch() {
        // Too few: expected 2, got 1
        let args_short = [InferredType::Known(FieldType::I64)];
        assert!(matches!(
            require_arg_class(&args_short, &[TypeClass::Numeric, TypeClass::Numeric]),
            Err(InferError::Arity {
                expected: 2,
                got: 1
            })
        ));

        // Too many: expected 2, got 3
        let args_long = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
        ];
        assert!(matches!(
            require_arg_class(&args_long, &[TypeClass::Numeric, TypeClass::Numeric]),
            Err(InferError::Arity {
                expected: 2,
                got: 3
            })
        ));
    }

    // ── Test 41: unify_var0_with_class rejects strict mismatch under class ────
    // Why: even when both args fit the class (both numeric), mixed int/float must still fail strict equality.

    #[test]
    fn unify_var0_with_class_rejects_strict_mismatch_under_class() {
        // Both args satisfy Numeric but fail strict equality
        let args = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::F64),
        ];
        let r = unify_var0_with_class(&args, &[0, 1], TypeClass::Numeric);
        assert!(matches!(r, Err(InferError::Unify { .. })));
    }

    // ── Test 42: unary_numeric_same propagates NullLiteral ────────────────────
    // Why: this is the only helper that returns the input type as-is. Null in → null out, not a fixed Known(...).

    #[test]
    fn unary_numeric_same_propagates_null_literal() {
        // Identity-preserving helper: NullLiteral in → NullLiteral out.
        // Unique to this helper; the others return a fixed Known(...) type.
        let args = [InferredType::NullLiteral];
        let r = unary_numeric_same(&args);
        assert_eq!(r.unwrap(), InferredType::NullLiteral);
    }

    // ── Test 43: polymorphic_var0_unify variadic (4 and 5 args) ───────────────
    // Why: coalesce takes any number of args. Test with 4 and 5 so we know loops don't stop early.

    #[test]
    fn polymorphic_var0_unify_variadic() {
        // 4 args, all same type
        let args4 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
        ];
        assert_eq!(
            polymorphic_var0_unify(&args4).unwrap(),
            InferredType::Known(FieldType::I64)
        );

        // 5 args, all same type (Str — verify not hardcoded to a numeric type)
        let args5 = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
        ];
        assert_eq!(
            polymorphic_var0_unify(&args5).unwrap(),
            InferredType::Known(FieldType::Str)
        );

        // 5 args with NullLiteral holes interspersed → concrete type
        let args5_holes = [
            InferredType::NullLiteral,
            InferredType::Known(FieldType::F64),
            InferredType::NullLiteral,
            InferredType::Known(FieldType::F64),
            InferredType::NullLiteral,
        ];
        assert_eq!(
            polymorphic_var0_unify(&args5_holes).unwrap(),
            InferredType::Known(FieldType::F64)
        );

        // 5 args with mismatch at idx 4 → Unify err (verify iteration doesn't
        // stop early at idx 2 or 3).
        let args5_late_mismatch = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::F64),
        ];
        assert!(matches!(
            polymorphic_var0_unify(&args5_late_mismatch),
            Err(InferError::Unify { .. })
        ));
    }

    // ── Tests 44–48: NullLiteral wildcard coverage for fixed-output helpers ──
    //
    // The wildcard rule (RFC-001 §5.1, mirrors null-propagating runtime) says
    // NullLiteral is accepted at any arg position. Helpers backed by
    // require_arg_types / require_arg_class inherit this. These tests pin the
    // contract per-helper so a future refactor that tightens any helper's
    // null-handling fails loudly.

    // ── Test 44: unary_numeric_to_f64 accepts NullLiteral ─────────────────────
    // Why: `log(null)` shouldn't be rejected — null is allowed; output is still float.

    #[test]
    fn unary_numeric_to_f64_accepts_null_literal() {
        let args = [InferredType::NullLiteral];
        assert_eq!(
            unary_numeric_to_f64(&args).unwrap(),
            InferredType::Known(FieldType::F64)
        );
    }

    // ── Test 45: binary_numeric_to_f64 accepts NullLiteral at each position ──
    // Why: `pow(null, 3)`, `pow(2, null)`, `pow(null, null)` all allowed — output type fixed at float.

    #[test]
    fn binary_numeric_to_f64_accepts_null_literal() {
        // NullLiteral at arg 0
        let args0 = [
            InferredType::NullLiteral,
            InferredType::Known(FieldType::F64),
        ];
        assert_eq!(
            binary_numeric_to_f64(&args0).unwrap(),
            InferredType::Known(FieldType::F64)
        );

        // NullLiteral at arg 1
        let args1 = [
            InferredType::Known(FieldType::I64),
            InferredType::NullLiteral,
        ];
        assert_eq!(
            binary_numeric_to_f64(&args1).unwrap(),
            InferredType::Known(FieldType::F64)
        );

        // Both NullLiteral
        let args_both = [InferredType::NullLiteral, InferredType::NullLiteral];
        assert_eq!(
            binary_numeric_to_f64(&args_both).unwrap(),
            InferredType::Known(FieldType::F64)
        );
    }

    // ── Test 46: string_search_to_bool accepts NullLiteral at each position ──
    // Why: `contains(null, "x")` etc. shouldn't be rejected at typecheck — null is allowed in either slot.

    #[test]
    fn string_search_to_bool_accepts_null_literal() {
        // NullLiteral at arg 0 (the haystack)
        let args0 = [
            InferredType::NullLiteral,
            InferredType::Known(FieldType::Str),
        ];
        assert_eq!(
            string_search_to_bool(&args0).unwrap(),
            InferredType::Known(FieldType::Bool)
        );

        // NullLiteral at arg 1 (the needle)
        let args1 = [
            InferredType::Known(FieldType::Str),
            InferredType::NullLiteral,
        ];
        assert_eq!(
            string_search_to_bool(&args1).unwrap(),
            InferredType::Known(FieldType::Bool)
        );

        // Both NullLiteral
        let args_both = [InferredType::NullLiteral, InferredType::NullLiteral];
        assert_eq!(
            string_search_to_bool(&args_both).unwrap(),
            InferredType::Known(FieldType::Bool)
        );
    }

    // ── Test 47: any_to_bool accepts every concrete FieldType ─────────────────
    // Why: Test 14 only covered a few types. This walks all remaining types so "Any" really means any.

    #[test]
    fn any_to_bool_accepts_every_concrete_field_type() {
        // Test 14 covers I64, Str, NullLiteral. This covers the remaining
        // FieldType variants to pin "any really means any" for the isnull
        // helper.
        for ft in [
            FieldType::Bool,
            FieldType::F64,
            FieldType::Bytes,
            FieldType::Datetime,
            FieldType::Json,
        ] {
            let args = [InferredType::Known(ft)];
            assert_eq!(
                any_to_bool(&args).unwrap(),
                InferredType::Known(FieldType::Bool),
                "any_to_bool should accept {ft:?}"
            );
        }
    }

    // ── Test 48: polymorphic_var0_unify with zero args ────────────────────────
    // Why: 0 args shouldn't happen in real use (arity check catches it first),
    // but the helper should still behave predictably — fall back to string,
    // same as the all-null case. Pins the contract so callers can rely on it.

    #[test]
    fn polymorphic_var0_unify_zero_args() {
        let args: [InferredType; 0] = [];
        // Documented behavior: empty binding falls back to Str (same as
        // all-null). Alternative would be an InferError; pinning the Ok(Str)
        // contract here means callers can rely on it.
        assert_eq!(
            polymorphic_var0_unify(&args).unwrap(),
            InferredType::Known(FieldType::Str)
        );
    }
}
