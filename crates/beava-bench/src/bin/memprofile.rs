//! Per-AggOp memory profile report for realistic workload configs.

use anyhow::{anyhow, Context, Result};
use beava_core::agg_op::{AggExtParams, AggKind, AggOp, AggOpDescriptor, SketchParams};
use beava_core::mem_usage::{MemBreakdown, MemProfile, MemUsage};
use beava_core::row::{json_value_to_beava_value, Row};
use clap::Parser;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

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
struct FeatureSpec {
    source_events: Vec<String>,
    derivation: String,
    feature: String,
    op_name: String,
    key_path: Vec<String>,
    desc: AggOpDescriptor,
}

#[derive(Debug, Clone)]
struct ProfileRow {
    source_events: Vec<String>,
    derivation: String,
    feature: String,
    op_name: String,
    key_path: Vec<String>,
    events_applied: u64,
    shape: ProfileShape,
    window_ms: Option<u64>,
    profile: MemProfile,
    recommendation: String,
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

#[derive(Debug, Clone)]
struct OpTotal {
    op_name: String,
    shape: ProfileShape,
    profile: MemProfile,
    rows: Vec<ProfileRow>,
}

struct ReportInput<'a> {
    workload: &'a str,
    events_requested: u64,
    events_generated: u64,
    events_by_source: &'a BTreeMap<String, u64>,
    derivation_count: usize,
    feature_count: usize,
    rows: &'a [ProfileRow],
    op_totals: &'a [OpTotal],
    per_entity_total: usize,
    metrics_placeholder: u64,
    tolerance: f64,
}

struct ProfileSlot {
    spec: FeatureSpec,
    op: AggOp,
    events_applied: u64,
}

impl ProfileSlot {
    fn matches_source(&self, event_name: &str) -> bool {
        self.spec.source_events.is_empty()
            || self
                .spec
                .source_events
                .iter()
                .any(|source| source == event_name)
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
    let features = feature_specs_from_register(&workload.register_payload)?;
    let feature_count = features.len();

    let mut slots = features
        .into_iter()
        .map(|spec| ProfileSlot {
            op: AggOp::new(&spec.desc),
            spec,
            events_applied: 0,
        })
        .collect::<Vec<_>>();

    let mut events_generated = 0;
    let mut events_by_source = BTreeMap::new();
    for (idx, event) in (workload.event_generator)(args.events).enumerate() {
        events_generated += 1;
        *events_by_source
            .entry(event.event_name.clone())
            .or_insert(0) += 1;
        let now_ms = event_time_ms(&event.fields).unwrap_or(1_000_000 + idx as i64 * 1_000);
        let row = row_from_fields(event.fields);
        for slot in slots
            .iter_mut()
            .filter(|slot| slot.matches_source(&event.event_name))
        {
            let field = slot.spec.desc.field.as_deref();
            slot.op.update(&row, now_ms, field, true);
            slot.events_applied += 1;
        }
    }

    let mut rows = Vec::with_capacity(slots.len());
    for slot in slots {
        let spec = slot.spec;
        let mut profile = slot.op.mem_profile();
        profile.label = format!(
            "{}::{}::{} ({})",
            format_sources(&spec.source_events),
            spec.derivation,
            spec.feature,
            spec.op_name
        );
        let shape = profile_shape(&spec.desc);
        let recommendation = recommendation_for(&spec.op_name, shape, &profile);
        rows.push(ProfileRow {
            source_events: spec.source_events,
            derivation: spec.derivation,
            feature: spec.feature,
            op_name: spec.op_name,
            key_path: spec.key_path,
            events_applied: slot.events_applied,
            shape,
            window_ms: spec.desc.window_ms,
            recommendation,
            profile,
        });
    }

    rows.sort_by(|a, b| {
        b.profile
            .total_bytes()
            .cmp(&a.profile.total_bytes())
            .then_with(|| a.profile.label.cmp(&b.profile.label))
    });

    let mut grouped: BTreeMap<(String, ProfileShape), Vec<ProfileRow>> = BTreeMap::new();
    for row in &rows {
        grouped
            .entry((row.op_name.clone(), row.shape))
            .or_default()
            .push(row.clone());
    }
    let mut op_totals: Vec<OpTotal> = grouped
        .into_iter()
        .map(|((op_name, shape), mut rows)| {
            let mut total = MemProfile::new(op_name.clone(), 0);
            for row in &rows {
                total.stack_bytes += row.profile.stack_bytes;
                total.enum_slot_bytes += row.profile.enum_slot_bytes;
                total.payload_bytes += row.profile.payload_bytes;
                total.slack_bytes += row.profile.slack_bytes;
                total.heap_bytes += row.profile.heap_bytes;
                total.breakdown.extend(row.profile.breakdown.clone());
            }
            rows.sort_by(compare_profile_rows);
            OpTotal {
                op_name,
                shape,
                profile: total,
                rows,
            }
        })
        .collect();
    op_totals.sort_by(|a, b| {
        b.profile
            .total_bytes()
            .cmp(&a.profile.total_bytes())
            .then_with(|| a.op_name.cmp(&b.op_name))
            .then_with(|| a.shape.cmp(&b.shape))
    });

    let per_entity_total: usize = rows.iter().map(|r| r.profile.total_bytes()).sum();
    Ok(render_markdown(ReportInput {
        workload: &args.workload,
        events_requested: args.events,
        events_generated,
        events_by_source: &events_by_source,
        derivation_count: workload.derivations.len(),
        feature_count,
        rows: &rows,
        op_totals: &op_totals,
        per_entity_total,
        metrics_placeholder: args.metrics_bytes_per_entity_p99,
        tolerance: args.tolerance,
    }))
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
        "- Per-entity structural estimate: `{}` bytes\n\n",
        input.per_entity_total
    ));

    out.push_str("## Sorted Op Table\n\n");
    out.push_str("| Rank | Op | Shape | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |\n");
    out.push_str("|------|----|-------|-------------|-----------------|---------------|-------------|------------|-------------|\n");
    for (idx, profile) in input.op_totals.iter().enumerate() {
        out.push_str(&format!(
            "| {} | `{}` | `{}` | {} | {} | {} | {} | {} | {} |\n",
            idx + 1,
            profile.op_name,
            profile.shape.as_str(),
            profile.profile.stack_bytes,
            profile.profile.enum_slot_bytes,
            profile.profile.payload_bytes,
            profile.profile.slack_bytes,
            profile.profile.heap_bytes,
            profile.profile.total_bytes()
        ));
    }

    out.push_str("\n## Sorted Op Path Details\n\n");
    out.push_str("Rows with `0` events applied show constructor footprint only; the workload generator did not emit an event for that path's upstream source.\n\n");
    for (idx, total) in input.op_totals.iter().enumerate() {
        out.push_str(&format!(
            "### {}. `{}` / `{}` paths\n\n",
            idx + 1,
            total.op_name,
            total.shape.as_str()
        ));
        out.push_str("| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |\n");
        out.push_str("|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|\n");
        for row in &total.rows {
            out.push_str(&format!(
                "| {} | `{}` | `{}` | `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} | {:.1}% |\n",
                idx + 1,
                format_sources(&row.source_events),
                row.derivation,
                row.feature,
                format_key_path(&row.key_path),
                row.events_applied,
                row.profile.stack_bytes,
                row.profile.enum_slot_bytes,
                row.profile.payload_bytes,
                row.profile.slack_bytes,
                row.profile.heap_bytes,
                row.profile.total_bytes(),
                percent_of(row.profile.total_bytes(), total.profile.total_bytes())
            ));
        }
        out.push('\n');
    }

    out.push_str("## Top 5 Offenders\n\n");
    for (idx, row) in input.rows.iter().take(5).enumerate() {
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
        out.push_str(&format!("- Recommendation: {}\n", row.recommendation));
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
    let observed = input.per_entity_total as f64;
    let delta = (observed - target).abs();
    let allowed = target * input.tolerance;
    out.push_str(&format!(
        "- `/metrics` `beava_bytes_per_entity_p99`: `{}` bytes\n",
        input.metrics_placeholder
    ));
    out.push_str(&format!(
        "- Profile per-entity estimate: `{}` bytes\n",
        input.per_entity_total
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
    out.push_str("- Path grain is `source_event -> derivation -> feature -> op -> shape`; path detail rows are children of the op/shape rollups above.\n");
    out
}

fn percent_of(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) * 100.0
    }
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
        .then_with(|| a.feature.cmp(&b.feature))
        .then_with(|| a.op_name.cmp(&b.op_name))
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

fn recommendation_for(op_name: &str, shape: ProfileShape, profile: &MemProfile) -> String {
    match (op_name, shape) {
        ("n_unique", ProfileShape::Windowed) => {
            "keep for now; quantify sketch precision and window bucket fanout separately"
                .to_string()
        }
        ("count" | "sum" | "mean", ProfileShape::Windowed) => {
            "keep scalar core; quantify whether wrapper and bucket fanout are the real cost"
                .to_string()
        }
        (
            "quantile" | "n_unique" | "top_k" | "bloom_member" | "entropy",
            ProfileShape::Lifetime,
        ) => "keep for now; quantify sparse-to-dense sketch options next".to_string(),
        ("burst_count" | "trend_residual", _) if profile.stack_bytes >= 80 => {
            "box smaller if the report confirms broad per-entity prevalence".to_string()
        }
        ("count" | "sum" | "mean", _) if profile.heap_bytes == 0 => {
            "keep; scalar state spends only the shared AggOp slot".to_string()
        }
        (_, ProfileShape::Windowed) => {
            "restructure only if lazy bucket materialization still dominates".to_string()
        }
        _ => "keep; no targeted restructuring until workload ranking justifies it".to_string(),
    }
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

fn feature_specs_from_register(register: &JsonValue) -> Result<Vec<FeatureSpec>> {
    let nodes = register
        .get("nodes")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| anyhow!("register payload missing nodes[]"))?;
    let mut out = Vec::new();
    for node in nodes
        .iter()
        .filter(|n| n.get("kind") == Some(&JsonValue::String("derivation".into())))
    {
        let derivation = node
            .get("name")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown_derivation")
            .to_string();
        let source_events = string_array_field(node, "upstreams");
        let table_key_path = string_array_field(node, "table_primary_key");
        let Some(ops) = node.get("ops").and_then(JsonValue::as_array) else {
            continue;
        };
        for step in ops {
            let Some(agg) = step.get("agg").and_then(JsonValue::as_object) else {
                continue;
            };
            let key_path = if table_key_path.is_empty() {
                string_array_field(step, "keys")
            } else {
                table_key_path.clone()
            };
            for (feature, spec) in agg {
                let op_name = spec
                    .get("op")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| anyhow!("feature {feature} missing op"))?
                    .to_string();
                let params = spec.get("params").unwrap_or(&JsonValue::Null);
                out.push(FeatureSpec {
                    source_events: source_events.clone(),
                    derivation: derivation.clone(),
                    feature: feature.clone(),
                    desc: descriptor_from_op(&op_name, params)?,
                    op_name,
                    key_path: key_path.clone(),
                });
            }
        }
    }
    Ok(out)
}

fn descriptor_from_op(op_name: &str, params: &JsonValue) -> Result<AggOpDescriptor> {
    let kind = agg_kind_from_name(op_name)?;
    let mut desc = AggOpDescriptor {
        kind,
        field: string_param(params, "field").or_else(|| string_param(params, "expr")),
        window_ms: duration_param(params, "window")?,
        n: number_param(params, "n").map(|n| n as u32),
        half_life_ms: duration_param(params, "half_life")?,
        sub_window_ms: duration_param(params, "sub_window")?,
        sigma: float_param(params, "sigma"),
        sketch_params: Some(SketchParams {
            percentile_q: float_param(params, "q"),
            top_k_k: number_param(params, "k"),
            bloom_capacity: number_param(params, "capacity"),
            bloom_fpr: float_param(params, "fpr"),
        }),
        ext: AggExtParams {
            buckets: array_f64_param(params, "buckets"),
            n: number_param(params, "n"),
            k: number_param(params, "k"),
            precision: number_param(params, "precision").map(|n| n as u32),
            lat_field: string_param(params, "lat"),
            lon_field: string_param(params, "lon"),
            samples: number_param(params, "samples"),
            categories: string_array_param(params, "categories"),
            max_categories: number_param(params, "max_categories"),
            ..Default::default()
        },
        ..Default::default()
    };
    if desc.field.is_none() && matches!(kind, AggKind::BloomMember | AggKind::Entropy) {
        desc.field = Some("__expr".into());
    }
    Ok(desc)
}

fn agg_kind_from_name(name: &str) -> Result<AggKind> {
    let kind = match name {
        "count" => AggKind::Count,
        "sum" => AggKind::Sum,
        "mean" => AggKind::Avg,
        "min" => AggKind::Min,
        "max" => AggKind::Max,
        "var" => AggKind::Variance,
        "std" => AggKind::StdDev,
        "quantile" => AggKind::Percentile,
        "n_unique" => AggKind::CountDistinct,
        "top_k" => AggKind::TopK,
        "bloom_member" => AggKind::BloomMember,
        "entropy" => AggKind::Entropy,
        "first" => AggKind::First,
        "last" => AggKind::Last,
        "first_n" => AggKind::FirstN,
        "last_n" => AggKind::LastN,
        "lag" => AggKind::Lag,
        "first_seen" => AggKind::FirstSeen,
        "last_seen" => AggKind::LastSeen,
        "age" => AggKind::Age,
        "has_seen" => AggKind::HasSeen,
        "time_since" => AggKind::TimeSince,
        "time_since_last_n" => AggKind::TimeSinceLastN,
        "first_seen_in_window" => AggKind::FirstSeenInWindow,
        "streak" => AggKind::Streak,
        "max_streak" => AggKind::MaxStreak,
        "negative_streak" => AggKind::NegativeStreak,
        "ewma" => AggKind::Ewma,
        "ewvar" => AggKind::EwVar,
        "ew_zscore" => AggKind::EwZScore,
        "decayed_sum" => AggKind::DecayedSum,
        "decayed_count" => AggKind::DecayedCount,
        "twa" => AggKind::Twa,
        "rate_of_change" => AggKind::RateOfChange,
        "inter_arrival_stats" => AggKind::InterArrivalStats,
        "burst_count" => AggKind::BurstCount,
        "delta_from_prev" => AggKind::DeltaFromPrev,
        "trend" => AggKind::Trend,
        "trend_residual" => AggKind::TrendResidual,
        "outlier_count" => AggKind::OutlierCount,
        "value_change_count" => AggKind::ValueChangeCount,
        "z_score" => AggKind::ZScore,
        "histogram" => AggKind::Histogram,
        "hour_of_day_histogram" => AggKind::HourOfDayHistogram,
        "dow_hour_histogram" => AggKind::DowHourHistogram,
        "seasonal_deviation" => AggKind::SeasonalDeviation,
        "event_type_mix" => AggKind::EventTypeMix,
        "most_recent_n" => AggKind::MostRecentN,
        "reservoir_sample" => AggKind::ReservoirSample,
        "geo_velocity" => AggKind::GeoVelocity,
        "geo_distance" => AggKind::GeoDistance,
        "geo_spread" => AggKind::GeoSpread,
        "distance_from_home" => AggKind::DistanceFromHome,
        other => return Err(anyhow!("unsupported agg op {other:?}")),
    };
    Ok(kind)
}

fn row_from_fields(fields: serde_json::Map<String, JsonValue>) -> Row {
    fields.into_iter().fold(Row::new(), |row, (field, value)| {
        row.with_field(&field, json_value_to_beava_value(value))
    })
}

fn event_time_ms(fields: &serde_json::Map<String, JsonValue>) -> Option<i64> {
    fields.get("event_time").and_then(JsonValue::as_i64)
}

fn string_array_field(value: &JsonValue, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn string_param(params: &JsonValue, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
}

fn number_param(params: &JsonValue, key: &str) -> Option<usize> {
    params
        .get(key)
        .and_then(JsonValue::as_u64)
        .map(|n| n as usize)
}

fn float_param(params: &JsonValue, key: &str) -> Option<f64> {
    params.get(key).and_then(JsonValue::as_f64)
}

fn array_f64_param(params: &JsonValue, key: &str) -> Option<Vec<f64>> {
    params.get(key)?.as_array().map(|items| {
        items
            .iter()
            .filter_map(JsonValue::as_f64)
            .collect::<Vec<f64>>()
    })
}

fn string_array_param(params: &JsonValue, key: &str) -> Option<Vec<String>> {
    params.get(key)?.as_array().map(|items| {
        items
            .iter()
            .filter_map(JsonValue::as_str)
            .map(str::to_string)
            .collect::<Vec<String>>()
    })
}

fn duration_param(params: &JsonValue, key: &str) -> Result<Option<u64>> {
    let Some(raw) = params.get(key).and_then(JsonValue::as_str) else {
        return Ok(None);
    };
    parse_duration_ms(raw)
        .map(Some)
        .with_context(|| format!("parse duration param {key}={raw:?}"))
}

fn parse_duration_ms(raw: &str) -> Result<u64> {
    let raw = raw.trim();
    let split_at = raw
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| anyhow!("duration missing unit: {raw}"))?;
    let (n, unit) = raw.split_at(split_at);
    let n: u64 = n.parse()?;
    let multiplier = match unit {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return Err(anyhow!("unsupported duration unit {unit:?}")),
    };
    Ok(n.saturating_mul(multiplier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memprofile_duration_parser_handles_fraud_units() {
        assert_eq!(parse_duration_ms("10s").unwrap(), 10_000);
        assert_eq!(parse_duration_ms("5m").unwrap(), 300_000);
        assert_eq!(parse_duration_ms("24h").unwrap(), 86_400_000);
        assert_eq!(parse_duration_ms("30d").unwrap(), 2_592_000_000);
    }

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
        assert!(report.contains("## Sorted Op Table"));
        assert!(report.contains("## Sorted Op Path Details"));
        assert!(report.contains("## Top 5 Offenders"));
        assert!(report.contains("## Metrics Coherence"));
        assert!(report.contains("Aggregate features discovered: `111`"));
        assert!(report.contains("| Rank | Op | Shape |"));
        assert!(report.contains(
            "| Parent rank | Source event | Derivation | Feature | Key path | Events applied |"
        ));
        assert!(report
            .contains("| `Login` | `LoginByUser` | `ips_distinct_login_1h` | `user_id` | 0 |"));
        assert!(report.contains("- Events applied: `5`"));
    }
}
