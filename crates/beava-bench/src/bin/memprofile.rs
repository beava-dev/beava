//! Per-AggOp memory profile report for realistic workload configs.

use anyhow::{anyhow, Context, Result};
use beava_core::agg_apply::apply_event_to_aggregations;
use beava_core::agg_descriptor::AggregationDescriptor;
use beava_core::agg_op::{AggKind, AggOpDescriptor};
use beava_core::agg_state_table::{new_state_tables_for, EntityKey, StateTables};
use beava_core::mem_usage::{MemBreakdown, MemProfile, MemUsage};
use beava_core::row::{json_value_to_beava_value, Row, Value};
use beava_core::{register_validate::validate_payload, registry::Registry};
use clap::Parser;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Parser)]
#[command(name = "memprofile", about = "Profile per-AggOp memory usage")]
struct Args {
    #[arg(long, default_value = "fraud")]
    workload: String,
    #[arg(long, default_value_t = 2_000)]
    events: u64,
    #[arg(long, default_value = "memory-profile-fraud-team.md")]
    output: PathBuf,
    #[arg(long, default_value_t = beava_server::http_admin::BYTES_PER_ENTITY_P99_V0_PLACEHOLDER)]
    metrics_bytes_per_entity_p99: u64,
    #[arg(long, default_value_t = 0.15)]
    tolerance: f64,
}

#[derive(Debug, Clone)]
struct ProfileRow {
    source_events: Vec<String>,
    derivation: String,
    entity_key: String,
    entity_events: u64,
    feature: String,
    op_name: String,
    key_path: Vec<String>,
    events_applied: u64,
    shape: ProfileShape,
    window_ms: Option<u64>,
    profile: MemProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProfileShape {
    Lifetime,
    Windowed,
}

impl ProfileShape {
    fn as_str(self) -> &'static str {
        match self {
            Self::Lifetime => "lifetime",
            Self::Windowed => "windowed",
        }
    }
}

struct ReportInput<'a> {
    workload: &'a str,
    events_requested: u64,
    events_generated: u64,
    events_by_source: &'a BTreeMap<String, u64>,
    derivation_count: usize,
    feature_count: usize,
    active_entity_count: usize,
    table_profiles: &'a [TableProfile],
    rows: &'a [ProfileRow],
    bytes_per_entity_p99: usize,
    metrics_placeholder: u64,
    tolerance: f64,
}

#[derive(Debug, Clone)]
struct EntityProfile {
    entity_key: String,
    events_applied: u64,
    profile: MemProfile,
    features: Vec<ProfileRow>,
}

#[derive(Debug, Clone)]
struct FeatureSummary {
    feature: String,
    op_name: String,
    shape: ProfileShape,
    stack_bytes: usize,
    heap_p50: usize,
    heap_p99: usize,
    heap_max: usize,
    total_p50: usize,
    total_p99: usize,
    total_max: usize,
}

#[derive(Debug, Clone)]
struct TableProfile {
    source_events: Vec<String>,
    derivation: String,
    key_path: Vec<String>,
    configured_features: usize,
    active_entities: usize,
    events_applied: u64,
    stack_p50: usize,
    stack_p99: usize,
    stack_max: usize,
    heap_p50: usize,
    heap_p99: usize,
    heap_max: usize,
    total_p50: usize,
    total_p99: usize,
    total_max: usize,
    feature_summaries: Vec<FeatureSummary>,
    entities: Vec<EntityProfile>,
}

#[derive(Debug, Default)]
struct ProfileCounters {
    table_events: BTreeMap<u32, u64>,
    entity_events: BTreeMap<(u32, String), u64>,
    feature_events: BTreeMap<(u32, String, usize), u64>,
    last_seen_ms: BTreeMap<(u32, String), u64>,
}

impl ProfileCounters {
    fn record_descriptor_entity(
        &mut self,
        desc: &AggregationDescriptor,
        entity_key: String,
        now_ms: i64,
        cold_after_ms: Option<u64>,
    ) {
        let entity_id = (desc.agg_id, entity_key.clone());
        if let Some(ttl_ms) = cold_after_ms {
            let now_ms = now_ms as u64;
            if self
                .last_seen_ms
                .get(&entity_id)
                .map(|last_seen| now_ms.saturating_sub(*last_seen) > ttl_ms)
                .unwrap_or(false)
            {
                self.entity_events.remove(&entity_id);
                for feature_index in 0..desc.features.len() {
                    self.feature_events
                        .remove(&(desc.agg_id, entity_key.clone(), feature_index));
                }
            }
            self.last_seen_ms.insert(entity_id.clone(), now_ms);
        }

        *self.table_events.entry(desc.agg_id).or_insert(0) += 1;
        *self.entity_events.entry(entity_id).or_insert(0) += 1;
        for feature_index in 0..desc.features.len() {
            *self
                .feature_events
                .entry((desc.agg_id, entity_key.clone(), feature_index))
                .or_insert(0) += 1;
        }
    }

    fn table_events(&self, agg_id: u32) -> u64 {
        self.table_events.get(&agg_id).copied().unwrap_or(0)
    }

    fn entity_events(&self, agg_id: u32, entity_key: &str) -> u64 {
        self.entity_events
            .get(&(agg_id, entity_key.to_string()))
            .copied()
            .unwrap_or(0)
    }

    fn feature_events(&self, agg_id: u32, entity_key: &str, feature_index: usize) -> u64 {
        self.feature_events
            .get(&(agg_id, entity_key.to_string(), feature_index))
            .copied()
            .unwrap_or(0)
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let report = build_report(&args)?;
    fs::write(&args.output, report).with_context(|| format!("write {}", args.output.display()))?;
    eprintln!("memprofile: wrote {}", args.output.display());
    Ok(())
}

fn build_report(args: &Args) -> Result<String> {
    let workload = beava_bench::workloads::load_by_name(&args.workload)
        .with_context(|| format!("load workload {:?}", args.workload))?;
    let registry = registry_from_register(&workload.register_payload)?;
    let descriptors = aggregation_descriptors(&registry);
    let feature_count = descriptors
        .iter()
        .map(|desc| desc.features.len())
        .sum::<usize>();
    let mut state_tables = new_state_tables_for(&registry);
    let mut counters = ProfileCounters::default();

    let mut events_generated = 0;
    let mut events_by_source = BTreeMap::new();
    for (idx, event) in (workload.event_generator)(args.events).enumerate() {
        events_generated += 1;
        *events_by_source
            .entry(event.event_name.clone())
            .or_insert(0) += 1;
        let now_ms = event_time_ms(&event.fields).unwrap_or(1_000_000 + idx as i64 * 1_000);
        let row = row_from_fields(event.fields);
        let cold_after_ms = registry
            .get_event_descriptor(&event.event_name)
            .and_then(|event_desc| event_desc.cold_after_ms);
        record_profile_counters(
            &registry,
            &mut counters,
            &event.event_name,
            &row,
            now_ms,
            cold_after_ms,
        );
        apply_event_to_aggregations(
            &event.event_name,
            &row,
            now_ms,
            idx as u64,
            &registry,
            &mut state_tables,
            cold_after_ms,
        );
    }

    let (mut table_profiles, mut rows) =
        collect_table_profiles(&registry, &state_tables, &counters);

    rows.sort_by(compare_profile_rows);

    table_profiles.sort_by(compare_table_profiles);
    let active_entity_count = table_profiles.iter().map(|t| t.active_entities).sum();
    let all_entity_totals = table_profiles
        .iter()
        .flat_map(|table| {
            table
                .entities
                .iter()
                .map(|entity| entity.profile.total_bytes())
        })
        .collect::<Vec<_>>();
    let bytes_per_entity_p99 = percentile_usize(all_entity_totals, 0.99);
    Ok(render_markdown(ReportInput {
        workload: &args.workload,
        events_requested: args.events,
        events_generated,
        events_by_source: &events_by_source,
        derivation_count: workload.derivations.len(),
        feature_count,
        active_entity_count,
        table_profiles: &table_profiles,
        rows: &rows,
        bytes_per_entity_p99,
        metrics_placeholder: args.metrics_bytes_per_entity_p99,
        tolerance: args.tolerance,
    }))
}

fn registry_from_register(register: &JsonValue) -> Result<Registry> {
    let payload: beava_server::register::RegisterPayload =
        serde_json::from_value(register.clone()).context("parse workload register payload")?;
    let registry = Registry::new();
    let validated = validate_payload(&registry.snapshot(), payload.nodes).map_err(|errors| {
        let first = errors
            .first()
            .map(|err| format!("{}: {}", err.path, err.reason))
            .unwrap_or_else(|| "unknown validation error".to_string());
        anyhow!("workload register payload failed validation: {first}")
    })?;
    let (nodes, compiled_chains, propagated_schemas, compiled_aggregations) =
        validated.into_parts();
    registry.apply_registration(
        nodes,
        compiled_chains,
        propagated_schemas,
        compiled_aggregations,
    );
    Ok(registry)
}

fn aggregation_descriptors(registry: &Registry) -> Vec<Arc<AggregationDescriptor>> {
    let mut descriptors = registry
        .snapshot()
        .compiled_aggregations
        .into_values()
        .collect::<Vec<_>>();
    descriptors.sort_by(|a, b| {
        a.agg_id
            .cmp(&b.agg_id)
            .then_with(|| a.node_name.cmp(&b.node_name))
    });
    descriptors
}

fn record_profile_counters(
    registry: &Registry,
    counters: &mut ProfileCounters,
    source_name: &str,
    row: &Row,
    now_ms: i64,
    cold_after_ms: Option<u64>,
) {
    for desc in registry.compiled_aggregations_for_source(source_name) {
        if let Some(chain) = registry.compiled_chain(&desc.node_name) {
            if chain.apply(row.clone()).is_none() {
                continue;
            }
        }
        let Some(entity_key) = EntityKey::from_row(&desc.group_keys, row) else {
            continue;
        };
        counters.record_descriptor_entity(
            &desc,
            format_entity_key(&entity_key),
            now_ms,
            cold_after_ms,
        );
    }
}

fn render_markdown(input: ReportInput<'_>) -> String {
    let mut out = String::new();
    out.push_str("# AggOp Memory Profile: fraud-team\n\n");
    out.push_str("## Workload Summary\n\n");
    out.push_str(&format!("- Workload: `{}`\n", input.workload));
    out.push_str(&format!(
        "- Events requested from generator: `{}`\n",
        input.events_requested
    ));
    out.push_str(&format!(
        "- Events replayed from generator: `{}`\n",
        input.events_generated
    ));
    out.push_str("- Events by source:\n");
    for (source, count) in input.events_by_source {
        out.push_str(&format!("  - `{source}`: `{count}`\n"));
    }
    out.push_str(&format!(
        "- Derivations discovered: `{}`\n",
        input.derivation_count
    ));
    out.push_str(&format!(
        "- Aggregate features discovered: `{}`\n",
        input.feature_count
    ));
    out.push_str(&format!(
        "- Active entity rows profiled: `{}`\n",
        input.active_entity_count
    ));
    out.push_str(&format!(
        "- Bytes per active entity row p99: `{}` bytes\n\n",
        input.bytes_per_entity_p99
    ));

    out.push_str("## Per-Entity Table Footprint\n\n");
    out.push_str("| Rank | Table | Source | group_by key | Active entities | Features/entity | Events applied | Stack p50 | Stack p99 | Stack max | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max | Top contributor |\n");
    out.push_str("|------|-------|--------|--------------|-----------------|-----------------|----------------|-----------|-----------|-----------|----------|----------|----------|-----------|-----------|-----------|-----------------|\n");
    for (idx, table) in input.table_profiles.iter().enumerate() {
        out.push_str(&format!(
            "| {} | `{}` | `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | `{}` |\n",
            idx + 1,
            table.derivation,
            format_sources(&table.source_events),
            format_key_path(&table.key_path),
            table.active_entities,
            table.configured_features,
            table.events_applied,
            table.stack_p50,
            table.stack_p99,
            table.stack_max,
            table.heap_p50,
            table.heap_p99,
            table.heap_max,
            table.total_p50,
            table.total_p99,
            table.total_max,
            table
                .feature_summaries
                .first()
                .map(|feature| feature.feature.as_str())
                .unwrap_or("-")
        ));
    }

    out.push_str("\n## AggOp Payload Bytes Plot\n\n");
    out.push_str(
        "Unique AggOp kinds grouped by max observed inline `payload_bytes` in 8-byte bands (`0-8`, `9-16`, ...). Bars use `#`; the detail table marks bands containing payloads at or above 48 bytes as boxing candidates.\n\n",
    );
    out.push_str("```text\n");
    out.push_str("Payload band | Op count | Plot\n");
    out.push_str("-------------|----------|----------------------------------\n");
    for line in render_payload_plot_lines(input.rows) {
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("```\n\n");
    out.push_str("| Payload band | Boxing candidate | Op count | AggOps |\n");
    out.push_str("|--------------|------------------|----------|--------|\n");
    for row in render_payload_detail_rows(input.rows) {
        out.push_str(&row);
        out.push('\n');
    }
    out.push('\n');

    out.push_str("## Per-Table Entity Details\n\n");
    for table in input.table_profiles {
        out.push_str(&format!(
            "### `{}` (`{}` by `{}`)\n\n",
            table.derivation,
            format_sources(&table.source_events),
            format_key_path(&table.key_path)
        ));
        if table.active_entities == 0 {
            out.push_str(&format!(
                "No active entity rows. Configured features: `{}`. The workload generator emitted no events for this table's source.\n\n",
                table.configured_features
            ));
            continue;
        }

        out.push_str("#### Feature Columns Across Entities\n\n");
        out.push_str("| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |\n");
        out.push_str("|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|\n");
        for feature in &table.feature_summaries {
            out.push_str(&format!(
                "| `{}` | `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
                feature.feature,
                feature.op_name,
                feature.shape.as_str(),
                feature.stack_bytes,
                feature.heap_p50,
                feature.heap_p99,
                feature.heap_max,
                feature.total_p50,
                feature.total_p99,
                feature.total_max
            ));
        }

        out.push_str("\n#### Largest Entity Rows\n\n");
        out.push_str("| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |\n");
        out.push_str("|------------|--------|-------------|------------|-------------|--------------------------|\n");
        for entity in table.entities.iter().take(5) {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {} |\n",
                entity.entity_key,
                entity.events_applied,
                entity.profile.stack_bytes,
                entity.profile.heap_bytes,
                entity.profile.total_bytes(),
                format_top_features(&entity.features, 3)
            ));
        }

        if let Some(entity) = table.entities.first() {
            out.push_str(&format!(
                "\n#### Feature Breakdown For Largest Entity `{}`\n\n",
                entity.entity_key
            ));
            out.push_str("| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |\n");
            out.push_str("|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|\n");
            for feature in &entity.features {
                out.push_str(&format!(
                    "| `{}` | `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
                    feature.feature,
                    feature.op_name,
                    feature.shape.as_str(),
                    feature.events_applied,
                    feature.profile.stack_bytes,
                    feature.profile.enum_slot_bytes,
                    feature.profile.payload_bytes,
                    feature.profile.slack_bytes,
                    feature.profile.heap_bytes,
                    feature.profile.total_bytes()
                ));
            }
        }
        out.push('\n');
    }

    out.push_str("## Top 5 Offenders\n\n");
    out.push_str("One heaviest entity-feature example per unique op.\n\n");
    for (idx, row) in top_unique_op_rows(input.rows, 5).iter().enumerate() {
        out.push_str(&format!(
            "### {}. `{}` / `{}` / `{}` / `{}`\n\n",
            idx + 1,
            format_sources(&row.source_events),
            row.derivation,
            row.feature,
            row.op_name
        ));
        out.push_str(&format!(
            "- Path: `{}` -> `{}` -> `{}` -> `{}` -> `{}`\n",
            format_sources(&row.source_events),
            row.derivation,
            row.feature,
            row.op_name,
            row.shape.as_str()
        ));
        out.push_str(&format!("- Entity key: `{}`\n", row.entity_key));
        out.push_str(&format!("- Entity events: `{}`\n", row.entity_events));
        out.push_str(&format!(
            "- Key path: `{}`\n",
            format_key_path(&row.key_path)
        ));
        out.push_str(&format!("- Events applied: `{}`\n", row.events_applied));
        out.push_str(&format!(
            "- Bytes: stack={} (enum_slot_bytes={} payload_bytes={} slack_bytes={}) heap={} total={}\n",
            row.profile.stack_bytes,
            row.profile.enum_slot_bytes,
            row.profile.payload_bytes,
            row.profile.slack_bytes,
            row.profile.heap_bytes,
            row.profile.total_bytes()
        ));
        out.push_str(&format!(
            "- Shape: `{}`{}\n",
            row.shape.as_str(),
            format_window_suffix(row.window_ms)
        ));
        if row.shape == ProfileShape::Windowed {
            out.push_str("- Breakdown rollup:\n");
            for entry in windowed_rollup(&row.profile.breakdown) {
                out.push_str(&format!(
                    "  - `{}`: {} bytes ({}, {})\n",
                    entry.label, entry.bytes, entry.kind, entry.note
                ));
            }
            out.push_str("- Raw breakdown:\n");
        } else {
            out.push_str("- Breakdown:\n");
        }
        for entry in top_breakdown(&row.profile.breakdown, 8) {
            out.push_str(&format!(
                "  - `{}`: {} bytes ({}, {})\n",
                entry.label, entry.bytes, entry.kind, entry.note
            ));
        }
        out.push('\n');
    }

    out.push_str("## Metrics Coherence\n\n");
    let target = input.metrics_placeholder as f64;
    let observed = input.bytes_per_entity_p99 as f64;
    let delta = (observed - target).abs();
    let allowed = target * input.tolerance;
    out.push_str(&format!(
        "- `/metrics` `beava_bytes_per_entity_p99`: `{}` bytes\n",
        input.metrics_placeholder
    ));
    out.push_str(&format!(
        "- Profile bytes-per-active-entity-row p99: `{}` bytes\n",
        input.bytes_per_entity_p99
    ));
    out.push_str(&format!("- Tolerance: `{:.1}%`\n", input.tolerance * 100.0));
    if delta <= allowed {
        out.push_str(
            "- Assertion: PASS - profile estimate is coherent with metrics placeholder.\n",
        );
    } else {
        out.push_str(&format!(
            "- Assertion: bytes_per_entity_p99 diverged by {:.0} bytes; file sibling work to replace the static placeholder with live sampling.\n",
            delta
        ));
    }

    out.push_str("\n## Notes\n\n");
    out.push_str("- `stack_bytes` is the inline `AggOp` enum slot for each feature.\n");
    out.push_str("- `enum_slot_bytes` is the fixed-size `AggOp` enum slot charged to a row; parent rows sum this across child paths.\n");
    out.push_str("- `payload_bytes` is the active variant payload inside the enum slot. For boxed variants this is the inline `Box<T>` pointer, while the boxed pointee remains in `heap_bytes`.\n");
    out.push_str("- `slack_bytes` is unused capacity in the fixed-size `AggOp` enum slot: `enum_slot_bytes - payload_bytes`.\n");
    out.push_str("- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.\n");
    out.push_str("- `AggOp` state is replayed through production `Registry` + `StateTables`; event counts come from a memprofile-only sidecar counter.\n");
    out.push_str("- Primary grain is `derivation table -> entity row -> feature column`; top offenders list one concrete entity-feature row per unique op.\n");
    out
}

fn collect_table_profiles(
    registry: &Registry,
    state_tables: &StateTables,
    counters: &ProfileCounters,
) -> (Vec<TableProfile>, Vec<ProfileRow>) {
    let mut all_rows = Vec::new();
    let mut table_profiles = Vec::new();
    for desc in aggregation_descriptors(registry) {
        let source_events = vec![desc.source_node_name.clone()];
        let derivation = desc.node_name.clone();
        let key_path = desc.group_keys.clone();
        let configured_features = desc.features.len();
        let mut entities = Vec::new();
        if let Some(table) = state_tables.get(desc.agg_id as usize) {
            for (entity_key, ops) in table.iter_sorted() {
                let entity_key = format_entity_key(&entity_key);
                let entity_events = counters.entity_events(desc.agg_id, &entity_key);
                let mut entity_profile = MemProfile::new(entity_key.clone(), 0);
                let mut feature_rows = Vec::with_capacity(desc.features.len());
                for (feature_index, feature) in desc.features.iter().enumerate() {
                    let Some(op) = ops.get(feature_index) else {
                        continue;
                    };
                    let mut profile = op.mem_profile();
                    let op_name = op_name_from_kind(feature.descriptor.kind).to_string();
                    let shape = profile_shape(&feature.descriptor);
                    let feature_name = feature.feature_name.clone();
                    let events_applied =
                        counters.feature_events(desc.agg_id, &entity_key, feature_index);
                    profile.label = format!(
                        "{}::{}[{}]::{} ({})",
                        format_sources(&source_events),
                        derivation,
                        entity_key,
                        feature_name,
                        op_name
                    );
                    add_profile_totals(&mut entity_profile, &profile);
                    let row = ProfileRow {
                        source_events: source_events.clone(),
                        derivation: derivation.clone(),
                        entity_key: entity_key.clone(),
                        entity_events,
                        feature: feature_name,
                        op_name,
                        key_path: key_path.clone(),
                        events_applied,
                        window_ms: feature.descriptor.window_ms,
                        shape,
                        profile,
                    };
                    feature_rows.push(row.clone());
                    all_rows.push(row);
                }
                feature_rows.sort_by(compare_profile_rows);
                entities.push(EntityProfile {
                    entity_key,
                    events_applied: entity_events,
                    profile: entity_profile,
                    features: feature_rows,
                });
            }
        }
        entities.sort_by(compare_entity_profiles);
        let feature_summaries = feature_summaries_for_table(&entities);
        let stack_values = entities
            .iter()
            .map(|entity| entity.profile.stack_bytes)
            .collect::<Vec<_>>();
        let heap_values = entities
            .iter()
            .map(|entity| entity.profile.heap_bytes)
            .collect::<Vec<_>>();
        let total_values = entities
            .iter()
            .map(|entity| entity.profile.total_bytes())
            .collect::<Vec<_>>();
        table_profiles.push(TableProfile {
            source_events,
            derivation,
            key_path,
            configured_features,
            active_entities: entities.len(),
            events_applied: counters.table_events(desc.agg_id),
            stack_p50: percentile_usize(stack_values.clone(), 0.50),
            stack_p99: percentile_usize(stack_values.clone(), 0.99),
            stack_max: stack_values.into_iter().max().unwrap_or(0),
            heap_p50: percentile_usize(heap_values.clone(), 0.50),
            heap_p99: percentile_usize(heap_values.clone(), 0.99),
            heap_max: heap_values.into_iter().max().unwrap_or(0),
            total_p50: percentile_usize(total_values.clone(), 0.50),
            total_p99: percentile_usize(total_values.clone(), 0.99),
            total_max: total_values.into_iter().max().unwrap_or(0),
            feature_summaries,
            entities,
        });
    }
    (table_profiles, all_rows)
}

fn feature_summaries_for_table(entities: &[EntityProfile]) -> Vec<FeatureSummary> {
    let mut grouped: BTreeMap<(String, String, ProfileShape), Vec<&ProfileRow>> = BTreeMap::new();
    for entity in entities {
        for feature in &entity.features {
            grouped
                .entry((
                    feature.feature.clone(),
                    feature.op_name.clone(),
                    feature.shape,
                ))
                .or_default()
                .push(feature);
        }
    }
    let mut summaries = grouped
        .into_iter()
        .map(|((feature, op_name, shape), rows)| {
            let stack_values = rows
                .iter()
                .map(|row| row.profile.stack_bytes)
                .collect::<Vec<_>>();
            let heap_values = rows
                .iter()
                .map(|row| row.profile.heap_bytes)
                .collect::<Vec<_>>();
            let total_values = rows
                .iter()
                .map(|row| row.profile.total_bytes())
                .collect::<Vec<_>>();
            FeatureSummary {
                feature,
                op_name,
                shape,
                stack_bytes: percentile_usize(stack_values, 0.99),
                heap_p50: percentile_usize(heap_values.clone(), 0.50),
                heap_p99: percentile_usize(heap_values.clone(), 0.99),
                heap_max: heap_values.into_iter().max().unwrap_or(0),
                total_p50: percentile_usize(total_values.clone(), 0.50),
                total_p99: percentile_usize(total_values.clone(), 0.99),
                total_max: total_values.into_iter().max().unwrap_or(0),
            }
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|a, b| {
        b.total_p99
            .cmp(&a.total_p99)
            .then_with(|| b.heap_p99.cmp(&a.heap_p99))
            .then_with(|| a.feature.cmp(&b.feature))
    });
    summaries
}

fn add_profile_totals(total: &mut MemProfile, profile: &MemProfile) {
    total.stack_bytes += profile.stack_bytes;
    total.enum_slot_bytes += profile.enum_slot_bytes;
    total.payload_bytes += profile.payload_bytes;
    total.slack_bytes += profile.slack_bytes;
    total.heap_bytes += profile.heap_bytes;
    total.breakdown.extend(profile.breakdown.clone());
}

fn percentile_usize(mut values: Vec<usize>, q: f64) -> usize {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let rank = ((values.len() as f64) * q).ceil() as usize;
    let idx = rank.saturating_sub(1).min(values.len() - 1);
    values[idx]
}

fn compare_table_profiles(a: &TableProfile, b: &TableProfile) -> std::cmp::Ordering {
    b.total_p99
        .cmp(&a.total_p99)
        .then_with(|| b.heap_p99.cmp(&a.heap_p99))
        .then_with(|| b.active_entities.cmp(&a.active_entities))
        .then_with(|| a.derivation.cmp(&b.derivation))
}

fn compare_entity_profiles(a: &EntityProfile, b: &EntityProfile) -> std::cmp::Ordering {
    b.profile
        .total_bytes()
        .cmp(&a.profile.total_bytes())
        .then_with(|| b.events_applied.cmp(&a.events_applied))
        .then_with(|| a.entity_key.cmp(&b.entity_key))
}

fn value_key_part(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Str(s) => s.to_string(),
        Value::I64(v) => v.to_string(),
        Value::F64(v) => format!("{v}"),
        Value::Bool(v) => v.to_string(),
        Value::Bytes(bytes) => format!("<{} bytes>", bytes.len()),
        Value::Datetime(v) => v.to_string(),
        Value::Json(v) => v.to_string(),
        Value::List(values) => format!("<list:{}>", values.len()),
        Value::Map(values) => format!("<map:{}>", values.len()),
    }
}

fn format_entity_key(key: &EntityKey) -> String {
    if key.0.is_empty() {
        return "<global>".to_string();
    }
    key.0
        .iter()
        .map(|(_, value)| value_key_part(value))
        .collect::<Vec<_>>()
        .join("|")
}

fn format_sources(sources: &[String]) -> String {
    if sources.is_empty() {
        "*".to_string()
    } else {
        sources.join("+")
    }
}

fn format_key_path(keys: &[String]) -> String {
    if keys.is_empty() {
        "-".to_string()
    } else {
        keys.join("+")
    }
}

fn format_top_features(features: &[ProfileRow], limit: usize) -> String {
    features
        .iter()
        .take(limit)
        .map(|feature| {
            format!(
                "`{}`={} bytes",
                feature.feature,
                feature.profile.total_bytes()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_payload_plot_lines(rows: &[ProfileRow]) -> Vec<String> {
    const BAR_WIDTH: usize = 16;

    let bands = payload_band_summaries(rows);
    let max_count = bands
        .iter()
        .map(|band| band.ops.len())
        .max()
        .unwrap_or(1)
        .max(1);
    let mut lines = Vec::new();
    for band in bands {
        let op_count = band.ops.len();
        let bar_len = if op_count == 0 {
            0
        } else {
            ((op_count * BAR_WIDTH).div_ceil(max_count)).max(1)
        };
        let bar = "#".repeat(bar_len);
        let plot = if bar.is_empty() {
            String::new()
        } else {
            format!(" {bar}")
        };
        lines.push(format!(
            "   {lower:>3}-{upper:<3} B | {count:>8} |{plot}",
            lower = band.lower,
            upper = band.upper,
            count = op_count
        ));
    }
    if lines.is_empty() {
        lines.push("      -      |        0 |".into());
    }
    lines
}

fn render_payload_detail_rows(rows: &[ProfileRow]) -> Vec<String> {
    const BOX_CANDIDATE_BYTES: usize = 48;

    let mut out = Vec::new();
    for band in payload_band_summaries(rows) {
        let candidate = if band
            .ops
            .iter()
            .any(|(_, payload_bytes)| *payload_bytes >= BOX_CANDIDATE_BYTES)
        {
            "yes"
        } else {
            "no"
        };
        let ops = if band.ops.is_empty() {
            "-".to_string()
        } else {
            band.ops
                .iter()
                .map(|(op, bytes)| format!("`{op}({bytes}B)`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push(format!(
            "| {}-{} B | {} | {} | {} |",
            band.lower,
            band.upper,
            candidate,
            band.ops.len(),
            ops
        ));
    }
    if out.is_empty() {
        out.push("| - | no | 0 | <no active ops> |".to_string());
    }
    out
}

#[derive(Debug)]
struct PayloadBandSummary {
    lower: usize,
    upper: usize,
    ops: Vec<(String, usize)>,
}

fn payload_band_summaries(rows: &[ProfileRow]) -> Vec<PayloadBandSummary> {
    let mut max_payload_by_op: BTreeMap<String, usize> = BTreeMap::new();
    for row in rows {
        max_payload_by_op
            .entry(row.op_name.clone())
            .and_modify(|bytes| *bytes = (*bytes).max(row.profile.payload_bytes))
            .or_insert(row.profile.payload_bytes);
    }

    let mut grouped: BTreeMap<(usize, usize), Vec<(String, usize)>> = BTreeMap::new();
    for (op_name, payload_bytes) in max_payload_by_op {
        let (lower, upper) = payload_band(payload_bytes);
        grouped
            .entry((lower, upper))
            .or_default()
            .push((op_name, payload_bytes));
    }

    let Some((first_band, _)) = grouped.first_key_value() else {
        return Vec::new();
    };
    let Some((last_band, _)) = grouped.last_key_value() else {
        return Vec::new();
    };

    let mut summaries = Vec::new();
    let mut upper = first_band.1;
    let last_upper = last_band.1;
    while upper <= last_upper {
        let lower = if upper <= 8 { 0 } else { upper - 7 };
        let mut ops = grouped.remove(&(lower, upper)).unwrap_or_default();
        summaries.push({
            ops.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            PayloadBandSummary { lower, upper, ops }
        });
        upper += 8;
    }
    summaries
}

fn payload_band(payload_bytes: usize) -> (usize, usize) {
    if payload_bytes <= 8 {
        (0, 8)
    } else {
        let upper = payload_bytes.div_ceil(8) * 8;
        (upper - 7, upper)
    }
}

fn top_breakdown(entries: &[MemBreakdown], limit: usize) -> Vec<MemBreakdown> {
    let mut entries = entries.to_vec();
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.label.cmp(&b.label)));
    entries.truncate(limit);
    entries
}

fn compare_profile_rows(a: &ProfileRow, b: &ProfileRow) -> std::cmp::Ordering {
    b.profile
        .total_bytes()
        .cmp(&a.profile.total_bytes())
        .then_with(|| b.events_applied.cmp(&a.events_applied))
        .then_with(|| format_sources(&a.source_events).cmp(&format_sources(&b.source_events)))
        .then_with(|| a.derivation.cmp(&b.derivation))
        .then_with(|| a.entity_key.cmp(&b.entity_key))
        .then_with(|| a.feature.cmp(&b.feature))
        .then_with(|| a.op_name.cmp(&b.op_name))
}

fn top_unique_op_rows(rows: &[ProfileRow], limit: usize) -> Vec<&ProfileRow> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for row in rows {
        if seen.insert(row.op_name.as_str()) {
            out.push(row);
            if out.len() == limit {
                break;
            }
        }
    }
    out
}

fn windowed_rollup(entries: &[MemBreakdown]) -> Vec<MemBreakdown> {
    let mut grouped: BTreeMap<String, MemBreakdown> = BTreeMap::new();
    for entry in entries {
        let Some((label, kind, note)) = windowed_rollup_bucket(entry) else {
            continue;
        };
        let slot = grouped.entry(label.clone()).or_insert(MemBreakdown {
            label,
            bytes: 0,
            kind,
            note,
        });
        slot.bytes = slot.bytes.saturating_add(entry.bytes);
    }
    let mut rolled = grouped.into_values().collect::<Vec<_>>();
    rolled.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.label.cmp(&b.label)));
    rolled
}

fn windowed_rollup_bucket(entry: &MemBreakdown) -> Option<(String, String, String)> {
    if entry.label == "Box<WindowedOp>" || entry.label == "WindowedOp spilled bucket SmallVec" {
        return Some((
            "Windowed wrapper overhead".to_string(),
            "WindowedOp".to_string(),
            "summed boxed WindowedOp payload and spilled bucket storage".to_string(),
        ));
    }
    if entry.label.starts_with("Windowed bucket ") && entry.label.ends_with(" Box<AggOp>") {
        return Some((
            "Windowed bucket shell overhead".to_string(),
            "Box".to_string(),
            "summed boxed AggOp enum slots across active buckets".to_string(),
        ));
    }
    let (_, nested) = entry.label.split_once(" / ")?;
    Some((
        format!("{nested} across buckets"),
        entry.kind.clone(),
        "summed across active window buckets".to_string(),
    ))
}

fn profile_shape(desc: &AggOpDescriptor) -> ProfileShape {
    if desc.window_ms.is_some() {
        ProfileShape::Windowed
    } else {
        ProfileShape::Lifetime
    }
}

fn format_window_suffix(window_ms: Option<u64>) -> String {
    window_ms
        .map(|ms| format!(" ({})", format_duration_ms(ms)))
        .unwrap_or_default()
}

fn format_duration_ms(ms: u64) -> String {
    match ms {
        ms if ms % 86_400_000 == 0 => format!("{}d", ms / 86_400_000),
        ms if ms % 3_600_000 == 0 => format!("{}h", ms / 3_600_000),
        ms if ms % 60_000 == 0 => format!("{}m", ms / 60_000),
        ms if ms % 1_000 == 0 => format!("{}s", ms / 1_000),
        ms => format!("{ms}ms"),
    }
}

fn op_name_from_kind(kind: AggKind) -> &'static str {
    match kind {
        AggKind::Count => "count",
        AggKind::Sum => "sum",
        AggKind::Avg => "mean",
        AggKind::Min => "min",
        AggKind::Max => "max",
        AggKind::Variance => "var",
        AggKind::StdDev => "std",
        AggKind::Ratio => "ratio",
        AggKind::First => "first",
        AggKind::Last => "last",
        AggKind::FirstN => "first_n",
        AggKind::LastN => "last_n",
        AggKind::Lag => "lag",
        AggKind::FirstSeen => "first_seen",
        AggKind::LastSeen => "last_seen",
        AggKind::Age => "age",
        AggKind::HasSeen => "has_seen",
        AggKind::TimeSince => "time_since",
        AggKind::TimeSinceLastN => "time_since_last_n",
        AggKind::Streak => "streak",
        AggKind::MaxStreak => "max_streak",
        AggKind::NegativeStreak => "negative_streak",
        AggKind::FirstSeenInWindow => "first_seen_in_window",
        AggKind::Ewma => "ewma",
        AggKind::EwVar => "ewvar",
        AggKind::EwZScore => "ew_zscore",
        AggKind::DecayedSum => "decayed_sum",
        AggKind::DecayedCount => "decayed_count",
        AggKind::Twa => "twa",
        AggKind::RateOfChange => "rate_of_change",
        AggKind::InterArrivalStats => "inter_arrival_stats",
        AggKind::BurstCount => "burst_count",
        AggKind::DeltaFromPrev => "delta_from_prev",
        AggKind::Trend => "trend",
        AggKind::TrendResidual => "trend_residual",
        AggKind::OutlierCount => "outlier_count",
        AggKind::ValueChangeCount => "value_change_count",
        AggKind::ZScore => "z_score",
        AggKind::CountDistinct => "n_unique",
        AggKind::Percentile => "quantile",
        AggKind::TopK => "top_k",
        AggKind::BloomMember => "bloom_member",
        AggKind::Entropy => "entropy",
        AggKind::Histogram => "histogram",
        AggKind::HourOfDayHistogram => "hour_of_day_histogram",
        AggKind::DowHourHistogram => "dow_hour_histogram",
        AggKind::SeasonalDeviation => "seasonal_deviation",
        AggKind::EventTypeMix => "event_type_mix",
        AggKind::MostRecentN => "most_recent_n",
        AggKind::ReservoirSample => "reservoir_sample",
        AggKind::GeoVelocity => "geo_velocity",
        AggKind::GeoDistance => "geo_distance",
        AggKind::GeoSpread => "geo_spread",
        AggKind::DistanceFromHome => "distance_from_home",
    }
}

fn row_from_fields(fields: serde_json::Map<String, JsonValue>) -> Row {
    fields.into_iter().fold(Row::new(), |row, (field, value)| {
        row.with_field(&field, json_value_to_beava_value(value))
    })
}

fn event_time_ms(fields: &serde_json::Map<String, JsonValue>) -> Option<i64> {
    fields.get("event_time").and_then(JsonValue::as_i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memprofile_report_contains_required_sections() {
        let args = Args {
            workload: "fraud".into(),
            events: 5,
            output: PathBuf::from("/tmp/unused.md"),
            metrics_bytes_per_entity_p99: 7_000,
            tolerance: 0.15,
        };
        let report = build_report(&args).unwrap();
        assert!(report.contains("# AggOp Memory Profile: fraud-team"));
        assert!(report.contains("Events requested from generator: `5`"));
        assert!(report.contains("Events replayed from generator: `5`"));
        assert!(report.contains("  - `Txn`: `5`"));
        assert!(!report.contains("Events replayed per op"));
        assert!(report.contains("Active entity rows profiled:"));
        assert!(report.contains("Bytes per active entity row p99:"));
        assert!(report.contains("## Per-Entity Table Footprint"));
        assert!(report.contains(
            "| Rank | Table | Source | group_by key | Active entities | Features/entity | Events applied | Stack p50 | Stack p99 | Stack max | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max | Top contributor |"
        ));
        assert!(report.contains("## AggOp Payload Bytes Plot"));
        assert!(report.contains("Payload band | Op count | Plot"));
        assert!(report.contains("| Payload band | Boxing candidate | Op count | AggOps |"));
        assert!(report.contains("| 41-48 B | yes |"));
        assert!(report.contains("`TxnByUser` | `Txn` | `user_id`"));
        assert!(report.contains("## Per-Table Entity Details"));
        assert!(report.contains("### `TxnByUser` (`Txn` by `user_id`)"));
        assert!(report.contains("#### Feature Columns Across Entities"));
        assert!(report.contains("#### Largest Entity Rows"));
        assert!(report.contains("#### Feature Breakdown For Largest Entity"));
        assert!(
            report.contains("The workload generator emitted no events for this table's source.")
        );
        assert!(!report.contains("## Sorted Op Table"));
        assert!(!report.contains("## Sorted Op Entity-Feature Details"));
        assert!(report.contains("## Top 5 Offenders"));
        assert!(report.contains("One heaviest entity-feature example per unique op."));
        assert!(!report.contains("- Recommendation:"));
        assert!(report.contains("## Metrics Coherence"));
        assert!(report.contains("Aggregate features discovered: `111`"));
        assert!(report.contains("`txn_count_lifetime` | `count` | `lifetime` | 1 |"));
        assert!(report.contains("- Entity key:"));
        assert!(report.contains("- Entity events:"));
        assert!(report.contains("- Events applied: `1`"));
    }
}
