//! Recovery on startup.
//!
//! 1. `load_snapshot_if_any(dir, dev_agg)` — descending-LSN scan; first valid
//!    snapshot wins; install registry descriptors + state tables; return its
//!    snapshot LSN plus the applied data-plane watermark stored in the body.
//!    Empty dir or all-corrupt files → 0 (cold start).
//! 2. `replay_wal_from_lsn(wal_dir, snapshot_lsn, dev_agg)` — replay every WAL
//!    record with `lsn > snapshot_lsn` in LSN order: `Event` decodes its
//!    payload and feeds `apply_event_to_aggregations`; `RegistryBump`
//!    bincode-decodes a `RegistryBumpPayload` and applies it.
//!
//! Apply-after-fsync holds on replay because every WAL record is durable by
//! definition; LSN order is apply order.

use crate::register::RegistryBumpPayload;
use crate::registry_debug::DevAggState;
use beava_core::row::{Row, Value};
use beava_core::snapshot_body::SnapshotBody;
use beava_persistence::{list_snapshots, Lsn, PersistError, RecordType, SnapshotReader, WalReader};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// Outcome counters reported back from `replay_wal_from_lsn`.
#[derive(Debug, Default)]
pub struct RecoveryOutcome {
    pub installed_from_snapshot: bool,
    pub snapshot_lsn: Lsn,
    pub replay_event_count: u64,
    pub replay_registry_bumps: u64,
    pub quarantined_records: u64,
    pub applied_registry_bump_after_snapshot: bool,
    pub last_lsn: Lsn,
}

/// Snapshot recovery result. `snapshot_lsn` comes from the snapshot header and
/// gates legacy persistence-WAL replay. `applied_lsn` comes from the snapshot
/// body and gates hand-rolled data-plane WAL replay, because older snapshots
/// can have a header LSN that does not match the data-plane state already
/// serialized into the body.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotLoadOutcome {
    pub snapshot_lsn: Lsn,
    pub applied_lsn: Lsn,
}

#[derive(Debug, Clone, Copy)]
enum WalQuarantineKind {
    HandrolledJsonBody,
    HandrolledMsgpackBody,
    PersistenceEventPayload,
}

impl WalQuarantineKind {
    fn as_str(self) -> &'static str {
        match self {
            WalQuarantineKind::HandrolledJsonBody => "handrolled-json-body",
            WalQuarantineKind::HandrolledMsgpackBody => "handrolled-msgpack-body",
            WalQuarantineKind::PersistenceEventPayload => "persistence-event-payload",
        }
    }
}

fn wal_quarantine_marker_path(wal_dir: &Path, lsn: Lsn, kind: WalQuarantineKind) -> PathBuf {
    wal_dir
        .join("quarantine")
        .join(format!("lsn-{lsn:016x}-{}.json", kind.as_str()))
}

fn wal_quarantine_marker_exists(wal_dir: &Path, lsn: Lsn, kind: WalQuarantineKind) -> bool {
    wal_quarantine_marker_path(wal_dir, lsn, kind).exists()
}

fn quarantine_wal_decode_failure(
    wal_dir: &Path,
    lsn: Lsn,
    kind: WalQuarantineKind,
    error: &dyn std::fmt::Display,
) {
    let marker = wal_quarantine_marker_path(wal_dir, lsn, kind);
    if marker.exists() {
        return;
    }

    let Some(dir) = marker.parent() else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!(
            target: "beava.recovery",
            kind = "recovery.wal_quarantine_write_failed",
            lsn,
            quarantine_kind = kind.as_str(),
            error = %e,
            "failed to create WAL quarantine directory"
        );
        return;
    }

    let body = serde_json::json!({
        "lsn": lsn,
        "kind": kind.as_str(),
        "reason": error.to_string(),
    });
    let bytes = serde_json::to_vec_pretty(&body).unwrap_or_default();
    let tmp = marker.with_extension(format!("json.tmp-{}", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, bytes) {
        tracing::warn!(
            target: "beava.recovery",
            kind = "recovery.wal_quarantine_write_failed",
            lsn,
            quarantine_kind = kind.as_str(),
            error = %e,
            "failed to write WAL quarantine marker"
        );
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &marker) {
        let _ = std::fs::remove_file(&tmp);
        tracing::warn!(
            target: "beava.recovery",
            kind = "recovery.wal_quarantine_write_failed",
            lsn,
            quarantine_kind = kind.as_str(),
            marker = %marker.display(),
            error = %e,
            "failed to commit WAL quarantine marker"
        );
        return;
    }

    tracing::warn!(
        target: "beava.recovery",
        kind = "recovery.wal_record_quarantined",
        lsn,
        quarantine_kind = kind.as_str(),
        marker = %marker.display(),
        error = %error,
        "WAL record decode failed; quarantined for future recovery passes"
    );
}

/// Scan `snapshot_dir` for the highest-LSN valid snapshot; install its
/// registry descriptors + state tables into `app_state`. Returns the
/// snapshot's LSN, or 0 if no valid snapshot exists (cold start).
pub fn load_snapshot_if_any(
    snapshot_dir: &Path,
    dev_agg: &DevAggState,
) -> Result<SnapshotLoadOutcome, PersistError> {
    let snaps = list_snapshots(snapshot_dir)?;
    for (lsn, path) in snaps {
        match SnapshotReader::open(&path) {
            Ok((header, body)) => match SnapshotBody::decode(&body) {
                Ok(snapshot_body) => {
                    let (registry_only, state_tables, next_event_id, query_time_ms) =
                        snapshot_body.into_parts();
                    dev_agg.registry.install_from_descriptors(&registry_only);
                    {
                        // Registry.install_from_descriptors assigns agg_ids
                        // deterministically (registration order); grow the
                        // table vector to fit and place each entry at its
                        // own slot via the name → agg_id reverse lookup.
                        let new_next_agg_id = dev_agg.registry.next_agg_id() as usize;
                        let mut tables = dev_agg.state_tables.lock();
                        tables.clear();
                        beava_core::agg_state_table::ensure_capacity_for(
                            &mut tables,
                            new_next_agg_id,
                        );
                        for (node_name, entries) in state_tables {
                            let agg_desc = match dev_agg.registry.compiled_aggregation(&node_name) {
                                Some(d) => d,
                                None => continue,
                            };
                            let agg_id = agg_desc.agg_id as usize;
                            let group_key_types = dev_agg
                                .registry
                                .get_event_descriptor(&agg_desc.source_node_name)
                                .map(|event| {
                                    agg_desc
                                        .group_keys
                                        .iter()
                                        .filter_map(|key| event.schema.fields.get(key).cloned())
                                        .collect::<Vec<_>>()
                                });
                            let tbl = &mut tables[agg_id];
                            for (key, ops) in entries {
                                tbl.insert_from_entity_key_with_types(
                                    key,
                                    ops,
                                    group_key_types.as_deref(),
                                );
                            }
                        }
                    }
                    dev_agg
                        .next_event_id
                        .store(next_event_id, Ordering::Relaxed);
                    if query_time_ms > 0 {
                        dev_agg
                            .query_time_ms
                            .store(query_time_ms as u64, Ordering::Relaxed);
                    }
                    tracing::info!(
                        target: "beava.recovery",
                        kind = "recovery.snapshot_loaded",
                        snapshot_lsn = lsn,
                        applied_lsn = next_event_id,
                        registry_version = header.registry_version,
                        "loaded snapshot"
                    );
                    return Ok(SnapshotLoadOutcome {
                        snapshot_lsn: lsn,
                        applied_lsn: next_event_id,
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        target: "beava.recovery",
                        kind = "recovery.snapshot_decode_failed",
                        snapshot_lsn = lsn,
                        error = %e,
                        "snapshot body decode failed; trying older snapshot"
                    );
                    continue;
                }
            },
            Err(e) => {
                tracing::warn!(
                    target: "beava.recovery",
                    kind = "recovery.snapshot_open_failed",
                    snapshot_lsn = lsn,
                    error = %e,
                    "snapshot open/verify failed; trying older snapshot"
                );
                continue;
            }
        }
    }
    Ok(SnapshotLoadOutcome::default())
}

/// JSON shape of a WAL Event record's payload (matches push.rs encoder).
#[derive(Debug, Deserialize)]
struct WalEventPayload {
    // reason: serde-shape parity with the WAL Event record format; recovery
    // doesn't consult `v` directly, but the field must be deserialized to
    // round-trip the format.
    #[allow(dead_code)]
    v: u64,
    // reason: see `v` above — serde-shape parity with the WAL Event record.
    #[allow(dead_code)]
    rv: u64,
    s: String,
    et: i64,
    b: serde_json::Value,
}

/// A single decoded record from the hand-rolled WAL file.
struct HandrolledWalRecord {
    lsn: Lsn,
    body_format: u8,
    // reason: parsed from the v=2 record header for completeness; recovery
    // doesn't depend on the per-record registry version.
    #[allow(dead_code)]
    rv: u32,
    et_ms: i64,
    event_name: String,
    body: Vec<u8>,
}

/// Parse all hand-rolled binary records from a contiguous byte slice.
///
/// v=2 format:
/// `[u8 v=2][u8 body_format][u32 rv BE][u64 et_ms BE]
///  [u16 name_len BE][N bytes name][u32 body_len BE][M bytes body]`
///
/// v=3 format:
/// `[u8 v=3][u64 assigned_lsn BE][u8 body_format][u32 rv BE][u64 et_ms BE]
///  [u16 name_len BE][N bytes name][u32 body_len BE][M bytes body]`
///
/// Stops at first byte that is not 0x02/0x03 (unknown version) or if bytes are
/// insufficient (truncated record — treat as EOF).
fn parse_handrolled_records(data: &[u8], base_lsn: Lsn) -> Vec<HandrolledWalRecord> {
    let mut records = Vec::new();
    let mut pos = 0usize;

    loop {
        if pos >= data.len() {
            break;
        }

        let version = data[pos];
        if version != 0x02 && version != 0x03 {
            break;
        }
        pos += 1;

        let assigned_lsn = if version == 0x03 {
            if pos + 8 > data.len() {
                break;
            }
            let lsn = u64::from_be_bytes(data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            Some(lsn)
        } else {
            None
        };

        // Remaining fixed header is 1+4+8+2 = 15 bytes.
        if pos + 15 > data.len() {
            break;
        }

        let body_format = data[pos];
        pos += 1;

        let rv = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap());
        pos += 4;

        let et_ms = i64::from_be_bytes(data[pos..pos + 8].try_into().unwrap());
        pos += 8;

        let name_len = u16::from_be_bytes(data[pos..pos + 2].try_into().unwrap()) as usize;
        pos += 2;

        if pos + name_len + 4 > data.len() {
            break;
        }

        let event_name = match std::str::from_utf8(&data[pos..pos + name_len]) {
            Ok(s) => s.to_string(),
            Err(_) => break,
        };
        pos += name_len;

        let body_len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;

        if pos + body_len > data.len() {
            break;
        }

        let body = data[pos..pos + body_len].to_vec();
        pos += body_len;

        records.push(HandrolledWalRecord {
            lsn: assigned_lsn.unwrap_or_else(|| base_lsn.saturating_add(pos as u64)),
            body_format,
            rv,
            et_ms,
            event_name,
            body,
        });
    }

    records
}

/// Replay hand-rolled WAL files (`*.wal`) from `wal_dir`.
///
/// Hand-rolled WAL files are written by `WalBufferRing` + `WalWriter` in the
/// binary push-record format (see `dispatch_push_sync` in `apply_shard`),
/// distinct from the `beava-persistence` `WalSink` format (`*.log`). Returns
/// recovery counters and replays only records with `lsn > from_lsn_exclusive`.
pub fn replay_handrolled_wal_dir(
    wal_dir: &Path,
    from_lsn_exclusive: Lsn,
    dev_agg: &DevAggState,
) -> Result<RecoveryOutcome, std::io::Error> {
    use beava_core::wire::CT_MSGPACK;
    let mut outcome = RecoveryOutcome {
        snapshot_lsn: from_lsn_exclusive,
        ..Default::default()
    };

    if !wal_dir.exists() {
        return Ok(outcome);
    }

    // *.wal filenames are LSN-prefixed, so lexicographic sort = LSN order.
    let mut wal_files: Vec<std::path::PathBuf> = std::fs::read_dir(wal_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("wal"))
        .collect();
    wal_files.sort();

    let mut base_lsn = 0;

    for wal_file in &wal_files {
        let data = std::fs::read(wal_file)?;
        let records = parse_handrolled_records(&data, base_lsn);
        base_lsn = base_lsn.saturating_add(data.len() as u64);

        for rec in records {
            if rec.lsn <= from_lsn_exclusive {
                continue;
            }
            outcome.last_lsn = outcome.last_lsn.max(rec.lsn);

            let quarantine_kind = if rec.body_format == CT_MSGPACK {
                WalQuarantineKind::HandrolledMsgpackBody
            } else {
                WalQuarantineKind::HandrolledJsonBody
            };
            if wal_quarantine_marker_exists(wal_dir, rec.lsn, quarantine_kind) {
                outcome.quarantined_records += 1;
                tracing::debug!(
                    target: "beava.recovery",
                    kind = "recovery.wal_quarantine_skip",
                    lsn = rec.lsn,
                    quarantine_kind = quarantine_kind.as_str(),
                    "skipping quarantined WAL record"
                );
                continue;
            }

            let row: Row = if rec.body_format == CT_MSGPACK {
                match rmp_serde::from_slice::<Row>(&rec.body) {
                    Ok(r) => r,
                    Err(e) => {
                        quarantine_wal_decode_failure(wal_dir, rec.lsn, quarantine_kind, &e);
                        outcome.quarantined_records += 1;
                        continue;
                    }
                }
            } else {
                match serde_json::from_slice::<Row>(&rec.body) {
                    Ok(r) => r,
                    Err(e) => {
                        quarantine_wal_decode_failure(wal_dir, rec.lsn, quarantine_kind, &e);
                        outcome.quarantined_records += 1;
                        continue;
                    }
                }
            };

            // Thread the source's cold_after_ms through apply so cold-TTL
            // eviction during replay reproduces live state. Missing event
            // descriptors yield `None` (defensive — register records replay
            // before event records).
            let cold_after_ms = dev_agg
                .registry
                .get_event_descriptor(&rec.event_name)
                .and_then(|d| d.cold_after_ms);
            {
                let mut tables = dev_agg.state_tables.lock();
                // Replay variant: route the event ONLY to aggregations
                // whose owning derivation was registered at or before the
                // event's stamped `rv`. Without this filter, a force-
                // replace that swaps `UserTxn(cnt)` for `UserTxn(total)`
                // would credit the pre-replace events to the post-replace
                // aggregation on recovery (bumps replay first, then
                // ALL events replay against the final registry). See
                // `Registry::compiled_aggregations_for_source_at_rv` for
                // the rationale.
                beava_core::agg_apply::apply_event_to_aggregations_replay(
                    &rec.event_name,
                    &row,
                    rec.et_ms,
                    rec.lsn,
                    rec.rv as u64,
                    &dev_agg.registry,
                    &mut tables,
                    cold_after_ms,
                );
            }

            dev_agg.next_event_id.fetch_max(rec.lsn, Ordering::Relaxed);
            if rec.et_ms > 0 {
                dev_agg
                    .query_time_ms
                    .fetch_max(rec.et_ms as u64, Ordering::Relaxed);
            }
            outcome.replay_event_count += 1;
        }
    }

    outcome.installed_from_snapshot = false;
    Ok(outcome)
}

/// Replay every WAL record in `wal_dir` whose LSN > `from_lsn_exclusive`,
/// applying them to `app_state`. Returns counters + last LSN seen.
///
/// Bumps `next_event_id` and `query_time_ms` as records replay so the
/// post-recovery server picks up monotonic counters consistent with the WAL.
pub fn replay_wal_from_lsn(
    wal_dir: &Path,
    from_lsn_exclusive: Lsn,
    dev_agg: &DevAggState,
) -> Result<RecoveryOutcome, PersistError> {
    let mut outcome = RecoveryOutcome {
        snapshot_lsn: from_lsn_exclusive,
        ..Default::default()
    };
    if !wal_dir.exists() {
        return Ok(outcome);
    }
    let records = WalReader::read_all(wal_dir)?;
    for rec in records {
        match rec.record_type {
            RecordType::Event => {
                if rec.lsn <= from_lsn_exclusive {
                    continue;
                }
                outcome.last_lsn = outcome.last_lsn.max(rec.lsn);
                let quarantine_kind = WalQuarantineKind::PersistenceEventPayload;
                if wal_quarantine_marker_exists(wal_dir, rec.lsn, quarantine_kind) {
                    outcome.quarantined_records += 1;
                    tracing::debug!(
                        target: "beava.recovery",
                        kind = "recovery.wal_quarantine_skip",
                        lsn = rec.lsn,
                        quarantine_kind = quarantine_kind.as_str(),
                        "skipping quarantined WAL record"
                    );
                    continue;
                }
                let payload: WalEventPayload = match serde_json::from_slice(&rec.payload) {
                    Ok(p) => p,
                    Err(e) => {
                        quarantine_wal_decode_failure(wal_dir, rec.lsn, quarantine_kind, &e);
                        outcome.quarantined_records += 1;
                        continue;
                    }
                };
                let row = json_object_to_row(&payload.b);
                let cold_after_ms = dev_agg
                    .registry
                    .get_event_descriptor(&payload.s)
                    .and_then(|d| d.cold_after_ms);
                {
                    let mut tables = dev_agg.state_tables.lock();
                    // Same per-event registry-version filter as the
                    // hand-rolled `*.wal` replay path — see comment in
                    // `replay_handrolled_wal_dir`.
                    beava_core::agg_apply::apply_event_to_aggregations_replay(
                        &payload.s,
                        &row,
                        payload.et,
                        rec.lsn,
                        payload.rv,
                        &dev_agg.registry,
                        &mut tables,
                        cold_after_ms,
                    );
                }
                dev_agg.next_event_id.fetch_max(rec.lsn, Ordering::Relaxed);
                if payload.et > 0 {
                    dev_agg
                        .query_time_ms
                        .fetch_max(payload.et as u64, Ordering::Relaxed);
                }
                outcome.replay_event_count += 1;
            }
            RecordType::RegistryBump => match RegistryBumpPayload::decode(&rec.payload) {
                Ok(bump) => {
                    outcome.last_lsn = outcome.last_lsn.max(rec.lsn);
                    if bump.new_version <= dev_agg.registry.version() {
                        continue;
                    }
                    match crate::register::apply_registry_bump(&dev_agg.registry, bump) {
                        Ok(()) => {
                            {
                                let mut tables = dev_agg.state_tables.lock();
                                beava_core::agg_state_table::ensure_capacity_for(
                                    &mut tables,
                                    dev_agg.registry.next_agg_id() as usize,
                                );
                            }
                            outcome.replay_registry_bumps += 1;
                            outcome.applied_registry_bump_after_snapshot = true;
                        }
                        Err(e) => {
                            // Apply-after-fsync invariant: a durable RegistryBump
                            // that fails to apply is a hard recovery failure —
                            // silently skipping would let durable corruption hide.
                            tracing::error!(
                                target: "beava.recovery",
                                kind = "recovery.registry_bump_apply_failed",
                                lsn = rec.lsn,
                                error = %e,
                                "RegistryBump apply failed during replay"
                            );
                            return Err(PersistError::Io(std::io::Error::other(format!(
                                "RegistryBump apply failed at LSN {}: {e}",
                                rec.lsn
                            ))));
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        target: "beava.recovery",
                        kind = "recovery.registry_bump_decode_failed",
                        lsn = rec.lsn,
                        error = %e,
                        "RegistryBump payload decode failed during replay"
                    );
                    return Err(PersistError::Io(std::io::Error::other(format!(
                        "RegistryBump decode failed at LSN {}: {e}",
                        rec.lsn
                    ))));
                }
            },
        }
    }
    outcome.installed_from_snapshot = from_lsn_exclusive > 0;
    tracing::debug!(
        target: "beava.recovery",
        kind = "recovery.complete",
        snapshot_lsn = outcome.snapshot_lsn,
        events_replayed = outcome.replay_event_count,
        registry_bumps_replayed = outcome.replay_registry_bumps,
        quarantined_records = outcome.quarantined_records,
        last_lsn = outcome.last_lsn,
        "recovery complete"
    );
    Ok(outcome)
}

fn json_object_to_row(jv: &serde_json::Value) -> Row {
    let mut row = Row::new();
    if let Some(obj) = jv.as_object() {
        for (field, jv) in obj {
            let v = match jv {
                serde_json::Value::Null => Value::Null,
                serde_json::Value::Bool(b) => Value::Bool(*b),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Value::I64(i)
                    } else if let Some(f) = n.as_f64() {
                        Value::F64(f)
                    } else {
                        Value::Null
                    }
                }
                serde_json::Value::String(s) => Value::Str(s.clone().into()),
                _ => Value::Null,
            };
            row = row.with_field(field.as_str(), v);
        }
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use beava_core::agg_op::AggOp;
    use beava_core::agg_state::CountState;
    use beava_core::agg_state_table::{ensure_capacity_for, AggStateTable, EntityKey};
    use beava_core::registry::Registry;
    use beava_persistence::{RecordType, SnapshotWriter, WalRecord};
    use compact_str::CompactString;
    use serde_json::json;
    use smallvec::smallvec;
    use std::sync::Arc;

    fn txn_register_payload() -> crate::register::RegisterPayload {
        serde_json::from_value(json!({
            "nodes": [
                {
                    "kind": "event",
                    "name": "Txn",
                    "schema": {"fields": {
                        "event_time": "i64",
                        "user_id": "str",
                        "amount": "f64"
                    }, "optional_fields": []}
                },
                {
                    "kind": "derivation",
                    "name": "TxnAgg",
                    "output_kind": "table",
                    "upstreams": ["Txn"],
                    "ops": [{"op": "group_by", "keys": ["user_id"], "agg": {
                        "cnt": {"op": "count", "params": {}}
                    }}],
                    "schema": {"fields": {"user_id": "str", "cnt": "i64"}, "optional_fields": []},
                    "table_primary_key": ["user_id"]
                }
            ]
        }))
        .expect("valid register payload")
    }

    fn install_txn_registry(dev_agg: &DevAggState) {
        let payload = txn_register_payload();
        let bump = crate::register::RegistryBumpPayload {
            new_version: 1,
            payload_nodes: payload.nodes,
            force_removed_descriptors: Vec::new(),
        };
        crate::register::apply_registry_bump(&dev_agg.registry, bump)
            .expect("install txn registry");
        let mut tables = dev_agg.state_tables.lock();
        ensure_capacity_for(&mut tables, dev_agg.registry.next_agg_id() as usize);
    }

    fn put_alice_count(dev_agg: &DevAggState, count: u64) {
        let mut tables = dev_agg.state_tables.lock();
        ensure_capacity_for(&mut tables, 1);
        let mut table = AggStateTable::new();
        let entity_key = EntityKey(smallvec![(
            CompactString::from("user_id"),
            Value::Str(CompactString::from("alice")),
        )]);
        table.insert_from_entity_key(entity_key, vec![AggOp::Count(CountState { n: count })]);
        tables[0] = table;
    }

    fn alice_count(dev_agg: &DevAggState) -> u64 {
        let tables = dev_agg.state_tables.lock();
        let Some(ops) = tables
            .first()
            .and_then(|table| table.single_str.get("alice"))
        else {
            return 0;
        };
        match ops.first() {
            Some(AggOp::Count(count)) => count.n,
            _ => 0,
        }
    }

    fn encode_handrolled_v3(
        buf: &mut Vec<u8>,
        lsn: Lsn,
        body_format: u8,
        rv: u32,
        event_name: &str,
        body: &[u8],
    ) {
        buf.push(0x03);
        buf.extend_from_slice(&lsn.to_be_bytes());
        buf.push(body_format);
        buf.extend_from_slice(&rv.to_be_bytes());
        buf.extend_from_slice(&(123i64).to_be_bytes());
        buf.extend_from_slice(&(event_name.len() as u16).to_be_bytes());
        buf.extend_from_slice(event_name.as_bytes());
        buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
        buf.extend_from_slice(body);
    }

    #[test]
    fn handrolled_v3_records_use_persisted_lsn() {
        let mut bytes = Vec::new();
        let body = br#"{"user_id":"alice","amount":1.0}"#;
        let name = b"Txn";

        bytes.push(0x03);
        bytes.extend_from_slice(&10_000u64.to_be_bytes());
        bytes.push(beava_core::wire::CT_JSON);
        bytes.extend_from_slice(&7u32.to_be_bytes());
        bytes.extend_from_slice(&(123i64).to_be_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
        bytes.extend_from_slice(body);

        let records = parse_handrolled_records(&bytes, 0);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].lsn, 10_000);
        assert_eq!(records[0].rv, 7);
        assert_eq!(records[0].event_name, "Txn");
    }

    #[test]
    fn handrolled_decode_failure_writes_quarantine_marker() {
        let wal = tempfile::tempdir().unwrap();
        let registry = Arc::new(Registry::new());
        let dev_agg = DevAggState::new(registry);
        let lsn: Lsn = 379_827;
        let body = b"not-json";
        let name = b"Txn";

        let mut bytes = Vec::new();
        bytes.push(0x03);
        bytes.extend_from_slice(&lsn.to_be_bytes());
        bytes.push(beava_core::wire::CT_JSON);
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&(123i64).to_be_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
        bytes.extend_from_slice(body);
        std::fs::write(wal.path().join("wal-0000000000000000.wal"), bytes).unwrap();

        let outcome = replay_handrolled_wal_dir(wal.path(), 0, &dev_agg).expect("replay");
        assert_eq!(outcome.last_lsn, lsn);
        assert_eq!(outcome.replay_event_count, 0);
        assert_eq!(outcome.quarantined_records, 1);
        assert!(wal_quarantine_marker_exists(
            wal.path(),
            lsn,
            WalQuarantineKind::HandrolledJsonBody
        ));

        let outcome = replay_handrolled_wal_dir(wal.path(), 0, &dev_agg).expect("replay again");
        assert_eq!(outcome.last_lsn, lsn);
        assert_eq!(outcome.quarantined_records, 1);
    }

    #[test]
    fn handrolled_msgpack_decode_failure_writes_quarantine_marker() {
        let wal = tempfile::tempdir().unwrap();
        let registry = Arc::new(Registry::new());
        let dev_agg = DevAggState::new(registry);
        let lsn: Lsn = 379_827;

        let mut bytes = Vec::new();
        encode_handrolled_v3(
            &mut bytes,
            lsn,
            beava_core::wire::CT_MSGPACK,
            1,
            "Txn",
            &[0xc1],
        );
        std::fs::write(wal.path().join("wal-0000000000000000.wal"), bytes).unwrap();

        let outcome = replay_handrolled_wal_dir(wal.path(), 0, &dev_agg).expect("replay");
        assert_eq!(outcome.last_lsn, lsn);
        assert_eq!(outcome.replay_event_count, 0);
        assert_eq!(outcome.quarantined_records, 1);
        assert!(wal_quarantine_marker_exists(
            wal.path(),
            lsn,
            WalQuarantineKind::HandrolledMsgpackBody
        ));
    }

    #[test]
    fn snapshot_load_body_applied_lsn_gates_handrolled_replay() {
        let wal = tempfile::tempdir().unwrap();
        let snap = tempfile::tempdir().unwrap();
        let registry = Arc::new(Registry::new());
        let dev_agg = DevAggState::new(registry);

        install_txn_registry(&dev_agg);
        put_alice_count(&dev_agg, 1);
        dev_agg.next_event_id.store(100, Ordering::Relaxed);

        let body = {
            let registry_snap = dev_agg.registry.snapshot();
            let tables = dev_agg.state_tables.lock();
            SnapshotBody::from_live(&registry_snap, &tables, 100, 123)
        };
        let encoded = body.encode().expect("encode snapshot");
        SnapshotWriter::write_with_stats(snap.path(), 5, body.registry.version, &encoded)
            .expect("write snapshot with older header LSN");

        let mut wal_bytes = Vec::new();
        encode_handrolled_v3(
            &mut wal_bytes,
            90,
            beava_core::wire::CT_JSON,
            dev_agg.registry.version() as u32,
            "Txn",
            br#"{"user_id":"alice","amount":1.0}"#,
        );
        std::fs::write(wal.path().join("wal-0000000000000000.wal"), wal_bytes).unwrap();

        let registry = Arc::new(Registry::new());
        let recovered = DevAggState::new(registry);
        let loaded = load_snapshot_if_any(snap.path(), &recovered).expect("load snapshot");
        assert_eq!(loaded.snapshot_lsn, 5);
        assert_eq!(loaded.applied_lsn, 100);
        assert_eq!(alice_count(&recovered), 1);

        let replay =
            replay_handrolled_wal_dir(wal.path(), loaded.applied_lsn, &recovered).expect("replay");
        assert_eq!(replay.replay_event_count, 0);
        assert_eq!(
            alice_count(&recovered),
            1,
            "using the snapshot body watermark must not double-apply covered v3 WAL records"
        );
    }

    #[test]
    fn persistence_event_decode_failure_writes_quarantine_marker() {
        let wal = tempfile::tempdir().unwrap();
        let registry = Arc::new(Registry::new());
        let dev_agg = DevAggState::new(registry);
        let lsn: Lsn = 379_827;

        let mut writer =
            beava_persistence::WalWriter::open(wal.path(), 100, dev_agg.registry.version() as u32)
                .expect("open wal writer");
        writer
            .append(&WalRecord {
                lsn,
                record_type: RecordType::Event,
                payload: b"not-json".to_vec(),
            })
            .expect("append event");
        writer.sync_data().expect("sync wal");
        drop(writer);

        let outcome = replay_wal_from_lsn(wal.path(), 0, &dev_agg).expect("replay wal");
        assert_eq!(outcome.last_lsn, lsn);
        assert_eq!(outcome.replay_event_count, 0);
        assert_eq!(outcome.quarantined_records, 1);
        assert!(wal_quarantine_marker_exists(
            wal.path(),
            lsn,
            WalQuarantineKind::PersistenceEventPayload
        ));

        let outcome = replay_wal_from_lsn(wal.path(), 0, &dev_agg).expect("replay wal again");
        assert_eq!(outcome.last_lsn, lsn);
        assert_eq!(outcome.quarantined_records, 1);
    }

    #[test]
    fn replay_skips_already_installed_registry_bump_even_past_snapshot_lsn() {
        let wal = tempfile::tempdir().unwrap();
        let registry = Arc::new(Registry::new());
        let dev_agg = DevAggState::new(registry);
        let payload = txn_register_payload();
        let bump = crate::register::RegistryBumpPayload {
            new_version: 1,
            payload_nodes: payload.nodes,
            force_removed_descriptors: Vec::new(),
        };

        crate::register::apply_registry_bump(&dev_agg.registry, bump.clone())
            .expect("install bump before replay");
        assert_eq!(dev_agg.registry.version(), 1);

        let mut writer =
            beava_persistence::WalWriter::open(wal.path(), 100, dev_agg.registry.version() as u32)
                .expect("open wal writer");
        writer
            .append(&WalRecord {
                lsn: 123,
                record_type: RecordType::RegistryBump,
                payload: bump.encode().expect("encode bump"),
            })
            .expect("append bump");
        writer.sync_data().expect("sync wal");
        drop(writer);

        let outcome = replay_wal_from_lsn(wal.path(), 42, &dev_agg).expect("replay wal");
        assert_eq!(outcome.last_lsn, 123);
        assert_eq!(outcome.replay_registry_bumps, 0);
        assert!(!outcome.applied_registry_bump_after_snapshot);
        assert_eq!(dev_agg.registry.version(), 1);
    }
}
