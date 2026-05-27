//! Empirical chart of fork() syscall cost vs beava process VM size.
//!
//! ## Headline finding
//!
//! On Apple M4 release-mode tests, fork()'s lock-hold has TWO regimes:
//!
//! **Fresh process (after boot, before VM grew):**
//! Linear in current RSS. ~1ms per GB. 1M entries → 0.7ms, 10M → 12ms.
//!
//! **Long-lived process (after big allocations + frees):**
//! Linear in *virtual* memory size — which on macOS+libmalloc and
//! Linux+glibc *does not shrink back* after frees in the usual case.
//! 14-18ms even after state shrinks, because the page table the allocator
//! has touched stays mapped.
//!
//! Production implication: a beava process running for days/weeks will
//! see fork latency closer to the long-lived numbers (10-20ms) than to
//! the fresh-process numbers (sub-millisecond).
//!
//! ## Is 10ms too long?
//!
//! For the kalshi-pulse incident (#151): no — going from 2.3 s lock-hold
//! to 15 ms is a 150× improvement. /ping never trips a 3 s healthcheck;
//! incident closed.
//!
//! For beava's 3M EPS/core target: borderline. A 15ms fork queues ~45k
//! events on the apply thread once per 60 s snapshot cycle = 0.025%
//! wall-clock blocked. Tolerable for fraud-serving workloads but visible
//! as a latency spike at snapshot time.
//!
//! ## Run
//!
//! ```sh
//! cargo test --release -p beava-server --test snapshot_fork_scaling \
//!     -- --ignored --nocapture --test-threads=1
//! ```

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

/// Read RSS in bytes (current resident set size of this process).
#[cfg(target_os = "macos")]
fn process_rss_bytes() -> Option<u64> {
    unsafe {
        let mut info: libc::mach_task_basic_info = std::mem::zeroed();
        let mut count = (std::mem::size_of::<libc::mach_task_basic_info>() / 4) as u32;
        #[allow(deprecated)] // libc still re-exports it; mach2 not in deps
        let task = libc::mach_task_self();
        let ret = libc::task_info(
            task,
            libc::MACH_TASK_BASIC_INFO,
            &mut info as *mut _ as *mut i32,
            &mut count,
        );
        if ret == 0 {
            Some(info.resident_size)
        } else {
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn process_rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let rss_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    Some(rss_pages * page_size)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_rss_bytes() -> Option<u64> {
    None
}

/// Sweep fork() syscall latency from 1M to 20M entries; print the curve
/// and the current process RSS at each scale. Informational — no
/// hard assertion, because the answer depends on OS allocator state.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "scaling sweep: builds up to ~6GB; run with --ignored --release"]
async fn fork_syscall_vs_process_rss() {
    println!();
    println!("=== fork() syscall latency vs beava process RSS ===");
    println!(
        "{:>10} {:>12} {:>14} {:>14}",
        "entries", "rss_MB", "fork_ms_median", "fork_ms_max"
    );
    println!("{}", "-".repeat(56));

    let sizes = [1_000_000usize, 2_500_000, 5_000_000, 10_000_000, 20_000_000];

    for &n in &sizes {
        let app_state = build_app_state(n);
        let rss_mb = process_rss_bytes().map(|b| b as f64 / (1024.0 * 1024.0));

        let _ = measure_fork(&app_state); // warm-up
        let mut samples: Vec<f64> = (0..5)
            .map(|_| measure_fork(&app_state).as_secs_f64() * 1000.0)
            .collect();
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = samples[samples.len() / 2];
        let max = *samples.last().unwrap();

        let rss_str = rss_mb
            .map(|m| format!("{:.0}", m))
            .unwrap_or_else(|| "?".to_string());
        println!(
            "{:>10} {:>9} MB {:>11.2}ms {:>11.2}ms",
            n, rss_str, median, max
        );

        drop(app_state);
    }

    println!();
    println!("Caveat: RSS shrinks when state is dropped, but the process's");
    println!("virtual address space (the thing fork() copies page tables for)");
    println!("often does NOT shrink back. Long-lived production processes will");
    println!("see fork closer to the WORST-CASE row in this chart even when");
    println!("current state is small.");
}

/// The user-relevant upper bound at the incident's apparent scale (~5M
/// entries). On a long-running process this is the realistic ceiling:
/// 50ms. Empirically Apple M4 release: 4-15ms depending on prior VM
/// growth. We assert 50ms (3× safety margin over observed worst case).
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "big-state: builds ~2GB; run with --ignored --release"]
async fn fork_lock_hold_at_5m_entries_under_50ms() {
    let app_state = build_app_state(5_000_000);
    let _ = measure_fork(&app_state);
    let mut samples: Vec<f64> = (0..5)
        .map(|_| measure_fork(&app_state).as_secs_f64() * 1000.0)
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[samples.len() / 2];
    let max = *samples.last().unwrap();
    println!();
    println!("fork lock-hold @ N=5M:    median {median:.2}ms  max {max:.2}ms");
    println!("  Apple M4 fresh-process: ~4ms; long-lived process: ~14ms.");
    // Production-realistic ceiling: 50ms. Anything under this means the
    // kalshi-pulse SEV-1 (#151) is resolved; no docker healthcheck is
    // going to trip on a 50ms /ping spike once per 60s.
    assert!(
        median < 50.0,
        "fork at 5M entries must be <50ms — got median {median:.2}ms (max {max:.2}ms)"
    );
}

/// Larger scale — 10M entries (≈3-4 GB RSS, beyond typical fraud workloads
/// but not unreasonable for behavioral analytics). Production-realistic
/// ceiling here: 100ms.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "big-state: builds ~3-4GB; run with --ignored --release"]
async fn fork_lock_hold_at_10m_entries_under_100ms() {
    let app_state = build_app_state(10_000_000);
    let _ = measure_fork(&app_state);
    let mut samples: Vec<f64> = (0..5)
        .map(|_| measure_fork(&app_state).as_secs_f64() * 1000.0)
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[samples.len() / 2];
    let max = *samples.last().unwrap();
    println!();
    println!("fork lock-hold @ N=10M:   median {median:.2}ms  max {max:.2}ms");
    println!("  Apple M4 fresh-process: ~12ms; long-lived process: ~14ms.");
    // 100ms ceiling. At this scale you should be considering whether
    // fork is still the right answer — at 20M+ you'd want ArcSwap or
    // a shard-the-state approach.
    assert!(
        median < 100.0,
        "fork at 10M entries must be <100ms — got median {median:.2}ms (max {max:.2}ms)"
    );
}

/// Stress: 20M entries (~6 GB RSS). This is well beyond what beava
/// recommends per-instance (the 7 KB/entity budget says 100M-entity boxes
/// should be sharded across multiple beava instances). Tests the fork
/// ceiling at near-worst-case single-instance scale.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "stress: builds ~6GB; run with --ignored --release"]
async fn fork_lock_hold_at_20m_entries_under_200ms() {
    let app_state = build_app_state(20_000_000);
    let _ = measure_fork(&app_state);
    let mut samples: Vec<f64> = (0..5)
        .map(|_| measure_fork(&app_state).as_secs_f64() * 1000.0)
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[samples.len() / 2];
    let max = *samples.last().unwrap();
    println!();
    println!("fork lock-hold @ N=20M:   median {median:.2}ms  max {max:.2}ms");
    println!("  Apple M4: ~18-20ms. Past 20M consider ArcSwap.");
    assert!(
        median < 200.0,
        "fork at 20M entries must be <200ms — got median {median:.2}ms (max {max:.2}ms)"
    );
}

/// Best-case floor: small state in a fresh process. This is what
/// production beava sees at boot, before any state has accumulated. Any
/// regression below this baseline (e.g. an extra page-touching allocation
/// somewhere in app_state) would show up here.
///
/// MUST be invoked as the FIRST test in this binary, e.g. by running just
/// this test on its own:
///
///     cargo test --release -p beava-server --test snapshot_fork_scaling \
///         fork_at_1m_fresh_process_under_5ms -- --ignored --nocapture
///
/// Running with the full --test-threads=1 sweep makes this test fail
/// because earlier tests bloat the process VM.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "fresh-process baseline: must run alone — bare cargo test cmd above"]
async fn fork_at_1m_fresh_process_under_5ms() {
    let app_state = build_app_state(1_000_000);
    let _ = measure_fork(&app_state);
    let mut samples: Vec<f64> = (0..5)
        .map(|_| measure_fork(&app_state).as_secs_f64() * 1000.0)
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[samples.len() / 2];
    println!();
    println!("fork @ N=1M (fresh process): median {median:.2}ms");
    println!("  Best-case floor. Long-running process: see other tests.");
    // 1M entries in a never-touched-larger-VM process: <5ms. Apple M4
    // empirical: ~0.7ms.
    assert!(
        median < 5.0,
        "fork at 1M entries on fresh process should be <5ms — got {median:.2}ms (was this test run alone? see comment)"
    );
}
