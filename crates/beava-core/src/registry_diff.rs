//! Registry diff engine: pure function that classifies each node in a registration
//! payload as `added`, `already_present`, or `changed` relative to the current registry.
//!
//! Additive-only rule: descriptors can be added but never mutated or removed.
//! Any deviation from an existing descriptor's content (excluding `registered_at_version`)
//! is a conflict.

use crate::registry::{DerivationDescriptor, EventDescriptor, RegistryInner, TableDescriptor};
use serde::{Deserialize, Serialize};

// ─── PayloadNode ──────────────────────────────────────────────────────────────

/// A parsed node from the `POST /register` payload, after `kind` discrimination but
/// before `registered_at_version` assignment. The HTTP handler (Plan 05) produces these.
/// The validation pass (Plan 04) guarantees each descriptor is structurally valid
/// before `compute_diff` runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PayloadNode {
    Event(EventDescriptor),
    Table(TableDescriptor),
    Derivation(DerivationDescriptor),
}

impl PayloadNode {
    pub fn name(&self) -> &str {
        match self {
            Self::Event(e) => &e.name,
            Self::Table(t) => &t.name,
            Self::Derivation(d) => &d.name,
        }
    }

    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Event(_) => "event",
            Self::Table(_) => "table",
            Self::Derivation(_) => "derivation",
        }
    }
}

// ─── Diff output types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegistryDiff {
    pub added: Vec<String>,
    pub already_present: Vec<String>,
    pub changed: Vec<ConflictDetail>,
}

// ─── Phase 13.4 Plan 06 — D-01 categorized diff types ─────────────────────────
//
// `RegisterDiff` + `DiffEntry` are the categorized-lists payload format
// emitted by Phase 13.4 Plan 06's `force_required` 409 response. The legacy
// `RegistryDiff` (above) stays in place for the existing
// `registration_conflict` envelope; D-01's new pathway adds these types
// alongside without disturbing the existing one (additive change to
// preserve back-compat for ~30 phase2/4/5 tests that already assert on the
// legacy shape).
//
// Per D-01 (USER-LOCKED):
//   - Destructive variants require `force=true` to apply: rename, type-change,
//     op removal, agg removal, window-change, key-cols change.
//   - Additive variants apply without force: new descriptor, new agg in
//     existing block, new field on event source.
//
// JSON shape: `{"additive": [...], "destructive": [...]}` — categorized lists,
// NOT JSON-Patch. Each entry uses `{"kind": "<class>", ...class-specific...}`
// internally-tagged serde representation. `from`/`to` carry the prior and
// proposed values for clearly-paired changes (rename, type_change,
// window_change, key_cols_change).

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterDiff {
    pub additive: Vec<DiffEntry>,
    pub destructive: Vec<DiffEntry>,
    /// Names of payload descriptors that exist in the current registry
    /// with byte-equivalent shape (modulo `registered_at_version`). The
    /// register response surfaces this list to callers so they can tell
    /// "you sent this exact descriptor, I already had it" apart from
    /// "you added this for the first time."
    ///
    /// Populated by `classify_register_diff`. Phase2 wire shape (asserted
    /// in `phase2_smoke.rs`) preserved when this struct displaces the
    /// legacy `RegistryDiff::already_present` field.
    #[serde(default)]
    pub already_present: Vec<String>,
}

impl RegisterDiff {
    pub fn empty() -> Self {
        Self {
            additive: Vec::new(),
            destructive: Vec::new(),
            already_present: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffEntry {
    // ── Destructive variants (require force=true) ──────────────────────────
    /// Rename: a descriptor name disappeared; another descriptor with
    /// matching shape but different name appeared in the same payload.
    Rename { from: String, to: String },
    /// TypeChange: a field's declared type changed within an existing
    /// event / table / derivation schema. `field` is `<descriptor>.<field>`.
    TypeChange {
        field: String,
        from: String,
        to: String,
    },
    /// OpRemoval: an op was removed from a derivation's `ops` chain (e.g.,
    /// shrinking the chain length, deleting a `group_by` step).
    OpRemoval { table: String, agg: String },
    /// AggRemoval: an aggregation feature was removed from an existing
    /// `agg` block within a derivation's group_by op.
    AggRemoval { table: String, agg: String },
    /// WindowChange: an aggregation feature's `window=` kwarg changed
    /// (different bucket → existing accumulated state is incompatible).
    WindowChange {
        agg: String,
        from: String,
        to: String,
    },
    /// KeyColsChange: group_by keys (or table primary_key) changed.
    KeyColsChange {
        table: String,
        from: Vec<String>,
        to: Vec<String>,
    },
    // ── Additive variants (allowed without force) ──────────────────────────
    /// NewDescriptor: a brand new descriptor (event / table / derivation)
    /// added to the registry. `kind` here is the descriptor kind label.
    NewDescriptor {
        descriptor_kind: String,
        name: String,
    },
    /// NewAgg: a new aggregation feature added inside an EXISTING `agg`
    /// block (same derivation name, additional features).
    NewAgg {
        table: String,
        agg: String,
        source: String,
    },
    /// NewField: a new field added to an existing event source schema.
    NewField {
        event: String,
        field: String,
        #[serde(rename = "type")]
        type_: String,
    },
}

impl DiffEntry {
    /// Sort key: `(kind_label, primary_field, secondary_field)`. Used for
    /// idempotent JSON output (Phase 13.4 Plan 06 Task 6.d Test 4) — two
    /// runs of `classify_register_diff` against the same inputs produce
    /// byte-identical JSON output.
    pub fn sort_key(&self) -> (u8, String, String) {
        match self {
            DiffEntry::Rename { from, to } => (0, from.clone(), to.clone()),
            DiffEntry::TypeChange { field, from, to } => {
                (1, field.clone(), format!("{from}->{to}"))
            }
            DiffEntry::OpRemoval { table, agg } => (2, table.clone(), agg.clone()),
            DiffEntry::AggRemoval { table, agg } => (3, table.clone(), agg.clone()),
            DiffEntry::WindowChange { agg, from, to } => (4, agg.clone(), format!("{from}->{to}")),
            DiffEntry::KeyColsChange { table, from, to } => {
                (5, table.clone(), format!("{from:?}->{to:?}"))
            }
            DiffEntry::NewDescriptor {
                descriptor_kind,
                name,
            } => (6, descriptor_kind.clone(), name.clone()),
            DiffEntry::NewAgg { table, agg, source } => {
                (7, table.clone(), format!("{agg}@{source}"))
            }
            DiffEntry::NewField {
                event,
                field,
                type_,
            } => (8, event.clone(), format!("{field}:{type_}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConflictDetail {
    pub name: String,
    pub reason: DiffReason,
    /// Human-readable explanation, safe to include in 409 body. NOT a stable wire contract
    /// (use `reason` enum for machine-readable signal). Tests assert `.contains(...)`.
    pub details: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffReason {
    KindMismatch,
    SchemaMismatch,
    /// Pre-pivot variant — emitted when an
    /// `event_time_field` value differed between current and submitted
    /// EventDescriptor. The diff classifier no longer raises this; the field
    /// is gone from EventDescriptor. Variant kept for wire-codec stability.
    // reason: wire-codec stability — discriminant retained per Phase 12.7
    // events-only pivot; never constructed at runtime.
    #[allow(dead_code)]
    EventTimeFieldMismatch,
    PrimaryKeyMismatch,
    TtlMismatch,
    DedupeKeyMismatch,
    OpsMismatch,
    UpstreamsMismatch,
    OutputKindMismatch,
    TableModeMismatch,
    TablePrimaryKeyMismatch,
    OtherFieldMismatch,
}

// ─── Core diff function ───────────────────────────────────────────────────────

/// Pure function: given current registry state and a validated payload, classify each node.
/// Input order of `payload` is preserved in all output vectors.
///
/// Does NOT mutate `current`. Thread-safe: called with a snapshot clone.
pub fn compute_diff(current: &RegistryInner, payload: &[PayloadNode]) -> RegistryDiff {
    let mut added = Vec::new();
    let mut already_present = Vec::new();
    let mut changed = Vec::new();

    for node in payload {
        let name = node.name();

        // Check if name exists in any kind map in current registry
        if let Some(existing) = current.events.get(name) {
            match node {
                PayloadNode::Event(submitted) => {
                    if existing.equiv_ignoring_version(submitted) {
                        already_present.push(name.to_string());
                    } else {
                        let (reason, details) = classify_event_diff(existing, submitted);
                        changed.push(ConflictDetail {
                            name: name.to_string(),
                            reason,
                            details,
                        });
                    }
                }
                _ => {
                    changed.push(ConflictDetail {
                        name: name.to_string(),
                        reason: DiffReason::KindMismatch,
                        details: format!("expected kind 'event', got kind '{}'", node.kind_str()),
                    });
                }
            }
        } else if let Some(existing) = current.tables.get(name) {
            match node {
                PayloadNode::Table(submitted) => {
                    if existing.equiv_ignoring_version(submitted) {
                        already_present.push(name.to_string());
                    } else {
                        let (reason, details) = classify_table_diff(existing, submitted);
                        changed.push(ConflictDetail {
                            name: name.to_string(),
                            reason,
                            details,
                        });
                    }
                }
                _ => {
                    changed.push(ConflictDetail {
                        name: name.to_string(),
                        reason: DiffReason::KindMismatch,
                        details: format!("expected kind 'table', got kind '{}'", node.kind_str()),
                    });
                }
            }
        } else if let Some(existing) = current.derivations.get(name) {
            match node {
                PayloadNode::Derivation(submitted) => {
                    if existing.equiv_ignoring_version(submitted) {
                        already_present.push(name.to_string());
                    } else {
                        let (reason, details) = classify_derivation_diff(existing, submitted);
                        changed.push(ConflictDetail {
                            name: name.to_string(),
                            reason,
                            details,
                        });
                    }
                }
                _ => {
                    changed.push(ConflictDetail {
                        name: name.to_string(),
                        reason: DiffReason::KindMismatch,
                        details: format!(
                            "expected kind 'derivation', got kind '{}'",
                            node.kind_str()
                        ),
                    });
                }
            }
        } else {
            // Not found in any map — newly added
            added.push(name.to_string());
        }
    }

    RegistryDiff {
        added,
        already_present,
        changed,
    }
}

// ─── Reason classifiers ───────────────────────────────────────────────────────

fn classify_event_diff(
    current: &EventDescriptor,
    submitted: &EventDescriptor,
) -> (DiffReason, String) {
    // Priority order: schema → dedupe_key → ttl fields → fallback
    //
    // `event_time_field` and `tolerate_delay_ms` were removed (events-only,
    // were deleted from EventDescriptor — the diff routine no longer compares
    // them (they aren't on the struct).  Stale fixtures get rejected at the
    // JSON-prelude layer before reaching this classifier.

    if let Some(detail) = describe_field_diff_event(&current.schema, &submitted.schema) {
        return (DiffReason::SchemaMismatch, detail);
    }

    if current.dedupe_key != submitted.dedupe_key {
        return (
            DiffReason::DedupeKeyMismatch,
            format!(
                "dedupe_key changed from {:?} to {:?}",
                current.dedupe_key, submitted.dedupe_key
            ),
        );
    }

    if current.dedupe_window_ms != submitted.dedupe_window_ms {
        return (
            DiffReason::TtlMismatch,
            format!(
                "dedupe_window_ms changed from {:?} to {:?}",
                current.dedupe_window_ms, submitted.dedupe_window_ms
            ),
        );
    }

    if current.keep_events_for_ms != submitted.keep_events_for_ms {
        return (
            DiffReason::TtlMismatch,
            format!(
                "keep_events_for_ms changed from {:?} to {:?}",
                current.keep_events_for_ms, submitted.keep_events_for_ms
            ),
        );
    }

    (
        DiffReason::OtherFieldMismatch,
        "descriptors differ in an unclassified field".to_string(),
    )
}

fn classify_table_diff(
    current: &TableDescriptor,
    submitted: &TableDescriptor,
) -> (DiffReason, String) {
    if let Some(detail) = describe_field_diff_table(&current.schema, &submitted.schema) {
        return (DiffReason::SchemaMismatch, detail);
    }

    if current.primary_key != submitted.primary_key {
        return (
            DiffReason::PrimaryKeyMismatch,
            format!(
                "primary_key changed from {:?} to {:?}",
                current.primary_key, submitted.primary_key
            ),
        );
    }

    if current.ttl_ms != submitted.ttl_ms {
        return (
            DiffReason::TtlMismatch,
            format!(
                "ttl_ms changed from {:?} to {:?}",
                current.ttl_ms, submitted.ttl_ms
            ),
        );
    }

    if current.mode != submitted.mode {
        return (
            DiffReason::TableModeMismatch,
            "table mode changed".to_string(),
        );
    }

    (
        DiffReason::OtherFieldMismatch,
        "table descriptors differ in an unclassified field".to_string(),
    )
}

fn classify_derivation_diff(
    current: &DerivationDescriptor,
    submitted: &DerivationDescriptor,
) -> (DiffReason, String) {
    if current.output_kind != submitted.output_kind {
        return (
            DiffReason::OutputKindMismatch,
            format!(
                "output_kind changed from '{:?}' to '{:?}'",
                current.output_kind, submitted.output_kind
            ),
        );
    }

    if current.upstreams != submitted.upstreams {
        return (
            DiffReason::UpstreamsMismatch,
            format!(
                "upstreams changed from {:?} to {:?}",
                current.upstreams, submitted.upstreams
            ),
        );
    }

    if current.ops != submitted.ops {
        return (
            DiffReason::OpsMismatch,
            "derivation ops list changed".to_string(),
        );
    }

    if let Some(detail) = describe_field_diff_derived(&current.schema, &submitted.schema) {
        return (DiffReason::SchemaMismatch, detail);
    }

    if current.table_primary_key != submitted.table_primary_key {
        return (
            DiffReason::TablePrimaryKeyMismatch,
            format!(
                "table_primary_key changed from {:?} to {:?}",
                current.table_primary_key, submitted.table_primary_key
            ),
        );
    }

    (
        DiffReason::OtherFieldMismatch,
        "derivation descriptors differ in an unclassified field".to_string(),
    )
}

// ─── Schema diff helpers ──────────────────────────────────────────────────────

use crate::schema::{DerivedSchema, EventSchema, FieldType, TableSchema};
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Arc;

fn describe_schema_diff(
    a_fields: &BTreeMap<String, FieldType>,
    b_fields: &BTreeMap<String, FieldType>,
    a_optional: &[String],
    b_optional: &[String],
) -> Option<String> {
    // Check for first differing field (BTreeMap iteration is sorted — deterministic)
    for (key, a_type) in a_fields {
        match b_fields.get(key) {
            None => {
                return Some(format!("field '{key}' removed"));
            }
            Some(b_type) if b_type != a_type => {
                return Some(format!(
                    "field '{key}' type changed from {a_type:?} to {b_type:?}"
                ));
            }
            _ => {}
        }
    }
    for key in b_fields.keys() {
        if !a_fields.contains_key(key) {
            return Some(format!("field '{key}' added"));
        }
    }

    // Check optional_fields sets
    let a_opt: std::collections::BTreeSet<&str> = a_optional.iter().map(|s| s.as_str()).collect();
    let b_opt: std::collections::BTreeSet<&str> = b_optional.iter().map(|s| s.as_str()).collect();
    if a_opt != b_opt {
        let added: Vec<&str> = b_opt.difference(&a_opt).copied().collect();
        let removed: Vec<&str> = a_opt.difference(&b_opt).copied().collect();
        return Some(format!(
            "optional_fields changed: added={added:?} removed={removed:?}"
        ));
    }

    None
}

fn describe_field_diff_event(a: &EventSchema, b: &EventSchema) -> Option<String> {
    describe_schema_diff(&a.fields, &b.fields, &a.optional_fields, &b.optional_fields)
}

fn describe_field_diff_table(a: &TableSchema, b: &TableSchema) -> Option<String> {
    describe_schema_diff(&a.fields, &b.fields, &a.optional_fields, &b.optional_fields)
}

fn describe_field_diff_derived(a: &DerivedSchema, b: &DerivedSchema) -> Option<String> {
    describe_schema_diff(&a.fields, &b.fields, &a.optional_fields, &b.optional_fields)
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{OutputKind, TableMode};
    use crate::schema::{DerivedSchema, EventSchema, FieldType, TableSchema};
    use std::collections::BTreeMap;

    // ── Test helpers ──────────────────────────────────────────────────────────

    fn empty_registry() -> RegistryInner {
        RegistryInner::default()
    }

    fn registry_with_event(name: &str, schema: EventSchema) -> RegistryInner {
        let mut r = RegistryInner::default();
        r.events.insert(
            name.to_string(),
            Arc::new(EventDescriptor {
                name: name.to_string(),
                schema,
                dedupe_key: None,
                dedupe_window_ms: None,
                keep_events_for_ms: None,
                cold_after_ms: None,
                registered_at_version: 1,
                name_arc: Arc::from(""),
                apply_field_names: vec![],
            }),
        );
        r.version = 1;
        r
    }

    fn simple_event_schema() -> EventSchema {
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("amount".to_string(), FieldType::F64);
        EventSchema {
            fields,
            optional_fields: vec![],
        }
    }

    fn event_node(name: &str, schema: EventSchema) -> PayloadNode {
        PayloadNode::Event(EventDescriptor {
            name: name.to_string(),
            schema,
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        })
    }

    fn table_node(name: &str, primary_key: Vec<String>, schema: TableSchema) -> PayloadNode {
        PayloadNode::Table(TableDescriptor {
            name: name.to_string(),
            primary_key,
            schema,
            ttl_ms: None,
            mode: TableMode::Upsert,
            registered_at_version: 0,
            temporal: false,
            retention_ms: None,
        })
    }

    fn simple_table_schema() -> TableSchema {
        let mut fields = BTreeMap::new();
        fields.insert("id".to_string(), FieldType::Str);
        TableSchema {
            fields,
            optional_fields: vec![],
        }
    }

    fn derivation_node(
        name: &str,
        upstreams: Vec<String>,
        ops: Vec<crate::op_node::OpNode>,
    ) -> PayloadNode {
        let mut fields = BTreeMap::new();
        fields.insert("amount".to_string(), FieldType::F64);
        PayloadNode::Derivation(DerivationDescriptor {
            name: name.to_string(),
            output_kind: OutputKind::Event,
            upstreams,
            ops,
            schema: DerivedSchema {
                fields,
                optional_fields: vec![],
            },
            table_primary_key: None,
            registered_at_version: 0,
        })
    }

    // ── A. Structural invariants ──────────────────────────────────────────────

    // Test 1
    #[test]
    fn empty_payload_empty_current() {
        let diff = compute_diff(&empty_registry(), &[]);
        assert!(diff.added.is_empty());
        assert!(diff.already_present.is_empty());
        assert!(diff.changed.is_empty());
    }

    // Test 2
    #[test]
    fn added_only_against_empty() {
        let mut t_fields = BTreeMap::new();
        t_fields.insert("id".to_string(), FieldType::Str);

        let payload = vec![
            event_node("A", simple_event_schema()),
            table_node(
                "B",
                vec!["id".to_string()],
                TableSchema {
                    fields: t_fields,
                    optional_fields: vec![],
                },
            ),
            derivation_node("C", vec!["A".to_string()], vec![]),
        ];
        let diff = compute_diff(&empty_registry(), &payload);
        assert_eq!(diff.added, vec!["A", "B", "C"]);
        assert!(diff.already_present.is_empty());
        assert!(diff.changed.is_empty());
    }

    // Test 3
    #[test]
    fn already_present_identical() {
        let schema = simple_event_schema();
        let current = registry_with_event("A", schema.clone());
        let payload = vec![event_node("A", schema)];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.already_present, vec!["A"]);
        assert!(diff.added.is_empty());
        assert!(diff.changed.is_empty());
    }

    // ── B. Change-reason classification ──────────────────────────────────────

    // Test 4
    #[test]
    fn schema_mismatch_field_type_changed() {
        let schema_a = simple_event_schema(); // amount: F64
        let mut schema_b = simple_event_schema();
        schema_b.fields.insert("amount".to_string(), FieldType::I64); // changed to I64

        let current = registry_with_event("A", schema_a.clone());
        let payload = vec![event_node("A", schema_b)];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].name, "A");
        assert_eq!(diff.changed[0].reason, DiffReason::SchemaMismatch);
        let details = &diff.changed[0].details;
        assert!(
            details.contains("amount"),
            "details should mention 'amount', got: {details}"
        );
        assert!(
            details.contains("F64") || details.contains("f64"),
            "details should mention original type, got: {details}"
        );
        assert!(
            details.contains("I64") || details.contains("i64"),
            "details should mention new type, got: {details}"
        );
    }

    // Test 5: adding a field to existing schema is a conflict
    #[test]
    fn schema_mismatch_field_added_strict() {
        let schema_a = simple_event_schema();
        let mut schema_b = simple_event_schema();
        schema_b.fields.insert("y".to_string(), FieldType::Str); // extra field

        let current = registry_with_event("A", schema_a);
        let payload = vec![event_node("A", schema_b)];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].reason, DiffReason::SchemaMismatch);
        let details = &diff.changed[0].details;
        assert!(
            details.contains("y"),
            "details should mention field 'y', got: {details}"
        );
        assert!(
            details.contains("added"),
            "details should say 'added', got: {details}"
        );
    }

    // Test 6
    #[test]
    fn schema_mismatch_field_removed() {
        let mut schema_a = simple_event_schema();
        schema_a.fields.insert("y".to_string(), FieldType::Str);
        let schema_b = simple_event_schema(); // y is gone

        let current = registry_with_event("A", schema_a);
        let payload = vec![event_node("A", schema_b)];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.changed[0].reason, DiffReason::SchemaMismatch);
        let details = &diff.changed[0].details;
        assert!(details.contains("y") && details.contains("removed"));
    }

    // Test 7 (Plan 12.6-06 D-03 hard rip): the pre-pivot
    // `event_time_field_mismatch` test is deleted — `event_time_field` is no
    // longer on EventDescriptor, so the diff classifier doesn't compare it.
    // Stale fixtures get rejected at the JSON-prelude layer
    // (`pre_check_legacy_event_time_keys`), not at the diff layer.

    // Test 8
    #[test]
    fn dedupe_key_mismatch() {
        let schema = simple_event_schema();
        let mut current = registry_with_event("A", schema.clone());
        // Events are Arc — Arc::make_mut to update.
        Arc::make_mut(current.events.get_mut("A").unwrap()).dedupe_key =
            Some("request_id".to_string());

        // Payload has no dedupe_key
        let payload = vec![event_node("A", schema)];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.changed[0].reason, DiffReason::DedupeKeyMismatch);
    }

    // Test 9
    #[test]
    fn ttl_mismatch() {
        let schema = simple_event_schema();
        let mut current = registry_with_event("A", schema.clone());
        Arc::make_mut(current.events.get_mut("A").unwrap()).keep_events_for_ms = Some(1000);

        // Payload has keep_events_for_ms = 1001 (differs by 1ms)
        let payload = vec![PayloadNode::Event(EventDescriptor {
            name: "A".to_string(),
            schema,
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: Some(1001),
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        })];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.changed[0].reason, DiffReason::TtlMismatch);
        assert!(diff.changed[0].details.contains("keep_events_for_ms"));
    }

    // Test 10
    #[test]
    fn primary_key_mismatch() {
        let mut fields = BTreeMap::new();
        fields.insert("id".to_string(), FieldType::Str);
        fields.insert("user_id".to_string(), FieldType::Str);
        let schema = TableSchema {
            fields: fields.clone(),
            optional_fields: vec![],
        };

        let mut current = RegistryInner::default();
        current.tables.insert(
            "T".to_string(),
            TableDescriptor {
                name: "T".to_string(),
                primary_key: vec!["id".to_string()],
                schema: schema.clone(),
                ttl_ms: None,
                mode: TableMode::Upsert,
                registered_at_version: 1,
                temporal: false,
                retention_ms: None,
            },
        );

        let payload = vec![PayloadNode::Table(TableDescriptor {
            name: "T".to_string(),
            primary_key: vec!["user_id".to_string()],
            schema,
            ttl_ms: None,
            mode: TableMode::Upsert,
            registered_at_version: 0,
            temporal: false,
            retention_ms: None,
        })];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.changed[0].reason, DiffReason::PrimaryKeyMismatch);
    }

    // Test 11: TableModeMismatch placeholder — v0 only has Append
    // TODO(v0.1): add test when Changelog variant ships
    #[test]
    fn table_mode_mismatch_placeholder() {
        // Acknowledged: v0 only has Append so this variant cannot be triggered today.
        // Verified by the code path existing in classify_table_diff.
        assert_eq!(
            format!("{:?}", DiffReason::TableModeMismatch),
            "TableModeMismatch"
        );
    }

    // Test 12
    #[test]
    fn kind_mismatch_event_vs_table() {
        let schema = simple_event_schema();
        let current = registry_with_event("Foo", schema);

        // Payload submits a Table named "Foo" (current has an Event "Foo")
        let payload = vec![table_node("Foo", vec![], simple_table_schema())];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.changed[0].reason, DiffReason::KindMismatch);
        let details = &diff.changed[0].details;
        assert!(details.contains("expected kind 'event'"));
        assert!(details.contains("got kind 'table'"));
    }

    // Test 13
    #[test]
    fn ops_mismatch_derivation() {
        use crate::op_node::OpNode;
        let ops_a = vec![OpNode::Filter {
            expr: "(a > 1)".to_string(),
        }];
        let ops_b = vec![OpNode::Filter {
            expr: "(a > 2)".to_string(),
        }];

        let mut current = RegistryInner::default();
        let mut fields = BTreeMap::new();
        fields.insert("amount".to_string(), FieldType::F64);
        current.derivations.insert(
            "D".to_string(),
            DerivationDescriptor {
                name: "D".to_string(),
                output_kind: OutputKind::Event,
                upstreams: vec!["A".to_string()],
                ops: ops_a,
                schema: DerivedSchema {
                    fields: fields.clone(),
                    optional_fields: vec![],
                },
                table_primary_key: None,
                registered_at_version: 1,
            },
        );

        let payload = vec![derivation_node("D", vec!["A".to_string()], ops_b)];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.changed[0].reason, DiffReason::OpsMismatch);
    }

    // Test 14
    #[test]
    fn upstreams_mismatch() {
        let mut current = RegistryInner::default();
        let mut fields = BTreeMap::new();
        fields.insert("amount".to_string(), FieldType::F64);
        current.derivations.insert(
            "D".to_string(),
            DerivationDescriptor {
                name: "D".to_string(),
                output_kind: OutputKind::Event,
                upstreams: vec!["A".to_string()],
                ops: vec![],
                schema: DerivedSchema {
                    fields: fields.clone(),
                    optional_fields: vec![],
                },
                table_primary_key: None,
                registered_at_version: 1,
            },
        );

        // Payload adds a second upstream
        let payload = vec![derivation_node(
            "D",
            vec!["A".to_string(), "B".to_string()],
            vec![],
        )];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.changed[0].reason, DiffReason::UpstreamsMismatch);
    }

    // Test 15
    #[test]
    fn output_kind_mismatch() {
        let mut current = RegistryInner::default();
        let mut fields = BTreeMap::new();
        fields.insert("amount".to_string(), FieldType::F64);
        current.derivations.insert(
            "D".to_string(),
            DerivationDescriptor {
                name: "D".to_string(),
                output_kind: OutputKind::Event,
                upstreams: vec!["A".to_string()],
                ops: vec![],
                schema: DerivedSchema {
                    fields: fields.clone(),
                    optional_fields: vec![],
                },
                table_primary_key: None,
                registered_at_version: 1,
            },
        );

        let payload = vec![PayloadNode::Derivation(DerivationDescriptor {
            name: "D".to_string(),
            output_kind: OutputKind::Table, // changed
            upstreams: vec!["A".to_string()],
            ops: vec![],
            schema: DerivedSchema {
                fields,
                optional_fields: vec![],
            },
            table_primary_key: Some(vec!["amount".to_string()]),
            registered_at_version: 0,
        })];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.changed[0].reason, DiffReason::OutputKindMismatch);
    }

    // ── C. Ordering + multi-node ──────────────────────────────────────────────

    // Test 16
    #[test]
    fn preserves_payload_order_added_and_already_present() {
        let schema = simple_event_schema();
        let mut current = RegistryInner::default();
        current.events.insert(
            "existingB".to_string(),
            Arc::new(EventDescriptor {
                name: "existingB".to_string(),
                schema: schema.clone(),
                dedupe_key: None,
                dedupe_window_ms: None,
                keep_events_for_ms: None,
                cold_after_ms: None,
                registered_at_version: 1,
                name_arc: Arc::from(""),
                apply_field_names: vec![],
            }),
        );
        current.events.insert(
            "existingD".to_string(),
            Arc::new(EventDescriptor {
                name: "existingD".to_string(),
                schema: schema.clone(),
                dedupe_key: None,
                dedupe_window_ms: None,
                keep_events_for_ms: None,
                cold_after_ms: None,
                registered_at_version: 1,
                name_arc: Arc::from(""),
                apply_field_names: vec![],
            }),
        );

        let payload = vec![
            event_node("newA", simple_event_schema()),
            event_node("existingB", schema.clone()),
            event_node("newC", simple_event_schema()),
            event_node("existingD", schema.clone()),
        ];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.added, vec!["newA", "newC"]);
        assert_eq!(diff.already_present, vec!["existingB", "existingD"]);
    }

    // Test 17
    #[test]
    fn mixed_added_already_changed() {
        let schema = simple_event_schema();

        // changedX: exists with f64 amount, payload changes to i64
        let mut schema_changed = simple_event_schema();
        schema_changed
            .fields
            .insert("amount".to_string(), FieldType::I64);

        let mut current = RegistryInner::default();
        current.events.insert(
            "X".to_string(),
            Arc::new(EventDescriptor {
                name: "X".to_string(),
                schema: schema.clone(),
                dedupe_key: None,
                dedupe_window_ms: None,
                keep_events_for_ms: None,
                cold_after_ms: None,
                registered_at_version: 1,
                name_arc: Arc::from(""),
                apply_field_names: vec![],
            }),
        );
        current.events.insert(
            "Z".to_string(),
            Arc::new(EventDescriptor {
                name: "Z".to_string(),
                schema: schema.clone(),
                dedupe_key: None,
                dedupe_window_ms: None,
                keep_events_for_ms: None,
                cold_after_ms: None,
                registered_at_version: 1,
                name_arc: Arc::from(""),
                apply_field_names: vec![],
            }),
        );

        let payload = vec![
            event_node("X", schema_changed),        // changed
            event_node("Y", simple_event_schema()), // new
            event_node("Z", schema.clone()),        // already present
        ];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.added, vec!["Y"]);
        assert_eq!(diff.already_present, vec!["Z"]);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].name, "X");
    }

    // ── D. equiv_ignoring_version semantics ───────────────────────────────────

    // Test 18
    #[test]
    fn version_field_ignored() {
        let schema = simple_event_schema();

        // Registry has EventA @ v1
        let current = registry_with_event("A", schema.clone());

        // Payload has EventA with registered_at_version = 99 (shouldn't matter)
        let payload = vec![PayloadNode::Event(EventDescriptor {
            name: "A".to_string(),
            schema,
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 99,
            name_arc: Arc::from(""), // server-assigned, should be ignored
            apply_field_names: vec![],
        })];
        let diff = compute_diff(&current, &payload);
        assert_eq!(diff.already_present, vec!["A"]);
        assert!(diff.added.is_empty());
        assert!(diff.changed.is_empty());
    }
}

// ─── Proptests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::registry::TableMode;
    use crate::schema::{EventSchema, FieldType, TableSchema};
    use proptest::prelude::*;

    // ── Strategies ────────────────────────────────────────────────────────────

    fn arb_field_type() -> impl Strategy<Value = FieldType> {
        prop_oneof![
            Just(FieldType::Str),
            Just(FieldType::F64),
            Just(FieldType::I64),
            Just(FieldType::Bool),
            Just(FieldType::Bytes),
            Just(FieldType::Datetime),
        ]
    }

    fn arb_name() -> impl Strategy<Value = String> {
        // Simple valid names: letter/underscore start, alphanumeric body, 1-16 chars
        "[A-Za-z_][A-Za-z0-9_]{0,15}".prop_filter("not reserved prefix", |s: &String| {
            !s.starts_with("_beava_")
        })
    }

    fn arb_event_descriptor() -> impl Strategy<Value = EventDescriptor> {
        (
            arb_name(),
            prop::collection::btree_map(arb_name(), arb_field_type(), 1..5usize),
        )
            .prop_map(|(name, mut extra_fields)| {
                // Always have event_time as I64
                extra_fields.insert("event_time".to_string(), FieldType::I64);
                EventDescriptor {
                    name,
                    schema: EventSchema {
                        fields: extra_fields,
                        optional_fields: vec![],
                    },
                    dedupe_key: None,
                    dedupe_window_ms: None,
                    keep_events_for_ms: None,
                    cold_after_ms: None,
                    registered_at_version: 0,
                    name_arc: Arc::from(""),
                    apply_field_names: vec![],
                }
            })
    }

    fn arb_table_descriptor() -> impl Strategy<Value = TableDescriptor> {
        (
            arb_name(),
            prop::collection::btree_map(arb_name(), arb_field_type(), 1..5usize),
        )
            .prop_map(|(name, fields)| {
                let pk = vec![fields.keys().next().unwrap().clone()]; // first field as PK
                TableDescriptor {
                    name,
                    primary_key: pk,
                    schema: TableSchema {
                        fields,
                        optional_fields: vec![],
                    },
                    ttl_ms: None,
                    mode: TableMode::Upsert,
                    registered_at_version: 0,
                    temporal: false,
                    retention_ms: None,
                }
            })
    }

    fn arb_registry_inner() -> impl Strategy<Value = RegistryInner> {
        (
            prop::collection::vec(arb_event_descriptor(), 0..4usize),
            prop::collection::vec(arb_table_descriptor(), 0..4usize),
        )
            .prop_map(|(events, tables)| {
                let mut inner = RegistryInner::default();
                // Deduplicate by name (last wins, mimics insert behavior)
                for (i, mut e) in events.into_iter().enumerate() {
                    e.registered_at_version = (i as u64) + 1;
                    let name = e.name.clone();
                    // Skip if name already used by table
                    if !inner.tables.contains_key(&name) {
                        inner.events.insert(name, Arc::new(e));
                    }
                }
                for (i, mut t) in tables.into_iter().enumerate() {
                    t.registered_at_version = (i as u64) + 1;
                    let name = t.name.clone();
                    // Skip if name already used by event
                    if !inner.events.contains_key(&name) {
                        inner.tables.insert(name, t);
                    }
                }
                let total = inner.events.len() + inner.tables.len();
                inner.version = total as u64;
                inner
            })
    }

    fn registry_inner_to_payload_nodes(reg: &RegistryInner) -> Vec<PayloadNode> {
        let mut nodes: Vec<PayloadNode> = Vec::new();
        for e in reg.events.values() {
            // Events are Arc-wrapped — clone the inner.
            nodes.push(PayloadNode::Event((**e).clone()));
        }
        for t in reg.tables.values() {
            nodes.push(PayloadNode::Table(t.clone()));
        }
        nodes
    }

    // ── Properties ───────────────────────────────────────────────────────────

    proptest! {
        // Prop 1: empty payload always produces empty diff
        #[test]
        fn empty_payload_produces_empty_diff(reg in arb_registry_inner()) {
            let diff = compute_diff(&reg, &[]);
            prop_assert!(diff.added.is_empty());
            prop_assert!(diff.already_present.is_empty());
            prop_assert!(diff.changed.is_empty());
        }

        // Prop 2: every name in added is NOT in current registry
        #[test]
        fn added_never_in_current(
            reg in arb_registry_inner(),
            extra in prop::collection::vec(arb_event_descriptor(), 0..4usize),
        ) {
            let payload: Vec<PayloadNode> = extra.iter()
                .map(|e| PayloadNode::Event(e.clone()))
                .collect();
            let diff = compute_diff(&reg, &payload);
            for name in &diff.added {
                prop_assert!(
                    !reg.events.contains_key(name) &&
                    !reg.tables.contains_key(name) &&
                    !reg.derivations.contains_key(name),
                    "added name '{name}' should not be in current registry"
                );
            }
        }

        // Prop 3: already_present is in current registry in the same kind map
        #[test]
        fn already_present_is_in_current_same_kind(reg in arb_registry_inner()) {
            let payload = registry_inner_to_payload_nodes(&reg);
            let diff = compute_diff(&reg, &payload);
            for name in &diff.already_present {
                let node = payload.iter().find(|n| n.name() == name.as_str()).unwrap();
                match node {
                    PayloadNode::Event(_) => prop_assert!(reg.events.contains_key(name)),
                    PayloadNode::Table(_) => prop_assert!(reg.tables.contains_key(name)),
                    PayloadNode::Derivation(_) => prop_assert!(reg.derivations.contains_key(name)),
                }
            }
        }

        // Prop 4: already_present descriptors are equiv_ignoring_version
        #[test]
        fn already_present_descriptors_equiv(reg in arb_registry_inner()) {
            let payload = registry_inner_to_payload_nodes(&reg);
            let diff = compute_diff(&reg, &payload);
            for name in &diff.already_present {
                let node = payload.iter().find(|n| n.name() == name.as_str()).unwrap();
                match node {
                    PayloadNode::Event(submitted) => {
                        let existing = &reg.events[name];
                        prop_assert!(existing.equiv_ignoring_version(submitted));
                    }
                    PayloadNode::Table(submitted) => {
                        let existing = &reg.tables[name];
                        prop_assert!(existing.equiv_ignoring_version(submitted));
                    }
                    PayloadNode::Derivation(submitted) => {
                        let existing = &reg.derivations[name];
                        prop_assert!(existing.equiv_ignoring_version(submitted));
                    }
                }
            }
        }

        // Prop 5: every changed entry has a non-empty details string
        #[test]
        fn changed_has_nonempty_details(
            reg in arb_registry_inner(),
            extra in prop::collection::vec(arb_event_descriptor(), 0..4usize),
        ) {
            let payload: Vec<PayloadNode> = extra.iter()
                .map(|e| PayloadNode::Event(e.clone()))
                .collect();
            let diff = compute_diff(&reg, &payload);
            for entry in &diff.changed {
                prop_assert!(!entry.details.is_empty(), "details must not be empty for {}", entry.name);
            }
        }

        // Prop 6: input order preserved within each output vector
        #[test]
        fn input_order_preserved(
            reg in arb_registry_inner(),
            extra in prop::collection::vec(arb_event_descriptor(), 1..6usize),
        ) {
            // Deduplicate by name (first-wins) so the payload has no duplicate
            // names, which would make `position()` return an earlier index and
            // falsely fail the ordering assertion.
            let mut seen_names = std::collections::HashSet::new();
            let payload: Vec<PayloadNode> = extra.iter()
                .filter(|e| seen_names.insert(e.name.clone()))
                .map(|e| PayloadNode::Event(e.clone()))
                .collect();
            prop_assume!(!payload.is_empty());
            let diff = compute_diff(&reg, &payload);

            // Build expected order: index of each name in payload
            let payload_order: Vec<&str> = payload.iter().map(|n| n.name()).collect();

            // Verify that added, already_present each appear in payload order
            let added_indices: Vec<usize> = diff.added.iter()
                .map(|n| payload_order.iter().position(|&p| p == n.as_str()).unwrap())
                .collect();
            let sorted_added = {
                let mut v = added_indices.clone();
                v.sort_unstable();
                v
            };
            prop_assert_eq!(&added_indices, &sorted_added, "added order must match payload order");

            let ap_indices: Vec<usize> = diff.already_present.iter()
                .map(|n| payload_order.iter().position(|&p| p == n.as_str()).unwrap())
                .collect();
            let sorted_ap = {
                let mut v = ap_indices.clone();
                v.sort_unstable();
                v
            };
            prop_assert_eq!(&ap_indices, &sorted_ap, "already_present order must match payload order");
        }

        // Prop 7: idempotent — converting all current descriptors back to PayloadNodes lands in already_present
        #[test]
        fn idempotent_with_self(reg in arb_registry_inner()) {
            let payload = registry_inner_to_payload_nodes(&reg);
            if payload.is_empty() {
                return Ok(());
            }
            let diff = compute_diff(&reg, &payload);
            prop_assert!(diff.added.is_empty(), "no new descriptors should be added on idempotent re-submit");
            prop_assert!(diff.changed.is_empty(), "no conflicts should arise on idempotent re-submit");
            prop_assert_eq!(diff.already_present.len(), payload.len());
        }

        // Prop 8: total_classification — each payload item lands in exactly one bucket
        #[test]
        fn total_classification(
            reg in arb_registry_inner(),
            extra in prop::collection::vec(arb_event_descriptor(), 0..4usize),
        ) {
            let mut payload = registry_inner_to_payload_nodes(&reg);
            payload.extend(extra.iter().map(|e| PayloadNode::Event(e.clone())));

            let diff = compute_diff(&reg, &payload);
            let total = diff.added.len() + diff.already_present.len() + diff.changed.len();
            prop_assert_eq!(
                total, payload.len(),
                "every payload item must be in exactly one bucket"
            );
        }
    }
}
