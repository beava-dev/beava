//! Op-chain executor: `OpChain::compile` + `OpChain::apply`.
//!
//! Compiles a `&[OpNode]` into a sequence of `CompiledOp`s — parsing
//! expression strings once at compile time and caching the `Expr` ASTs —
//! then evaluates the chain per event row via `OpChain::apply(row)`.
//!
//! # Error type
//!
//! Compile errors re-use `PropagationError` (aliased as `CompileError`) so
//! the caller only deals with one error type.
//!
//! # Apply semantics
//!
//! - Filter: `eval(expr, row)` → `Bool(true)` keeps the row; anything else
//!   (Bool(false), Null, or any other Value) drops it (returns `None`).
//! - All other ops return `Some(updated_row)`.
//! - Filter short-circuits: once a Filter drops the row, subsequent ops are
//!   skipped.

use std::collections::BTreeMap;

use crate::eval;
use crate::expr::{self, Expr};
use crate::builtins::BuiltinFn;
use crate::op_node::OpNode;
use crate::row::{Row, Value};
use crate::schema_propagate::{propagate_schema, Schema};

/// Compile-time error — re-exported from `schema_propagate` so callers only
/// import one error type.
pub use crate::schema_propagate::PropagationError as CompileError;

// ─── CastTarget ──────────────────────────────────────────────────────────────

/// Internal target type for Cast (mirrors cast-target string at compile time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CastTarget {
    Str,
    Int,
    Float,
    Bool,
}

impl CastTarget {
    fn as_value_str(self) -> Value {
        match self {
            CastTarget::Str => Value::Str("str".into()),
            CastTarget::Int => Value::Str("int".into()),
            CastTarget::Float => Value::Str("float".into()),
            CastTarget::Bool => Value::Str("bool".into()),
        }
    }
}

fn cast_target_from_str(s: &str) -> Option<CastTarget> {
    match s {
        "str" => Some(CastTarget::Str),
        "int" => Some(CastTarget::Int),
        "float" => Some(CastTarget::Float),
        "bool" => Some(CastTarget::Bool),
        _ => None,
    }
}

// ─── CompiledOp ──────────────────────────────────────────────────────────────

/// A compiled representation of a single op — expression strings have been
/// parsed into `Expr` ASTs; all compile-time checks have passed.
#[derive(Debug, Clone)]
enum CompiledOp {
    Filter(Expr),
    Select(Vec<String>),
    Drop(Vec<String>),
    Rename(BTreeMap<String, String>),
    WithColumns(Vec<(String, Expr)>),
    Cast(Vec<(String, CastTarget)>),
    Fillna(Vec<(String, Value)>),
}

// ─── OpChain ─────────────────────────────────────────────────────────────────

/// A compiled op-chain ready for per-row execution.
#[derive(Debug)]
pub struct OpChain {
    ops: Vec<CompiledOp>,
}

impl OpChain {
    /// Compile `ops` against `input_schema`.
    ///
    /// Calls `propagate_schema` first (validates field refs + type compat).
    /// On success, parses all expression strings and materialises the
    /// `CompiledOp` sequence.
    ///
    /// Returns `(OpChain, output_schema)` or a list of `CompileError`s.
    pub fn compile(
        input_schema: &Schema,
        ops: &[OpNode],
    ) -> Result<(Self, Schema), Vec<CompileError>> {
        // First: schema propagation validates all ops.
        let (final_schema, _per_step) = propagate_schema(input_schema, ops)?;

        // Second: build CompiledOp sequence (expressions already validated).
        let mut compiled: Vec<CompiledOp> = Vec::with_capacity(ops.len());

        for (op_loop_idx, op) in ops.iter().enumerate() {
            let cop = match op {
                OpNode::Filter { expr } => {
                    // parse() should succeed since propagate_schema already validated it.
                    let ast = expr::parse(expr).map_err(|pe| {
                        vec![CompileError::InvalidExpr {
                            op_index: op_loop_idx,
                            parse_error: pe,
                        }]
                    })?;
                    CompiledOp::Filter(ast)
                }

                OpNode::Select { fields } => CompiledOp::Select(fields.clone()),

                OpNode::Drop { fields } => CompiledOp::Drop(fields.clone()),

                OpNode::Rename { mapping } => CompiledOp::Rename(mapping.clone()),

                OpNode::WithColumns { exprs } | OpNode::Map { exprs } => {
                    let mut compiled_exprs: Vec<(String, Expr)> = Vec::new();
                    for (name, expr_src) in exprs {
                        let ast = expr::parse(expr_src).map_err(|pe| {
                            vec![CompileError::InvalidExpr {
                                op_index: op_loop_idx,
                                parse_error: pe,
                            }]
                        })?;
                        compiled_exprs.push((name.clone(), ast));
                    }
                    CompiledOp::WithColumns(compiled_exprs)
                }

                OpNode::Cast { type_map } => {
                    let mut entries: Vec<(String, CastTarget)> = Vec::new();
                    for (field, target_str) in type_map {
                        if let Some(ct) = cast_target_from_str(target_str) {
                            entries.push((field.clone(), ct));
                        }
                        // Invalid targets are already caught by propagate_schema.
                    }
                    CompiledOp::Cast(entries)
                }

                OpNode::Fillna { defaults } => {
                    let entries: Vec<(String, Value)> = defaults
                        .iter()
                        .map(|(k, v)| (k.clone(), json_to_value(v)))
                        .collect();
                    CompiledOp::Fillna(entries)
                }

                // GroupBy is rejected by propagate_schema as UnsupportedOp.
                // If we somehow reach here, skip silently (defensive).
                //
                // Phase 12.7 events-only: OpNode::Join / OpNode::Union variants
                // permanently deleted from the enum.
                OpNode::GroupBy { .. } => continue,
            };
            compiled.push(cop);
        }

        Ok((OpChain { ops: compiled }, final_schema))
    }

    /// Execute the compiled chain against `row`.
    ///
    /// Returns `None` if a Filter drops the row; `Some(updated_row)` otherwise.
    pub fn apply(&self, mut row: Row) -> Option<Row> {
        for op in &self.ops {
            match op {
                CompiledOp::Filter(ast) => {
                    let result = eval::eval(ast, &row);
                    match result {
                        Value::Bool(true) => {
                            // Row passes; continue.
                        }
                        _ => {
                            // Bool(false), Null, or any non-bool → drop row.
                            return None;
                        }
                    }
                }

                CompiledOp::Select(fields) => {
                    // Row.0 is SmallVec — find each requested field, take
                    // its value, build a new Row.
                    let mut new_row = Row::new();
                    for f in fields {
                        if let Some(idx) = row.0.iter().position(|(k, _)| k.as_str() == f.as_str())
                        {
                            let (_, v) = row.0.remove(idx);
                            new_row = new_row.with_field(f, v);
                        }
                    }
                    row = new_row;
                }

                CompiledOp::Drop(fields) => {
                    for f in fields {
                        row = row.without_field(f);
                    }
                }

                CompiledOp::Rename(mapping) => {
                    // Apply all renames atomically: collect values, then insert under new names.
                    let mut renames: Vec<(String, String, Value)> = Vec::new();
                    for (old, new) in mapping {
                        if let Some(idx) =
                            row.0.iter().position(|(k, _)| k.as_str() == old.as_str())
                        {
                            let (_, v) = row.0.remove(idx);
                            renames.push((old.clone(), new.clone(), v));
                        }
                    }
                    for (_old, new, v) in renames {
                        row = row.with_field(&new, v);
                    }
                }

                CompiledOp::WithColumns(exprs) => {
                    for (name, ast) in exprs {
                        let v = eval::eval(ast, &row);
                        row = row.with_field(name, v);
                    }
                }

                CompiledOp::Cast(entries) => {
                    for (field, target) in entries {
                        if let Some(field_val) = row.get(field).cloned() {
                            let args = [field_val, target.as_value_str()];
                            let cast_result = BuiltinFn::Cast.eval(&args);
                            row = row.with_field(field, cast_result);
                        }
                    }
                }

                CompiledOp::Fillna(defaults) => {
                    for (field, default_val) in defaults {
                        if let Some(v) = row.get(field) {
                            if matches!(v, Value::Null) {
                                row = row.with_field(field, default_val.clone());
                            }
                        }
                    }
                }
            }
        }

        Some(row)
    }
}

// ─── JSON → Value conversion ─────────────────────────────────────────────────

fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::I64(i)
            } else if let Some(f) = n.as_f64() {
                Value::F64(f)
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::Str(s.clone().into()),
        _ => Value::Null, // Array / Object → Null (not a scalar)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op_node::OpNode;
    use crate::schema::FieldType;

    // ── Fixtures ──────────────────────────────────────────────────────────────

    mod fixtures {
        use super::*;

        pub fn schema(pairs: &[(&str, FieldType)]) -> Schema {
            let mut fields = BTreeMap::new();
            for (k, v) in pairs {
                fields.insert(k.to_string(), *v);
            }
            Schema {
                fields,
                optional_fields: Vec::new(),
            }
        }

        pub fn schema_opt(pairs: &[(&str, FieldType)], opt: &[&str]) -> Schema {
            let mut s = schema(pairs);
            s.optional_fields = opt.iter().map(|f| f.to_string()).collect();
            s
        }

        pub fn row(pairs: &[(&str, Value)]) -> Row {
            let mut r = Row::new();
            for (k, v) in pairs {
                r = r.with_field(k, v.clone());
            }
            r
        }

        pub fn compile_ok(input: &Schema, ops: Vec<OpNode>) -> (OpChain, Schema) {
            OpChain::compile(input, &ops).expect("expected compile to succeed")
        }

        pub fn compile_err(input: &Schema, ops: Vec<OpNode>) -> Vec<CompileError> {
            OpChain::compile(input, &ops).expect_err("expected compile to fail")
        }
    }

    // ── Test 29: Filter keeps passing row ─────────────────────────────────────

    #[test]
    fn chain_filter_keeps_passing_row() {
        let input = fixtures::schema(&[("amount", FieldType::I64)]);
        let ops = vec![OpNode::Filter {
            expr: "(amount > 100)".to_string(),
        }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[("amount", Value::I64(150))]);
        let result = chain.apply(row);
        assert!(result.is_some(), "row with amount=150 should pass filter");
        let r = result.unwrap();
        assert_eq!(r.get("amount"), Some(&Value::I64(150)));
    }

    // ── Test 30: Filter drops failing row ─────────────────────────────────────

    #[test]
    fn chain_filter_drops_failing_row() {
        let input = fixtures::schema(&[("amount", FieldType::I64)]);
        let ops = vec![OpNode::Filter {
            expr: "(amount > 100)".to_string(),
        }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[("amount", Value::I64(50))]);
        assert_eq!(
            chain.apply(row),
            None,
            "row with amount=50 should be dropped"
        );
    }

    // ── Test 31: Filter null predicate drops row ──────────────────────────────

    #[test]
    fn chain_filter_null_predicate_drops_row() {
        // amount is optional (can be null); filter on null predicate → drop.
        let input = fixtures::schema_opt(&[("amount", FieldType::F64)], &["amount"]);
        let ops = vec![OpNode::Filter {
            expr: "(amount > 0)".to_string(),
        }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[("amount", Value::Null)]);
        assert_eq!(
            chain.apply(row),
            None,
            "null predicate (amount=Null, filter amount > 0) should drop row"
        );
    }

    // ── Test 32: Select keeps listed ──────────────────────────────────────────

    #[test]
    fn chain_select_keeps_listed() {
        let input = fixtures::schema(&[
            ("a", FieldType::I64),
            ("b", FieldType::I64),
            ("c", FieldType::I64),
        ]);
        let ops = vec![OpNode::Select {
            fields: vec!["a".to_string(), "b".to_string()],
        }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[
            ("a", Value::I64(1)),
            ("b", Value::I64(2)),
            ("c", Value::I64(3)),
        ]);
        let result = chain.apply(row).unwrap();
        assert_eq!(result.get("a"), Some(&Value::I64(1)));
        assert_eq!(result.get("b"), Some(&Value::I64(2)));
        assert_eq!(result.get("c"), None, "c should be dropped by select");
    }

    // ── Test 33: Drop removes listed ──────────────────────────────────────────

    #[test]
    fn chain_drop_removes_listed() {
        let input = fixtures::schema(&[
            ("a", FieldType::I64),
            ("b", FieldType::I64),
            ("c", FieldType::I64),
        ]);
        let ops = vec![OpNode::Drop {
            fields: vec!["b".to_string()],
        }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[
            ("a", Value::I64(1)),
            ("b", Value::I64(2)),
            ("c", Value::I64(3)),
        ]);
        let result = chain.apply(row).unwrap();
        assert_eq!(result.get("a"), Some(&Value::I64(1)));
        assert_eq!(result.get("b"), None, "b should be dropped");
        assert_eq!(result.get("c"), Some(&Value::I64(3)));
    }

    // ── Test 34: Rename applies ───────────────────────────────────────────────

    #[test]
    fn chain_rename_applies() {
        let input = fixtures::schema(&[("a", FieldType::I64)]);
        let mut mapping = BTreeMap::new();
        mapping.insert("a".to_string(), "x".to_string());
        let ops = vec![OpNode::Rename { mapping }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[("a", Value::I64(5))]);
        let result = chain.apply(row).unwrap();
        assert_eq!(
            result.get("x"),
            Some(&Value::I64(5)),
            "x should have a's value"
        );
        assert_eq!(result.get("a"), None, "a should be gone after rename");
    }

    // ── Test 35: WithColumns adds field ───────────────────────────────────────

    #[test]
    fn chain_with_columns_adds_field() {
        let input = fixtures::schema(&[("amount", FieldType::I64)]);
        let mut exprs = BTreeMap::new();
        exprs.insert("is_big".to_string(), "(amount > 500)".to_string());
        let ops = vec![OpNode::WithColumns { exprs }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[("amount", Value::I64(1000))]);
        let result = chain.apply(row).unwrap();
        assert_eq!(result.get("amount"), Some(&Value::I64(1000)));
        assert_eq!(
            result.get("is_big"),
            Some(&Value::Bool(true)),
            "1000 > 500 should be true"
        );
    }

    // ── Test 36: Map adds field (alias) ───────────────────────────────────────

    #[test]
    fn chain_map_adds_field_alias() {
        let input = fixtures::schema(&[("amount", FieldType::I64)]);
        let mut exprs = BTreeMap::new();
        exprs.insert("is_big".to_string(), "(amount > 500)".to_string());
        let ops = vec![OpNode::Map { exprs }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[("amount", Value::I64(1000))]);
        let result = chain.apply(row).unwrap();
        assert_eq!(
            result.get("is_big"),
            Some(&Value::Bool(true)),
            "Map with same expr as WithColumns should produce same result"
        );
    }

    // ── Test 37: Cast converts value ──────────────────────────────────────────

    #[test]
    fn chain_cast_converts_value() {
        let input = fixtures::schema(&[("amount", FieldType::Str)]);
        let mut type_map = BTreeMap::new();
        type_map.insert("amount".to_string(), "int".to_string());
        let ops = vec![OpNode::Cast { type_map }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[("amount", Value::Str("42".into()))]);
        let result = chain.apply(row).unwrap();
        assert_eq!(
            result.get("amount"),
            Some(&Value::I64(42)),
            "Str('42') cast to int should produce I64(42)"
        );
    }

    // ── Test 38: Cast failure yields Null ─────────────────────────────────────

    #[test]
    fn chain_cast_failure_yields_null() {
        let input = fixtures::schema(&[("amount", FieldType::Str)]);
        let mut type_map = BTreeMap::new();
        type_map.insert("amount".to_string(), "int".to_string());
        let ops = vec![OpNode::Cast { type_map }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[("amount", Value::Str("abc".into()))]);
        let result = chain.apply(row).unwrap();
        assert_eq!(
            result.get("amount"),
            Some(&Value::Null),
            "Str('abc') cast to int should yield Null (parse failure)"
        );
    }

    // ── Test 39: Fillna replaces Null ─────────────────────────────────────────

    #[test]
    fn chain_fillna_replaces_null() {
        let input = fixtures::schema_opt(&[("amount", FieldType::I64)], &["amount"]);
        let mut defaults = BTreeMap::new();
        defaults.insert("amount".to_string(), serde_json::json!(0));
        let ops = vec![OpNode::Fillna { defaults }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[("amount", Value::Null)]);
        let result = chain.apply(row).unwrap();
        assert_eq!(
            result.get("amount"),
            Some(&Value::I64(0)),
            "Null amount should be replaced with 0"
        );
    }

    // ── Test 40: Fillna leaves non-Null unchanged ─────────────────────────────

    #[test]
    fn chain_fillna_leaves_non_null_unchanged() {
        let input = fixtures::schema(&[("amount", FieldType::I64)]);
        let mut defaults = BTreeMap::new();
        defaults.insert("amount".to_string(), serde_json::json!(0));
        let ops = vec![OpNode::Fillna { defaults }];
        let (chain, _) = fixtures::compile_ok(&input, ops);
        let row = fixtures::row(&[("amount", Value::I64(5))]);
        let result = chain.apply(row).unwrap();
        assert_eq!(
            result.get("amount"),
            Some(&Value::I64(5)),
            "non-Null amount should not be overwritten by fillna"
        );
    }

    // ── Test 41: Compose filter + with_columns + cast ─────────────────────────

    #[test]
    fn chain_composes_filter_with_columns_cast() {
        let input = fixtures::schema(&[("amount", FieldType::F64)]);
        let mut with_exprs = BTreeMap::new();
        with_exprs.insert("is_big".to_string(), "(amount > 500)".to_string());
        let mut cast_map = BTreeMap::new();
        cast_map.insert("is_big".to_string(), "int".to_string());

        let ops = vec![
            OpNode::Filter {
                expr: "(amount > 0)".to_string(),
            },
            OpNode::WithColumns { exprs: with_exprs },
            OpNode::Cast { type_map: cast_map },
        ];

        let (chain, _) = fixtures::compile_ok(&input, ops);

        // Passing row: amount=1000.0 > 0 ✓; is_big = true → cast int = 1
        let row_pass = fixtures::row(&[("amount", Value::F64(1000.0))]);
        let result = chain.apply(row_pass).unwrap();
        assert_eq!(result.get("amount"), Some(&Value::F64(1000.0)));
        assert_eq!(
            result.get("is_big"),
            Some(&Value::I64(1)),
            "Bool(true) cast to int should be I64(1)"
        );

        // Failing row: amount=-1.0, filtered out
        let row_fail = fixtures::row(&[("amount", Value::F64(-1.0))]);
        assert_eq!(
            chain.apply(row_fail),
            None,
            "negative amount should be filtered out"
        );
    }

    // ── Test 42: compile returns output schema ────────────────────────────────

    #[test]
    fn chain_compile_returns_output_schema() {
        let input = fixtures::schema(&[("amount", FieldType::F64)]);
        let mut with_exprs = BTreeMap::new();
        with_exprs.insert("is_big".to_string(), "(amount > 500)".to_string());
        let mut cast_map = BTreeMap::new();
        cast_map.insert("is_big".to_string(), "int".to_string());

        let ops = vec![
            OpNode::Filter {
                expr: "(amount > 0)".to_string(),
            },
            OpNode::WithColumns { exprs: with_exprs },
            OpNode::Cast { type_map: cast_map },
        ];

        let (_chain, output_schema) = fixtures::compile_ok(&input, ops);
        assert_eq!(output_schema.fields.get("amount"), Some(&FieldType::F64));
        assert_eq!(
            output_schema.fields.get("is_big"),
            Some(&FieldType::I64),
            "output schema should reflect cast I64 for is_big"
        );
    }

    // ── Test 43: compile fails on bad expr ────────────────────────────────────

    #[test]
    fn chain_compile_fails_on_bad_expr() {
        let input = fixtures::schema(&[("amount", FieldType::F64)]);
        let ops = vec![OpNode::Filter {
            expr: "(".to_string(),
        }];
        let errs = fixtures::compile_err(&input, ops);
        assert!(!errs.is_empty(), "expected at least one CompileError");
        assert!(
            errs.iter()
                .any(|e| matches!(e, CompileError::InvalidExpr { .. })),
            "expected InvalidExpr error, got {errs:?}"
        );
    }
}
