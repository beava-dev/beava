//! Per-AggOp memory profile report for realistic workload configs.

use anyhow::{anyhow, Context, Result};
use beava_core::agg_op::{AggExtParams, AggKind, AggOp, AggOpDescriptor, SketchParams};
use beava_core::mem_usage::{sort_profiles_desc, MemBreakdown, MemProfile, MemUsage};
use beava_core::row::{Row, Value};
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
    derivation: String,
    feature: String,
    op_name: String,
    desc: AggOpDescriptor,
}

#[derive(Debug, Clone)]
struct ProfileRow {
    derivation: String,
    feature: String,
    op_name: String,
    profile: MemProfile,
    recommendation: String,
}

struct ReportInput<'a> {
    workload: &'a str,
    events: u64,
    derivation_count: usize,
    feature_count: usize,
    rows: &'a [ProfileRow],
    op_totals: &'a [MemProfile],
    per_entity_total: usize,
    metrics_placeholder: u64,
    tolerance: f64,
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
    let mut rows = Vec::with_capacity(features.len());

    for spec in &features {
        let mut op = AggOp::new(&spec.desc);
        let field = spec.desc.field.as_deref();
        for i in 0..args.events {
            let row = synthetic_row(i);
            op.update(&row, 1_000_000 + i as i64 * 1_000, field, true);
        }
        let mut profile = op.mem_profile();
        profile.label = format!("{}::{} ({})", spec.derivation, spec.feature, spec.op_name);
        rows.push(ProfileRow {
            derivation: spec.derivation.clone(),
            feature: spec.feature.clone(),
            op_name: spec.op_name.clone(),
            recommendation: recommendation_for(&spec.op_name, &profile),
            profile,
        });
    }

    rows.sort_by(|a, b| {
        b.profile
            .total_bytes()
            .cmp(&a.profile.total_bytes())
            .then_with(|| a.profile.label.cmp(&b.profile.label))
    });

    let mut grouped: BTreeMap<String, Vec<MemProfile>> = BTreeMap::new();
    for row in &rows {
        grouped
            .entry(row.op_name.clone())
            .or_default()
            .push(row.profile.clone());
    }
    let mut op_totals: Vec<MemProfile> = grouped
        .into_iter()
        .map(|(op, profiles)| {
            let mut total = MemProfile::new(op, 0);
            for profile in profiles {
                total.stack_bytes += profile.stack_bytes;
                total.heap_bytes += profile.heap_bytes;
                total.breakdown.extend(profile.breakdown);
            }
            total
        })
        .collect();
    sort_profiles_desc(&mut op_totals);

    let per_entity_total: usize = rows.iter().map(|r| r.profile.total_bytes()).sum();
    Ok(render_markdown(ReportInput {
        workload: &args.workload,
        events: args.events,
        derivation_count: workload.derivations.len(),
        feature_count: features.len(),
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
    out.push_str(&format!("- Events replayed per op: `{}`\n", input.events));
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
    out.push_str("| Rank | Op | Stack bytes | Heap bytes | Total bytes |\n");
    out.push_str("|------|----|-------------|------------|-------------|\n");
    for (idx, profile) in input.op_totals.iter().enumerate() {
        out.push_str(&format!(
            "| {} | `{}` | {} | {} | {} |\n",
            idx + 1,
            profile.label,
            profile.stack_bytes,
            profile.heap_bytes,
            profile.total_bytes()
        ));
    }

    out.push_str("\n## Top 5 Offenders\n\n");
    for (idx, row) in input.rows.iter().take(5).enumerate() {
        out.push_str(&format!(
            "### {}. `{}` / `{}` / `{}`\n\n",
            idx + 1,
            row.derivation,
            row.feature,
            row.op_name
        ));
        out.push_str(&format!(
            "- Bytes: stack={} heap={} total={}\n",
            row.profile.stack_bytes,
            row.profile.heap_bytes,
            row.profile.total_bytes()
        ));
        out.push_str(&format!("- Recommendation: {}\n", row.recommendation));
        out.push_str("- Breakdown:\n");
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
    out.push_str("- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.\n");
    out
}

fn top_breakdown(entries: &[MemBreakdown], limit: usize) -> Vec<MemBreakdown> {
    let mut entries = entries.to_vec();
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.label.cmp(&b.label)));
    entries.truncate(limit);
    entries
}

fn recommendation_for(op_name: &str, profile: &MemProfile) -> String {
    match op_name {
        "quantile" | "n_unique" | "top_k" | "bloom_member" | "entropy" => {
            "keep for now; quantify sparse-to-dense sketch options next".to_string()
        }
        "burst_count" | "trend_residual" if profile.stack_bytes >= 80 => {
            "box smaller if the report confirms broad per-entity prevalence".to_string()
        }
        "count" | "sum" | "mean" if profile.heap_bytes == 0 => {
            "keep; scalar state spends only the shared AggOp slot".to_string()
        }
        _ if op_name.contains("window") || profile.label.contains("Windowed") => {
            "restructure only if lazy bucket materialization still dominates".to_string()
        }
        _ => "keep; no targeted restructuring until workload ranking justifies it".to_string(),
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
        let Some(ops) = node.get("ops").and_then(JsonValue::as_array) else {
            continue;
        };
        for step in ops {
            let Some(agg) = step.get("agg").and_then(JsonValue::as_object) else {
                continue;
            };
            for (feature, spec) in agg {
                let op_name = spec
                    .get("op")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| anyhow!("feature {feature} missing op"))?
                    .to_string();
                let params = spec.get("params").unwrap_or(&JsonValue::Null);
                out.push(FeatureSpec {
                    derivation: derivation.clone(),
                    feature: feature.clone(),
                    desc: descriptor_from_op(&op_name, params)?,
                    op_name,
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

fn synthetic_row(i: u64) -> Row {
    let key = format!("k{:08}", i % 100_000);
    Row::new()
        .with_field("event_time", Value::I64(1_000_000 + i as i64 * 1_000))
        .with_field("user_id", Value::Str(key.clone().into()))
        .with_field("card_fp", Value::Str(format!("card{}", i % 2_048).into()))
        .with_field("device_id", Value::Str(format!("dev{}", i % 4_096).into()))
        .with_field(
            "ip_address",
            Value::Str(format!("10.0.{}.{}", (i / 255) % 255, i % 255).into()),
        )
        .with_field("merchant_id", Value::Str(format!("m{}", i % 512).into()))
        .with_field("mcc", Value::Str(format!("{}", 5000 + i % 120).into()))
        .with_field("amount", Value::F64(((i % 10_000) as f64) / 3.0 + 1.0))
        .with_field(
            "card_country",
            Value::Str((if i % 3 == 0 { "US" } else { "CA" }).into()),
        )
        .with_field(
            "ip_country",
            Value::Str((if i % 5 == 0 { "GB" } else { "US" }).into()),
        )
        .with_field("billing_country", Value::Str("US".into()))
        .with_field("lat", Value::F64(37.0 + (i % 100) as f64 * 0.001))
        .with_field("lon", Value::F64(-122.0 - (i % 100) as f64 * 0.001))
        .with_field("declined", Value::I64((i % 7 == 0) as i64))
        .with_field("success", Value::I64((i % 11 != 0) as i64))
        .with_field("is_chargeback", Value::I64((i % 13 == 0) as i64))
        .with_field("user_agent", Value::Str(format!("ua{}", i % 64).into()))
        .with_field("email", Value::Str(format!("u{}@x.test", i % 2048).into()))
        .with_field(
            "email_domain",
            Value::Str(format!("d{}.test", i % 64).into()),
        )
        .with_field("ssn_hash", Value::Str(format!("ssn{}", i % 8192).into()))
        .with_field("__expr", Value::Str(format!("cell{}", i % 4096).into()))
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
        assert!(report.contains("## Sorted Op Table"));
        assert!(report.contains("## Top 5 Offenders"));
        assert!(report.contains("## Metrics Coherence"));
        assert!(report.contains("Aggregate features discovered: `111`"));
    }
}
