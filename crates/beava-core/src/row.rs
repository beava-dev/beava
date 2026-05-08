//! Row + Value data model for Phase 4 stateless ops and expression evaluation.
//!
//! Implements the shapes defined in CONTEXT.md §D-03 and the SQL three-valued
//! null logic specified in CONTEXT.md §D-04.
//!
//! # Key design choices
//! - `Row` wraps a `BTreeMap<String, Value>` for deterministic iteration order
//!   (Phase 5 aggregation-key stability depends on this).
//! - `Value::F64` uses a custom `PartialEq` that treats NaN as never equal.
//! - Boolean helpers (`and_three_valued`, `or_three_valued`, `not_three_valued`)
//!   implement the full SQL null truth table including short-circuit semantics.
//! - Row mutation helpers (`with_field`, `without_field`, `renamed`) consume
//!   `self` and return the updated `Row`. This satisfies SDK-OPS-09: derivation
//!   op steps construct a new `Row` per step rather than mutating shared state.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::BTreeMap;

// ─── Value ────────────────────────────────────────────────────────────────────

/// A dynamically-typed scalar value used in Row fields and expression results.
///
/// Mirrors `FieldType` one-to-one (see `type_of()`). `Null` has no `FieldType`
/// equivalent and signals absence/unknown per SQL three-valued logic (§D-04).
///
/// # Plan 18-11 D-2: Value::Str payload is CompactString
///
/// Strings ≤24 bytes live inline (no heap allocation). For typical fraud event
/// values (account_id "acc_123", country "US", merchant "M_ACME") this
/// eliminates the per-field String heap traffic that dominated the body→Row
/// path (measured ~50% of total deserialise time before this change).
/// CompactString implements `Deref<Target=str>` so most read sites work
/// unchanged via auto-deref.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Null,
    Str(CompactString),
    I64(i64),
    /// NaN-safe: two `F64(NaN)` values are never equal (see `PartialEq` impl).
    F64(f64),
    Bool(bool),
    Bytes(Vec<u8>),
    /// Milliseconds since Unix epoch — matches the `event_time` convention.
    Datetime(i64),
    /// Structured JSON output (top_k returns array of {value, count}).
    /// The adapter keeps this bincode-decodable in snapshots; without it
    /// `serde_json::Value::deserialize_any` aborts the entire snapshot
    /// decode (see `crate::bincode_safe_json`).
    Json(#[serde(with = "crate::bincode_safe_json::value")] serde_json::Value),
    /// Ordered list of values used as an aggregation output (e.g.
    /// `most_recent_n`, `reservoir_sample`). Never appears in event/table
    /// rows — only as the output of `AggOp::query`. `type_of()` → None.
    List(Vec<Value>),
    /// Keyed map of values used as a structured aggregation output (e.g.
    /// `histogram`, `event_type_mix`). Never appears in event/table rows —
    /// only as the output of `AggOp::query`. `type_of()` → None.
    Map(BTreeMap<String, Value>),
}

/// Convert a `serde_json::Value` (JSON primitive) into the beava `Value` type.
///
/// Used by the Row Deserialize impl to convert wire-format JSON fields.
/// `serde_json::Value` uses `deserialize_any` which works with both serde_json
/// and rmp_serde (both support `deserialize_any`).
pub fn json_value_to_beava_value(jv: serde_json::Value) -> Value {
    match jv {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::I64(i)
            } else if let Some(f) = n.as_f64() {
                Value::F64(f)
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::Str(CompactString::from(s)),
        serde_json::Value::Array(arr) => {
            Value::List(arr.into_iter().map(json_value_to_beava_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = BTreeMap::new();
            for (k, v) in obj {
                map.insert(k, json_value_to_beava_value(v));
            }
            Value::Map(map)
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            // NaN is never equal to anything, including itself (IEEE-754).
            (Value::F64(a), Value::F64(b)) => !a.is_nan() && !b.is_nan() && a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::I64(a), Value::I64(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Datetime(a), Value::Datetime(b)) => a == b,
            (Value::Json(a), Value::Json(b)) => a == b,
            // List + Map recurse element-wise (BTreeMap iteration is ordered
            // + deterministic; PartialEq on Vec is positional).
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            // Cross-variant comparisons are always false.
            _ => false,
        }
    }
}

impl Value {
    /// Returns the corresponding `FieldType` for non-Null values, or `None` for `Null`.
    pub fn type_of(&self) -> Option<crate::schema::FieldType> {
        use crate::schema::FieldType;
        match self {
            Value::Null => None,
            Value::Str(_) => Some(FieldType::Str),
            Value::I64(_) => Some(FieldType::I64),
            Value::F64(_) => Some(FieldType::F64),
            Value::Bool(_) => Some(FieldType::Bool),
            Value::Bytes(_) => Some(FieldType::Bytes),
            Value::Datetime(_) => Some(FieldType::Datetime),
            Value::Json(_) => Some(FieldType::Json),
            // Structured outputs are not representable as a FieldType —
            // they only appear as aggregation outputs, never in event/table rows.
            Value::List(_) => None,
            Value::Map(_) => None,
        }
    }

    /// SQL three-valued AND.
    ///
    /// Truth table (§D-04):
    /// - `false AND null  = false`   (short-circuit)
    /// - `null  AND false = false`   (short-circuit)
    /// - `true  AND true  = true`
    /// - `true  AND null  = null`
    /// - `null  AND true  = null`
    /// - `null  AND null  = null`
    /// - Non-bool/non-null operands → `Null` (runtime-tolerant per §D-04)
    pub fn and_three_valued(&self, other: &Self) -> Self {
        match (self, other) {
            // Short-circuit: false on either side always yields false.
            (Value::Bool(false), Value::Bool(_))
            | (Value::Bool(false), Value::Null)
            | (Value::Bool(_), Value::Bool(false))
            | (Value::Null, Value::Bool(false)) => Value::Bool(false),
            // Both true.
            (Value::Bool(true), Value::Bool(true)) => Value::Bool(true),
            // At least one null, no short-circuit false → null.
            (Value::Null, Value::Null)
            | (Value::Null, Value::Bool(true))
            | (Value::Bool(true), Value::Null) => Value::Null,
            // Any non-bool/non-null operand → Null (runtime-tolerant).
            _ => Value::Null,
        }
    }

    /// SQL three-valued OR.
    ///
    /// Truth table (§D-04):
    /// - `true  OR null  = true`   (short-circuit)
    /// - `null  OR true  = true`   (short-circuit)
    /// - `false OR false = false`
    /// - `false OR null  = null`
    /// - `null  OR false = null`
    /// - `null  OR null  = null`
    /// - Non-bool/non-null operands → `Null` (runtime-tolerant per §D-04)
    pub fn or_three_valued(&self, other: &Self) -> Self {
        match (self, other) {
            // Short-circuit: true on either side always yields true.
            (Value::Bool(true), Value::Bool(_))
            | (Value::Bool(true), Value::Null)
            | (Value::Bool(_), Value::Bool(true))
            | (Value::Null, Value::Bool(true)) => Value::Bool(true),
            // Both false.
            (Value::Bool(false), Value::Bool(false)) => Value::Bool(false),
            // At least one null, no short-circuit true → null.
            (Value::Null, Value::Null)
            | (Value::Null, Value::Bool(false))
            | (Value::Bool(false), Value::Null) => Value::Null,
            // Any non-bool/non-null operand → Null (runtime-tolerant).
            _ => Value::Null,
        }
    }

    /// SQL three-valued NOT.
    ///
    /// - `NOT true  = false`
    /// - `NOT false = true`
    /// - `NOT null  = null`
    /// - Non-bool/non-null → `Null` (runtime-tolerant per §D-04)
    pub fn not_three_valued(&self) -> Self {
        match self {
            Value::Bool(b) => Value::Bool(!b),
            Value::Null => Value::Null,
            _ => Value::Null,
        }
    }
}

// ─── Row ──────────────────────────────────────────────────────────────────────

/// A named-field bag of `Value`s.
///
/// **Plan 18-11 D-1:** backed by `SmallVec<[(CompactString, Value); 8]>`
/// instead of the prior `BTreeMap<String, Value>`. Most events have ≤8
/// fields, so the storage is inline — zero heap allocation for the row
/// container. CompactString is inline for keys ≤24 bytes (most field
/// names + most short str values).
///
/// `Row::get(&str)` performs a linear scan over the SmallVec — for ≤8
/// fields this is faster than BTreeMap O(log N) and stays in cache.
///
/// `Row::with_field(field, value)`: if the field already exists, replaces
/// the value in-place (preserving insertion order). Otherwise pushes to
/// the SmallVec.
///
/// **Iteration order:** insertion order (was BTreeMap-sorted in v0–v1).
/// All call sites that depended on sorted iteration have been audited
/// — Row.iter() is only used by debug routes (registry_debug, temporal_http)
/// and by op_chain.rs for direct row-mutation; none rely on lexicographic
/// ordering.
///
/// All "mutation" helpers consume `self` and return a new `Row`. This is the
/// owning API contract required by SDK-OPS-09: stateless op steps must not
/// mutate a shared upstream row.
#[derive(Debug, Clone, PartialEq)]
pub struct Row(pub SmallVec<[(CompactString, Value); 8]>);

impl Row {
    /// Creates an empty Row.
    pub fn new() -> Self {
        Row(SmallVec::new())
    }

    /// Returns a reference to the value for `field`, or `None` if absent.
    /// Linear scan over the SmallVec — O(N) in the number of fields,
    /// but fast in cache for the common case (≤8 fields).
    pub fn get(&self, field: &str) -> Option<&Value> {
        #[cfg(feature = "test-utils")]
        {
            GET_COUNT.with(|c| {
                *c.borrow_mut() += 1;
            });
        }
        self.0
            .iter()
            .find(|(k, _)| k.as_str() == field)
            .map(|(_, v)| v)
    }

    /// Consumes `self`, inserts or overwrites `field` with `value`, and returns
    /// the updated `Row`.
    ///
    /// If `field` already exists, the existing value is replaced in-place
    /// (preserving the original insertion-order position). Otherwise the
    /// new pair is pushed to the SmallVec.
    ///
    /// SDK-OPS-09: callers that need to preserve the upstream `Row` must
    /// `.clone()` before calling this method. Derivation op steps construct a
    /// fresh copy per step and do not share mutable state.
    pub fn with_field(mut self, field: &str, value: Value) -> Self {
        if let Some(slot) = self.0.iter_mut().find(|(k, _)| k.as_str() == field) {
            slot.1 = value;
        } else {
            self.0.push((CompactString::from(field), value));
        }
        self
    }

    /// Insert when the key is already a CompactString — used by the Row
    /// Deserialize hot path to skip the &str→CompactString conversion when
    /// the key comes typed from `next_key::<CompactString>`.
    pub fn with_field_owned(mut self, field: CompactString, value: Value) -> Self {
        if let Some(slot) = self.0.iter_mut().find(|(k, _)| *k == field) {
            slot.1 = value;
        } else {
            self.0.push((field, value));
        }
        self
    }

    /// Consumes `self`, removes `field` (no-op if absent), and returns the
    /// updated `Row`.
    pub fn without_field(mut self, field: &str) -> Self {
        if let Some(idx) = self.0.iter().position(|(k, _)| k.as_str() == field) {
            self.0.remove(idx);
        }
        self
    }

    /// Consumes `self`, renames `old` to `new` (preserving the value), and
    /// returns the updated `Row`. If `old` is absent this is a no-op. If `new`
    /// already exists it is overwritten.
    pub fn renamed(mut self, old: &str, new: &str) -> Self {
        // Find old; if present, take its value, then insert under new (with
        // overwrite semantics if new already exists).
        let old_pos = self.0.iter().position(|(k, _)| k.as_str() == old);
        if let Some(idx) = old_pos {
            let (_, value) = self.0.remove(idx);
            // Reuse with_field for overwrite-or-push behaviour at `new`.
            self = self.with_field(new, value);
        }
        self
    }

    /// Returns an iterator over `(field_name, value)` pairs in insertion
    /// order. Yields `(&str, &Value)` to keep the existing call-site shape
    /// (BTreeMap previously yielded `(&String, &Value)` and call sites used
    /// `.as_str()` on the key).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Returns the number of fields in this Row.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if this Row contains no fields.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Row Deserialize (zero-alloc rewrite) ─────────────────────────────────────
//
// Custom Deserialize impl for Row that works with BOTH serde_json and rmp_serde.
//
// The visitor walks the deserializer's MapAccess directly — no
// `serde_json::Value` intermediate. Each field's value is deserialized via a
// `BeavaValueVisitor` that handles serde primitives (bool / i64 / u64 / f64 /
// str / unit / map / seq) and constructs a beava `Value` directly.
//
// This shaves the per-event JsonValue allocation that was the largest fixed
// cost in dispatch_push_sync. Post-rewrite the dispatch path runs ≈ 1,800 ns
// for both JSON and msgpack on the M4 reference box.

impl<'de> serde::Deserialize<'de> for Row {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct RowVisitor;

        impl<'de> serde::de::Visitor<'de> for RowVisitor {
            type Value = Row;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON/msgpack map of string field names to primitive values")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Row, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                // Walk values directly via BeavaValueVisitor — no JsonValue
                // heap allocation per field. Direct push into the SmallVec
                // storage — no with_field re-clone, no schema lookup.
                // Measured at ~146 ns msgpack / ~184 ns json on the M4 box.
                let mut row = Row(SmallVec::with_capacity(8));
                while let Some(key) = access.next_key::<CompactString>()? {
                    let value: Value = access.next_value_seed(BeavaValueSeed)?;
                    row.0.push((key, value));
                }
                Ok(row)
            }
        }

        deserializer.deserialize_map(RowVisitor)
    }
}

// ─── BeavaValueSeed: deserialize a serde-data-model value directly to beava Value ─

/// A `DeserializeSeed` that drives `deserialize_any` on the deserializer and
/// constructs a beava `Value` from whatever primitive it hits — no
/// `serde_json::Value` allocation.
///
/// Both `serde_json` and `rmp_serde` route `deserialize_any` to the visitor's
/// type-specific `visit_*` methods based on what's in the wire data, so this
/// works for either format.
struct BeavaValueSeed;

impl<'de> serde::de::DeserializeSeed<'de> for BeavaValueSeed {
    type Value = Value;
    fn deserialize<D>(self, deserializer: D) -> Result<Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(BeavaValueVisitor)
    }
}

struct BeavaValueVisitor;

impl<'de> serde::de::Visitor<'de> for BeavaValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a JSON/msgpack scalar, array, or object")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::I64(v))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        // Range check: convert to I64 if it fits, else fall back to F64
        // (matches the json_value_to_beava_value behaviour for large uints).
        if v <= i64::MAX as u64 {
            Ok(Value::I64(v as i64))
        } else {
            Ok(Value::F64(v as f64))
        }
    }
    fn visit_i128<E>(self, v: i128) -> Result<Value, E> {
        if v >= i64::MIN as i128 && v <= i64::MAX as i128 {
            Ok(Value::I64(v as i64))
        } else {
            Ok(Value::F64(v as f64))
        }
    }
    fn visit_u128<E>(self, v: u128) -> Result<Value, E> {
        if v <= i64::MAX as u128 {
            Ok(Value::I64(v as i64))
        } else {
            Ok(Value::F64(v as f64))
        }
    }
    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::F64(v))
    }
    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::Str(CompactString::from(v)))
    }
    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Value, E> {
        Ok(Value::Str(CompactString::from(v)))
    }
    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::Str(CompactString::from(v)))
    }
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Value, E> {
        // Bytes wire-type is rare in events; preserve as Bytes for any caller
        // that needs raw bytes (e.g. binary fields).
        Ok(Value::Bytes(v.to_vec()))
    }
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Value, E> {
        Ok(Value::Bytes(v))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_some<D>(self, deserializer: D) -> Result<Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(elem) = seq.next_element_seed(BeavaValueSeed)? {
            out.push(elem);
        }
        Ok(Value::List(out))
    }
    fn visit_map<A>(self, mut map: A) -> Result<Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut out = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            let value: Value = map.next_value_seed(BeavaValueSeed)?;
            out.insert(key, value);
        }
        Ok(Value::Map(out))
    }
}

/// Row serialization: serialize as a flat JSON object `{field: value, ...}`.
impl serde::Serialize for Row {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            // CompactString::as_str() — serialize the key as &str so the
            // wire format stays identical to the prior BTreeMap-backed Row.
            map.serialize_entry(k.as_str(), v)?;
        }
        map.end()
    }
}

// ─── Test probe: Row::get call counter ───────────────────────────────────────

// Thread-local counter incremented on every `Row::get` call when
// `feature = "test-utils"` is active. Lets integration tests assert that
// the apply-loop uses pre-extraction (O(distinct_fields) calls) rather
// than per-feature scanning (O(n_features × distinct_fields) calls).
// Never active in production builds.
#[cfg(feature = "test-utils")]
thread_local! {
    static GET_COUNT: std::cell::RefCell<usize> = const { std::cell::RefCell::new(0) };
}

/// Drain and return the accumulated `Row::get` call count for the current
/// thread. Resets the counter to zero. Available only when
/// `feature = "test-utils"` is enabled on the beava-core crate.
#[cfg(feature = "test-utils")]
pub fn _take_get_count() -> usize {
    GET_COUNT.with(|c| {
        let v = *c.borrow();
        *c.borrow_mut() = 0;
        v
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::FieldType;

    // Test 1: Value::type_of maps every non-Null variant to the expected FieldType;
    // Null returns None.
    #[test]
    fn value_type_of_maps_every_variant_to_fieldtype() {
        assert_eq!(Value::Null.type_of(), None);
        assert_eq!(Value::Str("x".into()).type_of(), Some(FieldType::Str));
        assert_eq!(Value::I64(1).type_of(), Some(FieldType::I64));
        assert_eq!(Value::F64(1.0).type_of(), Some(FieldType::F64));
        assert_eq!(Value::Bool(true).type_of(), Some(FieldType::Bool));
        assert_eq!(Value::Bytes(vec![]).type_of(), Some(FieldType::Bytes));
        assert_eq!(Value::Datetime(0).type_of(), Some(FieldType::Datetime));
    }

    // Test 2: NaN is never equal to itself.
    #[test]
    fn value_partialeq_nan_is_never_equal() {
        let a = Value::F64(f64::NAN);
        let b = Value::F64(f64::NAN);
        assert_ne!(a, b, "NaN != NaN per IEEE-754 (guarded in PartialEq)");
    }

    // Test 3: Structural equality — Value::Null == Value::Null at the Rust level.
    // SQL "null == null" yields Value::Null (tested through and/or helpers), but
    // the Rust PartialEq on Value treats Null structurally.
    #[test]
    fn value_partialeq_null_equals_null_structurally() {
        assert_eq!(
            Value::Null,
            Value::Null,
            "Value::Null must structurally equal itself"
        );
    }

    // Test 4: and_three_valued truth table.
    #[test]
    fn and_three_valued_truth_table() {
        let t = Value::Bool(true);
        let f = Value::Bool(false);
        let n = Value::Null;

        assert_eq!(t.and_three_valued(&t), Value::Bool(true));
        assert_eq!(t.and_three_valued(&f), Value::Bool(false));
        assert_eq!(t.and_three_valued(&n), Value::Null);
        assert_eq!(f.and_three_valued(&n), Value::Bool(false)); // short-circuit
        assert_eq!(n.and_three_valued(&f), Value::Bool(false)); // short-circuit
        assert_eq!(n.and_three_valued(&n), Value::Null);
        assert_eq!(n.and_three_valued(&t), Value::Null);
        assert_eq!(f.and_three_valued(&t), Value::Bool(false));
    }

    // Test 5: or_three_valued truth table.
    #[test]
    fn or_three_valued_truth_table() {
        let t = Value::Bool(true);
        let f = Value::Bool(false);
        let n = Value::Null;

        assert_eq!(t.or_three_valued(&n), Value::Bool(true)); // short-circuit
        assert_eq!(n.or_three_valued(&t), Value::Bool(true)); // short-circuit
        assert_eq!(f.or_three_valued(&n), Value::Null);
        assert_eq!(n.or_three_valued(&f), Value::Null);
        assert_eq!(n.or_three_valued(&n), Value::Null);
        assert_eq!(f.or_three_valued(&f), Value::Bool(false));
        assert_eq!(t.or_three_valued(&t), Value::Bool(true));
        assert_eq!(t.or_three_valued(&f), Value::Bool(true)); // short-circuit on left
    }

    // Test 6: not_three_valued truth table.
    #[test]
    fn not_three_valued_truth_table() {
        assert_eq!(Value::Bool(true).not_three_valued(), Value::Bool(false));
        assert_eq!(Value::Bool(false).not_three_valued(), Value::Bool(true));
        assert_eq!(Value::Null.not_three_valued(), Value::Null);
    }

    // Test 7: Non-bool operands to and/or return Null (runtime tolerance per §D-04).
    #[test]
    fn and_or_reject_non_bool_operands() {
        let i = Value::I64(1);
        let t = Value::Bool(true);
        let n = Value::Null;

        // i64 operand to AND → Null
        assert_eq!(i.and_three_valued(&t), Value::Null);
        assert_eq!(t.and_three_valued(&i), Value::Null);
        assert_eq!(i.and_three_valued(&n), Value::Null);

        // i64 operand to OR → Null
        assert_eq!(i.or_three_valued(&t), Value::Null);
        assert_eq!(t.or_three_valued(&i), Value::Null);
    }

    // Test 8: Row::new() produces an empty Row.
    #[test]
    fn row_new_is_empty() {
        let r = Row::new();
        assert_eq!(r.len(), 0);
        assert!(r.is_empty());
    }

    // Test 9: with_field consumes self and returns updated Row; original binding
    // cannot be used afterward (ownership transferred). Verify via a second alias.
    // SDK-OPS-09: owning API, no shared mutable state.
    #[test]
    fn row_with_field_returns_new_row_without_mutating_source() {
        let r1 = Row::new();
        let r2 = r1.with_field("x", Value::I64(7));
        // r1 has been consumed; r2 has "x"
        assert_eq!(r2.get("x"), Some(&Value::I64(7)));
        // Aliasing test: clone r2 before modifying to show originals are unaffected
        let r3 = r2.clone();
        let r4 = r3.with_field("y", Value::I64(99));
        // r2 still only has "x"
        assert_eq!(r2.get("y"), None);
        assert_eq!(r4.get("x"), Some(&Value::I64(7)));
        assert_eq!(r4.get("y"), Some(&Value::I64(99)));
    }

    // Test 10: without_field removes the named field.
    #[test]
    fn row_without_field_removes_field() {
        let r = Row::new()
            .with_field("x", Value::I64(1))
            .with_field("y", Value::I64(2));
        let r2 = r.without_field("x");
        assert_eq!(r2.get("x"), None);
        assert_eq!(r2.get("y"), Some(&Value::I64(2)));
    }

    // Test 11: renamed swaps key while preserving value; renaming absent key is a no-op.
    #[test]
    fn row_renamed_swaps_key_preserving_value() {
        let r = Row::new().with_field("a", Value::I64(5));
        let r2 = r.renamed("a", "b");
        assert_eq!(r2.get("b"), Some(&Value::I64(5)));
        assert_eq!(r2.get("a"), None);

        // Renaming absent field is a no-op
        let r3 = r2.renamed("no_such_field", "z");
        assert_eq!(r3.get("z"), None);
        assert_eq!(r3.get("b"), Some(&Value::I64(5)));
    }

    // Test 12 (Plan 18-11 update): iter yields keys in INSERTION order
    // (was BTreeMap-sorted). All call sites that depended on sorted iter
    // were audited — Row.iter() consumers handle insertion-order fine.
    #[test]
    fn row_iter_order_is_insertion_order() {
        let r = Row::new()
            .with_field("c", Value::I64(3))
            .with_field("a", Value::I64(1))
            .with_field("b", Value::I64(2));
        let keys: Vec<&str> = r.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["c", "a", "b"]);
    }

    // Test 13 (Plan 10-05): Value::Json variant exists for sketch top_k output.
    #[test]
    fn value_json_variant_exists() {
        let v = Value::Json(serde_json::json!([{"value": "a", "count": 5}]));
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains("count"));
    }

    // ── Phase 11: Value::List + Value::Map for structured outputs (D-01) ──────

    /// Value::List variant exists and round-trips through serde.
    #[test]
    fn value_list_round_trips_serde() {
        let v = Value::List(vec![Value::I64(1), Value::I64(2), Value::I64(3)]);
        let s = serde_json::to_string(&v).expect("serialize List");
        let v2: Value = serde_json::from_str(&s).expect("deserialize List");
        assert_eq!(v, v2);
    }

    /// Value::Map variant round-trips deterministically (BTreeMap).
    #[test]
    fn value_map_round_trips_serde() {
        let mut m = BTreeMap::new();
        m.insert("a".to_string(), Value::I64(10));
        m.insert("b".to_string(), Value::F64(2.5));
        let v = Value::Map(m);
        let s = serde_json::to_string(&v).expect("serialize Map");
        let v2: Value = serde_json::from_str(&s).expect("deserialize Map");
        assert_eq!(v, v2);
    }

    /// PartialEq on Value::List compares element-wise.
    #[test]
    fn value_list_partialeq_elementwise() {
        let a = Value::List(vec![Value::I64(1), Value::Str("x".into())]);
        let b = Value::List(vec![Value::I64(1), Value::Str("x".into())]);
        let c = Value::List(vec![Value::I64(1), Value::Str("y".into())]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    /// PartialEq on Value::Map matches when same keys + same values.
    #[test]
    fn value_map_partialeq_keys_and_values() {
        let mut m1 = BTreeMap::new();
        m1.insert("k".to_string(), Value::I64(1));
        let mut m2 = BTreeMap::new();
        m2.insert("k".to_string(), Value::I64(1));
        let mut m3 = BTreeMap::new();
        m3.insert("k".to_string(), Value::I64(2));
        assert_eq!(Value::Map(m1.clone()), Value::Map(m2));
        assert_ne!(Value::Map(m1), Value::Map(m3));
    }

    /// Cross-variant comparison: List vs Map must be false.
    #[test]
    fn value_list_vs_map_cross_variant_false() {
        let l = Value::List(vec![]);
        let m = Value::Map(BTreeMap::new());
        assert_ne!(l, m);
    }

    /// type_of returns None for List/Map (no FieldType representation; outputs only).
    #[test]
    fn value_list_map_type_of_is_none() {
        assert_eq!(Value::List(vec![]).type_of(), None);
        assert_eq!(Value::Map(BTreeMap::new()).type_of(), None);
    }

    // ── Value::Str(CompactString) ──────────────────────────────────────────────

    /// `Value::Str` payload is a `CompactString` (was `String`).
    /// CompactString does inline-storage for strings ≤24 bytes — no heap alloc.
    ///
    /// We don't measure the heap directly here (dhat is too heavy for unit
    /// scope); the structural guarantee is encoded by the type-name check
    /// + a CompactString-specific method call.
    #[test]
    fn value_str_uses_compact_string() {
        use compact_str::CompactString;

        // Construction from a literal must accept CompactString (zero-alloc
        // for short strings) — this fails to compile if Value::Str still
        // takes String.
        let cs: CompactString = CompactString::from("US");
        let v = Value::Str(cs.clone());

        match &v {
            Value::Str(s) => {
                // Type assertion: the inner is CompactString, not String.
                let _: &CompactString = s;
                // Inline-storage check: short strings (≤24 bytes) live inline.
                // CompactString::is_heap_allocated() is the canonical check.
                assert!(
                    !s.is_heap_allocated(),
                    "short literal 'US' must be inline (no heap alloc)"
                );
                assert_eq!(s.as_str(), "US");
            }
            _ => panic!("expected Value::Str"),
        }
    }

    // ── Row SmallVec<[(CompactString, Value); 8]> ─────────────────────────────

    /// `Row.0` storage is SmallVec inline for ≤8 fields. Most events have
    /// ≤8 fields → zero heap allocation for the row container.
    #[test]
    fn row_smallvec_inline_no_spill_for_six_fields() {
        use smallvec::SmallVec;
        // Construct a 6-field row.
        let row = Row::new()
            .with_field("amount", Value::F64(99.95))
            .with_field("ts", Value::I64(1_714_234_567_000))
            .with_field("account_id", Value::Str("acc_123".into()))
            .with_field("merchant", Value::Str("M_ACME".into()))
            .with_field("country", Value::Str("US".into()))
            .with_field("method", Value::Str("card".into()));

        // The backing is a SmallVec — exposes spilled() — and 6 fields fit
        // inline (capacity=8).
        let _: &SmallVec<[(compact_str::CompactString, Value); 8]> = &row.0;
        assert!(
            !row.0.spilled(),
            "6-field Row must use inline SmallVec storage (no heap)"
        );
        assert_eq!(row.0.len(), 6);
    }

    /// Row::get scans the SmallVec linearly (≤8 fields, cache-friendly).
    /// Faster than BTreeMap O(log N) for small rows. Verify functional
    /// correctness post-swap.
    #[test]
    fn row_get_linear_scan_returns_value() {
        let row = Row::new()
            .with_field("a", Value::I64(1))
            .with_field("b", Value::Str("hello".into()))
            .with_field("c", Value::F64(2.5));

        assert_eq!(row.get("a"), Some(&Value::I64(1)));
        assert_eq!(row.get("b"), Some(&Value::Str("hello".into())));
        assert_eq!(row.get("c"), Some(&Value::F64(2.5)));
        assert_eq!(row.get("nope"), None);
    }

    /// with_field: replaces in-place when the field already exists; pushes
    /// otherwise. Iteration order is insertion order (no longer alphabetical).
    #[test]
    fn row_with_field_replaces_existing_field_in_place() {
        let row = Row::new()
            .with_field("x", Value::I64(1))
            .with_field("y", Value::I64(2))
            .with_field("x", Value::I64(99)); // overwrite x

        assert_eq!(row.get("x"), Some(&Value::I64(99)));
        assert_eq!(row.get("y"), Some(&Value::I64(2)));
        assert_eq!(row.0.len(), 2, "no duplicate entry on overwrite");
    }
}
