//! Snapshot recovery time tests.
//!
//! Measure how long it takes to recover a snapshot end-to-end:
//! `SnapshotReader::open` (verify magic + CRC + read body bytes) plus
//! `SnapshotBody::decode` (bincode deserialize). Both are on the boot
//! critical path — a beava process that just restarted cannot serve
//! traffic until recovery completes.
//!
//! The user-visible question this test answers: **if production state is
//! 507 MB encoded (~5-10M entries), how long does a restart's snapshot
//! recovery take?**
//!
//! We also verify round-trip correctness: bytes written by
//! `SnapshotWriter::write` decode byte-identically via `SnapshotReader::open`
//! + `SnapshotBody::decode`.

use beava_core::agg_op::AggOp;
use beava_core::agg_state::CountState;
use beava_core::agg_state_table::EntityKey;
use beava_core::row::Value;
use beava_core::snapshot_body::{
    RegistryDescriptorsOnly, SerializedStateTables, SnapshotBody, SNAPSHOT_BODY_FORMAT_VERSION,
};
use beava_persistence::{SnapshotReader, SnapshotWriter};
use compact_str::CompactString;
use smallvec::smallvec;
use std::collections::BTreeMap;
use std::time::Instant;
use tempfile::TempDir;

/// Build a `SnapshotBody` populated with one aggregation node ("agg_0")
/// containing N entities × Count entries.
fn build_body(n_entities: usize) -> SnapshotBody {
    let mut entries: Vec<(EntityKey, Vec<AggOp>)> = Vec::with_capacity(n_entities);
    for ent in 0..n_entities {
        let key_str = format!("user_{ent:09}");
        let entity_key = EntityKey(smallvec![(
            CompactString::from("user_id"),
            Value::Str(CompactString::from(key_str.as_str())),
        )]);
        entries.push((entity_key, vec![AggOp::Count(CountState { n: ent as u64 })]));
    }
    let mut state_tables: SerializedStateTables = BTreeMap::new();
    state_tables.insert("agg_0".to_string(), entries);

    SnapshotBody {
        body_format_version: SNAPSHOT_BODY_FORMAT_VERSION,
        registry: RegistryDescriptorsOnly::default(),
        state_tables,
        next_event_id: 42,
        query_time_ms: 1_700_000_000_000,
    }
}

/// Encode + write a snapshot to disk; return the final file path.
fn write_snapshot(dir: &std::path::Path, lsn: u64, body: &SnapshotBody) -> std::path::PathBuf {
    let encoded = body.encode().expect("encode");
    SnapshotWriter::write(dir, lsn, body.registry.version, &encoded).expect("write")
}

#[test]
fn snapshot_round_trip_byte_identical() {
    // Smallest possible round-trip — verifies the contract.
    let tmp = TempDir::new().unwrap();
    let body = build_body(100);

    let encoded_in = body.encode().expect("encode");
    let path = write_snapshot(tmp.path(), 1, &body);

    let (header, encoded_out) = SnapshotReader::open(&path).expect("open");
    assert_eq!(header.snapshot_lsn, 1);
    assert_eq!(encoded_in, encoded_out, "encoded bytes must round-trip");

    let decoded = SnapshotBody::decode(&encoded_out).expect("decode");
    assert_eq!(decoded.next_event_id, body.next_event_id);
    assert_eq!(decoded.query_time_ms, body.query_time_ms);
    assert_eq!(decoded.state_tables.len(), 1);
    assert_eq!(decoded.state_tables["agg_0"].len(), 100);
}

#[test]
fn snapshot_recovery_time_scaling() {
    let sizes: &[usize] = &[1_000, 10_000, 100_000];

    println!();
    println!("=== Snapshot recovery time vs state size ===");
    println!("(SnapshotReader::open + SnapshotBody::decode)");
    println!();
    println!(
        "{:>10} {:>14} {:>14} {:>16} {:>18}",
        "entries", "encoded_KB", "open_ms", "decode_ms", "MB/s_decode"
    );
    println!("{}", "-".repeat(80));

    let tmp = TempDir::new().unwrap();
    for &n in sizes {
        let body = build_body(n);
        let path = write_snapshot(tmp.path(), n as u64, &body);
        let file_size_kb = std::fs::metadata(&path).unwrap().len() as f64 / 1024.0;

        // Median of 3 to smooth filesystem cache effects.
        let mut open_samples = Vec::with_capacity(3);
        let mut decode_samples = Vec::with_capacity(3);
        let mut last_body_len = 0usize;
        for _ in 0..3 {
            let t0 = Instant::now();
            let (_h, encoded) = SnapshotReader::open(&path).expect("open");
            let open_elapsed = t0.elapsed();

            let t1 = Instant::now();
            let decoded = SnapshotBody::decode(&encoded).expect("decode");
            let decode_elapsed = t1.elapsed();

            open_samples.push(open_elapsed.as_secs_f64() * 1000.0);
            decode_samples.push(decode_elapsed.as_secs_f64() * 1000.0);
            last_body_len = encoded.len();

            // Touch the decoded value so the optimizer doesn't elide it.
            assert_eq!(decoded.state_tables["agg_0"].len(), n);
        }
        open_samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        decode_samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let open_ms = open_samples[1];
        let decode_ms = decode_samples[1];
        let mb_per_s = (last_body_len as f64 / (1024.0 * 1024.0)) / (decode_ms / 1000.0);

        println!(
            "{:>10} {:>11.1} KB {:>11.2}ms {:>13.2}ms {:>15.1}",
            n, file_size_kb, open_ms, decode_ms, mb_per_s
        );
    }

    println!();
    println!("Projection to incident scale (507 MB encoded snapshot):");
    println!("- decode throughput at large N is ~constant (MB/s above)");
    println!("- recovery wall-clock = open + decode + install (install adds");
    println!("  per-entity HashMap insert cost; not measured here)");
    println!();
    println!("These numbers are the FLOOR — production state has fatter ops");
    println!("than Count (sketches, windowed). Real recovery throughput in");
    println!("MB/s is similar but per-entry latency is higher.");
}

#[test]
fn snapshot_decode_deterministic() {
    // Two encodes of the same body must produce byte-identical output;
    // recovery must produce byte-identical state. Locks the "no
    // non-determinism in the snapshot format" contract.
    let tmp = TempDir::new().unwrap();
    let body = build_body(500);

    let path1 = write_snapshot(tmp.path(), 1, &body);
    let path2 = write_snapshot(tmp.path(), 2, &body);

    let (h1, b1) = SnapshotReader::open(&path1).unwrap();
    let (h2, b2) = SnapshotReader::open(&path2).unwrap();

    // Bodies must be byte-identical.
    assert_eq!(
        b1, b2,
        "two encodes of same body must produce identical bytes"
    );
    // Headers differ on snapshot_lsn + created_at_ms; just verify body lens match.
    assert_eq!(h1.body_len, h2.body_len);

    let d1 = SnapshotBody::decode(&b1).unwrap();
    let d2 = SnapshotBody::decode(&b2).unwrap();
    assert_eq!(d1.next_event_id, d2.next_event_id);
    assert_eq!(
        d1.state_tables["agg_0"].len(),
        d2.state_tables["agg_0"].len()
    );
}

/// Verify that the fork-path snapshot file is decodable AND its body bytes
/// are byte-identical to an in-process encoding of the same input state.
///
/// We use an empty registry (so both paths emit zero serialized tables);
/// this still locks the contract that the fork path doesn't change the
/// header/format/body schema.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn fork_and_in_process_produce_identical_format() {
    use beava_core::registry::Registry;
    use beava_server::registry_debug::DevAggState;
    use beava_server::snapshot_fork::{do_snapshot_via_fork, ChildExit};
    use beava_server::AppState;
    use std::sync::Arc;

    let registry = Arc::new(Registry::new());
    let dev_agg = DevAggState::new(registry);
    let (wal_sink, _wal_join) = beava_persistence::WalSink::spawn_no_op();
    let idem_cache = Arc::new(beava_server::idem_cache::IdemCache::new());
    let app_state = AppState::new(dev_agg, wal_sink, idem_cache);

    // Write via the fork path.
    let tmp_fork = TempDir::new().unwrap();
    let exit = do_snapshot_via_fork(tmp_fork.path(), 99, &app_state)
        .await
        .expect("fork-snapshot");
    assert!(matches!(exit, ChildExit::Success { .. }));
    let fork_path = tmp_fork.path().join(format!("snapshot-{:016x}.bvs", 99u64));

    // Build the SAME SnapshotBody in-process and write via SnapshotWriter.
    let registry_snap = app_state.dev_agg.registry.snapshot();
    let tables = app_state.dev_agg.state_tables.lock();
    let body_inproc = SnapshotBody::from_live(&registry_snap, &tables, 0, 0);
    drop(tables);
    let encoded_inproc = body_inproc.encode().expect("encode");

    let tmp_inproc = TempDir::new().unwrap();
    let inproc_path = SnapshotWriter::write(tmp_inproc.path(), 99, 0, &encoded_inproc).unwrap();

    // Read both back and compare body bytes (header differs on
    // created_at_ms, that's expected).
    let (_h_fork, body_fork) = SnapshotReader::open(&fork_path).unwrap();
    let (_h_inproc, body_inproc_read) = SnapshotReader::open(&inproc_path).unwrap();
    assert_eq!(
        body_fork, body_inproc_read,
        "fork and in-process must produce identical body bytes"
    );
}
