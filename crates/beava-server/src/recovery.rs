//! Recovery on startup.
//!
//! 1. `load_snapshot_if_any(dir, dev_agg)` — descending-LSN scan; first valid
//!    snapshot wins; install registry descriptors + state tables; return its
//!    LSN. Empty dir or all-corrupt files → 0 (cold start).
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
use std::path::Path;
use std::sync::atomic::Ordering;

/// Outcome counters reported back from `replay_wal_from_lsn`.
#[derive(Debug, Default)]
pub struct RecoveryOutcome {
    pub installed_from_snapshot: bool,
    pub snapshot_lsn: Lsn,
    pub replay_event_count: u64,
    pub replay_registry_bumps: u64,
    pub last_lsn: Lsn,
}

/// Scan `snapshot_dir` for the highest-LSN valid snapshot; install its
/// registry descriptors + state tables into `app_state`. Returns the
/// snapshot's LSN, or 0 if no valid snapshot exists (cold start).
pub fn load_snapshot_if_any(
    snapshot_dir: &Path,
    dev_agg: &DevAggState,
) -> Result<Lsn, PersistError> {
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
                            let agg_id = match dev_agg.registry.compiled_aggregation(&node_name) {
                                Some(d) => d.agg_id as usize,
                                None => continue,
                            };
                            let tbl = &mut tables[agg_id];
                            for (key, ops) in entries {
                                tbl.insert_from_entity_key(key, ops);
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
                        registry_version = header.registry_version,
                        "loaded snapshot"
                    );
                    return Ok(lsn);
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
    Ok(0)
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

/// A single decoded v=2 record from the hand-rolled WAL file.
struct V2Record {
    body_format: u8,
    // reason: parsed from the v=2 record header for completeness; recovery
    // doesn't depend on the per-record registry version.
    #[allow(dead_code)]
    rv: u32,
    et_ms: i64,
    event_name: String,
    body: Vec<u8>,
}

/// Parse all v=2 binary records from a contiguous byte slice.
///
/// Format: `[u8 v=2][u8 body_format][u32 rv BE][u64 et_ms BE]
///           [u16 name_len BE][N bytes name][u32 body_len BE][M bytes body]`
///
/// Stops at first byte that is not 0x02 (unknown version) or if bytes are
/// insufficient (truncated record — treat as EOF).
fn parse_v2_records(data: &[u8]) -> Vec<V2Record> {
    let mut records = Vec::new();
    let mut pos = 0usize;

    loop {
        // Fixed header is 1+1+4+8+2 = 16 bytes.
        if pos + 16 > data.len() {
            break;
        }

        let version = data[pos];
        if version != 0x02 {
            break;
        }
        pos += 1;

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

        records.push(V2Record {
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
/// v=2 binary record format (see `dispatch_push_sync` in `apply_shard`),
/// distinct from the `beava-persistence` `WalSink` format (`*.log`). Returns
/// recovery counters; assigns monotonic LSNs starting from `lsn_start`.
pub fn replay_handrolled_wal_dir(
    wal_dir: &Path,
    lsn_start: Lsn,
    dev_agg: &DevAggState,
) -> Result<RecoveryOutcome, std::io::Error> {
    use beava_core::wire::CT_MSGPACK;
    let mut outcome = RecoveryOutcome {
        snapshot_lsn: lsn_start.saturating_sub(1),
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

    let mut next_lsn = lsn_start;

    for wal_file in &wal_files {
        let data = std::fs::read(wal_file)?;
        let records = parse_v2_records(&data);

        for rec in records {
            let lsn = next_lsn;
            next_lsn += 1;

            let row: Row = if rec.body_format == CT_MSGPACK {
                match rmp_serde::from_slice::<Row>(&rec.body) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(
                            target: "beava.recovery",
                            kind = "recovery.v2_msgpack_decode_failed",
                            lsn = lsn,
                            error = %e,
                            "v=2 msgpack body decode failed; skipping"
                        );
                        continue;
                    }
                }
            } else {
                match serde_json::from_slice::<Row>(&rec.body) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(
                            target: "beava.recovery",
                            kind = "recovery.v2_json_decode_failed",
                            lsn = lsn,
                            error = %e,
                            "v=2 JSON body decode failed; skipping"
                        );
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
                    lsn,
                    rec.rv as u64,
                    &dev_agg.registry,
                    &mut tables,
                    cold_after_ms,
                );
            }

            dev_agg.next_event_id.fetch_max(lsn, Ordering::Relaxed);
            if rec.et_ms > 0 {
                dev_agg
                    .query_time_ms
                    .fetch_max(rec.et_ms as u64, Ordering::Relaxed);
            }
            outcome.replay_event_count += 1;
            outcome.last_lsn = lsn;
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
        if rec.lsn <= from_lsn_exclusive {
            continue;
        }
        outcome.last_lsn = outcome.last_lsn.max(rec.lsn);
        match rec.record_type {
            RecordType::Event => {
                let payload: WalEventPayload = match serde_json::from_slice(&rec.payload) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(
                            target: "beava.recovery",
                            kind = "recovery.event_decode_failed",
                            lsn = rec.lsn,
                            error = %e,
                            "event payload decode failed; skipping"
                        );
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
                Ok(bump) => match crate::register::apply_registry_bump(&dev_agg.registry, bump) {
                    Ok(()) => {
                        outcome.replay_registry_bumps += 1;
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
                },
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
