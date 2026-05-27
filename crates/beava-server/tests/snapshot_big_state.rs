//! Big-state snapshot tests — 500k → 5M entries, matching the production
//! incident scale (#151 reported a 507 MB encoded snapshot, ~5-10M entries).
//!
//! All tests are `#[ignore]`'d by default — building 5M entries × `AggOp`
//! takes ~10s and ~1 GB RAM, too heavy for default `cargo test`.
//!
//! ## How to run
//!
//! ```sh
//! # Recommended (release mode is ~10× faster — debug-mode AggOp::clone is
//! # bottlenecked by debug-assertions and panic stubs, masking real perf):
//! cargo test --release -p beava-server --test snapshot_big_state -- --ignored --nocapture
//!
//! # Single test:
//! cargo test --release -p beava-server --test snapshot_big_state \
//!     lock_hold_at_1m_entries -- --ignored --nocapture
//! ```
//!
//! ## Why ignored by default
//!
//! - **Runtime:** ~30-60s end-to-end in release; ~5-10 min in debug.
//! - **Memory:** ~1 GB peak at 5M entries (entity-key strings + AggOp boxes).
//!
//! Default `cargo test` covers the same code paths at 100-200k entries
//! (`snapshot_lock_contention.rs`, `snapshot_recovery_time.rs`) — those
//! prove the contract; these tests confirm the contract still holds at
//! the production scale that triggered the incident.

use beava_core::agg_op::AggOp;
use beava_core::agg_state::CountState;
use beava_core::agg_state_table::{AggStateTable, EntityKey};
use beava_core::registry::Registry;
use beava_core::row::Value;
use beava_core::snapshot_body::{
    RegistryDescriptorsOnly, SerializedStateTables, SnapshotBody, SNAPSHOT_BODY_FORMAT_VERSION,
};
use beava_persistence::{SnapshotReader, SnapshotWriter};
use beava_server::registry_debug::DevAggState;
use beava_server::snapshot_fork::do_snapshot_via_fork;
use beava_server::AppState;
use compact_str::CompactString;
use smallvec::smallvec;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn build_app_state(n_entities: usize) -> AppState {
    let registry = Arc::new(Registry::new());
    let dev_agg = DevAggState::new(registry);
    {
        let mut tables = dev_agg.state_tables.lock();
        let mut table = AggStateTable::new();
        for ent in 0..n_entities {
            let key_str = format!("user_{ent:09}");
            let entity_key = EntityKey(smallvec![(
                CompactString::from("user_id"),
                Value::Str(CompactString::from(key_str.as_str())),
            )]);
            table.insert_from_entity_key(
                entity_key,
                vec![AggOp::Count(CountState { n: ent as u64 })],
            );
        }
        tables.push(table);
    }
    let (wal_sink, _wal_join) = beava_persistence::WalSink::spawn_no_op();
    let idem_cache = Arc::new(beava_server::idem_cache::IdemCache::new());
    AppState::new(dev_agg, wal_sink, idem_cache)
}

fn measure_legacy_lock_hold(app_state: &AppState) -> Duration {
    let lock_start = Instant::now();
    let _entries: Vec<(EntityKey, Vec<AggOp>)> = {
        let tables = app_state.dev_agg.state_tables.lock();
        tables[0]
            .iter_sorted()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    lock_start.elapsed()
}

#[cfg(unix)]
fn measure_fork_parent_lock_hold(app_state: &AppState) -> Duration {
    let lock_start = Instant::now();
    let state_lock = app_state.dev_agg.state_tables.lock();
    let pid = unsafe { libc::fork() };
    let lock_held = lock_start.elapsed();
    if pid == 0 {
        unsafe { libc::_exit(0) };
    }
    assert!(pid > 0, "fork failed: {}", std::io::Error::last_os_error());
    drop(state_lock);
    let mut status: libc::c_int = 0;
    unsafe {
        libc::waitpid(pid, &mut status, 0);
    }
    lock_held
}

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
        next_event_id: 0,
        query_time_ms: 0,
    }
}

// ─── Legacy lock-hold at big N ──────────────────────────────────────────────

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "big-state: builds ~1GB of state; run with --ignored --release"]
async fn legacy_lock_hold_at_1m_entries() {
    let app_state = build_app_state(1_000_000);
    let _ = measure_legacy_lock_hold(&app_state); // warm-up
    let mut samples: Vec<f64> = (0..3)
        .map(|_| measure_legacy_lock_hold(&app_state).as_secs_f64() * 1000.0)
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lock_ms = samples[1];
    println!();
    println!("legacy lock-hold @ N=1M:  {lock_ms:.1}ms");
    // 1M Count entries should hold the lock for ≥500ms in release, ≥1s in
    // debug. Loose floor (100ms) — the point is to prove the lock IS held
    // for a long time at production scale.
    assert!(
        lock_ms >= 100.0,
        "legacy lock-hold at 1M entries should be ≥100ms — got {lock_ms:.1}ms"
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "big-state: builds ~2GB of state; run with --ignored --release"]
async fn legacy_lock_hold_at_5m_entries() {
    // 5M entries ≈ the incident's apparent scale (~507 MB encoded /
    // ~100 B per entry encoded → ~5M entries).
    let app_state = build_app_state(5_000_000);
    let _ = measure_legacy_lock_hold(&app_state);
    let mut samples: Vec<f64> = (0..3)
        .map(|_| measure_legacy_lock_hold(&app_state).as_secs_f64() * 1000.0)
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lock_ms = samples[1];
    println!();
    println!(
        "legacy lock-hold @ N=5M:  {lock_ms:.1}ms  ({:.1}s)",
        lock_ms / 1000.0
    );
    // 5M Count entries — legacy should be in the seconds. This number is
    // the smoking gun for the incident: the lock is held for longer than
    // a 3s docker healthcheck timeout under any non-trivial state.
    assert!(
        lock_ms >= 500.0,
        "legacy lock-hold at 5M entries should be ≥500ms — got {lock_ms:.1}ms"
    );
}

// ─── Fork lock-hold at big N (must stay sub-millisecond) ────────────────────

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "big-state: builds ~1GB of state; run with --ignored --release"]
async fn fork_lock_hold_at_1m_entries() {
    let app_state = build_app_state(1_000_000);
    let _ = measure_fork_parent_lock_hold(&app_state); // warm-up
    let mut samples: Vec<f64> = (0..3)
        .map(|_| measure_fork_parent_lock_hold(&app_state).as_secs_f64() * 1000.0)
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lock_ms = samples[1];
    println!();
    println!("fork lock-hold @ N=1M:    {lock_ms:.2}ms");
    // Fork is O(1) — state size doesn't matter. Generous 100ms ceiling
    // for CI variance; empirically sub-millisecond.
    assert!(
        lock_ms < 100.0,
        "fork lock-hold at 1M entries must be O(1) — got {lock_ms:.2}ms"
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "big-state: builds ~2GB of state; run with --ignored --release"]
async fn fork_lock_hold_at_5m_entries() {
    let app_state = build_app_state(5_000_000);
    let _ = measure_fork_parent_lock_hold(&app_state);
    let mut samples: Vec<f64> = (0..3)
        .map(|_| measure_fork_parent_lock_hold(&app_state).as_secs_f64() * 1000.0)
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lock_ms = samples[1];
    println!();
    println!("fork lock-hold @ N=5M:    {lock_ms:.2}ms");
    // The key invariant: even at 5M entries, fork releases the lock in
    // milliseconds. (libc::fork's page-table copy on Linux scales weakly
    // with VM size, but should still be well under 100ms at this RAM size
    // on modern hardware.)
    assert!(
        lock_ms < 100.0,
        "fork lock-hold at 5M entries must be O(1) — got {lock_ms:.2}ms"
    );
}

// ─── Side-by-side speedup at production scale ───────────────────────────────

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "big-state: builds ~1GB of state; run with --ignored --release"]
async fn fork_speedup_at_1m_entries() {
    let app_state = build_app_state(1_000_000);
    let _ = measure_legacy_lock_hold(&app_state);
    let _ = measure_fork_parent_lock_hold(&app_state);

    let mut legacy: Vec<f64> = (0..3)
        .map(|_| measure_legacy_lock_hold(&app_state).as_secs_f64() * 1000.0)
        .collect();
    let mut fork: Vec<f64> = (0..3)
        .map(|_| measure_fork_parent_lock_hold(&app_state).as_secs_f64() * 1000.0)
        .collect();
    legacy.sort_by(|a, b| a.partial_cmp(b).unwrap());
    fork.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let legacy_ms = legacy[1];
    let fork_ms = fork[1];
    let speedup = legacy_ms / fork_ms.max(0.001);

    println!();
    println!("=== Lock-hold speedup at N=1M entities (production-scale) ===");
    println!("  legacy: {legacy_ms:>8.1}ms");
    println!("  fork:   {fork_ms:>8.2}ms");
    println!("  speedup: {speedup:>6.0}×");
    println!();

    // Speedup at 1M is empirically 50-1000× depending on OS memory state
    // (fork's page-table copy scales weakly with VM size after the runner
    // has already allocated/freed large state in earlier tests). 20× is
    // the CI floor — anything higher means the bug is fixed; the exact
    // multiplier doesn't matter beyond that.
    assert!(
        speedup >= 20.0,
        "speedup at 1M entries must be ≥20× — got {speedup:.0}× (legacy={legacy_ms}ms fork={fork_ms}ms)"
    );
}

// ─── Recovery decode at big N ────────────────────────────────────────────────

#[test]
#[ignore = "big-state: ~100 MB encoded snapshot; run with --ignored --release"]
fn recovery_decode_at_1m_entries() {
    let tmp = TempDir::new().unwrap();
    let body = build_body(1_000_000);
    let encoded = body.encode().expect("encode");
    let body_mb = encoded.len() as f64 / (1024.0 * 1024.0);
    SnapshotWriter::write(tmp.path(), 1, 0, &encoded).unwrap();
    let path = tmp.path().join(format!("snapshot-{:016x}.bvs", 1u64));

    // Median of 3 to smooth fs cache effects.
    let mut samples: Vec<(f64, f64)> = Vec::with_capacity(3);
    for _ in 0..3 {
        let t0 = Instant::now();
        let (_h, bytes) = SnapshotReader::open(&path).unwrap();
        let open_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = Instant::now();
        let _decoded = SnapshotBody::decode(&bytes).unwrap();
        let decode_ms = t1.elapsed().as_secs_f64() * 1000.0;
        samples.push((open_ms, decode_ms));
    }
    samples.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let (open_ms, decode_ms) = samples[1];
    let mb_per_s = body_mb / (decode_ms / 1000.0);

    println!();
    println!("=== Recovery decode @ N=1M entries ===");
    println!("  encoded:    {body_mb:>7.1} MB");
    println!("  open:       {open_ms:>7.2} ms");
    println!("  decode:     {decode_ms:>7.2} ms");
    println!("  throughput: {mb_per_s:>7.1} MB/s");
}

#[test]
#[ignore = "big-state: ~500 MB encoded snapshot — matches incident; run with --ignored --release"]
fn recovery_decode_at_5m_entries() {
    // Approx the incident's 507 MB snapshot size.
    let tmp = TempDir::new().unwrap();
    let body = build_body(5_000_000);
    let encoded = body.encode().expect("encode");
    let body_mb = encoded.len() as f64 / (1024.0 * 1024.0);
    SnapshotWriter::write(tmp.path(), 1, 0, &encoded).unwrap();
    let path = tmp.path().join(format!("snapshot-{:016x}.bvs", 1u64));

    // Single pass — encoding 5M entries once is expensive enough.
    let t0 = Instant::now();
    let (_h, bytes) = SnapshotReader::open(&path).unwrap();
    let open_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let t1 = Instant::now();
    let _decoded = SnapshotBody::decode(&bytes).unwrap();
    let decode_ms = t1.elapsed().as_secs_f64() * 1000.0;
    let mb_per_s = body_mb / (decode_ms / 1000.0);

    println!();
    println!("=== Recovery decode @ N=5M entries (incident scale) ===");
    println!("  encoded:    {body_mb:>7.1} MB");
    println!("  open:       {open_ms:>7.2} ms");
    println!(
        "  decode:     {decode_ms:>7.2} ms  ({:.2}s)",
        decode_ms / 1000.0
    );
    println!("  throughput: {mb_per_s:>7.1} MB/s");
    println!();
    println!("This is roughly the wall-clock cost of boot-time recovery on");
    println!("the production deployment. Apply (install_from_descriptors +");
    println!("per-table HashMap rebuild) adds further overhead not measured");
    println!("here.");
}

// ─── End-to-end fork snapshot at big N ──────────────────────────────────────

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "big-state: full fork snapshot at production scale; run with --ignored --release"]
async fn fork_full_snapshot_at_1m_entries() {
    let tmp = TempDir::new().unwrap();
    let app_state = build_app_state(1_000_000);

    let t0 = Instant::now();
    let exit = do_snapshot_via_fork(tmp.path(), 1, &app_state)
        .await
        .expect("fork-snapshot");
    let parent_elapsed = t0.elapsed();
    let exit_str = match exit {
        beava_server::snapshot_fork::ChildExit::Success { .. } => "success".to_string(),
        beava_server::snapshot_fork::ChildExit::Failure { code, message } => {
            format!("FAIL code={code} message={message}")
        }
    };

    println!();
    println!("=== Full fork snapshot @ N=1M entities ===");
    println!(
        "  parent wall-clock: {:.1}ms",
        parent_elapsed.as_secs_f64() * 1000.0
    );
    println!("  child exit:        {exit_str}");
    println!();
    println!("The parent's apply lock was held only across the fork syscall");
    println!("(measured separately, ~0.5ms). Everything else is child work +");
    println!("waitpid; the apply thread was free to serve traffic throughout.");
}
