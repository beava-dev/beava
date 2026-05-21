//! Type-check helpers shared across builtin functions.
//!
//! Each builtin (e.g. `log`, `contains`, `cast`) has a register-time
//! rule that validates its argument types and returns the result type.
//!
//! This file holds the reusable pieces those rules call into, so each
//! builtin can be a one-liner instead of repeating the same checks.
//!
//! Currently a stub — only the test module is here. The helpers and
//! error types the tests reference land in later steps.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{Expr, Literal, Span};
    use crate::schema::FieldType;
    use crate::schema_propagate::InferredType;

    // ── Fixtures ──────────────────────────────────────────────────────────────

    fn span() -> Span {
        Span { start: 0, end: 0 }
    }

    // ── Tests 1–16: Primitives ────────────────────────────────────────────────

    // ── Test 1: require_arg_types accepts exact match ─────────────────────────

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

    // ── Test 14: read_literal_type_name reads BareIdent ───────────────────────

    #[test]
    fn read_literal_type_name_reads_bare_ident() {
        // At idx 1
        let args1 = [
            Expr::Literal(Literal::Str("ignored".into()), span()),
            Expr::Literal(Literal::BareIdent("float".into()), span()),
        ];
        assert_eq!(read_literal_type_name(&args1, 1).unwrap(), "float");

        // At idx 0
        let args0 = [Expr::Literal(Literal::BareIdent("int".into()), span())];
        assert_eq!(read_literal_type_name(&args0, 0).unwrap(), "int");
    }

    // ── Test 15: read_literal_type_name reads Str literal ─────────────────────

    #[test]
    fn read_literal_type_name_reads_str_literal() {
        // At idx 1
        let args1 = [
            Expr::Literal(Literal::Str("ignored".into()), span()),
            Expr::Literal(Literal::Str("int".into()), span()),
        ];
        assert_eq!(read_literal_type_name(&args1, 1).unwrap(), "int");

        // At idx 0
        let args0 = [Expr::Literal(Literal::Str("float".into()), span())];
        assert_eq!(read_literal_type_name(&args0, 0).unwrap(), "float");
    }

    // ── Test 16: read_literal_type_name rejects non-string literal/non-literal ──

    #[test]
    fn read_literal_type_name_rejects_non_string_literal() {
        // Field (not a literal)
        let args_field = [Expr::Field {
            name: "x".into(),
            span: span(),
        }];
        assert!(matches!(
            read_literal_type_name(&args_field, 0),
            Err(InferError::Custom { .. })
        ));

        // Literal but not string-typed (Int)
        let args_int = [Expr::Literal(Literal::Int(42), span())];
        assert!(matches!(
            read_literal_type_name(&args_int, 0),
            Err(InferError::Custom { .. })
        ));

        // Literal but not string-typed (Bool)
        let args_bool = [Expr::Literal(Literal::Bool(true), span())];
        assert!(matches!(
            read_literal_type_name(&args_bool, 0),
            Err(InferError::Custom { .. })
        ));
    }

    // ── Tests 17–38: Helpers ──────────────────────────────────────────────────

    // ── Test 17: any_to_bool returns Bool for any input ───────────────────────

    #[test]
    fn any_to_bool_returns_bool_for_any_input() {
        let args_i64 = [InferredType::Known(FieldType::I64)];
        assert_eq!(
            any_to_bool(&args_i64, &[]).unwrap(),
            InferredType::Known(FieldType::Bool)
        );

        let args_str = [InferredType::Known(FieldType::Str)];
        assert_eq!(
            any_to_bool(&args_str, &[]).unwrap(),
            InferredType::Known(FieldType::Bool)
        );

        let args_null = [InferredType::NullLiteral];
        assert_eq!(
            any_to_bool(&args_null, &[]).unwrap(),
            InferredType::Known(FieldType::Bool)
        );
    }

    // ── Test 18: unary_str_to_str accepts Str (and NullLiteral) ───────────────

    #[test]
    fn unary_str_to_str_accepts_str() {
        let args_str = [InferredType::Known(FieldType::Str)];
        assert_eq!(
            unary_str_to_str(&args_str, &[]).unwrap(),
            InferredType::Known(FieldType::Str)
        );

        // NullLiteral input — primitive allows it, helper should too
        let args_null = [InferredType::NullLiteral];
        assert_eq!(
            unary_str_to_str(&args_null, &[]).unwrap(),
            InferredType::Known(FieldType::Str)
        );
    }

    // ── Test 19: unary_str_to_str rejects I64 ─────────────────────────────────

    #[test]
    fn unary_str_to_str_rejects_i64() {
        let args = [InferredType::Known(FieldType::I64)];
        let r = unary_str_to_str(&args, &[]);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));
    }

    // ── Test 20: unary_str_to_i64 accepts Str (and NullLiteral) ───────────────

    #[test]
    fn unary_str_to_i64_accepts_str() {
        let args_str = [InferredType::Known(FieldType::Str)];
        assert_eq!(
            unary_str_to_i64(&args_str, &[]).unwrap(),
            InferredType::Known(FieldType::I64)
        );

        let args_null = [InferredType::NullLiteral];
        assert_eq!(
            unary_str_to_i64(&args_null, &[]).unwrap(),
            InferredType::Known(FieldType::I64)
        );
    }

    // ── Test 21: unary_str_to_i64 rejects I64 ─────────────────────────────────

    #[test]
    fn unary_str_to_i64_rejects_i64() {
        let args = [InferredType::Known(FieldType::I64)];
        let r = unary_str_to_i64(&args, &[]);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));
    }

    // ── Test 22: unary_numeric_same I64 returns I64 ───────────────────────────

    #[test]
    fn unary_numeric_same_i64_returns_i64() {
        let args = [InferredType::Known(FieldType::I64)];
        let r = unary_numeric_same(&args, &[]);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::I64));
    }

    // ── Test 23: unary_numeric_same F64 returns F64 ───────────────────────────

    #[test]
    fn unary_numeric_same_f64_returns_f64() {
        let args = [InferredType::Known(FieldType::F64)];
        let r = unary_numeric_same(&args, &[]);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::F64));
    }

    // ── Test 24: unary_numeric_same rejects Str ───────────────────────────────

    #[test]
    fn unary_numeric_same_rejects_str() {
        let args = [InferredType::Known(FieldType::Str)];
        let r = unary_numeric_same(&args, &[]);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));
    }

    // ── Test 25: unary_numeric_to_f64 I64 returns F64 ─────────────────────────

    #[test]
    fn unary_numeric_to_f64_i64_returns_f64() {
        let args = [InferredType::Known(FieldType::I64)];
        let r = unary_numeric_to_f64(&args, &[]);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::F64));
    }

    // ── Test 26: unary_numeric_to_f64 F64 returns F64 ─────────────────────────

    #[test]
    fn unary_numeric_to_f64_f64_returns_f64() {
        let args = [InferredType::Known(FieldType::F64)];
        let r = unary_numeric_to_f64(&args, &[]);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::F64));
    }

    // ── Test 27: unary_numeric_to_f64 rejects Str ─────────────────────────────

    #[test]
    fn unary_numeric_to_f64_rejects_str() {
        let args = [InferredType::Known(FieldType::Str)];
        let r = unary_numeric_to_f64(&args, &[]);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));
    }

    // ── Test 28: binary_numeric_to_f64 I64/I64 returns F64 ────────────────────

    #[test]
    fn binary_numeric_to_f64_i64_i64_returns_f64() {
        let args = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
        ];
        let r = binary_numeric_to_f64(&args, &[]);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::F64));
    }

    // ── Test 29: binary_numeric_to_f64 F64/I64 returns F64 ────────────────────

    #[test]
    fn binary_numeric_to_f64_f64_i64_returns_f64() {
        let args = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::I64),
        ];
        let r = binary_numeric_to_f64(&args, &[]);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::F64));
    }

    // ── Test 30: binary_numeric_to_f64 rejects Str at either arg ──────────────

    #[test]
    fn binary_numeric_to_f64_rejects_str() {
        // Str at arg 0
        let args0 = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::I64),
        ];
        assert!(matches!(
            binary_numeric_to_f64(&args0, &[]),
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));

        // Str at arg 1
        let args1 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::Str),
        ];
        assert!(matches!(
            binary_numeric_to_f64(&args1, &[]),
            Err(InferError::TypeMismatch { arg_idx: 1, .. })
        ));
    }

    // ── Test 31: binary_numeric_to_f64 rejects wrong arity ────────────────────

    #[test]
    fn binary_numeric_to_f64_rejects_wrong_arity() {
        // Too few: expected 2, got 1
        let args_short = [InferredType::Known(FieldType::I64)];
        assert!(matches!(
            binary_numeric_to_f64(&args_short, &[]),
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
            binary_numeric_to_f64(&args_long, &[]),
            Err(InferError::Arity {
                expected: 2,
                got: 3
            })
        ));
    }

    // ── Test 32: string_search_to_bool Str/Str returns Bool ───────────────────

    #[test]
    fn string_search_to_bool_str_str_returns_bool() {
        let args = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
        ];
        let r = string_search_to_bool(&args, &[]);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::Bool));
    }

    // ── Test 33: string_search_to_bool rejects I64 at arg0 ────────────────────

    #[test]
    fn string_search_to_bool_rejects_i64_at_arg0() {
        let args = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::Str),
        ];
        let r = string_search_to_bool(&args, &[]);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 0, .. })
        ));
    }

    // ── Test 34: string_search_to_bool rejects I64 at arg1 ────────────────────

    #[test]
    fn string_search_to_bool_rejects_i64_at_arg1() {
        let args = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::I64),
        ];
        let r = string_search_to_bool(&args, &[]);
        assert!(matches!(
            r,
            Err(InferError::TypeMismatch { arg_idx: 1, .. })
        ));
    }

    // ── Test 35: polymorphic_var0_unify same type ─────────────────────────────

    #[test]
    fn polymorphic_var0_unify_same_type() {
        // I64
        let args_i64 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::I64),
        ];
        assert_eq!(
            polymorphic_var0_unify(&args_i64, &[]).unwrap(),
            InferredType::Known(FieldType::I64)
        );

        // F64 (verify identity not hard-coded to I64)
        let args_f64 = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::F64),
        ];
        assert_eq!(
            polymorphic_var0_unify(&args_f64, &[]).unwrap(),
            InferredType::Known(FieldType::F64)
        );

        // Str
        let args_str = [
            InferredType::Known(FieldType::Str),
            InferredType::Known(FieldType::Str),
        ];
        assert_eq!(
            polymorphic_var0_unify(&args_str, &[]).unwrap(),
            InferredType::Known(FieldType::Str)
        );
    }

    // ── Test 36: polymorphic_var0_unify NullLiteral as hole ───────────────────

    #[test]
    fn polymorphic_var0_unify_null_is_hole() {
        // NullLiteral at position 0
        let args0 = [
            InferredType::NullLiteral,
            InferredType::Known(FieldType::F64),
        ];
        assert_eq!(
            polymorphic_var0_unify(&args0, &[]).unwrap(),
            InferredType::Known(FieldType::F64)
        );

        // NullLiteral at position 1
        let args1 = [
            InferredType::Known(FieldType::F64),
            InferredType::NullLiteral,
        ];
        assert_eq!(
            polymorphic_var0_unify(&args1, &[]).unwrap(),
            InferredType::Known(FieldType::F64)
        );
    }

    // ── Test 37: polymorphic_var0_unify all-null falls back to Str ────────────

    #[test]
    fn polymorphic_var0_unify_all_null_falls_back_to_str() {
        let args = [InferredType::NullLiteral, InferredType::NullLiteral];
        let r = polymorphic_var0_unify(&args, &[]);
        assert_eq!(r.unwrap(), InferredType::Known(FieldType::Str));
    }

    // ── Test 38: polymorphic_var0_unify rejects I64 vs F64 ────────────────────

    #[test]
    fn polymorphic_var0_unify_rejects_i64_vs_f64() {
        // I64 then F64
        let args0 = [
            InferredType::Known(FieldType::I64),
            InferredType::Known(FieldType::F64),
        ];
        assert!(matches!(
            polymorphic_var0_unify(&args0, &[]),
            Err(InferError::Unify { .. })
        ));

        // F64 then I64 (order-independent)
        let args1 = [
            InferredType::Known(FieldType::F64),
            InferredType::Known(FieldType::I64),
        ];
        assert!(matches!(
            polymorphic_var0_unify(&args1, &[]),
            Err(InferError::Unify { .. })
        ));
    }

    // ── Tests 39–43: Additional coverage ──────────────────────────────────────

    // ── Test 39: TypeClass::accepts ───────────────────────────────────────────

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

    #[test]
    fn unary_numeric_same_propagates_null_literal() {
        // Identity-preserving helper: NullLiteral in → NullLiteral out.
        // Unique to this helper; the others return a fixed Known(...) type.
        let args = [InferredType::NullLiteral];
        let r = unary_numeric_same(&args, &[]);
        assert_eq!(r.unwrap(), InferredType::NullLiteral);
    }

    // ── Test 43: polymorphic_var0_unify variadic (4 and 5 args) ───────────────

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
            polymorphic_var0_unify(&args4, &[]).unwrap(),
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
            polymorphic_var0_unify(&args5, &[]).unwrap(),
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
            polymorphic_var0_unify(&args5_holes, &[]).unwrap(),
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
            polymorphic_var0_unify(&args5_late_mismatch, &[]),
            Err(InferError::Unify { .. })
        ));
    }
}
