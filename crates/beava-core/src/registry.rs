//! Registry data model: descriptor structs, `OutputKind`, `TableMode`,
//! `RegistryInner`, and the parking_lot::RwLock-guarded `Registry` wrapper.

use crate::agg_descriptor::AggregationDescriptor;
use crate::op_chain::OpChain;
use crate::op_node::OpNode;
use crate::schema::{DerivedSchema, EventSchema, TableSchema};
use parking_lot::{RwLock, RwLockReadGuard};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Event,
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableMode {
    Upsert,
}

/// Default for the `name_arc` field — populated server-side at registration,
/// so the deserialize default is just an empty `Arc<str>` placeholder. The
/// install_descriptors / apply_registration / install_from_descriptors paths
/// always overwrite this with `Arc::from(name.as_str())`.
fn default_event_name_arc() -> Arc<str> {
    Arc::from("")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventDescriptor {
    pub name: String,
    pub schema: EventSchema,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default)]
    pub dedupe_window_ms: Option<u64>,
    #[serde(default)]
    pub keep_events_for_ms: Option<u64>,
    /// Per-source cold-entity TTL (opt-in). When set, the apply hot path
    /// treats an entity whose `last_seen_ms` is older than
    /// `now_ms - cold_after_ms` as a fresh entity (clear state, increment
    /// `cold_entity_evictions_total{source=...}`). `None` (omitted from
    /// wire) = no expiry; preserves existing behavior for sources that
    /// don't opt in. Range is enforced at decorator-time on the Python
    /// side: `1_000 ≤ cold_after_ms ≤ 365 * 86_400_000`.
    ///
    /// Resurrect semantics are locked to FRESH state (Redis TTL pattern):
    /// no partial-state preservation. Reviving requires explicit user
    /// override + new ADR.
    #[serde(default)]
    pub cold_after_ms: Option<u64>,
    /// Assigned server-side; ignored (defaulted to 0) when deserializing from client JSON.
    #[serde(default)]
    pub registered_at_version: u64,
    /// Pre-allocated `Arc<str>` of `name`. The bookkeeping site in
    /// `dispatch_push_sync` clones this (refcount bump, ~5 ns) instead of
    /// calling `event_name.to_string()` (heap alloc, ~50-100 ns) on every
    /// push. Populated server-side at registration; client-supplied JSON
    /// omits it (skipped on serde, defaulted to `Arc::from("")` on
    /// deserialize, then overwritten to `Arc::from(name.as_str())` by the
    /// install/registration paths). Equality on `Arc<str>` is by `str`
    /// content, so derived PartialEq behaves intuitively even across
    /// different allocations.
    #[serde(skip, default = "default_event_name_arc")]
    pub name_arc: Arc<str>,
    /// Ordered list of distinct field names referenced by ALL aggregations
    /// that source from this event. Built as the union of all
    /// `AggregationDescriptor.field_names` lists across aggs for this
    /// source. Each `AggOpDescriptor.field_idx` is an index into this
    /// per-event list. The apply-loop pre-extracts
    /// `extracted[i] = row.get(apply_field_names[i])` once per event —
    /// `O(distinct_fields)` total — then each feature reads
    /// `extracted[feature.descriptor.field_idx]` in `O(1)`. Populated by
    /// `Registry::apply_registration`; client JSON omits it.
    #[serde(skip, default)]
    pub apply_field_names: Vec<String>,
}

impl EventDescriptor {
    /// Compare two descriptors field-by-field, EXCLUDING `registered_at_version`.
    /// Used by the diff engine to detect conflicts without false positives
    /// from version stamps.
    pub fn equiv_ignoring_version(&self, other: &Self) -> bool {
        self.name == other.name
            && self.schema == other.schema
            && self.dedupe_key == other.dedupe_key
            && self.dedupe_window_ms == other.dedupe_window_ms
            && self.keep_events_for_ms == other.keep_events_for_ms
            && self.cold_after_ms == other.cold_after_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableDescriptor {
    pub name: String,
    pub primary_key: Vec<String>,
    pub schema: TableSchema,
    #[serde(default)]
    pub ttl_ms: Option<u64>,
    pub mode: TableMode,
    /// Assigned server-side; ignored (defaulted to 0) when deserializing from client JSON.
    #[serde(default)]
    pub registered_at_version: u64,
    /// When `true`, the table is stored as an MVCC chain so `as_of=<lsn>`
    /// queries and `POST /retract` work. Defaults to `false` for backward
    /// compatibility with non-temporal client payloads.
    #[serde(default)]
    pub temporal: bool,
    /// MVCC history-window in wall-clock milliseconds. Distinct from
    /// `ttl_ms` (per-row TTL): `retention_ms` bounds how far back `as_of`
    /// queries and retractions can reach. `None` means "unbounded
    /// retention" (use with care; memory grows with history).
    ///
    /// `skip_serializing_if` is intentionally NOT used here — bincode's
    /// positional layout would otherwise become asymmetric with decode.
    /// JSON clients can still omit the field (serde `default` handles the
    /// missing case).
    #[serde(default)]
    pub retention_ms: Option<u64>,
}

impl TableDescriptor {
    /// Compare two descriptors field-by-field, EXCLUDING `registered_at_version`.
    pub fn equiv_ignoring_version(&self, other: &Self) -> bool {
        self.name == other.name
            && self.primary_key == other.primary_key
            && self.schema == other.schema
            && self.ttl_ms == other.ttl_ms
            && self.mode == other.mode
            && self.temporal == other.temporal
            && self.retention_ms == other.retention_ms
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivationDescriptor {
    pub name: String,
    pub output_kind: OutputKind,
    pub upstreams: Vec<String>,
    /// Strongly-typed op pipeline.
    #[serde(default)]
    pub ops: Vec<OpNode>,
    /// `serde(default)`-able so clients can omit the schema field entirely.
    /// Server's `validate_expressions` runs schema-propagation from upstream
    /// chain (via `OpChain::compile` → `propagated_schemas`) and writes the
    /// inferred schema back to the registry post-validation. This is the
    /// single source of truth — Python SDK does not mirror it.
    #[serde(default)]
    pub schema: DerivedSchema,
    #[serde(default)]
    pub table_primary_key: Option<Vec<String>>,
    /// Assigned server-side; ignored (defaulted to 0) when deserializing from client JSON.
    #[serde(default)]
    pub registered_at_version: u64,
}

impl DerivationDescriptor {
    /// Compare two descriptors field-by-field, EXCLUDING `registered_at_version`.
    pub fn equiv_ignoring_version(&self, other: &Self) -> bool {
        self.name == other.name
            && self.output_kind == other.output_kind
            && self.upstreams == other.upstreams
            && self.ops == other.ops
            && self.schema == other.schema
            && self.table_primary_key == other.table_primary_key
    }
}

/// Runtime-only compiled op-chain cache. Not serialized — rebuilt from
/// ops at register time.
#[derive(Debug, Default, Clone)]
pub struct RegistryInner {
    pub version: u64,
    /// Events stored as `Arc` so `dispatch_push_sync` can grab a cheap
    /// refcount-bump pointer instead of cloning the `EventDescriptor` on
    /// every push. Snapshot/install paths convert via `Arc::new` and
    /// `(*arc).clone()` at the boundaries (cold paths).
    pub events: BTreeMap<String, Arc<EventDescriptor>>,
    pub tables: BTreeMap<String, TableDescriptor>,
    pub derivations: BTreeMap<String, DerivationDescriptor>,
    /// Compiled op-chains keyed by derivation name. Populated by
    /// `apply_registration` when a derivation with ops is installed.
    pub compiled_chains: BTreeMap<String, Arc<OpChain>>,
    /// Compiled aggregation descriptors keyed by derivation name.
    /// Populated by `apply_registration` when a derivation with `GroupBy`
    /// ops is installed.
    pub compiled_aggregations: BTreeMap<String, Arc<AggregationDescriptor>>,
    /// Reverse index from feature name to (aggregation node_name,
    /// feature_index). Built at register time alongside
    /// `compiled_aggregations`. Enables `O(1)` feature-name → aggregation
    /// lookup at query time.
    pub feature_index: BTreeMap<String, (String, usize)>,
    /// Precomputed per-source index. Maps a source event/table name to
    /// the list of compiled aggregations that watch it. Lookup is `O(1)`
    /// at apply time. Built register-time alongside
    /// `compiled_aggregations`; tracked here so it survives
    /// `Registry::clone`.
    pub aggregations_by_source: std::collections::HashMap<String, Vec<Arc<AggregationDescriptor>>>,
    /// Monotonic counter for stable u32 IDs assigned to each new
    /// aggregation at `apply_registration` time. Used as `O(1)` Vec
    /// index into `DevAggState.state_tables`. Increments by 1 per new
    /// aggregation; IDs are stable for process lifetime (additive-only
    /// registration). Default = 0; first aggregation gets ID 0.
    pub next_agg_id: u32,
    /// Maps a cluster-signature hash to a stable u32 `cluster_id`.
    /// Aggregations sharing the same `group_keys` signature
    /// (declaration-order hash, NOT sorted-lex) share a `cluster_id` so
    /// the apply loop builds `EntityKey` ONCE per cluster.
    pub cluster_id_by_signature: std::collections::HashMap<u64, u32>,
    /// Monotonic counter for `cluster_id` assignment. Default = 0; first
    /// unique cluster gets ID 0.
    pub next_cluster_id: u32,
}

#[derive(Debug, Default)]
pub struct Registry {
    inner: RwLock<RegistryInner>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn version(&self) -> u64 {
        self.inner.read().version
    }

    /// Read the registry's monotonic `agg_id` counter. Server-side
    /// register handlers call this after `apply_registration` to resize
    /// `DevAggState.state_tables` (a `Vec<AggStateTable>`) so the apply
    /// hot path can index by `desc.agg_id` without bounds issues.
    pub fn next_agg_id(&self) -> u32 {
        self.inner.read().next_agg_id
    }

    pub fn read(&self) -> RwLockReadGuard<'_, RegistryInner> {
        self.inner.read()
    }

    pub fn snapshot(&self) -> RegistryInner {
        self.inner.read().clone()
    }

    /// Return the compiled `OpChain` for a derivation (if cached).
    /// Returns `None` if the derivation has no ops or was not yet registered.
    pub fn compiled_chain(&self, derivation_name: &str) -> Option<Arc<OpChain>> {
        self.inner
            .read()
            .compiled_chains
            .get(derivation_name)
            .cloned()
    }

    /// Return the compiled `AggregationDescriptor` for a derivation (if cached).
    /// Returns `None` if the derivation has no `GroupBy` ops or was not yet registered.
    pub fn compiled_aggregation(
        &self,
        derivation_name: &str,
    ) -> Option<Arc<AggregationDescriptor>> {
        self.inner
            .read()
            .compiled_aggregations
            .get(derivation_name)
            .cloned()
    }

    /// `O(1)` Arc-backed event-descriptor lookup.
    ///
    /// Returns `None` if the event isn't registered. The returned `Arc` is a
    /// refcount bump on the registry-owned `Arc` — `dispatch_push_sync` can
    /// hold it for the duration of one push without cloning the
    /// `EventDescriptor`.
    pub fn get_event_descriptor(&self, name: &str) -> Option<Arc<EventDescriptor>> {
        self.inner.read().events.get(name).cloned()
    }

    /// Return the `(aggregation node_name, feature_index)` for a feature name,
    /// or `None` if the feature name is not registered. `O(1)` reverse lookup
    /// into `feature_index`.
    pub fn resolve_feature(&self, feature_name: &str) -> Option<(String, usize)> {
        self.inner.read().feature_index.get(feature_name).cloned()
    }

    /// Return all compiled `AggregationDescriptor`s whose `source_node_name`
    /// matches `source_name`. Used by `apply_event_to_aggregations` to route
    /// an incoming event to every aggregation that watches the event's
    /// source.
    ///
    /// `O(1)` HashMap lookup via the precomputed `aggregations_by_source`
    /// index. The returned `Vec` is cloned from the index — cheap because
    /// (a) it's a `Vec<Arc<...>>`, and (b) typical apps have 1–3
    /// aggregations per source.
    pub fn compiled_aggregations_for_source(
        &self,
        source_name: &str,
    ) -> Vec<Arc<AggregationDescriptor>> {
        self.inner
            .read()
            .aggregations_by_source
            .get(source_name)
            .cloned()
            .unwrap_or_default()
    }

    /// Install descriptors into the registry under a write lock. Monotonically
    /// bumps the version to `new_version`. Panics in debug if `new_version` is
    /// not strictly greater than the current version.
    ///
    /// Low-level helper. `apply_registration` sits on top and handles
    /// `PayloadNode` dispatch + skips already-present descriptors.
    pub fn install_descriptors(
        &self,
        new_version: u64,
        events: Vec<EventDescriptor>,
        tables: Vec<TableDescriptor>,
        derivations: Vec<DerivationDescriptor>,
    ) {
        let mut w = self.inner.write();
        debug_assert!(
            new_version > w.version,
            "install_descriptors: new_version ({new_version}) must be > current version ({})",
            w.version
        );
        for mut e in events {
            e.registered_at_version = new_version;
            // Pre-allocate the Arc<str> for the bookkeeping hot path.
            // Client-supplied descriptors deserialize with an empty
            // placeholder; we always overwrite it here.
            e.name_arc = Arc::from(e.name.as_str());
            w.events.insert(e.name.clone(), Arc::new(e));
        }
        for mut t in tables {
            t.registered_at_version = new_version;
            w.tables.insert(t.name.clone(), t);
        }
        for mut d in derivations {
            d.registered_at_version = new_version;
            w.derivations.insert(d.name.clone(), d);
        }
        w.version = new_version;
    }

    /// Atomically install a batch of already-validated, non-conflicting
    /// `PayloadNode`s. Bumps version by 1 and stamps each NEW descriptor
    /// with `registered_at_version = new_version`. Existing
    /// (already_present) descriptors are left unchanged.
    ///
    /// Also installs compiled `OpChain`s (`compiled_chains`) and overwrites
    /// the derivation schema for any derivation that has a server-propagated
    /// schema (`propagated_schemas`). Both lists come from
    /// `ValidatedPayload::into_parts()`.
    ///
    /// Also installs compiled `AggregationDescriptor`s
    /// (`compiled_aggregations`). For aggregation derivations, the schema
    /// is overwritten with the server-authoritative aggregation output
    /// schema.
    ///
    /// Precondition: `nodes` has passed `validate_payload` and
    /// `classify_register_diff` yielded `destructive = []` AND at least one
    /// `NewDescriptor` in `additive`. Existing descriptors with matching
    /// names are silently skipped (insert-if-absent), so destructive
    /// changes must have been pre-removed by the caller.
    ///
    /// Returns the new version number.
    pub fn apply_registration(
        &self,
        nodes: Vec<crate::registry_diff::PayloadNode>,
        compiled_chains: Vec<(String, Arc<OpChain>)>,
        propagated_schemas: Vec<(String, crate::schema::DerivedSchema)>,
        compiled_aggregations: Vec<(String, Arc<AggregationDescriptor>)>,
    ) -> u64 {
        let mut w = self.inner.write();
        let new_version = w.version + 1;

        // Build lookup maps for propagated schemas, compiled chains, and
        // compiled aggregations so we can apply them alongside their
        // descriptor in the same write-lock pass. Chains/aggregations are
        // inserted ONLY when the derivation descriptor is new — this
        // prevents stale entries from accumulating if `apply_registration`
        // is ever called with a derivation that is already present.
        let schema_map: std::collections::HashMap<String, crate::schema::DerivedSchema> =
            propagated_schemas.into_iter().collect();
        let mut chains_map: std::collections::HashMap<String, Arc<OpChain>> =
            compiled_chains.into_iter().collect();

        // Pre-compute the per-source field-union (alphabetical-sorted
        // distinct fields any incoming agg consumes) so the
        // `EventDescriptor.apply_field_names` can be set as the new event
        // is inserted. The union is the union of declared fields across
        // all aggs targeting the same `source_node_name`. `BTreeSet`'s
        // iteration order is alphabetical — required for deterministic
        // `field_idx_into_event_extracted` resolution at register-time
        // and snapshot replay.
        let mut new_union_per_source: std::collections::HashMap<
            String,
            std::collections::BTreeSet<String>,
        > = std::collections::HashMap::new();
        for (_, agg_arc) in compiled_aggregations.iter() {
            let entry = new_union_per_source
                .entry(agg_arc.source_node_name.clone())
                .or_default();
            for feat in agg_arc.features.iter() {
                if let Some(f) = &feat.descriptor.field {
                    entry.insert(f.clone());
                }
                if let Some(lat) = &feat.descriptor.ext.lat_field {
                    entry.insert(lat.clone());
                }
                if let Some(lon) = &feat.descriptor.ext.lon_field {
                    entry.insert(lon.clone());
                }
            }
        }

        let mut agg_map: std::collections::HashMap<String, Arc<AggregationDescriptor>> =
            compiled_aggregations.into_iter().collect();

        // Track newly inserted aggregation node names for O(N_new) index update.
        let mut newly_inserted_agg_names: Vec<String> = Vec::new();

        for n in nodes {
            match n {
                crate::registry_diff::PayloadNode::Event(mut e) => {
                    if !w.events.contains_key(&e.name) {
                        e.registered_at_version = new_version;
                        // Pre-allocate the Arc<str> for the bookkeeping hot
                        // path (refcount bump per push, no String alloc).
                        // See `install_descriptors` for the companion site.
                        e.name_arc = Arc::from(e.name.as_str());
                        // Seed apply_field_names from the alphabetical-sorted
                        // field union for any aggs in this batch targeting
                        // this source. If a future `apply_registration`
                        // adds aggs targeting an existing source, the
                        // post-loop union-extend pass below re-derives
                        // `apply_field_names` against ALL aggs (new +
                        // pre-existing) so the union stays consistent.
                        if let Some(union) = new_union_per_source.get(&e.name) {
                            e.apply_field_names = union.iter().cloned().collect();
                        }
                        w.events.insert(e.name.clone(), Arc::new(e));
                    }
                }
                crate::registry_diff::PayloadNode::Table(mut t) => {
                    if !w.tables.contains_key(&t.name) {
                        t.registered_at_version = new_version;
                        w.tables.insert(t.name.clone(), t);
                    }
                }
                crate::registry_diff::PayloadNode::Derivation(mut d) => {
                    if !w.derivations.contains_key(&d.name) {
                        d.registered_at_version = new_version;
                        // Overwrite client-supplied schema with the
                        // server-authoritative propagated schema, if
                        // available.
                        if let Some(propagated) = schema_map.get(&d.name) {
                            d.schema = propagated.clone();
                        }
                        // Install compiled chain alongside descriptor —
                        // only for new derivations, so stale chains never
                        // accumulate.
                        if let Some(chain) = chains_map.remove(&d.name) {
                            w.compiled_chains.insert(d.name.clone(), chain);
                        }
                        // Install compiled aggregation descriptor and
                        // update the per-source index. Assign a stable
                        // u32 `agg_id` from the monotonic counter and
                        // write it into the descriptor before inserting.
                        // We must clone+mutate since the caller passed
                        // `Arc<Desc>`.
                        if let Some(agg) = agg_map.remove(&d.name) {
                            newly_inserted_agg_names.push(d.name.clone());
                            // Assign the next available agg_id.
                            let mut agg_owned = (*agg).clone();
                            agg_owned.agg_id = w.next_agg_id;
                            w.next_agg_id += 1;

                            // Assign cluster_id — aggregations sharing the
                            // same group_keys signature (declaration-order
                            // hash) share a cluster_id so the apply loop
                            // builds EntityKey ONCE per cluster, not once
                            // per agg. The signature is stable across
                            // restarts because it is computed from the
                            // group_keys in registration order (NOT
                            // sorted-lex) and uses 0u8 separators to avoid
                            // prefix collisions ("ab","c" ≠ "a","bc").
                            let sig = Self::cluster_signature(&agg_owned.group_keys);
                            agg_owned.cluster_id =
                                if let Some(&existing) = w.cluster_id_by_signature.get(&sig) {
                                    existing
                                } else {
                                    let id = w.next_cluster_id;
                                    w.next_cluster_id += 1;
                                    w.cluster_id_by_signature.insert(sig, id);
                                    id
                                };

                            // Resolve field indices at registration time.
                            // Look up the source event's schema to validate
                            // field references and populate `field_idx` on
                            // each feature descriptor, plus build
                            // `agg.field_names` (the per-agg distinct-fields
                            // list). `field_idx` indexes into
                            // `agg.field_names`; the apply loop pre-extracts
                            // by iterating `agg.field_names` once per event.
                            // Silently skip if the source event is not yet
                            // registered (the register-validate pass
                            // enforces ordering before we reach here).
                            if let Some(src_event) = w.events.get(&agg_owned.source_node_name) {
                                let schema = src_event.schema.clone();
                                // Pass the per-source apply_field_names
                                // union so the resolver can populate
                                // `field_idx_into_event_extracted`. For new
                                // events in this batch the union was seeded
                                // by the Event branch above; for cross-batch
                                // additions to existing events the union
                                // may be a subset of what THIS agg's fields
                                // ultimately need — the post-loop pass then
                                // refreshes `apply_field_names` (super-set)
                                // and re-resolves the indices.
                                let source_union = src_event.apply_field_names.clone();
                                // Ignore errors: register-validate already
                                // checked field refs. Any remaining
                                // mismatch is a latent inconsistency — do
                                // not panic in the write path.
                                let _ = Self::resolve_field_indices_for_agg_mut_inner(
                                    &mut agg_owned,
                                    &schema,
                                    &source_union,
                                );
                            }

                            let agg = Arc::new(agg_owned);
                            // Update aggregations_by_source for O(1) lookup at apply time.
                            let source_name = agg.source_node_name.clone();
                            w.aggregations_by_source
                                .entry(source_name)
                                .or_default()
                                .push(Arc::clone(&agg));
                            w.compiled_aggregations.insert(d.name.clone(), agg);
                        }
                        w.derivations.insert(d.name.clone(), d);
                    }
                }
            }
        }

        // Post-loop apply_field_names pass: walk the CURRENT registry's
        // `compiled_aggregations` (post-insert) and rebuild each affected
        // source's `apply_field_names` as the union of declared fields
        // across ALL aggs targeting that source. This handles the
        // cross-batch case where an Event was registered in a prior call
        // and a new agg in this batch declares fields beyond what the
        // prior union covered. Cost: O(N_aggs × M_features) at register
        // time only — register-time is cold-path; the apply-loop reads
        // the precomputed `apply_field_names` slice.
        //
        // Determinism: the union is built via BTreeSet → Vec, so iteration
        // order is alphabetical. `field_idx_into_event_extracted`
        // resolution reads this same alphabetical ordering, ensuring
        // reproducibility across snapshot replay.
        let affected_sources: std::collections::BTreeSet<String> = newly_inserted_agg_names
            .iter()
            .filter_map(|node_name| {
                w.compiled_aggregations
                    .get(node_name)
                    .map(|a| a.source_node_name.clone())
            })
            .collect();
        for source_name in affected_sources {
            // Recompute the alphabetical-sorted union of declared fields across
            // every agg that targets this source.
            let mut union: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            if let Some(aggs) = w.aggregations_by_source.get(&source_name) {
                for agg in aggs.iter() {
                    for feat in agg.features.iter() {
                        if let Some(f) = &feat.descriptor.field {
                            union.insert(f.clone());
                        }
                        if let Some(lat) = &feat.descriptor.ext.lat_field {
                            union.insert(lat.clone());
                        }
                        if let Some(lon) = &feat.descriptor.ext.lon_field {
                            union.insert(lon.clone());
                        }
                    }
                }
            }
            let new_fields: Vec<String> = union.into_iter().collect();
            // Update only if the union changed; avoids unnecessary Arc clones.
            if let Some(existing_arc) = w.events.get(&source_name) {
                if existing_arc.apply_field_names != new_fields {
                    let mut updated = (**existing_arc).clone();
                    updated.apply_field_names = new_fields.clone();
                    w.events.insert(source_name.clone(), Arc::new(updated));
                }
            }

            // Post-loop field_idx_into_event_extracted pass: for every
            // agg targeting `source_name`, re-resolve the per-feature
            // mapping against the (possibly extended) `new_fields` union.
            // The inline resolver call inside the per-derivation block
            // above ran with whatever `apply_field_names` was on the
            // EventDescriptor at that moment — which may have been a
            // subset if the source event was registered in a prior batch
            // and this batch only contributes new aggs. The post-pass
            // walks every Arc in `aggregations_by_source[source]`, clones
            // it, re-runs `resolve_field_indices_for_agg_mut_inner`
            // against the final union, and re-Arcs it. Both
            // `aggregations_by_source` and `compiled_aggregations` hold
            // `Arc<AggregationDescriptor>`, so we must replace the Arc in
            // BOTH maps to keep them consistent.
            let agg_clones: Vec<Arc<AggregationDescriptor>> = w
                .aggregations_by_source
                .get(&source_name)
                .map(|aggs| aggs.iter().map(Arc::clone).collect())
                .unwrap_or_default();
            let schema = w.events.get(&source_name).map(|e| e.schema.clone());
            if let Some(schema) = schema {
                let mut new_aggs: Vec<Arc<AggregationDescriptor>> =
                    Vec::with_capacity(agg_clones.len());
                for old_agg in agg_clones.iter() {
                    let mut owned = (**old_agg).clone();
                    let _ = Self::resolve_field_indices_for_agg_mut_inner(
                        &mut owned,
                        &schema,
                        &new_fields,
                    );
                    new_aggs.push(Arc::new(owned));
                }
                // Replace the Arcs in both maps to keep them consistent.
                if let Some(slot) = w.aggregations_by_source.get_mut(&source_name) {
                    *slot = new_aggs.clone();
                }
                for new_agg in new_aggs {
                    if let Some(slot) = w.compiled_aggregations.get_mut(new_agg.node_name.as_str())
                    {
                        *slot = new_agg;
                    }
                }
            }
        }

        // Update feature_index for ONLY the newly inserted aggregation
        // nodes (`O(N_new)` instead of `O(N_total)`). Additive-only:
        // existing entries are preserved via `entry().or_insert()`.
        // Collect new entries first to avoid simultaneous mutable +
        // immutable borrows of `w`.
        let new_index_entries: Vec<(String, String, usize)> = newly_inserted_agg_names
            .iter()
            .filter_map(|node_name| {
                w.compiled_aggregations
                    .get(node_name)
                    .map(|d| (node_name, d))
            })
            .flat_map(|(node_name, agg_desc)| {
                agg_desc
                    .features
                    .iter()
                    .enumerate()
                    .map(|(idx, named_op)| (named_op.feature_name.clone(), node_name.clone(), idx))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (feature_name, node_name, feature_idx) in new_index_entries {
            w.feature_index
                .entry(feature_name)
                .or_insert((node_name, feature_idx));
        }

        w.version = new_version;
        new_version
    }

    /// Drop ALL descriptors + ALL compiled chains/aggregations + ALL
    /// reverse indices, and bump `version` by 1 so any cached client
    /// `registry_version` becomes stale.
    ///
    /// Used exclusively by `OP_RESET`. The bump-by-1 (rather than reset to
    /// 0) preserves the "registry_version is monotonic" invariant that
    /// callers rely on for idempotent-replay deduplication and stale-cache
    /// detection. Callers that wish to observe the change without a
    /// re-register first can do so by polling `registry().version()`.
    ///
    /// Per-entity state tables are NOT touched here — they live in
    /// `DevAggState.state_tables` and are cleared by the caller (the apply
    /// shard's `dispatch_reset_sync`).
    pub fn clear(&self) {
        let mut w = self.inner.write();
        w.events.clear();
        w.tables.clear();
        w.derivations.clear();
        w.compiled_chains.clear();
        w.compiled_aggregations.clear();
        w.feature_index.clear();
        w.aggregations_by_source.clear();
        w.cluster_id_by_signature.clear();
        // next_agg_id stays monotonic — descriptors registered after a reset
        // get fresh IDs that don't collide with any prior state-table slot.
        // Same rationale for next_cluster_id.
        w.version += 1;
    }

    /// Returns the number of descriptors that were actually removed.
    pub fn force_remove_descriptors(&self, names: &[String]) -> usize {
        let mut w = self.inner.write();
        let mut removed = 0usize;
        for name in names {
            if w.events.remove(name).is_some() {
                removed += 1;
            }
            if w.tables.remove(name).is_some() {
                removed += 1;
            }
            if w.derivations.remove(name).is_some() {
                removed += 1;
            }
            // Compiled-side bookkeeping: also drop chains + aggregations + reverse indices.
            w.compiled_chains.remove(name);
            if let Some(agg) = w.compiled_aggregations.remove(name) {
                // Drop reverse-index entries pointing at this agg.
                if let Some(per_source) = w.aggregations_by_source.get_mut(&agg.source_node_name) {
                    per_source.retain(|a| a.node_name != agg.node_name);
                }
                // Drop feature_index entries that map to this agg's node_name.
                w.feature_index.retain(|_, v| v.0 != agg.node_name);
            }
        }
        removed
    }

    /// Validate field references in `agg` against `schema` and return an
    /// error if any field is missing. Does NOT mutate the descriptor.
    /// Use `resolve_field_indices_for_agg_mut` for the in-place mutation
    /// path.
    ///
    /// Error message format:
    ///   `"aggregation '{node}': field '{fname}' referenced by feature '{feature}' is not in source schema for event '{source}'"`
    pub fn resolve_field_indices_for_agg(
        &self,
        agg: &crate::agg_descriptor::AggregationDescriptor,
        schema: &crate::schema::EventSchema,
    ) -> Result<(), String> {
        for feat in &agg.features {
            if let Some(fname) = &feat.descriptor.field {
                if !schema.fields.contains_key(fname.as_str()) {
                    return Err(format!(
                        "aggregation '{}': field '{}' referenced by feature '{}' is not in source schema for event '{}'",
                        agg.node_name, fname, feat.feature_name, agg.source_node_name
                    ));
                }
            }
            // Geo ops: validate ext.lat_field + ext.lon_field if present.
            if let Some(lat) = &feat.descriptor.ext.lat_field {
                if !schema.fields.contains_key(lat.as_str()) {
                    return Err(format!(
                        "aggregation '{}': geo lat_field '{}' referenced by feature '{}' is not in source schema for event '{}'",
                        agg.node_name, lat, feat.feature_name, agg.source_node_name
                    ));
                }
            }
            if let Some(lon) = &feat.descriptor.ext.lon_field {
                if !schema.fields.contains_key(lon.as_str()) {
                    return Err(format!(
                        "aggregation '{}': geo lon_field '{}' referenced by feature '{}' is not in source schema for event '{}'",
                        agg.node_name, lon, feat.feature_name, agg.source_node_name
                    ));
                }
            }
        }
        Ok(())
    }

    /// Resolve field indices in-place on `agg`.
    ///
    /// For each feature with `field: Some(fname)`:
    ///   - Validates that `fname` exists in `schema`. Returns `Err` if not.
    ///   - Assigns `feature.descriptor.field_idx` as the index into
    ///     `agg.field_names` (inserting if not already present).
    ///   - Two features referencing the same field get the same `field_idx`.
    ///
    /// Features with `field: None` keep `field_idx = FIELD_IDX_NONE`.
    ///
    /// Also resolves geo `ext.lat_idx`/`ext.lon_idx` from
    /// `ext.lat_field`/`ext.lon_field` against the same `field_names` list,
    /// engaging the geo `update_at` fast path for every geo feature whose
    /// lat/lon fields exist in schema.
    ///
    /// Also writes `field_idx_into_event_extracted` on each feature — a
    /// `Vec<u8>` mapping the agg's local field positions
    /// (i.e. `agg.field_names` indices) to the per-source-event
    /// `apply_field_names` union indices. The apply-loop uses this
    /// mapping to remap `field_idx` lookups against the per-event union
    /// slice without per-descriptor rebuild scaffolding. When
    /// `source_union` is empty (e.g. test fixtures or callers that
    /// haven't migrated to the union form yet), this resolver leaves
    /// `field_idx_into_event_extracted` empty and `Sum`/`Min`/`Max` etc.
    /// fall back to the per-agg `field_idx` against `agg.field_names`.
    ///
    /// Populates `agg.field_names` with the distinct field list in
    /// resolution order.
    pub fn resolve_field_indices_for_agg_mut(
        &self,
        agg: &mut crate::agg_descriptor::AggregationDescriptor,
        schema: &crate::schema::EventSchema,
        source_union: &[String],
    ) -> Result<(), String> {
        use crate::agg_op::FIELD_IDX_NONE;

        // First pass: validate all field references exist.
        self.resolve_field_indices_for_agg(agg, schema)?;

        // Second pass: build field_names and assign field_idx +
        // lat_idx/lon_idx to each feature. The same `field_names` list is
        // referenced by `field_idx` (single-field ops) and
        // `lat_idx`/`lon_idx` (geo ops); apply-loop pre-extraction
        // populates one slot per `field_names` entry.
        let mut field_names: Vec<String> = Vec::new();

        // Build an O(1) lookup from union field name to its position in
        // source_union (the per-event-source `apply_field_names` union,
        // alphabetically sorted). Empty when `source_union` is empty —
        // signals that the apply path is still on the per-desc rebuild
        // codepath and the `field_idx_into_event_extracted` mapping is
        // unused.
        let union_lookup: std::collections::HashMap<&str, u8> = source_union
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i as u8))
            .collect();

        for feat in &mut agg.features {
            // Reset the union mapping; rebuilt below from the agg's local fields.
            feat.descriptor.field_idx_into_event_extracted.clear();

            if let Some(fname) = &feat.descriptor.field {
                let idx = if let Some(pos) = field_names.iter().position(|f| f == fname) {
                    pos
                } else {
                    let pos = field_names.len();
                    field_names.push(fname.clone());
                    pos
                };
                feat.descriptor.field_idx = idx as u8;
            } else {
                feat.descriptor.field_idx = FIELD_IDX_NONE;
            }

            // Resolve geo lat_idx/lon_idx alongside field_idx. Engages the
            // geo `update_at` fast path — the dispatch arms branch on
            // `if lat_idx != FIELD_IDX_NONE`, falling through to the slow
            // `update()` `row.get` path when unresolved.
            match (
                &feat.descriptor.ext.lat_field,
                &feat.descriptor.ext.lon_field,
            ) {
                (Some(lat_name), Some(lon_name)) => {
                    let lat_idx = if let Some(pos) = field_names.iter().position(|f| f == lat_name)
                    {
                        pos
                    } else {
                        let pos = field_names.len();
                        field_names.push(lat_name.clone());
                        pos
                    };
                    let lon_idx = if let Some(pos) = field_names.iter().position(|f| f == lon_name)
                    {
                        pos
                    } else {
                        let pos = field_names.len();
                        field_names.push(lon_name.clone());
                        pos
                    };
                    feat.descriptor.ext.lat_idx = lat_idx as u8;
                    feat.descriptor.ext.lon_idx = lon_idx as u8;
                }
                _ => {
                    // Partial or absent geo declaration — keep sentinel;
                    // dispatch falls through to the slow `update()` path
                    // which reads by field-name. Partial resolution would
                    // be a bug because dispatch only checks `lat_idx`.
                    feat.descriptor.ext.lat_idx = FIELD_IDX_NONE;
                    feat.descriptor.ext.lon_idx = FIELD_IDX_NONE;
                }
            }
        }

        agg.field_names = field_names;

        // Populate `field_idx_into_event_extracted` AFTER
        // `agg.field_names` is finalized. The mapping is per-feature:
        //   - For features WITH a declared `field` or geo `lat_field`/
        //     `lon_field`: the mapping has length = `agg.field_names.len()`,
        //     with entry `i` = union position of `agg.field_names[i]`.
        //     The apply-path consumer indexes via
        //     `field_idx_into_event_extracted[feat.descriptor.field_idx as usize]`
        //     (single-field ops) or
        //     `field_idx_into_event_extracted[feat.descriptor.ext.lat_idx as usize]`
        //     (geo ops).
        //   - For features WITHOUT a declared field (e.g. AggKind::Count):
        //     the mapping stays empty (length 0). The apply-path
        //     `if feat.descriptor.field_idx != FIELD_IDX_NONE` check
        //     short-circuits before indexing into the empty mapping.
        // When `source_union` is empty (legacy test path that hasn't
        // migrated to the union form), all mappings stay empty regardless
        // of feature shape; apply-loop dispatch falls back to the per-agg
        // `field_idx` codepath against `desc.field_names` and `extracted`
        // from a per-desc rebuild.
        if !source_union.is_empty() {
            for feat in &mut agg.features {
                let has_field = feat.descriptor.field.is_some()
                    || feat.descriptor.ext.lat_field.is_some()
                    || feat.descriptor.ext.lon_field.is_some();
                if !has_field {
                    continue;
                }
                for fname in agg.field_names.iter() {
                    if let Some(&u) = union_lookup.get(fname.as_str()) {
                        feat.descriptor.field_idx_into_event_extracted.push(u);
                    } else {
                        // Defensive: by construction every declared field
                        // is in the union. If a field is missing, the
                        // cross-batch post-pass hasn't yet refreshed the
                        // `EventDescriptor` — fall back to the
                        // `FIELD_IDX_NONE` sentinel so apply-path
                        // recognizes the absent slot and routes to the
                        // slow path.
                        feat.descriptor
                            .field_idx_into_event_extracted
                            .push(FIELD_IDX_NONE);
                    }
                }

                // Under the apply-loop hoist, `update_with_extracted`
                // consumes the per-event union slice (event_extracted) and
                // indexes it via lat_idx/lon_idx directly. Re-point
                // lat_idx/lon_idx from per-agg `agg.field_names` positions
                // to per-source `apply_field_names` union positions so the
                // geo dispatch arm reads from the hoisted slice correctly.
                if let (Some(lat), Some(lon)) = (
                    &feat.descriptor.ext.lat_field,
                    &feat.descriptor.ext.lon_field,
                ) {
                    if let (Some(&lat_u), Some(&lon_u)) = (
                        union_lookup.get(lat.as_str()),
                        union_lookup.get(lon.as_str()),
                    ) {
                        feat.descriptor.ext.lat_idx = lat_u;
                        feat.descriptor.ext.lon_idx = lon_u;
                    }
                    // If `union_lookup` is missing lat/lon, the prior
                    // assignment (per-agg `field_names` index) stays —
                    // this never happens in production because the
                    // post-pass ensures every declared field is in the
                    // union.
                }
            }
        }

        Ok(())
    }

    /// Static (no `&self`) version of `resolve_field_indices_for_agg_mut`,
    /// called inside the write-locked `apply_registration` closure where
    /// borrowing `self` is not possible.
    ///
    /// Same contract as `resolve_field_indices_for_agg_mut`:
    ///   - Validates field refs against `schema`. Returns `Err` on the
    ///     first missing field.
    ///   - Assigns `field_idx` (index into the per-agg `agg.field_names`
    ///     list).
    ///   - Resolves geo `ext.lat_idx`/`ext.lon_idx` against the same
    ///     `field_names` list — engages the `update_at` fast path. The
    ///     apply path calls THIS function, so the lat_idx/lon_idx
    ///     resolution must mirror the public version exactly.
    ///   - Writes per-feature `field_idx_into_event_extracted: Vec<u8>`
    ///     mapping the agg-local field positions to the per-source-event
    ///     `apply_field_names` union indices. See the public version's
    ///     doc-comment for shape semantics.
    ///   - Populates `agg.field_names` with the distinct ordered field
    ///     list.
    fn resolve_field_indices_for_agg_mut_inner(
        agg: &mut crate::agg_descriptor::AggregationDescriptor,
        schema: &crate::schema::EventSchema,
        source_union: &[String],
    ) -> Result<(), String> {
        use crate::agg_op::FIELD_IDX_NONE;

        // Validate all field references first.
        for feat in &agg.features {
            if let Some(fname) = &feat.descriptor.field {
                if !schema.fields.contains_key(fname.as_str()) {
                    return Err(format!(
                        "aggregation '{}': field '{}' referenced by feature '{}' is not in source schema for event '{}'",
                        agg.node_name, fname, feat.feature_name, agg.source_node_name
                    ));
                }
            }
            if let Some(lat) = &feat.descriptor.ext.lat_field {
                if !schema.fields.contains_key(lat.as_str()) {
                    return Err(format!(
                        "aggregation '{}': geo lat_field '{}' referenced by feature '{}' is not in source schema for event '{}'",
                        agg.node_name, lat, feat.feature_name, agg.source_node_name
                    ));
                }
            }
            if let Some(lon) = &feat.descriptor.ext.lon_field {
                if !schema.fields.contains_key(lon.as_str()) {
                    return Err(format!(
                        "aggregation '{}': geo lon_field '{}' referenced by feature '{}' is not in source schema for event '{}'",
                        agg.node_name, lon, feat.feature_name, agg.source_node_name
                    ));
                }
            }
        }

        // Build field_names and assign field_idx + lat_idx/lon_idx.
        // IDENTICAL logic to `resolve_field_indices_for_agg_mut`; both
        // functions produce the same `field_names` ordering for the same
        // input agg/schema.
        let mut field_names: Vec<String> = Vec::new();

        // O(1) lookup from union field name to its position in
        // `source_union` (same shape as the public sibling).
        let union_lookup: std::collections::HashMap<&str, u8> = source_union
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i as u8))
            .collect();

        for feat in &mut agg.features {
            // Clear the mapping; rebuilt below.
            feat.descriptor.field_idx_into_event_extracted.clear();

            if let Some(fname) = &feat.descriptor.field {
                let idx = if let Some(pos) = field_names.iter().position(|f| f == fname) {
                    pos
                } else {
                    let pos = field_names.len();
                    field_names.push(fname.clone());
                    pos
                };
                feat.descriptor.field_idx = idx as u8;
            } else {
                feat.descriptor.field_idx = FIELD_IDX_NONE;
            }

            // Geo lat_idx/lon_idx resolution. This is the runtime-critical
            // path: `apply_registration` invokes `_inner` from its
            // write-lock closure. Without this block fraud-team's geo
            // features stay on the slow `update()` arm.
            match (
                &feat.descriptor.ext.lat_field,
                &feat.descriptor.ext.lon_field,
            ) {
                (Some(lat_name), Some(lon_name)) => {
                    let lat_idx = if let Some(pos) = field_names.iter().position(|f| f == lat_name)
                    {
                        pos
                    } else {
                        let pos = field_names.len();
                        field_names.push(lat_name.clone());
                        pos
                    };
                    let lon_idx = if let Some(pos) = field_names.iter().position(|f| f == lon_name)
                    {
                        pos
                    } else {
                        let pos = field_names.len();
                        field_names.push(lon_name.clone());
                        pos
                    };
                    feat.descriptor.ext.lat_idx = lat_idx as u8;
                    feat.descriptor.ext.lon_idx = lon_idx as u8;
                }
                _ => {
                    feat.descriptor.ext.lat_idx = FIELD_IDX_NONE;
                    feat.descriptor.ext.lon_idx = FIELD_IDX_NONE;
                }
            }
        }
        agg.field_names = field_names;

        // Populate `field_idx_into_event_extracted` per feature against
        // the per-source `apply_field_names` union. Matches the public
        // sibling's logic exactly so both resolvers produce identical
        // mappings (snapshot replay invariance). Empty mapping for
        // features without a declared field. Also re-points
        // lat_idx/lon_idx into the union when present (apply-loop hoist
        // requirement).
        if !source_union.is_empty() {
            for feat in &mut agg.features {
                let has_field = feat.descriptor.field.is_some()
                    || feat.descriptor.ext.lat_field.is_some()
                    || feat.descriptor.ext.lon_field.is_some();
                if !has_field {
                    continue;
                }
                for fname in agg.field_names.iter() {
                    if let Some(&u) = union_lookup.get(fname.as_str()) {
                        feat.descriptor.field_idx_into_event_extracted.push(u);
                    } else {
                        feat.descriptor
                            .field_idx_into_event_extracted
                            .push(FIELD_IDX_NONE);
                    }
                }

                // Re-point lat_idx/lon_idx into the per-source union.
                if let (Some(lat), Some(lon)) = (
                    &feat.descriptor.ext.lat_field,
                    &feat.descriptor.ext.lon_field,
                ) {
                    if let (Some(&lat_u), Some(&lon_u)) = (
                        union_lookup.get(lat.as_str()),
                        union_lookup.get(lon.as_str()),
                    ) {
                        feat.descriptor.ext.lat_idx = lat_u;
                        feat.descriptor.ext.lon_idx = lon_u;
                    }
                }
            }
        }
        Ok(())
    }

    /// Validate that none of the aggregation's `group_keys` reference a
    /// float-typed column.
    ///
    /// Float group keys are rejected at register-time because NaN values are
    /// silently dropped at push time (they produce `None` from
    /// `EntityKeyShape::from_row`), which could cause confusing event drops.
    /// Users must cast float columns or ensure non-NaN before using as group key.
    ///
    /// Returns `Ok(())` if no float group keys are found. Returns `Err(String)`
    /// with a descriptive message if any group key has `FieldType::F64`.
    pub fn validate_group_keys_for_agg(
        &self,
        agg: &crate::agg_descriptor::AggregationDescriptor,
        schema: &crate::schema::EventSchema,
    ) -> Result<(), String> {
        for key in &agg.group_keys {
            if let Some(field_type) = schema.fields.get(key.as_str()) {
                if *field_type == crate::schema::FieldType::F64 {
                    return Err(format!(
                        "aggregation '{}': group_keys cannot include float field '{}' \
                         — float NaN values are rejected at push time. \
                         Use cast or ensure non-NaN.",
                        agg.node_name, key
                    ));
                }
            }
        }
        Ok(())
    }

    /// Compute a stable cluster signature hash for the given `group_keys`
    /// in DECLARATION ORDER (NOT sorted-lex).
    ///
    /// `EntityKey::from_row` produces order-sensitive keys (column_name +
    /// value pairs in a SmallVec; `Hash` derived over the SmallVec).
    /// Sorting would create `cluster_id` collisions for aggs that produce
    /// different `EntityKey`s at runtime, breaking the shared-lookup
    /// invariant.
    ///
    /// A separator byte (`0u8`) is hashed between each key so that
    /// `["a","bc"]` and `["ab","c"]` produce different signatures.
    fn cluster_signature(group_keys: &[String]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = fxhash::FxHasher::default();
        for k in group_keys {
            k.as_str().hash(&mut h);
            0u8.hash(&mut h); // separator to prevent prefix collisions
        }
        h.finish()
    }

    /// Install descriptors loaded from a snapshot.
    ///
    /// Replaces the in-memory registry contents with the descriptor set
    /// carried by a `RegistryDescriptorsOnly` (the projection produced by
    /// `SnapshotBody::from_live`). Runtime caches (compiled chains,
    /// compiled aggregations, feature index) are NOT rebuilt here —
    /// recovery replays `RegistryBump` WAL records via
    /// `apply_registration`, which compiles and caches them in normal
    /// flow. Cold start with snapshot only (no WAL records past the
    /// snapshot LSN): caches are empty until next register.
    ///
    /// Idempotent: calling twice with the same descriptors leaves the
    /// same state (modulo `version`, which is overwritten). Caller MUST
    /// hold the invariant that this runs BEFORE any concurrent reader
    /// (`Server::bind` runs recovery before flipping readiness).
    ///
    /// Snapshot recovery rebuilds **both** the descriptors AND the runtime
    /// caches (`compiled_chains`, `compiled_aggregations`, `feature_index`,
    /// `aggregations_by_source`, agg_id / cluster_id counters). Earlier
    /// versions deferred cache rebuild to a WAL replay of the original
    /// register record — but that record sits at LSN ≤ snapshot_lsn and
    /// the WAL replay path skips records up to `snapshot_lsn`, so the
    /// caches stayed empty whenever snapshot decode actually succeeded.
    /// (Pre-fix behavior accidentally "worked" because snapshot decode
    /// always failed for non-trivial pipelines and recovery fell through
    /// to a from-LSN-0 replay; once the bincode/`serde_json::Value` decode
    /// path was repaired, the latent gap surfaced as `feature_not_found`
    /// 404s on every restart.) Re-running the full register-validate
    /// compile path against the snapshot's descriptors restores the same
    /// shape the live register endpoint produces, so the apply hot path
    /// works unchanged.
    pub fn install_from_descriptors(&self, body: &crate::snapshot_body::RegistryDescriptorsOnly) {
        // Reset to a clean inner so `apply_registration` can re-insert
        // every descriptor + compiled state without hitting its
        // "already-present, skip" fast-path.
        {
            let mut w = self.inner.write();
            *w = RegistryInner::default();
        }

        // Build the payload in topological order: events / tables first,
        // derivations last. Mirrors what the live register endpoint accepts.
        let mut payload: Vec<crate::registry_diff::PayloadNode> = Vec::new();
        for e in body.events.values() {
            // Strip name_arc / apply_field_names — they're `#[serde(skip)]`
            // on the wire and `apply_registration` re-derives them. The
            // snapshot-body events come from `from_live`'s `(**v).clone()`
            // which carries whatever arc the live registry held; clearing
            // it puts us on the same footing as a fresh /register call.
            let mut ev = e.clone();
            ev.name_arc = default_event_name_arc();
            ev.apply_field_names = Vec::new();
            payload.push(crate::registry_diff::PayloadNode::Event(ev));
        }
        for t in body.tables.values() {
            payload.push(crate::registry_diff::PayloadNode::Table(t.clone()));
        }
        for d in body.derivations.values() {
            payload.push(crate::registry_diff::PayloadNode::Derivation(d.clone()));
        }

        let empty = RegistryInner::default();
        let validated = match crate::register_validate::validate_payload(&empty, payload) {
            Ok(v) => v,
            Err(errors) => {
                // The snapshot-stored descriptors were validated when
                // the original /register accepted them. If they fail
                // re-validation here it's a code bug (e.g. validation
                // rule tightened post-snapshot). Surface as a loud
                // error and fall back to descriptor-only install so
                // the server can still boot — caches stay empty
                // (matches pre-fix behavior, recovers via WAL replay
                // when the register record happens to be present).
                tracing::error!(
                    target: "beava.recovery",
                    kind = "recovery.snapshot_revalidate_failed",
                    error_count = errors.len(),
                    first_error = ?errors.first(),
                    "snapshot descriptors failed re-validation; falling back to descriptor-only install"
                );
                let mut w = self.inner.write();
                w.version = body.version;
                w.events = body
                    .events
                    .iter()
                    .map(|(k, v)| {
                        let mut ev = v.clone();
                        ev.name_arc = Arc::from(ev.name.as_str());
                        (k.clone(), Arc::new(ev))
                    })
                    .collect();
                w.tables = body.tables.clone();
                w.derivations = body.derivations.clone();
                return;
            }
        };

        // Re-run the same registration path the live `/register` handler
        // uses. apply_registration starts from version 0 (we just
        // cleared), so it increments to 1 — we override below to
        // preserve the snapshot's recorded version.
        self.apply_registration(
            validated.nodes,
            validated.compiled_chains,
            validated.propagated_schemas,
            validated.compiled_aggregations,
        );

        // Override version to match the snapshot's recorded version.
        // `apply_registration` set it to its own incremented value
        // (typically 1 after a clean reset).
        {
            let mut w = self.inner.write();
            w.version = body.version;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{EventSchema, FieldType};
    use std::collections::BTreeMap;

    /// Guards the post-pivot surface at the `EventDescriptor` level. Reads
    /// the source via `include_str!` and asserts the legacy event-time
    /// tokens are absent.
    ///
    /// **Forbidden tokens are reconstructed at runtime via chunked
    /// `concat`** so the test source itself does not contain the literals
    /// it forbids. The function name is also chunk-friendly (avoids
    /// `event_time_field` / `tolerate_delay_ms` as a substring) so the
    /// `include_str!` grep doesn't flag the test on itself.
    #[test]
    fn event_descriptor_post_d03_has_no_legacy_decorator_keys() {
        let src = include_str!("registry.rs");
        // Strip line comments so doc-comments mentioning the historical
        // field name don't false-positive the assertion.
        let stripped: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .filter(|l| !l.trim_start().starts_with("///"))
            .filter(|l| !l.trim_start().starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n");
        let forbidden_etf = ["event", "_time_field:"].concat();
        let forbidden_tdm = ["tolerate", "_delay_ms:"].concat();
        assert!(
            !stripped.contains(&forbidden_etf),
            "EventDescriptor must not declare an `{forbidden_etf}` field. Found in registry.rs source."
        );
        assert!(
            !stripped.contains(&forbidden_tdm),
            "EventDescriptor must not declare a `{forbidden_tdm}` field. Found in registry.rs source."
        );
    }

    fn make_event_schema() -> EventSchema {
        let mut fields = BTreeMap::new();
        fields.insert("card_id".to_string(), FieldType::Str);
        fields.insert("amount".to_string(), FieldType::F64);
        fields.insert("merchant_id".to_string(), FieldType::Str);
        fields.insert("event_time".to_string(), FieldType::I64);
        EventSchema {
            fields,
            optional_fields: vec![],
        }
    }

    // EventDescriptor JSON round-trip (Transaction fixture).
    //
    // Per `project_redis_shaped_no_event_time_ever` the legacy
    // `event_time_field` / `tolerate_delay_ms` keys never round-trip the
    // post-pivot descriptor. (The strict-deny shim in
    // `register_validate::pre_check_legacy_event_time_keys` catches them
    // at the dispatch layer; this test exercises the type-level round-trip
    // without those keys.)
    #[test]
    fn event_descriptor_json_round_trip() {
        let json = r#"{
            "name": "Transaction",
            "schema": {
                "fields": {
                    "card_id": "str",
                    "amount": "f64",
                    "merchant_id": "str"
                },
                "optional_fields": []
            },
            "dedupe_key": "request_id",
            "dedupe_window_ms": 86400000,
            "keep_events_for_ms": 604800000
        }"#;

        let desc: EventDescriptor = serde_json::from_str(json).unwrap();
        assert_eq!(desc.name, "Transaction");
        assert_eq!(desc.dedupe_key, Some("request_id".to_string()));
        assert_eq!(desc.dedupe_window_ms, Some(86_400_000));
        assert_eq!(desc.keep_events_for_ms, Some(604_800_000));
        assert_eq!(desc.registered_at_version, 0); // defaulted
        assert_eq!(desc.schema.fields.get("amount"), Some(&FieldType::F64));

        // Re-serialize and re-parse → must match
        let re_json = serde_json::to_string(&desc).unwrap();
        let desc2: EventDescriptor = serde_json::from_str(&re_json).unwrap();
        assert_eq!(desc, desc2);
    }

    #[test]
    fn table_descriptor_json_round_trip() {
        let json = r#"{
            "name": "Merchant",
            "primary_key": ["merchant_id"],
            "schema": {
                "fields": {
                    "merchant_id": "str",
                    "name": "str",
                    "category": "str"
                },
                "optional_fields": ["category"]
            },
            "ttl_ms": 2592000000,
            "mode": "upsert"
        }"#;

        let desc: TableDescriptor = serde_json::from_str(json).unwrap();
        assert_eq!(desc.name, "Merchant");
        assert_eq!(desc.primary_key, vec!["merchant_id".to_string()]);
        assert_eq!(desc.ttl_ms, Some(2_592_000_000));
        assert_eq!(desc.mode, TableMode::Upsert);
        assert_eq!(desc.schema.optional_fields, vec!["category".to_string()]);
        assert_eq!(desc.registered_at_version, 0);

        let re_json = serde_json::to_string(&desc).unwrap();
        let desc2: TableDescriptor = serde_json::from_str(&re_json).unwrap();
        assert_eq!(desc, desc2);
    }

    // Temporal table flag + retention_ms round-trip. `TableDescriptor`
    // carries `temporal: bool` and optional `retention_ms: u64` (MVCC
    // history-window). Defaults to (false, None) when absent so legacy
    // tables continue to deserialize.
    #[test]
    fn temporal_table_descriptor_round_trips() {
        // Sub-assertion 1: explicit construction round-trips.
        let desc = TableDescriptor {
            name: "merch".to_string(),
            primary_key: vec!["mid".to_string()],
            schema: TableSchema {
                fields: [("mid".to_string(), FieldType::Str)].into_iter().collect(),
                optional_fields: vec![],
            },
            ttl_ms: None,
            mode: TableMode::Upsert,
            registered_at_version: 0,
            temporal: true,
            retention_ms: Some(7 * 86_400_000),
        };
        let s = serde_json::to_string(&desc).unwrap();
        let back: TableDescriptor = serde_json::from_str(&s).unwrap();
        assert!(back.temporal);
        assert_eq!(back.retention_ms, Some(604_800_000));

        // Sub-assertion 2: client-shape JSON parses with both new fields set.
        let json = r#"{
            "name": "merch",
            "primary_key": ["mid"],
            "schema": {"fields": {"mid": "str"}, "optional_fields": []},
            "mode": "upsert",
            "temporal": true,
            "retention_ms": 3600000
        }"#;
        let desc2: TableDescriptor = serde_json::from_str(json).unwrap();
        assert!(desc2.temporal);
        assert_eq!(desc2.retention_ms, Some(3_600_000));

        // Sub-assertion 3: backwards-compat — JSON without the new fields
        // defaults `temporal` to false and `retention_ms` to None.
        let legacy_json = r#"{
            "name": "u",
            "primary_key": ["k"],
            "schema": {"fields": {"k": "str"}, "optional_fields": []},
            "mode": "upsert"
        }"#;
        let desc3: TableDescriptor = serde_json::from_str(legacy_json).unwrap();
        assert!(!desc3.temporal);
        assert_eq!(desc3.retention_ms, None);
    }

    #[test]
    fn table_mode_unknown_variant_rejected() {
        let result: Result<TableMode, _> = serde_json::from_str("\"changelog\"");
        assert!(result.is_err(), "expected Err for 'changelog'");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("unknown variant") || msg.contains("changelog"),
            "error should mention unknown variant, got: {msg}"
        );
    }

    #[test]
    fn output_kind_serde() {
        let e: OutputKind = serde_json::from_str("\"event\"").unwrap();
        assert_eq!(e, OutputKind::Event);
        let t: OutputKind = serde_json::from_str("\"table\"").unwrap();
        assert_eq!(t, OutputKind::Table);

        let result: Result<OutputKind, _> = serde_json::from_str("\"derivation\"");
        assert!(result.is_err(), "expected Err for 'derivation'");
    }

    #[test]
    fn registry_new_starts_empty() {
        let r = Registry::new();
        assert_eq!(r.version(), 0);
        {
            let inner = r.read();
            assert!(inner.events.is_empty());
            assert!(inner.tables.is_empty());
            assert!(inner.derivations.is_empty());
        }
        let snap = r.snapshot();
        assert_eq!(snap.version, 0);
        assert!(snap.events.is_empty());
    }

    #[test]
    fn equality_ignores_registered_at_version() {
        let schema = make_event_schema();
        let a = EventDescriptor {
            name: "A".to_string(),
            schema: schema.clone(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 1,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };
        let mut b = a.clone();
        b.registered_at_version = 99;

        assert_ne!(a, b, "derived PartialEq includes registered_at_version");

        assert!(
            a.equiv_ignoring_version(&b),
            "equiv_ignoring_version must ignore registered_at_version"
        );
    }

    #[test]
    fn install_descriptors_increments_version() {
        let r = Registry::new();
        let schema = make_event_schema();

        let event_a = EventDescriptor {
            name: "Transaction".to_string(),
            schema: schema.clone(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };

        r.install_descriptors(1, vec![event_a], vec![], vec![]);
        assert_eq!(r.version(), 1);
        {
            let inner = r.read();
            assert!(inner.events.contains_key("Transaction"));
            assert_eq!(
                inner.events["Transaction"].registered_at_version, 1,
                "registered_at_version should be stamped with install version"
            );
        }

        let deriv = DerivationDescriptor {
            name: "BigTx".to_string(),
            output_kind: OutputKind::Event,
            upstreams: vec!["Transaction".to_string()],
            ops: vec![],
            schema: crate::schema::DerivedSchema {
                fields: {
                    let mut m = BTreeMap::new();
                    m.insert("amount".to_string(), FieldType::F64);
                    m
                },
                optional_fields: vec![],
            },
            table_primary_key: None,
            registered_at_version: 0,
        };
        r.install_descriptors(2, vec![], vec![], vec![deriv]);
        assert_eq!(r.version(), 2);
        let snap = r.snapshot();
        assert!(snap.events.contains_key("Transaction"));
        assert!(snap.derivations.contains_key("BigTx"));
        assert_eq!(snap.derivations["BigTx"].registered_at_version, 2);
    }

    // The outer JSON uses "kind" discrimination which is handled at the
    // payload-parsing layer; here we test the inner descriptor shape
    // directly without "kind".
    #[test]
    fn derivation_with_ops_round_trip() {
        let json = r#"{
            "name": "BigTx",
            "output_kind": "event",
            "upstreams": ["Transaction"],
            "ops": [{"op": "filter", "expr": "(amount > 500)"}],
            "schema": {
                "fields": {
                    "card_id": "str",
                    "amount": "f64",
                    "event_time": "i64"
                },
                "optional_fields": []
            }
        }"#;
        let d: DerivationDescriptor = serde_json::from_str(json).unwrap();
        assert_eq!(d.ops.len(), 1);
        assert_eq!(
            d.ops[0],
            crate::op_node::OpNode::Filter {
                expr: "(amount > 500)".to_string()
            }
        );
        let j2 = serde_json::to_string(&d).unwrap();
        let d2: DerivationDescriptor = serde_json::from_str(&j2).unwrap();
        assert_eq!(d.name, d2.name);
        assert_eq!(d.ops, d2.ops);
    }

    #[test]
    fn derivation_with_group_by_round_trip() {
        let json = r#"{
            "name": "UserTxCount",
            "output_kind": "table",
            "upstreams": ["Transaction"],
            "ops": [{"op": "group_by", "keys": ["card_id"], "agg": {"cnt": {"op": "count", "params": {}}}}],
            "schema": {"fields": {"card_id": "str", "cnt": "i64"}, "optional_fields": []},
            "table_primary_key": ["card_id"]
        }"#;
        let d: DerivationDescriptor = serde_json::from_str(json).unwrap();
        assert_eq!(d.output_kind, OutputKind::Table);
        assert_eq!(d.table_primary_key, Some(vec!["card_id".to_string()]));
        let j2 = serde_json::to_string(&d).unwrap();
        let d2: DerivationDescriptor = serde_json::from_str(&j2).unwrap();
        assert_eq!(d.ops, d2.ops);
    }

    #[test]
    fn derivation_equiv_ignoring_version_with_ops() {
        let schema = crate::schema::DerivedSchema {
            fields: {
                let mut m = BTreeMap::new();
                m.insert("amount".to_string(), FieldType::F64);
                m
            },
            optional_fields: vec![],
        };
        let base = DerivationDescriptor {
            name: "D".to_string(),
            output_kind: OutputKind::Event,
            upstreams: vec!["A".to_string()],
            ops: vec![crate::op_node::OpNode::Filter {
                expr: "(amount > 1)".to_string(),
            }],
            schema: schema.clone(),
            table_primary_key: None,
            registered_at_version: 1,
        };

        let mut same_diff_version = base.clone();
        same_diff_version.registered_at_version = 99;
        assert!(
            base.equiv_ignoring_version(&same_diff_version),
            "must be equiv when only version differs"
        );

        let mut diff_ops = base.clone();
        diff_ops.ops = vec![crate::op_node::OpNode::Filter {
            expr: "(amount > 999)".to_string(),
        }];
        assert!(
            !base.equiv_ignoring_version(&diff_ops),
            "must NOT be equiv when ops differ"
        );
    }

    #[test]
    fn apply_registration_installs_events() {
        use crate::registry_diff::PayloadNode;
        let r = Registry::new();
        let schema = make_event_schema();
        let event_a = EventDescriptor {
            name: "A".to_string(),
            schema,
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };
        let new_version =
            r.apply_registration(vec![PayloadNode::Event(event_a)], vec![], vec![], vec![]);
        assert_eq!(new_version, 1);
        assert_eq!(r.version(), 1);
        let snap = r.snapshot();
        assert!(snap.events.contains_key("A"));
        assert_eq!(snap.events["A"].registered_at_version, 1);
    }

    #[test]
    fn apply_registration_bumps_version_linear() {
        use crate::registry_diff::PayloadNode;
        let r = Registry::new();

        let e1 = EventDescriptor {
            name: "E1".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };
        let v1 = r.apply_registration(vec![PayloadNode::Event(e1)], vec![], vec![], vec![]);
        assert_eq!(v1, 1);

        let e2 = EventDescriptor {
            name: "E2".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };
        let v2 = r.apply_registration(vec![PayloadNode::Event(e2)], vec![], vec![], vec![]);
        assert_eq!(v2, 2);

        let snap = r.snapshot();
        assert_eq!(snap.events["E1"].registered_at_version, 1);
        assert_eq!(snap.events["E2"].registered_at_version, 2);
    }

    #[test]
    fn apply_registration_skips_already_present() {
        use crate::registry_diff::PayloadNode;
        let r = Registry::new();

        let event_a = EventDescriptor {
            name: "A".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };
        r.apply_registration(
            vec![PayloadNode::Event(event_a.clone())],
            vec![],
            vec![],
            vec![],
        );
        assert_eq!(r.version(), 1);

        let event_b = EventDescriptor {
            name: "B".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };
        let v2 = r.apply_registration(
            vec![PayloadNode::Event(event_a), PayloadNode::Event(event_b)],
            vec![],
            vec![],
            vec![],
        );
        assert_eq!(v2, 2);
        let snap = r.snapshot();
        assert_eq!(snap.events["A"].registered_at_version, 1);
        assert_eq!(snap.events["B"].registered_at_version, 2);
    }

    #[test]
    fn resolve_feature_after_register() {
        use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
        use crate::agg_op::{AggKind, AggOpDescriptor};
        use crate::registry_diff::PayloadNode;

        let r = Registry::new();
        let event = EventDescriptor {
            name: "Txn".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };
        let agg_desc = AggregationDescriptor {
            node_name: "AggTable".to_string(),
            source_node_name: "Txn".to_string(),
            group_keys: vec!["card_id".to_string()],
            features: vec![
                NamedAggOp {
                    feature_name: "cnt".to_string(),
                    descriptor: AggOpDescriptor {
                        kind: AggKind::Count,
                        field: None,
                        window_ms: Some(300_000),
                        where_expr: None,
                        n: None,
                        half_life_ms: None,
                        sub_window_ms: None,
                        sigma: None,
                        sketch_params: None,
                        ext: Default::default(),
                        field_idx: crate::agg_op::FIELD_IDX_NONE,
                        field_idx_into_event_extracted: Vec::new(),
                    },
                },
                NamedAggOp {
                    feature_name: "total".to_string(),
                    descriptor: AggOpDescriptor {
                        kind: AggKind::Sum,
                        field: Some("amount".to_string()),
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
                    },
                },
            ],
            agg_id: 0, // placeholder; registry overwrites at apply_registration
            field_names: vec![],
            cluster_id: 0,
        };
        let deriv = DerivationDescriptor {
            name: "AggTable".to_string(),
            output_kind: OutputKind::Table,
            upstreams: vec!["Txn".to_string()],
            ops: vec![],
            schema: crate::schema::DerivedSchema {
                fields: {
                    let mut m = BTreeMap::new();
                    m.insert("card_id".to_string(), crate::schema::FieldType::Str);
                    m.insert("cnt".to_string(), crate::schema::FieldType::I64);
                    m.insert("total".to_string(), crate::schema::FieldType::F64);
                    m
                },
                optional_fields: vec![],
            },
            table_primary_key: Some(vec!["card_id".to_string()]),
            registered_at_version: 0,
        };
        r.apply_registration(
            vec![PayloadNode::Event(event), PayloadNode::Derivation(deriv)],
            vec![],
            vec![],
            vec![("AggTable".to_string(), Arc::new(agg_desc))],
        );

        let cnt = r.resolve_feature("cnt");
        assert!(cnt.is_some(), "resolve_feature('cnt') must return Some");
        let (node, idx) = cnt.unwrap();
        assert_eq!(node, "AggTable");
        assert_eq!(idx, 0, "cnt is at feature_index 0");

        let total = r.resolve_feature("total");
        assert!(total.is_some(), "resolve_feature('total') must return Some");
        let (node2, idx2) = total.unwrap();
        assert_eq!(node2, "AggTable");
        assert_eq!(idx2, 1, "total is at feature_index 1");
    }

    #[test]
    fn resolve_feature_missing_returns_none() {
        let r = Registry::new();
        assert!(
            r.resolve_feature("unknown").is_none(),
            "resolve_feature('unknown') must return None on empty registry"
        );
    }

    #[test]
    fn resolve_feature_index_rebuilt_on_register() {
        use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
        use crate::agg_op::{AggKind, AggOpDescriptor};
        use crate::registry_diff::PayloadNode;

        let r = Registry::new();

        let event_a = EventDescriptor {
            name: "EvA".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };
        let agg_a = Arc::new(AggregationDescriptor {
            node_name: "AggA".to_string(),
            source_node_name: "EvA".to_string(),
            group_keys: vec!["card_id".to_string()],
            features: vec![NamedAggOp {
                feature_name: "cnt".to_string(),
                descriptor: AggOpDescriptor {
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
                },
            }],
            agg_id: 0,
            field_names: vec![],
            cluster_id: 0,
        });
        let deriv_a = DerivationDescriptor {
            name: "AggA".to_string(),
            output_kind: OutputKind::Table,
            upstreams: vec!["EvA".to_string()],
            ops: vec![],
            schema: crate::schema::DerivedSchema {
                fields: {
                    let mut m = BTreeMap::new();
                    m.insert("card_id".to_string(), crate::schema::FieldType::Str);
                    m.insert("cnt".to_string(), crate::schema::FieldType::I64);
                    m
                },
                optional_fields: vec![],
            },
            table_primary_key: Some(vec!["card_id".to_string()]),
            registered_at_version: 0,
        };
        r.apply_registration(
            vec![
                PayloadNode::Event(event_a),
                PayloadNode::Derivation(deriv_a),
            ],
            vec![],
            vec![],
            vec![("AggA".to_string(), agg_a)],
        );
        assert!(
            r.resolve_feature("cnt").is_some(),
            "cnt must be in index after first register"
        );

        let event_b = EventDescriptor {
            name: "EvB".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };
        let agg_b = Arc::new(AggregationDescriptor {
            node_name: "AggB".to_string(),
            source_node_name: "EvB".to_string(),
            group_keys: vec!["card_id".to_string()],
            features: vec![NamedAggOp {
                feature_name: "revenue".to_string(),
                descriptor: AggOpDescriptor {
                    kind: AggKind::Sum,
                    field: Some("amount".to_string()),
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
                },
            }],
            agg_id: 0,
            field_names: vec![],
            cluster_id: 0,
        });
        let deriv_b = DerivationDescriptor {
            name: "AggB".to_string(),
            output_kind: OutputKind::Table,
            upstreams: vec!["EvB".to_string()],
            ops: vec![],
            schema: crate::schema::DerivedSchema {
                fields: {
                    let mut m = BTreeMap::new();
                    m.insert("card_id".to_string(), crate::schema::FieldType::Str);
                    m.insert("revenue".to_string(), crate::schema::FieldType::F64);
                    m
                },
                optional_fields: vec![],
            },
            table_primary_key: Some(vec!["card_id".to_string()]),
            registered_at_version: 0,
        };
        r.apply_registration(
            vec![
                PayloadNode::Event(event_b),
                PayloadNode::Derivation(deriv_b),
            ],
            vec![],
            vec![],
            vec![("AggB".to_string(), agg_b)],
        );

        assert!(
            r.resolve_feature("cnt").is_some(),
            "cnt must still be in index after second register"
        );
        assert!(
            r.resolve_feature("revenue").is_some(),
            "revenue must be in index after second register"
        );
        let (node, _) = r.resolve_feature("revenue").unwrap();
        assert_eq!(node, "AggB");
    }

    #[test]
    fn compiled_aggregations_cached_after_apply_registration() {
        use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
        use crate::agg_op::{AggKind, AggOpDescriptor};
        use crate::registry_diff::PayloadNode;

        let r = Registry::new();

        let event = EventDescriptor {
            name: "Txn".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };

        let agg_desc = AggregationDescriptor {
            node_name: "AggTable".to_string(),
            source_node_name: "Txn".to_string(),
            group_keys: vec!["card_id".to_string()],
            features: vec![NamedAggOp {
                feature_name: "cnt".to_string(),
                descriptor: AggOpDescriptor {
                    kind: AggKind::Count,
                    field: None,
                    window_ms: Some(300_000),
                    where_expr: None,
                    n: None,
                    half_life_ms: None,
                    sub_window_ms: None,
                    sigma: None,
                    sketch_params: None,
                    ext: Default::default(),
                    field_idx: crate::agg_op::FIELD_IDX_NONE,
                    field_idx_into_event_extracted: Vec::new(),
                },
            }],
            agg_id: 0, // placeholder; registry overwrites at apply_registration
            field_names: vec![],
            cluster_id: 0,
        };
        let agg_arc = Arc::new(agg_desc);

        let deriv = crate::registry::DerivationDescriptor {
            name: "AggTable".to_string(),
            output_kind: crate::registry::OutputKind::Table,
            upstreams: vec!["Txn".to_string()],
            ops: vec![],
            schema: crate::schema::DerivedSchema {
                fields: {
                    let mut m = BTreeMap::new();
                    m.insert("card_id".to_string(), crate::schema::FieldType::Str);
                    m.insert("cnt".to_string(), crate::schema::FieldType::I64);
                    m
                },
                optional_fields: vec![],
            },
            table_primary_key: Some(vec!["card_id".to_string()]),
            registered_at_version: 0,
        };

        r.apply_registration(
            vec![PayloadNode::Event(event), PayloadNode::Derivation(deriv)],
            vec![],
            vec![],
            vec![("AggTable".to_string(), agg_arc)],
        );

        let cached = r.compiled_aggregation("AggTable");
        assert!(
            cached.is_some(),
            "registry.compiled_aggregation('AggTable') must return Some after registration"
        );
        let cached = cached.unwrap();
        assert_eq!(cached.node_name, "AggTable");
        assert_eq!(cached.source_node_name, "Txn");
        assert_eq!(cached.group_keys, vec!["card_id"]);
        assert_eq!(cached.features.len(), 1);
        assert_eq!(cached.features[0].feature_name, "cnt");
    }

    // Pins three `name_arc` invariants:
    //   1. `name_arc.as_ref() == name` after install (population at
    //      register time).
    //   2. Consecutive `get_event_descriptor` calls return `Arc`s whose
    //      `name_arc` field shares the same `Arc<str>` allocation (proves
    //      the Arc is registry-owned, not re-derived per-call).
    //   3. The `EventDescriptor` itself is also shared via `Arc::ptr_eq`
    //      (carrier for invariant 2).
    #[test]
    fn event_descriptor_has_name_arc_and_registry_shares_it() {
        let r = Registry::new();
        let event = EventDescriptor {
            name: "Txn".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""), // server overwrites at install; value here is a placeholder
            apply_field_names: vec![],
        };
        r.install_descriptors(1, vec![event], vec![], vec![]);

        let d1 = r
            .get_event_descriptor("Txn")
            .expect("descriptor should be present after install");
        let d2 = r
            .get_event_descriptor("Txn")
            .expect("descriptor should be present on second lookup");

        assert_eq!(
            d1.name_arc.as_ref(),
            "Txn",
            "install_descriptors must populate name_arc to the descriptor's name"
        );

        assert!(
            Arc::ptr_eq(&d1.name_arc, &d2.name_arc),
            "consecutive get_event_descriptor calls must share the same name_arc allocation"
        );

        assert!(
            Arc::ptr_eq(&d1, &d2),
            "consecutive get_event_descriptor calls must share the same EventDescriptor Arc"
        );
    }

    // Companion: `apply_registration` also populates `name_arc`. Mirrors
    // the `install_descriptors` path so both registry-entry sites stay
    // covered.
    #[test]
    fn apply_registration_populates_name_arc() {
        use crate::registry_diff::PayloadNode;

        let r = Registry::new();
        let event = EventDescriptor {
            name: "Click".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""), // overwritten server-side at apply_registration
            apply_field_names: vec![],
        };
        r.apply_registration(vec![PayloadNode::Event(event)], vec![], vec![], vec![]);

        let d = r
            .get_event_descriptor("Click")
            .expect("descriptor should be present after apply_registration");
        assert_eq!(
            d.name_arc.as_ref(),
            "Click",
            "apply_registration must populate name_arc to the descriptor's name"
        );
    }

    // `AggregationDescriptor.agg_id` must be assigned monotonically.
    // Registering two aggregations sequentially must yield agg_id 0 and 1;
    // re-registering an existing aggregation must be a no-op (additive
    // idempotence — agg_id stays the same).
    #[test]
    fn test_agg_id_assigned_monotonically() {
        use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
        use crate::agg_op::{AggKind, AggOpDescriptor};
        use crate::registry_diff::PayloadNode;

        let r = Registry::new();

        let make_agg = |node_name: &str, source: &str| -> AggregationDescriptor {
            AggregationDescriptor {
                node_name: node_name.to_string(),
                source_node_name: source.to_string(),
                group_keys: vec!["user_id".to_string()],
                features: vec![NamedAggOp {
                    feature_name: "cnt".to_string(),
                    descriptor: AggOpDescriptor {
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
                    },
                }],
                agg_id: 0, // overwritten by the registry at registration time
                field_names: vec![],
                cluster_id: 0,
            }
        };

        let make_event = |name: &str| -> EventDescriptor {
            EventDescriptor {
                name: name.to_string(),
                schema: make_event_schema(),
                dedupe_key: None,
                dedupe_window_ms: None,
                keep_events_for_ms: None,
                cold_after_ms: None,
                registered_at_version: 0,
                name_arc: Arc::from(""),
                apply_field_names: vec![],
            }
        };

        let make_deriv = |name: &str, source: &str| -> DerivationDescriptor {
            DerivationDescriptor {
                name: name.to_string(),
                output_kind: OutputKind::Table,
                upstreams: vec![source.to_string()],
                ops: vec![],
                schema: crate::schema::DerivedSchema {
                    fields: {
                        let mut m = BTreeMap::new();
                        m.insert("user_id".to_string(), crate::schema::FieldType::Str);
                        m.insert("cnt".to_string(), crate::schema::FieldType::I64);
                        m
                    },
                    optional_fields: vec![],
                },
                table_primary_key: Some(vec!["user_id".to_string()]),
                registered_at_version: 0,
            }
        };

        let agg_a = Arc::new(make_agg("AggA", "EvA"));
        r.apply_registration(
            vec![
                PayloadNode::Event(make_event("EvA")),
                PayloadNode::Derivation(make_deriv("AggA", "EvA")),
            ],
            vec![],
            vec![],
            vec![("AggA".to_string(), Arc::clone(&agg_a))],
        );

        let cached_a = r
            .compiled_aggregation("AggA")
            .expect("AggA must be in registry after registration");
        assert_eq!(
            cached_a.agg_id, 0,
            "first registered aggregation must get agg_id=0"
        );

        let agg_b = Arc::new(make_agg("AggB", "EvB"));
        r.apply_registration(
            vec![
                PayloadNode::Event(make_event("EvB")),
                PayloadNode::Derivation(make_deriv("AggB", "EvB")),
            ],
            vec![],
            vec![],
            vec![("AggB".to_string(), Arc::clone(&agg_b))],
        );

        let cached_b = r
            .compiled_aggregation("AggB")
            .expect("AggB must be in registry after registration");
        assert_eq!(
            cached_b.agg_id, 1,
            "second registered aggregation must get agg_id=1"
        );

        let agg_a_again = Arc::new(make_agg("AggA", "EvA"));
        r.apply_registration(
            vec![PayloadNode::Derivation(make_deriv("AggA", "EvA"))],
            vec![],
            vec![],
            vec![("AggA".to_string(), Arc::clone(&agg_a_again))],
        );

        let cached_a_again = r
            .compiled_aggregation("AggA")
            .expect("AggA must still be in registry after re-registration");
        assert_eq!(
            cached_a_again.agg_id, 0,
            "re-registration must not change agg_id (additive idempotence)"
        );
    }

    // `lat_idx`/`lon_idx` must be resolved at register time so the geo
    // `update_at` fast path engages on the apply hot path. Before this
    // resolution lat/lon defaulted to `FIELD_IDX_NONE` and the dispatch
    // fell through to the slow `update()` arm.
    #[test]
    fn geo_feature_resolves_lat_idx_and_lon_idx_at_register_time() {
        use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
        use crate::agg_op::{AggExtParams, AggKind, AggOpDescriptor, FIELD_IDX_NONE};
        use crate::schema::{EventSchema, FieldType};

        let r = Registry::new();

        let mut fields = std::collections::BTreeMap::new();
        fields.insert("lat".to_string(), FieldType::F64);
        fields.insert("lon".to_string(), FieldType::F64);
        fields.insert("user_id".to_string(), FieldType::Str);
        let schema = EventSchema {
            fields,
            optional_fields: vec![],
        };

        let mut agg = AggregationDescriptor {
            node_name: "geo_test".to_string(),
            source_node_name: "Txn".to_string(),
            group_keys: vec!["user_id".to_string()],
            features: vec![NamedAggOp {
                feature_name: "max_kmh".to_string(),
                descriptor: AggOpDescriptor {
                    kind: AggKind::GeoVelocity,
                    field: None,
                    window_ms: None,
                    where_expr: None,
                    n: None,
                    half_life_ms: None,
                    sub_window_ms: None,
                    sigma: None,
                    sketch_params: None,
                    ext: AggExtParams {
                        lat_field: Some("lat".to_string()),
                        lon_field: Some("lon".to_string()),
                        ..Default::default()
                    },
                    field_idx: FIELD_IDX_NONE,
                    field_idx_into_event_extracted: Vec::new(),
                },
            }],
            agg_id: 0,
            field_names: vec![],
            cluster_id: 0,
        };

        r.resolve_field_indices_for_agg_mut(&mut agg, &schema, &[])
            .expect("registration must succeed for valid geo schema");

        let feat = &agg.features[0];
        assert_ne!(
            feat.descriptor.ext.lat_idx, FIELD_IDX_NONE,
            "lat_idx must be resolved at register time so the geo update_at \
             fast path engages instead of the slow update() arm"
        );
        assert_ne!(
            feat.descriptor.ext.lon_idx, FIELD_IDX_NONE,
            "lon_idx must be resolved at register time"
        );
        assert_ne!(
            feat.descriptor.ext.lat_idx, feat.descriptor.ext.lon_idx,
            "lat_idx and lon_idx must point at distinct ExtractedFields slots"
        );
        assert!(
            agg.field_names.iter().any(|n| n == "lat"),
            "lat must be in agg.field_names so apply-loop pre-extraction populates extracted[lat_idx]; got {:?}",
            agg.field_names
        );
        assert!(
            agg.field_names.iter().any(|n| n == "lon"),
            "lon must be in agg.field_names so apply-loop pre-extraction populates extracted[lon_idx]; got {:?}",
            agg.field_names
        );
    }

    // `EventDescriptor.apply_field_names` must be populated at
    // register-time as the field-union of all aggs on this source.
    #[test]
    fn event_descriptor_apply_field_names_is_populated_at_registration() {
        use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
        use crate::agg_op::{AggKind, AggOpDescriptor, FIELD_IDX_NONE};
        use crate::registry_diff::PayloadNode;

        let r = Registry::new();

        let event_txn = EventDescriptor {
            name: "Txn".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![], // populated by the resolver
        };

        // Two-feature aggregation on Txn source: Count (no field) + Sum(amount).
        let agg_desc = AggregationDescriptor {
            node_name: "AggTable".to_string(),
            source_node_name: "Txn".to_string(),
            group_keys: vec!["card_id".to_string()],
            features: vec![
                NamedAggOp {
                    feature_name: "txn_cnt".to_string(),
                    descriptor: AggOpDescriptor {
                        kind: AggKind::Count,
                        field: None,
                        window_ms: Some(300_000),
                        where_expr: None,
                        n: None,
                        half_life_ms: None,
                        sub_window_ms: None,
                        sigma: None,
                        sketch_params: None,
                        ext: Default::default(),
                        field_idx: FIELD_IDX_NONE,
                        field_idx_into_event_extracted: Vec::new(),
                    },
                },
                NamedAggOp {
                    feature_name: "amount_sum".to_string(),
                    descriptor: AggOpDescriptor {
                        kind: AggKind::Sum,
                        field: Some("amount".to_string()),
                        window_ms: Some(300_000),
                        where_expr: None,
                        n: None,
                        half_life_ms: None,
                        sub_window_ms: None,
                        sigma: None,
                        sketch_params: None,
                        ext: Default::default(),
                        field_idx: FIELD_IDX_NONE,
                        field_idx_into_event_extracted: Vec::new(),
                    },
                },
            ],
            agg_id: 0,
            field_names: vec![],
            cluster_id: 0,
        };
        let agg_arc = Arc::new(agg_desc);

        let deriv = crate::registry::DerivationDescriptor {
            name: "AggTable".to_string(),
            output_kind: crate::registry::OutputKind::Table,
            upstreams: vec!["Txn".to_string()],
            ops: vec![],
            schema: crate::schema::DerivedSchema {
                fields: {
                    let mut m = BTreeMap::new();
                    m.insert("card_id".to_string(), crate::schema::FieldType::Str);
                    m.insert("txn_cnt".to_string(), crate::schema::FieldType::I64);
                    m.insert("amount_sum".to_string(), crate::schema::FieldType::F64);
                    m
                },
                optional_fields: vec![],
            },
            table_primary_key: Some(vec!["card_id".to_string()]),
            registered_at_version: 0,
        };

        r.apply_registration(
            vec![
                PayloadNode::Event(event_txn),
                PayloadNode::Derivation(deriv),
            ],
            vec![],
            vec![],
            vec![("AggTable".to_string(), agg_arc)],
        );

        let txn_desc = r
            .get_event_descriptor("Txn")
            .expect("Txn must be registered");
        assert!(
            !txn_desc.apply_field_names.is_empty(),
            "EventDescriptor.apply_field_names must be populated at \
             register-time as the union of agg-declared fields"
        );
        assert!(
            txn_desc.apply_field_names.iter().any(|n| n == "amount"),
            "amount field (consumed by amount_sum agg) must be in the union; got {:?}",
            txn_desc.apply_field_names
        );
        let schema_fields = make_event_schema().fields;
        for n in txn_desc.apply_field_names.iter() {
            assert!(
                schema_fields.contains_key(n.as_str()),
                "apply_field_names entry '{}' must resolve to a real schema field",
                n
            );
        }
    }

    // Each agg feature must carry `field_idx_into_event_extracted` —
    // a map from its declared fields to per-event union indices.
    #[test]
    fn agg_field_idx_into_event_extracted_resolved_against_union() {
        use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
        use crate::agg_op::{AggKind, AggOpDescriptor, FIELD_IDX_NONE};
        use crate::registry_diff::PayloadNode;

        let r = Registry::new();

        let event_txn = EventDescriptor {
            name: "Txn".to_string(),
            schema: make_event_schema(),
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        };

        let agg_desc = AggregationDescriptor {
            node_name: "AggTable2".to_string(),
            source_node_name: "Txn".to_string(),
            group_keys: vec!["card_id".to_string()],
            features: vec![
                NamedAggOp {
                    feature_name: "txn_cnt".to_string(),
                    descriptor: AggOpDescriptor {
                        kind: AggKind::Count,
                        field: None,
                        window_ms: Some(300_000),
                        where_expr: None,
                        n: None,
                        half_life_ms: None,
                        sub_window_ms: None,
                        sigma: None,
                        sketch_params: None,
                        ext: Default::default(),
                        field_idx: FIELD_IDX_NONE,
                        field_idx_into_event_extracted: Vec::new(),
                    },
                },
                NamedAggOp {
                    feature_name: "amount_sum".to_string(),
                    descriptor: AggOpDescriptor {
                        kind: AggKind::Sum,
                        field: Some("amount".to_string()),
                        window_ms: Some(300_000),
                        where_expr: None,
                        n: None,
                        half_life_ms: None,
                        sub_window_ms: None,
                        sigma: None,
                        sketch_params: None,
                        ext: Default::default(),
                        field_idx: FIELD_IDX_NONE,
                        field_idx_into_event_extracted: Vec::new(),
                    },
                },
            ],
            agg_id: 0,
            field_names: vec![],
            cluster_id: 0,
        };
        let agg_arc = Arc::new(agg_desc);

        let deriv = crate::registry::DerivationDescriptor {
            name: "AggTable2".to_string(),
            output_kind: crate::registry::OutputKind::Table,
            upstreams: vec!["Txn".to_string()],
            ops: vec![],
            schema: crate::schema::DerivedSchema {
                fields: {
                    let mut m = BTreeMap::new();
                    m.insert("card_id".to_string(), crate::schema::FieldType::Str);
                    m.insert("txn_cnt".to_string(), crate::schema::FieldType::I64);
                    m.insert("amount_sum".to_string(), crate::schema::FieldType::F64);
                    m
                },
                optional_fields: vec![],
            },
            table_primary_key: Some(vec!["card_id".to_string()]),
            registered_at_version: 0,
        };

        r.apply_registration(
            vec![
                PayloadNode::Event(event_txn),
                PayloadNode::Derivation(deriv),
            ],
            vec![],
            vec![],
            vec![("AggTable2".to_string(), agg_arc)],
        );

        let cached = r.compiled_aggregation("AggTable2").expect("agg present");
        let txn_desc = r.get_event_descriptor("Txn").expect("event present");

        // Locate the index of "amount" in the alphabetical union.
        let union_amount_idx = txn_desc
            .apply_field_names
            .iter()
            .position(|n| n == "amount")
            .expect("amount must be in the union") as u8;

        let count_feat = cached
            .features
            .iter()
            .find(|f| f.feature_name == "txn_cnt")
            .expect("txn_cnt feature");
        let sum_feat = cached
            .features
            .iter()
            .find(|f| f.feature_name == "amount_sum")
            .expect("amount_sum feature");

        assert!(
            count_feat
                .descriptor
                .field_idx_into_event_extracted
                .is_empty(),
            "Count agg has no field; field_idx_into_event_extracted must be empty"
        );
        assert_eq!(
            sum_feat.descriptor.field_idx_into_event_extracted.len(),
            1,
            "Sum agg has 1 declared field; mapping len must be 1"
        );
        assert_eq!(
            sum_feat.descriptor.field_idx_into_event_extracted[0], union_amount_idx,
            "Sum's mapping[0] must equal 'amount'-position in the union"
        );
    }
}
