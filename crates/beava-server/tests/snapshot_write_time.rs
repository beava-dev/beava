//! Direct measurement of snapshot write-time components.
//!
//! Breaks down the wall-clock cost of `do_snapshot` into:
//! 1. encode    — `SnapshotBody::encode()` (bincode serialize, CPU-bound)
//! 2. write+fsync — `SnapshotWriter::write` (file IO + sync_all + dir fsync)
//! 3. total     — encode + write
//!
//! The legacy path adds (1)+(2) on the snapshot task thread plus the
//! clone-collect under state_tables.lock() (measured separately in
//! `snapshot_lock_contention.rs`). The fork path keeps (1)+(2) in the
//! child process — they don't block the apply thread.

use beava_core::agg_op::AggOp;
use beava_core::agg_state::CountState;
use beava_core::agg_state_table::EntityKey;
use beava_core::row::Value;
use beava_core::snapshot_body::{
    RegistryDescriptorsOnly, SerializedStateTables, SnapshotBody, SNAPSHOT_BODY_FORMAT_VERSION,
};
use beava_persistence::SnapshotWriter;
use compact_str::CompactString;
use smallvec::smallvec;
use std::collections::BTreeMap;
use std::time::Instant;
use tempfile::TempDir;

fn build_body(n: usize) -> SnapshotBody {
    let mut entries: Vec<(EntityKey, Vec<AggOp>)> = Vec::with_capacity(n);
    for ent in 0..n {
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
        next_event_id: 0,
        query_time_ms: 0,
    }
}

#[test]
fn snapshot_write_time_scaling() {
    let sizes = [10_000usize, 100_000, 500_000];
    let tmp = TempDir::new().unwrap();

    println!();
    println!("=== Snapshot WRITE-time breakdown ===");
    println!(
        "{:>10} {:>10} {:>10} {:>12} {:>12} {:>10}",
        "entries", "bytes_MB", "encode_ms", "write+fsync", "total_ms", "MB/s"
    );
    println!("{}", "-".repeat(70));

    for (i, &n) in sizes.iter().enumerate() {
        let body = build_body(n);
        // Median of 3 to smooth filesystem cache effects.
        let mut samples: Vec<(f64, f64, usize)> = Vec::with_capacity(3);
        for trial in 0..3 {
            let t0 = Instant::now();
            let encoded = body.encode().expect("encode");
            let encode_ms = t0.elapsed().as_secs_f64() * 1000.0;

            let t1 = Instant::now();
            SnapshotWriter::write(
                tmp.path(),
                (i * 100 + trial) as u64,
                body.registry.version,
                &encoded,
            )
            .expect("write");
            let write_ms = t1.elapsed().as_secs_f64() * 1000.0;

            samples.push((encode_ms, write_ms, encoded.len()));
        }
        samples.sort_by(|a, b| (a.0 + a.1).partial_cmp(&(b.0 + b.1)).unwrap());
        let (encode_ms, write_ms, bytes) = samples[1];
        let total = encode_ms + write_ms;
        let mb = bytes as f64 / (1024.0 * 1024.0);
        let mbps = mb / (total / 1000.0);

        println!(
            "{:>10} {:>7.1} MB {:>7.2}ms {:>9.2}ms {:>9.2}ms {:>8.1}",
            n, mb, encode_ms, write_ms, total, mbps
        );
    }

    println!();
    println!("Notes:");
    println!("- encode is CPU-bound (bincode serialize); release ~1 GB/s, debug ~3-5×");
    println!(
        "- write+fsync is disk-bound: header write + body write + sync_all + rename + dir fsync"
    );
    println!("- on SSD ~500 MB/s sequential write; on slow containerized volumes much less");
    println!("- LEGACY path: also pays state_tables.lock() clone-collect upstream of these");
    println!("- FORK path: encode + write happen in child; parent's apply thread untouched");
}
