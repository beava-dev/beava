//! MioBackend — per-worker mio::Poll adapter.
//!
//! Implements `IoBackend` using `mio::Poll` + `mio::Waker`. Each worker thread
//! owns one `MioBackend` exclusively; no sharing between threads.
//!
//! # Token layout
//!
//! `mio::Token` is a `usize`. We use:
//!  - Token(WAKER_TOKEN) = 0 — reserved for the `mio::Waker` sentinel
//!  - Token(slot_idx + 1) — each client; `slot_idx` starts at 0 so add 1 to
//!    avoid collision with WAKER_TOKEN.
//!
//! # Interest tracking
//!
//! Clients start with `mio::Interest::READABLE` only. When the apply thread
//! delivers a response via `write_tx`, the worker calls
//! `set_interest_writable(slot_idx, true)` and re-registers with
//! `READABLE | WRITABLE`. After the write buffer drains, it re-registers with
//! `READABLE` only.

use super::{IoBackend, IoEvent, MioTcpStream, WakerHandle, WORKER_WAKE_CALLS};
use bytes::BytesMut;
use mio::{Events, Interest, Poll, Token, Waker};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

/// mio token for the Waker (sentinel).
const WAKER_TOKEN: usize = 0;

/// Per-client mio state owned by the worker.
struct MioClientEntry {
    stream: MioTcpStream,
    interest_writable: bool,
}

/// Cross-thread waker backed by a `mio::Waker`.
struct MioWakerHandle {
    inner: Waker,
}

impl WakerHandle for MioWakerHandle {
    fn wake(&self) -> std::io::Result<()> {
        // Bump the wake counter before the actual wake. Tests use this to
        // verify response batching amortises worker wakes.
        WORKER_WAKE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.inner.wake()
    }
}

/// Per-worker mio::Poll backend.
///
/// Each IoPool worker thread owns one `MioBackend`. The apply thread has its
/// own separate `mio::Poll` for the two listeners only.
pub struct MioBackend {
    poll: Poll,
    events: Events,
    waker: Arc<MioWakerHandle>,
    clients: HashMap<u64, MioClientEntry>,
}

impl IoBackend for MioBackend {
    fn new() -> std::io::Result<Self> {
        let poll = Poll::new()?;
        let waker = Arc::new(MioWakerHandle {
            inner: Waker::new(poll.registry(), Token(WAKER_TOKEN))?,
        });
        let events = Events::with_capacity(256);
        Ok(Self {
            poll,
            events,
            waker,
            clients: HashMap::new(),
        })
    }

    fn add_client(&mut self, mut stream: MioTcpStream, slot_idx: u64) -> std::io::Result<()> {
        let token = Token(slot_idx as usize + 1); // +1 to skip WAKER_TOKEN
        self.poll
            .registry()
            .register(&mut stream, token, Interest::READABLE)?;
        self.clients.insert(
            slot_idx,
            MioClientEntry {
                stream,
                interest_writable: false,
            },
        );
        Ok(())
    }

    fn poll(
        &mut self,
        timeout: Option<Duration>,
        events_out: &mut Vec<IoEvent>,
    ) -> std::io::Result<()> {
        self.poll.poll(&mut self.events, timeout)?;

        for event in self.events.iter() {
            let tok = event.token();
            if tok.0 == WAKER_TOKEN {
                events_out.push(IoEvent::WakerSentinel);
                continue;
            }
            let slot_idx = (tok.0 - 1) as u64; // reverse the +1 offset
            if event.is_read_closed() || event.is_error() {
                events_out.push(IoEvent::Closed(slot_idx));
            } else {
                if event.is_readable() {
                    events_out.push(IoEvent::Readable(slot_idx));
                }
                if event.is_writable() {
                    events_out.push(IoEvent::Writable(slot_idx));
                }
            }
        }

        Ok(())
    }

    fn read(&mut self, slot_idx: u64, buf: &mut BytesMut) -> std::io::Result<usize> {
        let entry = match self.clients.get_mut(&slot_idx) {
            Some(e) => e,
            None => return Ok(0),
        };

        let mut tmp = [0u8; 16 * 1024];
        let mut total = 0usize;
        loop {
            match entry.stream.read(&mut tmp) {
                Ok(0) => {
                    // EOF
                    return Ok(total);
                }
                Ok(n) => {
                    buf.extend_from_slice(&tmp[..n]);
                    total += n;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(total)
    }

    fn write(&mut self, slot_idx: u64, data: &[u8]) -> std::io::Result<usize> {
        let entry = match self.clients.get_mut(&slot_idx) {
            Some(e) => e,
            None => return Ok(0),
        };
        match entry.stream.write(data) {
            Ok(n) => Ok(n),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
            Err(e) => Err(e),
        }
    }

    fn close(&mut self, slot_idx: u64) {
        if let Some(mut entry) = self.clients.remove(&slot_idx) {
            let token = Token(slot_idx as usize + 1);
            let _ = self.poll.registry().deregister(&mut entry.stream);
            drop(entry);
            let _ = token; // used implicitly by deregister
        }
    }

    fn waker_handle(&self) -> Arc<dyn WakerHandle> {
        self.waker.clone()
    }

    fn set_interest_writable(&mut self, slot_idx: u64, writable: bool) {
        let entry = match self.clients.get_mut(&slot_idx) {
            Some(e) => e,
            None => return,
        };
        if entry.interest_writable == writable {
            return; // no change needed
        }
        entry.interest_writable = writable;
        let token = Token(slot_idx as usize + 1);
        let interest = if writable {
            Interest::READABLE | Interest::WRITABLE
        } else {
            Interest::READABLE
        };
        let _ = self
            .poll
            .registry()
            .reregister(&mut entry.stream, token, interest);
    }
}
