//! Lock-hold comparison: legacy in-process snapshot vs fork() snapshot.
//!
//! The legacy path holds `state_tables.lock()` for the entire duration of
//! `SnapshotBody::from_live` (which deep-clones every entry). The fork path
//! holds it only across the `fork()` syscall (~µs).
//!
//! These tests measure the lock-hold duration **directly inline** rather
//! than racing a side-thread sampler, so they're CI-stable.

use beava_core::agg_op::AggOp;
use beava_core::agg_state::CountState;
use beava_core::agg_state_table::{AggStateTable, EntityKey};
use beava_core::registry::Registry;
use beava_core::row::Value;
use beava_core::snapshot_body::SnapshotBody;
use beava_persistence::SnapshotWriter;
use beava_server::registry_debug::DevAggState;
use beava_server::snapshot_fork::do_snapshot_via_fork;
use beava_server::AppState;
use compact_str::CompactString;
use smallvec::smallvec;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[allow(dead_code)] // SnapshotBody used only in the full-snapshot helper
fn _import_anchor() {
    let _: Option<&SnapshotBody> = None;
}

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

/// Measure the legacy lock-hold scope **directly**.
///
/// Mirrors the inner loop of `SnapshotBody::from_live` byte-for-byte
/// (`table.iter_sorted().map(clone, clone).collect()`) — that's the
/// operation under `state_tables.lock()` in the legacy path.
///
/// We bypass `SnapshotBody::from_live` here because that function iterates
/// `Registry::compiled_aggregations`, which is empty in this test
/// harness (we populate StateTables directly to avoid the register-
/// validate path's complexity). The clone-collect we time below is
/// what `from_live` would do once for each registered aggregation.
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

/// End-to-end legacy snapshot (used to confirm the full path still works
/// — not used for lock-hold timing). Returns total wall time.
#[allow(dead_code)]
fn run_legacy_snapshot_full(app_state: &AppState, dir: &Path, lsn: u64) -> Duration {
    let next_event_id = app_state.dev_agg.next_event_id.load(Ordering::Relaxed);
    let query_time_ms = app_state.dev_agg.query_time_ms.load(Ordering::Relaxed) as i64;
    let t0 = Instant::now();
    let body = {
        let registry_snap = app_state.dev_agg.registry.snapshot();
        let tables = app_state.dev_agg.state_tables.lock();
        SnapshotBody::from_live(&registry_snap, &tables, next_event_id, query_time_ms)
    };
    let encoded = body.encode().expect("encode");
    SnapshotWriter::write(dir, lsn, body.registry.version, &encoded).expect("write");
    t0.elapsed()
}

/// Direct measurement of the fork path's lock-hold scope — the parent
/// only holds the lock across the `libc::fork()` syscall.
#[cfg(unix)]
fn measure_fork_parent_lock_hold(app_state: &AppState) -> Duration {
    // Mirrors snapshot_fork::do_snapshot_via_fork lock scope verbatim.
    // We can't reuse the public function because it does the whole
    // snapshot. This measures ONLY the parent-side lock scope.
    let lock_start = Instant::now();
    let state_lock = app_state.dev_agg.state_tables.lock();
    let pid = unsafe { libc::fork() };
    let lock_held = lock_start.elapsed();

    if pid == 0 {
        // Child: exit immediately. Snapshot itself is tested elsewhere.
        unsafe { libc::_exit(0) };
    }
    assert!(pid > 0, "fork failed: {}", std::io::Error::last_os_error());

    drop(state_lock);

    // Reap so we don't leak a zombie.
    let mut status: libc::c_int = 0;
    unsafe {
        libc::waitpid(pid, &mut status, 0);
    }

    lock_held
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn legacy_lock_hold_scales_with_state_size() {
    println!();
    println!("=== Legacy in-process snapshot: lock-hold scales with N ===");
    println!("{:>10} {:>14}", "entries", "lock_held_ms");
    println!("{}", "-".repeat(28));

    let mut prev_lock_ms = 0.0;
    for &n in &[1_000usize, 50_000, 200_000] {
        let app_state = build_app_state(n);
        let _ = measure_legacy_lock_hold(&app_state); // warm-up
        let mut samples: Vec<f64> = (0..3)
            .map(|_| measure_legacy_lock_hold(&app_state).as_secs_f64() * 1000.0)
            .collect();
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let lock_ms = samples[1];

        println!("{:>10} {:>11.2}ms", n, lock_ms);

        // Each step up in N must increase lock_held — proves the legacy
        // path is O(N) in state size.
        if n > 1_000 {
            assert!(
                lock_ms > prev_lock_ms,
                "legacy lock_held must scale with N — N={n} got {lock_ms:.2}ms, prev {prev_lock_ms:.2}ms"
            );
        }
        prev_lock_ms = lock_ms;
    }

    // At 200k entries the lock-held duration must be visibly non-trivial.
    // Loose floor (1ms) to avoid CI flake while still proving blocking
    // behavior exists.
    assert!(
        prev_lock_ms >= 1.0,
        "legacy lock-hold at 200k entries should be >=1ms — got {prev_lock_ms:.2}ms"
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn fork_lock_hold_is_microseconds_regardless_of_state_size() {
    println!();
    println!("=== fork() snapshot: lock-hold is O(1) (fork syscall only) ===");
    println!("{:>10} {:>14}", "entries", "lock_held_ms");
    println!("{}", "-".repeat(28));

    for &n in &[1_000usize, 50_000, 200_000] {
        let app_state = build_app_state(n);

        // Warm-up.
        let _ = measure_fork_parent_lock_hold(&app_state);

        // Median of 3.
        let mut samples = Vec::with_capacity(3);
        for _ in 0..3 {
            samples.push(measure_fork_parent_lock_hold(&app_state).as_secs_f64() * 1000.0);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let lock_ms = samples[1];

        println!("{:>10} {:>11.2}ms", n, lock_ms);

        // Generous CI margin: fork syscall + lock release should be well
        // under 50ms even on slow runners. Production sub-millisecond on
        // Apple M4 + low-single-digit ms on Linux CI.
        assert!(
            lock_ms < 50.0,
            "fork lock-hold must be O(1) — N={n} got {lock_ms:.2}ms"
        );
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn fork_vs_legacy_lock_hold_at_same_state_size() {
    // Side-by-side comparison at a fixed state size. Fork should be
    // dramatically shorter (orders of magnitude).
    let app_state = build_app_state(100_000);

    // Warm-up both.
    let _ = measure_legacy_lock_hold(&app_state);
    let _ = measure_fork_parent_lock_hold(&app_state);

    // Median of 3 for both.
    let mut legacy_samples: Vec<f64> = (0..3)
        .map(|_| measure_legacy_lock_hold(&app_state).as_secs_f64() * 1000.0)
        .collect();
    let mut fork_samples: Vec<f64> = (0..3)
        .map(|_| measure_fork_parent_lock_hold(&app_state).as_secs_f64() * 1000.0)
        .collect();
    legacy_samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    fork_samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let legacy_ms = legacy_samples[1];
    let fork_ms = fork_samples[1];
    let speedup = legacy_ms / fork_ms.max(0.001);

    println!();
    println!("=== Lock-hold comparison @ N=100k entities ===");
    println!("  legacy: {legacy_ms:>7.2}ms (median of 3)");
    println!("  fork:   {fork_ms:>7.2}ms (median of 3)");
    println!("  speedup: {speedup:>5.1}×");
    println!();

    // The point of the fix: fork must be at least 5× faster than legacy
    // at this scale. (Empirically Apple M4: legacy ~30ms, fork ~0.3ms,
    // speedup ~100×. CI floor 5× covers worst-case Linux runner variance.)
    assert!(
        speedup >= 5.0,
        "fork must be >=5× faster than legacy at N=100k — got {speedup:.1}× (legacy={legacy_ms:.2}ms fork={fork_ms:.2}ms)"
    );
}

/// End-to-end: full `do_snapshot_via_fork` (including the child's encode +
/// write + waitpid) must still leave the parent's apply lock available
/// quickly. This is the integration-level guarantee.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn fork_full_path_apply_lock_available_during_child_work() {
    let tmp = TempDir::new().unwrap();
    let app_state = build_app_state(100_000);

    let app_for_probe = app_state.clone();
    let probe = std::thread::spawn(move || {
        // Try to grab the lock in a tight loop after a brief warm-up.
        // After fork() returns in the parent, the lock should be available
        // immediately even though the child is still serializing.
        std::thread::sleep(Duration::from_millis(2));
        let mut acquired_ms = None;
        let t0 = Instant::now();
        loop {
            if let Some(_g) = app_for_probe.dev_agg.state_tables.try_lock() {
                acquired_ms = Some(t0.elapsed());
                break;
            }
            if t0.elapsed() > Duration::from_secs(5) {
                break;
            }
            std::thread::sleep(Duration::from_micros(50));
        }
        acquired_ms
    });

    let t_parent = Instant::now();
    let _ = do_snapshot_via_fork(tmp.path(), 1, &app_state)
        .await
        .expect("fork-snapshot");
    let parent_total = t_parent.elapsed();
    let acquired_ms = probe.join().unwrap();

    println!();
    println!("=== Full fork path: parent apply-lock availability ===");
    println!(
        "  parent total (incl. waitpid): {:.2}ms",
        parent_total.as_secs_f64() * 1000.0
    );
    if let Some(d) = acquired_ms {
        println!(
            "  probe acquired lock at: +{:.2}ms after probe start",
            d.as_secs_f64() * 1000.0
        );
    } else {
        println!("  probe never acquired lock within 5s");
    }

    let acquired = acquired_ms.expect("probe should acquire the lock");
    // Apply thread should be unblocked within 100ms even at 100k entries.
    // This is the user-visible guarantee.
    assert!(
        acquired < Duration::from_millis(100),
        "apply lock must be available within 100ms — got {:?}",
        acquired
    );
}
