//! Per-worker continuous-loop worker.
//!
//! Each worker thread owns:
//!  - One `IoBackend` instance (own mio::Poll + Waker + client map)
//!  - `new_client_rx`: receives new clients from the apply thread
//!  - `write_rx`: receives encoded responses to flush to client sockets
//!  - `read_tx`: sends parsed `RingItem`s to the apply thread
//!  - `stop`: shared shutdown flag
//!
//! # Architecture (Valkey 8 model)
//!
//! Workers run continuously, never blocking the apply thread with a join_all
//! or spin barrier. The apply thread hands off clients via `new_client_tx[w]`
//! and responses via `write_tx[w]`, then wakes the worker with `Waker::wake()`.
//! Workers parse frames, push to `read_tx`, encode+write responses, and drain
//! `new_client_rx` and `write_rx` at the top of each loop iteration.
//!
//! # Client-to-worker routing
//!
//! The apply thread assigns clients deterministically: `slot_idx % N_workers`.
//! This is enforced by the apply thread; the worker trusts the assignment.

use crate::bytes_pool::BytesMutPool;
use crate::io_backend::{IoBackend, IoEvent, MioTcpStream, WakerHandle};
use crate::wire_request::WireRequest;
use crate::work_ring::{ParseErrorKind, RingItem};
use bytes::BytesMut;
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Protocol tag for a client connection.
///
/// Determines which parser and encoder to use in the worker loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerProto {
    /// HTTP/1.1 text-based protocol (via httparse).
    Http,
    /// Framed TCP wire protocol (`[u32 length][u16 op][u8 ct][payload]`).
    Tcp,
}

/// Encoder closure that runs on the worker thread.
///
/// Apply thread builds the closure with the typed response captured by move
/// and ships it through `write_rx`. The worker invokes the closure with the
/// client's protocol, a handle to its `BytesMutPool`, and a mutable reference
/// to the client's `write_buf` — JSON serialization + HTTP/TCP framing
/// happen on the worker, not apply.
///
/// `FnOnce` is appropriate because each closure encodes exactly one response.
/// `Send + 'static` is required for crossbeam channels.
///
/// The encoder takes a `&BytesMutPool` so it can acquire a pre-sized pool
/// buffer for the encode work, fill it, then extend `client_write_buf` from
/// it and release the pool buffer back. The per-response
/// `BytesMut::with_capacity(4096)` allocator hit is amortized over the pool
/// (recycled after the first ~256 responses warm it).
pub type WriteEncoder =
    Box<dyn FnOnce(WorkerProto, &BytesMutPool, &mut bytes::BytesMut) + Send + 'static>;

/// A new client handed from the apply thread to a worker.
pub struct NewClient {
    /// The TCP stream to register with the worker's backend.
    pub stream: MioTcpStream,
    /// Deterministic slot index (`accept_sequence % MAX_SLOTS`).
    pub slot_idx: u64,
    /// Protocol for this connection.
    pub proto: WorkerProto,
}

/// Per-client state owned by the worker (not the apply thread).
struct WorkerClient {
    read_buf: BytesMut,
    write_buf: BytesMut,
    proto: WorkerProto,
    closed: bool,
}

impl WorkerClient {
    fn new(proto: WorkerProto) -> Self {
        Self {
            read_buf: BytesMut::with_capacity(8 * 1024),
            write_buf: BytesMut::new(),
            proto,
            closed: false,
        }
    }
}

/// Configuration passed to `start_worker`.
pub struct WorkerConfig {
    /// Numeric ID of this worker (0..N_workers).
    pub worker_id: usize,
    /// Total number of workers in the pool.
    pub n_workers: usize,
    /// Channel to push parsed `RingItem`s toward the apply thread.
    pub read_tx: Sender<RingItem>,
    /// Channel from apply thread carrying `(slot_idx, encoder)` write jobs.
    ///
    /// Encode is a closure that runs on the worker thread (the worker calls
    /// `encoder(proto, &pool, &mut client.write_buf)`). This moves response
    /// encoding off the apply hot path — apply just boxes the encoder
    /// closure with the response captured by move; the worker pays the JSON
    /// serialization + frame-header cost AND the BytesMut allocation is
    /// served from a per-worker pool (no malloc on the steady-state hot path).
    pub write_rx: Receiver<(u64, WriteEncoder)>,
    /// Channel from apply thread carrying new clients to register.
    pub new_client_rx: Receiver<NewClient>,
    /// Shared stop flag. When `true`, the worker exits after the current iteration.
    pub stop: Arc<AtomicBool>,
    /// Optional `mio::Waker` registered with the apply thread's listener
    /// `EventLoop`. The worker fires this after each successful
    /// `read_tx.send(...)` so apply doesn't sit in `tick(timeout)` while the
    /// worker has fresh items in `read_rx`. `None` keeps apply polling on
    /// its own cadence.
    pub apply_waker: Option<Arc<mio::Waker>>,
    /// Maximum TCP frame size (bytes) accepted by the wire decoder.
    /// Per-worker (not global) so parallel TestServers don't leak limits
    /// across each other. Production callers populate from
    /// `cfg.tcp.max_frame_bytes` (which itself honors the env-var at config
    /// load time). Default in `WorkerConfig`-construction sites: 4 MiB.
    pub tcp_max_frame_bytes: u32,
}

/// Handle to a running worker thread. Returned by `start_worker`.
pub struct WorkerHandle {
    id: usize,
    new_client_tx: Sender<NewClient>,
    /// Waker to interrupt the worker's `backend.poll()` when new work arrives.
    waker: Arc<dyn WakerHandle>,
    /// JoinHandle for the worker OS thread.
    join: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    /// Return the numeric ID of this worker (0..N_workers).
    pub fn worker_id(&self) -> usize {
        self.id
    }

    /// Join the worker OS thread. Consumes the handle.
    pub fn join(mut self) {
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }

    /// Send a new client to this worker (default: TCP protocol).
    ///
    /// The apply thread selects the worker with `slot_idx % N_workers`.
    /// After sending, wakes the worker out of `backend.poll()`.
    pub fn send_new_client(
        &self,
        stream: MioTcpStream,
        slot_idx: u64,
    ) -> Result<(), crossbeam_channel::SendError<NewClient>> {
        self.new_client_tx.send(NewClient {
            stream,
            slot_idx,
            proto: WorkerProto::Tcp,
        })?;
        let _ = self.waker.wake();
        Ok(())
    }

    /// Send a new client with explicit protocol tag.
    pub fn send_new_client_with_proto(
        &self,
        stream: MioTcpStream,
        slot_idx: u64,
        proto: WorkerProto,
    ) -> Result<(), crossbeam_channel::SendError<NewClient>> {
        self.new_client_tx.send(NewClient {
            stream,
            slot_idx,
            proto,
        })?;
        let _ = self.waker.wake();
        Ok(())
    }

    /// Return a clone of the waker handle. Apply thread uses this to wake the
    /// worker after pushing to `write_tx[w]`.
    pub fn waker(&self) -> Arc<dyn WakerHandle> {
        Arc::clone(&self.waker)
    }
}

/// Spawn a worker OS thread and return its `WorkerHandle`.
///
/// `new_client_tx` and `write_tx` are the apply-side sender ends stored in
/// the returned `WorkerHandle`. Their receiver counterparts (`cfg.new_client_rx`,
/// `cfg.write_rx`) are moved into the worker thread.
pub fn start_worker<B: IoBackend>(
    cfg: WorkerConfig,
    new_client_tx: Sender<NewClient>,
    _write_tx: Sender<(u64, WriteEncoder)>,
) -> WorkerHandle {
    let worker_id = cfg.worker_id;

    // Create the backend *before* spawning — this lets us extract the waker
    // handle (which the apply thread needs to interrupt poll()) before the
    // backend moves into the worker thread.
    let backend = B::new().expect("IoBackend::new() failed in worker spawn");
    let waker = backend.waker_handle();
    let waker_for_handle = Arc::clone(&waker);

    let join = std::thread::Builder::new()
        .name(format!("beava-io-worker-{worker_id}"))
        .spawn(move || {
            worker_main_loop(backend, cfg);
        })
        .expect("failed to spawn worker thread");

    // Drop the local clone — we only need `waker_for_handle` in the handle.
    drop(waker);

    WorkerHandle {
        id: worker_id,
        new_client_tx,
        waker: waker_for_handle,
        join: Some(join),
    }
}

/// The main loop executed on each worker OS thread.
///
/// Per iteration:
///  1. Drain `new_client_rx` — register new clients with the backend.
///  2. Drain `write_rx` — append encoded bytes to per-client write buffers.
///  3. `backend.poll(Some(IDLE_TIMEOUT), &mut events)` — block until I/O ready.
///  4. For each event:
///     - `Readable(s)` → read + parse → push `RingItem` to `read_tx`.
///     - `Writable(s)` → flush write buffer to socket.
///     - `Closed(s)` → remove client.
///     - `WakerSentinel` → re-loop (channels may have new work).
///  5. Check stop flag; break if set.
fn worker_main_loop<B: IoBackend>(mut backend: B, cfg: WorkerConfig) {
    let WorkerConfig {
        worker_id: _worker_id,
        n_workers: _n_workers,
        read_tx,
        write_rx,
        new_client_rx,
        stop,
        apply_waker,
        tcp_max_frame_bytes,
    } = cfg;

    // Idle poll timeout: 1 second. Waker interrupts this for hot paths.
    const IDLE_TIMEOUT: Duration = Duration::from_secs(1);

    // Per-worker BytesMutPool. Sized for typical fraud-team batch response
    // (~3-5 KiB JSON); 256 cap = 1 MiB per worker; 4096 capacity each. NOT
    // shared across workers — each worker constructs its own pool here so
    // there's no cross-thread contention on acquire/release.
    const POOL_CAP: usize = 256;
    const POOL_BUF_CAPACITY: usize = 4096;
    let pool = BytesMutPool::new(POOL_CAP, POOL_BUF_CAPACITY);

    let mut events: Vec<IoEvent> = Vec::with_capacity(64);
    let mut clients: HashMap<u64, WorkerClient> = HashMap::new();

    loop {
        // 1. Drain new_client_rx — register all queued new clients.
        while let Ok(nc) = new_client_rx.try_recv() {
            if let Ok(()) = backend.add_client(nc.stream, nc.slot_idx) {
                clients.insert(nc.slot_idx, WorkerClient::new(nc.proto));
            }
        }

        // 2. Drain write_rx — invoke each encoder closure to fill write_buf.
        // Encoder runs HERE (worker thread), not on apply: JSON serialization
        // + frame headers happen off the apply hot path. The closure uses
        // `pool` to acquire a pre-sized BytesMut, fills it with the framed
        // response, extends `c.write_buf` from the pool buffer, then releases
        // it back — zero per-response BytesMut::with_capacity calls on the
        // steady-state hot path.
        while let Ok((slot, encoder)) = write_rx.try_recv() {
            if let Some(c) = clients.get_mut(&slot) {
                let proto = c.proto;
                encoder(proto, &pool, &mut c.write_buf);
                // Arm WRITABLE interest so the backend fires Writable events.
                backend.set_interest_writable(slot, true);
            }
        }

        // 3. Poll for I/O events.
        events.clear();
        if let Err(e) = backend.poll(Some(IDLE_TIMEOUT), &mut events) {
            tracing::warn!(target: "beava.worker", "poll error: {e}");
            if stop.load(Ordering::Acquire) {
                break;
            }
            continue;
        }

        // 4. Process events.
        let mut to_close: Vec<u64> = Vec::new();
        // Track whether we sent anything to apply this pass. If yes, fire
        // `apply_waker` once at the end so apply doesn't sit in
        // `event_loop.tick(timeout)` while there's work in `read_rx`.
        let mut sent_to_apply = false;
        for ev in events.drain(..) {
            match ev {
                IoEvent::Readable(slot) => {
                    let client = match clients.get_mut(&slot) {
                        Some(c) => c,
                        None => continue,
                    };
                    if client.closed {
                        continue;
                    }
                    let n = match backend.read(slot, &mut client.read_buf) {
                        Ok(n) => n,
                        Err(_) => {
                            client.closed = true;
                            continue;
                        }
                    };
                    if n == 0 {
                        // EOF
                        client.closed = true;
                        continue;
                    }
                    // Parse frames and push to read_tx.
                    if parse_and_push(slot, client, &read_tx, tcp_max_frame_bytes) > 0 {
                        sent_to_apply = true;
                    }
                }
                IoEvent::Writable(slot) => {
                    let client = match clients.get_mut(&slot) {
                        Some(c) => c,
                        None => continue,
                    };
                    flush_write_buf(slot, client, &mut backend);
                }
                IoEvent::Closed(slot) => {
                    to_close.push(slot);
                }
                IoEvent::WakerSentinel => {
                    // Re-loop: new work may have arrived on the channels.
                }
            }
        }

        // Wake apply iff we actually pushed RingItems this pass. The waker
        // is edge-triggered + idempotent; one wake per pass is enough to
        // interrupt apply's `event_loop.tick()` and force it to re-drain
        // `read_rx`.
        if sent_to_apply {
            if let Some(w) = &apply_waker {
                let _ = w.wake();
            }
        }

        // Process closed events (can't borrow clients and backend mutably at same time).
        for slot in to_close {
            clients.remove(&slot);
            backend.close(slot);
        }

        // Clean up clients that were marked closed during read/write.
        // Flush any pending write_buf before close — otherwise inline-encoded
        // error frames (frame_too_large) are lost when the close cleanup
        // runs in the same loop iteration as the parse error.
        let closed: Vec<u64> = clients
            .iter()
            .filter(|(_, c)| c.closed)
            .map(|(&s, _)| s)
            .collect();
        for slot in closed {
            if let Some(client) = clients.get_mut(&slot) {
                if !client.write_buf.is_empty() {
                    flush_write_buf(slot, client, &mut backend);
                }
            }
            clients.remove(&slot);
            backend.close(slot);
        }

        // 5. Check stop flag.
        if stop.load(Ordering::Acquire) {
            break;
        }
    }
}

/// Inline-encode a `frame_too_large` error frame into the client's
/// `write_buf`. The connection MUST close after the frame, so we can't take
/// the apply→worker round-trip without racing against the close-cleanup
/// loop.
///
/// Wire format mirrors `encode_glue_response_tcp`'s TcpError arm.
fn inline_encode_frame_too_large(write_buf: &mut bytes::BytesMut, declared: u32, limit: u32) {
    use beava_core::wire::{CT_JSON, OP_ERROR_RESPONSE};
    use bytes::BufMut;
    let body = serde_json::json!({
        "error": {
            "code": "frame_too_large",
            "message": format!("declared frame length {declared} exceeds limit {limit}"),
            "limit": limit,
            "declared": declared,
        },
    });
    let payload = serde_json::to_vec(&body).unwrap_or_default();
    // Frame: [u32 length BE][u16 op BE][u8 ct][payload]
    let frame_len = 2 + 1 + payload.len() as u32;
    write_buf.put_u32(frame_len);
    write_buf.put_u16(OP_ERROR_RESPONSE);
    write_buf.put_u8(CT_JSON);
    write_buf.extend_from_slice(&payload);
}

/// Parse frames from a client's `read_buf` and push `RingItem`s to `read_tx`.
/// Returns the number of `RingItem`s sent so callers can wake apply if any
/// were pushed.
fn parse_and_push(
    slot: u64,
    client: &mut WorkerClient,
    read_tx: &Sender<RingItem>,
    tcp_max_frame_bytes: u32,
) -> usize {
    use crate::http_listener::parse_http_request;
    use crate::tcp_listener::parse_wire_request;
    use beava_core::row::Row;
    use beava_core::wire::CT_MSGPACK;

    if client.read_buf.is_empty() {
        return 0;
    }

    // Pre-deserialise body→Row for push variants (same as old IoPool worker).
    let body_to_row = |req: &WireRequest| -> Option<Row> {
        match req {
            WireRequest::TcpPush {
                body, body_format, ..
            }
            | WireRequest::HttpPush {
                body, body_format, ..
            }
            | WireRequest::HttpPushSync {
                body, body_format, ..
            }
            | WireRequest::HttpPushBatch {
                body, body_format, ..
            } => {
                if *body_format == CT_MSGPACK {
                    rmp_serde::from_slice::<Row>(body).ok()
                } else {
                    sonic_rs::from_slice::<Row>(body).ok()
                }
            }
            _ => None,
        }
    };

    let slot_u32 = slot as u32;
    let mut sent: usize = 0;

    match client.proto {
        WorkerProto::Tcp => loop {
            match parse_wire_request(&mut client.read_buf, tcp_max_frame_bytes) {
                Ok(Some(req)) => {
                    let parsed_row = body_to_row(&req);
                    if read_tx
                        .send(RingItem::Request {
                            slot_idx: slot_u32,
                            keep_alive: false,
                            request: req,
                            parsed_row,
                        })
                        .is_err()
                    {
                        return sent; // receiver dropped = shutting down
                    }
                    sent += 1;
                }
                Ok(None) => break,
                Err(e) => {
                    // Distinguish frame_too_large from generic frame parse
                    // errors so the apply path can emit the rich error frame
                    // (with `limit` field) and close the connection.
                    //
                    // For frame_too_large specifically: the connection MUST
                    // close after the error frame. Inline-write the error
                    // frame into client.write_buf BEFORE marking closed —
                    // the apply→worker→write round-trip would race against
                    // the close cleanup loop and drop the frame.
                    let kind = match e {
                        beava_core::wire::FrameError::TooLarge {
                            declared_len,
                            limit,
                        } => {
                            inline_encode_frame_too_large(
                                &mut client.write_buf,
                                declared_len,
                                limit,
                            );
                            ParseErrorKind::TcpFrameTooLarge {
                                declared: declared_len,
                                limit,
                            }
                        }
                        _ => ParseErrorKind::TcpFrame,
                    };
                    if read_tx
                        .send(RingItem::ParseError {
                            slot_idx: slot_u32,
                            kind,
                        })
                        .is_ok()
                    {
                        sent += 1;
                    }
                    client.closed = true;
                    break;
                }
            }
        },
        WorkerProto::Http => loop {
            match parse_http_request(&mut client.read_buf) {
                Ok(Some((req, keep_alive))) => {
                    let parsed_row = body_to_row(&req);
                    if read_tx
                        .send(RingItem::Request {
                            slot_idx: slot_u32,
                            keep_alive,
                            request: req,
                            parsed_row,
                        })
                        .is_err()
                    {
                        return sent;
                    }
                    sent += 1;
                }
                Ok(None) => break,
                Err(_) => {
                    if read_tx
                        .send(RingItem::ParseError {
                            slot_idx: slot_u32,
                            kind: ParseErrorKind::HttpProtocol,
                        })
                        .is_ok()
                    {
                        sent += 1;
                    }
                    client.closed = true;
                    break;
                }
            }
        },
    }

    sent
}

/// Flush `client.write_buf` to the socket via `backend.write()`.
/// Removes fully-written bytes; disarms WRITABLE interest if buffer is empty.
fn flush_write_buf<B: IoBackend>(slot: u64, client: &mut WorkerClient, backend: &mut B) {
    while !client.write_buf.is_empty() {
        match backend.write(slot, &client.write_buf) {
            Ok(0) => break, // WouldBlock or closed
            Ok(n) => {
                let _ = client.write_buf.split_to(n);
            }
            Err(_) => {
                client.closed = true;
                return;
            }
        }
    }
    if client.write_buf.is_empty() {
        // Revert to READABLE-only to avoid busy-looping on writable.
        backend.set_interest_writable(slot, false);
    }
}
