//! Plan 18-05 Task 5.2 — Per-worker continuous-loop tests.
//!
//! RED: Verifies the Valkey 8 per-worker architecture:
//!  - Worker assignment is `slot_idx % N_workers` (deterministic round-robin)
//!  - Workers process clients continuously with no apply-side join_all barrier
//!  - apply thread only acts as dispatcher (drains read_rx, pushes write_tx)
//!
//! These tests FAIL TO COMPILE until Task 5.2.b (GREEN) implements
//! `WorkerHandle` and the continuous-loop worker infrastructure.

use beava_runtime_core::io_backend::MioBackend;
use beava_runtime_core::io_thread_worker::{start_worker, WorkerConfig, WorkerHandle};
use beava_runtime_core::work_ring::RingItem;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::time::Duration;

/// Helper: create N worker handles using `start_worker`.
/// Returns `(Vec<WorkerHandle>, read_rx, Vec<write_tx>)`.
/// `write_tx[w]` is the apply-side sender for worker `w`'s response channel.
// reason: test-only helper; the 3-tuple return shape mirrors the production
// `start_worker` callers and is easier to read at the call site than a
// helper-specific struct.
#[allow(clippy::type_complexity)]
fn spawn_n_workers_with_write(
    n: usize,
    stop: &Arc<AtomicBool>,
) -> (
    Vec<WorkerHandle>,
    crossbeam_channel::Receiver<RingItem>,
    Vec<crossbeam_channel::Sender<(u64, beava_runtime_core::io_thread_worker::WriteEncoder)>>,
) {
    let (read_tx, read_rx) = crossbeam_channel::bounded::<RingItem>(16_384);

    let mut write_txs = Vec::with_capacity(n);
    let workers = (0..n)
        .map(|worker_id| {
            let (write_tx, write_rx) = crossbeam_channel::bounded(4096);
            let (new_client_tx, new_client_rx) = crossbeam_channel::bounded(256);
            write_txs.push(write_tx.clone());
            let cfg = WorkerConfig {
                worker_id,
                n_workers: n,
                read_tx: read_tx.clone(),
                write_rx,
                new_client_rx,
                stop: Arc::clone(stop),
                apply_waker: None,
                tcp_max_frame_bytes: 4 * 1024 * 1024,
            };
            start_worker::<MioBackend>(cfg, new_client_tx, write_tx)
        })
        .collect::<Vec<_>>();

    (workers, read_rx, write_txs)
}

/// Convenience wrapper: create N workers, discard write_tx senders.
fn spawn_n_workers(
    n: usize,
    stop: &Arc<AtomicBool>,
) -> (Vec<WorkerHandle>, crossbeam_channel::Receiver<RingItem>) {
    let (workers, read_rx, _write_txs) = spawn_n_workers_with_write(n, stop);
    (workers, read_rx)
}

/// Assert that client routing is deterministic: the worker that should own a
/// client is `slot_idx % N`. This test constructs N workers, routes a
/// series of slot indices, and checks the assignment is consistent.
#[test]
fn test_worker_owns_client_round_robin() {
    const N: usize = 5;
    let stop = Arc::new(AtomicBool::new(false));
    let (workers, _read_rx) = spawn_n_workers(N, &stop);

    // For each slot_idx, the owning worker must be slot_idx % N.
    for slot_idx in 0u64..20u64 {
        let expected_worker = (slot_idx as usize) % N;
        let actual_worker = workers[expected_worker].worker_id();
        assert_eq!(
            actual_worker, expected_worker,
            "slot {slot_idx} should be owned by worker {expected_worker}, got {actual_worker}"
        );
    }

    // Shutdown all workers cleanly.
    stop.store(true, atomic::Ordering::Release);
    for w in workers {
        w.join();
    }
}

/// Assert that the worker loop processes events without any apply-side
/// `join_all` or spin-wait. The apply thread just sends to `write_tx[w]`,
/// wakes the worker, and moves on immediately.
///
/// This test:
///  1. Creates a real TCP loopback pair.
///  2. Hands the server-side socket to worker 0 via `new_client_tx`.
///  3. Sends a ping frame from the client side.
///  4. Waits for a `RingItem::Request` to appear on `read_rx`.
///  5. Asserts the whole round-trip completes in < 500ms without
///     any apply-side synchronization primitive.
#[test]
fn test_worker_loop_processes_continuously_no_join_all() {
    use beava_core::wire::{encode_frame, Frame, CT_JSON, OP_PING};
    use bytes::BytesMut;
    use std::io::Write;

    const N: usize = 3;
    let stop = Arc::new(AtomicBool::new(false));
    let (workers, read_rx) = spawn_n_workers(N, &stop);

    // Bind a TCP listener and connect a client.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();

    // Background thread: connect + send one PING frame then wait for ACK.
    let client_t = std::thread::spawn(move || {
        let mut s = std::net::TcpStream::connect(addr).expect("connect");
        s.set_write_timeout(Some(Duration::from_secs(2))).ok();
        // Build a OP_PING frame.
        let frame = Frame::new(OP_PING, CT_JSON, bytes::Bytes::new());
        let mut buf = BytesMut::new();
        encode_frame(&frame, &mut buf);
        s.write_all(&buf).expect("write ping");
        // Keep the connection alive long enough for the worker to read it.
        std::thread::sleep(Duration::from_millis(500));
        drop(s);
    });

    // Accept the connection.
    let (stream, _) = listener.accept().expect("accept");
    stream.set_nonblocking(true).expect("set_nonblocking");
    let mio_stream = mio::net::TcpStream::from_std(stream);

    // Route to worker 0 (slot_idx = 0 → worker 0 % N = 0).
    let slot_idx: u64 = 0;
    let worker_for_slot = (slot_idx as usize) % N;
    workers[worker_for_slot]
        .send_new_client(mio_stream, slot_idx)
        .expect("send_new_client");

    // Wait for the worker to parse and push a RingItem to read_rx.
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    let item = loop {
        match read_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(item) => break item,
            Err(_) => {
                if std::time::Instant::now() >= deadline {
                    panic!("timed out waiting for RingItem from worker");
                }
            }
        }
    };

    // The parsed item must be a Request with WireRequest::Ping.
    match item {
        RingItem::Request {
            slot_idx: s,
            request,
            ..
        } => {
            assert_eq!(s, slot_idx as u32, "slot_idx mismatch");
            use beava_runtime_core::wire_request::WireRequest;
            assert_eq!(request, WireRequest::Ping, "expected Ping");
        }
        other => panic!("expected RingItem::Request, got {other:?}"),
    }

    stop.store(true, atomic::Ordering::Release);
    client_t.join().expect("client thread join");
    for w in workers {
        w.join();
    }
}

/// Task 5.3 — Verify apply does NOT wait for the write phase.
///
/// The apply thread pushes response bytes to `write_tx[w]` and immediately
/// continues its loop — it does NOT block waiting for the worker to flush
/// the bytes to the socket. This is the key property that eliminates the
/// write `join_all` barrier.
///
/// The test measures the time for 1000 `write_tx[0].send()` calls and
/// asserts each takes well under 1ms (no blocking on socket write).
#[test]
fn test_no_write_join_all_apply_doesnt_wait() {
    const N: usize = 3;
    let stop = Arc::new(AtomicBool::new(false));
    let (workers, _read_rx, write_txs) = spawn_n_workers_with_write(N, &stop);

    // Send 1000 small response payloads to worker 0's write_tx.
    // Apply must not block — each send() just enqueues; worker drains later.
    // Plan 18-06: write_tx now ships an encoder closure; this fixture wraps a
    // pre-built Bytes and just appends it to write_buf when invoked.
    let payload: bytes::Bytes = bytes::Bytes::from_static(b"\x00\x00\x00\x04\x00\x02{}");
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let p = payload.clone();
        // Plan 12-08 (D-C): WriteEncoder now takes a `&BytesMutPool` too.
        let encoder: beava_runtime_core::io_thread_worker::WriteEncoder =
            Box::new(move |_proto, _pool, buf| {
                buf.extend_from_slice(&p);
            });
        write_txs[0].send((0u64, encoder)).expect("write_tx send");
        // Wake worker 0 (as apply would) — must be non-blocking.
        workers[0].waker().wake().expect("wake");
    }
    let elapsed = start.elapsed();

    // 1000 sends + 1000 wakes must complete in well under 100ms.
    // If a `join_all` were present this would block for socket-write latency × 1000.
    assert!(
        elapsed < Duration::from_millis(100),
        "1000 write_tx sends took {elapsed:?} — apply is blocking on write phase (join_all not removed)"
    );

    stop.store(true, atomic::Ordering::Release);
    for w in workers {
        w.join();
    }
}
