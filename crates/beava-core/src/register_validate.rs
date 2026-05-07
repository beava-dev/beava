//! Pre-diff validation pass for `POST /register` payloads.
//!
//! Validates all 9 rules from 02-CONTEXT.md §Validation pass:
//! 1. Node uniqueness within payload
//! 2. Reserved names / pattern / length
//! 3. Event schema: non-empty; if event_time_field is Some, it must exist and be I64.
//!    If event_time_field is None, the server will stamp wall-clock time on push.
//! 4. Table schema: primary_key ≥ 1 and ≤ 4 fields, all in schema
//! 5. Derivation upstreams: each name resolves in payload OR current registry
//! 6. Derivation schema non-empty; output_kind=Table requires table_primary_key
//! 7. DAG acyclicity (DFS, reports first cycle)
//! 8. Topological order (upstreams-within-payload appear before dependents)
//! 9. Dedupe key: if present, must be in schema; dedupe_window_ms must be positive

use crate::op_chain::OpChain;
use crate::registry::{EventDescriptor, RegistryInner, TableDescriptor};
use crate::registry_diff::PayloadNode;
use crate::schema::{validate_descriptor_name, DescriptorNameError};
use crate::schema_propagate::{PropagationError, Schema};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ─── Public types ─────────────────────────────────────────────────────────────

/// Machine-readable error code for each validation rule violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRegistration,
    NameDuplicate,
    NameReservedPrefix,
    NameBadPattern,
    NameEmpty,
    NameTooLong,
    /// Phase 12.6 / Phase 12.7 events-only pivot, pre-pivot variant — emitted
    /// when the `event_time_field` decorator referenced an unknown schema
    /// field. The validator no longer raises this; stale fixtures get rejected
    /// at the JSON-prelude layer (`pre_check_legacy_event_time_keys`) before
    /// the validator runs. Variant kept for wire-codec stability and to avoid
    /// an `ErrorCode` discriminant shift.
    // reason: wire-codec stability — discriminant retained per Phase 12.7
    // events-only pivot; never constructed at runtime.
    #[allow(dead_code)]
    EventTimeFieldMissing,
    /// Pre-pivot variant — emitted when the `event_time_field` decorator
    /// referenced a field of the wrong type (non-i64). Same posture as
    /// `EventTimeFieldMissing` — variant kept, no longer raised.
    // reason: wire-codec stability — see `EventTimeFieldMissing` above.
    #[allow(dead_code)]
    EventTimeFieldWrongType,
    EventSchemaEmpty,
    TablePrimaryKeyEmpty,
    TablePrimaryKeyTooLong,
    TablePrimaryKeyUnknownField,
    DerivationUpstreamUnknown,
    DerivationSchemaEmpty,
    RegistrationCycle,
    TopologicalOrderViolation,
    DedupeKeyUnknownField,
    DedupeWindowNonPositive,
    DerivationOutputKindTableMissingPrimaryKey,
    // Rule 10: expression / schema-propagation validation.
    /// Parse error in a Filter / WithColumns / Map expression.
    InvalidExpression,
    /// Expression references a field not in the upstream schema at that op step.
    UnknownFieldReference,
    /// Type mismatch / rename collision / propagation failure from schema_propagate.
    SchemaPropagationFailure,
    /// Cast target type string is not one of {"str","int","float","bool"} or the
    /// source→target pair is illegal (e.g., Bytes cannot be cast).
    InvalidCastTarget,
    /// GroupBy / Join / Union appearing in an op chain — treated as
    /// pass-through (not an error); variant exists for future use but
    /// Rule 10 does NOT emit this — it treats them as warnings.
    // reason: variant reserved for future Rule 10 strictening; never
    // constructed at runtime. ErrorCode discriminant pinned by wire-codec.
    #[allow(dead_code)]
    UnsupportedOpInPhase4,
    // Rule 11: aggregation validation.
    /// Aggregation source is a Table — not supported in v0 (SDK-AGG-05).
    AggregationOnTableNotSupported,
    /// group_by key or op.field references a field not in the upstream schema.
    AggregationUnknownField,
    /// where predicate parse error or references an unknown field.
    AggregationInvalidWhere,
    /// window duration string does not match `\d+(ms|s|m|h|d)` or `forever`.
    AggregationInvalidWindow,
    /// aggregation op string is not in the hardcoded whitelist.
    AggregationUnknownOp,
    /// Two features within one GroupBy share the same name.
    /// NOTE: BTreeMap deserialization deduplicates JSON keys (last-writer-wins),
    /// so this variant is currently unreachable via normal JSON parsing.
    /// Reserved for future Vec-based deserialization that preserves duplicates.
    // reason: variant reserved for future Vec-based deserialization that
    // preserves duplicate JSON keys; never constructed at runtime today.
    #[allow(dead_code)]
    AggregationDuplicateFeatureName,
    /// A feature name collides with a group_by key.
    AggregationGroupKeyCollidesWithFeature,
    /// Two different aggregation nodes expose the same feature name —
    /// e.g., AggA exposes "cnt" AND AggB exposes "cnt" → reject at register time.
    AggregationFeatureNameCollisionAcrossAggregations,
    // Decay / velocity ops.
    /// Decay op (`ewma`, `ewvar`, `ew_zscore`, `decayed_sum`, `decayed_count`)
    /// missing `params.half_life` or value unparseable / non-positive / `"forever"`.
    AggregationInvalidHalfLife,
    /// `burst_count` missing `params.sub_window` or value unparseable / non-positive.
    AggregationInvalidSubWindow,
    // Sketch op validation.
    /// bloom_member used with `window=` kwarg → rejected (windowless-only).
    WindowNotSupported,
    /// percentile.q out of (0.0, 1.0).
    InvalidPercentileQ,
    /// top_k.k out of (0, 1024].
    InvalidTopKK,
    /// bloom_member.fpr out of (0.0, 1.0).
    InvalidBloomFpr,
}

/// A single structured validation error. `path` uses pseudo-JSON-pointer format
/// (e.g., `"nodes[2].upstreams[0]"`). `reason` is human-readable.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidationError {
    pub code: ErrorCode,
    pub path: String,
    pub reason: String,
}

// ─── Removed-op JSON-prelude shim ─────────────────────────────────────────────

/// Structured "feature removed" error emitted by `pre_check_removed_ops`.
///
/// This is distinct from `ValidationError` because the wire `code` is a fixed
/// string (`feature_removed_no_joins_v0` / `feature_removed_no_unions_v0`) per
/// CONTEXT.md §Implementation Decisions / Bucket 5, NOT one of the standard
/// `ErrorCode` variants. The dispatch site (apply_shard / runtime_core_glue /
/// post_register) maps this directly into a `{"error": {"code", "path",
/// "reason"}}` HTTP 400 body, bypassing the normal `RegisterOutcome` flow.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeatureRemovedError {
    pub code: &'static str,
    pub op_label: &'static str,
    pub path: String,
    pub reason: String,
}

/// Walks the request JSON looking for op nodes whose `"op"` field names a
/// permanently-removed feature (`"join"` or `"union"`). Returns `Some(_)` on
/// the first hit; `None` if the payload is clean.
///
/// Runs at the JSON layer BEFORE strict `RegisterPayload` deserialize so the
/// rejection works whether or not the corresponding `OpNode` variants still
/// exist in the enum. With `OpNode::Join` / `OpNode::Union` deleted, serde
/// would otherwise return "unknown variant `join`" and
/// the structured error code would be lost — this shim preserves it.
///
/// Architectural commitment per `project_redis_shaped_no_event_time_ever`
/// (locked 2026-04-30): joins and unions are removed from v0 permanently.
/// Reviving either requires explicit user override + a new ADR.
pub fn pre_check_removed_ops(body: &serde_json::Value) -> Option<FeatureRemovedError> {
    // Walk `body.nodes[*]` looking for derivation nodes with an `ops` array.
    let nodes = body.get("nodes")?.as_array()?;
    for (node_idx, node) in nodes.iter().enumerate() {
        // Only derivation nodes carry ops.
        let kind = node.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind != "derivation" {
            continue;
        }
        let ops = match node.get("ops").and_then(|v| v.as_array()) {
            Some(o) => o,
            None => continue,
        };
        let deriv_name = node.get("name").and_then(|v| v.as_str()).unwrap_or("");
        for (op_idx, op_value) in ops.iter().enumerate() {
            let op_str = op_value.get("op").and_then(|v| v.as_str()).unwrap_or("");
            let path_prefix = if deriv_name.is_empty() {
                format!("nodes[{node_idx}].ops[{op_idx}]")
            } else {
                format!("nodes[{node_idx}].{deriv_name}.ops[{op_idx}]")
            };
            match op_str {
                "join" => {
                    return Some(FeatureRemovedError {
                        code: "feature_removed_no_joins_v0",
                        op_label: "join",
                        path: path_prefix,
                        reason: "Joins were permanently removed from v0 in the \
                                 2026-04-30 architectural pivot to a Redis-shaped, \
                                 processing-time-only feature server. There is no \
                                 deprecation period and no compat shim — push events \
                                 to each stream independently and compose features \
                                 client-side instead. See \
                                 .planning/phases/12.6-v0-surface-reduction/ for \
                                 context."
                            .to_string(),
                    });
                }
                "union" => {
                    return Some(FeatureRemovedError {
                        code: "feature_removed_no_unions_v0",
                        op_label: "union",
                        path: path_prefix,
                        reason: "Unions were permanently removed from v0 in the \
                                 2026-04-30 architectural pivot. There is no \
                                 deprecation period and no compat shim. See \
                                 .planning/phases/12.6-v0-surface-reduction/ for \
                                 context."
                            .to_string(),
                    });
                }
                _ => {}
            }
        }
    }
    None
}

// ─── Legacy event-time JSON-key strict-deny shim (Phase 12.6 Plan 06) ─────────

/// Walks the request JSON looking for legacy `event_time_field` /
/// `tolerate_delay_ms` keys on any payload node. Returns `Some(_)` on the
/// first hit; `None` if the payload is clean.
///
/// Runs at the JSON layer BEFORE strict `RegisterPayload` deserialize so the
/// rejection is independent of whether the corresponding `EventDescriptor`
/// fields still exist.  Once Plan 06 deletes
/// `EventDescriptor.event_time_field` / `EventDescriptor.tolerate_delay_ms`,
/// serde would otherwise either silently strip the keys (no-op) or surface a
/// generic "unknown field" message — neither matches the D-03 contract that
/// stale fixtures get a structured error code.
///
/// **D-03 verbatim:** "Hard rip everywhere — zero `event_time_ms` compat at any
/// layer. … No deprecation window, no parse-and-strip, no warn-then-error."
/// Silent-strip is parse-and-strip — explicitly forbidden.
///
/// Architectural commitment per `project_redis_shaped_no_event_time_ever`
/// (locked 2026-04-30): event_time / watermarks / joins / PIT removed from v0
/// permanently. Reviving any of these requires explicit user override + a new
/// ADR.
pub fn pre_check_legacy_event_time_keys(body: &serde_json::Value) -> Option<FeatureRemovedError> {
    let nodes = body.get("nodes")?.as_array()?;
    for (node_idx, node) in nodes.iter().enumerate() {
        // Path prefix uses the node `name` if present, otherwise just the index.
        let node_name = node.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let path_prefix = if node_name.is_empty() {
            format!("nodes[{node_idx}]")
        } else {
            format!("nodes[{node_idx}].{node_name}")
        };
        if node.get("event_time_field").is_some() {
            return Some(FeatureRemovedError {
                code: "unknown_field_event_time_v0",
                op_label: "event_time_field",
                path: format!("{path_prefix}.event_time_field"),
                reason: "The `event_time_field` decorator key was permanently \
                         removed from v0 in the 2026-04-30 architectural pivot to \
                         a Redis-shaped, processing-time-only feature server. \
                         Windowed operators (rolling counts, decay, velocity, \
                         etc.) now bucket on **server-side wall-clock at \
                         dispatch** (`SystemTime::now()`), not on a body-derived \
                         event timestamp. There is no deprecation period and no \
                         compat shim — drop the `event_time_field` key from your \
                         registration payload. See \
                         .planning/phases/12.6-v0-surface-reduction/ for context."
                    .to_string(),
            });
        }
        if node.get("tolerate_delay_ms").is_some() {
            return Some(FeatureRemovedError {
                code: "unknown_field_tolerate_delay_v0",
                op_label: "tolerate_delay_ms",
                path: format!("{path_prefix}.tolerate_delay_ms"),
                reason: "The `tolerate_delay_ms` decorator key was permanently \
                         removed from v0 in the 2026-04-30 architectural pivot. \
                         Out-of-order tolerance was an event-time concept; \
                         post-pivot bucketing uses server arrival-time exclusively \
                         and tolerance is degenerate (the server never sees \
                         out-of-order arrivals — it timestamps at dispatch). Drop \
                         the `tolerate_delay_ms` key from your registration \
                         payload. See \
                         .planning/phases/12.6-v0-surface-reduction/ for context."
                    .to_string(),
            });
        }
    }
    None
}

// ─── Unsupported-node-kind JSON-prelude shim (Phase 12.7 Plan 01) ─────────────

/// Walks the request JSON looking for nodes whose `"kind"` field names a
/// node type not supported in v0 (anything other than `"event"` or
/// `"derivation"` — most notably `"table"`). Returns `Some(_)` on the first
/// hit; `None` if the payload is clean.
///
/// PayloadNode discriminator is `kind` per `crates/beava-core/src/registry_diff.rs`
/// (`#[serde(tag = "kind", rename_all = "snake_case")]`). Strict serde would
/// surface "unknown variant `table`" once `OpNode::Table` / `PayloadNode::Table`
/// are removed in Phase 12.7 Wave 2; this shim catches it FIRST so the wire
/// error is the structured `unsupported_node_kind` code.
///
/// Per CONTEXT.md D-02 framing ("not supported in v0", NOT "feature removed"):
/// the code is `unsupported_node_kind` (forward-looking) not a retrospective
/// "feature removed" code. v0 is the FIRST public release; users never knew
/// tables existed in v0, so a retrospective frame would confuse fresh users.
///
/// Architectural commitment per `project_v0_events_only_scope` (locked
/// 2026-04-30): v0 ships events-only — tables, table-aggregation, and session
/// windows return in v0.1+ if/when justified by demand. Reviving any of these
/// requires explicit user override + a new ADR overturning
/// `project_v0_events_only_scope`.
pub fn pre_check_unsupported_node_kind(body: &serde_json::Value) -> Option<FeatureRemovedError> {
    let nodes = body.get("nodes")?.as_array()?;
    for (node_idx, node) in nodes.iter().enumerate() {
        let kind = node.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        // Whitelist (post-12.7): only "event" and "derivation" remain.
        // Empty kind is ignored here — strict serde catches it later with
        // "missing field `kind`".
        if kind == "event" || kind == "derivation" || kind.is_empty() {
            continue;
        }
        let node_name = node.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let path_prefix = if node_name.is_empty() {
            format!("nodes[{node_idx}].kind")
        } else {
            format!("nodes[{node_idx}].{node_name}.kind")
        };
        return Some(FeatureRemovedError {
            code: "unsupported_node_kind",
            // op_label carries the rejected kind string verbatim (e.g. "table").
            // Box::leak: per-rejection one-time leak; the kind string is bounded
            // by JSON parser's max-string-length (existing axum/mio limit).
            // Identical pattern to 12.6 Plan 04's pre_check_removed_ops on
            // attacker-controlled op strings.
            op_label: Box::leak(kind.to_string().into_boxed_str()),
            path: path_prefix,
            reason: format!(
                "Node kind `{kind}` is not supported in v0. Beava v0 ships \
                 events-only (supported kinds: \"event\", \"derivation\"). \
                 Tables, table-aggregation, and session windows return in v0.1+ \
                 if/when justified by demand. See \
                 .planning/phases/12.7-table-strip/ for context."
            ),
        });
    }
    None
}

// ─── Unbounded-lifetime-op JSON-prelude shim (Phase 12.8 Plan 01) ─────────────

/// Per-op lifetime memory-bound classification.
///
/// **Phase 12.8-01 lands this enum with a placeholder `lifetime_bound_for_op_str`
/// helper that returns `Unbounded` for every op kind. Phase 12.8-04 populates
/// the per-op classification table with real values, turning most ops into
/// `O1` / `BoundedSketch` / `BoundedByRequiredKwarg` / `BoundedByConfig`.**
///
/// Per CONTEXT.md D-03: every operator declares either an O(1) bound, a
/// bounded-sketch bound, a bound-by-required-kwarg, or a bound-by-config-with-default.
/// `Unbounded` is the catch-all for ops that have not been classified yet
/// (Plan 01 state) — the shim rejects all such ops in lifetime mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpLifetimeBound {
    /// Constant per-entity memory regardless of input — core scalar ops,
    /// recency markers + streaks, decay / velocity scalar ops.
    O1,
    /// Sketch with a fixed structural bound — HLL precision (~12 KB),
    /// DDSketch buckets (~few KB), SpaceSaving (k entries), BloomFilter
    /// (capacity bits), Entropy (max_categories cap).
    BoundedSketch,
    /// Bounded by a REQUIRED kwarg the user must supply. Variant carries
    /// the kwarg name (e.g., "n" for first_n / last_n / lag / most_recent_n /
    /// time_since_last_n; "samples" for reservoir_sample).
    BoundedByRequiredKwarg(&'static str),
    /// Bounded by a config kwarg the user MAY supply, with a sensible default
    /// applied if absent (e.g., histogram(num_buckets=256),
    /// event_type_mix(max_categories=256), seasonal_deviation).
    /// First field = kwarg name; second = default cap.
    BoundedByConfig(&'static str, usize),
    /// No declared per-entity bound — REJECTED in lifetime mode.
    Unbounded,
}

/// Classifier helper: maps an op-string from a register payload to its
/// declared lifetime bound. Phase 12.8 memory-governance: every op-string
/// returns a non-`Unbounded` variant; `Unbounded` is the catch-all for
/// typos / unknown op-strings (rejected at register-time when the
/// memory-governance gate is ON).
///
/// Op-string list mirrors `crates/beava-core/src/agg_compile.rs::parse_agg_kind`
/// (53 `AggKind` variants + the `"ema"` SDK alias for `AggKind::Ewma` =
/// 54 distinct op-strings, plus standalone `first` / `last` single-element ops).
///
/// **Bound classes:**
/// - `O1` — constant per-entity memory regardless of input.
/// - `BoundedSketch` — sketch with a fixed structural bound (HLL precision,
///   DDSketch buckets, BloomFilter capacity).
/// - `BoundedByRequiredKwarg(name)` — REQUIRES the named kwarg in lifetime
///   mode (e.g. `n` for first_n / last_n / lag / most_recent_n /
///   time_since_last_n; `samples` for reservoir_sample; `buckets` for
///   histogram).
/// - `BoundedByConfig(name, default)` — kwarg is OPTIONAL with a sensible
///   default applied if absent (top_k k=10, entropy max_categories=256,
///   event_type_mix max_categories=256, distance_from_home samples=100).
/// - `Unbounded` — unknown op (typo / unclassified) — REJECTED.
///
/// `top_k` uses `BoundedByConfig("k", 10)` (NOT `BoundedByRequiredKwarg("k")`)
/// for backward-compat with existing top_k tests that don't specify `k`
/// (`agg_compile.rs`: `sp.top_k_k.unwrap_or(10).max(1)`). Histogram has no
/// such default in the existing wire convention; it is elevated to
/// `BoundedByRequiredKwarg("buckets")`.
pub fn lifetime_bound_for_op_str(op_str: &str) -> OpLifetimeBound {
    match op_str {
        // ── Core scalar: O(1) ──────────────────────────────────────────
        // Per ADR-002: avg→mean, variance→var, stddev→std.
        "count" | "sum" | "mean" | "min" | "max" | "var" | "std" | "ratio" => OpLifetimeBound::O1,
        // ── Single-element: O(1) ───────────────────────────────────────
        "first" | "last" => OpLifetimeBound::O1,
        // ── Point/ordinal: BoundedByRequiredKwarg("n") ─────────────────
        "first_n" | "last_n" | "lag" | "time_since_last_n" => {
            OpLifetimeBound::BoundedByRequiredKwarg("n")
        }
        // ── Recency markers: O(1) ──────────────────────────────────────
        "first_seen" | "last_seen" | "age" | "has_seen" | "time_since" => OpLifetimeBound::O1,
        // ── Streaks: O(1) ──────────────────────────────────────────────
        "streak" | "max_streak" | "negative_streak" => OpLifetimeBound::O1,
        // ── Windowed recency: O(1) (carries window_ms as a lifetime
        //    parameter; storage stays constant per entity) ──────────────
        "first_seen_in_window" => OpLifetimeBound::O1,
        // ── Decay: O(1) — `ema` is an SDK alias for ewma ───────────────
        "ewma" | "ema" | "ewvar" | "ew_zscore" | "decayed_sum" | "decayed_count" | "twa" => {
            OpLifetimeBound::O1
        }
        // ── Velocity: O(1) ─────────────────────────────────────────────
        "rate_of_change"
        | "inter_arrival_stats"
        | "burst_count"
        | "delta_from_prev"
        | "trend"
        | "trend_residual"
        | "outlier_count"
        | "value_change_count" => OpLifetimeBound::O1,
        // ── Entity z-score: O(1) ───────────────────────────────────────
        "z_score" => OpLifetimeBound::O1,
        // ── Sketches: BoundedSketch (fixed structural cap) ─────────────
        // Per ADR-002: count_distinct→n_unique, percentile→quantile.
        "n_unique" | "quantile" | "bloom_member" => OpLifetimeBound::BoundedSketch,
        // ── Entropy: BoundedByConfig("max_categories", 256) ────────────
        "entropy" => OpLifetimeBound::BoundedByConfig("max_categories", 256),
        // ── top_k: BoundedByConfig("k", 10) ────────────────────────────
        // Soft default keeps backward compat with ~10 existing top_k tests
        // that don't specify k. The shim treats top_k as having a finite
        // bound (10) without requiring the user to spell it out.
        "top_k" => OpLifetimeBound::BoundedByConfig("k", 10),
        // ── Buffer ops: histogram is HARD-required ─────────────────────
        // Wire convention is `params.buckets: Vec<f64>`; this is a
        // register-time requirement. Empty / missing buckets array →
        // reject with cap-kwarg suggestion.
        "histogram" => OpLifetimeBound::BoundedByRequiredKwarg("buckets"),
        // ── Fixed-size histograms: O(1) ────────────────────────────────
        // 24 buckets (hour_of_day) and 24×24 = 576 buckets (dow_hour) and
        // 24 hourly slots (seasonal_deviation) are structural caps; no
        // user-supplied kwarg required.
        "hour_of_day_histogram" => OpLifetimeBound::O1,
        "dow_hour_histogram" => OpLifetimeBound::O1,
        "seasonal_deviation" => OpLifetimeBound::O1,
        // ── event_type_mix: BoundedByConfig("max_categories", 256) ─────
        "event_type_mix" => OpLifetimeBound::BoundedByConfig("max_categories", 256),
        // ── most_recent_n: BoundedByRequiredKwarg("n") ─────────────────
        "most_recent_n" => OpLifetimeBound::BoundedByRequiredKwarg("n"),
        // ── reservoir_sample: BoundedByRequiredKwarg("samples") ────────
        "reservoir_sample" => OpLifetimeBound::BoundedByRequiredKwarg("samples"),
        // ── Geo: O(1) ──────────────────────────────────────────────────
        "geo_velocity" | "geo_distance" | "geo_spread" => OpLifetimeBound::O1,
        // ── distance_from_home: BoundedByConfig("samples", 100) ────────
        // (Carries a `samples` kwarg with default 100.)
        "distance_from_home" => OpLifetimeBound::BoundedByConfig("samples", 100),
        // ── Catch-all: typo / unclassified op ──────────────────────────
        // Reject in lifetime mode. New ops added to AggKind must extend
        // this match; the architectural test
        // `phase12_8_lifetime_ops_have_bounds` walks the catalogue and
        // locks the invariant in CI.
        _ => OpLifetimeBound::Unbounded,
    }
}

/// Walks the request JSON looking for derivation nodes that contain a
/// windowless op whose lifetime memory bound is `Unbounded` (per
/// `lifetime_bound_for_op_str`). Returns `Some(_)` on the first such hit;
/// `None` if every op is either windowed (`params.window` present) or has
/// a finite lifetime bound declared.
///
/// Per CONTEXT D-03: hard reject at register-time. Per CONTEXT D-02
/// framing, the error code is `unbounded_op_in_lifetime_mode` (forward-
/// looking — "requires explicit memory bound in v0", NOT a retrospective
/// "feature removed" code).
///
/// Architectural commitment per `project_v0_events_only_scope` (locked
/// 2026-04-30): each operator declares its lifetime memory ceiling at
/// register-time. Reviving an unbounded-in-lifetime path requires explicit
/// user override + a new ADR.
pub fn pre_check_unbounded_op_in_lifetime_mode(
    body: &serde_json::Value,
) -> Option<FeatureRemovedError> {
    let nodes = body.get("nodes")?.as_array()?;
    for (node_idx, node) in nodes.iter().enumerate() {
        let kind = node.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind != "derivation" {
            continue;
        }
        let ops = match node.get("ops").and_then(|v| v.as_array()) {
            Some(o) => o,
            None => continue,
        };
        let deriv_name = node.get("name").and_then(|v| v.as_str()).unwrap_or("");
        for (op_idx, op_value) in ops.iter().enumerate() {
            // Walk the agg map (group_by ops carry an `agg` map of named
            // features). For any feature whose op is windowless and has
            // an Unbounded lifetime bound, reject.
            let agg_map = match op_value.get("agg").and_then(|v| v.as_object()) {
                Some(m) => m,
                // Non-group_by ops (filter, select, etc.) carry no agg map —
                // skip; only windowless aggregation ops are candidates for
                // the bound check.
                None => continue,
            };
            let path_prefix_node = if deriv_name.is_empty() {
                format!("nodes[{node_idx}].ops[{op_idx}]")
            } else {
                format!("nodes[{node_idx}].{deriv_name}.ops[{op_idx}]")
            };
            for (feature_name, feature_value) in agg_map {
                let op_str = feature_value
                    .get("op")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let params = feature_value.get("params");
                let has_window = params
                    .and_then(|p| p.get("window"))
                    .and_then(|w| w.as_str())
                    .is_some();
                if has_window {
                    // Windowed path — naturally bounded by 64-bucket cap.
                    continue;
                }
                // Lifetime-mode op — check the bound classifier.
                let bound = lifetime_bound_for_op_str(op_str);
                let path = format!("{path_prefix_node}.agg.{feature_name}");
                match bound {
                    OpLifetimeBound::O1
                    | OpLifetimeBound::BoundedSketch
                    | OpLifetimeBound::BoundedByConfig(..) => {
                        // Has a declared bound (or a sensible default
                        // applied at compile-time) — accept.
                        continue;
                    }
                    OpLifetimeBound::BoundedByRequiredKwarg(kwarg_name) => {
                        // Op needs a required cap kwarg in lifetime mode.
                        // Two acceptable shapes:
                        //   1. Integer kwarg (n / samples / num_buckets) →
                        //      must be present and > 0.
                        //   2. Array kwarg (buckets: Vec<f64>) → must be
                        //      present and non-empty.
                        let kwarg_value_u64 = params
                            .and_then(|p| p.get(kwarg_name))
                            .and_then(|v| v.as_u64());
                        let kwarg_value_array_nonempty = params
                            .and_then(|p| p.get(kwarg_name))
                            .and_then(|v| v.as_array())
                            .map(|a| !a.is_empty())
                            .unwrap_or(false);
                        let valid = matches!(kwarg_value_u64, Some(n) if n > 0)
                            || kwarg_value_array_nonempty;
                        if valid {
                            // Has the required cap kwarg with a positive /
                            // non-empty value — accept.
                            continue;
                        }
                        // Missing or invalid required kwarg — reject with a
                        // suggested default that mirrors the cap class.
                        let suggested_default = match kwarg_name {
                            "n" => "n=5",
                            "samples" => "samples=100",
                            "buckets" => "buckets=[10, 50, 100, 500]",
                            _ => "<value>",
                        };
                        let reason = format!(
                            "Aggregation op `{op_str}` requires explicit memory bound \
                             in v0 — the `{kwarg_name}` kwarg is missing or invalid. \
                             Add `{suggested_default}` (or your chosen cap) to the op \
                             params, or add `windowed=\"<duration>\"` (e.g. \
                             windowed=\"60s\") to use the rolling 64-bucket window. \
                             See .planning/phases/12.8-memory-governance/12.8-CONTEXT.md \
                             for the full lifetime-bound contract."
                        );
                        return Some(FeatureRemovedError {
                            code: "unbounded_op_in_lifetime_mode",
                            op_label: Box::leak(op_str.to_string().into_boxed_str()),
                            path,
                            reason,
                        });
                    }
                    OpLifetimeBound::Unbounded => {
                        // Unknown / unclassified op (typo or new op missing
                        // from the classification table) — reject with the
                        // generic v0-framing message.
                        let reason = format!(
                            "Aggregation op `{op_str}` requires explicit memory bound \
                             in v0. Add a `windowed=\"<duration>\"` kwarg (e.g. \
                             windowed=\"60s\") to use the rolling 64-bucket window, \
                             or add the op-specific cap kwarg (e.g. histogram \
                             requires num_buckets=256, top_k requires k=N, \
                             first_n/last_n/lag require n=N). See \
                             .planning/phases/12.8-memory-governance/12.8-CONTEXT.md \
                             for the full lifetime-bound contract."
                        );
                        return Some(FeatureRemovedError {
                            code: "unbounded_op_in_lifetime_mode",
                            // op_label carries the op kind verbatim. Box::leak the
                            // attacker-controlled string identically to 12.7-01's
                            // pattern for `unsupported_node_kind`.
                            op_label: Box::leak(op_str.to_string().into_boxed_str()),
                            path,
                            reason,
                        });
                    }
                }
            }
        }
    }
    None
}

/// Newtype wrapper: a `Vec<PayloadNode>` that has passed all validation rules.
/// `classify_register_diff` accepts `&[PayloadNode]` via `as_slice()`. The
/// endpoint extracts the inner vec via `into_inner()`. Carries compiled
/// OpChains, propagated schemas, and compiled aggregation descriptors
/// (Rule 10 + 11).
#[derive(Debug)]
pub struct ValidatedPayload {
    pub(crate) nodes: Vec<PayloadNode>,
    /// Compiled OpChain per derivation name (Arc-wrapped for cheap sharing).
    /// Populated by Rule 10 (validate_expressions).
    pub compiled_chains: Vec<(String, std::sync::Arc<crate::op_chain::OpChain>)>,
    /// Server-propagated DerivedSchema per derivation name.
    /// Replaces the client-supplied schema for derivations with ops.
    pub propagated_schemas: Vec<(String, crate::schema::DerivedSchema)>,
    /// Compiled AggregationDescriptor per derivation name (Arc-wrapped).
    /// Populated by Rule 11 (validate_aggregations).
    pub compiled_aggregations: Vec<(
        String,
        std::sync::Arc<crate::agg_descriptor::AggregationDescriptor>,
    )>,
}

impl ValidatedPayload {
    /// Construct a ValidatedPayload from plain nodes (no compiled chains or schemas).
    /// Used by tests and back-compat construction sites.
    pub fn from_nodes(nodes: Vec<PayloadNode>) -> Self {
        Self {
            nodes,
            compiled_chains: vec![],
            propagated_schemas: vec![],
            compiled_aggregations: vec![],
        }
    }

    pub fn as_slice(&self) -> &[PayloadNode] {
        &self.nodes
    }

    /// Backward-compat: extract the inner nodes vec.
    pub fn into_inner(self) -> Vec<PayloadNode> {
        self.nodes
    }

    /// Decompose into (nodes, compiled_chains, propagated_schemas, compiled_aggregations).
    // reason: 4-tuple return is the documented decomposition shape; introducing
    // a wrapper struct just for the lint would obscure the call-site pattern
    // (caller destructures with `let (nodes, chains, schemas, aggs) = ...`).
    #[allow(clippy::type_complexity)]
    pub fn into_parts(
        self,
    ) -> (
        Vec<PayloadNode>,
        Vec<(String, std::sync::Arc<crate::op_chain::OpChain>)>,
        Vec<(String, crate::schema::DerivedSchema)>,
        Vec<(
            String,
            std::sync::Arc<crate::agg_descriptor::AggregationDescriptor>,
        )>,
    ) {
        (
            self.nodes,
            self.compiled_chains,
            self.propagated_schemas,
            self.compiled_aggregations,
        )
    }
}

// ─── Main entry point ─────────────────────────────────────────────────────────

/// Validate a registration payload against the current registry state.
///
/// Returns `Ok(ValidatedPayload)` if all 9 rules pass.
/// Returns `Err(Vec<ValidationError>)` with ALL detected violations (fail-soft, not fail-fast),
/// except for the three cross-node rules (5, 7, 8) which are appended after per-node checks.
///
/// An empty payload is valid and results in a no-op at the endpoint.
pub fn validate_payload(
    current: &RegistryInner,
    payload: Vec<PayloadNode>,
) -> Result<ValidatedPayload, Vec<ValidationError>> {
    if payload.is_empty() {
        return Ok(ValidatedPayload::from_nodes(payload));
    }

    let mut errors: Vec<ValidationError> = Vec::new();

    // Rule 1: uniqueness within payload
    validate_uniqueness_within_payload(&payload, &mut errors);

    // Rules 2, 3, 4, 6, 9 — per node
    for (i, node) in payload.iter().enumerate() {
        validate_node_name(i, node, &mut errors);
        match node {
            PayloadNode::Event(e) => validate_event(i, e, &mut errors),
            PayloadNode::Table(t) => validate_table(i, t, &mut errors),
            PayloadNode::Derivation(d) => validate_derivation_struct(i, d, &mut errors),
        }
    }

    // Rule 5: upstream resolution
    validate_upstreams(&payload, current, &mut errors);

    // Rule 8: topological order (upstreams-within-payload must appear before dependents)
    validate_topological_order(&payload, &mut errors);

    // Rule 7: DAG acyclicity (across payload + current)
    validate_acyclicity(&payload, current, &mut errors);

    // Rule 10: expression parsing + schema propagation (Phase 4)
    let mut compiled_chains: Vec<(String, Arc<OpChain>)> = Vec::new();
    let mut propagated_schemas: Vec<(String, crate::schema::DerivedSchema)> = Vec::new();
    if errors.is_empty() {
        // Only run Rule 10 if all structural rules passed (avoids noisy cascading errors
        // when upstreams are missing or the DAG has cycles).
        validate_expressions(
            &payload,
            current,
            &mut errors,
            &mut compiled_chains,
            &mut propagated_schemas,
        );
    }

    // Rule 11: aggregation validation (Phase 5 Plan 04)
    let mut compiled_aggregations: Vec<(
        String,
        std::sync::Arc<crate::agg_descriptor::AggregationDescriptor>,
    )> = Vec::new();
    if errors.is_empty() {
        // Only run Rule 11 if Rules 1-10 passed (avoids cascading errors from
        // missing upstreams or expressions that failed Rule 10).
        let (agg_compiled, agg_errors) =
            crate::agg_compile::compile_aggregations_from_nodes(&payload, current);
        compiled_aggregations = agg_compiled;
        errors.extend(agg_errors);
    }

    if errors.is_empty() {
        Ok(ValidatedPayload {
            nodes: payload,
            compiled_chains,
            propagated_schemas,
            compiled_aggregations,
        })
    } else {
        Err(errors)
    }
}

// ─── Phase 13.4 Plan 06 — D-01 force=true diff matrix ────────────────────────
//
// `classify_register_diff` walks `(prev_registry, new_payload)` and emits a
// `RegisterDiff` (categorized lists) per D-01:
//
//   - Destructive: rename, type_change, op_removal, agg_removal,
//     window_change, key_cols_change (require force=true to apply)
//   - Additive: new_descriptor, new_agg, new_field (allowed without force)
//
// `register_check_force_required` consumes the diff + the wire `force` flag
// and returns `Err(ForceRequiredError)` when destructive entries exist
// without force=true. The `dry_run` flag short-circuits separately at the
// dispatch site (apply_shard.rs); this fn does NOT branch on dry_run.
//
// The diff classifier is PURE: same inputs always produce the same output
// (post-sort), no internal state, no allocations beyond the output Vec.

use crate::registry::{DerivationDescriptor, OutputKind};
use crate::registry_diff::{DiffEntry, RegisterDiff};

/// Error returned by `register_check_force_required` when destructive diff
/// entries exist and `force=true` was not set in the request body. Wire shape
/// is `{"error": {"code": "force_required", "reason": ..., "diff": {...}}}`.
///
/// The dispatch site (apply_shard.rs) emits HTTP 409 + this body. Per A-04 in
/// SCRATCH-PLANNER-NOTES.md the error code is `force_required`
/// (forward-looking) — consistent with `unsupported_node_kind`,
/// `feature_removed_no_*_v0`, etc.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ForceRequiredError {
    pub code: &'static str,
    pub reason: String,
    pub diff: RegisterDiff,
}

/// Pure function: walk `(prev_registry, new_payload)` and produce the D-01
/// categorized diff. Both `additive` and `destructive` lists are sorted by
/// `DiffEntry::sort_key` so two calls with the same inputs produce identical
/// output (Phase 13.4 Plan 06 Task 6.d Test 4 idempotency).
pub fn classify_register_diff(prev: &RegistryInner, new_payload: &[PayloadNode]) -> RegisterDiff {
    let mut additive: Vec<DiffEntry> = Vec::new();
    let mut destructive: Vec<DiffEntry> = Vec::new();
    // Names of payload descriptors whose shape exactly matches what's in the
    // current registry (no additive/destructive entry was pushed for them).
    // Surfaced in the success/noop response per Phase 2 wire contract.
    let mut already_present: Vec<String> = Vec::new();

    // ─── Phase 1: index the payload + current registry by name ────────────
    let mut payload_names: HashSet<String> = HashSet::new();
    for n in new_payload {
        payload_names.insert(n.name().to_string());
    }
    let mut current_names: HashSet<String> = HashSet::new();
    for k in prev.events.keys() {
        current_names.insert(k.clone());
    }
    for k in prev.tables.keys() {
        current_names.insert(k.clone());
    }
    for k in prev.derivations.keys() {
        current_names.insert(k.clone());
    }

    // ─── Phase 2: classify removed-from-current and rename detection ──────
    //
    // A descriptor present in `current_names` but missing from `payload_names`
    // is "removed from prev". Rename heuristic: if exactly-one removal +
    // exactly-one addition with matching kind, emit a Rename pair instead of
    // a NewDescriptor + (no-op for the removal). Per A-04 + D-01 the rename
    // is the destructive class; the matching pair is collapsed to a single
    // Rename entry.
    let removed_names: Vec<String> = current_names
        .iter()
        .filter(|n| !payload_names.contains(*n))
        .cloned()
        .collect();
    let added_payload_nodes: Vec<&PayloadNode> = new_payload
        .iter()
        .filter(|n| !current_names.contains(n.name()))
        .collect();

    // Rename heuristic: pair each removed descriptor with an added descriptor
    // of the same kind (event/table/derivation). When a unique match exists,
    // emit Rename; otherwise treat the removal as an implicit destructive
    // class (collapsed below) and the addition as a NewDescriptor.
    let mut paired_removals: HashSet<String> = HashSet::new();
    let mut paired_additions: HashSet<String> = HashSet::new();

    for removed_name in &removed_names {
        let removed_kind = current_descriptor_kind(prev, removed_name);
        // Pair with first added node of the same kind (deterministic — input order).
        for added in &added_payload_nodes {
            if paired_additions.contains(added.name()) {
                continue;
            }
            if added.kind_str() == removed_kind {
                destructive.push(DiffEntry::Rename {
                    from: removed_name.clone(),
                    to: added.name().to_string(),
                });
                paired_removals.insert(removed_name.clone());
                paired_additions.insert(added.name().to_string());
                break;
            }
        }
    }

    // Unpaired removals: still a destructive change (a descriptor disappeared
    // entirely). Per D-01 the canonical class for a "lost descriptor with no
    // matching new descriptor" is OpRemoval / AggRemoval depending on what
    // was lost. For events/tables we map to AggRemoval{table=name, agg=""}
    // as a stand-in destructive entry — the diff payload still flags
    // `force=true` is required.
    for removed_name in &removed_names {
        if paired_removals.contains(removed_name) {
            continue;
        }
        // Stand-in: signal "the prior descriptor was removed" via AggRemoval
        // with agg="" (sentinel for descriptor-level removal). The wire
        // contract is "force=true is required"; the SDK can decide how to
        // present descriptor-removal vs feature-removal to the user.
        destructive.push(DiffEntry::AggRemoval {
            table: removed_name.clone(),
            agg: String::new(),
        });
    }

    // ─── Phase 3: per-payload-node classification ────────────────────────
    for node in new_payload {
        let name = node.name().to_string();

        if !current_names.contains(&name) {
            // New descriptor (unless paired in rename above).
            if paired_additions.contains(&name) {
                continue;
            }
            additive.push(DiffEntry::NewDescriptor {
                descriptor_kind: node.kind_str().to_string(),
                name,
            });
            continue;
        }

        // Same name in both — descriptor potentially modified. Walk the
        // shape diff and emit per-class entries. If neither classify_*
        // helper pushed anything, the descriptor matched exactly →
        // record as already_present.
        let additive_before = additive.len();
        let destructive_before = destructive.len();
        match node {
            PayloadNode::Event(new_evt) => {
                if let Some(prev_evt) = prev.events.get(&name) {
                    classify_event_changes(prev_evt, new_evt, &mut additive, &mut destructive);
                }
            }
            PayloadNode::Table(new_tbl) => {
                if let Some(prev_tbl) = prev.tables.get(&name) {
                    classify_table_changes(prev_tbl, new_tbl, &mut destructive);
                }
            }
            PayloadNode::Derivation(new_der) => {
                if let Some(prev_der) = prev.derivations.get(&name) {
                    classify_derivation_changes(prev_der, new_der, &mut additive, &mut destructive);
                }
            }
        }
        if additive.len() == additive_before && destructive.len() == destructive_before {
            already_present.push(name);
        }
    }

    // ─── Sort for idempotency (Phase 13.4 Plan 06 Task 6.d Test 4) ────────
    additive.sort_by_key(|e| e.sort_key());
    destructive.sort_by_key(|e| e.sort_key());
    already_present.sort();

    RegisterDiff {
        additive,
        destructive,
        already_present,
    }
}

fn current_descriptor_kind(prev: &RegistryInner, name: &str) -> &'static str {
    if prev.events.contains_key(name) {
        "event"
    } else if prev.tables.contains_key(name) {
        "table"
    } else if prev.derivations.contains_key(name) {
        "derivation"
    } else {
        "unknown"
    }
}

fn classify_event_changes(
    prev: &EventDescriptor,
    new: &EventDescriptor,
    additive: &mut Vec<DiffEntry>,
    destructive: &mut Vec<DiffEntry>,
) {
    // Field type changes + new fields.
    for (field_name, prev_type) in &prev.schema.fields {
        match new.schema.fields.get(field_name) {
            None => {
                // Field removed → treat as TypeChange{from=<type>, to="<absent>"}.
                destructive.push(DiffEntry::TypeChange {
                    field: format!("{}.{}", prev.name, field_name),
                    from: format!("{prev_type:?}").to_lowercase(),
                    to: "<absent>".to_string(),
                });
            }
            Some(new_type) if new_type != prev_type => {
                destructive.push(DiffEntry::TypeChange {
                    field: format!("{}.{}", prev.name, field_name),
                    from: format!("{prev_type:?}").to_lowercase(),
                    to: format!("{new_type:?}").to_lowercase(),
                });
            }
            _ => {}
        }
    }
    for (field_name, new_type) in &new.schema.fields {
        if !prev.schema.fields.contains_key(field_name) {
            additive.push(DiffEntry::NewField {
                event: prev.name.clone(),
                field: field_name.clone(),
                type_: format!("{new_type:?}").to_lowercase(),
            });
        }
    }
}

fn classify_table_changes(
    prev: &TableDescriptor,
    new: &TableDescriptor,
    destructive: &mut Vec<DiffEntry>,
) {
    if prev.primary_key != new.primary_key {
        destructive.push(DiffEntry::KeyColsChange {
            table: prev.name.clone(),
            from: prev.primary_key.clone(),
            to: new.primary_key.clone(),
        });
    }
    // Field type changes / removals are destructive in TableDescriptor too.
    for (field_name, prev_type) in &prev.schema.fields {
        match new.schema.fields.get(field_name) {
            None => destructive.push(DiffEntry::TypeChange {
                field: format!("{}.{}", prev.name, field_name),
                from: format!("{prev_type:?}").to_lowercase(),
                to: "<absent>".to_string(),
            }),
            Some(new_type) if new_type != prev_type => {
                destructive.push(DiffEntry::TypeChange {
                    field: format!("{}.{}", prev.name, field_name),
                    from: format!("{prev_type:?}").to_lowercase(),
                    to: format!("{new_type:?}").to_lowercase(),
                });
            }
            _ => {}
        }
    }
}

fn classify_derivation_changes(
    prev: &DerivationDescriptor,
    new: &DerivationDescriptor,
    additive: &mut Vec<DiffEntry>,
    destructive: &mut Vec<DiffEntry>,
) {
    use crate::op_node::OpNode;

    // Schema-level changes propagate from the upstream event source; do not
    // re-emit here (handled by the event-side classify_event_changes).

    // ─── ops length / structure change ──────────────────────────────────
    // Op removal: any op present in prev but absent in new.
    if prev.ops.len() > new.ops.len() {
        // Find which ops are missing by index. Pair index-by-index for a
        // best-effort label.
        for (idx, prev_op) in prev.ops.iter().enumerate() {
            if new.ops.get(idx).is_none() {
                destructive.push(DiffEntry::OpRemoval {
                    table: prev.name.clone(),
                    agg: op_label(prev_op, idx),
                });
            }
        }
    }

    // ─── group_by-level: agg map deltas + key changes + window changes ──
    //
    // Walk both ops chains in parallel; for matching GroupBy positions
    // compare keys + agg map.
    let pair_len = prev.ops.len().min(new.ops.len());
    for idx in 0..pair_len {
        if let (OpNode::GroupBy { keys: pk, agg: pa }, OpNode::GroupBy { keys: nk, agg: na }) =
            (&prev.ops[idx], &new.ops[idx])
        {
            // Key cols change → destructive.
            if pk != nk {
                destructive.push(DiffEntry::KeyColsChange {
                    table: prev.name.clone(),
                    from: pk.clone(),
                    to: nk.clone(),
                });
            }
            // Aggregation map deltas.
            for (agg_name, prev_spec) in pa {
                match na.get(agg_name) {
                    None => {
                        destructive.push(DiffEntry::AggRemoval {
                            table: prev.name.clone(),
                            agg: agg_name.clone(),
                        });
                    }
                    Some(new_spec) => {
                        // Window change?
                        let prev_window = prev_spec
                            .params
                            .get("window")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let new_window = new_spec
                            .params
                            .get("window")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if prev_window != new_window {
                            destructive.push(DiffEntry::WindowChange {
                                agg: format!("{}.{}", prev.name, agg_name),
                                from: prev_window.to_string(),
                                to: new_window.to_string(),
                            });
                        }
                        // Op rename within an agg (e.g. count → sum) is a
                        // destructive op_removal + new_agg in spirit; keep
                        // it simple and emit AggRemoval+NewAgg.
                        if prev_spec.op != new_spec.op {
                            destructive.push(DiffEntry::AggRemoval {
                                table: prev.name.clone(),
                                agg: agg_name.clone(),
                            });
                            additive.push(DiffEntry::NewAgg {
                                table: prev.name.clone(),
                                agg: agg_name.clone(),
                                source: new_spec.op.clone(),
                            });
                        }
                    }
                }
            }
            for (agg_name, new_spec) in na {
                if !pa.contains_key(agg_name) {
                    additive.push(DiffEntry::NewAgg {
                        table: prev.name.clone(),
                        agg: agg_name.clone(),
                        source: new_spec.op.clone(),
                    });
                }
            }
        }
    }

    // table_primary_key changes for output_kind=table derivations.
    if (prev.output_kind == OutputKind::Table || new.output_kind == OutputKind::Table)
        && prev.table_primary_key != new.table_primary_key
    {
        destructive.push(DiffEntry::KeyColsChange {
            table: prev.name.clone(),
            from: prev.table_primary_key.clone().unwrap_or_default(),
            to: new.table_primary_key.clone().unwrap_or_default(),
        });
    }
}

fn op_label(op: &crate::op_node::OpNode, idx: usize) -> String {
    use crate::op_node::OpNode;
    match op {
        OpNode::Filter { .. } => format!("filter[{idx}]"),
        OpNode::Select { .. } => format!("select[{idx}]"),
        OpNode::Drop { .. } => format!("drop[{idx}]"),
        OpNode::Rename { .. } => format!("rename[{idx}]"),
        OpNode::WithColumns { .. } => format!("with_columns[{idx}]"),
        OpNode::Map { .. } => format!("map[{idx}]"),
        OpNode::Cast { .. } => format!("cast[{idx}]"),
        OpNode::Fillna { .. } => format!("fillna[{idx}]"),
        OpNode::GroupBy { .. } => format!("group_by[{idx}]"),
    }
}

/// D-01 force-required gate. Returns `Err(ForceRequiredError)` if `diff.destructive`
/// is non-empty and `force` is `false`. Otherwise `Ok(())`.
pub fn register_check_force_required(
    diff: &RegisterDiff,
    force: bool,
) -> Result<(), ForceRequiredError> {
    if !diff.destructive.is_empty() && !force {
        return Err(ForceRequiredError {
            code: "force_required",
            reason: "Destructive registry change requires force=true. See diff for details."
                .to_string(),
            diff: diff.clone(),
        });
    }
    Ok(())
}

// ─── Path helpers ─────────────────────────────────────────────────────────────

fn path_node(i: usize) -> String {
    format!("nodes[{i}]")
}

fn path_field(i: usize, suffix: &str) -> String {
    format!("nodes[{i}].{suffix}")
}

// ─── Rule 1: uniqueness within payload ────────────────────────────────────────

fn validate_uniqueness_within_payload(payload: &[PayloadNode], errors: &mut Vec<ValidationError>) {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for (i, node) in payload.iter().enumerate() {
        let name = node.name();
        if let Some(first_idx) = seen.get(name) {
            errors.push(ValidationError {
                code: ErrorCode::NameDuplicate,
                path: path_field(i, "name"),
                reason: format!(
                    "duplicate descriptor name '{name}'; first seen at nodes[{first_idx}]"
                ),
            });
        } else {
            seen.insert(name, i);
        }
    }
}

// ─── Rule 2: name validation ──────────────────────────────────────────────────

fn validate_node_name(i: usize, node: &PayloadNode, errors: &mut Vec<ValidationError>) {
    let name = node.name();
    match validate_descriptor_name(name) {
        Ok(()) => {}
        Err(DescriptorNameError::Empty) => errors.push(ValidationError {
            code: ErrorCode::NameEmpty,
            path: path_field(i, "name"),
            reason: "descriptor name must not be empty".to_string(),
        }),
        Err(DescriptorNameError::BadPattern(n)) => errors.push(ValidationError {
            code: ErrorCode::NameBadPattern,
            path: path_field(i, "name"),
            reason: format!(
                "descriptor name '{n}' must match [A-Za-z_][A-Za-z0-9_]* (no hyphens or leading digits)"
            ),
        }),
        Err(DescriptorNameError::ReservedPrefix(n)) => errors.push(ValidationError {
            code: ErrorCode::NameReservedPrefix,
            path: path_field(i, "name"),
            reason: format!("descriptor name '{n}' uses reserved prefix '_beava_'"),
        }),
        Err(DescriptorNameError::TooLong { len }) => errors.push(ValidationError {
            code: ErrorCode::NameTooLong,
            path: path_field(i, "name"),
            reason: format!("descriptor name is {len} chars; maximum is 128"),
        }),
    }
}

// ─── Rule 3: event schema validation ─────────────────────────────────────────

fn validate_event(i: usize, e: &EventDescriptor, errors: &mut Vec<ValidationError>) {
    // `event_time_field` deleted from EventDescriptor — windowed-op bucketing
    // uses server-side wall-clock exclusively (Redis-shaped, processing-time
    // only in v0). Stale fixtures sending `event_time_field` get rejected at
    // the JSON-prelude layer (`pre_check_legacy_event_time_keys`) before
    // reaching this validator. Schema must be non-empty (any fields OK).
    if e.schema.fields.is_empty() {
        errors.push(ValidationError {
            code: ErrorCode::EventSchemaEmpty,
            path: path_field(i, "schema.fields"),
            reason: "event schema must have at least one field".to_string(),
        });
    }

    // Rule 9: dedupe_key
    if let Some(ref key) = e.dedupe_key {
        if !e.schema.fields.contains_key(key) {
            errors.push(ValidationError {
                code: ErrorCode::DedupeKeyUnknownField,
                path: path_field(i, "dedupe_key"),
                reason: format!("dedupe_key '{key}' is not a field in schema"),
            });
        }
    }
    if let Some(ttl) = e.dedupe_window_ms {
        if ttl == 0 {
            errors.push(ValidationError {
                code: ErrorCode::DedupeWindowNonPositive,
                path: path_field(i, "dedupe_window_ms"),
                reason: "dedupe_window_ms must be positive (> 0)".to_string(),
            });
        }
    }
}

// ─── Rule 4: table schema validation ─────────────────────────────────────────

fn validate_table(i: usize, t: &TableDescriptor, errors: &mut Vec<ValidationError>) {
    if t.primary_key.is_empty() {
        errors.push(ValidationError {
            code: ErrorCode::TablePrimaryKeyEmpty,
            path: path_field(i, "primary_key"),
            reason: "primary_key must have at least 1 field".to_string(),
        });
        return; // don't check unknown fields if key is empty
    }
    if t.primary_key.len() > 4 {
        errors.push(ValidationError {
            code: ErrorCode::TablePrimaryKeyTooLong,
            path: path_field(i, "primary_key"),
            reason: format!(
                "primary_key has {} fields; maximum is 4",
                t.primary_key.len()
            ),
        });
    }
    for (j, key_field) in t.primary_key.iter().enumerate() {
        if !t.schema.fields.contains_key(key_field) {
            errors.push(ValidationError {
                code: ErrorCode::TablePrimaryKeyUnknownField,
                path: path_field(i, &format!("primary_key[{j}]")),
                reason: format!("primary_key field '{key_field}' does not exist in schema.fields"),
            });
        }
    }
}

// ─── Rule 6: derivation schema + output_kind=Table check ─────────────────────

fn validate_derivation_struct(
    i: usize,
    d: &crate::registry::DerivationDescriptor,
    errors: &mut Vec<ValidationError>,
) {
    // Clients may omit `schema.fields` entirely (the descriptor field is
    // `serde(default)`-able). The empty-fields check only fires when the
    // derivation ALSO has no ops — i.e. no chain to infer from at the
    // `validate_expressions` (Rule 10) pass. With ops present,
    // schema-propagation runs from upstream + chain and writes the
    // resulting fields back to the registry post-validation.
    if d.schema.fields.is_empty() && d.ops.is_empty() {
        errors.push(ValidationError {
            code: ErrorCode::DerivationSchemaEmpty,
            path: path_field(i, "schema.fields"),
            reason: "derivation schema must have at least one field".to_string(),
        });
    }

    if d.output_kind == crate::registry::OutputKind::Table && d.table_primary_key.is_none() {
        errors.push(ValidationError {
            code: ErrorCode::DerivationOutputKindTableMissingPrimaryKey,
            path: path_field(i, "table_primary_key"),
            reason: "derivation with output_kind='table' must specify table_primary_key"
                .to_string(),
        });
    }
}

// ─── Rule 5: upstream resolution ─────────────────────────────────────────────

fn validate_upstreams(
    payload: &[PayloadNode],
    current: &RegistryInner,
    errors: &mut Vec<ValidationError>,
) {
    let payload_names: HashSet<&str> = payload.iter().map(|n| n.name()).collect();

    for (i, node) in payload.iter().enumerate() {
        if let PayloadNode::Derivation(d) = node {
            for (j, upstream) in d.upstreams.iter().enumerate() {
                let known_in_payload = payload_names.contains(upstream.as_str());
                let known_in_current = current.events.contains_key(upstream)
                    || current.tables.contains_key(upstream)
                    || current.derivations.contains_key(upstream);
                if !known_in_payload && !known_in_current {
                    errors.push(ValidationError {
                        code: ErrorCode::DerivationUpstreamUnknown,
                        path: path_field(i, &format!("upstreams[{j}]")),
                        reason: format!(
                            "upstream '{upstream}' is not declared in this payload or in the registry"
                        ),
                    });
                }
            }
        }
    }
}

// ─── Rule 8: topological order ────────────────────────────────────────────────

fn validate_topological_order(payload: &[PayloadNode], errors: &mut Vec<ValidationError>) {
    // Build index: name → position in payload
    let payload_index: HashMap<&str, usize> = payload
        .iter()
        .enumerate()
        .map(|(i, n)| (n.name(), i))
        .collect();

    for (i, node) in payload.iter().enumerate() {
        if let PayloadNode::Derivation(d) = node {
            for (j, upstream) in d.upstreams.iter().enumerate() {
                // Only check upstreams that appear in this payload (not registry-resolved ones)
                if let Some(&upstream_idx) = payload_index.get(upstream.as_str()) {
                    if upstream_idx > i {
                        errors.push(ValidationError {
                            code: ErrorCode::TopologicalOrderViolation,
                            path: path_field(i, &format!("upstreams[{j}]")),
                            reason: format!(
                                "upstream '{upstream}' appears later in payload at nodes[{upstream_idx}]"
                            ),
                        });
                    }
                }
            }
        }
    }
}

// ─── Rule 7: acyclicity (DFS, three-color) ────────────────────────────────────

fn validate_acyclicity(
    payload: &[PayloadNode],
    current: &RegistryInner,
    errors: &mut Vec<ValidationError>,
) {
    // Build adjacency: name → Vec<upstream_name>
    // Payload nodes shadow current nodes of the same name.
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();

    // Start with current registry's derivations
    for (name, d) in &current.derivations {
        adj.insert(name.clone(), d.upstreams.clone());
    }

    // Overlay with payload (payload shadows current)
    for node in payload {
        if let PayloadNode::Derivation(d) = node {
            adj.insert(d.name.clone(), d.upstreams.clone());
        }
    }

    // Build payload index for error reporting
    let payload_index: HashMap<&str, usize> = payload
        .iter()
        .enumerate()
        .map(|(i, n)| (n.name(), i))
        .collect();

    // Collect all node names (only derivations can form cycles; events/tables have no upstreams)
    let all_names: Vec<String> = adj.keys().cloned().collect();

    // Three-color DFS
    // 0 = white (unvisited), 1 = gray (in stack), 2 = black (done)
    let mut color: HashMap<String, u8> = HashMap::new();
    let mut stack: Vec<String> = Vec::new();

    for start in &all_names {
        if color.get(start).copied().unwrap_or(0) == 0 {
            if let Some(cycle) = dfs_cycle(start, &adj, &mut color, &mut stack) {
                // Find which payload node is in the cycle for error path
                let cycle_str = cycle.join(" -> ");
                // Pick the first payload node that's part of the cycle for path
                let path = cycle
                    .iter()
                    .filter_map(|n| payload_index.get(n.as_str()))
                    .next()
                    .map(|idx| path_node(*idx))
                    .unwrap_or_else(|| "nodes".to_string());

                errors.push(ValidationError {
                    code: ErrorCode::RegistrationCycle,
                    path,
                    reason: format!("cycle detected: {cycle_str}"),
                });
                return; // report only the first cycle (CONTEXT.md: first-wins)
            }
        }
    }
}

fn dfs_cycle(
    node: &str,
    adj: &HashMap<String, Vec<String>>,
    color: &mut HashMap<String, u8>,
    stack: &mut Vec<String>,
) -> Option<Vec<String>> {
    color.insert(node.to_string(), 1); // gray
    stack.push(node.to_string());

    if let Some(neighbors) = adj.get(node) {
        for neighbor in neighbors {
            let c = color.get(neighbor).copied().unwrap_or(0);
            if c == 1 {
                // Back edge → cycle found; extract cycle from stack
                let cycle_start = stack.iter().position(|n| n == neighbor).unwrap_or(0);
                let mut cycle: Vec<String> = stack[cycle_start..].to_vec();
                cycle.push(neighbor.to_string()); // close the cycle
                return Some(cycle);
            }
            if c == 0 {
                if let Some(cycle) = dfs_cycle(neighbor, adj, color, stack) {
                    return Some(cycle);
                }
            }
        }
    }

    stack.pop();
    color.insert(node.to_string(), 2); // black
    None
}

// ─── Rule 10: Expression validation + schema propagation (Phase 4) ────────────

/// Resolve the combined input `Schema` for a derivation by unioning all upstream schemas.
///
/// Lookup order per upstream name:
/// 1. Payload (events/tables/derivations already seen in this batch — must appear before
///    dependents due to Rule 8 topological order).
/// 2. Current registry (already-registered events, tables, derivations).
///
/// Uses `Schema::from_event` / `from_table` / `from_derived` adapters.
fn resolve_upstream_schema(
    upstream_name: &str,
    payload: &[PayloadNode],
    current: &RegistryInner,
) -> Option<Schema> {
    // Check payload first (topological order guarantees upstreams come before dependents).
    for node in payload {
        match node {
            PayloadNode::Event(e) if e.name == upstream_name => {
                return Some(Schema::from_event(&e.schema));
            }
            PayloadNode::Table(t) if t.name == upstream_name => {
                return Some(Schema::from_table(&t.schema));
            }
            PayloadNode::Derivation(d) if d.name == upstream_name => {
                return Some(Schema::from_derived(&d.schema));
            }
            _ => {}
        }
    }
    // Check registry.
    if let Some(e) = current.events.get(upstream_name) {
        return Some(Schema::from_event(&e.schema));
    }
    if let Some(t) = current.tables.get(upstream_name) {
        return Some(Schema::from_table(&t.schema));
    }
    if let Some(d) = current.derivations.get(upstream_name) {
        return Some(Schema::from_derived(&d.schema));
    }
    None
}

/// Union multiple schemas by merging fields (later schemas' fields take precedence on collision).
fn union_schemas(schemas: Vec<Schema>) -> Schema {
    let mut result = Schema::new();
    for s in schemas {
        for (k, v) in s.fields {
            result.fields.insert(k, v);
        }
        for opt in s.optional_fields {
            if !result.optional_fields.contains(&opt) {
                result.optional_fields.push(opt);
            }
        }
    }
    result
}

/// Map a `PropagationError` to a `ValidationError` at the given derivation index `node_idx`
/// and op index `op_idx`.
fn propagation_error_to_validation(
    e: &PropagationError,
    node_idx: usize,
    op_idx: usize,
) -> ValidationError {
    match e {
        PropagationError::InvalidExpr {
            parse_error: pe, ..
        } => ValidationError {
            code: ErrorCode::InvalidExpression,
            path: format!("nodes[{node_idx}].ops[{op_idx}].expr"),
            reason: format!("col {}: {}", pe.col, pe.reason),
        },
        PropagationError::FieldMissing { field, .. } => ValidationError {
            code: ErrorCode::UnknownFieldReference,
            path: format!("nodes[{node_idx}].ops[{op_idx}].expr"),
            reason: format!("field '{field}' not found in upstream schema"),
        },
        PropagationError::TypeMismatch { reason, .. } => ValidationError {
            code: ErrorCode::SchemaPropagationFailure,
            path: format!("nodes[{node_idx}].ops[{op_idx}]"),
            reason: reason.clone(),
        },
        PropagationError::RenameCollision { new, .. } => ValidationError {
            code: ErrorCode::SchemaPropagationFailure,
            path: format!("nodes[{node_idx}].ops[{op_idx}].mapping"),
            reason: format!("rename collision on field '{new}'"),
        },
        PropagationError::UnsupportedOp { .. } => {
            // Treated as pass-through in Phase 4 — not an error.
            // This branch should never be called (callers filter UnsupportedOp before
            // calling this function). Include as unreachable-but-safe fallback.
            ValidationError {
                code: ErrorCode::SchemaPropagationFailure,
                path: format!("nodes[{node_idx}].ops[{op_idx}]"),
                reason: "unsupported op (pass-through in Phase 4)".to_string(),
            }
        }
    }
}

/// Rule 10: parse every expression in every derivation's op chain, walk schema propagation,
/// and compile each chain. Appends to `errors` on failure (fail-soft). On success, appends
/// to `compiled_chains` and `propagated_schemas`.
///
/// This function is only called when rules 1-9 have already passed.
fn validate_expressions(
    payload: &[PayloadNode],
    current: &RegistryInner,
    errors: &mut Vec<ValidationError>,
    compiled_chains: &mut Vec<(String, Arc<OpChain>)>,
    propagated_schemas: &mut Vec<(String, crate::schema::DerivedSchema)>,
) {
    // Build a map from derivation name → propagated DerivedSchema so downstream
    // derivations in the same payload see the server-authoritative schema.
    // (Because payload is topologically ordered, we process in order.)
    let mut propagated_in_batch: HashMap<String, crate::schema::DerivedSchema> = HashMap::new();

    for (node_idx, node) in payload.iter().enumerate() {
        let deriv = match node {
            PayloadNode::Derivation(d) => d,
            _ => continue, // Events and tables have no ops — skip.
        };

        if deriv.ops.is_empty() {
            // No ops: propagated schema = union of upstream schemas (or client-supplied
            // if we can't resolve upstreams — defensive fallback).
            let upstream_schemas: Vec<Schema> = deriv
                .upstreams
                .iter()
                .filter_map(|u| {
                    // Check already-propagated batch first (server-authoritative for
                    // upstream derivations in same payload).
                    if let Some(ds) = propagated_in_batch.get(u) {
                        return Some(Schema::from_derived(ds));
                    }
                    resolve_upstream_schema(u, payload, current)
                })
                .collect();
            let propagated = if upstream_schemas.is_empty() {
                // Fallback: use client-supplied schema.
                Schema::from_derived(&deriv.schema)
            } else {
                union_schemas(upstream_schemas)
            };
            let derived = propagated.into_derived();
            propagated_in_batch.insert(deriv.name.clone(), derived.clone());
            propagated_schemas.push((deriv.name.clone(), derived));
            continue;
        }

        // Resolve combined input schema from all upstreams.
        let upstream_schemas: Vec<Schema> = deriv
            .upstreams
            .iter()
            .filter_map(|u| {
                if let Some(ds) = propagated_in_batch.get(u) {
                    return Some(Schema::from_derived(ds));
                }
                resolve_upstream_schema(u, payload, current)
            })
            .collect();

        let combined_input = if upstream_schemas.is_empty() {
            // No resolvable upstreams (structural rules should have caught unknown
            // upstreams, but be defensive). Skip Rule 10 for this derivation.
            propagated_schemas.push((deriv.name.clone(), deriv.schema.clone()));
            propagated_in_batch.insert(deriv.name.clone(), deriv.schema.clone());
            continue;
        } else {
            union_schemas(upstream_schemas)
        };

        // Check for UnsupportedOp ops (GroupBy) — treat as pass-through.
        // Filter them out before calling OpChain::compile so we don't get
        // spurious errors. Treated as warnings; register succeeds.
        //
        // Phase 12.7 events-only: OpNode::Join / OpNode::Union arms removed.
        // The JSON-prelude shim `pre_check_removed_ops` runs at the dispatch
        // layer BEFORE strict RegisterPayload deserialize and emits structured
        // error codes feature_removed_no_joins_v0 / feature_removed_no_unions_v0
        // — joins/unions never reach this point.
        let has_unsupported = deriv
            .ops
            .iter()
            .any(|op| matches!(op, crate::op_node::OpNode::GroupBy { .. }));

        if has_unsupported {
            // Log a warning (at build time this is a tracing warn; tests won't see it,
            // but that's fine — the behavior is documented). Accept the registration.
            tracing::warn!(
                kind = "register.rule10.unsupported_op",
                derivation = %deriv.name,
                "derivation contains GroupBy ops which are not validated by \
                 Rule 10 (pass-through)"
            );
            // Still compile the chain prefix (filter / select / drop / rename /
            // with_columns / cast / fillna) so the apply path can transform
            // events at runtime BEFORE the aggregation evaluates.
            // `OpChain::compile` internally skips `GroupBy` ops; we discard
            // its `final_schema` because the post-agg table schema (already
            // on `deriv.schema`) is the canonical propagated schema for
            // downstream consumers.
            //
            // Required for the `bv.lit('web')` constant-column flow — without
            // this, the runtime apply path never executes the prefix chain
            // and where-predicates / agg field-refs that target chain-added
            // columns silently see None.
            //
            // Filter out GroupBy ops before passing to OpChain::compile —
            // `propagate_schema` rejects GroupBy as `UnsupportedOp`, which
            // would cause `?` propagation to abort the compile entirely. We
            // want only the chain prefix (the row-transform ops). The agg
            // semantics are owned by Rule 11 (compile_aggregations_from_nodes).
            let prefix_ops: Vec<crate::op_node::OpNode> = deriv
                .ops
                .iter()
                .filter(|op| !matches!(op, crate::op_node::OpNode::GroupBy { .. }))
                .cloned()
                .collect();
            if !prefix_ops.is_empty() {
                match OpChain::compile(&combined_input, &prefix_ops) {
                    Ok((chain, _)) => {
                        compiled_chains.push((deriv.name.clone(), Arc::new(chain)));
                    }
                    Err(compile_errors) => {
                        for ce in &compile_errors {
                            let op_idx = match ce {
                                PropagationError::InvalidExpr { op_index, .. }
                                | PropagationError::FieldMissing { op_index, .. }
                                | PropagationError::TypeMismatch { op_index, .. }
                                | PropagationError::RenameCollision { op_index, .. }
                                | PropagationError::UnsupportedOp { op_index, .. } => *op_index,
                            };
                            if matches!(ce, PropagationError::UnsupportedOp { .. }) {
                                continue;
                            }
                            errors.push(propagation_error_to_validation(ce, node_idx, op_idx));
                        }
                    }
                }
            }
            propagated_schemas.push((deriv.name.clone(), deriv.schema.clone()));
            propagated_in_batch.insert(deriv.name.clone(), deriv.schema.clone());
            continue;
        }

        // Compile the chain: propagate_schema + build CompiledOp sequence.
        match OpChain::compile(&combined_input, &deriv.ops) {
            Ok((chain, final_schema)) => {
                compiled_chains.push((deriv.name.clone(), Arc::new(chain)));
                let derived = final_schema.into_derived();
                propagated_in_batch.insert(deriv.name.clone(), derived.clone());
                propagated_schemas.push((deriv.name.clone(), derived));
            }
            Err(compile_errors) => {
                // Fail-soft: translate each error and collect; continue with other derivations.
                // For path purposes, we need to re-map op_index from PropagationError
                // to the actual position in deriv.ops.
                for ce in &compile_errors {
                    let op_idx = match ce {
                        PropagationError::InvalidExpr { op_index, .. }
                        | PropagationError::FieldMissing { op_index, .. }
                        | PropagationError::TypeMismatch { op_index, .. }
                        | PropagationError::RenameCollision { op_index, .. }
                        | PropagationError::UnsupportedOp { op_index, .. } => *op_index,
                    };
                    // Skip UnsupportedOp — treated as pass-through.
                    if matches!(ce, PropagationError::UnsupportedOp { .. }) {
                        continue;
                    }
                    errors.push(propagation_error_to_validation(ce, node_idx, op_idx));
                }
                // Use client-supplied schema as best-effort carry-forward.
                propagated_in_batch.insert(deriv.name.clone(), deriv.schema.clone());
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests_structural {
    use super::*;
    use crate::registry::{
        DerivationDescriptor, EventDescriptor, OutputKind, TableDescriptor, TableMode,
    };
    use crate::schema::{DerivedSchema, EventSchema, FieldType, TableSchema};
    use std::collections::BTreeMap;

    // ── Test helpers ──────────────────────────────────────────────────────────

    fn empty_current() -> RegistryInner {
        RegistryInner::default()
    }

    fn minimal_event(name: &str) -> PayloadNode {
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        PayloadNode::Event(EventDescriptor {
            name: name.to_string(),
            schema: EventSchema {
                fields,
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

    fn minimal_table(name: &str, pk: Vec<&str>) -> PayloadNode {
        let mut fields = BTreeMap::new();
        for k in &pk {
            fields.insert(k.to_string(), FieldType::Str);
        }
        fields.insert("extra".to_string(), FieldType::Str);
        PayloadNode::Table(TableDescriptor {
            name: name.to_string(),
            primary_key: pk.iter().map(|s| s.to_string()).collect(),
            schema: TableSchema {
                fields,
                optional_fields: vec![],
            },
            ttl_ms: None,
            mode: TableMode::Upsert,
            registered_at_version: 0,
            temporal: false,
            retention_ms: None,
        })
    }

    fn minimal_derivation(name: &str, upstreams: Vec<&str>) -> PayloadNode {
        let mut fields = BTreeMap::new();
        fields.insert("amount".to_string(), FieldType::F64);
        PayloadNode::Derivation(DerivationDescriptor {
            name: name.to_string(),
            output_kind: OutputKind::Event,
            upstreams: upstreams.iter().map(|s| s.to_string()).collect(),
            ops: vec![],
            schema: DerivedSchema {
                fields,
                optional_fields: vec![],
            },
            table_primary_key: None,
            registered_at_version: 0,
        })
    }

    fn assert_ok(payload: Vec<PayloadNode>) {
        match validate_payload(&empty_current(), payload) {
            Ok(_) => {}
            Err(errs) => panic!("expected Ok, got {} errors: {errs:#?}", errs.len()),
        }
    }

    fn assert_err_contains(
        payload: Vec<PayloadNode>,
        expected_code: ErrorCode,
        expected_path_contains: &str,
    ) {
        let errs = validate_payload(&empty_current(), payload).expect_err("expected Err");
        let found = errs
            .iter()
            .any(|e| e.code == expected_code && e.path.contains(expected_path_contains));
        assert!(
            found,
            "expected error code {expected_code:?} with path containing '{expected_path_contains}', got: {errs:#?}"
        );
    }

    fn assert_err_contains_with_current(
        current: &RegistryInner,
        payload: Vec<PayloadNode>,
        expected_code: ErrorCode,
        expected_path_contains: &str,
    ) {
        let errs = validate_payload(current, payload).expect_err("expected Err");
        let found = errs
            .iter()
            .any(|e| e.code == expected_code && e.path.contains(expected_path_contains));
        assert!(
            found,
            "expected error code {expected_code:?} with path containing '{expected_path_contains}', got: {errs:#?}"
        );
    }

    // ── Rule 1: Node uniqueness ───────────────────────────────────────────────

    #[test]
    fn rule1_pass_distinct_names() {
        assert_ok(vec![minimal_event("A"), minimal_table("B", vec!["extra"])]);
    }

    #[test]
    fn rule1_fail_duplicate_event() {
        assert_err_contains(
            vec![minimal_event("A"), minimal_event("A")],
            ErrorCode::NameDuplicate,
            "nodes[1].name",
        );
    }

    #[test]
    fn rule1_fail_duplicate_cross_kind() {
        assert_err_contains(
            vec![minimal_event("Foo"), minimal_table("Foo", vec!["extra"])],
            ErrorCode::NameDuplicate,
            "nodes[1].name",
        );
    }

    // ── Rule 2: Name validation ───────────────────────────────────────────────

    #[test]
    fn rule2_pass_valid_name() {
        assert_ok(vec![minimal_event("Transaction_1")]);
    }

    #[test]
    fn rule2_fail_empty_name() {
        // We can't construct a PayloadNode with "" name easily via minimal_event,
        // so build it directly.
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        let node = PayloadNode::Event(EventDescriptor {
            name: "".to_string(),
            schema: EventSchema {
                fields,
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        });
        assert_err_contains(vec![node], ErrorCode::NameEmpty, "nodes[0].name");
    }

    #[test]
    fn rule2_fail_bad_pattern_leading_digit() {
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        let node = PayloadNode::Event(EventDescriptor {
            name: "1foo".to_string(),
            schema: EventSchema {
                fields,
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        });
        assert_err_contains(vec![node], ErrorCode::NameBadPattern, "nodes[0].name");
    }

    #[test]
    fn rule2_fail_reserved_prefix() {
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        let node = PayloadNode::Event(EventDescriptor {
            name: "_beava_internal".to_string(),
            schema: EventSchema {
                fields,
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        });
        assert_err_contains(vec![node], ErrorCode::NameReservedPrefix, "nodes[0].name");
    }

    #[test]
    fn rule2_fail_name_too_long() {
        let long_name = "a".repeat(129);
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        let node = PayloadNode::Event(EventDescriptor {
            name: long_name,
            schema: EventSchema {
                fields,
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        });
        assert_err_contains(vec![node], ErrorCode::NameTooLong, "nodes[0].name");
    }

    // ── Rule 3: Event schema ──────────────────────────────────────────────────

    #[test]
    fn rule3_pass_valid_event() {
        assert_ok(vec![minimal_event("T")]);
    }

    // Tests `rule3_fail_event_time_field_missing` and
    // `rule3_fail_event_time_field_wrong_type` were deleted —
    // `event_time_field` is gone from EventDescriptor (and the validator
    // no longer checks it). The strict-deny path on stale `event_time_field`
    // JSON keys lives in `register_validate::pre_check_legacy_event_time_keys`
    // and is exercised by `phase12_6_event_time_hard_rip.rs`.

    #[test]
    fn rule3_fail_event_schema_empty() {
        // Schema with zero fields fails. The rule is "fields must be
        // non-empty" — the pre-pivot rule was "must have ≥1 non-event_time
        // field" (the special-case is gone with event_time_field deletion).
        let node = PayloadNode::Event(EventDescriptor {
            name: "T".to_string(),
            schema: EventSchema {
                fields: BTreeMap::new(),
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        });
        assert_err_contains(
            vec![node],
            ErrorCode::EventSchemaEmpty,
            "nodes[0].schema.fields",
        );
    }

    #[test]
    fn rule3_pass_event_time_field_omitted() {
        // Event with NO event_time_field (server will stamp wall-clock on push)
        let current = RegistryInner::default();
        let payload = vec![PayloadNode::Event(EventDescriptor {
            name: "Heartbeat".to_string(),
            schema: EventSchema {
                fields: {
                    let mut m = BTreeMap::new();
                    m.insert("user_id".to_string(), FieldType::Str);
                    m
                },
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        })];
        let result = validate_payload(&current, payload);
        assert!(
            result.is_ok(),
            "event without event_time_field should be valid"
        );
    }

    // ── Rule 4: Table schema ──────────────────────────────────────────────────

    #[test]
    fn rule4_pass_valid_table() {
        assert_ok(vec![minimal_table("M", vec!["extra"])]);
    }

    #[test]
    fn rule4_fail_primary_key_unknown_field() {
        let mut fields = BTreeMap::new();
        fields.insert("name".to_string(), FieldType::Str);
        let node = PayloadNode::Table(TableDescriptor {
            name: "M".to_string(),
            primary_key: vec!["id".to_string()], // "id" not in schema
            schema: TableSchema {
                fields,
                optional_fields: vec![],
            },
            ttl_ms: None,
            mode: TableMode::Upsert,
            registered_at_version: 0,
            temporal: false,
            retention_ms: None,
        });
        assert_err_contains(
            vec![node],
            ErrorCode::TablePrimaryKeyUnknownField,
            "nodes[0].primary_key[0]",
        );
    }

    #[test]
    fn rule4_fail_primary_key_empty() {
        let mut fields = BTreeMap::new();
        fields.insert("id".to_string(), FieldType::Str);
        let node = PayloadNode::Table(TableDescriptor {
            name: "M".to_string(),
            primary_key: vec![], // empty
            schema: TableSchema {
                fields,
                optional_fields: vec![],
            },
            ttl_ms: None,
            mode: TableMode::Upsert,
            registered_at_version: 0,
            temporal: false,
            retention_ms: None,
        });
        assert_err_contains(
            vec![node],
            ErrorCode::TablePrimaryKeyEmpty,
            "nodes[0].primary_key",
        );
    }

    #[test]
    fn rule4_fail_primary_key_too_long() {
        let mut fields = BTreeMap::new();
        let pk: Vec<String> = (0..5).map(|i| format!("k{i}")).collect();
        for k in &pk {
            fields.insert(k.clone(), FieldType::Str);
        }
        let node = PayloadNode::Table(TableDescriptor {
            name: "M".to_string(),
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
        });
        assert_err_contains(
            vec![node],
            ErrorCode::TablePrimaryKeyTooLong,
            "nodes[0].primary_key",
        );
    }

    // ── Rule 6: Derivation schema ─────────────────────────────────────────────

    #[test]
    fn rule6_pass_nonempty_schema() {
        assert_ok(vec![minimal_event("A"), minimal_derivation("D", vec!["A"])]);
    }

    #[test]
    fn rule6_fail_empty_schema() {
        let node = PayloadNode::Derivation(DerivationDescriptor {
            name: "D".to_string(),
            output_kind: OutputKind::Event,
            upstreams: vec![],
            ops: vec![],
            schema: DerivedSchema {
                fields: BTreeMap::new(), // empty
                optional_fields: vec![],
            },
            table_primary_key: None,
            registered_at_version: 0,
        });
        assert_err_contains(
            vec![node],
            ErrorCode::DerivationSchemaEmpty,
            "nodes[0].schema.fields",
        );
    }

    #[test]
    fn rule6b_pass_output_kind_table_with_primary_key() {
        let mut fields = BTreeMap::new();
        fields.insert("user".to_string(), FieldType::Str);
        fields.insert("count".to_string(), FieldType::I64);
        let node = PayloadNode::Derivation(DerivationDescriptor {
            name: "D".to_string(),
            output_kind: OutputKind::Table,
            upstreams: vec![],
            ops: vec![],
            schema: DerivedSchema {
                fields,
                optional_fields: vec![],
            },
            table_primary_key: Some(vec!["user".to_string()]),
            registered_at_version: 0,
        });
        // upstreams empty but that's a rule5 concern; test only rule6b here
        // We allow empty upstreams for this specific test by seeding them in current
        assert_ok(vec![node]);
    }

    #[test]
    fn rule6b_fail_output_kind_table_missing_primary_key() {
        let mut fields = BTreeMap::new();
        fields.insert("user".to_string(), FieldType::Str);
        let node = PayloadNode::Derivation(DerivationDescriptor {
            name: "D".to_string(),
            output_kind: OutputKind::Table,
            upstreams: vec![],
            ops: vec![],
            schema: DerivedSchema {
                fields,
                optional_fields: vec![],
            },
            table_primary_key: None, // missing
            registered_at_version: 0,
        });
        assert_err_contains(
            vec![node],
            ErrorCode::DerivationOutputKindTableMissingPrimaryKey,
            "nodes[0].table_primary_key",
        );
    }

    // ── Rule 9: Idempotency ───────────────────────────────────────────────────

    #[test]
    fn rule9_pass_valid_dedupe() {
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        fields.insert("request_id".to_string(), FieldType::Str);
        let node = PayloadNode::Event(EventDescriptor {
            name: "T".to_string(),
            schema: EventSchema {
                fields,
                optional_fields: vec![],
            },
            dedupe_key: Some("request_id".to_string()),
            dedupe_window_ms: Some(1000),
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        });
        assert_ok(vec![node]);
    }

    #[test]
    fn rule9_fail_dedupe_key_unknown_field() {
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        let node = PayloadNode::Event(EventDescriptor {
            name: "T".to_string(),
            schema: EventSchema {
                fields,
                optional_fields: vec![],
            },
            dedupe_key: Some("missing".to_string()),
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        });
        assert_err_contains(
            vec![node],
            ErrorCode::DedupeKeyUnknownField,
            "nodes[0].dedupe_key",
        );
    }

    #[test]
    fn rule9_fail_dedupe_window_zero() {
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        let node = PayloadNode::Event(EventDescriptor {
            name: "T".to_string(),
            schema: EventSchema {
                fields,
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: Some(0), // zero = non-positive
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        });
        assert_err_contains(
            vec![node],
            ErrorCode::DedupeWindowNonPositive,
            "nodes[0].dedupe_window_ms",
        );
    }

    // ── Multi-error collection ────────────────────────────────────────────────

    #[test]
    fn collects_multiple_errors() {
        // 3 nodes each with independent errors:
        // node 0: bad name
        // node 1: empty event schema (was "bad event_time_field" pre-pivot;
        //         the event_time_field validation rule was deleted with the
        //         field itself)
        // node 2: empty derivation schema
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        let node0 = PayloadNode::Event(EventDescriptor {
            name: "1bad".to_string(), // bad pattern
            schema: EventSchema {
                fields: fields.clone(),
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        });
        let node1 = PayloadNode::Event(EventDescriptor {
            name: "EmptySchema".to_string(),
            schema: EventSchema {
                fields: BTreeMap::new(), // empty → triggers EventSchemaEmpty
                optional_fields: vec![],
            },
            dedupe_key: None,
            dedupe_window_ms: None,
            keep_events_for_ms: None,
            cold_after_ms: None,
            registered_at_version: 0,
            name_arc: Arc::from(""),
            apply_field_names: vec![],
        });
        let node2 = PayloadNode::Derivation(DerivationDescriptor {
            name: "EmptyDeriv".to_string(),
            output_kind: OutputKind::Event,
            upstreams: vec![],
            ops: vec![],
            schema: DerivedSchema {
                fields: BTreeMap::new(),
                optional_fields: vec![],
            },
            table_primary_key: None,
            registered_at_version: 0,
        });
        let errs = validate_payload(&empty_current(), vec![node0, node1, node2])
            .expect_err("expected Err");
        assert!(
            errs.len() >= 3,
            "expected at least 3 errors (one per bad node), got {}: {errs:#?}",
            errs.len()
        );
        // Verify paths are distinct
        let paths: std::collections::HashSet<&str> = errs.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.len() >= 3, "paths should be distinct, got: {paths:?}");
    }

    // ── Rule 5: Upstream resolution ───────────────────────────────────────────

    #[test]
    fn rule5_pass_upstream_in_payload() {
        assert_ok(vec![minimal_event("A"), minimal_derivation("D", vec!["A"])]);
    }

    #[test]
    fn rule5_pass_upstream_in_current_registry() {
        let mut current = RegistryInner::default();
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        current.events.insert(
            "A".to_string(),
            Arc::new(EventDescriptor {
                name: "A".to_string(),
                schema: EventSchema {
                    fields,
                    optional_fields: vec![],
                },
                dedupe_key: None,
                dedupe_window_ms: None,
                keep_events_for_ms: None,
                cold_after_ms: None,
                registered_at_version: 1,
                name_arc: Arc::from(""),
                apply_field_names: vec![],
            }),
        );
        current.version = 1;
        let result = validate_payload(&current, vec![minimal_derivation("D", vec!["A"])]);
        assert!(
            result.is_ok(),
            "upstream in current registry should pass: {result:?}"
        );
    }

    #[test]
    fn rule5_fail_upstream_unknown() {
        assert_err_contains(
            vec![minimal_derivation("D", vec!["Missing"])],
            ErrorCode::DerivationUpstreamUnknown,
            "nodes[0].upstreams[0]",
        );
    }

    #[test]
    fn rule5_fail_second_upstream_missing() {
        let mut current = RegistryInner::default();
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        current.events.insert(
            "KnownA".to_string(),
            Arc::new(EventDescriptor {
                name: "KnownA".to_string(),
                schema: EventSchema {
                    fields: fields.clone(),
                    optional_fields: vec![],
                },
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
            "KnownB".to_string(),
            Arc::new(EventDescriptor {
                name: "KnownB".to_string(),
                schema: EventSchema {
                    fields,
                    optional_fields: vec![],
                },
                dedupe_key: None,
                dedupe_window_ms: None,
                keep_events_for_ms: None,
                cold_after_ms: None,
                registered_at_version: 1,
                name_arc: Arc::from(""),
                apply_field_names: vec![],
            }),
        );
        // D has 3 upstreams: KnownA (ok), KnownB (ok), Missing (bad)
        let errs = validate_payload(
            &current,
            vec![minimal_derivation("D", vec!["KnownA", "KnownB", "Missing"])],
        )
        .expect_err("expected Err");
        assert_eq!(errs.len(), 1, "only one error for one missing upstream");
        assert_eq!(errs[0].code, ErrorCode::DerivationUpstreamUnknown);
        assert!(errs[0].path.contains("upstreams[2]")); // index 2
    }

    // ── Rule 7: Acyclicity ────────────────────────────────────────────────────

    #[test]
    fn rule7_pass_linear_chain() {
        assert_ok(vec![
            minimal_event("A"),
            minimal_derivation("D1", vec!["A"]),
            minimal_derivation("D2", vec!["D1"]),
            minimal_derivation("D3", vec!["D2"]),
        ]);
    }

    #[test]
    fn rule7_fail_two_node_cycle() {
        let errs = validate_payload(
            &empty_current(),
            vec![
                minimal_derivation("D1", vec!["D2"]),
                minimal_derivation("D2", vec!["D1"]),
            ],
        )
        .expect_err("expected cycle error");
        let found = errs.iter().any(|e| e.code == ErrorCode::RegistrationCycle);
        assert!(found, "expected RegistrationCycle error, got: {errs:#?}");
        let cycle_err = errs
            .iter()
            .find(|e| e.code == ErrorCode::RegistrationCycle)
            .unwrap();
        assert!(
            cycle_err.reason.contains("D1") || cycle_err.reason.contains("D2"),
            "cycle reason should name the nodes: {}",
            cycle_err.reason
        );
    }

    #[test]
    fn rule7_fail_self_loop() {
        let errs = validate_payload(&empty_current(), vec![minimal_derivation("D1", vec!["D1"])])
            .expect_err("expected cycle error");
        assert!(errs.iter().any(|e| e.code == ErrorCode::RegistrationCycle));
    }

    #[test]
    fn rule7_fail_three_node_cycle() {
        // A → B → C → A (all in payload)
        let errs = validate_payload(
            &empty_current(),
            vec![
                minimal_derivation("A", vec!["C"]),
                minimal_derivation("B", vec!["A"]),
                minimal_derivation("C", vec!["B"]),
            ],
        )
        .expect_err("expected cycle");
        assert!(errs.iter().any(|e| e.code == ErrorCode::RegistrationCycle));
    }

    // ── Rule 8: Topological order ─────────────────────────────────────────────

    #[test]
    fn rule8_pass_correct_order() {
        assert_ok(vec![minimal_event("A"), minimal_derivation("D", vec!["A"])]);
    }

    #[test]
    fn rule8_fail_dependent_before_upstream() {
        let errs = validate_payload(
            &empty_current(),
            vec![
                // D appears at index 0, but its upstream A appears at index 1
                minimal_derivation("D", vec!["A"]),
                minimal_event("A"),
            ],
        )
        .expect_err("expected TopologicalOrderViolation");
        let found = errs
            .iter()
            .any(|e| e.code == ErrorCode::TopologicalOrderViolation);
        assert!(found, "expected TopologicalOrderViolation: {errs:#?}");
        let topo_err = errs
            .iter()
            .find(|e| e.code == ErrorCode::TopologicalOrderViolation)
            .unwrap();
        assert!(
            topo_err.reason.contains("A") && topo_err.reason.contains("nodes[1]"),
            "reason should mention 'A' and 'nodes[1]': {}",
            topo_err.reason
        );
    }

    // ── Rule 7+8 cooperate ────────────────────────────────────────────────────

    #[test]
    fn rule7_and_rule8_cooperate() {
        // D1 at index 0 depends on D2 (which is at index 1): both cycle AND topo violation
        let errs = validate_payload(
            &empty_current(),
            vec![
                minimal_derivation("D1", vec!["D2"]),
                minimal_derivation("D2", vec!["D1"]),
            ],
        )
        .expect_err("expected errors");
        let has_cycle = errs.iter().any(|e| e.code == ErrorCode::RegistrationCycle);
        let has_topo = errs
            .iter()
            .any(|e| e.code == ErrorCode::TopologicalOrderViolation);
        assert!(has_cycle, "expected RegistrationCycle");
        assert!(has_topo, "expected TopologicalOrderViolation");
    }

    // ── Rule 10: Expression validation ───────────────────────────────────────
    // These tests assert on new Phase 4 behavior. They call validate_payload
    // with derivations that have ops, and expect expression-level errors.
    // Currently these tests FAIL because Rule 10 is not yet implemented —
    // that is the intended "red" state for TDD phase 04-05.

    fn event_with_amount(name: &str) -> PayloadNode {
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("amount".to_string(), FieldType::F64);
        PayloadNode::Event(EventDescriptor {
            name: name.to_string(),
            schema: EventSchema {
                fields,
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

    fn derivation_with_ops(
        name: &str,
        upstreams: Vec<&str>,
        ops: Vec<crate::op_node::OpNode>,
        schema_fields: Vec<(&str, FieldType)>,
    ) -> PayloadNode {
        let fields: BTreeMap<String, FieldType> = schema_fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        PayloadNode::Derivation(DerivationDescriptor {
            name: name.to_string(),
            output_kind: OutputKind::Event,
            upstreams: upstreams.iter().map(|s| s.to_string()).collect(),
            ops,
            schema: DerivedSchema {
                fields,
                optional_fields: vec![],
            },
            table_primary_key: None,
            registered_at_version: 0,
        })
    }

    /// Rule 10 test 1: Filter with parse error → InvalidExpression at nodes[1].ops[0].expr
    #[test]
    fn rule10_fail_filter_with_parse_error() {
        use crate::op_node::OpNode;
        let payload = vec![
            event_with_amount("A"),
            derivation_with_ops(
                "D",
                vec!["A"],
                vec![OpNode::Filter {
                    expr: "(amount > ".to_string(), // truncated — parse error
                }],
                vec![("amount", FieldType::F64)],
            ),
        ];
        let errs = validate_payload(&empty_current(), payload).expect_err("expected Err");
        let found = errs
            .iter()
            .any(|e| e.code == ErrorCode::InvalidExpression && e.path.contains("nodes[1].ops[0]"));
        assert!(
            found,
            "expected InvalidExpression at nodes[1].ops[0], got: {errs:#?}"
        );
    }

    /// Rule 10 test 2: Filter with unknown field → UnknownFieldReference
    #[test]
    fn rule10_fail_filter_references_unknown_field() {
        use crate::op_node::OpNode;
        let payload = vec![
            event_with_amount("A"),
            derivation_with_ops(
                "D",
                vec!["A"],
                vec![OpNode::Filter {
                    expr: "(missing > 0)".to_string(),
                }],
                vec![("amount", FieldType::F64)],
            ),
        ];
        let errs = validate_payload(&empty_current(), payload).expect_err("expected Err");
        let found = errs.iter().any(|e| {
            e.code == ErrorCode::UnknownFieldReference
                && e.path.contains("nodes[1].ops[0]")
                && e.reason.contains("missing")
        });
        assert!(
            found,
            "expected UnknownFieldReference mentioning 'missing' at nodes[1].ops[0], got: {errs:#?}"
        );
    }

    /// Rule 10 test 3: WithColumns type mismatch → SchemaPropagationFailure
    #[test]
    fn rule10_fail_with_columns_type_mismatch() {
        use crate::op_node::OpNode;
        let mut exprs = std::collections::BTreeMap::new();
        // "amount and true" — boolean op on F64 amount → TypeMismatch
        exprs.insert("bad".to_string(), "(amount and true)".to_string());
        let payload = vec![
            event_with_amount("A"),
            derivation_with_ops(
                "D",
                vec!["A"],
                vec![OpNode::WithColumns { exprs }],
                vec![("amount", FieldType::F64), ("bad", FieldType::Bool)],
            ),
        ];
        let errs = validate_payload(&empty_current(), payload).expect_err("expected Err");
        let found = errs.iter().any(|e| {
            e.code == ErrorCode::SchemaPropagationFailure && e.path.contains("nodes[1].ops[0]")
        });
        assert!(
            found,
            "expected SchemaPropagationFailure at nodes[1].ops[0], got: {errs:#?}"
        );
    }

    /// Rule 10 test 4: Cast with invalid target → SchemaPropagationFailure
    #[test]
    fn rule10_fail_cast_invalid_target() {
        use crate::op_node::OpNode;
        let mut type_map = std::collections::BTreeMap::new();
        type_map.insert("amount".to_string(), "blob".to_string()); // unknown cast target
        let payload = vec![
            event_with_amount("A"),
            derivation_with_ops(
                "D",
                vec!["A"],
                vec![OpNode::Cast { type_map }],
                vec![("amount", FieldType::F64)],
            ),
        ];
        let errs = validate_payload(&empty_current(), payload).expect_err("expected Err");
        let found = errs.iter().any(|e| {
            (e.code == ErrorCode::SchemaPropagationFailure
                || e.code == ErrorCode::InvalidCastTarget)
                && e.path.contains("nodes[1].ops[0]")
        });
        assert!(
            found,
            "expected SchemaPropagationFailure or InvalidCastTarget at nodes[1].ops[0], got: {errs:#?}"
        );
    }

    /// Rule 10 test 5: Select with unknown field → SchemaPropagationFailure or UnknownFieldReference
    #[test]
    fn rule10_fail_select_unknown_field() {
        use crate::op_node::OpNode;
        let payload = vec![
            event_with_amount("A"),
            derivation_with_ops(
                "D",
                vec!["A"],
                vec![OpNode::Select {
                    fields: vec!["missing".to_string()],
                }],
                vec![("amount", FieldType::F64)],
            ),
        ];
        let errs = validate_payload(&empty_current(), payload).expect_err("expected Err");
        let found = errs.iter().any(|e| {
            (e.code == ErrorCode::SchemaPropagationFailure
                || e.code == ErrorCode::UnknownFieldReference)
                && e.path.contains("nodes[1].ops[0]")
        });
        assert!(
            found,
            "expected SchemaPropagationFailure or UnknownFieldReference at nodes[1].ops[0], got: {errs:#?}"
        );
    }

    /// Rule 10 test 6: Valid filter + with_columns chain → Ok
    #[test]
    fn rule10_pass_valid_filter_and_with_columns_chain() {
        use crate::op_node::OpNode;
        let mut exprs = std::collections::BTreeMap::new();
        exprs.insert("is_big".to_string(), "(amount > 500)".to_string());
        let payload = vec![
            event_with_amount("A"),
            derivation_with_ops(
                "D",
                vec!["A"],
                vec![
                    OpNode::Filter {
                        expr: "(amount > 100)".to_string(),
                    },
                    OpNode::WithColumns { exprs },
                ],
                vec![("amount", FieldType::F64), ("is_big", FieldType::Bool)],
            ),
        ];
        let result = validate_payload(&empty_current(), payload);
        assert!(
            result.is_ok(),
            "valid filter+with_columns chain should pass Rule 10: {result:#?}"
        );
    }

    /// Rule 10 test 7: Server must propagate schema and replace client-supplied schema
    /// The derivation declares schema={amount: F64} but propagation infers
    /// {amount: F64, is_big: Bool} after the WithColumns op.
    /// After a successful register, GET /registry shows the propagated schema.
    /// At the validate_payload level: Ok(ValidatedPayload) is returned;
    /// the propagated_schemas field carries the corrected schema.
    #[test]
    fn rule10_client_supplied_derivation_schema_must_match_propagated() {
        use crate::op_node::OpNode;
        let mut exprs = std::collections::BTreeMap::new();
        exprs.insert("is_big".to_string(), "(amount > 500)".to_string());
        // Client says schema is {amount: F64} — missing is_big
        let payload = vec![
            event_with_amount("A"),
            derivation_with_ops(
                "D",
                vec!["A"],
                vec![OpNode::WithColumns {
                    exprs: exprs.clone(),
                }],
                vec![("amount", FieldType::F64)], // wrong: missing is_big
            ),
        ];
        let result = validate_payload(&empty_current(), payload).expect("should be Ok");
        // The propagated_schemas should contain {amount: F64, is_big: Bool}
        let propagated = result
            .propagated_schemas
            .iter()
            .find(|(name, _)| name == "D");
        assert!(propagated.is_some(), "D must have a propagated schema");
        let (_, schema) = propagated.unwrap();
        assert_eq!(
            schema.fields.get("is_big"),
            Some(&FieldType::Bool),
            "propagated schema for D must include is_big: Bool"
        );
        assert_eq!(
            schema.fields.get("amount"),
            Some(&FieldType::F64),
            "propagated schema for D must include amount: F64"
        );
    }

    /// Rule 10 test 8: Empty ops passes trivially
    #[test]
    fn rule10_empty_ops_is_fine() {
        let payload = vec![
            event_with_amount("A"),
            minimal_derivation("D", vec!["A"]), // has no ops
        ];
        assert_ok(payload);
    }

    /// Rule 10 test 9: Two bad ops collect two errors (fail-soft)
    #[test]
    fn rule10_fail_soft_collects_expression_errors_per_op() {
        use crate::op_node::OpNode;
        let mut type_map = std::collections::BTreeMap::new();
        type_map.insert("amount".to_string(), "blob".to_string());
        let payload = vec![
            event_with_amount("A"),
            derivation_with_ops(
                "D",
                vec!["A"],
                vec![
                    OpNode::Filter {
                        expr: "(missing > 0)".to_string(), // UnknownFieldReference at op[0]
                    },
                    OpNode::Cast { type_map }, // SchemaPropagationFailure at op[1]
                ],
                vec![("amount", FieldType::F64)],
            ),
        ];
        let errs = validate_payload(&empty_current(), payload).expect_err("expected Err");
        // Should have at least 2 errors: one for op[0], one for op[1]
        let op0_err = errs
            .iter()
            .any(|e| e.path.contains("nodes[1].ops[0]") && e.path.contains("nodes[1].ops[0]"));
        let op1_err = errs.iter().any(|e| e.path.contains("nodes[1].ops[1]"));
        assert!(op0_err, "expected error at nodes[1].ops[0], got: {errs:#?}");
        assert!(op1_err, "expected error at nodes[1].ops[1], got: {errs:#?}");
    }

    /// Rule 10 test 10: Events and tables with no ops field are not touched by Rule 10
    #[test]
    fn rule10_event_or_table_with_ops_is_ignored() {
        // Events and tables don't have ops in Phase 2's wire shape.
        // Verify the validator ignores non-derivation nodes in Rule 10.
        let payload = vec![event_with_amount("A"), minimal_table("T", vec!["extra"])];
        assert_ok(payload);
    }

    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn multiple_upstreams_partial_missing() {
        // D with 3 upstreams: KnownA, KnownB, Missing
        let mut current = RegistryInner::default();
        let mut fields = BTreeMap::new();
        fields.insert("event_time".to_string(), FieldType::I64);
        fields.insert("x".to_string(), FieldType::F64);
        current.events.insert(
            "KnownA".to_string(),
            Arc::new(EventDescriptor {
                name: "KnownA".to_string(),
                schema: EventSchema {
                    fields: fields.clone(),
                    optional_fields: vec![],
                },
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
            "KnownB".to_string(),
            Arc::new(EventDescriptor {
                name: "KnownB".to_string(),
                schema: EventSchema {
                    fields,
                    optional_fields: vec![],
                },
                dedupe_key: None,
                dedupe_window_ms: None,
                keep_events_for_ms: None,
                cold_after_ms: None,
                registered_at_version: 1,
                name_arc: Arc::from(""),
                apply_field_names: vec![],
            }),
        );
        assert_err_contains_with_current(
            &current,
            vec![minimal_derivation("D", vec!["KnownA", "KnownB", "Missing"])],
            ErrorCode::DerivationUpstreamUnknown,
            "upstreams[2]",
        );
    }
}

// ─── classify_register_diff unit tests ───────────────────────────────────────
//
// `classify_register_diff` is the canonical diff after the Plan 06 dual-system
// consolidation: it returns `additive`, `destructive`, AND `already_present`,
// replacing the legacy `compute_diff`. These tests cover the cases the
// deleted `compute_diff` unit tests covered, plus the new `already_present`
// invariants.

#[cfg(test)]
mod tests_classify_register_diff {
    use super::*;
    use crate::registry::EventDescriptor;
    use crate::schema::{EventSchema, FieldType};
    use std::collections::BTreeMap;

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

    fn event_schema_amount_f64() -> EventSchema {
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

    #[test]
    fn empty_payload_against_empty_current_yields_empty_diff() {
        let prev = RegistryInner::default();
        let diff = classify_register_diff(&prev, &[]);
        assert!(diff.additive.is_empty());
        assert!(diff.destructive.is_empty());
        assert!(diff.already_present.is_empty());
    }

    #[test]
    fn brand_new_descriptor_is_additive_not_already_present() {
        let prev = RegistryInner::default();
        let payload = vec![event_node("A", event_schema_amount_f64())];
        let diff = classify_register_diff(&prev, &payload);
        assert_eq!(diff.additive.len(), 1);
        assert!(matches!(
            &diff.additive[0],
            DiffEntry::NewDescriptor { name, .. } if name == "A"
        ));
        assert!(diff.destructive.is_empty());
        assert!(diff.already_present.is_empty());
    }

    #[test]
    fn identical_descriptor_is_already_present_only() {
        let schema = event_schema_amount_f64();
        let prev = registry_with_event("A", schema.clone());
        let payload = vec![event_node("A", schema)];
        let diff = classify_register_diff(&prev, &payload);
        assert!(diff.additive.is_empty(), "additive: {:?}", diff.additive);
        assert!(
            diff.destructive.is_empty(),
            "destructive: {:?}",
            diff.destructive
        );
        assert_eq!(diff.already_present, vec!["A".to_string()]);
    }

    #[test]
    fn added_field_on_existing_event_is_additive_not_already_present() {
        let prev = registry_with_event("A", event_schema_amount_f64());
        let mut new_schema = event_schema_amount_f64();
        new_schema
            .fields
            .insert("session_id".to_string(), FieldType::Str);
        let payload = vec![event_node("A", new_schema)];
        let diff = classify_register_diff(&prev, &payload);
        // NewField on the existing event source — additive, NOT already_present.
        assert!(matches!(
            diff.additive.as_slice(),
            [DiffEntry::NewField { event, field, .. }] if event == "A" && field == "session_id"
        ));
        assert!(diff.destructive.is_empty());
        assert!(
            diff.already_present.is_empty(),
            "modified descriptor must not appear in already_present, got: {:?}",
            diff.already_present
        );
    }

    #[test]
    fn type_change_on_existing_event_is_destructive_not_already_present() {
        let prev = registry_with_event("A", event_schema_amount_f64());
        let mut new_schema = event_schema_amount_f64();
        // f64 → i64 on `amount`
        new_schema
            .fields
            .insert("amount".to_string(), FieldType::I64);
        let payload = vec![event_node("A", new_schema)];
        let diff = classify_register_diff(&prev, &payload);
        assert!(diff.additive.is_empty());
        assert!(matches!(
            diff.destructive.as_slice(),
            [DiffEntry::TypeChange { field, .. }] if field == "A.amount"
        ));
        assert!(diff.already_present.is_empty());
    }

    #[test]
    fn mixed_payload_partitions_correctly() {
        // Current: A (amount: f64). Payload: A unchanged + B brand-new + C
        // with a renamed field. Expected: additive has NewDescriptor(B); B
        // and C don't appear in already_present (B is new, C is changed); A
        // appears in already_present.
        let prev = registry_with_event("A", event_schema_amount_f64());
        let payload = vec![
            event_node("A", event_schema_amount_f64()),
            event_node("B", event_schema_amount_f64()),
        ];
        let diff = classify_register_diff(&prev, &payload);
        assert_eq!(diff.additive.len(), 1);
        assert!(matches!(
            &diff.additive[0],
            DiffEntry::NewDescriptor { name, .. } if name == "B"
        ));
        assert!(diff.destructive.is_empty());
        assert_eq!(diff.already_present, vec!["A".to_string()]);
    }
}

// ─── Phase 12.7 Plan 01 — pre_check_unsupported_node_kind unit tests ──────────

#[cfg(test)]
mod tests_pre_check_unsupported_node_kind {
    use super::*;

    #[test]
    fn pre_check_unsupported_node_kind_rejects_table_node() {
        let body = serde_json::json!({"nodes": [{"kind": "table", "name": "Users"}]});
        let err = pre_check_unsupported_node_kind(&body).expect("table kind should be rejected");
        assert_eq!(err.code, "unsupported_node_kind");
        assert_eq!(err.op_label, "table");
        assert_eq!(err.path, "nodes[0].Users.kind");
        assert!(err.reason.contains("not supported in v0"));
        assert!(err.reason.contains("events-only"));
    }

    #[test]
    fn pre_check_unsupported_node_kind_passes_event_and_derivation() {
        let body = serde_json::json!({"nodes": [
            {"kind": "event", "name": "Tx"},
            {"kind": "derivation", "name": "Filtered"}
        ]});
        assert!(pre_check_unsupported_node_kind(&body).is_none());
    }

    #[test]
    fn pre_check_unsupported_node_kind_returns_first_offender() {
        let body = serde_json::json!({"nodes": [
            {"kind": "event", "name": "Tx"},
            {"kind": "table", "name": "Users"},
            {"kind": "derivation", "name": "Filtered"}
        ]});
        let err = pre_check_unsupported_node_kind(&body).unwrap();
        assert_eq!(err.path, "nodes[1].Users.kind");
    }

    #[test]
    fn pre_check_unsupported_node_kind_handles_unnamed_node() {
        // No `name` field on the node — path falls back to the index-only form.
        let body = serde_json::json!({"nodes": [{"kind": "table"}]});
        let err = pre_check_unsupported_node_kind(&body).unwrap();
        assert_eq!(err.code, "unsupported_node_kind");
        assert_eq!(err.path, "nodes[0].kind");
    }

    #[test]
    fn pre_check_unsupported_node_kind_skips_empty_kind() {
        // Empty kind is left for strict serde to surface as "missing field `kind`".
        // The shim must not emit a structured error here.
        let body = serde_json::json!({"nodes": [{"name": "X"}]});
        assert!(pre_check_unsupported_node_kind(&body).is_none());
    }

    #[test]
    fn pre_check_unsupported_node_kind_returns_none_on_empty_payload() {
        let body = serde_json::json!({});
        assert!(pre_check_unsupported_node_kind(&body).is_none());
        let body = serde_json::json!({"nodes": []});
        assert!(pre_check_unsupported_node_kind(&body).is_none());
    }
}

// ─── Unit tests: pre_check_unbounded_op_in_lifetime_mode (Phase 12.8 Plan 01) ──

#[cfg(test)]
mod tests_pre_check_unbounded_op_in_lifetime_mode {
    use super::*;

    /// Helper: build a derivation-node payload with a single group_by op whose
    /// `agg` map carries one named feature. Caller controls `op_str` (e.g.
    /// "count") and optional window.
    fn payload_with_op(
        deriv_name: Option<&str>,
        feature_name: &str,
        op_str: &str,
        with_window: bool,
    ) -> serde_json::Value {
        let params = if with_window {
            serde_json::json!({"window": "60s"})
        } else {
            serde_json::json!({})
        };
        let mut deriv = serde_json::json!({
            "kind": "derivation",
            "output_kind": "event",
            "upstreams": ["Tx"],
            "ops": [
                {
                    "op": "group_by",
                    "keys": ["user_id"],
                    "agg": {
                        feature_name: {"op": op_str, "params": params}
                    }
                }
            ],
            "schema": {"fields": {"user_id": "str", feature_name: "i64"}, "optional_fields": []}
        });
        if let Some(name) = deriv_name {
            deriv["name"] = serde_json::Value::String(name.to_string());
        }
        serde_json::json!({
            "nodes": [
                {
                    "kind": "event",
                    "name": "Tx",
                    "schema": {"fields": {"user_id": "str", "amount": "f64"}, "optional_fields": []}
                },
                deriv
            ]
        })
    }

    /// Sentinel op-string that does not appear in `lifetime_bound_for_op_str`
    /// — used by the rejection-tests below as a stand-in for any genuinely-
    /// unclassified op. Earlier tests used "count", but the classifier
    /// labels count as O1 — these tests switched to a typo sentinel that
    /// round-trips through `Unbounded` deterministically.
    const UNCLASSIFIED_OP: &str = "nonexistent_op_zzz";

    #[test]
    fn pre_check_unbounded_op_returns_some_for_unbounded_op() {
        // Only genuinely-unclassified op-strings hit Unbounded. The sentinel
        // `nonexistent_op_zzz` is the catch-all canary.
        let body = payload_with_op(Some("ByUser"), "cnt", UNCLASSIFIED_OP, false);
        let err = pre_check_unbounded_op_in_lifetime_mode(&body)
            .expect("windowless unclassified op should be rejected (catch-all → Unbounded)");
        assert_eq!(err.code, "unbounded_op_in_lifetime_mode");
        assert_eq!(err.op_label, UNCLASSIFIED_OP);
        assert_eq!(err.path, "nodes[1].ByUser.ops[0].agg.cnt");
        assert!(
            err.reason.contains("requires explicit memory bound in v0"),
            "reason should contain the v0 framing, got: {}",
            err.reason
        );
        assert!(
            err.reason.contains(UNCLASSIFIED_OP),
            "reason should name the op, got: {}",
            err.reason
        );
    }

    #[test]
    fn pre_check_unbounded_op_skips_windowed_op() {
        // params.window present → windowed path is naturally bounded by the
        // 64-bucket cap; the shim must skip and return None.
        // (Tested with the unclassified sentinel — even unclassified ops
        // bypass the bound check when windowed.)
        let body = payload_with_op(Some("ByUser"), "cnt_60s", UNCLASSIFIED_OP, true);
        assert!(
            pre_check_unbounded_op_in_lifetime_mode(&body).is_none(),
            "windowed op should bypass the bound check (even when op-string is \
             unclassified — the windowed runtime imposes its own 64-bucket cap)"
        );
    }

    #[test]
    fn pre_check_unbounded_op_returns_none_on_empty_payload() {
        let body = serde_json::json!({});
        assert!(pre_check_unbounded_op_in_lifetime_mode(&body).is_none());
        let body = serde_json::json!({"nodes": []});
        assert!(pre_check_unbounded_op_in_lifetime_mode(&body).is_none());
        // Event-only payload (no derivation) — no ops to classify.
        let body = serde_json::json!({
            "nodes": [
                {
                    "kind": "event",
                    "name": "Tx",
                    "schema": {"fields": {"user_id": "str"}, "optional_fields": []}
                }
            ]
        });
        assert!(pre_check_unbounded_op_in_lifetime_mode(&body).is_none());
    }

    #[test]
    fn pre_check_unbounded_op_path_format_named_derivation() {
        // Named derivation → path is `nodes[N].<name>.ops[K].agg.<feature>`.
        let body = payload_with_op(Some("MyDeriv"), "feat_a", UNCLASSIFIED_OP, false);
        let err = pre_check_unbounded_op_in_lifetime_mode(&body).unwrap();
        assert_eq!(err.path, "nodes[1].MyDeriv.ops[0].agg.feat_a");
    }

    #[test]
    fn pre_check_unbounded_op_path_format_unnamed_derivation() {
        // Unnamed derivation (no `name` field) → path falls back to
        // `nodes[N].ops[K].agg.<feature>` (no derivation-name segment).
        let body = payload_with_op(None, "feat_a", UNCLASSIFIED_OP, false);
        let err = pre_check_unbounded_op_in_lifetime_mode(&body).unwrap();
        assert_eq!(err.path, "nodes[1].ops[0].agg.feat_a");
    }
}
