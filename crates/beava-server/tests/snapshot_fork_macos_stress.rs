//! macOS-specific stress test for the fork()+COW snapshot path.
//!
//! Why this exists: macOS does not install `pthread_atfork` handlers for
//! libdispatch / GCD, and Apple explicitly does not support fork-without-
//! exec when any framework code is loaded. The current child path uses
//! only pure-Rust std (`std::fs`, `bincode`) and `libc::_exit`,
//! deliberately avoiding any surface that could touch GCD. This file
//! guards against the regressions that would invalidate that contract:
//!
//! 1. A future dependency pulling in a libdispatch-using API along the
//!    child path (Foundation / CoreFoundation / NSURL / mach ports).
//! 2. A future code change calling something that hits libdispatch
//!    indirectly through a transitive macOS framework.
//! 3. Cumulative state corruption across many repeated forks (e.g. a
//!    malloc-arena leak or a parking_lot lock that subtly desyncs).
//!
//! Failure mode on macOS: the classic libdispatch corruption symptom is
//! a child that hangs forever in `_dispatch_root_queue_push` (or similar).
//! `do_snapshot_via_fork` has an internal kill/reap timeout, and every
//! assertion below is also bounded with `tokio::time::timeout` so a hang
//! becomes a loud, attributable test failure instead of a CI timeout-kill
//! 10 minutes later.

#![cfg(target_os = "macos")]

use beava_core::agg_op::AggOp;
use beava_core::agg_state::CountState;
use beava_core::agg_state_table::{AggStateTable, EntityKey};
use beava_core::registry::Registry;
use beava_core::row::Value;
use beava_core::snapshot_body::SnapshotBody;
use beava_persistence::SnapshotReader;
use beava_server::registry_debug::DevAggState;
use beava_server::snapshot_fork::{do_snapshot_via_fork_with_wait_timeout, ChildExit};
use beava_server::AppState;
use compact_str::CompactString;
use smallvec::smallvec;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// Per-iteration wall-clock cap. A child that touches GCD on macOS
/// typically hangs immediately; 30 s is well past the legitimate fork
/// snapshot cost for `n_entities=500` (sub-second on Apple-M4).
const PER_ITER_TIMEOUT_SECS: u64 = 30;
const CHILD_WAIT_TIMEOUT_SECS: u64 = PER_ITER_TIMEOUT_SECS - 5;

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

/// 20 fork-snapshots back-to-back, each bounded by `PER_ITER_TIMEOUT_SECS`.
/// If any iteration hangs (classic libdispatch symptom in the child), the
/// timeout fires with an attributable panic instead of CI hanging.
#[tokio::test(flavor = "current_thread")]
async fn fork_snapshot_repeated_macos_does_not_hang() {
    let tmp = TempDir::new().unwrap();
    let app_state = build_app_state(500);

    for i in 0..20u64 {
        let snapshot_lsn = i + 1;
        let exit = match tokio::time::timeout(
            Duration::from_secs(PER_ITER_TIMEOUT_SECS),
            do_snapshot_via_fork_with_wait_timeout(
                tmp.path(),
                snapshot_lsn,
                &app_state,
                Duration::from_secs(CHILD_WAIT_TIMEOUT_SECS),
            ),
        )
        .await
        {
            Ok(res) => {
                res.unwrap_or_else(|e| panic!("iter {i}: fork-snapshot returned error: {e}"))
            }
            Err(_) => panic!(
                "iter {i}: fork-snapshot hung > {PER_ITER_TIMEOUT_SECS}s — \
                 possible libdispatch / GCD corruption in child path"
            ),
        };

        match exit {
            ChildExit::Success { .. } => {}
            ChildExit::Failure { code, message } => {
                panic!("iter {i}: child failed code={code} message={message}");
            }
        }

        let path = tmp.path().join(format!("snapshot-{snapshot_lsn:016x}.bvs"));
        assert!(path.exists(), "iter {i}: snapshot file missing at {path:?}");
        let (header, body) = SnapshotReader::open(&path)
            .unwrap_or_else(|e| panic!("iter {i}: snapshot must decode: {e}"));
        assert_eq!(header.snapshot_lsn, snapshot_lsn);
        let _decoded = SnapshotBody::decode(&body)
            .unwrap_or_else(|e| panic!("iter {i}: body must decode: {e}"));

        // Parent must still be able to acquire the state_tables lock.
        // If the fork path leaked a held lock back into the parent, this
        // would deadlock and the next iteration's timeout would catch it.
        let _guard = app_state.dev_agg.state_tables.lock();
    }
}

/// 10 fork-snapshots while a sibling thread continuously grabs the
/// `state_tables` lock to mutate state. Validates that lock contention
/// + concurrent allocation in the parent doesn't poison the child path
/// (e.g. a malloc arena left mid-mutation at fork time).
#[tokio::test(flavor = "current_thread")]
async fn fork_snapshot_under_concurrent_mutation_macos_stable() {
    let tmp = TempDir::new().unwrap();
    let app_state = Arc::new(build_app_state(500));
    let stop = Arc::new(AtomicBool::new(false));

    let writer_state = Arc::clone(&app_state);
    let writer_stop = Arc::clone(&stop);
    let writer = std::thread::spawn(move || {
        let mut bump: u64 = 0;
        while !writer_stop.load(Ordering::Relaxed) {
            {
                let mut tables = writer_state.dev_agg.state_tables.lock();
                if let Some(table) = tables.get_mut(0) {
                    let key_str = format!("mut_{:03}", bump % 64);
                    let entity_key = EntityKey(smallvec![(
                        CompactString::from("user_id"),
                        Value::Str(CompactString::from(key_str.as_str())),
                    )]);
                    table.insert_from_entity_key(
                        entity_key,
                        vec![AggOp::Count(CountState { n: bump })],
                    );
                    if let Some(ops) = table.single_str.values_mut().next() {
                        if let Some(AggOp::Count(count)) = ops.get_mut(0) {
                            count.n = count.n.wrapping_add(1);
                        }
                    }
                }
            }
            bump = bump.wrapping_add(1);
            if bump % 256 == 0 {
                std::thread::yield_now();
            }
        }
    });

    for i in 0..10u64 {
        let snapshot_lsn = 100 + i;
        let exit = match tokio::time::timeout(
            Duration::from_secs(PER_ITER_TIMEOUT_SECS),
            do_snapshot_via_fork_with_wait_timeout(
                tmp.path(),
                snapshot_lsn,
                &app_state,
                Duration::from_secs(CHILD_WAIT_TIMEOUT_SECS),
            ),
        )
        .await
        {
            Ok(res) => res.unwrap_or_else(|e| panic!("iter {i}: fork-snapshot error: {e}")),
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                panic!(
                    "iter {i}: fork-snapshot hung > {PER_ITER_TIMEOUT_SECS}s \
                     under concurrent parent mutation"
                );
            }
        };
        assert!(
            matches!(exit, ChildExit::Success { .. }),
            "iter {i}: {exit:?}"
        );
    }

    stop.store(true, Ordering::Relaxed);
    writer.join().expect("writer thread join");
}
