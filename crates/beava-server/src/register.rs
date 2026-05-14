//! POST /register endpoint — parse, validate, diff, install, respond.
//!
//! Pipeline:
//! 1. Content-Type check → 415
//! 2. JSON parse → 400
//! 3. Snapshot current registry for validation + diff
//! 4. validate_payload → 400
//! 5. classify_register_diff (apply_shard's pre-flight has already
//!    pre-removed any descriptor whose shape would change with force=true,
//!    or returned 409 force_required without force; destructive entries
//!    here are an invariant violation → 503)
//! 6. No-op (no NewDescriptor entries in additive) → 200 same version
//! 7. Additive install → 200 new version

use beava_core::{
    register_validate::validate_payload, register_validate::ErrorCode, registry::Registry,
    registry_diff::PayloadNode,
};
use beava_persistence::{PersistError, RecordType, WalSink};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

/// WAL record carrying a registration bump. Persisted before the in-memory
/// registry mutates (apply-after-fsync).
///
/// Encoded with `serde_json` rather than `bincode`: `PayloadNode` carries
/// `serde_json::Value` fields (`AggSpec.params`, `OpNode::Fillna.defaults`)
/// that bincode 1.x cannot round-trip (`DeserializeAnyNotSupported`).
/// `/register` is cold-path so the size delta is irrelevant; JSON also gives
/// recovery a self-describing, forward/backward-compatible payload as
/// descriptor shapes evolve.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryBumpPayload {
    /// New version number (post-bump). For replay diagnostics only — does
    /// not override the registry's monotonic version assignment.
    pub new_version: u64,
    /// Validated `PayloadNode`s that produced this bump.
    pub payload_nodes: Vec<PayloadNode>,
    /// Descriptor names force-removed by `apply_shard`'s force-handling
    /// block before this bump was applied (post-cascade closure). On
    /// recovery, `apply_registry_bump` replays this list through
    /// `force_remove_descriptors` BEFORE re-validating + re-applying
    /// `payload_nodes` — without this, WAL replay rebuilds pre-replace
    /// state from prior records and silently overrides the force boundary.
    ///
    /// Empty on non-force registers. `#[serde(default)]` keeps the JSON
    /// codec forward/backward compatible with WAL records written before
    /// this field existed — they decode with an empty Vec → no
    /// force-removal on replay, identical to prior behaviour.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub force_removed_descriptors: Vec<String>,
}

impl RegistryBumpPayload {
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// Re-apply a recovered RegistryBump record to the in-memory registry.
///
/// 1. If `bump.force_removed_descriptors` is non-empty, replay those
///    removals through `force_remove_descriptors`. The live-path cascade
///    closure (in `apply_shard.rs`) is already baked into this list, so
///    recovery does NOT recompute cascade — it just replays.
/// 2. Re-run validation + compile to rebuild caches against the
///    post-removal snapshot.
/// 3. Call `apply_registration`. Idempotent on the descriptor set: if a
///    node is already present, it is left in place.
pub fn apply_registry_bump(
    registry: &Arc<Registry>,
    bump: RegistryBumpPayload,
) -> Result<(), String> {
    if !bump.force_removed_descriptors.is_empty() {
        registry.force_remove_descriptors(&bump.force_removed_descriptors);
    }
    let snapshot = registry.snapshot();
    let validated = match validate_payload(&snapshot, bump.payload_nodes) {
        Ok(v) => v,
        Err(errs) => {
            return Err(format!(
                "validation failed during recovery (first error: {:?})",
                errs.first().map(|e| &e.reason)
            ));
        }
    };
    let (nodes, compiled_chains, propagated_schemas, compiled_aggregations) =
        validated.into_parts();
    registry.apply_registration(
        nodes,
        compiled_chains,
        propagated_schemas,
        compiled_aggregations,
    );
    Ok(())
}

/// Errors specific to the WAL-backed register pipeline.
#[derive(Debug)]
pub enum RegisterWalError {
    Encode(String),
    Persist(PersistError),
}

impl std::fmt::Display for RegisterWalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterWalError::Encode(s) => write!(f, "encode: {s}"),
            RegisterWalError::Persist(e) => write!(f, "persist: {e}"),
        }
    }
}

/// Wire shape of `POST /register` request body.
#[derive(Debug, Deserialize)]
pub struct RegisterPayload {
    pub nodes: Vec<PayloadNode>,
}

/// Shared axum state.
#[derive(Clone)]
pub struct RegisterAppState {
    pub registry: Arc<Registry>,
    /// When `Some`, `/register` writes a `RegistryBump` WAL record before
    /// mutating the in-memory registry. `None` for legacy tests with no
    /// WAL plumbing.
    pub wal_sink: Option<WalSink>,
}

#[derive(Debug, Serialize)]
pub struct RegisterSuccess {
    pub status: &'static str, // always "ok"
    pub registry_version: u64,
    pub registered_descriptors: Vec<String>, // input order
    pub added: Vec<String>,
    pub already_present: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RegisterErrorBody {
    pub error: RegisterError,
    pub registry_version: u64,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RegisterError {
    Validation {
        code: &'static str, // "invalid_registration"
        path: String,
        reason: String,
    },
    UnsupportedMediaType {
        code: &'static str, // "unsupported_media_type"
        path: String,
        reason: String,
    },
}

/// Transport-agnostic register outcome — HTTP's response builder and the TCP
/// frame encoder both consume it.
///
/// Destructive-change detection lives in apply_shard's pre-flight
/// (`register_check_force_required`); without `force=true` it returns a
/// 409 `force_required` envelope before this module's outcome enum is
/// constructed. This enum carries only the post-pre-flight outcomes.
#[derive(Debug)]
pub enum RegisterOutcome {
    /// Additive install succeeded; version bumped.
    Success {
        version: u64,
        registered_descriptors: Vec<String>,
        added: Vec<String>,
        already_present: Vec<String>,
    },
    /// Payload was empty (zero nodes). 200 OK, version unchanged.
    EmptyPayload { version: u64 },
    /// Every node already present, none added. 200 OK, version unchanged.
    Noop {
        version: u64,
        registered_descriptors: Vec<String>,
        already_present: Vec<String>,
    },
    /// validate_payload returned at least one error. 400.
    /// v0 exposes "first-error-wins" on the wire; full Vec is logged at WARN.
    ValidationFailed {
        version: u64,
        // reason: structured fields recorded for future logging/metrics; the
        // wire response only emits first_error_path / first_error_reason in
        // v0 (first-error-wins).
        #[allow(dead_code)]
        first_error_code: ErrorCode,
        first_error_path: String,
        first_error_reason: String,
        // reason: see `first_error_code` above — recorded for future use.
        #[allow(dead_code)]
        all_errors_count: usize,
    },
    /// WAL append for the `RegistryBump` record failed, OR apply_shard's
    /// pre-flight invariant was violated (destructive entries reached
    /// `execute_register_inner` despite `register_check_force_required`'s
    /// gate). 503.
    WalUnavailable { version: u64 },
}

/// WAL-backed entry point. When `wal_sink` is `Some` a `RegistryBump` record
/// is written and fsynced BEFORE the in-memory registry mutates (apply-after-
/// fsync). On WAL failure returns `WalUnavailable` so HTTP maps to 503.
///
/// `force_removed` carries the post-cascade list of descriptors that
/// `apply_shard`'s force-handling block removed before delegating here.
/// It is recorded verbatim in the `RegistryBump` payload so recovery
/// replays the same removal step before re-validating the new payload —
/// without this, WAL replay rebuilds the pre-removal state and silently
/// loses the force boundary.
pub(crate) async fn execute_register_with_wal(
    registry: &Arc<Registry>,
    payload: RegisterPayload,
    wal_sink: &WalSink,
    force_removed: Vec<String>,
) -> RegisterOutcome {
    execute_register_inner(registry, payload, Some(wal_sink), force_removed).await
}

async fn execute_register_inner(
    registry: &Arc<Registry>,
    payload: RegisterPayload,
    wal_sink: Option<&WalSink>,
    force_removed: Vec<String>,
) -> RegisterOutcome {
    if payload.nodes.is_empty() {
        let version = registry.version();
        info!(
            kind = "register.noop",
            version,
            nodes = 0,
            "register empty payload"
        );
        return RegisterOutcome::EmptyPayload { version };
    }

    let current_snapshot = registry.snapshot();

    // Validate fail-soft: collects every error before returning so the
    // operator gets the full diagnostic set.
    let validated = match validate_payload(&current_snapshot, payload.nodes) {
        Ok(v) => v,
        Err(errs) => {
            let first = &errs[0];
            warn!(
                kind = "register.validation_failed",
                path = %first.path,
                code = ?first.code,
                error_count = errs.len(),
                "register validation failed"
            );
            return RegisterOutcome::ValidationFailed {
                version: current_snapshot.version,
                first_error_code: first.code,
                first_error_path: first.path.clone(),
                first_error_reason: first.reason.clone(),
                all_errors_count: errs.len(),
            };
        }
    };

    let (nodes, compiled_chains, propagated_schemas, compiled_aggregations) =
        validated.into_parts();
    let registered_descriptors: Vec<String> = nodes.iter().map(|n| n.name().to_string()).collect();

    // Use the new (Phase 13.4 Plan 06) categorized diff. apply_shard's
    // force-handling block pre-removes every existing descriptor the
    // payload would change (destructive + additive-against-existing,
    // when force=true) BEFORE this function runs. By the time we get
    // here, classify against the post-pre-removal snapshot should never
    // produce destructive entries — every remaining diff is additive
    // (NewDescriptor) or already_present (exact match).
    let diff = beava_core::register_validate::classify_register_diff(&current_snapshot, &nodes);

    // Invariant check: apply_shard's force-handling block pre-removes every
    // descriptor whose shape would change before this function is called.
    // If destructive entries reach here, the caller bypassed apply_shard
    // (or its pre-flight has a bug) — surface as WalUnavailable (503) so
    // operators see a noisy server-side error rather than a silent no-op.
    if !diff.destructive.is_empty() {
        warn!(
            kind = "register.preflight_invariant_violated",
            version = current_snapshot.version,
            destructive = ?diff.destructive,
            "execute_register saw destructive entries — apply_shard pre-flight should have pre-removed them; refusing to mutate"
        );
        return RegisterOutcome::WalUnavailable {
            version: current_snapshot.version,
        };
    }

    // Net-new descriptors are entries with `kind: new_descriptor`. Other
    // additive variants (NewField, NewAgg) shouldn't appear here in normal
    // flow — apply_shard pre-removes their target descriptor with
    // force=true so they re-land here as NewDescriptor against the
    // post-removal snapshot.
    let added: Vec<String> = diff
        .additive
        .iter()
        .filter_map(|e| match e {
            beava_core::registry_diff::DiffEntry::NewDescriptor { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();

    if added.is_empty() {
        info!(
            kind = "register.noop",
            version = current_snapshot.version,
            nodes = registered_descriptors.len(),
            "register no-op"
        );
        return RegisterOutcome::Noop {
            version: current_snapshot.version,
            registered_descriptors,
            already_present: diff.already_present,
        };
    }

    // Apply-after-fsync: append + fsync the `RegistryBump` BEFORE mutating
    // the live registry, so recovery only sees descriptors that survived
    // the durability boundary.
    if let Some(sink) = wal_sink {
        let bump = RegistryBumpPayload {
            new_version: current_snapshot.version + 1,
            payload_nodes: nodes.clone(),
            force_removed_descriptors: force_removed.clone(),
        };
        let encoded = match bump.encode() {
            Ok(b) => b,
            Err(e) => {
                warn!(
                    kind = "register.wal_encode_failed",
                    error = %e,
                    "RegistryBump encode failed"
                );
                return RegisterOutcome::WalUnavailable {
                    version: current_snapshot.version,
                };
            }
        };
        if let Err(e) = sink.append_record(RecordType::RegistryBump, encoded).await {
            warn!(
                kind = "register.wal_append_failed",
                error = %e,
                "RegistryBump WAL append failed"
            );
            return RegisterOutcome::WalUnavailable {
                version: current_snapshot.version,
            };
        }
    }

    let new_version = registry.apply_registration(
        nodes,
        compiled_chains,
        propagated_schemas,
        compiled_aggregations,
    );
    info!(
        kind = "register.success",
        version = new_version,
        added = ?added,
        already_present_count = diff.already_present.len(),
        "register succeeded"
    );
    RegisterOutcome::Success {
        version: new_version,
        registered_descriptors,
        added,
        already_present: diff.already_present,
    }
}

/// Serialize a response to `serde_json::Value`. All current response types
/// are infallible to serialise; the fallback exists only so that a future
/// non-serialisable field fails gracefully with a 500-shaped sentinel
/// rather than panicking the runtime.
fn to_json_value<T: serde::Serialize>(v: T) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or_else(|e| {
        tracing::error!(
            kind = "register.serialization_error",
            error = %e,
            "BUG: response struct failed to serialize — returning 500 sentinel"
        );
        serde_json::json!({
            "status": "error",
            "error": {"code": "internal_error", "reason": "response serialization failed"}
        })
    })
}

/// Serialise a `RegisterOutcome` into the
/// `GlueResponse::Register { http_status, body, tcp_op }` triple consumed
/// by the mio-path HTTP and TCP encoders. `tcp_op` is `OP_REGISTER` on
/// success, `OP_ERROR_RESPONSE` on failure. Used by
/// `apply_shard.rs::dispatch_one` and the data-plane glue.
pub fn register_outcome_to_glue(outcome: RegisterOutcome) -> (u16, bytes::Bytes, u16) {
    use beava_core::wire::{OP_ERROR_RESPONSE, OP_REGISTER};

    let (status, value) = map_outcome_to_response(outcome);
    let body_bytes = bytes::Bytes::from(serde_json::to_vec(&value).unwrap_or_default());
    let tcp_op = if status == 200 {
        OP_REGISTER
    } else {
        OP_ERROR_RESPONSE
    };
    (status, body_bytes, tcp_op)
}

/// Map a `RegisterOutcome` to a `(http_status_code, json_body)` pair shared
/// by the HTTP and TCP encoders.
fn map_outcome_to_response(outcome: RegisterOutcome) -> (u16, serde_json::Value) {
    match outcome {
        RegisterOutcome::Success {
            version,
            registered_descriptors,
            added,
            already_present,
        } => {
            let resp = RegisterSuccess {
                status: "ok",
                registry_version: version,
                registered_descriptors,
                added,
                already_present,
            };
            (200, to_json_value(resp))
        }
        RegisterOutcome::EmptyPayload { version } => {
            let resp = RegisterSuccess {
                status: "ok",
                registry_version: version,
                registered_descriptors: vec![],
                added: vec![],
                already_present: vec![],
            };
            (200, to_json_value(resp))
        }
        RegisterOutcome::Noop {
            version,
            registered_descriptors,
            already_present,
        } => {
            let resp = RegisterSuccess {
                status: "ok",
                registry_version: version,
                registered_descriptors,
                added: vec![],
                already_present,
            };
            (200, to_json_value(resp))
        }
        RegisterOutcome::ValidationFailed {
            version,
            first_error_code,
            first_error_path,
            first_error_reason,
            ..
        } => {
            let wire_code = error_code_to_wire_str(first_error_code);
            let body = RegisterErrorBody {
                error: RegisterError::Validation {
                    code: wire_code,
                    path: first_error_path,
                    reason: first_error_reason,
                },
                registry_version: version,
            };
            (400, to_json_value(body))
        }
        RegisterOutcome::WalUnavailable { version } => {
            let body = serde_json::json!({
                "error": {
                    "code": "wal_unavailable",
                    "reason": "WAL append for registry bump failed; registry not mutated"
                },
                "registry_version": version,
            });
            (503, body)
        }
    }
}

/// Map an `ErrorCode` to its wire string. Expression-shape errors collapse
/// to `"invalid_expression"`; aggregation and sketch codes keep their own
/// strings so clients can distinguish them from structural failures.
pub(crate) fn error_code_to_wire_str(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InvalidExpression
        | ErrorCode::UnknownFieldReference
        | ErrorCode::SchemaPropagationFailure
        | ErrorCode::InvalidCastTarget => "invalid_expression",
        ErrorCode::AggregationOnTableNotSupported => "aggregation_on_table_not_supported",
        ErrorCode::AggregationUnknownField => "aggregation_unknown_field",
        ErrorCode::AggregationInvalidWhere => "aggregation_invalid_where",
        ErrorCode::AggregationInvalidWindow => "aggregation_invalid_window",
        ErrorCode::AggregationUnknownOp => "aggregation_unknown_op",
        ErrorCode::AggregationDuplicateFeatureName => "aggregation_duplicate_feature_name",
        ErrorCode::AggregationGroupKeyCollidesWithFeature => {
            "aggregation_group_key_collides_with_feature"
        }
        ErrorCode::AggregationFeatureNameCollisionAcrossAggregations => {
            "aggregation_feature_name_collision_across_aggregations"
        }
        ErrorCode::WindowNotSupported => "window_not_supported",
        ErrorCode::InvalidPercentileQ => "invalid_percentile_q",
        ErrorCode::InvalidTopKK => "invalid_top_k_k",
        ErrorCode::InvalidBloomFpr => "invalid_bloom_fpr",
        _ => "invalid_registration",
    }
}

/// Returns `("<body>", err.to_string())`. Used by the mio glue layer to
/// produce `error.path` + `error.reason` pairs for malformed `/register`
/// request bodies. Richer JSON-pointer paths are deferred work.
pub fn format_serde_error_public(e: &serde_json::Error) -> (String, String) {
    ("<body>".to_string(), e.to_string())
}
