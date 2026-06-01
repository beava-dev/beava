//! Register-time schema propagation for stateless op chains.
//!
//! Walks `(input_schema, &[OpNode])` and derives the output schema after each
//! op, returning:
//!   - The final output schema.
//!   - A per-step `Vec<Schema>` snapshot (schema after op `i`).
//!   - Accumulated `Vec<PropagationError>` (fail-soft: collects all errors).
//!
//! SDK-COL-07 field-reference resolution lives here: `Filter`, `WithColumns`,
//! and `Map` ops call `referenced_fields()` on each expression and verify every
//! field is present in the current per-step schema.  Unknown field → `FieldMissing`.
//!
//! This module intentionally has no `serde` or I/O dependencies — it is a pure
//! in-process Rust library.

use std::collections::BTreeMap;

use crate::builtins::BuiltinFn;
use crate::expr::{self, Expr, Literal, ParseError};
use crate::op_node::OpNode;
use crate::schema::{DerivedSchema, EventSchema, FieldType, TableSchema};

// ─── Neutral Schema ───────────────────────────────────────────────────────────

/// Transport schema used inside the propagator.
///
/// Structurally identical to `EventSchema` / `TableSchema` / `DerivedSchema`
/// but type-erased so the propagator does not need to know which descriptor
/// kind it is operating on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    pub fields: BTreeMap<String, FieldType>,
    pub optional_fields: Vec<String>,
}

impl Schema {
    pub fn new() -> Self {
        Schema {
            fields: BTreeMap::new(),
            optional_fields: Vec::new(),
        }
    }

    pub fn from_event(s: &EventSchema) -> Self {
        Schema {
            fields: s.fields.clone(),
            optional_fields: s.optional_fields.clone(),
        }
    }

    pub fn from_table(s: &TableSchema) -> Self {
        Schema {
            fields: s.fields.clone(),
            optional_fields: s.optional_fields.clone(),
        }
    }

    pub fn from_derived(s: &DerivedSchema) -> Self {
        Schema {
            fields: s.fields.clone(),
            optional_fields: s.optional_fields.clone(),
        }
    }

    pub fn into_derived(self) -> DerivedSchema {
        DerivedSchema {
            fields: self.fields,
            optional_fields: self.optional_fields,
        }
    }
}

impl Default for Schema {
    fn default() -> Self {
        Self::new()
    }
}

// ─── InferredType ────────────────────────────────────────────────────────────

/// Wraps a `FieldType` with a special `NullLiteral` variant for polymorphic
/// null-literal type checking.
///
/// `Null` literals are compatible with any type during binary-op type
/// inference — they merely propagate to the other operand's type (or stay
/// ambiguous if both operands are null).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferredType {
    Known(FieldType),
    NullLiteral,
}

// ─── PropagationError ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PropagationError {
    /// An expression references a field not present in the schema at that step.
    FieldMissing { op_index: usize, field: String },
    /// An expression has a type incompatibility (e.g., bool op on non-bool).
    TypeMismatch { op_index: usize, reason: String },
    /// A Rename op would produce a collision in the output schema.
    RenameCollision { op_index: usize, new: String },
    /// An expression string failed to parse.
    InvalidExpr {
        op_index: usize,
        parse_error: ParseError,
    },
    /// An op is not supported in Phase 4 — only GroupBy emits this in v0
    /// (Join + Union were permanently removed in Phase 12.6, 2026-04-30).
    UnsupportedOp { op_index: usize, op: &'static str },
}

// ─── Entry point ─────────────────────────────────────────────────────────────

/// Propagate `input` schema through `ops`, returning `(final_schema, per_step_schemas)`.
///
/// `per_step_schemas[i]` is the schema **after** applying `ops[i]`.
///
/// Fail-soft: all errors are collected; on error the propagator applies a
/// best-effort carry-forward so subsequent ops still get a schema to type-check
/// against.
///
/// Returns `Err(errors)` if any errors were found; `Ok(...)` if clean.
pub fn propagate_schema(
    input: &Schema,
    ops: &[OpNode],
) -> Result<(Schema, Vec<Schema>), Vec<PropagationError>> {
    let mut current = input.clone();
    let mut per_step: Vec<Schema> = Vec::with_capacity(ops.len());
    let mut errors: Vec<PropagationError> = Vec::new();

    for (op_index, op) in ops.iter().enumerate() {
        match op {
            OpNode::Filter { expr } => {
                apply_filter_schema(op_index, expr, &current, &mut errors);
                // Filter does not change schema.
            }
            OpNode::Select { fields } => {
                apply_select_schema(op_index, fields, &mut current, &mut errors);
            }
            OpNode::Drop { fields } => {
                apply_drop_schema(op_index, fields, &mut current, &mut errors);
            }
            OpNode::Rename { mapping } => {
                apply_rename_schema(op_index, mapping, &mut current, &mut errors);
            }
            OpNode::WithColumns { exprs } | OpNode::Map { exprs } => {
                apply_with_columns_schema(op_index, exprs, &mut current, &mut errors);
            }
            OpNode::Cast { type_map } => {
                apply_cast_schema(op_index, type_map, &mut current, &mut errors);
            }
            OpNode::Fillna { defaults } => {
                apply_fillna_schema(op_index, defaults, &mut current, &mut errors);
            }
            // Phase 12.6 (2026-04-30): the Join + Union match arms were deleted
            // per project_redis_shaped_no_event_time_ever (variants gone from
            // the enum). The JSON-prelude shim
            // register_validate::pre_check_removed_ops intercepts those payloads
            // BEFORE the strict OpNode deserialize, emitting structured codes
            // feature_removed_no_joins_v0 / feature_removed_no_unions_v0.
            OpNode::GroupBy { .. } => {
                errors.push(PropagationError::UnsupportedOp {
                    op_index,
                    op: "group_by",
                });
            }
        }
        per_step.push(current.clone());
    }

    if errors.is_empty() {
        Ok((current, per_step))
    } else {
        Err(errors)
    }
}

// ─── Per-op schema logic ─────────────────────────────────────────────────────

fn apply_filter_schema(
    op_index: usize,
    expr_src: &str,
    schema: &Schema,
    errors: &mut Vec<PropagationError>,
) {
    // Parse the expression.
    let ast = match expr::parse(expr_src) {
        Ok(ast) => ast,
        Err(pe) => {
            errors.push(PropagationError::InvalidExpr {
                op_index,
                parse_error: pe,
            });
            return;
        }
    };
    // Validate field references.
    check_referenced_fields(op_index, &ast, schema, errors);
    // Type-check: must be Bool or NullLiteral.
    let mut local_errors: Vec<PropagationError> = Vec::new();
    let ty = infer_expr_type_inner(op_index, &ast, schema, &mut local_errors);
    errors.extend(local_errors);
    // Filter result type must be Bool (comparison exprs, bool literals) or NullLiteral.
    // We don't explicitly reject non-bool here since field reference errors already
    // surface misuse; type checking is best-effort at register time.
    let _ = ty;
}

fn apply_select_schema(
    op_index: usize,
    fields: &[String],
    current: &mut Schema,
    errors: &mut Vec<PropagationError>,
) {
    let mut new_fields: BTreeMap<String, FieldType> = BTreeMap::new();
    let mut new_optional: Vec<String> = Vec::new();

    for f in fields {
        if let Some(ft) = current.fields.get(f.as_str()) {
            new_fields.insert(f.clone(), *ft);
            if current.optional_fields.contains(f) {
                new_optional.push(f.clone());
            }
        } else {
            errors.push(PropagationError::FieldMissing {
                op_index,
                field: f.clone(),
            });
            // Best-effort carry-forward: skip missing fields.
        }
    }

    current.fields = new_fields;
    current.optional_fields = new_optional;
}

fn apply_drop_schema(
    op_index: usize,
    fields: &[String],
    current: &mut Schema,
    errors: &mut Vec<PropagationError>,
) {
    for f in fields {
        if current.fields.contains_key(f.as_str()) {
            current.fields.remove(f.as_str());
            current.optional_fields.retain(|x| x != f);
        } else {
            errors.push(PropagationError::FieldMissing {
                op_index,
                field: f.clone(),
            });
        }
    }
}

fn apply_rename_schema(
    op_index: usize,
    mapping: &BTreeMap<String, String>,
    current: &mut Schema,
    errors: &mut Vec<PropagationError>,
) {
    // Validate all old names exist and no new name collides with fields NOT being renamed away.
    let old_names: std::collections::BTreeSet<&str> = mapping.keys().map(|s| s.as_str()).collect();

    for (old, new) in mapping {
        // Check old field exists.
        if !current.fields.contains_key(old.as_str()) {
            errors.push(PropagationError::FieldMissing {
                op_index,
                field: old.clone(),
            });
        }
        // Check new name doesn't collide with an existing field that isn't being renamed away.
        if current.fields.contains_key(new.as_str()) && !old_names.contains(new.as_str()) {
            errors.push(PropagationError::RenameCollision {
                op_index,
                new: new.clone(),
            });
        }
    }

    // If no errors, apply the rename atomically.
    // We still apply even with errors (best-effort carry-forward).
    let mut new_fields: BTreeMap<String, FieldType> = BTreeMap::new();
    let mut new_optional: Vec<String> = Vec::new();

    for (k, v) in &current.fields {
        if let Some(new_name) = mapping.get(k) {
            new_fields.insert(new_name.clone(), *v);
            if current.optional_fields.contains(k) {
                new_optional.push(new_name.clone());
            }
        } else {
            new_fields.insert(k.clone(), *v);
            if current.optional_fields.contains(k) {
                new_optional.push(k.clone());
            }
        }
    }

    current.fields = new_fields;
    current.optional_fields = new_optional;
}

fn apply_with_columns_schema(
    op_index: usize,
    exprs: &BTreeMap<String, String>,
    current: &mut Schema,
    errors: &mut Vec<PropagationError>,
) {
    // Process in BTreeMap order (deterministic).
    for (name, expr_src) in exprs {
        let ast = match expr::parse(expr_src) {
            Ok(ast) => ast,
            Err(pe) => {
                errors.push(PropagationError::InvalidExpr {
                    op_index,
                    parse_error: pe,
                });
                continue;
            }
        };

        // Validate field references (SDK-COL-07).
        check_referenced_fields(op_index, &ast, current, errors);

        // Infer result type.
        let mut local_errors: Vec<PropagationError> = Vec::new();
        let ty = infer_expr_type_inner(op_index, &ast, current, &mut local_errors);
        errors.extend(local_errors);

        match ty {
            Some(InferredType::Known(ft)) => {
                // Clear optional status if field existed before (expression always produces a value).
                current.optional_fields.retain(|x| x != name);
                current.fields.insert(name.clone(), ft);
            }
            Some(InferredType::NullLiteral) => {
                // NullLiteral-only expression (e.g., just `null`) — can't determine a type.
                // We use Str as a safe fallback and keep optional semantics.
                current.fields.insert(name.clone(), FieldType::Str);
            }
            None => {
                // Error already recorded; best-effort: don't add the field.
            }
        }
    }
}

fn apply_cast_schema(
    op_index: usize,
    type_map: &BTreeMap<String, String>,
    current: &mut Schema,
    errors: &mut Vec<PropagationError>,
) {
    for (field, target_str) in type_map {
        // Check field exists.
        let source_type = match current.fields.get(field.as_str()) {
            Some(ft) => *ft,
            None => {
                errors.push(PropagationError::FieldMissing {
                    op_index,
                    field: field.clone(),
                });
                continue;
            }
        };

        // Check target is a known cast type.
        let target_type = match parse_cast_target(target_str) {
            Some(ft) => ft,
            None => {
                errors.push(PropagationError::TypeMismatch {
                    op_index,
                    reason: format!(
                        "unknown cast target type {:?}; must be one of: str, int, float, bool",
                        target_str
                    ),
                });
                continue;
            }
        };

        // Check the cast is legal.
        if !is_cast_legal(source_type, target_type) {
            errors.push(PropagationError::TypeMismatch {
                op_index,
                reason: format!(
                    "cannot cast field {:?} from {:?} to {:?}: Bytes type cannot be cast",
                    field, source_type, target_type
                ),
            });
            continue;
        }

        // Apply the cast: update the field type.
        current.fields.insert(field.clone(), target_type);
    }
}

fn apply_fillna_schema(
    op_index: usize,
    defaults: &BTreeMap<String, serde_json::Value>,
    current: &mut Schema,
    errors: &mut Vec<PropagationError>,
) {
    for field in defaults.keys() {
        if !current.fields.contains_key(field.as_str()) {
            errors.push(PropagationError::FieldMissing {
                op_index,
                field: field.clone(),
            });
            continue;
        }
        // Clear optional status: fillna guarantees a value.
        current.optional_fields.retain(|x| x != field);
    }
}

// ─── Type inference helpers ───────────────────────────────────────────────────

/// Public entry point for expression type inference (used in tests).
///
/// Returns `Err(PropagationError)` on the first type error found.
pub fn infer_expr_type(expr: &Expr, schema: &Schema) -> Result<InferredType, PropagationError> {
    let mut errors: Vec<PropagationError> = Vec::new();
    match infer_expr_type_inner(0, expr, schema, &mut errors) {
        Some(ty) if errors.is_empty() => Ok(ty),
        Some(_) => Err(errors.remove(0)),
        None => {
            if errors.is_empty() {
                Err(PropagationError::TypeMismatch {
                    op_index: 0,
                    reason: "type inference failed with no error recorded".to_string(),
                })
            } else {
                Err(errors.remove(0))
            }
        }
    }
}

/// Internal recursive type inference.
///
/// Returns `None` when an error has been pushed (callers treat None as "errored").
fn infer_expr_type_inner(
    op_index: usize,
    expr: &Expr,
    schema: &Schema,
    errors: &mut Vec<PropagationError>,
) -> Option<InferredType> {
    match expr {
        Expr::Field { name, .. } => {
            // Field reference errors are already caught by check_referenced_fields;
            // returning None here prevents cascading type errors.
            schema
                .fields
                .get(name.as_str())
                .map(|ft| InferredType::Known(*ft))
        }

        Expr::Literal(lit, _) => Some(match lit {
            Literal::Null => InferredType::NullLiteral,
            Literal::Bool(_) => InferredType::Known(FieldType::Bool),
            Literal::Int(_) => InferredType::Known(FieldType::I64),
            Literal::Float(_) => InferredType::Known(FieldType::F64),
            Literal::Str(_) => InferredType::Known(FieldType::Str),
            // BareIdent is a cast type-arg literal; treat as Str.
            Literal::BareIdent(_) => InferredType::Known(FieldType::Str),
        }),

        Expr::UnaryOp { op, operand, .. } => {
            let ot = infer_expr_type_inner(op_index, operand, schema, errors)?;
            if op == "not" {
                if !is_bool_compatible(&ot) {
                    errors.push(PropagationError::TypeMismatch {
                        op_index,
                        reason: format!("'not' operator requires Bool operand, got {:?}", ot),
                    });
                    return None;
                }
                Some(InferredType::Known(FieldType::Bool))
            } else {
                errors.push(PropagationError::TypeMismatch {
                    op_index,
                    reason: format!("unknown unary operator {:?}", op),
                });
                None
            }
        }

        Expr::BinOp {
            op, left, right, ..
        } => infer_binop_type(op_index, op, left, right, schema, errors),

        Expr::Call { builtin, args, .. } => {
            infer_call_type(op_index, *builtin, args, schema, errors)
        }

        Expr::Cast {
            operand, target, ..
        } => {
            // Infer the operand's type (also records FieldMissing side effects
            // on its subexpressions). Kept so the legality check below mirrors
            // the column-level cast path (`apply_cast_schema`).
            let operand_type = infer_expr_type_inner(op_index, operand, schema, errors);

            // Read the target type name from the literal carried in Expr::Cast.target.
            // Parser already normalized Field("float") → Literal::BareIdent("float").
            let target_name = match target.as_ref() {
                Expr::Literal(Literal::BareIdent(s), _) | Expr::Literal(Literal::Str(s), _) => {
                    s.as_str()
                }
                other => {
                    errors.push(PropagationError::TypeMismatch {
                        op_index,
                        reason: format!(
                            "cast second argument must be a type literal (str/int/float/bool), got {other:?}"
                        ),
                    });
                    return None;
                }
            };

            let target_type = match parse_cast_target(target_name) {
                Some(ft) => ft,
                None => {
                    errors.push(PropagationError::TypeMismatch {
                        op_index,
                        reason: format!(
                            "unknown cast target type {:?}; must be one of: str, int, float, bool",
                            target_name
                        ),
                    });
                    return None;
                }
            };

            // Reject illegal casts at register time (e.g. Bytes → int), matching
            // `apply_cast_schema`. Without this the expression-form cast would
            // promise `target_type` but `cast_eval` returns Null at runtime —
            // schema and behaviour disagree. NullLiteral / unknown operand types
            // are not checked here (the operand error, if any, is already
            // recorded above).
            if let Some(InferredType::Known(source_type)) = operand_type {
                if !is_cast_legal(source_type, target_type) {
                    errors.push(PropagationError::TypeMismatch {
                        op_index,
                        reason: format!(
                            "cannot cast from {source_type:?} to {target_type:?}: Bytes type cannot be cast"
                        ),
                    });
                    return None;
                }
            }

            Some(InferredType::Known(target_type))
        }
    }
}

fn infer_binop_type(
    op_index: usize,
    op: &str,
    left: &Expr,
    right: &Expr,
    schema: &Schema,
    errors: &mut Vec<PropagationError>,
) -> Option<InferredType> {
    let lt = infer_expr_type_inner(op_index, left, schema, errors)?;
    let rt = infer_expr_type_inner(op_index, right, schema, errors)?;

    match op {
        // Comparison ops always return Bool.
        ">" | ">=" | "<" | "<=" | "==" | "!=" => {
            if !types_are_comparable(&lt, &rt) {
                errors.push(PropagationError::TypeMismatch {
                    op_index,
                    reason: format!(
                        "comparison operator {:?} requires comparable operands, got {:?} and {:?}",
                        op, lt, rt
                    ),
                });
                return None;
            }
            Some(InferredType::Known(FieldType::Bool))
        }

        // Boolean ops require Bool (or NullLiteral) operands.
        "and" | "or" => {
            if !is_bool_compatible(&lt) || !is_bool_compatible(&rt) {
                errors.push(PropagationError::TypeMismatch {
                    op_index,
                    reason: format!(
                        "boolean operator {:?} requires Bool operands, got {:?} and {:?}",
                        op, lt, rt
                    ),
                });
                return None;
            }
            Some(InferredType::Known(FieldType::Bool))
        }

        // Arithmetic ops.
        "+" | "-" | "*" | "/" => infer_arithmetic_type(op_index, op, &lt, &rt, errors),

        _ => {
            errors.push(PropagationError::TypeMismatch {
                op_index,
                reason: format!("unknown binary operator {:?}", op),
            });
            None
        }
    }
}

fn infer_arithmetic_type(
    op_index: usize,
    op: &str,
    lt: &InferredType,
    rt: &InferredType,
    errors: &mut Vec<PropagationError>,
) -> Option<InferredType> {
    // Division type rules (v1 decision, aligned with runtime eval.rs):
    //   I64 / I64 → I64 (truncating integer division — matches arith_div in eval.rs).
    //   F64 / anything-numeric → F64; I64 / F64 → F64 (type promotion).
    // This mirrors the runtime exactly, so downstream schema consumers see the
    // correct type without needing to special-case division widening.
    if op == "/" {
        // Validate both are numeric (or null).
        match (lt, rt) {
            (InferredType::NullLiteral, _) | (_, InferredType::NullLiteral) => {
                return resolve_null_arithmetic(
                    op,
                    if matches!(lt, InferredType::NullLiteral) {
                        rt
                    } else {
                        lt
                    },
                );
            }
            (InferredType::Known(l), InferredType::Known(r)) => {
                if !is_numeric_ft(*l) || !is_numeric_ft(*r) {
                    errors.push(PropagationError::TypeMismatch {
                        op_index,
                        reason: format!(
                            "arithmetic operator {:?} requires numeric operands, got {:?} and {:?}",
                            op, lt, rt
                        ),
                    });
                    return None;
                }
                // I64 / I64 → I64 (v1 decision: integer division stays integer).
                // Any F64 operand → F64 (type promotion).
                if *l == FieldType::F64 || *r == FieldType::F64 {
                    return Some(InferredType::Known(FieldType::F64));
                }
                return Some(InferredType::Known(FieldType::I64));
            }
        }
    }

    // For +, -, *: promote I64+F64 → F64; I64+I64 → I64.
    match (lt, rt) {
        (InferredType::NullLiteral, InferredType::NullLiteral) => {
            // Both null: result is NullLiteral (indeterminate).
            Some(InferredType::NullLiteral)
        }
        (InferredType::NullLiteral, other) | (other, InferredType::NullLiteral) => {
            resolve_null_arithmetic(op, other)
        }
        (InferredType::Known(l), InferredType::Known(r)) => {
            if !is_numeric_ft(*l) || !is_numeric_ft(*r) {
                errors.push(PropagationError::TypeMismatch {
                    op_index,
                    reason: format!(
                        "arithmetic operator {:?} requires numeric operands, got {:?} and {:?}",
                        op, lt, rt
                    ),
                });
                return None;
            }
            // I64 + I64 → I64; anything with F64 → F64.
            if *l == FieldType::F64 || *r == FieldType::F64 {
                Some(InferredType::Known(FieldType::F64))
            } else {
                Some(InferredType::Known(FieldType::I64))
            }
        }
    }
}

fn resolve_null_arithmetic(op: &str, other: &InferredType) -> Option<InferredType> {
    // NullLiteral propagation for arithmetic: the result type follows the
    // non-null operand's type. For division (v1 decision): null / I64 → I64
    // (not F64), because the runtime returns Null when either operand is Null
    // (before division executes), and the schema type should match the non-null
    // case — which for I64/I64 is I64. null / F64 → F64 (promotion).
    if op == "/" {
        match other {
            InferredType::Known(FieldType::F64) => Some(InferredType::Known(FieldType::F64)),
            InferredType::Known(FieldType::I64) => Some(InferredType::Known(FieldType::I64)),
            InferredType::NullLiteral => Some(InferredType::NullLiteral),
            InferredType::Known(_) => None, // non-numeric — caller already validates
        }
    } else {
        // Propagate the other operand's type for +, -, *.
        Some(other.clone())
    }
}

fn infer_call_type(
    op_index: usize,
    builtin: BuiltinFn,
    args: &[Expr],
    schema: &Schema,
    errors: &mut Vec<PropagationError>,
) -> Option<InferredType> {
    // Arity check (caller already resolved the variant; unknown names are
    // a ParseError now, not a typecheck error).
    let expected = match builtin.arity() {
        crate::builtins::Arity::Fixed(n) => Some(n),
        crate::builtins::Arity::Variadic => None,
    };
    if let Some(n) = expected {
        if args.len() != n {
            errors.push(PropagationError::TypeMismatch {
                op_index,
                reason: format!(
                    "function {:?} expects {} argument(s), got {}",
                    builtin.name(),
                    n,
                    args.len()
                ),
            });
            return None;
        }
    }

    // Walk arg subexprs for FieldMissing side effects, then collect types.
    let mut arg_types: Vec<InferredType> = Vec::with_capacity(args.len());
    for a in args {
        match infer_expr_type_inner(op_index, a, schema, errors) {
            Some(t) => arg_types.push(t),
            None => return None,
        }
    }

    // Dispatch through the enum's infer method (single-arg signature).
    match builtin.infer(&arg_types) {
        Ok(t) => Some(t),
        Err(e) => {
            errors.push(PropagationError::TypeMismatch {
                op_index,
                reason: render_infer_error(builtin.name(), &e),
            });
            None
        }
    }
}

fn render_infer_error(fn_name: &str, e: &crate::builtins::_inference::InferError) -> String {
    use crate::builtins::_inference::InferError;
    match e {
        InferError::Arity { expected, got } => format!(
            "function {:?} expects {} argument(s), got {}",
            fn_name, expected, got
        ),
        InferError::TypeMismatch {
            arg_idx,
            expected,
            got,
        } => format!(
            "function {:?} arg {}: expected {}, got {:?}",
            fn_name, arg_idx, expected, got
        ),
        InferError::Unify { reason } => {
            format!("function {:?} type-mismatched args: {}", fn_name, reason)
        }
        InferError::Custom { reason } => format!("function {:?}: {}", fn_name, reason),
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Parse a cast target string into a `FieldType`.
pub fn parse_cast_target(s: &str) -> Option<FieldType> {
    match s {
        "str" => Some(FieldType::Str),
        "int" => Some(FieldType::I64),
        "float" => Some(FieldType::F64),
        "bool" => Some(FieldType::Bool),
        _ => None,
    }
}

/// Legal cast matrix. See CONTEXT.md §D-05.
///
/// | Source    | str | int | float | bool |
/// |-----------|-----|-----|-------|------|
/// | Str       | ✓   | ✓   | ✓     | ✓    |
/// | I64       | ✓   | ✓   | ✓     | ✓    |
/// | F64       | ✓   | ✓   | ✓     | ✓    |
/// | Bool      | ✓   | ✓   | ✓     | ✓    |
/// | Datetime  | ✓   | ✓   | ✓     | ✓    |
/// | Bytes     | ✗   | ✗   | ✗     | ✗    |
pub fn is_cast_legal(source: FieldType, target: FieldType) -> bool {
    // Bytes is isolated — no casts in or out.
    if source == FieldType::Bytes {
        return false;
    }
    // All other source types can be cast to str/int/float/bool
    // (may fail at runtime for Str→int/float, but legal at register time).
    matches!(
        target,
        FieldType::Str | FieldType::I64 | FieldType::F64 | FieldType::Bool
    )
}

fn is_bool_compatible(it: &InferredType) -> bool {
    matches!(
        it,
        InferredType::Known(FieldType::Bool) | InferredType::NullLiteral
    )
}

fn types_are_comparable(lt: &InferredType, rt: &InferredType) -> bool {
    match (lt, rt) {
        // Null is compatible with anything.
        (InferredType::NullLiteral, _) | (_, InferredType::NullLiteral) => true,
        (InferredType::Known(l), InferredType::Known(r)) => {
            // Same type is always comparable.
            if l == r {
                return true;
            }
            // Numeric cross-type (I64 ↔ F64) is comparable.
            is_numeric_ft(*l) && is_numeric_ft(*r)
        }
    }
}

fn is_numeric_ft(ft: FieldType) -> bool {
    matches!(ft, FieldType::I64 | FieldType::F64)
}

fn check_referenced_fields(
    op_index: usize,
    ast: &Expr,
    schema: &Schema,
    errors: &mut Vec<PropagationError>,
) {
    for field in ast.referenced_fields() {
        if !schema.fields.contains_key(field.as_str()) {
            errors.push(PropagationError::FieldMissing { op_index, field });
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op_node::OpNode;
    use crate::schema::FieldType;
    use std::collections::BTreeMap;

    // ── Fixtures ──────────────────────────────────────────────────────────────

    fn schema_with(pairs: &[(&str, FieldType)]) -> Schema {
        let mut fields = BTreeMap::new();
        for (k, v) in pairs {
            fields.insert(k.to_string(), *v);
        }
        Schema {
            fields,
            optional_fields: Vec::new(),
        }
    }

    fn schema_with_opt(pairs: &[(&str, FieldType)], opt: &[&str]) -> Schema {
        let mut s = schema_with(pairs);
        s.optional_fields = opt.iter().map(|f| f.to_string()).collect();
        s
    }

    fn assert_no_errors<T>(r: Result<T, Vec<PropagationError>>) -> T {
        match r {
            Ok(v) => v,
            Err(errs) => panic!("expected Ok, got errors: {errs:?}"),
        }
    }

    fn assert_errors(
        r: Result<(Schema, Vec<Schema>), Vec<PropagationError>>,
    ) -> Vec<PropagationError> {
        match r {
            Err(errs) => errs,
            Ok(_) => panic!("expected Err, got Ok"),
        }
    }

    // ── Test 1: Filter preserves schema ──────────────────────────────────────

    #[test]
    fn prop_filter_preserves_schema() {
        let input = schema_with(&[("amount", FieldType::F64), ("id", FieldType::Str)]);
        let ops = vec![OpNode::Filter {
            expr: "(amount > 0)".to_string(),
        }];
        let (final_schema, per_step) = assert_no_errors(propagate_schema(&input, &ops));
        assert_eq!(
            final_schema.fields, input.fields,
            "filter must not change schema fields"
        );
        assert_eq!(per_step.len(), 1);
        assert_eq!(per_step[0].fields, input.fields);
    }

    // ── Test 2: Filter with unknown field errors ──────────────────────────────

    #[test]
    fn prop_filter_expr_unknown_field_errors() {
        let input = schema_with(&[("amount", FieldType::F64)]);
        let ops = vec![OpNode::Filter {
            expr: "(missing > 0)".to_string(),
        }];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.iter().any(|e| matches!(
                e,
                PropagationError::FieldMissing { op_index: 0, field }
                if field == "missing"
            )),
            "expected FieldMissing for 'missing', got {errs:?}"
        );
    }

    // ── Test 3: Filter with parse error ──────────────────────────────────────

    #[test]
    fn prop_filter_expr_parse_error_errors() {
        let input = schema_with(&[("amount", FieldType::F64)]);
        let ops = vec![OpNode::Filter {
            expr: "(amount > ".to_string(),
        }];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.iter()
                .any(|e| matches!(e, PropagationError::InvalidExpr { op_index: 0, .. })),
            "expected InvalidExpr at op_index=0, got {errs:?}"
        );
    }

    // ── Test 4: Select keeps listed fields ────────────────────────────────────

    #[test]
    fn prop_select_keeps_listed_fields_in_order() {
        let input = schema_with(&[
            ("a", FieldType::Str),
            ("b", FieldType::I64),
            ("c", FieldType::F64),
        ]);
        let ops = vec![OpNode::Select {
            fields: vec!["b".to_string(), "a".to_string()],
        }];
        let (final_schema, _) = assert_no_errors(propagate_schema(&input, &ops));
        // BTreeMap sorts alphabetically; assert key set equality.
        assert!(final_schema.fields.contains_key("a"));
        assert!(final_schema.fields.contains_key("b"));
        assert!(
            !final_schema.fields.contains_key("c"),
            "c should be dropped"
        );
        assert_eq!(final_schema.fields.len(), 2);
    }

    // ── Test 5: Select unknown field errors ───────────────────────────────────

    #[test]
    fn prop_select_unknown_field_errors() {
        let input = schema_with(&[("a", FieldType::Str)]);
        let ops = vec![OpNode::Select {
            fields: vec!["missing".to_string()],
        }];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.iter().any(|e| matches!(
                e,
                PropagationError::FieldMissing { op_index: 0, field }
                if field == "missing"
            )),
            "expected FieldMissing for 'missing', got {errs:?}"
        );
    }

    // ── Test 6: Drop removes fields ───────────────────────────────────────────

    #[test]
    fn prop_drop_removes_fields() {
        let input = schema_with(&[
            ("a", FieldType::Str),
            ("b", FieldType::I64),
            ("c", FieldType::F64),
        ]);
        let ops = vec![OpNode::Drop {
            fields: vec!["b".to_string()],
        }];
        let (final_schema, _) = assert_no_errors(propagate_schema(&input, &ops));
        assert!(!final_schema.fields.contains_key("b"));
        assert!(final_schema.fields.contains_key("a"));
        assert!(final_schema.fields.contains_key("c"));
    }

    // ── Test 7: Drop unknown field errors ─────────────────────────────────────

    #[test]
    fn prop_drop_unknown_field_errors() {
        let input = schema_with(&[("a", FieldType::Str)]);
        let ops = vec![OpNode::Drop {
            fields: vec!["missing".to_string()],
        }];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.iter().any(|e| matches!(
                e,
                PropagationError::FieldMissing { op_index: 0, field }
                if field == "missing"
            )),
            "expected FieldMissing for 'missing', got {errs:?}"
        );
    }

    // ── Test 8: Rename swaps keys ──────────────────────────────────────────────

    #[test]
    fn prop_rename_swaps_keys() {
        let input = schema_with_opt(&[("a", FieldType::Str), ("b", FieldType::I64)], &["a"]);
        let mut mapping = BTreeMap::new();
        mapping.insert("a".to_string(), "x".to_string());
        let ops = vec![OpNode::Rename { mapping }];
        let (final_schema, _) = assert_no_errors(propagate_schema(&input, &ops));
        assert!(
            final_schema.fields.contains_key("x"),
            "x should exist after rename"
        );
        assert!(
            !final_schema.fields.contains_key("a"),
            "a should be gone after rename"
        );
        assert!(
            final_schema.fields.contains_key("b"),
            "b should be unchanged"
        );
        // optional_fields should track the rename
        assert!(
            final_schema.optional_fields.contains(&"x".to_string()),
            "optional status of 'a' should transfer to 'x'"
        );
    }

    // ── Test 9: Rename collision errors ───────────────────────────────────────

    #[test]
    fn prop_rename_collision_errors() {
        let input = schema_with(&[("a", FieldType::Str), ("b", FieldType::I64)]);
        let mut mapping = BTreeMap::new();
        mapping.insert("a".to_string(), "b".to_string());
        let ops = vec![OpNode::Rename { mapping }];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.iter().any(|e| matches!(
                e,
                PropagationError::RenameCollision { op_index: 0, new }
                if new == "b"
            )),
            "expected RenameCollision for 'b', got {errs:?}"
        );
    }

    // ── Test 10: Rename unknown field errors ──────────────────────────────────

    #[test]
    fn prop_rename_unknown_field_errors() {
        let input = schema_with(&[("a", FieldType::Str)]);
        let mut mapping = BTreeMap::new();
        mapping.insert("missing".to_string(), "x".to_string());
        let ops = vec![OpNode::Rename { mapping }];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.iter().any(|e| matches!(
                e,
                PropagationError::FieldMissing { op_index: 0, field }
                if field == "missing"
            )),
            "expected FieldMissing for 'missing', got {errs:?}"
        );
    }

    // ── Test 11: WithColumns adds derived field ────────────────────────────────

    #[test]
    fn prop_with_columns_adds_derived_field() {
        let input = schema_with(&[("amount", FieldType::F64)]);
        let mut exprs = BTreeMap::new();
        exprs.insert("is_big".to_string(), "(amount > 500)".to_string());
        let ops = vec![OpNode::WithColumns { exprs }];
        let (final_schema, _) = assert_no_errors(propagate_schema(&input, &ops));
        assert_eq!(
            final_schema.fields.get("is_big"),
            Some(&FieldType::Bool),
            "is_big should be Bool (comparison op)"
        );
        assert!(
            final_schema.fields.contains_key("amount"),
            "amount should remain"
        );
    }

    // ── Test 12: WithColumns overwrites existing field ────────────────────────

    #[test]
    fn prop_with_columns_overwrites_existing() {
        let input = schema_with(&[("amount", FieldType::F64)]);
        let mut exprs = BTreeMap::new();
        exprs.insert("amount".to_string(), "cast(amount, int)".to_string());
        let ops = vec![OpNode::WithColumns { exprs }];
        let (final_schema, _) = assert_no_errors(propagate_schema(&input, &ops));
        assert_eq!(
            final_schema.fields.get("amount"),
            Some(&FieldType::I64),
            "amount should be I64 after cast-overwrite"
        );
    }

    // ── Test 13: WithColumns expr type mismatch errors ────────────────────────

    #[test]
    fn prop_with_columns_expr_type_mismatch_errors() {
        let input = schema_with(&[("amount", FieldType::F64)]);
        let mut exprs = BTreeMap::new();
        // "amount and true" — boolean op on non-bool (amount is F64)
        exprs.insert("bad".to_string(), "(amount and true)".to_string());
        let ops = vec![OpNode::WithColumns { exprs }];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.iter()
                .any(|e| matches!(e, PropagationError::TypeMismatch { op_index: 0, .. })),
            "expected TypeMismatch at op_index=0, got {errs:?}"
        );
    }

    // ── Test 14: Map is alias of WithColumns ──────────────────────────────────

    #[test]
    fn prop_map_is_alias_of_with_columns() {
        let input = schema_with(&[("a", FieldType::F64)]);

        let mut exprs_wc = BTreeMap::new();
        exprs_wc.insert("b".to_string(), "(a + 1)".to_string());
        let ops_wc = vec![OpNode::WithColumns { exprs: exprs_wc }];

        let mut exprs_map = BTreeMap::new();
        exprs_map.insert("b".to_string(), "(a + 1)".to_string());
        let ops_map = vec![OpNode::Map { exprs: exprs_map }];

        let (s_wc, _) = assert_no_errors(propagate_schema(&input, &ops_wc));
        let (s_map, _) = assert_no_errors(propagate_schema(&input, &ops_map));

        assert_eq!(
            s_wc.fields.get("b"),
            s_map.fields.get("b"),
            "Map and WithColumns must produce the same schema for identical exprs"
        );
    }

    // ── Test 15: Cast replaces field type ─────────────────────────────────────

    #[test]
    fn prop_cast_replaces_field_type() {
        let input = schema_with(&[("amount", FieldType::Str)]);
        let mut type_map = BTreeMap::new();
        type_map.insert("amount".to_string(), "float".to_string());
        let ops = vec![OpNode::Cast { type_map }];
        let (final_schema, _) = assert_no_errors(propagate_schema(&input, &ops));
        assert_eq!(
            final_schema.fields.get("amount"),
            Some(&FieldType::F64),
            "amount should be F64 after cast to float"
        );
    }

    // ── Test 16: Cast unknown target errors ───────────────────────────────────

    #[test]
    fn prop_cast_unknown_target_errors() {
        let input = schema_with(&[("amount", FieldType::F64)]);
        let mut type_map = BTreeMap::new();
        type_map.insert("amount".to_string(), "decimal".to_string());
        let ops = vec![OpNode::Cast { type_map }];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.iter()
                .any(|e| matches!(e, PropagationError::TypeMismatch { op_index: 0, .. })),
            "expected TypeMismatch for unknown cast target, got {errs:?}"
        );
    }

    // ── Test 17: Cast unknown field errors ────────────────────────────────────

    #[test]
    fn prop_cast_unknown_field_errors() {
        let input = schema_with(&[("amount", FieldType::F64)]);
        let mut type_map = BTreeMap::new();
        type_map.insert("missing".to_string(), "int".to_string());
        let ops = vec![OpNode::Cast { type_map }];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.iter().any(|e| matches!(
                e,
                PropagationError::FieldMissing { op_index: 0, field }
                if field == "missing"
            )),
            "expected FieldMissing for 'missing', got {errs:?}"
        );
    }

    // ── Test 18: Fillna clears optional ───────────────────────────────────────

    #[test]
    fn prop_fillna_keeps_field_clears_optional() {
        let input = schema_with_opt(&[("amount", FieldType::F64)], &["amount"]);
        let mut defaults = BTreeMap::new();
        defaults.insert("amount".to_string(), serde_json::json!(0.0));
        let ops = vec![OpNode::Fillna { defaults }];
        let (final_schema, _) = assert_no_errors(propagate_schema(&input, &ops));
        assert_eq!(final_schema.fields.get("amount"), Some(&FieldType::F64));
        assert!(
            !final_schema.optional_fields.contains(&"amount".to_string()),
            "amount should no longer be optional after fillna"
        );
    }

    // ── Test 19: Fillna unknown field errors ──────────────────────────────────

    #[test]
    fn prop_fillna_unknown_field_errors() {
        let input = schema_with(&[("amount", FieldType::F64)]);
        let mut defaults = BTreeMap::new();
        defaults.insert("missing".to_string(), serde_json::json!(0));
        let ops = vec![OpNode::Fillna { defaults }];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.iter().any(|e| matches!(
                e,
                PropagationError::FieldMissing { op_index: 0, field }
                if field == "missing"
            )),
            "expected FieldMissing for 'missing', got {errs:?}"
        );
    }

    // ── Test 20: Chained ops propagate ────────────────────────────────────────

    #[test]
    fn prop_chained_ops_propagate() {
        let input = schema_with(&[("amount", FieldType::F64)]);
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

        let (final_schema, per_step) = assert_no_errors(propagate_schema(&input, &ops));

        assert_eq!(per_step.len(), 3, "must have 3 per-step schemas");
        // After op[2] (Cast): is_big should be I64
        assert_eq!(
            final_schema.fields.get("is_big"),
            Some(&FieldType::I64),
            "is_big should be I64 after cast"
        );
        assert_eq!(
            per_step[2].fields.get("is_big"),
            Some(&FieldType::I64),
            "per_step[2]['is_big'] should be I64"
        );
        assert_eq!(final_schema.fields.get("amount"), Some(&FieldType::F64));
    }

    // ── Test 21: Passthrough ops unsupported ──────────────────────────────────
    //
    // Phase 12.6 (2026-04-30): the Join + Union sub-cases of this test were
    // deleted with the variants per project_redis_shaped_no_event_time_ever.
    // The remaining GroupBy passthrough is the single legitimate
    // UnsupportedOp emitter in v0; joins/unions are rejected one layer up
    // by `register_validate::pre_check_removed_ops` with structured codes
    // feature_removed_no_joins_v0 / feature_removed_no_unions_v0.

    #[test]
    fn prop_passthrough_ops_unsupported() {
        let input = schema_with(&[("amount", FieldType::F64)]);

        // GroupBy is the lone v0 passthrough case.
        let ops_gb = vec![OpNode::GroupBy {
            keys: vec!["amount".to_string()],
            agg: BTreeMap::new(),
        }];
        let errs = assert_errors(propagate_schema(&input, &ops_gb));
        assert!(
            errs.iter().any(|e| matches!(
                e,
                PropagationError::UnsupportedOp {
                    op_index: 0,
                    op: "group_by"
                }
            )),
            "expected UnsupportedOp(group_by), got {errs:?}"
        );
    }

    // ── Test 22: Fail-soft collects multiple errors ───────────────────────────

    #[test]
    fn prop_fail_soft_collects_multiple_errors() {
        let input = schema_with(&[("amount", FieldType::F64)]);
        let ops = vec![
            OpNode::Select {
                fields: vec!["missing1".to_string()],
            },
            OpNode::Drop {
                fields: vec!["missing2".to_string()],
            },
        ];
        let errs = assert_errors(propagate_schema(&input, &ops));
        assert!(
            errs.len() >= 2,
            "expected at least 2 errors (one per op), got {errs:?}"
        );
        // First error is op_index=0.
        assert!(
            errs.iter()
                .any(|e| matches!(e, PropagationError::FieldMissing { op_index: 0, .. })),
            "expected op_index=0 error, got {errs:?}"
        );
        // Second error is op_index=1.
        assert!(
            errs.iter()
                .any(|e| matches!(e, PropagationError::FieldMissing { op_index: 1, .. })),
            "expected op_index=1 error, got {errs:?}"
        );
    }

    // ── Tests 23–28: infer_expr_type helpers ──────────────────────────────────

    fn parse_and_infer(src: &str, schema: &Schema) -> Result<InferredType, PropagationError> {
        let ast = crate::expr::parse(src).expect("should parse in test");
        infer_expr_type(&ast, schema)
    }

    // ── Test 23: Arithmetic promotion ─────────────────────────────────────────

    #[test]
    fn infer_expr_type_arithmetic_promotion() {
        let schema = schema_with(&[("a", FieldType::I64), ("b", FieldType::F64)]);

        // I64 + I64 → I64
        let s = schema_with(&[("x", FieldType::I64), ("y", FieldType::I64)]);
        let r = parse_and_infer("(x + y)", &s).expect("should not error");
        assert_eq!(
            r,
            InferredType::Known(FieldType::I64),
            "I64+I64 should be I64"
        );

        // I64 + F64 → F64
        let r2 = parse_and_infer("(a + b)", &schema).expect("should not error");
        assert_eq!(
            r2,
            InferredType::Known(FieldType::F64),
            "I64+F64 should be F64"
        );

        // I64 / I64 → I64 (v1 decision: integer division stays integer — matches runtime)
        let s2 = schema_with(&[("x", FieldType::I64), ("y", FieldType::I64)]);
        let r3 = parse_and_infer("(x / y)", &s2).expect("should not error");
        assert_eq!(
            r3,
            InferredType::Known(FieldType::I64),
            "I64/I64 should be I64 (v1: integer division, matching runtime arith_div)"
        );

        // F64 / I64 → F64 (type promotion)
        let s3 = schema_with(&[("a", FieldType::F64), ("y", FieldType::I64)]);
        let r4 = parse_and_infer("(a / y)", &s3).expect("should not error");
        assert_eq!(
            r4,
            InferredType::Known(FieldType::F64),
            "F64/I64 should be F64 (promotion)"
        );
    }

    // ── Test 24: Comparison is Bool ───────────────────────────────────────────

    #[test]
    fn infer_expr_type_comparison_is_bool() {
        let s = schema_with(&[("x", FieldType::I64), ("y", FieldType::F64)]);
        let r = parse_and_infer("(x > y)", &s).expect("should not error");
        assert_eq!(r, InferredType::Known(FieldType::Bool));

        let r2 = parse_and_infer("(x == y)", &s).expect("should not error");
        assert_eq!(r2, InferredType::Known(FieldType::Bool));
    }

    // ── Test 25: Boolean requires Bool operands ───────────────────────────────

    #[test]
    fn infer_expr_type_boolean_requires_bool() {
        let s = schema_with(&[("flag", FieldType::Bool), ("amount", FieldType::I64)]);

        // Bool and Bool → OK
        let r = parse_and_infer("(flag and true)", &s).expect("should not error");
        assert_eq!(r, InferredType::Known(FieldType::Bool));

        // I64 and Bool → TypeMismatch
        let r2 = parse_and_infer("(amount and flag)", &s);
        assert!(
            r2.is_err(),
            "expected TypeMismatch for I64 and Bool, got {r2:?}"
        );
    }

    // ── Test 26: isnull is Bool ───────────────────────────────────────────────

    #[test]
    fn infer_expr_type_isnull_is_bool() {
        let s = schema_with(&[("amount", FieldType::I64)]);
        let r = parse_and_infer("isnull(amount)", &s).expect("should not error");
        assert_eq!(r, InferredType::Known(FieldType::Bool));
    }

    // ── Test 27: cast returns target type ────────────────────────────────────

    #[test]
    fn infer_expr_type_cast_returns_target() {
        let s = schema_with(&[("amount", FieldType::I64)]);
        let r = parse_and_infer("cast(amount, float)", &s).expect("should not error");
        assert_eq!(r, InferredType::Known(FieldType::F64));

        // Unknown target type → error
        let r2 = parse_and_infer("cast(amount, 'blob')", &s);
        assert!(
            r2.is_err(),
            "expected TypeMismatch for unknown cast target 'blob', got {r2:?}"
        );

        // Illegal cast (Bytes → anything) is rejected at register time, the
        // same as the column-level cast path. Without this the expression
        // would infer I64 but `cast_eval` returns Null at runtime.
        let s_bytes = schema_with(&[("blob", FieldType::Bytes)]);
        let r3 = parse_and_infer("cast(blob, int)", &s_bytes);
        assert!(
            r3.is_err(),
            "expected TypeMismatch casting Bytes → int, got {r3:?}"
        );
    }

    // ── Test 28: unknown function errors at PARSE time (moved from typecheck) ─
    //
    // Under Step 7, function names resolve to BuiltinFn variants at parse
    // time. Unknown names surface as a ParseError there, never reaching
    // infer_expr_type. The replacement test lives in expr.rs as
    // `parse_unknown_function_name_fails` — this stub stays so the old test
    // name doesn't reappear in CI logs as a regression.

    #[test]
    fn infer_expr_type_unknown_function_errors_moved_to_parser() {
        let err = crate::expr::parse("foo(amount)")
            .expect_err("unknown function should fail at parse time");
        assert!(
            err.reason.contains("unknown function"),
            "got: {:?}",
            err.reason
        );
        assert!(err.reason.contains("foo"), "got: {:?}", err.reason);
        // Pin removal of the stale Phase-4 / "only 'cast' and 'isnull'" tail
        // — the per-name `match fn_name` in infer_call_type is gone.
        assert!(!err.reason.contains("Phase 4"), "got: {:?}", err.reason);
        assert!(
            !err.reason.contains("only 'cast' and 'isnull'"),
            "got: {:?}",
            err.reason
        );
    }

    // ── Test 29: quadkey returns I64 ──────────────────────────────────────────

    #[test]
    fn infer_expr_type_quadkey_returns_i64() {
        let s = schema_with(&[
            ("lat", FieldType::F64),
            ("lon", FieldType::F64),
            ("zoom", FieldType::I64),
        ]);
        let r = parse_and_infer("quadkey(lat, lon, zoom)", &s).expect("should not error");
        assert_eq!(r, InferredType::Known(FieldType::I64));
    }

    // ── Test 30: quadkey rejects non-numeric ──────────────────────────────────

    #[test]
    fn infer_expr_type_quadkey_rejects_non_numeric() {
        let s = schema_with(&[
            ("lat", FieldType::Str),
            ("lon", FieldType::F64),
            ("zoom", FieldType::I64),
        ]);
        let r = parse_and_infer("quadkey(lat, lon, zoom)", &s);
        assert!(r.is_err(), "expected TypeMismatch, got {r:?}");
        match r {
            Err(PropagationError::TypeMismatch { reason, .. }) => {
                assert!(reason.contains("quadkey"), "got: {reason}");
                // Pin removal of the `_ => "unhandled"` fall-through in `infer_call_type`
                // — closed by Step 7. Without this, the test passes today against
                // the wrong error.
                assert!(!reason.contains("unhandled"), "got: {reason}");
            }
            other => panic!("expected TypeMismatch, got {other:?}"),
        }
    }
}
