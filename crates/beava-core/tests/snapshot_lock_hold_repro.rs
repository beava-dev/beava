//! Local reproduction of the kalshi-pulse incident (2026-05-21):
//! measure how long `state_tables.lock()` is held during snapshot encoding.
//!
//! The hot operation under the `parking_lot::Mutex` in
//! `crates/beava-server/src/snapshot_task.rs::do_snapshot` is exactly:
//!
//!     for (node_name, desc) in registry.compiled_aggregations.iter() {
//!         if let Some(table) = state_tables.get(agg_id) {
//!             let entries: Vec<(EntityKey, Vec<AggOp>)> = table
//!                 .iter_sorted()
//!                 .map(|(k, v)| (k.clone(), v.clone()))   // ← clone every entry
//!                 .collect();
//!             serialized_tables.insert(node_name.clone(), entries);
//!         }
//!     }
//!
//! We bypass the Registry plumbing and time the inner clone-collect directly
//! on a populated `AggStateTable`. This is the SAME byte-for-byte operation
//! that parks the apply thread in production, just without the registry
//! enumeration overhead (which is negligible — BTreeMap of ~14 aggs).
//!
//! Run with:
//!   cargo test -p beava-core --release --test snapshot_lock_hold_repro -- --nocapture
//!
//! Use `--release` — debug-mode `AggOp::clone` is ~10× slower than release
//! and would mislead the projection.

use beava_core::agg_op::AggOp;
use beava_core::agg_state::CountState;
use beava_core::agg_state_table::{AggStateTable, EntityKey};
use beava_core::row::Value;
use compact_str::CompactString;
use smallvec::smallvec;
use std::time::Instant;

/// What op shape to put in each entry. Count is the smallest variant;
/// CountDistinct is a representative sketch (HLL with 1024 registers per
/// entity).
#[derive(Copy, Clone)]
enum OpShape {
    Count,
    CountDistinctHll1024,
}

impl OpShape {
    fn build(self, n: u64) -> AggOp {
        match self {
            OpShape::Count => AggOp::Count(CountState { n }),
            OpShape::CountDistinctHll1024 => AggOp::CountDistinct(Box::default()),
        }
    }
    fn label(self) -> &'static str {
        match self {
            OpShape::Count => "Count",
            OpShape::CountDistinctHll1024 => "CountDistinct(HLL-1024)",
        }
    }
}

/// Populate one `AggStateTable` with N entities; every entry has a single
/// op of the requested shape.
fn build_table(n_entities: usize, shape: OpShape) -> AggStateTable {
    let mut table = AggStateTable::new();
    for ent in 0..n_entities {
        let key_str = format!("user_{ent:09}");
        let entity_key = EntityKey(smallvec![(
            CompactString::from("user_id"),
            Value::Str(CompactString::from(key_str.as_str())),
        )]);
        table.insert_from_entity_key(entity_key, vec![shape.build(ent as u64)]);
    }
    table
}

/// The exact clone-collect that runs under `state_tables.lock()` in
/// `snapshot_task.rs::do_snapshot`. Returns (lock_held_ms, entries_collected,
/// approx_bytes_cloned).
fn measure_lock_hold(table: &AggStateTable) -> (f64, usize, usize) {
    let t0 = Instant::now();
    let entries: Vec<(EntityKey, Vec<AggOp>)> = table
        .iter_sorted()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // Rough byte estimate: AggOp ≤ 80 B (CI tripwire). EntityKey is a SmallVec
    // of (CompactString, Value::Str(CompactString)). Inline string fit varies;
    // assume ~32 B per EntityKey on average for "user_NNNNNNNNN" keys.
    let n = entries.len();
    let approx_bytes = n * (80 + 32 + 24); // AggOp + EntityKey + Vec<AggOp> overhead
    (elapsed_ms, n, approx_bytes)
}

fn run_shape(label: &str, shape: OpShape, sizes: &[usize]) -> Vec<(usize, f64)> {
    println!();
    println!("=== {label} ===");
    println!(
        "{:>12} {:>14} {:>18}",
        "entities", "lock_held_ms", "ns_per_entry"
    );
    println!("{}", "-".repeat(48));

    let mut out = Vec::new();
    for &n in sizes {
        let table = build_table(n, shape);
        let _ = measure_lock_hold(&table); // warm-up
        let mut samples: Vec<f64> = (0..3).map(|_| measure_lock_hold(&table).0).collect();
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median_ms = samples[1];
        let ns_per_entry = median_ms * 1_000_000.0 / (n as f64);
        out.push((n, ns_per_entry));
        println!("{:>12} {:>11.1}ms {:>15.0}ns", n, median_ms, ns_per_entry);
    }
    out
}

#[test]
#[ignore = "diagnostic repro: allocates millions of entries; run manually in release with --ignored --nocapture"]
fn measure_snapshot_lock_hold_time() {
    println!();
    println!("=== state_tables.lock() hold-time measurement ===");
    println!("Operation timed (the exact body of the parking_lot lock-hold in");
    println!("crates/beava-server/src/snapshot_task.rs::do_snapshot):");
    println!("  table.iter_sorted().map(|(k,v)| (k.clone(), v.clone())).collect()");

    let small = &[
        10_000usize,
        100_000,
        500_000,
        1_000_000,
        2_000_000,
        4_000_000,
    ];
    let small_sketch = &[10_000usize, 100_000, 500_000];

    let count_samples = run_shape(OpShape::Count.label(), OpShape::Count, small);
    let hll_samples = run_shape(
        OpShape::CountDistinctHll1024.label(),
        OpShape::CountDistinctHll1024,
        small_sketch,
    );

    // Average ns/entry at large-N (skip the noisy small ones).
    let count_ns: f64 = {
        let tail: Vec<f64> = count_samples
            .iter()
            .rev()
            .take(2)
            .map(|(_, n)| *n)
            .collect();
        tail.iter().sum::<f64>() / tail.len() as f64
    };
    let hll_ns: f64 = {
        let tail: Vec<f64> = hll_samples.iter().rev().take(2).map(|(_, n)| *n).collect();
        tail.iter().sum::<f64>() / tail.len() as f64
    };

    println!();
    println!("=== Projection to production scale ===");
    println!("Per-entry clone cost (large-N median):");
    println!("  Count                   : {count_ns:>8.0} ns");
    println!(
        "  CountDistinct(HLL-1024) : {hll_ns:>8.0} ns ({:.1}× Count)",
        hll_ns / count_ns
    );
    println!();
    println!("For the 507 MB encoded production snapshot:");
    println!("(actual entry count depends on op-mix; we project two cases)");
    for entries in &[1_000_000usize, 5_000_000usize, 10_000_000usize] {
        let count_s = count_ns * (*entries as f64) / 1_000_000_000.0;
        let hll_s = hll_ns * (*entries as f64) / 1_000_000_000.0;
        println!("  {entries:>10} entries → Count: {count_s:>5.2}s   HLL: {hll_s:>6.2}s lock-hold",);
    }

    println!();
    println!("=== Interpretation ===");
    println!("/ping is FIFO-queued behind any push waiting on state_tables.lock().");
    println!("Once the lock is held by the snapshot task, no push can dispatch,");
    println!("and /ping (handled on the same single-threaded apply loop) cannot");
    println!("progress until the lock is released.");
    println!();
    println!("Even with all-Count workloads, multi-second lock-holds at 5-10M");
    println!("entities are enough to blow past a 3s docker healthcheck timeout.");
    println!("Real production workloads with sketches push this to tens of");
    println!("seconds, which matches the incident's 60-90s observation.");
    println!();
    println!("Additional time accrues OUTSIDE the lock (encode + WAL truncate +");
    println!("fsync of 507 MB) — those don't park the apply thread directly,");
    println!("but they extend the snapshot task's wall-clock cycle, and once");
    println!("CPU/IO bandwidth is saturated, the next snapshot tick fires before");
    println!("the previous one has fully drained, compounding the problem.");
}
