//! Opt-in "extreme" fork test — 1M entries, asserts fork() lock-hold stays
//! under 10 ms. This is ignored by default because it allocates ~700 MB and is
//! timing-sensitive under shared CI runners.
//!
//! Lives in its own test binary so the process VM is fresh — sibling
//! tests in other files can't bloat the allocator's reserved range and
//! skew the measurement.
//!
//! Memory: ~700 MB peak. Runtime: ~1 s release, ~10 s debug.
//!
//! Why 1M (and not 5M / 10M):
//! - 1M is the "extreme" tier that remains useful as an opt-in regression
//!   check without making default CI depend on 700 MB of spare memory.
//! - 5M+ requires the `--ignored --release` opt-in tests in
//!   `snapshot_big_state.rs` / `snapshot_fork_scaling.rs` /
//!   `snapshot_fork_extreme.rs`.
//! - The <10 ms ceiling locks in the most important production guarantee:
//!   the kalshi-pulse incident (#151) class is gone for any state size up
//!   to 1M entries, on any fresh-or-monotonically-growing process.

use beava_core::agg_op::AggOp;
use beava_core::agg_state::CountState;
use beava_core::agg_state_table::{AggStateTable, EntityKey};
use beava_core::registry::Registry;
use beava_core::row::Value;
use beava_server::registry_debug::DevAggState;
use beava_server::AppState;
use compact_str::CompactString;
use smallvec::smallvec;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

#[cfg(unix)]
fn measure_fork(app_state: &AppState) -> Duration {
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

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "large timing test: allocates ~700 MB; run manually with --ignored"]
async fn fork_at_extreme_state_under_10ms() {
    // 1M entries = ~700 MB RSS. On any modern hardware (Linux physical,
    // macOS arm64, modern EC2 HVM) fork should be well under 10 ms.
    //
    // This is the regression tripwire for the SEV-1 incident class:
    // if anyone re-introduces an O(N) operation under state_tables.lock(),
    // this test detects it at scale.
    let app_state = build_app_state(1_000_000);

    // Warm-up (first fork has cold-cache overhead).
    let _ = measure_fork(&app_state);

    // Median of 5 samples — fork is noisy and we want a stable read.
    let mut samples: Vec<f64> = (0..5)
        .map(|_| measure_fork(&app_state).as_secs_f64() * 1000.0)
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[samples.len() / 2];
    let max = *samples.last().unwrap();

    println!();
    println!("=== fork() lock-hold at 1M entries (extreme, default test) ===");
    println!("  median: {median:.2}ms");
    println!("  max:    {max:.2}ms");
    println!("  samples: {samples:?}");

    // Hard ceiling: 10 ms. Production-relevant guarantee.
    //
    // Empirical (Apple M4 release): median ~0.7 ms.
    // Empirical (debug build): median ~5-8 ms (allocator/syscall debug
    // overhead, not the fix's fault — production runs release).
    //
    // CI margin: 10 ms ceiling absorbs Linux-runner variance + debug-mode
    // overhead while still proving the <10 ms guarantee under the user's
    // documented target.
    assert!(
        median < 10.0,
        "fork lock-hold at 1M entries must be <10ms — got median {median:.2}ms (max {max:.2}ms, samples {samples:?})"
    );
}
