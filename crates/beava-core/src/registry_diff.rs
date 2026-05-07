//! Registry diff engine — categorized-lists payload format
//! (`{additive, destructive, already_present}`).
//!
//! `classify_register_diff` (in `register_validate.rs`) produces a
//! `RegisterDiff` from `(prev_registry, payload_nodes)`. The HTTP handler
//! consumes it to gate `force_required` (destructive without `force=true`
//! → 409) and to populate the `already_present` field of success responses.
//!
//! Per D-01 (USER-LOCKED):
//!   - Destructive variants require `force=true`: rename, type-change,
//!     op removal, agg removal, window-change, key-cols change.
//!   - Additive variants apply without force: new descriptor, new agg in
//!     existing block, new field on event source.
//!
//! Wire shape: `{"additive": [...], "destructive": [...]}` — categorized
//! lists, NOT JSON-Patch. Each entry uses `{"kind": "<class>", ...}`
//! internally-tagged serde representation. `from` / `to` carry the prior
//! and proposed values for clearly-paired changes.

use crate::registry::{DerivationDescriptor, EventDescriptor, TableDescriptor};
use serde::{Deserialize, Serialize};

// ─── PayloadNode ──────────────────────────────────────────────────────────────

/// A parsed node from the `POST /register` payload, after `kind`
/// discrimination but before `registered_at_version` assignment. The HTTP
/// handler (Plan 05) produces these. The validation pass (Plan 04)
/// guarantees each descriptor is structurally valid before
/// `classify_register_diff` runs.
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
