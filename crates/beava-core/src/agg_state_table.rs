//! Per-aggregation, per-entity state storage.
//!
//! # Overview
//!
//! `AggStateTable` maps entity → Vec<AggOp> where each slot corresponds (in
//! order) to `AggregationDescriptor::features`.
//!
//! ## Plan 19.2-03 (D-03): EntityKey hybrid storage
//!
//! Instead of one `HashMap<EntityKey, Vec<AggOp>>` (which requires SmallVec
//! build + CompactString canonicalization on every event), `AggStateTable` now
//! carries three specialized sub-maps selected by the entity-key shape:
//!
//! | Shape | Key type | Cost |
//! |-------|----------|------|
//! | SingleU64 | u64 | pure u64 HashMap lookup; zero alloc |
//! | SingleStr | CompactString | single-string HashMap lookup; zero alloc for ≤24 bytes |
//! | Multi | EntityKey (SmallVec) | same as old EntityKey; used for compound keys |
//!
//! `EntityKeyShape::from_row` selects the shape at apply time. `SingleU64`
//! covers all numeric + bool + datetime single-keys via a tagged bit-cast; the
//! tag bits prevent I64/F64/Bool/Datetime collisions within the same u64 space.
//!
//! ## Key design invariants
//!
//! - Null or missing group-key values produce `None` from `EntityKeyShape::from_row`,
//!   causing the apply loop to drop the event for that aggregation.
//! - NaN F64 group-key values produce `None` at push time.
//! - F64-typed group_key columns are rejected at register-time (see
//!   `Registry::validate_group_keys_for_agg`).
//! - `EntityKey` (legacy struct) remains for snapshot serialization,
//!   snapshot-load (recovery), and query-side key parsing. It is the
//!   stable serialized shape; the 3 sub-maps are the runtime hot-path shape.
//!
//! # Value variants accepted
//!
//! | `Value` variant    | Single-key shape   | Multi-key inclusion |
//! |--------------------|--------------------|--------------------|
//! | `Str(CompactStr)`  | SingleStr          | included           |
//! | `I64(n)`           | SingleU64          | included (str-ized)|
//! | `F64(f)` (non-NaN) | SingleU64          | included (str-ized)|
//! | `F64(NaN)`         | None (drop)        | None (drop)        |
//! | `Bool(b)`          | SingleU64          | included (str-ized)|
//! | `Datetime(ms)`     | SingleU64          | included (str-ized)|
//! | `Bytes(_)`         | None (drop)        | None (drop)        |
//! | `Null`             | None (drop)        | None (drop)        |
//! | `Json/List/Map`    | None (drop)        | None (drop)        |

use std::hash::{Hash, Hasher};

use crate::agg_descriptor::AggregationDescriptor;
use crate::agg_op::AggOp;
use crate::row::{Row, Value};
use crate::schema::FieldType;
use compact_str::CompactString;
use fxhash::FxBuildHasher;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

// ─── EntityKey (legacy / snapshot-compat shape) ───────────────────────────────

/// Stable entity identifier for snapshot serialization and query-side key
/// parsing. The apply hot path uses `EntityKeyShape` instead (D-03).
///
/// SmallVec inline storage of `(CompactString, Value)` pairs in declaration
/// order. Serde-derived so it survives snapshot round-trips.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityKey(pub SmallVec<[(CompactString, Value); 2]>);

impl PartialEq for EntityKey {
    fn eq(&self, other: &Self) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }
        self.0
            .iter()
            .zip(other.0.iter())
            .all(|((ak, av), (bk, bv))| ak == bk && entity_value_eq(av, bv))
    }
}

impl Eq for EntityKey {}

impl Hash for EntityKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.len().hash(state);
        for (k, v) in &self.0 {
            k.hash(state);
            hash_entity_value(v, state);
        }
    }
}

impl PartialOrd for EntityKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EntityKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        for ((ak, av), (bk, bv)) in self.0.iter().zip(other.0.iter()) {
            match ak.cmp(bk) {
                std::cmp::Ordering::Equal => {}
                ord => return ord,
            }
            match cmp_entity_value(av, bv) {
                std::cmp::Ordering::Equal => {}
                ord => return ord,
            }
        }
        self.0.len().cmp(&other.0.len())
    }
}

/// Equality over the Value variants permitted inside an EntityKey.
fn entity_value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::I64(x), Value::I64(y)) => x == y,
        (Value::F64(x), Value::F64(y)) => x.to_bits() == y.to_bits(),
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Datetime(x), Value::Datetime(y)) => x == y,
        _ => false,
    }
}

fn hash_entity_value<H: Hasher>(v: &Value, state: &mut H) {
    match v {
        Value::Str(s) => {
            0u8.hash(state);
            s.as_str().hash(state);
        }
        Value::I64(n) => {
            1u8.hash(state);
            n.hash(state);
        }
        Value::F64(f) => {
            2u8.hash(state);
            f.to_bits().hash(state);
        }
        Value::Bool(b) => {
            3u8.hash(state);
            b.hash(state);
        }
        Value::Datetime(ms) => {
            4u8.hash(state);
            ms.hash(state);
        }
        _ => {
            255u8.hash(state);
        }
    }
}

fn cmp_entity_value(a: &Value, b: &Value) -> std::cmp::Ordering {
    fn rank(v: &Value) -> u8 {
        match v {
            Value::Str(_) => 0,
            Value::I64(_) => 1,
            Value::F64(_) => 2,
            Value::Bool(_) => 3,
            Value::Datetime(_) => 4,
            _ => 255,
        }
    }
    match rank(a).cmp(&rank(b)) {
        std::cmp::Ordering::Equal => match (a, b) {
            (Value::Str(x), Value::Str(y)) => x.cmp(y),
            (Value::I64(x), Value::I64(y)) => x.cmp(y),
            (Value::F64(x), Value::F64(y)) => x.total_cmp(y),
            (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
            (Value::Datetime(x), Value::Datetime(y)) => x.cmp(y),
            _ => std::cmp::Ordering::Equal,
        },
        ord => ord,
    }
}

impl EntityKey {
    /// Build an `EntityKey` from a `Row` (legacy path for snapshot/query compat).
    ///
    /// Canonicalizes group-key values to `Value::Str`. Returns `None` if
    /// any group-key field is absent or non-key-safe.
    pub fn from_row(group_keys: &[String], row: &Row) -> Option<EntityKey> {
        let mut pairs: SmallVec<[(CompactString, Value); 2]> =
            SmallVec::with_capacity(group_keys.len());
        for key in group_keys {
            let canonical: CompactString = match row.get(key) {
                None => return None,
                Some(Value::Null) => return None,
                Some(Value::Bytes(_)) => return None,
                Some(Value::Json(_)) => return None,
                Some(Value::List(_)) | Some(Value::Map(_)) => return None,
                Some(Value::Str(s)) => s.clone(),
                Some(Value::I64(n)) => n.to_string().into(),
                Some(Value::F64(f)) => format!("{:?}", f).into(),
                Some(Value::Bool(b)) => b.to_string().into(),
                Some(Value::Datetime(ms)) => ms.to_string().into(),
            };
            pairs.push((CompactString::from(key.as_str()), Value::Str(canonical)));
        }
        Some(EntityKey(pairs))
    }
}

// ─── EntityKeyShape (hot-path type) ────────────────────────────────────────

/// EntityKey hybrid — three storage shapes selected by group_keys
/// cardinality + value type. Avoids SmallVec build + CompactString
/// canonicalization on the single-key numeric path.
///
/// ## Tag layout for SingleU64
///
/// The high 4 bits of the u64 carry a variant tag so that the same numeric
/// payload from different Value types does not collide:
/// - I64      → tag = 1
/// - F64      → tag = 2
/// - Bool     → tag = 3
/// - Datetime → tag = 4
///
/// Payloads are masked to the low 60 bits. I64 negatives lose their sign
/// extension above bit 59 — for fraud workloads (entity IDs, timestamps,
/// booleans) all values are well below 2^59, making this a non-issue.
/// Documented as a v0 limitation.
///
/// ## SingleStr collision behavior
///
/// Stores both the FxHash (for fast bucket lookup) and the original
/// CompactString (for equality comparison). Hash collisions cause
/// bucket-slot collisions in hashbrown but NOT wrong-entity merges:
/// hashbrown falls back to Eq on the stored CompactString.
#[derive(Debug, Clone)]
pub enum EntityKeyShape {
    /// Numeric or bool or datetime single-key. High 4 bits = variant tag.
    SingleU64(u64),
    /// String single-key. (FxHash of string, original string).
    SingleStr(u64, CompactString),
    /// Compound key (≥2 group_keys columns). Stored as canonical EntityKey.
    Multi(EntityKey),
}

#[derive(Debug, Clone, Copy)]
enum VariantTag {
    I64 = 1,
    F64 = 2,
    Bool = 3,
    Datetime = 4,
}

impl EntityKeyShape {
    /// Build an `EntityKeyShape` from a `Row` by extracting `group_keys` fields.
    ///
    /// Returns `None` if any group-key field is absent, null, NaN (for F64),
    /// or carries an unsupported Value variant.
    pub fn from_row(group_keys: &[String], row: &Row) -> Option<Self> {
        #[cfg(any(test, feature = "test-utils"))]
        EKS_BUILDS.with(|c| c.set(c.get() + 1));

        if group_keys.len() == 1 {
            let key = &group_keys[0];
            match row.get(key.as_str())? {
                Value::Null => None,
                Value::Str(s) => {
                    let hash = Self::hash_str(s.as_str());
                    Some(Self::SingleStr(hash, s.clone()))
                }
                Value::I64(n) => Some(Self::SingleU64(Self::tag_u64(VariantTag::I64, *n as u64))),
                Value::F64(f) => {
                    if f.is_nan() {
                        return None; // NaN → drop event for this agg
                    }
                    Some(Self::SingleU64(Self::tag_u64(VariantTag::F64, f.to_bits())))
                }
                Value::Bool(b) => Some(Self::SingleU64(Self::tag_u64(VariantTag::Bool, *b as u64))),
                Value::Datetime(ms) => Some(Self::SingleU64(Self::tag_u64(
                    VariantTag::Datetime,
                    *ms as u64,
                ))),
                // Bytes/Json/List/Map → drop
                _ => None,
            }
        } else {
            // Compound key: build EntityKey in declaration order (canonical
            // Value::Str pairs, same as EntityKey::from_row).
            let mut pairs: SmallVec<[(CompactString, Value); 2]> =
                SmallVec::with_capacity(group_keys.len());
            for key in group_keys {
                let canonical: CompactString = match row.get(key.as_str()) {
                    None => return None,
                    Some(Value::Null) => return None,
                    Some(Value::Bytes(_) | Value::Json(_) | Value::List(_) | Value::Map(_)) => {
                        return None
                    }
                    Some(Value::Str(s)) => s.clone(),
                    Some(Value::I64(n)) => n.to_string().into(),
                    Some(Value::F64(f)) if f.is_nan() => return None,
                    Some(Value::F64(f)) => format!("{:?}", f).into(),
                    Some(Value::Bool(b)) => b.to_string().into(),
                    Some(Value::Datetime(ms)) => ms.to_string().into(),
                };
                pairs.push((CompactString::from(key.as_str()), Value::Str(canonical)));
            }
            Some(Self::Multi(EntityKey(pairs)))
        }
    }

    #[inline]
    fn hash_str(s: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = fxhash::FxHasher::default();
        s.hash(&mut h);
        h.finish()
    }

    #[inline]
    fn tag_u64(tag: VariantTag, payload: u64) -> u64 {
        // Place tag in high 4 bits; payload in low 60.
        ((tag as u64) << 60) | (payload & 0x0FFF_FFFF_FFFF_FFFF)
    }
}

impl PartialEq for EntityKeyShape {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::SingleU64(a), Self::SingleU64(b)) => a == b,
            // Compare by string content, not hash (hash collision safety).
            (Self::SingleStr(_, a), Self::SingleStr(_, b)) => a == b,
            (Self::Multi(a), Self::Multi(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for EntityKeyShape {}

impl Hash for EntityKeyShape {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::SingleU64(k) => {
                0u8.hash(state);
                k.hash(state);
            }
            Self::SingleStr(h, _) => {
                1u8.hash(state);
                h.hash(state);
            }
            Self::Multi(ek) => {
                2u8.hash(state);
                ek.hash(state);
            }
        }
    }
}

// ─── Test instrument ──────────────────────────────────────────────────────────

#[cfg(any(test, feature = "test-utils"))]
thread_local! {
    static EKS_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Take + reset the thread-local EntityKeyShape::from_row call counter.
/// Used by cluster dispatch tests to verify EntityKey builds per event.
/// Available in unit tests (`#[cfg(test)]`) and integration tests that
/// enable the `test-utils` feature flag.
#[cfg(any(test, feature = "test-utils"))]
pub fn _take_entity_key_build_count() -> usize {
    EKS_BUILDS.with(|c| {
        let n = c.get();
        c.set(0);
        n
    })
}

// ─── StateTables type + helpers ───────────────────────────────────────────────

/// Outer state-tables vector, indexed by `AggregationDescriptor.agg_id`.
///
/// Pure array indexing by `agg_id` at apply time. Server-side register
/// handler resizes via `ensure_capacity_for` after
/// `Registry::apply_registration` so the Vec has at least `next_agg_id` slots.
pub type StateTables = Vec<AggStateTable>;

/// Ensure `state_tables` has at least `min_len` entries, growing with
/// default-initialised `AggStateTable`s if needed.
pub fn ensure_capacity_for(state_tables: &mut StateTables, min_len: usize) {
    if state_tables.len() < min_len {
        state_tables.resize_with(min_len, AggStateTable::new);
    }
}

/// Build a fresh `StateTables` sized to fit the registry's current `agg_id` counter.
pub fn new_state_tables_for(registry: &crate::registry::Registry) -> StateTables {
    let n = registry.next_agg_id() as usize;
    (0..n).map(|_| AggStateTable::new()).collect()
}

/// Test/cold-path helper: look up a table by aggregation node name.
pub fn lookup_table_by_name<'a>(
    state_tables: &'a StateTables,
    registry: &crate::registry::Registry,
    name: &str,
) -> Option<&'a AggStateTable> {
    let agg_id = registry.compiled_aggregation(name)?.agg_id as usize;
    state_tables.get(agg_id)
}

/// Test helper: returns true if a table for `name` exists and is non-empty.
pub fn has_entries_for_name(
    state_tables: &StateTables,
    registry: &crate::registry::Registry,
    name: &str,
) -> bool {
    lookup_table_by_name(state_tables, registry, name)
        .map(|t| t.entity_count() > 0)
        .unwrap_or(false)
}

// ─── AggStateTable ────────────────────────────────────────────────────────────

/// Per-aggregation state store. Plan 19.2-03 (D-03): three specialized storage
/// maps selected by entity-key shape. `get_or_init_by_shape` dispatches to
/// the appropriate sub-map at apply time.
///
/// - `single_u64`: numeric/bool/datetime single-key → `HashMap<u64, Vec<AggOp>>`
/// - `single_str`: string single-key → `HashMap<CompactString, Vec<AggOp>>`
/// - `multi`: compound key (or legacy EntityKey) → `HashMap<EntityKey, Vec<AggOp>>`
///
/// **Snapshot serialization (back-compat):** `iter_sorted` reconstructs
/// canonical `EntityKey` from the three maps, preserving the serialized
/// snapshot format. `insert_from_entity_key` is the recovery-path inverse
/// and restores each key into the same runtime sub-map the apply path uses.
///
/// **Query path:** `query_feature` takes an `EntityKey` (built by the query
/// parser from URL/JSON parameters) and routes through the appropriate map.
pub struct AggStateTable {
    /// Fast numeric / bool / datetime single-key storage. Zero alloc on lookup.
    pub single_u64: HashMap<u64, Vec<AggOp>, FxBuildHasher>,
    /// String single-key storage. FxBuildHasher; hashbrown raw_entry for
    /// zero-clone lookup.
    pub single_str: HashMap<CompactString, Vec<AggOp>, FxBuildHasher>,
    /// Compound key or legacy EntityKey storage.
    /// EntityKey carries manual Hash+Eq impls so this compiles cleanly.
    pub multi: HashMap<EntityKey, Vec<AggOp>, FxBuildHasher>,
    /// group_keys stored at table construction for iter_sorted reconstruction.
    /// Populated by `get_or_init_by_shape` on first call; matches the
    /// descriptor's group_keys.
    pub group_keys: Vec<String>,
    /// Phase 12.8 memory-governance: per-entity last_seen_ms sidecar for
    /// cold-TTL eviction. Mirrors the 3-shape dispatch (single_u64 /
    /// single_str / multi). Populated/updated only when the source has
    /// `EventDescriptor.cold_after_ms = Some(_)` — `None` source skips the
    /// read entirely (zero cost).
    ///
    /// Stored separately from `Vec<AggOp>` to avoid cascading the
    /// (Vec<AggOp>, u64) shape through query_feature / iter_sorted /
    /// snapshot code paths. Sidecar costs ~16 bytes/entity (key
    /// replication + u64 timestamp).
    ///
    /// On eviction (when `now_ms - last_seen_ms > cold_after_ms`), the
    /// entity's Vec<AggOp> is removed via `evict_entity_by_shape_if_cold`
    /// AND its last_seen_ms entry is removed. The next event call's
    /// `get_or_init_by_shape` then allocates a fresh Vec via `init_row`,
    /// realising FRESH state on resurrect (Redis TTL pattern — locked
    /// architectural commitment per Phase 12.8).
    pub last_seen_u64: HashMap<u64, u64, FxBuildHasher>,
    pub last_seen_str: HashMap<CompactString, u64, FxBuildHasher>,
    pub last_seen_multi: HashMap<EntityKey, u64, FxBuildHasher>,
}

impl AggStateTable {
    /// Create an empty table.
    pub fn new() -> Self {
        AggStateTable {
            single_u64: HashMap::with_hasher(FxBuildHasher::default()),
            single_str: HashMap::with_hasher(FxBuildHasher::default()),
            multi: HashMap::with_hasher(FxBuildHasher::default()),
            group_keys: vec![],
            last_seen_u64: HashMap::with_hasher(FxBuildHasher::default()),
            last_seen_str: HashMap::with_hasher(FxBuildHasher::default()),
            last_seen_multi: HashMap::with_hasher(FxBuildHasher::default()),
        }
    }

    /// Look up or initialise the per-entity `Vec<AggOp>` via `EntityKeyShape`
    /// dispatch. This is the HOT PATH.
    ///
    /// - `SingleU64` → `single_u64` HashMap lookup (O(1), no alloc)
    /// - `SingleStr` → `single_str` HashMap lookup (O(1), no alloc for ≤24-byte keys)
    /// - `Multi` → `multi` HashMap lookup (O(1), EntityKey clone only on cold insert)
    pub fn get_or_init_by_shape(
        &mut self,
        shape: &EntityKeyShape,
        desc: &AggregationDescriptor,
    ) -> &mut Vec<AggOp> {
        // Store group_keys for iter_sorted reconstruction (idempotent).
        if self.group_keys.is_empty() && !desc.group_keys.is_empty() {
            self.group_keys = desc.group_keys.clone();
        }

        match shape {
            EntityKeyShape::SingleU64(k) => self
                .single_u64
                .entry(*k)
                .or_insert_with(|| Self::init_row(desc)),
            EntityKeyShape::SingleStr(_, s) => {
                use hashbrown::hash_map::RawEntryMut;
                match self.single_str.raw_entry_mut().from_key(s.as_str()) {
                    RawEntryMut::Occupied(o) => o.into_mut(),
                    RawEntryMut::Vacant(v) => v.insert(s.clone(), Self::init_row(desc)).1,
                }
            }
            EntityKeyShape::Multi(ek) => {
                use hashbrown::hash_map::RawEntryMut;
                match self.multi.raw_entry_mut().from_key(ek) {
                    RawEntryMut::Occupied(o) => o.into_mut(),
                    RawEntryMut::Vacant(v) => v.insert(ek.clone(), Self::init_row(desc)).1,
                }
            }
        }
    }

    /// Legacy API: look up or initialise via an `EntityKey` (used by tests and
    /// backward-compat paths). EntityKey always stores Value::Str-canonical
    /// pairs → routes via `multi`.
    pub fn get_or_init(
        &mut self,
        key: &EntityKey,
        descriptor: &AggregationDescriptor,
    ) -> &mut Vec<AggOp> {
        if self.group_keys.is_empty() && !descriptor.group_keys.is_empty() {
            self.group_keys = descriptor.group_keys.clone();
        }
        use hashbrown::hash_map::RawEntryMut;
        match self.multi.raw_entry_mut().from_key(key) {
            RawEntryMut::Occupied(o) => o.into_mut(),
            RawEntryMut::Vacant(v) => v.insert(key.clone(), Self::init_row(descriptor)).1,
        }
    }

    /// Recovery path: insert a `(EntityKey, Vec<AggOp>)` pair loaded from a
    /// snapshot. Routes through the same sub-map selection as the apply hot
    /// path, so post-snapshot WAL replay updates the restored row instead of
    /// creating a duplicate in a different storage shape.
    pub fn insert_from_entity_key(&mut self, key: EntityKey, ops: Vec<AggOp>) {
        self.insert_from_entity_key_with_types(key, ops, None);
    }

    /// Recovery path with descriptor field types available. Older snapshots
    /// serialized single numeric/bool/datetime keys as `Value::Str`; use the
    /// source schema to coerce those legacy keys back into the hot-path shape.
    pub fn insert_from_entity_key_with_types(
        &mut self,
        key: EntityKey,
        ops: Vec<AggOp>,
        group_key_types: Option<&[FieldType]>,
    ) {
        if self.group_keys.is_empty() {
            self.group_keys = key.0.iter().map(|(name, _)| name.to_string()).collect();
        }

        if key.0.len() == 1 {
            let (_, value) = &key.0[0];
            let value = match (value, group_key_types.and_then(|types| types.first())) {
                (Value::Str(s), Some(FieldType::I64)) => s.parse::<i64>().ok().map(Value::I64),
                (Value::Str(s), Some(FieldType::F64)) => s.parse::<f64>().ok().map(Value::F64),
                (Value::Str(s), Some(FieldType::Bool)) => s.parse::<bool>().ok().map(Value::Bool),
                (Value::Str(s), Some(FieldType::Datetime)) => {
                    s.parse::<i64>().ok().map(Value::Datetime)
                }
                _ => None,
            }
            .unwrap_or_else(|| value.clone());

            match value {
                Value::Str(s) => {
                    self.single_str.insert(s, ops);
                    return;
                }
                Value::I64(n) => {
                    self.single_u64
                        .insert(EntityKeyShape::tag_u64(VariantTag::I64, n as u64), ops);
                    return;
                }
                Value::F64(f) if !f.is_nan() => {
                    self.single_u64
                        .insert(EntityKeyShape::tag_u64(VariantTag::F64, f.to_bits()), ops);
                    return;
                }
                Value::Bool(b) => {
                    self.single_u64
                        .insert(EntityKeyShape::tag_u64(VariantTag::Bool, b as u64), ops);
                    return;
                }
                Value::Datetime(ms) => {
                    self.single_u64.insert(
                        EntityKeyShape::tag_u64(VariantTag::Datetime, ms as u64),
                        ops,
                    );
                    return;
                }
                _ => {}
            }
        }

        self.multi.insert(key, ops);
    }

    /// Query feature `feature_index` for entity `key` at `query_time_ms`.
    ///
    /// Returns `None` if the key is not present or the index is out of range.
    /// The `key` argument uses the legacy `EntityKey` shape (SmallVec of
    /// Value::Str pairs) as built by the query path.
    pub fn query_feature(
        &self,
        key: &EntityKey,
        feature_index: usize,
        query_time_ms: i64,
    ) -> Option<Value> {
        // Route through the correct sub-map based on the EntityKey's shape.
        //
        // The HTTP query path always produces Value::Str pairs (pipe-separated
        // key parsing), so single-key Str queries look up in single_str.
        // The test path builds EntityKey with the actual typed Value, so we
        // must also handle I64/F64/Bool/Datetime via single_u64.
        // Compound keys (len > 1) always go through multi.
        let ops: &Vec<AggOp> = if key.0.len() == 1 {
            let (_field_name, val) = &key.0[0];
            match val {
                Value::Str(s) => {
                    // single_str is the primary path for string-typed group keys.
                    // Fall back to multi for legacy entries inserted before
                    // snapshot recovery restored keys into their hot-path shape.
                    if let Some(ops) = self.single_str.get(s) {
                        ops
                    } else {
                        self.multi.get(key)?
                    }
                }
                Value::I64(n) => {
                    let k = EntityKeyShape::tag_u64(VariantTag::I64, *n as u64);
                    self.single_u64.get(&k)?
                }
                Value::F64(f) => {
                    let k = EntityKeyShape::tag_u64(VariantTag::F64, f.to_bits());
                    self.single_u64.get(&k)?
                }
                Value::Bool(b) => {
                    let k = EntityKeyShape::tag_u64(VariantTag::Bool, *b as u64);
                    self.single_u64.get(&k)?
                }
                Value::Datetime(ms) => {
                    let k = EntityKeyShape::tag_u64(VariantTag::Datetime, *ms as u64);
                    self.single_u64.get(&k)?
                }
                Value::Null => return None,
                _ => self.multi.get(key)?,
            }
        } else {
            self.multi.get(key)?
        };
        ops.get(feature_index).map(|op| op.query(query_time_ms))
    }

    /// Return the number of distinct entities across all three sub-maps.
    ///
    /// Also used by the `/metrics` admin sidecar to populate
    /// `beava_entity_count_resident`. The apply path sums this across all
    /// state tables and writes the total into a process-static `AtomicU64`
    /// snapshot the admin handler reads with `.load(Relaxed)` — zero-lock
    /// metric exposition.
    pub fn entity_count(&self) -> usize {
        self.single_u64.len() + self.single_str.len() + self.multi.len()
    }

    /// Phase 12.8 memory-governance: cold-TTL eviction check.
    ///
    /// Reads the entity's `last_seen_ms` (from the appropriate sidecar map);
    /// if older than `now_ms - cold_after_ms`, REMOVES the entity from the
    /// state map (Vec<AggOp> dropped) AND removes the last_seen_ms entry,
    /// returning `true`. Returns `false` if entity is warm or absent (first
    /// event for entity).
    ///
    /// Caller MUST update `last_seen_ms` via `record_last_seen_by_shape` after
    /// the apply call (regardless of whether eviction fired). This is what
    /// keeps the sidecar tracking the most recent event per entity.
    ///
    /// Redis TTL semantics (locked architectural commitment): on resurrect
    /// the entity is treated as fresh — no partial-state preservation. The
    /// `get_or_init_by_shape` call after eviction allocates a new
    /// `Vec<AggOp>` via `init_row`.
    ///
    /// `now_ms.saturating_sub(last_seen)` is used so the comparison is robust
    /// to wall-clock skew (a `last_seen` value greater than `now_ms` would
    /// underflow without `saturating_sub`).
    pub fn evict_entity_by_shape_if_cold(
        &mut self,
        shape: &EntityKeyShape,
        now_ms: u64,
        cold_after_ms: u64,
    ) -> bool {
        // Read last_seen_ms for this shape; bail if absent (first event for
        // this entity — no eviction needed).
        let last_seen_ms = match shape {
            EntityKeyShape::SingleU64(k) => self.last_seen_u64.get(k).copied(),
            EntityKeyShape::SingleStr(_, s) => self.last_seen_str.get(s.as_str()).copied(),
            EntityKeyShape::Multi(ek) => self.last_seen_multi.get(ek).copied(),
        };
        let last_seen = match last_seen_ms {
            Some(t) => t,
            None => return false,
        };
        // Check cold threshold. saturating_sub handles wall-clock skew.
        if now_ms.saturating_sub(last_seen) <= cold_after_ms {
            return false; // warm
        }
        // COLD — remove the Vec<AggOp> and last_seen entry. Caller's
        // get_or_init_by_shape will allocate a fresh Vec for the new event.
        match shape {
            EntityKeyShape::SingleU64(k) => {
                self.single_u64.remove(k);
                self.last_seen_u64.remove(k);
            }
            EntityKeyShape::SingleStr(_, s) => {
                self.single_str.remove(s.as_str());
                self.last_seen_str.remove(s.as_str());
            }
            EntityKeyShape::Multi(ek) => {
                self.multi.remove(ek);
                self.last_seen_multi.remove(ek);
            }
        }
        true
    }

    /// Record the wall-clock arrival time for this entity. Called from the
    /// apply path AFTER `apply_event_to_aggregations` — only when source
    /// has `cold_after_ms = Some(_)`.
    ///
    /// Uses `raw_entry_mut` for the str/multi shapes to avoid an extra clone
    /// of the key when the entry is already present (the common warm path).
    pub fn record_last_seen_by_shape(&mut self, shape: &EntityKeyShape, now_ms: u64) {
        match shape {
            EntityKeyShape::SingleU64(k) => {
                self.last_seen_u64.insert(*k, now_ms);
            }
            EntityKeyShape::SingleStr(_, s) => {
                use hashbrown::hash_map::RawEntryMut;
                match self.last_seen_str.raw_entry_mut().from_key(s.as_str()) {
                    RawEntryMut::Occupied(mut o) => {
                        *o.get_mut() = now_ms;
                    }
                    RawEntryMut::Vacant(v) => {
                        v.insert(s.clone(), now_ms);
                    }
                }
            }
            EntityKeyShape::Multi(ek) => {
                use hashbrown::hash_map::RawEntryMut;
                match self.last_seen_multi.raw_entry_mut().from_key(ek) {
                    RawEntryMut::Occupied(mut o) => {
                        *o.get_mut() = now_ms;
                    }
                    RawEntryMut::Vacant(v) => {
                        v.insert(ek.clone(), now_ms);
                    }
                }
            }
        }
    }

    /// Iterate entries in EntityKey-sorted order for snapshot serialization.
    ///
    /// Reconstructs canonical `EntityKey` from single_u64 and single_str entries
    /// using stored `group_keys`. Multi entries already hold an EntityKey.
    pub fn iter_sorted(&self) -> impl Iterator<Item = (EntityKey, &Vec<AggOp>)> {
        let mut entries: Vec<(EntityKey, &Vec<AggOp>)> = Vec::new();

        let key_name = self.group_keys.first().cloned().unwrap_or_default();

        // Reconstruct EntityKey from single_u64 entries. Preserve the typed
        // Value variant so snapshot load restores into single_u64 again.
        for (k, v) in &self.single_u64 {
            let tag = k >> 60;
            let payload = k & 0x0FFF_FFFF_FFFF_FFFF;
            let value = match tag {
                1 => Value::I64(payload as i64),
                2 => {
                    // F64: restore bits; upper 4 bits were replaced by tag=2.
                    Value::F64(f64::from_bits(payload | (2u64 << 60)))
                }
                3 => Value::Bool(payload != 0),
                4 => Value::Datetime(payload as i64),
                _ => Value::Str("unknown".into()),
            };
            let mut pairs: SmallVec<[(CompactString, Value); 2]> = SmallVec::new();
            pairs.push((CompactString::from(key_name.as_str()), value));
            entries.push((EntityKey(pairs), v));
        }

        // Reconstruct EntityKey from single_str entries.
        for (s, v) in &self.single_str {
            let mut pairs: SmallVec<[(CompactString, Value); 2]> = SmallVec::new();
            pairs.push((
                CompactString::from(key_name.as_str()),
                Value::Str(s.clone()),
            ));
            entries.push((EntityKey(pairs), v));
        }

        // Multi entries: EntityKey IS the key.
        for (ek, v) in &self.multi {
            entries.push((ek.clone(), v));
        }

        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        entries.into_iter()
    }

    fn init_row(desc: &AggregationDescriptor) -> Vec<AggOp> {
        desc.features
            .iter()
            .map(|f| AggOp::new(&f.descriptor))
            .collect()
    }
}

impl Default for AggStateTable {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
    use crate::agg_op::{AggKind, AggOpDescriptor};
    use crate::row::{Row, Value};

    fn make_user_key(user_id: &str) -> EntityKey {
        let pair: (CompactString, Value) = ("user_id".into(), Value::Str(user_id.into()));
        EntityKey(SmallVec::from_buf_and_len(
            [pair, ("".into(), Value::Null)],
            1,
        ))
    }

    fn make_user_key_from_i64(n: i64) -> EntityKey {
        let pair: (CompactString, Value) = ("user_id".into(), Value::Str(n.to_string().into()));
        EntityKey(SmallVec::from_buf_and_len(
            [pair, ("".into(), Value::Null)],
            1,
        ))
    }

    fn count_op_desc() -> AggOpDescriptor {
        AggOpDescriptor {
            kind: AggKind::Count,
            field: None,
            window_ms: None,
            where_expr: None,
            n: None,
            half_life_ms: None,
            sub_window_ms: None,
            sigma: None,
            sketch_params: None,
            ext: Default::default(),
            field_idx: crate::agg_op::FIELD_IDX_NONE,
            field_idx_into_event_extracted: Vec::new(),
        }
    }

    fn sum_op_desc(field: &str) -> AggOpDescriptor {
        AggOpDescriptor {
            kind: AggKind::Sum,
            field: Some(field.to_string()),
            window_ms: None,
            where_expr: None,
            n: None,
            half_life_ms: None,
            sub_window_ms: None,
            sigma: None,
            sketch_params: None,
            ext: Default::default(),
            field_idx: crate::agg_op::FIELD_IDX_NONE,
            field_idx_into_event_extracted: Vec::new(),
        }
    }

    fn make_descriptor(
        node_name: &str,
        source: &str,
        keys: &[&str],
        features: &[(&str, AggOpDescriptor)],
    ) -> AggregationDescriptor {
        AggregationDescriptor {
            node_name: node_name.to_string(),
            source_node_name: source.to_string(),
            group_keys: keys.iter().map(|k| k.to_string()).collect(),
            features: features
                .iter()
                .map(|(name, d)| NamedAggOp {
                    feature_name: name.to_string(),
                    descriptor: d.clone(),
                })
                .collect(),
            agg_id: 0,
            field_names: vec![],
            cluster_id: 0,
        }
    }

    #[test]
    fn entity_key_from_row_extracts_group_keys_in_order() {
        let keys = vec!["user_id".to_string(), "merchant_id".to_string()];
        let row = Row::new()
            .with_field("user_id", Value::Str("a".into()))
            .with_field("merchant_id", Value::Str("m1".into()))
            .with_field("amount", Value::F64(10.0));
        let ek = EntityKey::from_row(&keys, &row).expect("should succeed");
        let expected: SmallVec<[(CompactString, Value); 2]> = SmallVec::from_buf([
            ("user_id".into(), Value::Str("a".into())),
            ("merchant_id".into(), Value::Str("m1".into())),
        ]);
        assert_eq!(ek, EntityKey(expected));
    }

    #[test]
    fn entity_key_from_row_returns_none_on_null_field() {
        let keys = vec!["user_id".to_string()];
        let row = Row::new().with_field("user_id", Value::Null);
        assert!(EntityKey::from_row(&keys, &row).is_none());
    }

    #[test]
    fn entity_key_from_row_returns_none_on_missing_field() {
        let keys = vec!["user_id".to_string()];
        let row = Row::new();
        assert!(EntityKey::from_row(&keys, &row).is_none());
    }

    #[test]
    fn entity_key_normalises_numeric_values_deterministically() {
        let keys = vec!["id".to_string()];
        let row_i64 = Row::new().with_field("id", Value::I64(42));
        let row_f64 = Row::new().with_field("id", Value::F64(42.0));
        let ek_i = EntityKey::from_row(&keys, &row_i64).expect("I64 key");
        let ek_f = EntityKey::from_row(&keys, &row_f64).expect("F64 key");
        assert_ne!(ek_i, ek_f);
        let ek_i2 = EntityKey::from_row(&keys, &row_i64).expect("I64 key again");
        assert_eq!(ek_i, ek_i2);
    }

    #[test]
    fn entity_key_returns_none_for_bytes_value() {
        let keys = vec!["id".to_string()];
        let row = Row::new().with_field("id", Value::Bytes(vec![0x01, 0x02]));
        assert!(EntityKey::from_row(&keys, &row).is_none());
    }

    #[test]
    fn agg_state_table_get_or_init_creates_row_of_correct_arity() {
        let desc = make_descriptor(
            "MyAgg",
            "Txn",
            &["user_id"],
            &[
                ("cnt", count_op_desc()),
                ("total", sum_op_desc("amount")),
                ("cnt2", count_op_desc()),
            ],
        );
        let mut table = AggStateTable::new();
        let key = make_user_key("alice");
        let row = table.get_or_init(&key, &desc);
        assert_eq!(row.len(), 3);
    }

    #[test]
    fn agg_state_table_get_or_init_returns_existing_on_repeat() {
        let desc = make_descriptor("A", "S", &["user_id"], &[("cnt", count_op_desc())]);
        let mut table = AggStateTable::new();
        let key = make_user_key("alice");
        {
            let row = table.get_or_init(&key, &desc);
            row[0].update(&Row::new(), 0, None, true);
        }
        {
            let row2 = table.get_or_init(&key, &desc);
            assert_eq!(row2[0].query(0), Value::I64(1));
        }
    }

    #[test]
    fn insert_from_entity_key_restores_single_str_hot_shape() {
        let desc = make_descriptor("A", "S", &["user_id"], &[("cnt", count_op_desc())]);
        let mut table = AggStateTable::new();
        table.insert_from_entity_key(
            make_user_key("alice"),
            vec![AggOp::Count(crate::agg_state::CountState { n: 5 })],
        );

        assert_eq!(table.single_str.len(), 1);
        assert!(table.multi.is_empty());

        let event_row = Row::new().with_field("user_id", Value::Str("alice".into()));
        let shape = EntityKeyShape::from_row(&desc.group_keys, &event_row).expect("shape");
        let ops = table.get_or_init_by_shape(&shape, &desc);
        ops[0].update(&event_row, 0, None, true);

        assert_eq!(ops[0].query(0), Value::I64(6));
    }

    #[test]
    fn snapshot_roundtrip_restores_single_u64_hot_shape() {
        let desc = make_descriptor("A", "S", &["user_id"], &[("cnt", count_op_desc())]);
        let event_row = Row::new().with_field("user_id", Value::I64(42));
        let shape = EntityKeyShape::from_row(&desc.group_keys, &event_row).expect("shape");

        let mut table = AggStateTable::new();
        {
            let ops = table.get_or_init_by_shape(&shape, &desc);
            ops[0].update(&event_row, 0, None, true);
        }

        let mut restored = AggStateTable::new();
        for (key, ops) in table.iter_sorted() {
            restored.insert_from_entity_key(key, ops.clone());
        }

        assert_eq!(restored.single_u64.len(), 1);
        assert!(restored.single_str.is_empty());
        assert!(restored.multi.is_empty());

        let ops = restored.get_or_init_by_shape(&shape, &desc);
        ops[0].update(&event_row, 0, None, true);
        assert_eq!(ops[0].query(0), Value::I64(2));
    }

    #[test]
    fn insert_from_entity_key_with_types_coerces_legacy_i64_string() {
        let desc = make_descriptor("A", "S", &["user_id"], &[("cnt", count_op_desc())]);
        let mut table = AggStateTable::new();
        table.insert_from_entity_key_with_types(
            make_user_key_from_i64(42),
            vec![AggOp::Count(crate::agg_state::CountState { n: 5 })],
            Some(&[FieldType::I64]),
        );

        assert_eq!(table.single_u64.len(), 1);
        assert!(table.single_str.is_empty());

        let event_row = Row::new().with_field("user_id", Value::I64(42));
        let shape = EntityKeyShape::from_row(&desc.group_keys, &event_row).expect("shape");
        let ops = table.get_or_init_by_shape(&shape, &desc);
        ops[0].update(&event_row, 0, None, true);
        assert_eq!(ops[0].query(0), Value::I64(6));
    }

    #[test]
    fn agg_state_table_entity_count_counts_distinct_keys() {
        let desc = make_descriptor("A", "S", &["user_id"], &[("cnt", count_op_desc())]);
        let mut table = AggStateTable::new();
        for i in 0..5 {
            let key = make_user_key_from_i64(i);
            table.get_or_init(&key, &desc);
        }
        assert_eq!(table.entity_count(), 5);
    }

    #[test]
    fn agg_state_table_query_feature_returns_value() {
        let desc = make_descriptor("A", "S", &["user_id"], &[("cnt", count_op_desc())]);
        let mut table = AggStateTable::new();
        let key = make_user_key("alice");
        {
            let row = table.get_or_init(&key, &desc);
            for _ in 0..3 {
                row[0].update(&Row::new(), 0, None, true);
            }
        }
        let val = table.query_feature(&key, 0, 0);
        assert_eq!(val, Some(Value::I64(3)));
    }

    #[test]
    fn agg_state_table_query_feature_returns_none_for_unknown_key() {
        let desc = make_descriptor("A", "S", &["user_id"], &[("cnt", count_op_desc())]);
        let table = AggStateTable::new();
        let key = make_user_key("unknown");
        let _ = desc;
        assert!(table.query_feature(&key, 0, 0).is_none());
    }

    #[test]
    fn agg_state_table_iter_sorted_byte_identical_for_same_inputs() {
        let desc = make_descriptor("A", "S", &["user_id"], &[("cnt", count_op_desc())]);
        let users = ["alice", "bob", "carol", "dan"];
        let mut t1 = AggStateTable::new();
        let mut t2 = AggStateTable::new();
        for u in users {
            t1.get_or_init(&make_user_key(u), &desc);
            t2.get_or_init(&make_user_key(u), &desc);
        }
        let view1: Vec<EntityKey> = t1.iter_sorted().map(|(k, _)| k).collect();
        let view2: Vec<EntityKey> = t2.iter_sorted().map(|(k, _)| k).collect();
        assert_eq!(view1, view2);
        let names: Vec<String> = view1
            .iter()
            .map(|k| match &k.0[0].1 {
                Value::Str(s) => s.to_string(),
                _ => panic!("expected Value::Str"),
            })
            .collect();
        assert_eq!(names, vec!["alice", "bob", "carol", "dan"]);
    }

    #[test]
    fn entity_key_smallvec_inline() {
        use compact_str::CompactString;
        use smallvec::SmallVec;

        let pair: (CompactString, Value) = ("user_id".into(), Value::Str("alice".into()));
        let inline_storage: SmallVec<[(CompactString, Value); 2]> =
            SmallVec::from_buf_and_len([pair, ("".into(), Value::Null)], 1);
        let ek = EntityKey(inline_storage);
        assert!(!ek.0.spilled());
        assert_eq!(ek.0.len(), 1);

        let pair_a: (CompactString, Value) = ("user_id".into(), Value::Str("a".into()));
        let pair_b: (CompactString, Value) = ("merchant_id".into(), Value::Str("m1".into()));
        let two: SmallVec<[(CompactString, Value); 2]> = SmallVec::from_buf([pair_a, pair_b]);
        let ek2 = EntityKey(two);
        assert!(!ek2.0.spilled());
    }

    #[test]
    fn agg_state_table_uses_hashbrown_multi_map() {
        let table: AggStateTable = AggStateTable::new();
        let typename = std::any::type_name_of_val(&table.multi);
        assert!(
            typename.contains("hashbrown") && typename.contains("HashMap"),
            "AggStateTable.multi must be hashbrown::HashMap, got {}",
            typename
        );
    }

    #[test]
    fn agg_state_table_iter_sorted_is_deterministic() {
        let desc = make_descriptor("A", "S", &["user_id"], &[("cnt", count_op_desc())]);
        let mut table = AggStateTable::new();
        for u in ["zebra", "alice", "monkey", "bob"] {
            let key = make_user_key(u);
            table.get_or_init(&key, &desc);
        }
        let sorted_users: Vec<String> = table
            .iter_sorted()
            .map(|(k, _)| match &k.0[0].1 {
                Value::Str(s) => s.to_string(),
                other => panic!("expected Value::Str, got {:?}", other),
            })
            .collect();
        assert_eq!(sorted_users, vec!["alice", "bob", "monkey", "zebra"]);
    }

    #[test]
    fn entity_key_from_row_yields_canonicalised_str_pairs() {
        let keys = vec!["user_id".to_string()];
        let row_i64 = Row::new().with_field("user_id", Value::I64(42));
        let row_f64 = Row::new().with_field("user_id", Value::F64(42.0));
        let ek_i = EntityKey::from_row(&keys, &row_i64).expect("I64 EntityKey");
        let ek_f = EntityKey::from_row(&keys, &row_f64).expect("F64 EntityKey");
        assert_ne!(ek_i, ek_f);
        match &ek_i.0[0].1 {
            Value::Str(s) if s.as_str() == "42" => {}
            other => panic!("expected Value::Str(\"42\"), got {:?}", other),
        }
        match &ek_f.0[0].1 {
            Value::Str(s) if s.as_str() == "42.0" => {}
            other => panic!("expected Value::Str(\"42.0\"), got {:?}", other),
        }
    }
}
