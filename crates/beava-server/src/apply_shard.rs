//! Single-writer apply shard for the mio event loop.
//!
//! `ApplyShard` wraps the shared `AppState` (behind a `parking_lot::Mutex`
//! held inside an `Arc`) with a synchronous dispatch path. No `.await`,
//! no mpsc, no tokio on the hot path.
//!
//! `ApplyShard` is `Send + Sync` because every interior-mutable field is
//! an `Arc`. In the serve loop only the single apply thread calls
//! `dispatch_wire_request_sync`, so the Mutex is uncontended and a
//! lock+unlock costs ~10–20 ns on macOS/Linux.

use crate::register::RegisterPayload;
use crate::runtime_core_glue::GlueResponse;
use crate::AppState;
use beava_core::row::{Row, Value};
use beava_runtime_core::wal_buffer::WalBufferRing;
use beava_runtime_core::wal_lsn::WalLsn;
use beava_runtime_core::wire_request::WireRequest;
use bytes::Bytes;
use std::sync::Arc;

/// Single-writer apply shard. Owns the apply path for the mio event loop.
///
/// Created once by `ServerV18::serve_with_dirs` and used exclusively from the
/// apply thread. The `AppState` Arc is shared with admin endpoints (read-only).
pub struct ApplyShard {
    state: Arc<AppState>,
    wal_ring: Arc<WalBufferRing>,
    // reason: WalLsn handle retained on the shard for future WAL-coordinated
    // dispatches; the current apply path drives writes through `wal_ring`
    // directly. Drop only when watermark observation can be shown unneeded.
    #[allow(dead_code)]
    wal_lsn: Arc<WalLsn>,
}

impl ApplyShard {
    /// Create a new `ApplyShard`.
    ///
    /// `state` — shared application state (registry, aggregations, idem-cache).
    /// `wal_ring` — hand-rolled WAL ring buffer (lock-free append on apply thread).
    /// `wal_lsn` — four-watermark LSN tracker (committed/written/synced/acked).
    pub fn new(state: Arc<AppState>, wal_ring: Arc<WalBufferRing>, wal_lsn: Arc<WalLsn>) -> Self {
        Self {
            state,
            wal_ring,
            wal_lsn,
        }
    }

    /// Synchronous dispatch — the hot path for the apply thread.
    ///
    /// Processes one `WireRequest` and returns a `Vec<GlueResponse>`
    /// (almost always exactly one element; the `Vec` is for future
    /// pipelining / batch expansion). The WAL append uses
    /// `WalBufferRing::append` (lock-free memcpy + atomic position bump).
    pub fn dispatch_wire_request_sync(&self, req: WireRequest) -> Vec<GlueResponse> {
        vec![self.dispatch_one(req, None)]
    }

    /// Dispatch with an optional pre-parsed `Row`. The IoPool worker
    /// deserialises push-frame bodies into `Row` while bytes are hot in
    /// L1 and hands the result here; the apply thread skips the
    /// redundant `from_slice::<Row>` call (~190 ns per push at
    /// parallel=4/pd=64).
    ///
    /// `pre_parsed_row = None` is the fallback path:
    /// - non-push variants (`Ping`, `Register`, `GetSingle`, …)
    /// - IoPool pre-parse failed (malformed body) → apply retries the
    ///   parse and emits `invalid_event`
    /// - test / legacy callers that don't run through IoPool.
    pub fn dispatch_wire_request_with_row(
        &self,
        req: WireRequest,
        pre_parsed_row: Option<Row>,
    ) -> Vec<GlueResponse> {
        vec![self.dispatch_one(req, pre_parsed_row)]
    }

    fn dispatch_one(&self, req: WireRequest, pre_parsed_row: Option<Row>) -> GlueResponse {
        match req {
            WireRequest::Ping => GlueResponse::Pong {
                registry_version: self.state.dev_agg.registry.version() as u32,
            },

            // Register is the cold path on the mio loop. It funnels here so
            // the apply thread owns every registry mutation. Durability is
            // still routed through the legacy `WalSink` path; the mio
            // direct path doesn't yet WAL-append the `RegistryBump`.
            WireRequest::Register { payload } => {
                // JSON-prelude shims run BEFORE strict `RegisterPayload`
                // deserialize so rejection paths stay independent of
                // whether the corresponding `OpNode` / `PayloadNode`
                // variants still exist in the enum. Each shim emits a
                // structured error code (see `register_validate`) instead
                // of opaque serde "unknown variant" errors.
                if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&payload) {
                    // Joins / unions: rejected as feature-removed in v0.
                    if let Some(removed) =
                        beava_core::register_validate::pre_check_removed_ops(&json_value)
                    {
                        let body = serde_json::json!({
                            "error": {
                                "code": removed.code,
                                "path": removed.path,
                                "reason": removed.reason,
                            },
                            "registry_version": self.state.dev_agg.registry.version(),
                        });
                        return GlueResponse::Register {
                            http_status: 400,
                            body: bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                            tcp_op: beava_core::wire::OP_ERROR_RESPONSE,
                        };
                    }
                    // Legacy event-time keys (`event_time_field`,
                    // `tolerate_delay_ms`): rejected as unknown fields in
                    // a Redis-shaped (processing-time) world.
                    if let Some(removed) =
                        beava_core::register_validate::pre_check_legacy_event_time_keys(&json_value)
                    {
                        let body = serde_json::json!({
                            "error": {
                                "code": removed.code,
                                "path": removed.path,
                                "reason": removed.reason,
                            },
                            "registry_version": self.state.dev_agg.registry.version(),
                        });
                        return GlueResponse::Register {
                            http_status: 400,
                            body: bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                            tcp_op: beava_core::wire::OP_ERROR_RESPONSE,
                        };
                    }
                    // Events-only enforcement: reject payloads with
                    // `{"kind": "table", ...}` (or any other non-event /
                    // non-derivation kind) with `unsupported_node_kind`.
                    // This is the events-only invariant gate at register
                    // time — see `CLAUDE.md` §"Events-Only Invariant".
                    if let Some(removed) =
                        beava_core::register_validate::pre_check_unsupported_node_kind(&json_value)
                    {
                        let body = serde_json::json!({
                            "error": {
                                "code": removed.code,
                                "path": removed.path,
                                "reason": removed.reason,
                            },
                            "registry_version": self.state.dev_agg.registry.version(),
                        });
                        return GlueResponse::Register {
                            http_status: 400,
                            body: bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                            tcp_op: beava_core::wire::OP_ERROR_RESPONSE,
                        };
                    }
                    // Memory governance: reject windowless aggregation
                    // ops whose lifetime bound is `Unbounded` (per
                    // `lifetime_bound_for_op_str`). Default ON; the
                    // escape hatch is `BEAVA_MEMORY_GOV_ENFORCE=0` in
                    // production or `.memory_governance_enforce(false)`
                    // in tests, both resolved at boot into
                    // `AppState.memory_governance_enforce` so the cold
                    // register path doesn't read process env per-call.
                    if self.state.memory_governance_enforce {
                        if let Some(removed) =
                            beava_core::register_validate::pre_check_unbounded_op_in_lifetime_mode(
                                &json_value,
                            )
                        {
                            let body = serde_json::json!({
                                "error": {
                                    "code": removed.code,
                                    "path": removed.path,
                                    "reason": removed.reason,
                                },
                                "registry_version": self.state.dev_agg.registry.version(),
                            });
                            return GlueResponse::Register {
                                http_status: 400,
                                body: bytes::Bytes::from(
                                    serde_json::to_vec(&body).unwrap_or_default(),
                                ),
                                tcp_op: beava_core::wire::OP_ERROR_RESPONSE,
                            };
                        }
                    }
                }
                // Parse + dispatch on the apply thread, then funnel the
                // outcome through `register_outcome_to_glue` so wire
                // bytes are identical across HTTP and TCP.
                let reg_payload: RegisterPayload = match serde_json::from_slice(&payload) {
                    Ok(p) => p,
                    Err(e) => {
                        let (path, reason) = crate::register::format_serde_error_public(&e);
                        let body = serde_json::json!({
                            "error": {
                                "code": "invalid_registration",
                                "path": path,
                                "reason": reason,
                            },
                            "registry_version": self.state.dev_agg.registry.version(),
                        });
                        return GlueResponse::Register {
                            http_status: 400,
                            body: bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                            tcp_op: beava_core::wire::OP_ERROR_RESPONSE,
                        };
                    }
                };

                // After the JSON-prelude shims and strict serde, but BEFORE
                // the additive-conflict path: classify the diff with the
                // categorised-lists schema. `dry_run` fires first (returns
                // the diff JSON without applying). The force gate runs
                // second: destructive entries without `force=true` return
                // 409 + `force_required`; with `force=true` we compute the
                // exact removal set but leave the live registry untouched
                // until the WAL-backed register path has fsynced the bump.
                let (force, dry_run) = match serde_json::from_slice::<serde_json::Value>(&payload) {
                    Ok(v) => (
                        v.get("force").and_then(|x| x.as_bool()).unwrap_or(false),
                        v.get("dry_run").and_then(|x| x.as_bool()).unwrap_or(false),
                    ),
                    Err(_) => (false, false),
                };
                let prev_snapshot = self.state.dev_agg.registry.snapshot();
                let diff = beava_core::register_validate::classify_register_diff(
                    &prev_snapshot,
                    &reg_payload.nodes,
                );

                if dry_run {
                    // `dry_run=true, force=true` is treated as dry_run —
                    // dry_run wins so `force` never escalates a "what
                    // would this do?" call into a real mutation.
                    let body = serde_json::json!({
                        "diff": diff,
                        "would_apply": false,
                    });
                    return GlueResponse::Register {
                        http_status: 200,
                        body: bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                        tcp_op: beava_core::wire::OP_REGISTER,
                    };
                }

                if let Err(force_err) =
                    beava_core::register_validate::register_check_force_required(&diff, force)
                {
                    let body = serde_json::json!({
                        "error": force_err,
                        "registry_version": self.state.dev_agg.registry.version(),
                    });
                    return GlueResponse::Register {
                        http_status: 409,
                        body: bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                        tcp_op: beava_core::wire::OP_ERROR_RESPONSE,
                    };
                }

                // `force=true` carries the explicit "drop existing state and
                // replace fully" intent for any descriptor that already
                // exists in the registry. Compute every existing descriptor
                // the new payload would change and record it in the durable
                // `RegistryBump`; the WAL-backed path applies the removal
                // after fsync. Two reasons we can't gate solely on
                // `diff.destructive`:
                //
                //   1. `classify_register_diff` classifies new fields on
                //      an existing event source and new aggregations in
                //      an existing block as `additive`, not destructive.
                //      Pre-removal must cover them too — otherwise their
                //      target descriptor stays put with the old shape.
                //
                //   2. `NewDescriptor` is the only additive variant we
                //      skip — it's genuinely-new (not in current
                //      registry), so there's nothing to pre-remove.
                //
                // Forced removal drops compiled chains, aggregations, feature
                // index entries, and the old agg slots become unreachable.
                // That state loss is the documented `force=true` semantic;
                // callers who want to add a field without losing state should
                // send the request without `force`.
                let mut force_removed: Vec<String> = Vec::new();
                if force {
                    let mut to_remove: Vec<String> = Vec::new();
                    for entry in &diff.destructive {
                        match entry {
                            beava_core::registry_diff::DiffEntry::Rename { from, .. } => {
                                to_remove.push(from.clone());
                            }
                            beava_core::registry_diff::DiffEntry::TypeChange { field, .. } => {
                                // `field` is `"<descriptor>.<field>"`; remove
                                // the owning descriptor so it can re-install.
                                if let Some((descriptor, _)) = field.split_once('.') {
                                    to_remove.push(descriptor.to_string());
                                }
                            }
                            beava_core::registry_diff::DiffEntry::OpRemoval { table, .. }
                            | beava_core::registry_diff::DiffEntry::AggRemoval { table, .. }
                            | beava_core::registry_diff::DiffEntry::KeyColsChange {
                                table, ..
                            } => {
                                to_remove.push(table.clone());
                            }
                            beava_core::registry_diff::DiffEntry::WindowChange { agg, .. } => {
                                if let Some((descriptor, _)) = agg.split_once('.') {
                                    to_remove.push(descriptor.to_string());
                                }
                            }
                            // Additive variants don't appear in
                            // `destructive`; the list is destructive-only.
                            _ => {}
                        }
                    }
                    // Additive-against-existing: the descriptor already
                    // exists, so `apply_registration` would skip-if-present
                    // and silently drop the new field/agg. Pre-remove here
                    // so the new shape lands as a clean add.
                    for entry in &diff.additive {
                        match entry {
                            beava_core::registry_diff::DiffEntry::NewField { event, .. } => {
                                to_remove.push(event.clone());
                            }
                            beava_core::registry_diff::DiffEntry::NewAgg { table, .. } => {
                                to_remove.push(table.clone());
                            }
                            // `NewDescriptor` is genuinely-new — not in
                            // current registry, nothing to pre-remove.
                            _ => {}
                        }
                    }
                    to_remove.sort();
                    to_remove.dedup();

                    // Cascade closure: every downstream derivation whose
                    // `upstreams` reference a descriptor in `to_remove` is
                    // structurally broken by the force-removal — its
                    // compiled chain references the old upstream shape and
                    // its accumulator slot is keyed off the same agg_id.
                    // Re-registering the upstream without cascading would
                    // leave the orphan downstream wired to stale state, so
                    // pull every transitive downstream into the removal
                    // list.
                    //
                    // Walk over the pre-removal snapshot (already captured
                    // above as `prev_snapshot`) — we already hold it and
                    // it has not mutated since classify_register_diff ran.
                    // The registry is a DAG by construction (cycles
                    // forbidden at register time); we still cap the
                    // fixpoint iteration at `descriptor_count` as a
                    // belt-and-braces termination guard.
                    let descriptor_count = prev_snapshot.events.len()
                        + prev_snapshot.tables.len()
                        + prev_snapshot.derivations.len();
                    for _ in 0..descriptor_count {
                        let to_remove_set: std::collections::HashSet<String> =
                            to_remove.iter().cloned().collect();
                        let mut additions: Vec<String> = Vec::new();
                        for (name, deriv) in prev_snapshot.derivations.iter() {
                            if to_remove_set.contains(name.as_str()) {
                                continue;
                            }
                            if deriv
                                .upstreams
                                .iter()
                                .any(|u| to_remove_set.contains(u.as_str()))
                            {
                                additions.push(name.clone());
                            }
                        }
                        if additions.is_empty() {
                            break;
                        }
                        to_remove.extend(additions);
                        to_remove.sort();
                        to_remove.dedup();
                    }

                    // Captured for the RegistryBump WAL record so recovery
                    // can replay the same force-removal closure BEFORE
                    // re-applying `payload_nodes` — without this, replay
                    // resurfaces pre-removal state.
                    force_removed = to_remove;
                }

                // Register is cold path: delegate to the async WAL-backed
                // function on a one-shot single-threaded tokio runtime so
                // we don't pull tokio into the hot path.
                let state_clone = Arc::clone(&self.state);
                let registry_bump_min_lsn = state_clone
                    .dev_agg
                    .next_event_id
                    .load(std::sync::atomic::Ordering::Acquire)
                    .saturating_add(1);
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("temp tokio rt for register");
                let outcome = rt.block_on(crate::register::execute_register_with_wal(
                    &state_clone.dev_agg.registry,
                    reg_payload,
                    &state_clone.wal_sink,
                    force_removed,
                    registry_bump_min_lsn,
                ));
                // Grow `state_tables` so the hot path can index by
                // `desc.agg_id` without bounds issues. Register is rare,
                // so the lock + resize is fine here on the cold path.
                let new_next_agg_id = state_clone.dev_agg.registry.next_agg_id() as usize;
                if new_next_agg_id > 0 {
                    let mut tables = state_clone.dev_agg.state_tables.lock();
                    beava_core::agg_state_table::ensure_capacity_for(&mut tables, new_next_agg_id);
                }
                let (http_status, body, tcp_op) =
                    crate::register::register_outcome_to_glue(outcome);
                // Refresh the shared admin-snapshot Arc so the tokio
                // sidecar's `/registry` JSON and Prometheus gauges
                // (`beava_registry_version`, `beava_node_count`) reflect
                // the new registry shape on every successful register
                // (200). Failure paths (e.g. 409 `force_required`)
                // returned earlier and never reach this point.
                //
                // Lock-ordering: read `version()` / `node_count()` first
                // (each drops the registry inner-RwLock before we touch
                // the snapshot Arc), THEN take the snapshot's write lock.
                if http_status == 200 {
                    let new_version = state_clone.dev_agg.registry.version();
                    let new_node_count = state_clone.dev_agg.registry.node_count();
                    let mut snap = state_clone
                        .admin_snapshot
                        .write()
                        .unwrap_or_else(|p| p.into_inner());
                    snap.version = new_version;
                    snap.node_count = new_node_count;
                }
                GlueResponse::Register {
                    http_status,
                    body,
                    tcp_op,
                }
            }

            WireRequest::TcpPush {
                event_name,
                body,
                body_format,
            }
            | WireRequest::HttpPush {
                event_name,
                body,
                body_format,
            } => self.dispatch_push_sync(&event_name, body, body_format, pre_parsed_row),

            // HTTP push-sync (per-event acks). The full
            // wait-for-synced blocking call would stall the apply thread,
            // so today we treat it as periodic push; per-event durability
            // is future work.
            WireRequest::HttpPushSync {
                event_name,
                body,
                body_format,
            } => self.dispatch_push_sync(&event_name, body, body_format, pre_parsed_row),

            WireRequest::HttpPushBatch {
                event_name,
                body,
                body_format,
            } => {
                // Treat batch as single push for now; per-event batching
                // is a future optimisation.
                self.dispatch_push_sync(&event_name, body, body_format, pre_parsed_row)
            }

            WireRequest::HttpGetSingle { feature, key } => {
                fn trace_apply_enabled() -> bool {
                    use std::sync::OnceLock;
                    static FLAG: OnceLock<bool> = OnceLock::new();
                    *FLAG.get_or_init(|| {
                        std::env::var("BEAVA_TRACE_APPLY_TIMING").ok().as_deref() == Some("1")
                    })
                }
                let trace_apply = trace_apply_enabled();
                let t0 = if trace_apply {
                    Some(std::time::Instant::now())
                } else {
                    None
                };
                let resp = crate::runtime_core_glue::dispatch_get_single_sync(
                    &self.state,
                    &feature,
                    &key,
                    beava_core::wire::CT_JSON,
                );
                if let Some(t0_inst) = t0 {
                    let total = t0_inst.elapsed();
                    eprintln!(
                        "TRACE_APPLY ns get: feature_len={} key_len={} TOTAL={}",
                        feature.len(),
                        key.len(),
                        total.as_nanos()
                    );
                }
                resp
            }

            // POST /get — verb-style single-row GET. Body parses to
            // `{"table", "key", "features"?}`. Three-step ladder:
            //   (a) try strict-deserialise the new shape;
            //   (b) on parse failure, look for legacy `{keys, features}`
            //       (2D cell) or `{feature, key}` (single-feature) and
            //       return `UnsupportedRequestShape`;
            //   (c) on no match, surface `InternalError` so genuinely
            //       malformed JSON still produces a clear error.
            WireRequest::HttpGet { body } => {
                use beava_core::wire::CT_JSON;
                #[derive(serde::Deserialize)]
                struct HttpGetReq {
                    table: String,
                    key: String,
                    #[serde(default)]
                    features: Option<Vec<String>>,
                }
                match serde_json::from_slice::<HttpGetReq>(&body) {
                    Ok(req) => crate::runtime_core_glue::dispatch_get_single_verb_style_sync(
                        &self.state,
                        &req.table,
                        &req.key,
                        req.features.as_deref(),
                        CT_JSON,
                    ),
                    Err(parse_err) => {
                        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) {
                            let is_legacy_2d =
                                value.get("keys").map(|v| v.is_array()).unwrap_or(false)
                                    && value.get("features").map(|v| v.is_array()).unwrap_or(false);
                            let is_legacy_single =
                                value.get("feature").map(|v| v.is_string()).unwrap_or(false)
                                    && value.get("key").map(|v| v.is_string()).unwrap_or(false);
                            if is_legacy_2d || is_legacy_single {
                                let hint = "POST /get expects {table, key, features?}; received legacy {keys, features} shape — see docs/http-api.md#post-get".to_string();
                                return GlueResponse::UnsupportedRequestShape { hint };
                            }
                        }
                        GlueResponse::InternalError {
                            reason: parse_err.to_string(),
                        }
                    }
                }
            }

            // TCP `OP_GET` — verb-style single-row. Same body shape as
            // `POST /get`; codec selected by the frame's content-type
            // byte. Legacy-shape detection runs on `CT_JSON` only —
            // msgpack clients are already verb-style aware.
            WireRequest::TcpGet { body, body_format } => {
                use beava_core::wire::{CT_JSON, CT_MSGPACK};
                #[derive(serde::Deserialize)]
                struct TcpGetReq {
                    table: String,
                    key: String,
                    #[serde(default)]
                    features: Option<Vec<String>>,
                }
                let parse_result: Result<TcpGetReq, String> = match body_format {
                    CT_JSON => serde_json::from_slice(&body).map_err(|e| e.to_string()),
                    CT_MSGPACK => rmp_serde::from_slice(&body).map_err(|e| e.to_string()),
                    other => {
                        return GlueResponse::InternalError {
                            reason: format!("unsupported content_type: {other:#04x}"),
                        };
                    }
                };
                match parse_result {
                    Ok(req) => crate::runtime_core_glue::dispatch_get_single_verb_style_sync(
                        &self.state,
                        &req.table,
                        &req.key,
                        req.features.as_deref(),
                        body_format,
                    ),
                    Err(parse_err) => {
                        if body_format == CT_JSON {
                            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) {
                                let is_legacy_2d = value
                                    .get("keys")
                                    .map(|v| v.is_array())
                                    .unwrap_or(false)
                                    && value.get("features").map(|v| v.is_array()).unwrap_or(false);
                                let is_legacy_single =
                                    value.get("feature").map(|v| v.is_string()).unwrap_or(false)
                                        && value.get("key").map(|v| v.is_string()).unwrap_or(false);
                                if is_legacy_2d || is_legacy_single {
                                    let hint = "OP_GET expects {table, key, features?}; received legacy {keys, features} or {feature, key} shape — see docs/wire-spec.md#op_get-0x0020".to_string();
                                    return GlueResponse::UnsupportedRequestShape { hint };
                                }
                            }
                        }
                        GlueResponse::InternalError { reason: parse_err }
                    }
                }
            }

            // TCP `/mget` (single feature, multi key). Body
            // `{"feature", "keys"}` is materialised as a batch with a
            // single-feature list and routed through
            // `dispatch_get_batch_sync`.
            //
            // TODO: pass keys / features directly into a batch helper so
            // we can skip the re-serialise round-trip.
            WireRequest::TcpMGet { body, body_format } => {
                use beava_core::wire::{CT_JSON, CT_MSGPACK};
                #[derive(serde::Deserialize)]
                struct TcpMGetReq {
                    feature: String,
                    keys: Vec<String>,
                }
                let req: TcpMGetReq = match body_format {
                    CT_JSON => match serde_json::from_slice(&body) {
                        Ok(r) => r,
                        Err(e) => {
                            return GlueResponse::InternalError {
                                reason: e.to_string(),
                            };
                        }
                    },
                    CT_MSGPACK => match rmp_serde::from_slice(&body) {
                        Ok(r) => r,
                        Err(e) => {
                            return GlueResponse::InternalError {
                                reason: e.to_string(),
                            };
                        }
                    },
                    other => {
                        return GlueResponse::InternalError {
                            reason: format!("unsupported content_type: {other:#04x}"),
                        };
                    }
                };
                let batch_body = serde_json::json!({
                    "keys": req.keys,
                    "features": [req.feature],
                });
                let batch_bytes = match body_format {
                    CT_JSON => match serde_json::to_vec(&batch_body) {
                        Ok(b) => bytes::Bytes::from(b),
                        Err(e) => {
                            return GlueResponse::InternalError {
                                reason: e.to_string(),
                            };
                        }
                    },
                    CT_MSGPACK => match rmp_serde::to_vec_named(&batch_body) {
                        Ok(b) => bytes::Bytes::from(b),
                        Err(e) => {
                            return GlueResponse::InternalError {
                                reason: e.to_string(),
                            };
                        }
                    },
                    _ => unreachable!("validated above"),
                };
                crate::runtime_core_glue::dispatch_get_batch_sync(
                    &self.state,
                    &batch_bytes,
                    body_format,
                )
            }

            // TCP `/get-multi` (multi feature, multi key). Body shape
            // mirrors HTTP `/get`. The codec is selected inside
            // `dispatch_get_batch_sync` from `body_format`.
            WireRequest::TcpGetMulti { body, body_format } => {
                crate::runtime_core_glue::dispatch_get_batch_sync(&self.state, &body, body_format)
            }

            // `OP_BATCH_GET` / `POST /batch_get` — heterogeneous batched
            // read. Each `(table, entity_id)` is looked up; partial
            // failures (unknown table, etc.) surface per-tuple inside
            // `results` rather than as a whole-batch 4xx. Empty-string
            // `entity_id` is the global-table sentinel and forwards as-
            // is to `parse_entity_key`.
            WireRequest::TcpBatchGet { body, body_format } => {
                dispatch_batch_get_sync(&self.state, &body, body_format)
            }
            WireRequest::HttpBatchGet { body } => {
                dispatch_batch_get_sync(&self.state, &body, beava_core::wire::CT_JSON)
            }

            // `OP_RESET` / `POST /reset` — both transports route through
            // `dispatch_reset_sync`. The body is intentionally not
            // parsed; reset is a no-arg operation.
            WireRequest::TcpReset { .. } | WireRequest::HttpReset { .. } => {
                dispatch_reset_sync(&self.state)
            }

            // 415 Unsupported Media Type — POST without
            // `Content-Type: application/json`.
            WireRequest::HttpUnsupportedMediaType { received, path } => {
                GlueResponse::HttpUnsupportedMediaType { received, path }
            }

            // `/health` is an inline shim with no AppState consult or
            // recovery dependency. Health probes (`read_bench.py`,
            // Kubernetes liveness) run on a 0.5 s per-attempt budget;
            // gating on apply-thread responsiveness would race startup
            // recovery on cold replicas. Returning OK unconditionally
            // matches the Kubernetes liveness contract: "yes the process
            // is up and accepting connections".
            WireRequest::HttpHealth => GlueResponse::HealthOk,

            // `POST /ping` — verb-style liveness mirror of TCP
            // `OP_PING (0x0000)`. Returns `200 {"pong": true,
            // "registry_version": <n>}` so SDK clients can use it as a
            // cheap registry-version probe (cache invalidation,
            // schema-evolution detection on long-lived connections).
            // We're already on the apply thread here, so reading
            // `registry.version()` is free — no extra round-trip.
            // For dumb-liveness fixtures that don't care about the
            // registry counter, `/health` keeps the original
            // `{"status":"ok"}` shape.
            WireRequest::HttpPing => GlueResponse::Pong {
                registry_version: self.state.dev_agg.registry.version() as u32,
            },

            // Data-plane `/ready` and `/registry` shims for `TestServer`
            // back-compat. `/ready` mirrors the admin sidecar's body;
            // `/registry` serialises the live registry via
            // `build_registry_dump` and is gated on `dev_endpoints`.
            WireRequest::HttpReady => GlueResponse::ReadyOk,
            WireRequest::HttpRegistry => {
                if !self.state.dev_endpoints_enabled() {
                    GlueResponse::HttpRouteNotFound {
                        path: "/registry".to_owned(),
                    }
                } else {
                    let dump =
                        crate::registry_debug::build_registry_dump(&self.state.dev_agg.registry);
                    let body = serde_json::to_vec(&dump).unwrap_or_default();
                    GlueResponse::RegistrySnapshot {
                        body: bytes::Bytes::from(body),
                    }
                }
            }
            // Route-level errors (unknown path / wrong method) surface
            // as 404 / 405. Wire-level decode failures (`ParseError`)
            // and unknown TCP opcodes route to `Unsupported` / `TcpError`
            // below.
            WireRequest::HttpNotFound { path } => GlueResponse::HttpRouteNotFound { path },
            WireRequest::HttpMethodNotAllowed { method, path } => {
                GlueResponse::HttpMethodNotAllowed { method, path }
            }

            // Known-but-deferred opcodes return a rich
            // `op_not_implemented` frame; truly unknown opcodes return
            // `unknown_op`. Both keep the TCP connection open.
            WireRequest::Unknown { op } => {
                use beava_core::wire::OP_PUSH_SYNC;
                if op == OP_PUSH_SYNC {
                    GlueResponse::TcpError {
                        code: "op_not_implemented",
                        message: format!("opcode {op:#06x} (push_sync) is not yet implemented",),
                        extras: serde_json::json!({"op": op}),
                    }
                } else {
                    GlueResponse::TcpError {
                        code: "unknown_op",
                        message: format!("opcode {op:#06x} is not recognised by this server"),
                        extras: serde_json::json!({"op": op}),
                    }
                }
            }
            // `ParseError` distinguishes content-type rejections
            // (special prefix → dedicated `unsupported_content_type`
            // TcpError) from verb-style push body-parse failures
            // (`missing_event_name_in_body`, `invalid_json_body` → 400
            // `PushError`) and generic decode failures
            // (`Unsupported`).
            WireRequest::ParseError { reason } => {
                if reason.starts_with("unsupported content_type") {
                    GlueResponse::TcpError {
                        code: "unsupported_content_type",
                        message: reason,
                        extras: serde_json::json!({}),
                    }
                } else if reason == "missing_event_name_in_body" {
                    GlueResponse::PushError {
                        code: "missing_event_name_in_body",
                        registry_version: self.state.dev_agg.registry.version() as u32,
                    }
                } else if reason == "invalid_json_body" {
                    GlueResponse::PushError {
                        code: "invalid_json_body",
                        registry_version: self.state.dev_agg.registry.version() as u32,
                    }
                } else {
                    GlueResponse::Unsupported
                }
            }
        }
    }

    /// Synchronous push — the hot path.
    ///
    /// Body parses directly into `beava_core::row::Row` via its
    /// `Deserialize` impl (walks `MapAccess` directly, so no
    /// `serde_json::Value` intermediate is allocated).
    ///
    /// 1. Parse body → `Row`.
    /// 2. Look up event descriptor.
    /// 3. Schema validate.
    /// 4. Dedupe lookup.
    /// 5. Serialise WAL payload (body bytes pass through unchanged).
    /// 6. `WalBufferRing::append`.
    /// 7. `apply_event_to_aggregations`.
    /// 8. Build and return the `GlueResponse`.
    fn dispatch_push_sync(
        &self,
        event_name: &str,
        body: Bytes,
        body_format: u8,
        pre_parsed_row: Option<Row>,
    ) -> GlueResponse {
        use beava_core::agg_apply::apply_event_to_aggregations;
        use beava_core::defaults::DEFAULT_DEDUPE_WINDOW_MS;
        use beava_core::wire::CT_MSGPACK;
        use std::sync::atomic::Ordering;
        use std::time::{Instant, SystemTime, UNIX_EPOCH};

        // Per-stage apply-path timing (env-gated). Cache the env lookup
        // in a `OnceLock` so the env read happens once per process, not
        // per push — saves ~200–500 ns per event when tracing is off.
        fn trace_apply_enabled() -> bool {
            use std::sync::OnceLock;
            static FLAG: OnceLock<bool> = OnceLock::new();
            *FLAG.get_or_init(|| {
                std::env::var("BEAVA_TRACE_APPLY_TIMING").ok().as_deref() == Some("1")
            })
        }
        let trace_apply = trace_apply_enabled();
        let t0 = if trace_apply {
            Some(Instant::now())
        } else {
            None
        };
        // Inter-event gap (time since previous push completed on this thread).
        thread_local! {
            static LAST_PUSH_END: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
        }
        let gap = if trace_apply {
            LAST_PUSH_END.with(|cell| cell.take()).map(|t| t.elapsed())
        } else {
            None
        };

        let registry_version = self.state.dev_agg.registry.version() as u32;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // 1. Prefer the pre-parsed `Row` from the IoPool worker when
        //    present; fall back to inline parse for tests / legacy admin
        //    callers and for IoPool pre-parse failures. `Row::Deserialize`
        //    walks `MapAccess` directly (no intermediate `JsonValue`
        //    allocation), and `serde_json` and `rmp_serde` share the
        //    same visitor.
        let row: Row = match pre_parsed_row {
            Some(r) => r,
            None => {
                if body_format == CT_MSGPACK {
                    match rmp_serde::from_slice::<Row>(&body) {
                        Ok(r) => r,
                        Err(_) => {
                            return GlueResponse::PushError {
                                code: "invalid_event",
                                registry_version,
                            };
                        }
                    }
                } else {
                    match sonic_rs::from_slice::<Row>(&body) {
                        Ok(r) => r,
                        Err(_) => {
                            return GlueResponse::PushError {
                                code: "invalid_event",
                                registry_version,
                            };
                        }
                    }
                }
            }
        };
        let t_parse = t0.map(|t| t.elapsed());

        if string_has_control_chars(event_name) || row_has_control_chars(&row) {
            return GlueResponse::PushError {
                code: "control_character_in_string",
                registry_version,
            };
        }

        // 2. Lookup event descriptor (Arc-backed; refcount bump only).
        let descriptor = match self.state.dev_agg.registry.get_event_descriptor(event_name) {
            Some(d) => d,
            None => {
                return GlueResponse::PushError {
                    code: "event_not_found",
                    registry_version,
                };
            }
        };
        let t_lookup = t0.map(|t| t.elapsed());

        // 3. Strict-deny on legacy `event_time` / `event_time_ms` fields
        //    and any other unknown field. The wire schema is processing-
        //    time only, so clients sending event-time data get a
        //    structured 400 (`unknown_field_event_time_v0`) rather than
        //    a silent-ignore. The check runs against the parsed `Row`;
        //    `Row` is a generic key-value map (event schemas are
        //    user-defined), so the correct boundary is the
        //    `EventDescriptor`: any field absent from
        //    `descriptor.schema.fields` (and not declared optional) is
        //    forbidden.
        for (field_name, _) in row.iter() {
            if !descriptor.schema.fields.contains_key(field_name)
                && !descriptor
                    .schema
                    .optional_fields
                    .iter()
                    .any(|f| f == field_name)
            {
                let code: &'static str =
                    if field_name == "event_time" || field_name == "event_time_ms" {
                        "unknown_field_event_time_v0"
                    } else {
                        "unknown_field_v0"
                    };
                return GlueResponse::PushError {
                    code,
                    registry_version,
                };
            }
        }

        // 4. Schema validate against Row.fields directly.
        if !validate_row_against_descriptor(&descriptor, &row) {
            return GlueResponse::PushError {
                code: "invalid_event",
                registry_version,
            };
        }

        // 4. Dedupe lookup against Row.fields.
        let dedupe_str = descriptor
            .dedupe_key
            .as_deref()
            .and_then(|k| extract_dedupe_str_from_row(&row, k));

        if let (Some(_), Some(ref key_str)) = (descriptor.dedupe_key.as_ref(), &dedupe_str) {
            if let Some((cached_ack_lsn, cached_body)) = self
                .state
                .idem_cache
                .get_with_ack_lsn(event_name, key_str, now_ms)
            {
                // Byte-identical replay: HTTP returns `cached_body`
                // verbatim; TCP uses `ack_lsn` to build a `{ack_lsn,
                // idempotent_replay: true, …}` body (TCP has no replay
                // header, so the body flag IS the discriminator).
                return GlueResponse::PushReplay {
                    registry_version,
                    ack_lsn: Some(cached_ack_lsn),
                    cached_body: Some(cached_body),
                };
            }
        }

        // 5. Time source: server wall-clock at dispatch. The apply path
        //    never reads event-time from the body — Beava is Redis-shaped
        //    and processing-time only (`project_redis_shaped_no_event_time_ever`).
        //    `now_ms` is the single time threaded into the operator
        //    surface (windowed bucketing) and into the `query_time_ms`
        //    watermark below.
        let now_ms_i64: i64 = now_ms as i64;
        let t_validate = t0.map(|t| t.elapsed());

        // 6. Serialize WAL payload — v=3 binary format.
        //
        // `[u8 v=3][u64 assigned_lsn BE][u8 body_format][u32 rv BE][u64 et_ms BE]
        //  [u16 event_name_len BE][N bytes name][u32 body_len BE][M bytes body]`
        //
        // `body` is the exact bytes received from the wire — zero-copy
        // from parse → WAL → disk, no re-serialise. The `et_ms` slot
        // carries the server `now_ms` (processing-time, not a
        // body-derived event-time).
        let name_bytes = event_name.as_bytes();
        let name_len = name_bytes.len() as u16;
        let body_len = body.len() as u32;
        let payload_len = 1 + 8 + 1 + 4 + 8 + 2 + name_bytes.len() + 4 + body.len();
        self.wal_lsn
            .mark_committed_at_least(self.state.wal_sink.durable_lsn());
        let expected_ack_lsn = self.wal_lsn.committed().saturating_add(payload_len as u64);
        let mut payload_bytes = Vec::with_capacity(payload_len);
        payload_bytes.push(0x03u8);
        payload_bytes.extend_from_slice(&expected_ack_lsn.to_be_bytes());
        payload_bytes.push(body_format);
        payload_bytes.extend_from_slice(&registry_version.to_be_bytes());
        payload_bytes.extend_from_slice(&now_ms.to_be_bytes());
        payload_bytes.extend_from_slice(&name_len.to_be_bytes());
        payload_bytes.extend_from_slice(name_bytes);
        payload_bytes.extend_from_slice(&body_len.to_be_bytes());
        payload_bytes.extend_from_slice(&body);
        let t_wal_build = t0.map(|t| t.elapsed());

        // 7. WAL append — lock-free, no Mutex, no channel.
        let ack_lsn = self.wal_ring.append(&payload_bytes);
        debug_assert_eq!(ack_lsn, expected_ack_lsn);
        let t_wal_append = t0.map(|t| t.elapsed());

        // 8. Apply to aggregations under the table lock (uncontended on
        //    the apply thread). `now_ms_i64` is the only time source
        //    threaded into `apply_event_to_aggregations` — windowed
        //    bucketing uses processing-time. `cold_after_ms` is a `Copy`
        //    of `Option<u64>` from the descriptor and runs the per-event
        //    cold-TTL eviction check inline.
        {
            let mut tables = self.state.dev_agg.state_tables.lock();
            apply_event_to_aggregations(
                event_name,
                &row,
                now_ms_i64,
                ack_lsn,
                &self.state.dev_agg.registry,
                &mut tables,
                descriptor.cold_after_ms,
            );
            self.state
                .dev_agg
                .next_event_id
                .fetch_max(ack_lsn, Ordering::Release);
            if now_ms > 0 {
                self.state
                    .dev_agg
                    .query_time_ms
                    .fetch_max(now_ms, Ordering::Release);
            }
            // Refresh the process-static `beava_entity_count_resident`
            // gauge under the lock we already hold. O(N_tables) sum of
            // three `HashMap.len()` reads; typically < 30 tables (one
            // per registered aggregation), well under 100 ns even with
            // cache misses.
            let total_entities: usize = tables.iter().map(|t| t.entity_count()).sum();
            beava_core::agg_state::EntityCountResidentSnapshot::store(total_entities as u64);
        }
        let t_agg = t0.map(|t| t.elapsed());

        // 9. Monotonic counters were bumped while holding `state_tables`, so a
        //    snapshot cannot observe applied table state without the matching
        //    applied LSN/query-time watermark.
        let t_bk_counters = t0.map(|t| t.elapsed());

        // `t_bk_evid` keeps the trace shape stable; the per-stage delta
        // is just two `Instant::now` calls (~5 ns) now that the
        // event-id side-table bookkeeping is gone (events-only).
        let t_bk_evid = t0.map(|t| t.elapsed());

        // 11. Cache on dedupe path.
        if let Some(key_str) = dedupe_str {
            let ack = serde_json::json!({
                "ack_lsn": ack_lsn,
                "idempotent_replay": false,
                "registry_version": registry_version,
            });
            let response_bytes = serde_json::to_vec(&ack)
                .map(Bytes::from)
                .unwrap_or_default();
            let window_ms = descriptor
                .dedupe_window_ms
                .unwrap_or(DEFAULT_DEDUPE_WINDOW_MS);
            self.state.idem_cache.put(
                event_name.to_string(),
                key_str,
                crate::idem_cache::CachedEntry {
                    response_bytes,
                    ack_lsn,
                    inserted_at_ms: now_ms,
                    expires_at_ms: now_ms.saturating_add(window_ms),
                },
            );
        }

        if let (
            Some(t0_inst),
            Some(parse),
            Some(lookup),
            Some(validate),
            Some(wal_b),
            Some(wal_a),
            Some(agg),
            Some(bk_counters),
            Some(bk_evid),
        ) = (
            t0,
            t_parse,
            t_lookup,
            t_validate,
            t_wal_build,
            t_wal_append,
            t_agg,
            t_bk_counters,
            t_bk_evid,
        ) {
            let total = t0_inst.elapsed();
            // gap = ns since the previous push on this thread; "first"
            // for the first push.
            let gap_str = match gap {
                Some(g) => format!("{}", g.as_nanos()),
                None => "first".to_string(),
            };
            eprintln!(
                "TRACE_APPLY ns push: gap={} parse={} lookup={} validate={} wal_build={} wal_append={} agg={} bk_counters={} bk_evid={} bk_dedupe={} bookkeeping={} TOTAL={}",
                gap_str,
                parse.as_nanos(),
                (lookup - parse).as_nanos(),
                (validate - lookup).as_nanos(),
                (wal_b - validate).as_nanos(),
                (wal_a - wal_b).as_nanos(),
                (agg - wal_a).as_nanos(),
                (bk_counters - agg).as_nanos(),
                (bk_evid - bk_counters).as_nanos(),
                (total - bk_evid).as_nanos(),
                (total - agg).as_nanos(),
                total.as_nanos()
            );
        }
        if trace_apply {
            LAST_PUSH_END.with(|cell| cell.set(Some(Instant::now())));
        }

        GlueResponse::PushAck {
            ack_lsn,
            registry_version,
        }
    }
}

/// Schema validation against a `beava_core::row::Row` directly. For each
/// non-optional schema field the row must contain a typed `Value`
/// compatible with the declared `FieldType`.
fn validate_row_against_descriptor(
    descriptor: &beava_core::registry::EventDescriptor,
    row: &beava_core::row::Row,
) -> bool {
    for (field_name, field_type) in &descriptor.schema.fields {
        if descriptor.schema.optional_fields.contains(field_name) {
            continue;
        }
        let val = match row.get(field_name) {
            Some(v) => v,
            None => return false,
        };
        if !value_type_compatible(val, field_type) {
            return false;
        }
    }
    true
}

/// `FieldType` ↔ `Value` compatibility. Numeric coercion (i64 ↔ f64) is
/// permitted because the wire data may be either; the apply path
/// consumes the typed `Value` as-is.
fn value_type_compatible(val: &beava_core::row::Value, ft: &beava_core::schema::FieldType) -> bool {
    use beava_core::row::Value;
    use beava_core::schema::FieldType;
    match ft {
        FieldType::I64 | FieldType::F64 => matches!(val, Value::I64(_) | Value::F64(_)),
        FieldType::Str => matches!(val, Value::Str(_)),
        FieldType::Bool => matches!(val, Value::Bool(_)),
        // Bytes/Datetime/Json: accept any non-null value for forward compat.
        FieldType::Bytes | FieldType::Datetime | FieldType::Json => !matches!(val, Value::Null),
    }
}

/// Extract the dedupe key as a string from a `Row` field. Strings pass
/// through; other types are stringified.
fn extract_dedupe_str_from_row(row: &beava_core::row::Row, key: &str) -> Option<String> {
    use beava_core::row::Value;
    row.get(key).map(|v| match v {
        Value::Str(s) => s.to_string(),
        Value::I64(i) => i.to_string(),
        Value::F64(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Bytes(b) => format!("{:?}", b),
        Value::Datetime(i) => i.to_string(),
        Value::Json(j) => j.to_string(),
        Value::List(l) => format!("{:?}", l),
        Value::Map(m) => format!("{:?}", m),
    })
}

fn row_has_control_chars(row: &Row) -> bool {
    row.iter()
        .any(|(field, value)| string_has_control_chars(field) || value_has_control_chars(value))
}

fn value_has_control_chars(value: &Value) -> bool {
    match value {
        Value::Str(s) => string_has_control_chars(s),
        Value::Json(jv) => json_value_has_control_chars(jv),
        Value::List(values) => values.iter().any(value_has_control_chars),
        Value::Map(map) => map
            .iter()
            .any(|(key, value)| string_has_control_chars(key) || value_has_control_chars(value)),
        _ => false,
    }
}

fn json_value_has_control_chars(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => string_has_control_chars(s),
        serde_json::Value::Array(values) => values.iter().any(json_value_has_control_chars),
        serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
            string_has_control_chars(key) || json_value_has_control_chars(value)
        }),
        _ => false,
    }
}

fn string_has_control_chars(value: &str) -> bool {
    value.chars().any(char::is_control)
}

/// One entry in the `OP_BATCH_GET` request payload.
///
/// - `table`: aggregation node name.
/// - `key`: per-entity scoping value; parsed via `parse_entity_key`.
/// - `features`: optional per-entry filter; `None` returns every
///   feature, `Some(vec)` narrows to those names (omit-on-absent).
///
/// `entity_id` is accepted as a legacy alias of `key` for one release;
/// `from_alias` is set by the custom `Deserialize` so the dispatch loop
/// can emit a WARN once per alias use without false-positives on the
/// canonical path.
struct BatchGetReqEntry {
    table: String,
    key: String,
    features: Option<Vec<String>>,
    /// Internal — not serialised; set by the custom `Deserialize` impl.
    from_alias: bool,
}

// Custom `Deserialize` that tracks whether the source field was `key`
// (canonical) or `entity_id` (legacy alias). Emits a serde error on
// missing-both / both-present.
impl<'de> serde::Deserialize<'de> for BatchGetReqEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct EntryVisitor;
        impl<'de> Visitor<'de> for EntryVisitor {
            type Value = BatchGetReqEntry;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a BatchGetReqEntry map with {table, key|entity_id, features?}")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut table: Option<String> = None;
                let mut key: Option<String> = None;
                let mut features: Option<Vec<String>> = None;
                let mut from_alias = false;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "table" => {
                            if table.is_some() {
                                return Err(de::Error::duplicate_field("table"));
                            }
                            table = Some(map.next_value()?);
                        }
                        "key" => {
                            if key.is_some() {
                                return Err(de::Error::duplicate_field("key"));
                            }
                            key = Some(map.next_value()?);
                        }
                        "entity_id" => {
                            if key.is_some() {
                                return Err(de::Error::custom(
                                    "BatchGetReqEntry: cannot specify both `key` and `entity_id` (entity_id is a deprecated alias)",
                                ));
                            }
                            key = Some(map.next_value()?);
                            from_alias = true;
                        }
                        "features" => {
                            if features.is_some() {
                                return Err(de::Error::duplicate_field("features"));
                            }
                            features = Some(map.next_value()?);
                        }
                        _ => {
                            // Skip unknown fields for forward compatibility.
                            let _ignored: serde::de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                Ok(BatchGetReqEntry {
                    table: table.ok_or_else(|| de::Error::missing_field("table"))?,
                    key: key.ok_or_else(|| de::Error::missing_field("key"))?,
                    features,
                    from_alias,
                })
            }
        }

        deserializer.deserialize_map(EntryVisitor)
    }
}

/// `OP_BATCH_GET` dispatch — walks `{requests:[{table, key,
/// features?}, ...]}`, looks up each tuple, and aggregates into
/// `{results: [...]}` with partial-failure semantics.
///
/// - Per-tuple `{table, entity_id, features: <flat_dict>}` on success.
/// - Per-tuple `{table, entity_id, error: {code: "unknown_table",
///   reason}}` on unknown table — the rest of the batch still completes.
/// - Empty `requests: []` returns `{"results": []}` HTTP 200.
///
/// Empty-string `entity_id` is the global-table sentinel (forwarded as-
/// is to `parse_entity_key`).
///
/// `body_format` is the wire content-type byte (`CT_JSON` or
/// `CT_MSGPACK`). HTTP always passes `CT_JSON`; TCP forwards the frame's
/// byte. Response body is always JSON — the response opcode is
/// `OP_GET_RESPONSE`, whose body shape contract is JSON in v0.
pub fn dispatch_batch_get_sync(
    app: &std::sync::Arc<crate::AppState>,
    body: &Bytes,
    body_format: u8,
) -> GlueResponse {
    use beava_core::wire::{CT_JSON, CT_MSGPACK};

    #[derive(serde::Deserialize)]
    struct BatchGetBody {
        requests: Vec<BatchGetReqEntry>,
    }

    let req: BatchGetBody = match body_format {
        CT_JSON => match serde_json::from_slice(body) {
            Ok(r) => r,
            Err(e) => {
                return GlueResponse::InternalError {
                    reason: e.to_string(),
                };
            }
        },
        CT_MSGPACK => match rmp_serde::from_slice(body) {
            Ok(r) => r,
            Err(e) => {
                return GlueResponse::InternalError {
                    reason: e.to_string(),
                };
            }
        },
        other => {
            return GlueResponse::InternalError {
                reason: format!("unsupported content_type: {other:#04x}"),
            };
        }
    };

    if req.requests.is_empty() {
        let body_bytes = serde_json::to_vec(&serde_json::json!({"results": []}))
            .unwrap_or_else(|_| b"{\"results\":[]}".to_vec());
        return GlueResponse::QueryResult {
            body: Bytes::from(body_bytes),
            format: CT_JSON,
        };
    }

    // Emit a `tracing::warn!` once per entry using the legacy
    // `entity_id` alias. Detection is strict: false-positives on the
    // canonical `key` path would defeat the alias-removal warning.
    for entry in &req.requests {
        if entry.from_alias {
            tracing::warn!(
                kind = "batch_get.entity_id_alias",
                table = %entry.table,
                "BatchGetReqEntry: deprecated 'entity_id' field name; rename to 'key'; alias removed in v0.0.x"
            );
        }
    }

    // Compute `query_time_ms` once for the whole batch (matches
    // `dispatch_get_batch`'s policy).
    let query_time_ms = {
        let raw = app
            .dev_agg
            .query_time_ms
            .load(std::sync::atomic::Ordering::Acquire);
        if raw == 0 {
            0i64
        } else {
            raw as i64
        }
    };

    let registry = &app.dev_agg.registry;

    // Whole-batch reject on registry-typo: if any entry's `features`
    // filter mentions a name not in that table's descriptor, abort with
    // `InternalError`. Per-tuple errors (`unknown_table`,
    // `key_parse_failure`, per-entity sparsity) stay per-tuple in the
    // dispatch loop below.
    for entry in &req.requests {
        let Some(filter) = entry.features.as_deref() else {
            continue;
        };
        let descriptor = match registry.compiled_aggregation(&entry.table) {
            // Unknown table here is a per-tuple error (not D-06 first clause);
            // let the dispatch loop below handle it.
            Some(d) => d,
            None => continue,
        };
        let mut unknown: Vec<String> = Vec::new();
        for name in filter {
            if !descriptor.features.iter().any(|f| &f.feature_name == name) {
                unknown.push(name.clone());
            }
        }
        if !unknown.is_empty() {
            return GlueResponse::InternalError {
                reason: format!(
                    "feature_not_found: missing={:?} table={}",
                    unknown, entry.table
                ),
            };
        }
    }

    // 4. Acquire the state_tables lock once for the whole batch (matches
    //    `dispatch_get_batch`'s single-critical-section discipline).
    let tables = app.dev_agg.state_tables.lock();

    let mut results: Vec<serde_json::Value> = Vec::with_capacity(req.requests.len());

    for entry in &req.requests {
        // Look up the table by name in the registry as an aggregation node.
        let descriptor = match registry.compiled_aggregation(&entry.table) {
            Some(d) => d,
            None => {
                // Flat per-tuple error tuple (no table/entity_id wrapping)
                // per the get-batch wire contract.
                results.push(serde_json::json!({
                    "error": {
                        "code": "unknown_table",
                        "message": format!(
                            "table '{}' is not a registered aggregation",
                            entry.table
                        ),
                    },
                }));
                continue;
            }
        };

        // Per ADR-003: forward `key` verbatim to parse_entity_key. For
        // `key_cols: []` (global table) the empty string parses to a 0-arity
        // key; the register-time path makes empty group_keys legal.
        let entity_key =
            match crate::feature_query::parse_entity_key(&entry.key, &descriptor.group_keys) {
                Some(k) => k,
                None => {
                    // Flat per-tuple error tuple.
                    results.push(serde_json::json!({
                        "error": {
                            "code": "key_parse_failure",
                            "message": format!(
                                "key '{}' does not match table '{}' group_keys arity ({})",
                                entry.key,
                                entry.table,
                                descriptor.group_keys.len()
                            ),
                        },
                    }));
                    continue;
                }
            };

        // FLAT row response with per-entry features filter. Drop the
        // `{table, entity_id, features:{...}}` envelope.
        let mut feature_map = serde_json::Map::new();
        if let Some(table) = tables.get(descriptor.agg_id as usize) {
            for (idx, named_op) in descriptor.features.iter().enumerate() {
                // D-06 features-filter narrowing pass.
                if let Some(filter) = entry.features.as_deref() {
                    if !filter.iter().any(|f| f == &named_op.feature_name) {
                        continue;
                    }
                }
                if let Some(v) = table.query_feature(&entity_key, idx, query_time_ms) {
                    feature_map.insert(
                        named_op.feature_name.clone(),
                        crate::feature_query::value_to_json(v),
                    );
                }
                // D-06 omit-on-absent: when query_feature returns None, do
                // NOT insert the key. Cold-start entity → empty feature_map.
            }
        }

        // FLAT row — feature_map IS the result. Cold-start = `{}` per the
        // wire-spec ("Per-entry cold-start is `{}`, not an error").
        results.push(serde_json::Value::Object(feature_map));
    }
    drop(tables);

    let body_json = serde_json::json!({"results": results});
    let body_bytes = match serde_json::to_vec(&body_json) {
        Ok(b) => b,
        Err(e) => {
            return GlueResponse::InternalError {
                reason: e.to_string(),
            };
        }
    };
    GlueResponse::QueryResult {
        body: Bytes::from(body_bytes),
        format: CT_JSON,
    }
}

// ─── Plan 13.4-08 (D-03 USER-LOCKED) — OP_RESET dispatch ──────────────────────
//
// Honors the boot-time `effective_test_mode` flag stamped on AppState. When
// the flag is FALSE (production-by-default boot) the dispatch returns a
// structured `reset_disabled_in_production` error — HTTP 403 / TCP
// OP_ERROR_RESPONSE (0xFFFF). When TRUE the dispatch:
//
// 1. Acquires the `state_tables` Mutex (single-writer apply discipline).
// 2. Empties EVERY per-entity aggregation state by clearing the `Vec`.
// 3. Drops every registered descriptor + every compiled chain/aggregation
//    via `Registry::clear()`. The clear() call bumps `registry_version` by
//    1 so any cached client `registry_version` becomes stale.
// 4. Resets `next_event_id` and `query_time_ms` to 0 (cold-start state).
// 5. Returns `GlueResponse::ResetOk { registry_version }` — the encoder
//    layer maps this to HTTP 200 + `{"reset": true, "registry_version": N}`
//    or TCP OP_GET_RESPONSE with the same body.
//
// Per-event WAL ring buffers and the legacy /register WAL sink are NOT
// touched here — the in-memory state is the source of truth for v0; on
// restart the disk-mode path replays the WAL and rebuilds, but a fresh
// reset followed by a re-register starts from a clean slate. Memory mode
// has no WAL by design.
//
// **Threat model coverage** (Plan 13.4-08 §threat_model):
// - T-13.4-08-01 (Tampering: client wipes prod state): the
//   `if !state.effective_test_mode` early-return IS the defense.
// - T-13.4-08-03 (Spoofing: env var read at runtime): the flag is set ONCE
//   at bind time and never re-read; runtime escalation impossible.
pub fn dispatch_reset_sync(state: &std::sync::Arc<crate::AppState>) -> GlueResponse {
    if !state.effective_test_mode {
        return GlueResponse::ResetForbidden;
    }

    // 1+2 — acquire state_tables lock + empty every per-entity table.
    {
        let mut tables = state.dev_agg.state_tables.lock();
        tables.clear();
    }

    // 3 — drop every descriptor + bump registry_version.
    state.dev_agg.registry.clear();

    // 4 — reset cold-start counters. event_id is cumulative for the apply
    // path; resetting to 0 means the next push starts from event_id=1
    // again. query_time_ms is the latest server-side wall-clock the apply
    // path saw; resetting to 0 makes the next GET fall back to the live
    // wall clock until the first post-reset push lands.
    state
        .dev_agg
        .next_event_id
        .store(0, std::sync::atomic::Ordering::Release);
    state
        .dev_agg
        .query_time_ms
        .store(0, std::sync::atomic::Ordering::Release);

    let registry_version = state.dev_agg.registry.version();
    tracing::info!(
        target: "beava.server",
        kind = "server.reset_completed",
        registry_version,
        "OP_RESET completed: state + registry cleared (D-03 USER-LOCKED)"
    );
    GlueResponse::ResetOk { registry_version }
}
