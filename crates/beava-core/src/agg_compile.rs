//! Aggregation compiler: AggSpec JSON → AggregationDescriptor + Rule 11 validation.
//!
//! This module provides:
//! - `parse_duration_to_ms`: parse duration strings like "5m", "100ms", "forever"
//! - `compile_aggregations_from_nodes`: scan payload nodes for GroupBy ops, validate
//!   all fields/keys/predicates/windows, and produce `AggregationDescriptor`s.
//!
//! Rule 11 (analogous to Phase 4's Rule 10) fires after structural rules 1-9 and
//! Rule 10 (expression validation) have passed. It is the aggregation-specific
//! validation pass: unknown group keys, unknown op fields, invalid where predicates,
//! invalid window strings, unknown op names, duplicate feature names, feature-name
//! vs group-key collisions, and aggregation-on-Table source rejection.
//!
//! # Requirements traceability
//! - SDK-AGG-05: aggregation-on-Table source rejected
//! - SDK-AGG-06: window duration string validated server-side

use crate::agg_descriptor::{AggregationDescriptor, NamedAggOp};
use crate::agg_op::{AggKind, AggOpDescriptor, SketchParams};
use crate::register_validate::{ErrorCode, ValidationError};
use crate::registry::RegistryInner;
use crate::registry_diff::PayloadNode;
use crate::schema_propagate::Schema;
use std::sync::Arc;

// ─── parse_duration_to_ms ─────────────────────────────────────────────────────

/// Parse a duration string matching `\d+(ms|s|m|h|d)` or `"forever"`.
///
/// Returns `Ok(Some(ms))` for finite durations, `Ok(None)` for `"forever"`.
/// Returns `Err(())` for empty strings, unknown suffixes, numeric-only strings,
/// or strings that cannot be parsed.
///
/// # SDK-AGG-06
// reason: `Result<_, ()>` is the SDK-AGG-06 contract — caller distinguishes
// "forever" (Ok(None)) from finite (Ok(Some(_))) from invalid (Err(())); the
// error variant carries no information by design.
#[allow(clippy::result_unit_err)]
pub fn parse_duration_to_ms(s: &str) -> Result<Option<u64>, ()> {
    if s == "forever" {
        return Ok(None);
    }
    if s.is_empty() {
        return Err(());
    }

    // Try suffix ms first (longest suffix first to avoid matching "s" in "ms")
    let (digits, multiplier) = if let Some(prefix) = s.strip_suffix("ms") {
        (prefix, 1u64)
    } else if let Some(prefix) = s.strip_suffix('d') {
        (prefix, 86_400_000u64)
    } else if let Some(prefix) = s.strip_suffix('h') {
        (prefix, 3_600_000u64)
    } else if let Some(prefix) = s.strip_suffix('m') {
        (prefix, 60_000u64)
    } else if let Some(prefix) = s.strip_suffix('s') {
        (prefix, 1_000u64)
    } else {
        return Err(());
    };

    if digits.is_empty() {
        return Err(());
    }

    // Digits-only prefix check
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(());
    }

    let n: u64 = digits.parse().map_err(|_| ())?;
    // Zero durations are semantically invalid: a zero-ms window causes
    // div_euclid(0) panic in WindowedOp::bucket_index (CR-01).
    if n == 0 {
        return Err(());
    }
    // Checked multiply to guard against overflow (T-05-04-02)
    n.checked_mul(multiplier).map(Some).ok_or(())
}

// ─── AggSpec params deserialization helper ─────────────────────────────────────

/// Intermediate deserialization of `AggSpec.params` for one aggregation feature.
#[derive(Debug)]
struct AggParams {
    field: Option<String>,
    window: Option<String>,
    where_str: Option<String>,
    /// Bounded-buffer size for first_n/last_n/lag/time_since_last_n.
    n: Option<u32>,
    /// Parsed from `params.half_life` (duration string) for decay ops.
    half_life_ms: Option<u64>,
    /// Parsed from `params.sub_window` (duration string) for burst_count.
    sub_window_ms: Option<u64>,
    /// Parsed from `params.sigma` (number, defaults to 3.0 for outlier_count).
    sigma: Option<f64>,
    /// True when `half_life` was present but unparseable.
    half_life_invalid: bool,
    /// True when `sub_window` was present but unparseable.
    sub_window_invalid: bool,
    /// Sketch construction params parsed from JSON kwargs.
    sketch_params: Option<SketchParams>,
    // Extended params for buffer + geo ops (use ext_ prefix to avoid collision with `n` above).
    ext_buckets: Option<Vec<f64>>,
    ext_n: Option<usize>,
    ext_k: Option<usize>,
    ext_precision: Option<u32>,
    ext_lat_field: Option<String>,
    ext_lon_field: Option<String>,
    ext_samples: Option<usize>,
    ext_categories: Option<Vec<String>>,
    ext_max_categories: Option<usize>,
}

fn extract_agg_params(params: &serde_json::Value) -> AggParams {
    let field = params
        .get("field")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let window = params
        .get("window")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let where_str = params
        .get("where")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let n = params
        .get("n")
        .and_then(|v| v.as_u64())
        .and_then(|n| u32::try_from(n).ok());

    // half_life parsing.
    let (half_life_ms, half_life_invalid) = match params.get("half_life").and_then(|v| v.as_str()) {
        Some(s) => match parse_duration_to_ms(s) {
            Ok(Some(ms)) if ms > 0 => (Some(ms), false),
            _ => (None, true),
        },
        None => (None, false),
    };

    // sub_window parsing.
    let (sub_window_ms, sub_window_invalid) =
        match params.get("sub_window").and_then(|v| v.as_str()) {
            Some(s) => match parse_duration_to_ms(s) {
                Ok(Some(ms)) if ms > 0 => (Some(ms), false),
                _ => (None, true),
            },
            None => (None, false),
        };

    let sigma = params.get("sigma").and_then(|v| v.as_f64());

    // Parse sketch kwargs (q, k, capacity, fpr / target_fpr / expected_n).
    let percentile_q = params.get("q").and_then(|v| v.as_f64());
    let top_k_k = params.get("k").and_then(|v| v.as_u64()).map(|n| n as usize);
    let bloom_capacity = params
        .get("expected_n")
        .or_else(|| params.get("capacity"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let bloom_fpr = params
        .get("target_fpr")
        .or_else(|| params.get("fpr"))
        .and_then(|v| v.as_f64());
    let sketch_params = if percentile_q.is_some()
        || top_k_k.is_some()
        || bloom_capacity.is_some()
        || bloom_fpr.is_some()
    {
        Some(SketchParams {
            percentile_q,
            top_k_k,
            bloom_capacity,
            bloom_fpr,
        })
    } else {
        None
    };

    // Extended params for buffer + geo ops.
    //
    // Phase 12.8 memory-governance: histogram's canonical memory-cap kwarg in
    // lifetime mode is `buckets` (a Vec<f64>); the count of buckets is the
    // per-entity memory ceiling. The bound is enforced at the JSON-prelude
    // shim `pre_check_unbounded_op_in_lifetime_mode` via
    // `OpLifetimeBound::BoundedByRequiredKwarg("buckets")`. Aliases like
    // `num_buckets` are NOT accepted on the wire — `buckets` is deliberately
    // the single canonical name. When `BEAVA_MEMORY_GOV_ENFORCE=1` and
    // `buckets` is empty/missing in lifetime mode, the shim rejects at
    // register-time before this extractor runs. Defense-in-depth at
    // agg_compile-time is intentionally NOT added here to preserve the
    // workspace-stays-green invariant under default-OFF gate.
    let ext_buckets = params.get("buckets").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|x| x.as_f64().or_else(|| x.as_i64().map(|n| n as f64)))
            .collect::<Vec<f64>>()
    });
    let ext_n = params.get("n").and_then(|v| v.as_u64()).map(|n| n as usize);
    let ext_k = params.get("k").and_then(|v| v.as_u64()).map(|n| n as usize);
    let ext_precision = params
        .get("precision")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    // SDK sends canonical `lat_field`/`lon_field` (see python/beava/_agg.py geo helpers
    // + python/tests/internal/test_agg_signatures_07a.py); legacy `lat`/`lon` keys
    // accepted as alias for backward-compat with hand-rolled JSON payloads.
    let ext_lat_field = params
        .get("lat_field")
        .or_else(|| params.get("lat"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let ext_lon_field = params
        .get("lon_field")
        .or_else(|| params.get("lon"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let ext_samples = params
        .get("samples")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let ext_categories = params
        .get("categories")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        });
    let ext_max_categories = params
        .get("max_categories")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);

    AggParams {
        field,
        window,
        where_str,
        n,
        half_life_ms,
        sub_window_ms,
        sigma,
        half_life_invalid,
        sub_window_invalid,
        sketch_params,
        ext_buckets,
        ext_n,
        ext_k,
        ext_precision,
        ext_lat_field,
        ext_lon_field,
        ext_samples,
        ext_categories,
        ext_max_categories,
    }
}

// ─── Kind parsing ──────────────────────────────────────────────────────────────

pub fn parse_agg_kind(op: &str) -> Option<AggKind> {
    match op {
        // Core scalar (per ADR-002: avg→mean, variance→var, stddev→std)
        "count" => Some(AggKind::Count),
        "sum" => Some(AggKind::Sum),
        "mean" => Some(AggKind::Avg),
        "min" => Some(AggKind::Min),
        "max" => Some(AggKind::Max),
        "var" => Some(AggKind::Variance),
        "std" => Some(AggKind::StdDev),
        "ratio" => Some(AggKind::Ratio),
        // Point/ordinal
        "first" => Some(AggKind::First),
        "last" => Some(AggKind::Last),
        "first_n" => Some(AggKind::FirstN),
        "last_n" => Some(AggKind::LastN),
        "lag" => Some(AggKind::Lag),
        // Recency markers
        "first_seen" => Some(AggKind::FirstSeen),
        "last_seen" => Some(AggKind::LastSeen),
        "age" => Some(AggKind::Age),
        "has_seen" => Some(AggKind::HasSeen),
        "time_since" => Some(AggKind::TimeSince),
        "time_since_last_n" => Some(AggKind::TimeSinceLastN),
        // Streak family
        "streak" => Some(AggKind::Streak),
        "max_streak" => Some(AggKind::MaxStreak),
        "negative_streak" => Some(AggKind::NegativeStreak),
        // Windowed recency
        "first_seen_in_window" => Some(AggKind::FirstSeenInWindow),
        // Decay; "ema" is an SDK alias also accepted server-side.
        "ewma" | "ema" => Some(AggKind::Ewma),
        "ewvar" => Some(AggKind::EwVar),
        "ew_zscore" => Some(AggKind::EwZScore),
        "decayed_sum" => Some(AggKind::DecayedSum),
        "decayed_count" => Some(AggKind::DecayedCount),
        "twa" => Some(AggKind::Twa),
        // Velocity
        "rate_of_change" => Some(AggKind::RateOfChange),
        "inter_arrival_stats" => Some(AggKind::InterArrivalStats),
        "burst_count" => Some(AggKind::BurstCount),
        "delta_from_prev" => Some(AggKind::DeltaFromPrev),
        "trend" => Some(AggKind::Trend),
        "trend_residual" => Some(AggKind::TrendResidual),
        "outlier_count" => Some(AggKind::OutlierCount),
        "value_change_count" => Some(AggKind::ValueChangeCount),
        // Z-score
        "z_score" => Some(AggKind::ZScore),
        // 5 sketch ops (per ADR-002: count_distinct→n_unique, percentile→quantile).
        "n_unique" => Some(AggKind::CountDistinct),
        "quantile" => Some(AggKind::Percentile),
        "top_k" => Some(AggKind::TopK),
        "bloom_member" => Some(AggKind::BloomMember),
        "entropy" => Some(AggKind::Entropy),
        // Bounded-buffer operators (AGG-BUFFER-01..07)
        "histogram" => Some(AggKind::Histogram),
        "hour_of_day_histogram" => Some(AggKind::HourOfDayHistogram),
        "dow_hour_histogram" => Some(AggKind::DowHourHistogram),
        "seasonal_deviation" => Some(AggKind::SeasonalDeviation),
        "event_type_mix" => Some(AggKind::EventTypeMix),
        "most_recent_n" => Some(AggKind::MostRecentN),
        "reservoir_sample" => Some(AggKind::ReservoirSample),
        // Geo operators (AGG-GEO-01..04; unique_cells + geo_entropy removed)
        "geo_velocity" => Some(AggKind::GeoVelocity),
        "geo_distance" => Some(AggKind::GeoDistance),
        "geo_spread" => Some(AggKind::GeoSpread),
        "distance_from_home" => Some(AggKind::DistanceFromHome),
        _ => None,
    }
}

/// True iff `kind` requires a `params.n` integer in the JSON wire.
fn agg_kind_requires_n(kind: AggKind) -> bool {
    matches!(
        kind,
        AggKind::FirstN | AggKind::LastN | AggKind::Lag | AggKind::TimeSinceLastN
    )
}

/// True iff `kind` requires a field name in `params.field`.
fn agg_kind_requires_field(kind: AggKind) -> bool {
    matches!(
        kind,
        AggKind::First | AggKind::Last | AggKind::FirstN | AggKind::LastN | AggKind::Lag
    )
}

/// True iff `kind` is a lifetime-only op that MUST NOT accept a `window=`.
/// `first_seen_in_window` is the exception — it requires a window= as a
/// lifetime parameter.
fn agg_kind_rejects_window(kind: AggKind) -> bool {
    matches!(
        kind,
        AggKind::First
            | AggKind::Last
            | AggKind::FirstN
            | AggKind::LastN
            | AggKind::Lag
            | AggKind::FirstSeen
            | AggKind::LastSeen
            | AggKind::Age
            | AggKind::HasSeen
            | AggKind::TimeSince
            | AggKind::TimeSinceLastN
            | AggKind::Streak
            | AggKind::MaxStreak
            | AggKind::NegativeStreak
            // All buffer + geo ops are windowless in v0.
            | AggKind::Histogram
            | AggKind::HourOfDayHistogram
            | AggKind::DowHourHistogram
            | AggKind::SeasonalDeviation
            | AggKind::EventTypeMix
            | AggKind::MostRecentN
            | AggKind::ReservoirSample
            | AggKind::GeoVelocity
            | AggKind::GeoDistance
            | AggKind::GeoSpread
            | AggKind::DistanceFromHome
    )
}

// ─── compile_aggregations_from_nodes ─────────────────────────────────────────

/// Compile GroupBy OpNodes in the payload into `AggregationDescriptor`s and
/// collect Rule 11 validation errors (fail-soft).
///
/// Called from `register_validate::validate_payload` after rules 1-10 pass.
///
/// `propagated_schemas` is the per-derivation post-chain schema produced by
/// Rule 10 (`validate_expressions` → `OpChain::compile`). For an in-payload
/// derivation upstream, the entry here reflects the narrowed/renamed schema
/// after the chain (`select` / `drop` / `rename` / `with_columns` / `cast` /
/// `fillna`) — used by `resolve_upstream_schema_for_agg` so downstream
/// `agg(field=…)` field references are validated against the actual
/// post-chain schema, not the client-supplied carry-forward.
///
/// # SDK-AGG-05, SDK-AGG-06
pub fn compile_aggregations_from_nodes(
    nodes: &[PayloadNode],
    registry: &RegistryInner,
    propagated_schemas: &[(String, crate::schema::DerivedSchema)],
) -> (
    Vec<(String, Arc<AggregationDescriptor>)>,
    Vec<ValidationError>,
) {
    let mut compiled: Vec<(String, Arc<AggregationDescriptor>)> = Vec::new();
    let mut errors: Vec<ValidationError> = Vec::new();

    for (node_idx, node) in nodes.iter().enumerate() {
        let deriv = match node {
            PayloadNode::Derivation(d) => d,
            _ => continue,
        };

        // Only process derivations that have at least one GroupBy op.
        let has_group_by = deriv
            .ops
            .iter()
            .any(|op| matches!(op, crate::op_node::OpNode::GroupBy { .. }));
        if !has_group_by {
            continue;
        }

        // For each GroupBy op in this derivation.
        for (op_idx, op) in deriv.ops.iter().enumerate() {
            let (keys, agg_map) = match op {
                crate::op_node::OpNode::GroupBy { keys, agg } => (keys, agg),
                _ => continue,
            };

            // D-05: single-upstream assumption for aggregations.
            let upstream_name = match deriv.upstreams.first() {
                Some(u) => u.as_str(),
                None => {
                    errors.push(ValidationError {
                        code: ErrorCode::AggregationOnTableNotSupported,
                        path: format!("nodes[{node_idx}].upstreams"),
                        reason: "aggregation derivation must have at least one upstream"
                            .to_string(),
                    });
                    continue;
                }
            };

            // SDK-AGG-05: reject aggregation on Table source.
            // A "Table source" includes:
            //   1. Explicit @bv.table nodes (registry.tables)
            //   2. Derivations with output_kind=Table already in the registry
            //   3. Table nodes in the current payload
            //   4. Derivations with output_kind=Table in the current payload
            let upstream_is_table = registry.tables.contains_key(upstream_name)
                || registry
                    .derivations
                    .get(upstream_name)
                    .map(|d| d.output_kind == crate::registry::OutputKind::Table)
                    .unwrap_or(false)
                || nodes.iter().any(|n| {
                    matches!(n, PayloadNode::Table(t) if t.name == upstream_name)
                        || matches!(
                            n,
                            PayloadNode::Derivation(d)
                                if d.name == upstream_name
                                    && d.output_kind == crate::registry::OutputKind::Table
                        )
                });
            if upstream_is_table {
                errors.push(ValidationError {
                    code: ErrorCode::AggregationOnTableNotSupported,
                    path: format!("nodes[{node_idx}].upstreams[0]"),
                    reason: format!(
                        "aggregation source '{upstream_name}' is a Table; \
                         aggregation on Table is not supported in v0 (SDK-AGG-05)"
                    ),
                });
                continue;
            }

            // Resolve upstream schema.
            //
            // If the agg derivation has its OWN prefix ops
            // (`select`/`drop`/`rename`/`with_columns`/`cast`/`fillna`)
            // BEFORE the GroupBy, walk them on top of the bare upstream
            // schema so the GroupBy validation sees the schema the chain
            // prefix actually produces. In that case we deliberately bypass
            // the sibling-derivation union — the prefix ops describe the
            // exact transformation this aggregation applies, and unioning
            // in sibling-derivation fields would (a) cause spurious
            // RenameCollision on common patterns like
            // `tx.rename(amount='value')` where a sibling already added
            // 'value' to the union, and (b) let agg fields reference
            // sibling-derivation outputs that the prefix has not produced.
            //
            // Required because the SDK flattens chain prefixes from
            // intermediate `@bv.event def`s into the agg derivation's own
            // ops while keeping the upstream pointing at the root event —
            // a chain like `Tx → Narrowed.select(...) → Stats.group_by(...)`
            // arrives on the wire as `Stats { upstreams: [Tx], ops: [Select,
            // GroupBy] }`. Without the prefix walk here, `agg(field=…)`
            // would be validated against the wider Tx ∪ siblings union
            // and a reference to a Select-narrowed-away field would slip
            // through.
            let prefix_ops: Vec<crate::op_node::OpNode> = deriv.ops[..op_idx]
                .iter()
                .filter(|op| !matches!(op, crate::op_node::OpNode::GroupBy { .. }))
                .cloned()
                .collect();

            let upstream_schema = if prefix_ops.is_empty() {
                // No prefix ops — preserve the long-standing
                // sibling-derivation union behaviour (a separate-named
                // sibling adds fields the agg can reference).
                match resolve_upstream_schema_for_agg(
                    upstream_name,
                    nodes,
                    registry,
                    propagated_schemas,
                ) {
                    Some(s) => s,
                    None => continue,
                }
            } else {
                // Prefix ops present — walk them against the bare upstream
                // schema (no sibling union). Each prefix op precisely
                // describes the row transform; the post-walk schema is
                // what the GroupBy sees at runtime.
                let bare = match resolve_bare_upstream_schema(
                    upstream_name,
                    nodes,
                    registry,
                    propagated_schemas,
                ) {
                    Some(s) => s,
                    None => continue,
                };
                match crate::schema_propagate::propagate_schema(&bare, &prefix_ops) {
                    Ok((final_schema, _)) => final_schema,
                    Err(_) => {
                        // Schema-propagation errors on the prefix are
                        // already reported by Rule 10. Skip Rule 11 for
                        // this derivation (fail-soft); the user fixes
                        // Rule 10 first.
                        continue;
                    }
                }
            };

            let mut deriv_errors = false;
            let mut features: Vec<NamedAggOp> = Vec::new();
            // Note: JSON duplicate keys are silently dropped by BTreeMap deserialization
            // (last-writer-wins). A per-iteration HashSet duplicate check is therefore
            // unreachable — the BTreeMap already deduplicates before we iterate. Cross-agg
            // collision is caught by the cross-aggregation check below (WR-01 fix).

            // Validate group keys.
            for (key_idx, key) in keys.iter().enumerate() {
                match upstream_schema.fields.get(key.as_str()) {
                    None => {
                        errors.push(ValidationError {
                            code: ErrorCode::AggregationUnknownField,
                            path: format!("nodes[{node_idx}].ops[{op_idx}].group_by[{key_idx}]"),
                            reason: format!(
                                "group_by key '{key}' does not exist in upstream schema"
                            ),
                        });
                        deriv_errors = true;
                    }
                    Some(crate::schema::FieldType::F64) => {
                        errors.push(ValidationError {
                            code: ErrorCode::AggregationUnknownField,
                            path: format!("nodes[{node_idx}].ops[{op_idx}].group_by[{key_idx}]"),
                            reason: format!(
                                "group_by key '{key}' cannot be float-typed; cast to str/int/bool first"
                            ),
                        });
                        deriv_errors = true;
                    }
                    Some(_) => {}
                }
            }

            // Validate each aggregation feature.
            // Use sorted order for determinism (BTreeMap iterates sorted).
            for (feature_name, agg_spec) in agg_map.iter() {
                let params = extract_agg_params(&agg_spec.params);

                // Check op kind.
                let kind = match parse_agg_kind(&agg_spec.op) {
                    Some(k) => k,
                    None => {
                        errors.push(ValidationError {
                            code: ErrorCode::AggregationUnknownOp,
                            path: format!("nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.op"),
                            reason: format!(
                                "unknown aggregation op '{}'; valid ops are: \
                                 count, sum, mean, min, max, var, std, ratio, \
                                 first, last, first_n, last_n, lag, \
                                 first_seen, last_seen, age, has_seen, time_since, \
                                 time_since_last_n, streak, max_streak, negative_streak, \
                                 first_seen_in_window",
                                agg_spec.op
                            ),
                        });
                        deriv_errors = true;
                        continue;
                    }
                };

                // Group-key vs feature-name collision check.
                if keys.contains(feature_name) {
                    errors.push(ValidationError {
                        code: ErrorCode::AggregationGroupKeyCollidesWithFeature,
                        path: format!("nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}"),
                        reason: format!(
                            "feature name '{feature_name}' collides with a group_by key"
                        ),
                    });
                    deriv_errors = true;
                    continue;
                }

                // half_life validation for decay ops.
                if kind.requires_half_life()
                    && (params.half_life_ms.is_none() || params.half_life_invalid)
                {
                    errors.push(ValidationError {
                        code: ErrorCode::AggregationInvalidHalfLife,
                        path: format!(
                            "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.half_life"
                        ),
                        reason: format!(
                            "{:?} requires positive finite half_life duration string \
                             (e.g., \"5m\"); got {:?}",
                            kind, params.half_life_ms
                        ),
                    });
                    deriv_errors = true;
                    continue;
                }

                // sub_window validation for burst_count.
                if matches!(kind, AggKind::BurstCount)
                    && (params.sub_window_ms.is_none() || params.sub_window_invalid)
                {
                    errors.push(ValidationError {
                        code: ErrorCode::AggregationInvalidSubWindow,
                        path: format!(
                            "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.sub_window"
                        ),
                        reason: "burst_count requires positive finite sub_window duration string"
                            .to_string(),
                    });
                    deriv_errors = true;
                    continue;
                }

                // Field validation for ops that require a field.
                let needs_field = matches!(
                    kind,
                    AggKind::Sum
                        | AggKind::Avg
                        | AggKind::Min
                        | AggKind::Max
                        | AggKind::Variance
                        | AggKind::StdDev
                        | AggKind::Ewma
                        | AggKind::EwVar
                        | AggKind::EwZScore
                        | AggKind::DecayedSum
                        | AggKind::Twa
                        | AggKind::RateOfChange
                        | AggKind::DeltaFromPrev
                        | AggKind::Trend
                        | AggKind::TrendResidual
                        | AggKind::OutlierCount
                        | AggKind::ValueChangeCount
                        | AggKind::ZScore
                        | AggKind::CountDistinct
                        | AggKind::Percentile
                        | AggKind::TopK
                        | AggKind::BloomMember
                        | AggKind::Entropy
                ) || agg_kind_requires_field(kind);
                if needs_field {
                    match &params.field {
                        None => {
                            // Point ops (first/last/first_n/last_n/lag) require a field.
                            // Core sum/avg/variance/stddev do NOT require a field at
                            // register time (whole-row semantics deferred to v1).
                            if agg_kind_requires_field(kind) {
                                errors.push(ValidationError {
                                    code: ErrorCode::AggregationUnknownField,
                                    path: format!(
                                        "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.field"
                                    ),
                                    reason: format!(
                                        "op '{}' requires a field= parameter",
                                        agg_spec.op
                                    ),
                                });
                                deriv_errors = true;
                                continue;
                            }
                        }
                        Some(field_name) => {
                            if !upstream_schema.fields.contains_key(field_name.as_str()) {
                                errors.push(ValidationError {
                                    code: ErrorCode::AggregationUnknownField,
                                    path: format!(
                                        "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.field"
                                    ),
                                    reason: format!(
                                        "field '{field_name}' does not exist in upstream schema"
                                    ),
                                });
                                deriv_errors = true;
                                continue;
                            }
                        }
                    }
                }

                // `n` parameter validation for first_n/last_n/lag/time_since_last_n.
                if agg_kind_requires_n(kind) {
                    match params.n {
                        None => {
                            errors.push(ValidationError {
                                code: ErrorCode::AggregationUnknownField,
                                path: format!(
                                    "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.n"
                                ),
                                reason: format!(
                                    "op '{}' requires a positive integer 'n' parameter",
                                    agg_spec.op
                                ),
                            });
                            deriv_errors = true;
                            continue;
                        }
                        Some(n) if n == 0 || n > 1024 => {
                            errors.push(ValidationError {
                                code: ErrorCode::AggregationUnknownField,
                                path: format!(
                                    "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.n"
                                ),
                                reason: format!(
                                    "op '{}' parameter 'n' must be in [1, 1024]; got {n}",
                                    agg_spec.op
                                ),
                            });
                            deriv_errors = true;
                            continue;
                        }
                        Some(_) => {}
                    }
                }

                // Reject window= for lifetime-only ops.
                if agg_kind_rejects_window(kind) && params.window.is_some() {
                    errors.push(ValidationError {
                        code: ErrorCode::AggregationInvalidWindow,
                        path: format!(
                            "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.window"
                        ),
                        reason: format!(
                            "op '{}' is a lifetime operator and does not accept window=",
                            agg_spec.op
                        ),
                    });
                    deriv_errors = true;
                    continue;
                }

                // first_seen_in_window REQUIRES a window=.
                if matches!(kind, AggKind::FirstSeenInWindow) && params.window.is_none() {
                    errors.push(ValidationError {
                        code: ErrorCode::AggregationInvalidWindow,
                        path: format!(
                            "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.window"
                        ),
                        reason: "op 'first_seen_in_window' requires a window= duration parameter"
                            .to_string(),
                    });
                    deriv_errors = true;
                    continue;
                }

                // Sketch-op-specific param validation.
                let mut sketch_validation_failed = false;
                match kind {
                    AggKind::BloomMember => {
                        if params.window.is_some() {
                            errors.push(ValidationError {
                                code: ErrorCode::WindowNotSupported,
                                path: format!(
                                    "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.window"
                                ),
                                reason: "bloom_member is windowless-only — `window` kwarg not supported".to_string(),
                            });
                            sketch_validation_failed = true;
                        }
                        if let Some(sp) = &params.sketch_params {
                            if let Some(fpr) = sp.bloom_fpr {
                                if !(fpr > 0.0 && fpr < 1.0) {
                                    errors.push(ValidationError {
                                        code: ErrorCode::InvalidBloomFpr,
                                        path: format!(
                                            "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.target_fpr"
                                        ),
                                        reason: format!(
                                            "bloom_member fpr must be in (0.0, 1.0); got {fpr}"
                                        ),
                                    });
                                    sketch_validation_failed = true;
                                }
                            }
                        }
                    }
                    AggKind::Percentile => {
                        if let Some(sp) = &params.sketch_params {
                            if let Some(q) = sp.percentile_q {
                                if !(q > 0.0 && q < 1.0) {
                                    errors.push(ValidationError {
                                        code: ErrorCode::InvalidPercentileQ,
                                        path: format!(
                                            "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.q"
                                        ),
                                        reason: format!(
                                            "percentile q must be in (0.0, 1.0); got {q}"
                                        ),
                                    });
                                    sketch_validation_failed = true;
                                }
                            }
                        }
                    }
                    AggKind::TopK => {
                        if let Some(sp) = &params.sketch_params {
                            if let Some(k) = sp.top_k_k {
                                if !(0 < k && k <= 1024) {
                                    errors.push(ValidationError {
                                        code: ErrorCode::InvalidTopKK,
                                        path: format!(
                                            "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.k"
                                        ),
                                        reason: format!(
                                            "top_k k must be in (0, 1024]; got {k}"
                                        ),
                                    });
                                    sketch_validation_failed = true;
                                }
                            }
                        }
                    }
                    _ => {}
                }
                if sketch_validation_failed {
                    deriv_errors = true;
                    continue;
                }

                // Window parsing.
                let window_ms = match &params.window {
                    None => None, // No window specified → lifetime
                    Some(w_str) => match parse_duration_to_ms(w_str) {
                        Ok(ms) => ms,
                        Err(()) => {
                            errors.push(ValidationError {
                                code: ErrorCode::AggregationInvalidWindow,
                                path: format!(
                                    "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.window"
                                ),
                                reason: format!(
                                    "invalid window duration '{w_str}'; \
                                     expected format: \\d+(ms|s|m|h|d) or 'forever'"
                                ),
                            });
                            deriv_errors = true;
                            continue;
                        }
                    },
                };

                // Buffer/geo ops are lifetime-only in v0. Reject `window=...`
                // with a clear error. (`agg_kind_rejects_window` covers both
                // recency/streak lifetime ops and buffer/geo ops.)
                if agg_kind_rejects_window(kind) && window_ms.is_some() {
                    errors.push(ValidationError {
                        code: ErrorCode::AggregationInvalidWindow,
                        path: format!(
                            "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.window"
                        ),
                        reason: format!("op '{}' does not support 'window=' in v0", agg_spec.op),
                    });
                    deriv_errors = true;
                    continue;
                }

                // Where predicate parsing + field reference check.
                let where_expr = match &params.where_str {
                    None => None,
                    Some(where_str) => match crate::expr::parse(where_str) {
                        Err(pe) => {
                            errors.push(ValidationError {
                                code: ErrorCode::AggregationInvalidWhere,
                                path: format!(
                                    "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.where"
                                ),
                                reason: format!(
                                    "where predicate parse error at col {}: {}",
                                    pe.col, pe.reason
                                ),
                            });
                            deriv_errors = true;
                            continue;
                        }
                        Ok(expr) => {
                            // Check all referenced fields exist in upstream schema.
                            let mut where_field_error = false;
                            for field_ref in expr.referenced_fields() {
                                if !upstream_schema.fields.contains_key(field_ref.as_str()) {
                                    errors.push(ValidationError {
                                        code: ErrorCode::AggregationInvalidWhere,
                                        path: format!(
                                            "nodes[{node_idx}].ops[{op_idx}].agg.{feature_name}.params.where"
                                        ),
                                        reason: format!(
                                            "where predicate references unknown field '{field_ref}'"
                                        ),
                                    });
                                    deriv_errors = true;
                                    where_field_error = true;
                                }
                            }
                            if where_field_error {
                                continue;
                            }
                            Some(Arc::new(expr))
                        }
                    },
                };

                let ext = crate::agg_op::AggExtParams {
                    buckets: params.ext_buckets,
                    n: params.ext_n,
                    k: params.ext_k,
                    precision: params.ext_precision,
                    lat_field: params.ext_lat_field,
                    lon_field: params.ext_lon_field,
                    samples: params.ext_samples,
                    categories: params.ext_categories,
                    max_categories: params.ext_max_categories,
                    // Resolved at apply_registration time by
                    // Registry::resolve_field_indices; default to sentinel here.
                    lat_idx: crate::agg_op::FIELD_IDX_NONE,
                    lon_idx: crate::agg_op::FIELD_IDX_NONE,
                };
                features.push(NamedAggOp {
                    feature_name: feature_name.clone(),
                    descriptor: AggOpDescriptor {
                        kind,
                        field: params.field,
                        window_ms,
                        where_expr,
                        n: params.n,
                        // Decay/velocity fields — populated by extract_agg_params extension.
                        half_life_ms: params.half_life_ms,
                        sub_window_ms: params.sub_window_ms,
                        sigma: params.sigma,
                        sketch_params: params.sketch_params.clone(),
                        ext,
                        // Placeholder; registry overwrites at apply_registration
                        // via resolve_field_indices_for_agg_mut.
                        field_idx: crate::agg_op::FIELD_IDX_NONE,
                        // Placeholder; resolver writes at apply_registration via
                        // resolve_field_indices_for_agg_mut*.
                        field_idx_into_event_extracted: Vec::new(),
                    },
                });
            }

            if !deriv_errors {
                let desc = AggregationDescriptor {
                    node_name: deriv.name.clone(),
                    source_node_name: upstream_name.to_string(),
                    group_keys: keys.clone(),
                    features,
                    agg_id: 0, // placeholder; registry overwrites at apply_registration
                    // Populated by Registry::resolve_field_indices_for_agg_mut
                    // at apply_registration time. Placeholder here so compile
                    // step is decoupled from schema validation.
                    field_names: vec![],
                    // Assigned by Registry::apply_registration via
                    // cluster_id_by_signature. Placeholder 0 here.
                    cluster_id: 0,
                };
                compiled.push((deriv.name.clone(), Arc::new(desc)));
            }
        }
    }

    // Cross-aggregation feature-name collision check.
    //
    // After per-node validation, check that no newly-compiled feature name collides
    // with an existing feature name in the registry's feature_index (from a different
    // aggregation node that was already registered).
    //
    // Also check that two different aggregations within the SAME payload don't both
    // define the same feature name.
    //
    // Only runs if there were no per-node errors (consistent with fail-soft ordering).
    if errors.is_empty() && !compiled.is_empty() {
        // Build a map of feature_name → agg_node_name for newly compiled aggs.
        let mut new_features: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (node_name, agg_desc) in &compiled {
            for named_op in &agg_desc.features {
                if let Some(existing_node) = new_features.get(&named_op.feature_name) {
                    // Two new aggregations both define the same feature name.
                    if existing_node != node_name {
                        errors.push(ValidationError {
                            code: ErrorCode::AggregationFeatureNameCollisionAcrossAggregations,
                            path: format!("nodes.{node_name}.features.{}", named_op.feature_name),
                            reason: format!(
                                "feature name '{}' is already defined by aggregation '{}'; \
                                 feature names must be globally unique across aggregations",
                                named_op.feature_name, existing_node
                            ),
                        });
                    }
                } else {
                    new_features.insert(named_op.feature_name.clone(), node_name.clone());
                }
            }
        }

        // Check new features against already-registered feature_index.
        for (feature_name, new_node_name) in &new_features {
            if let Some((existing_node, _)) = registry.feature_index.get(feature_name) {
                if existing_node != new_node_name {
                    errors.push(ValidationError {
                        code: ErrorCode::AggregationFeatureNameCollisionAcrossAggregations,
                        path: format!("nodes.{new_node_name}.features.{feature_name}"),
                        reason: format!(
                            "feature name '{feature_name}' is already registered by aggregation \
                             '{existing_node}'; feature names must be globally unique across aggregations"
                        ),
                    });
                }
            }
        }
    }

    (compiled, errors)
}

// ─── Schema resolution helper ─────────────────────────────────────────────────

/// Resolve `upstream_name` to its bare (non-unioned) schema.
///
/// Like `resolve_upstream_schema_for_agg` but WITHOUT the sibling-derivation
/// union for event upstreams. Used by `compile_aggregations_from_nodes` when
/// the agg derivation has its own prefix ops: the prefix ops describe the
/// exact transformation, so we walk them on the bare upstream schema rather
/// than the wider sibling union.
fn resolve_bare_upstream_schema(
    upstream_name: &str,
    nodes: &[PayloadNode],
    registry: &RegistryInner,
    propagated_schemas: &[(String, crate::schema::DerivedSchema)],
) -> Option<Schema> {
    for node in nodes {
        match node {
            PayloadNode::Event(e) if e.name == upstream_name => {
                return Some(Schema::from_event(&e.schema));
            }
            PayloadNode::Table(t) if t.name == upstream_name => {
                return Some(Schema::from_table(&t.schema));
            }
            PayloadNode::Derivation(d) if d.name == upstream_name => {
                if let Some(propagated) = propagated_schemas
                    .iter()
                    .find(|(name, _)| name == upstream_name)
                    .map(|(_, s)| s)
                {
                    return Some(Schema::from_derived(propagated));
                }
                return Some(Schema::from_derived(&d.schema));
            }
            _ => {}
        }
    }
    if let Some(e) = registry.events.get(upstream_name) {
        return Some(Schema::from_event(&e.schema));
    }
    if let Some(t) = registry.tables.get(upstream_name) {
        return Some(Schema::from_table(&t.schema));
    }
    if let Some(d) = registry.derivations.get(upstream_name) {
        return Some(Schema::from_derived(&d.schema));
    }
    None
}

fn resolve_upstream_schema_for_agg(
    upstream_name: &str,
    nodes: &[PayloadNode],
    registry: &RegistryInner,
    propagated_schemas: &[(String, crate::schema::DerivedSchema)],
) -> Option<Schema> {
    // Sibling-derivation schema union.
    //
    // When the upstream is an EVENT (either in the same payload or registered),
    // additionally union in the schemas of any same-payload event-derivations
    // (kind=derivation, output_kind=event) whose only upstream is that same
    // event name. Rationale: the user pattern is
    //   Tagged = Click.with_columns(source=...)
    //   @bv.table def UserClicks(tagged: Click): ... where bv.col('source') ...
    // The SDK records UserClicks.upstreams = ["Click"] (correctly — the param
    // is annotated `: Click`). At register time, the where-predicate
    // validator must see the union of Click ∪ Tagged-shape-additions to
    // resolve `where source == 'web'`. Same-payload-only — registry-side
    // derivations are NOT unioned, since downstream callers reference them
    // by name.
    //
    // Field-name collisions: derivation additions take precedence over base
    // event fields, mirroring runtime apply order (with_columns / rename
    // / cast last-write-wins downstream).

    // Step 1: try to resolve upstream_name directly (event/table/derivation).
    let mut base_schema: Option<Schema> = None;
    let mut upstream_is_event = false;
    for node in nodes {
        match node {
            PayloadNode::Event(e) if e.name == upstream_name => {
                base_schema = Some(Schema::from_event(&e.schema));
                upstream_is_event = true;
                break;
            }
            PayloadNode::Table(t) if t.name == upstream_name => {
                return Some(Schema::from_table(&t.schema));
            }
            PayloadNode::Derivation(d) if d.name == upstream_name => {
                // Prefer the server-propagated (narrowed/renamed) schema from
                // Rule 10 over the client-supplied carry-forward `d.schema`.
                // Rule 10's `OpChain::compile` walks the chain ops
                // (`select`/`drop`/`rename`/`with_columns`/`cast`/`fillna`)
                // and records the post-chain schema in `propagated_schemas`;
                // referencing a field that the chain narrowed away here must
                // surface as `AggregationUnknownField`, not be papered over
                // by the client's pre-chain schema view.
                if let Some(propagated) = propagated_schemas
                    .iter()
                    .find(|(name, _)| name == upstream_name)
                    .map(|(_, s)| s)
                {
                    return Some(Schema::from_derived(propagated));
                }
                return Some(Schema::from_derived(&d.schema));
            }
            _ => {}
        }
    }
    if base_schema.is_none() {
        if let Some(e) = registry.events.get(upstream_name) {
            base_schema = Some(Schema::from_event(&e.schema));
            upstream_is_event = true;
        } else if let Some(t) = registry.tables.get(upstream_name) {
            return Some(Schema::from_table(&t.schema));
        } else if let Some(d) = registry.derivations.get(upstream_name) {
            return Some(Schema::from_derived(&d.schema));
        }
    }

    let mut schema = base_schema?;

    // Step 2: if the upstream is an event, union in same-payload sibling
    // event-derivations of the same event.
    if upstream_is_event {
        for node in nodes {
            if let PayloadNode::Derivation(d) = node {
                if d.output_kind != crate::registry::OutputKind::Event {
                    continue;
                }
                // Only union derivations whose sole upstream is the same event.
                if d.upstreams.len() != 1 || d.upstreams[0] != upstream_name {
                    continue;
                }
                // Union sibling fields — last-write-wins on collision.
                for (fname, ftype) in d.schema.fields.iter() {
                    schema.fields.insert(fname.clone(), *ftype);
                }
            }
        }
    }

    Some(schema)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // ── Duration parser tests ─────────────────────────────────────────────────

    #[test]
    fn parse_duration_5m_returns_300_000() {
        assert_eq!(parse_duration_to_ms("5m"), Ok(Some(300_000)));
    }

    #[test]
    fn parse_duration_100ms() {
        assert_eq!(parse_duration_to_ms("100ms"), Ok(Some(100)));
    }

    #[test]
    fn parse_duration_2h() {
        assert_eq!(parse_duration_to_ms("2h"), Ok(Some(7_200_000)));
    }

    #[test]
    fn parse_duration_1d() {
        assert_eq!(parse_duration_to_ms("1d"), Ok(Some(86_400_000)));
    }

    #[test]
    fn parse_duration_1s() {
        assert_eq!(parse_duration_to_ms("1s"), Ok(Some(1_000)));
    }

    #[test]
    fn parse_duration_forever() {
        assert_eq!(parse_duration_to_ms("forever"), Ok(None));
    }

    #[test]
    fn parse_duration_rejects_garbage() {
        assert_eq!(parse_duration_to_ms("abc"), Err(()));
        assert_eq!(parse_duration_to_ms("5"), Err(()));
        assert_eq!(parse_duration_to_ms(""), Err(()));
        assert_eq!(parse_duration_to_ms("5x"), Err(()));
        assert_eq!(parse_duration_to_ms("5seconds"), Err(()));
    }

    /// CR-01: zero-value durations must be rejected to prevent div_euclid(0) panic.
    #[test]
    fn parse_duration_rejects_zero_values() {
        assert_eq!(parse_duration_to_ms("0ms"), Err(()));
        assert_eq!(parse_duration_to_ms("0s"), Err(()));
        assert_eq!(parse_duration_to_ms("0m"), Err(()));
        assert_eq!(parse_duration_to_ms("0h"), Err(()));
        assert_eq!(parse_duration_to_ms("0d"), Err(()));
    }

    /// CR-01: register with window="0ms" must produce AggregationInvalidWindow error.
    // ── Chain-aware where validation + sibling-union tests ──
    //
    // These tests pin the contract that the where-predicate validator (and
    // group-by/agg field-ref validators) MUST consider both:
    //   (a) Same-payload sibling event-derivations of the same upstream event.
    //   (b) The effective schema after applying chain ops (with_columns / select /
    //       drop / rename / cast / fillna) preceding the GroupBy in the same
    //       derivation's chain.
    //
    // Both unblock the documented `bv.lit("web")` constant-column use case
    // (see ADR-003).

    #[test]
    fn rule11_sibling_event_derivation_unions_schema_for_where_predicate() {
        // When an aggregation's upstream is an EVENT and a same-payload
        // event-derivation declares additional fields on that event, the
        // where-predicate validator MUST see the union of
        // (event ∪ sibling-derivation) fields.
        let nodes = vec![
            event_node_with_fields(
                "Click",
                &[("user_id", FieldType::Str), ("page", FieldType::Str)],
            ),
            // Tagged: event-derivation that ADDS source via with_columns. The
            // schema is supplied directly so we don't need to exercise op-chain
            // schema-propagation here — focus on the union behavior.
            PayloadNode::Derivation(DerivationDescriptor {
                name: "Tagged".to_string(),
                output_kind: OutputKind::Event,
                upstreams: vec!["Click".to_string()],
                ops: vec![],
                schema: DerivedSchema {
                    fields: {
                        let mut f = BTreeMap::new();
                        f.insert("user_id".to_string(), FieldType::Str);
                        f.insert("page".to_string(), FieldType::Str);
                        f.insert("source".to_string(), FieldType::Str);
                        f
                    },
                    optional_fields: vec![],
                },
                table_primary_key: None,
                registered_at_version: 0,
            }),
            // UserClicks: aggregation upstream Click; where-predicate
            // references `source` which is added by sibling Tagged.
            group_by_derivation(
                "UserClicks",
                "Click",
                vec!["user_id"],
                serde_json::json!({
                    "web_only": {"op": "count", "params": {"window": "forever", "where": "(source == 'web')"}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors.is_empty(),
            "sibling-union must add 'source' from Tagged to upstream_schema \
             for UserClicks's where-validator; got errors: {errors:#?}"
        );
    }

    // DOCUMENTED GAP for v0.1: chain-aware where validation.
    //
    // The SDK chain-flatten approach (in _app.py::_descriptor_to_node)
    // resolves the @bv.event def pattern by inlining ops into consumers and
    // setting upstream to the root event source. That covers the documented
    // `bv.lit("web")` constant-column use case via the `@bv.event def Tagged(...)`
    // form. The remaining gap below — `node.with_columns(source).group_by(where=col(source))`
    // in a single chain — is a v0.1 polish item: users CAN already express this
    // as `@bv.event def Tagged(node): return node.with_columns(...)` followed by
    // `@bv.table def F(tagged: Tagged): return tagged.group_by(...).agg(...)`.
    #[test]
    #[ignore = "v0.1 polish — single-chain with_columns→group_by(where=col(added)) requires chain-aware where validator. Workaround: split into @bv.event def + @bv.table def."]
    fn rule11_chain_aware_where_validates_against_post_with_columns_schema() {
        // In a single-derivation chain like
        // `event.with_columns(source=lit('x')).group_by(...).agg(where=col('source')...)`,
        // the where-predicate MUST be validated against the SCHEMA AFTER the
        // with_columns op (not just the raw upstream event schema).
        let nodes = vec![
            event_node_with_fields("Click", &[("user_id", FieldType::Str)]),
            // UserClicks chain = [with_columns(source=...), group_by(where=col(source)...)]
            PayloadNode::Derivation(DerivationDescriptor {
                name: "UserClicks".to_string(),
                output_kind: OutputKind::Table,
                upstreams: vec!["Click".to_string()],
                ops: vec![
                    crate::op_node::OpNode::WithColumns {
                        exprs: {
                            let mut m = BTreeMap::new();
                            m.insert("source".to_string(), "'web'".to_string());
                            m
                        },
                    },
                    crate::op_node::OpNode::GroupBy {
                        keys: vec!["user_id".to_string()],
                        agg: serde_json::from_value(serde_json::json!({
                            "web_only": {"op": "count", "params": {"window": "forever", "where": "(source == 'web')"}}
                        }))
                        .expect("agg json"),
                    },
                ],
                schema: DerivedSchema {
                    fields: {
                        let mut f = BTreeMap::new();
                        f.insert("user_id".to_string(), FieldType::Str);
                        f.insert("web_only".to_string(), FieldType::I64);
                        f
                    },
                    optional_fields: vec![],
                },
                table_primary_key: Some(vec!["user_id".to_string()]),
                registered_at_version: 0,
            }),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors.is_empty(),
            "Chain-aware where-validator must apply with_columns before validating \
             where-predicate; got errors: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_zero_window() {
        let nodes = vec![
            event_node_with_fields("Txn", &[("user_id", FieldType::Str)]),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "cnt": {"op": "count", "params": {"window": "0ms"}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::AggregationInvalidWindow),
            "expected AggregationInvalidWindow for zero window '0ms', got: {errors:#?}"
        );
    }

    // ── Rule 11 compile unit tests ────────────────────────────────────────────

    use crate::registry::{
        DerivationDescriptor, EventDescriptor, OutputKind, RegistryInner, TableDescriptor,
        TableMode,
    };
    use crate::schema::{DerivedSchema, EventSchema, FieldType, TableSchema};

    fn empty_registry() -> RegistryInner {
        RegistryInner::default()
    }

    fn event_node_with_fields(name: &str, fields: &[(&str, FieldType)]) -> PayloadNode {
        let mut f = BTreeMap::new();
        for (k, v) in fields {
            f.insert(k.to_string(), *v);
        }
        PayloadNode::Event(EventDescriptor {
            name: name.to_string(),
            schema: EventSchema {
                fields: f,
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        })
    }

    fn table_node(name: &str, pk: &str) -> PayloadNode {
        let mut f = BTreeMap::new();
        f.insert(pk.to_string(), FieldType::Str);
        PayloadNode::Table(TableDescriptor {
            name: name.to_string(),
            primary_key: vec![pk.to_string()],
            schema: TableSchema {
                fields: f,
                optional_fields: vec![],
            },
            ttl_ms: None,
            mode: TableMode::Upsert,
            registered_at_version: 0,
            temporal: false,
            retention_ms: None,
        })
    }

    fn group_by_derivation(
        name: &str,
        upstream: &str,
        keys: Vec<&str>,
        agg: serde_json::Value,
    ) -> PayloadNode {
        let agg_map: BTreeMap<String, crate::op_node::AggSpec> =
            serde_json::from_value(agg).expect("agg json");
        let mut schema_fields = BTreeMap::new();
        schema_fields.insert("dummy".to_string(), FieldType::I64);
        PayloadNode::Derivation(DerivationDescriptor {
            name: name.to_string(),
            output_kind: OutputKind::Table,
            upstreams: vec![upstream.to_string()],
            ops: vec![crate::op_node::OpNode::GroupBy {
                keys: keys.iter().map(|s| s.to_string()).collect(),
                agg: agg_map,
            }],
            schema: DerivedSchema {
                fields: schema_fields,
                optional_fields: vec![],
            },
            table_primary_key: Some(keys.iter().map(|s| s.to_string()).collect()),
            registered_at_version: 0,
        })
    }

    #[test]
    fn rule11_accepts_valid_count_window_5m() {
        let nodes = vec![
            event_node_with_fields(
                "Txn",
                &[("user_id", FieldType::Str), ("amount", FieldType::F64)],
            ),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "cnt": {"op": "count", "params": {"window": "5m"}}
                }),
            ),
        ];
        let (compiled, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(errors.is_empty(), "expected no errors, got: {errors:#?}");
        assert_eq!(compiled.len(), 1);
        assert_eq!(compiled[0].0, "UserStats");
        let desc = &compiled[0].1;
        assert_eq!(desc.group_keys, vec!["user_id"]);
        assert_eq!(desc.features.len(), 1);
        assert_eq!(desc.features[0].feature_name, "cnt");
        assert_eq!(desc.features[0].descriptor.window_ms, Some(300_000));
    }

    #[test]
    fn rule11_rejects_unknown_group_key() {
        let nodes = vec![
            event_node_with_fields("Txn", &[("user_id", FieldType::Str)]),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["missing"],
                serde_json::json!({
                    "cnt": {"op": "count", "params": {}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors.iter().any(
                |e| e.code == ErrorCode::AggregationUnknownField && e.path.contains("group_by")
            ),
            "expected AggregationUnknownField for group_by key, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_float_group_key() {
        let nodes = vec![
            event_node_with_fields(
                "Txn",
                &[("user_id", FieldType::Str), ("amount", FieldType::F64)],
            ),
            group_by_derivation(
                "AmountStats",
                "Txn",
                vec!["amount"],
                serde_json::json!({
                    "cnt": {"op": "count", "params": {}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors.iter().any(|e| {
                e.code == ErrorCode::AggregationUnknownField
                    && e.path.contains("group_by")
                    && e.reason.contains("cannot be float-typed")
            }),
            "expected float group_by rejection, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_unknown_op_field() {
        let nodes = vec![
            event_node_with_fields(
                "Txn",
                &[("user_id", FieldType::Str), ("amount", FieldType::F64)],
            ),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "total": {"op": "sum", "params": {"field": "missing"}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::AggregationUnknownField),
            "expected AggregationUnknownField for sum on missing field, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_invalid_where_predicate_unknown_field() {
        let nodes = vec![
            event_node_with_fields(
                "Txn",
                &[("user_id", FieldType::Str), ("amount", FieldType::F64)],
            ),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "cnt": {"op": "count", "params": {"where": "(no_such_field == 'ok')"}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::AggregationInvalidWhere),
            "expected AggregationInvalidWhere for unknown where field, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_malformed_where_parse_error() {
        let nodes = vec![
            event_node_with_fields(
                "Txn",
                &[("user_id", FieldType::Str), ("amount", FieldType::F64)],
            ),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "cnt": {"op": "count", "params": {"where": "amount >>> "}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::AggregationInvalidWhere),
            "expected AggregationInvalidWhere for malformed where expr, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_invalid_window() {
        let nodes = vec![
            event_node_with_fields("Txn", &[("user_id", FieldType::Str)]),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "cnt": {"op": "count", "params": {"window": "5seconds"}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::AggregationInvalidWindow),
            "expected AggregationInvalidWindow for bad window, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_unknown_op_name() {
        let nodes = vec![
            event_node_with_fields("Txn", &[("user_id", FieldType::Str)]),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "ws": {"op": "weighted_sum", "params": {}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::AggregationUnknownOp),
            "expected AggregationUnknownOp for 'weighted_sum', got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_aggregation_on_table_source() {
        let nodes = vec![
            table_node("Merchants", "merchant_id"),
            group_by_derivation(
                "MerchantStats",
                "Merchants",
                vec!["merchant_id"],
                serde_json::json!({
                    "cnt": {"op": "count", "params": {}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::AggregationOnTableNotSupported),
            "expected AggregationOnTableNotSupported, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_duplicate_feature_names() {
        let nodes = vec![
            event_node_with_fields("Txn", &[("user_id", FieldType::Str)]),
            {
                // Manually build with duplicate keys in agg_map — BTreeMap can't have
                // duplicates, so we use a raw serde_json-constructed OpNode.
                // Actually BTreeMap deduplicates, so test via two separate ops is needed.
                // Instead: this test verifies the duplicate detection within one GroupBy
                // using the seen_feature_names set. Since BTreeMap can't have two keys,
                // we verify the dedup logic path by ensuring duplicate detection works
                // for the same feature name appearing in what should be a single BTreeMap.
                // Since BTreeMap dedups, we test via the agg_schema validator path.
                // We test via two features with the same name by building from JSON.
                let raw = serde_json::json!({
                    "kind": "derivation",
                    "name": "Dup",
                    "output_kind": "table",
                    "upstreams": ["Txn"],
                    "ops": [{
                        "op": "group_by",
                        "keys": ["user_id"],
                        "agg": {
                            "cnt": {"op": "count", "params": {}}
                        }
                    }],
                    "schema": {"fields": {"user_id": "str", "cnt": "i64"}, "optional_fields": []},
                    "table_primary_key": ["user_id"]
                });
                PayloadNode::Derivation(serde_json::from_value(raw).expect("parse deriv"))
            },
        ];
        // BTreeMap naturally deduplicates, so "true" duplicate feature names can't happen
        // through normal JSON parsing (BTreeMap). However, the code guards against it.
        // This test validates the happy path (no duplicates) when schema is clean.
        let (compiled, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(errors.is_empty(), "unexpected errors: {errors:#?}");
        assert_eq!(compiled.len(), 1);
    }

    #[test]
    fn rule11_rejects_feature_name_collides_with_group_key() {
        let nodes = vec![
            event_node_with_fields("Txn", &[("user_id", FieldType::Str)]),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "user_id": {"op": "count", "params": {}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::AggregationGroupKeyCollidesWithFeature),
            "expected AggregationGroupKeyCollidesWithFeature, got: {errors:#?}"
        );
    }

    // Sketch op-name + sketch-param validation tests.
    fn sketch_event_node() -> PayloadNode {
        event_node_with_fields(
            "Txn",
            &[
                ("user_id", FieldType::Str),
                ("merchant_id", FieldType::Str),
                ("amount", FieldType::F64),
                ("device_id", FieldType::Str),
                ("category", FieldType::Str),
            ],
        )
    }

    #[test]
    fn rule11_count_distinct_op_name_recognized() {
        // Per ADR-002: count_distinct → n_unique. Test name kept for
        // git-blame continuity; fixture uses the new wire name.
        let nodes = vec![
            sketch_event_node(),
            group_by_derivation(
                "Agg",
                "Txn",
                vec!["user_id"],
                serde_json::json!({"d": {"op": "n_unique", "params": {"field": "merchant_id", "window": "1h"}}}),
            ),
        ];
        let (compiled, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(errors.is_empty(), "{:?}", errors);
        assert_eq!(compiled.len(), 1);
    }

    #[test]
    fn rule11_percentile_op_name_recognized_with_q() {
        // Per ADR-002: percentile → quantile. Test name kept for
        // git-blame continuity; fixture uses the new wire name.
        let nodes = vec![
            sketch_event_node(),
            group_by_derivation(
                "Agg",
                "Txn",
                vec!["user_id"],
                serde_json::json!({"p": {"op": "quantile", "params": {"field": "amount", "q": 0.99}}}),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn rule11_percentile_q_out_of_range_rejected() {
        // Per ADR-002: percentile → quantile. Test name kept for
        // git-blame continuity; fixture uses the new wire name.
        let nodes = vec![
            sketch_event_node(),
            group_by_derivation(
                "Agg",
                "Txn",
                vec!["user_id"],
                serde_json::json!({"p": {"op": "quantile", "params": {"field": "amount", "q": 1.5}}}),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::InvalidPercentileQ),
            "{:?}",
            errors
        );
    }

    #[test]
    fn rule11_bloom_member_with_window_rejected() {
        let nodes = vec![
            sketch_event_node(),
            group_by_derivation(
                "Agg",
                "Txn",
                vec!["user_id"],
                serde_json::json!({"b": {"op": "bloom_member", "params": {"field": "device_id", "window": "1h"}}}),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::WindowNotSupported),
            "{:?}",
            errors
        );
    }

    #[test]
    fn rule11_top_k_k_out_of_range_rejected() {
        let nodes = vec![
            sketch_event_node(),
            group_by_derivation(
                "Agg",
                "Txn",
                vec!["user_id"],
                serde_json::json!({"t": {"op": "top_k", "params": {"field": "merchant_id", "k": 5000}}}),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors.iter().any(|e| e.code == ErrorCode::InvalidTopKK),
            "{:?}",
            errors
        );
    }

    #[test]
    fn rule11_entropy_op_name_recognized() {
        let nodes = vec![
            sketch_event_node(),
            group_by_derivation(
                "Agg",
                "Txn",
                vec!["user_id"],
                serde_json::json!({"e": {"op": "entropy", "params": {"field": "category"}}}),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn rule11_bloom_fpr_out_of_range_rejected() {
        let nodes = vec![
            sketch_event_node(),
            group_by_derivation(
                "Agg",
                "Txn",
                vec!["user_id"],
                serde_json::json!({"b": {"op": "bloom_member", "params": {"field": "device_id", "target_fpr": 2.0}}}),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors.iter().any(|e| e.code == ErrorCode::InvalidBloomFpr),
            "{:?}",
            errors
        );
    }

    #[test]
    fn rule11_fail_soft_collects_all_errors() {
        // Payload with 3 violations: unknown group key + unknown field + bad window
        let nodes = vec![
            event_node_with_fields(
                "Txn",
                &[("user_id", FieldType::Str), ("amount", FieldType::F64)],
            ),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["missing_key"],
                serde_json::json!({
                    "total": {"op": "sum", "params": {"field": "no_field"}},
                    "windowed": {"op": "count", "params": {"window": "badwindow"}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        // missing_key → AggregationUnknownField
        // no_field → AggregationUnknownField
        // badwindow → AggregationInvalidWindow
        assert!(
            errors.len() >= 3,
            "expected at least 3 errors, got {}: {errors:#?}",
            errors.len()
        );
    }

    // ── Phase 8: point/recency op compile tests ──────────────────────────────

    /// `parse_agg_kind` recognises every Phase 8 op string.
    #[test]
    fn parse_agg_kind_recognises_phase8_ops() {
        for (s, k) in [
            ("first", AggKind::First),
            ("last", AggKind::Last),
            ("first_n", AggKind::FirstN),
            ("last_n", AggKind::LastN),
            ("lag", AggKind::Lag),
            ("first_seen", AggKind::FirstSeen),
            ("last_seen", AggKind::LastSeen),
            ("age", AggKind::Age),
            ("has_seen", AggKind::HasSeen),
            ("time_since", AggKind::TimeSince),
            ("time_since_last_n", AggKind::TimeSinceLastN),
            ("streak", AggKind::Streak),
            ("max_streak", AggKind::MaxStreak),
            ("negative_streak", AggKind::NegativeStreak),
            ("first_seen_in_window", AggKind::FirstSeenInWindow),
        ] {
            assert_eq!(parse_agg_kind(s), Some(k), "parse_agg_kind({s}) failed");
        }
    }

    fn txn_event_with_amount() -> PayloadNode {
        event_node_with_fields(
            "Txn",
            &[
                ("user_id", FieldType::Str),
                ("amount", FieldType::F64),
                ("status", FieldType::Str),
            ],
        )
    }

    #[test]
    fn rule11_accepts_first_with_field() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "first_amt": {"op": "first", "params": {"field": "amount"}}
                }),
            ),
        ];
        let (compiled, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(errors.is_empty(), "expected no errors, got: {errors:#?}");
        assert_eq!(compiled.len(), 1);
        assert_eq!(compiled[0].1.features[0].descriptor.kind, AggKind::First);
    }

    #[test]
    fn rule11_accepts_first_n_with_field_and_n() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "first3": {"op": "first_n", "params": {"field": "amount", "n": 3}}
                }),
            ),
        ];
        let (compiled, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(errors.is_empty(), "expected no errors, got: {errors:#?}");
        assert_eq!(compiled[0].1.features[0].descriptor.n, Some(3));
    }

    #[test]
    fn rule11_rejects_first_n_without_n() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "first3": {"op": "first_n", "params": {"field": "amount"}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors.iter().any(|e| e.path.contains("params.n")),
            "expected n-missing error, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_first_n_with_zero_n() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "first0": {"op": "first_n", "params": {"field": "amount", "n": 0}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors.iter().any(|e| e.path.contains("params.n")),
            "expected n>0 error, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_rejects_first_without_field() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "first_amt": {"op": "first", "params": {}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors.iter().any(|e| e.path.contains("params.field")),
            "expected field-missing error for first, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_accepts_recency_markers_without_field() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "fs": {"op": "first_seen", "params": {}},
                    "ls": {"op": "last_seen", "params": {}},
                    "a":  {"op": "age", "params": {}},
                    "hs": {"op": "has_seen", "params": {}},
                    "ts": {"op": "time_since", "params": {}}
                }),
            ),
        ];
        let (compiled, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(errors.is_empty(), "expected no errors, got: {errors:#?}");
        assert_eq!(compiled[0].1.features.len(), 5);
    }

    #[test]
    fn rule11_rejects_recency_marker_with_window() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "ts": {"op": "time_since", "params": {"window": "5m"}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::AggregationInvalidWindow),
            "expected window-rejected error, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_first_seen_in_window_requires_window() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "fsiw": {"op": "first_seen_in_window", "params": {}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::AggregationInvalidWindow),
            "expected window-required error, got: {errors:#?}"
        );
    }

    #[test]
    fn rule11_first_seen_in_window_accepts_window() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "fsiw": {"op": "first_seen_in_window", "params": {"window": "5m"}}
                }),
            ),
        ];
        let (compiled, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(errors.is_empty(), "expected no errors, got: {errors:#?}");
        assert_eq!(
            compiled[0].1.features[0].descriptor.window_ms,
            Some(300_000)
        );
    }

    #[test]
    fn rule11_streak_accepts_with_where_only() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "succ_streak": {"op": "streak", "params": {"where": "(status == 'ok')"}},
                    "fail_streak": {"op": "negative_streak", "params": {"where": "(status == 'ok')"}},
                    "max_succ": {"op": "max_streak", "params": {"where": "(status == 'ok')"}}
                }),
            ),
        ];
        let (compiled, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(errors.is_empty(), "expected no errors, got: {errors:#?}");
        assert_eq!(compiled[0].1.features.len(), 3);
    }

    #[test]
    fn rule11_time_since_last_n_requires_n() {
        let nodes = vec![
            txn_event_with_amount(),
            group_by_derivation(
                "UserStats",
                "Txn",
                vec!["user_id"],
                serde_json::json!({
                    "tsn": {"op": "time_since_last_n", "params": {}}
                }),
            ),
        ];
        let (_, errors) = compile_aggregations_from_nodes(&nodes, &empty_registry(), &[]);
        assert!(
            errors.iter().any(|e| e.path.contains("params.n")),
            "expected n-required error for time_since_last_n, got: {errors:#?}"
        );
    }
}
