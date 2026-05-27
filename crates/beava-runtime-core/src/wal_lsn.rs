//! WAL LSN (Log Sequence Number) watermark tracking.
//!
//! # Four-watermark discipline
//!
//! Four monotonic `AtomicU64` watermarks track durability progress:
//!
//! | Watermark       | Owned by     | Meaning |
//! |-----------------|--------------|---------|
//! | `committed_lsn` | apply thread | Bytes accepted into in-memory buffer (valid for `/push` Periodic acks) |
//! | `written_lsn`   | writer thread | `write(fd)` to kernel page cache completed |
//! | `synced_lsn`    | writer thread | `fsync(fd)` returned (valid for `/push-sync` PerEvent acks) |
//! | `acked_lsn`     | derived       | Policy-selected ack fence (`committed` for Periodic, `synced` for PerEvent) |
//!
//! # Memory ordering
//!
//! All advances use `Release`; all loads use `Acquire`.  This ensures the bytes
//! written before `mark_written` / `mark_synced` are visible to any thread that
//! subsequently observes the watermark advance (Acquire–Release pair).
//!
//! `record()` uses `AcqRel` fetch_add (it is called by the **single** apply
//! thread so there is no write–write race; AcqRel ensures the load side also
//! sees prior releases, which keeps it composable if the design ever grows a
//! second writer).
//!
//! # PerEvent waiter design
//!
//! `/push-sync` callers wait on a `Condvar`. The writer thread calls
//! `notify_all()` after every `mark_synced`. Spurious wakeups are handled by
//! the `while !synced_at_least(lsn)` loop in `wait_for_synced`.
//!
//! This is the **only** blocking primitive on the PerEvent path. The apply hot
//! path (`record`) never touches the Mutex/Condvar.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// A byte-offset used as an LSN (Log Sequence Number).
///
/// Represents the number of bytes appended to the WAL since server start
/// (or since the current WAL segment was opened). Monotonically increasing.
pub type Lsn = u64;

/// Error returned by `wait_for_synced` when the timeout expires before
/// `synced_lsn` reaches the requested value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitTimeout {
    /// The LSN we were waiting for.
    pub requested: Lsn,
    /// The synced watermark at the time of timeout.
    pub synced_at_timeout: Lsn,
}

impl std::fmt::Display for WaitTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "wait_for_synced timeout: requested={}, synced_at_timeout={}",
            self.requested, self.synced_at_timeout
        )
    }
}

impl std::error::Error for WaitTimeout {}

/// Four atomic LSN watermarks + Condvar for PerEvent `/push-sync` waiters.
///
/// The struct is `Send + Sync`; callers wrap it in `Arc<WalLsn>` so both the
/// apply thread and the writer/fsync thread can hold a handle.
pub struct WalLsn {
    /// Bytes accepted into the active in-memory buffer. Set by the apply thread.
    committed: AtomicU64,
    /// `write(fd)` completed to kernel page cache. Set by writer thread.
    written: AtomicU64,
    /// `fsync(fd)` returned. Set by writer thread.
    synced: AtomicU64,
    /// Condvar + dummy Mutex used to notify PerEvent waiters when `synced`
    /// advances. The Mutex guards no real data — just the Condvar protocol.
    synced_condvar: Condvar,
    synced_mutex: Mutex<()>,
}

impl std::fmt::Debug for WalLsn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WalLsn")
            .field("committed", &self.committed())
            .field("written", &self.written())
            .field("synced", &self.synced())
            .finish()
    }
}

impl WalLsn {
    /// Create a new `WalLsn` with all watermarks at zero.
    pub fn new() -> Self {
        Self::new_at(0)
    }

    /// Create a new `WalLsn` with all watermarks already advanced to `lsn`.
    ///
    /// Used after recovery so the hand-rolled WAL ring continues from the
    /// recovered high-water mark instead of reusing low LSNs after restart.
    pub fn new_at(lsn: Lsn) -> Self {
        Self {
            committed: AtomicU64::new(lsn),
            written: AtomicU64::new(lsn),
            synced: AtomicU64::new(lsn),
            synced_condvar: Condvar::new(),
            synced_mutex: Mutex::new(()),
        }
    }

    /// Load the committed watermark (Acquire).
    #[inline]
    pub fn committed(&self) -> Lsn {
        self.committed.load(Ordering::Acquire)
    }

    /// Load the written watermark (Acquire).
    #[inline]
    pub fn written(&self) -> Lsn {
        self.written.load(Ordering::Acquire)
    }

    /// Load the synced watermark (Acquire).
    #[inline]
    pub fn synced(&self) -> Lsn {
        self.synced.load(Ordering::Acquire)
    }

    /// Advance `committed_lsn` by `n` bytes and return the new high-water mark.
    ///
    /// Called by the **apply thread only** after copying `n` bytes into the
    /// active WAL buffer. Hot path: fetch_add + return.
    ///
    /// # Memory ordering
    ///
    /// `AcqRel` — load side ensures the apply thread sees any prior release
    /// from the writer thread (e.g., buffer-free transitions); store side
    /// ensures the new committed value is visible to readers after this call.
    #[inline]
    pub fn record(&self, n: u64) -> Lsn {
        self.committed.fetch_add(n, Ordering::AcqRel) + n
    }

    /// Raise the committed watermark to at least `lsn` without appending
    /// bytes to the hand-rolled WAL.
    ///
    /// The server uses one logical LSN namespace across the legacy
    /// `WalSink` registry WAL and the data-plane ring WAL. When a registry
    /// bump advances the legacy WAL first, the next data-plane append must
    /// jump past that durable point so snapshots can gate both WAL streams
    /// with a single LSN.
    pub fn mark_committed_at_least(&self, lsn: Lsn) {
        self.committed.fetch_max(lsn, Ordering::AcqRel);
    }

    /// Advance `written_lsn` to at least `lsn`.
    ///
    /// Called by the writer thread after `write(fd, ...)` returns.
    /// Uses `Release` so the apply thread can observe the new value via `Acquire`.
    pub fn mark_written(&self, lsn: Lsn) {
        // Monotone: only advance, never retreat.
        let prev = self.written.load(Ordering::Acquire);
        if lsn > prev {
            self.written.store(lsn, Ordering::Release);
        }
    }

    /// Advance `synced_lsn` to at least `lsn` and notify all PerEvent waiters.
    ///
    /// Called by the writer thread after `fsync(fd)` returns.
    /// Acquires the dummy Mutex before notifying Condvar (required by Condvar
    /// protocol) then immediately releases it before the wake to minimise
    /// contention.
    pub fn mark_synced(&self, lsn: Lsn) {
        // Advance synced watermark first (Release so waiters see it).
        let prev = self.synced.load(Ordering::Acquire);
        if lsn > prev {
            self.synced.store(lsn, Ordering::Release);
        }
        // Notify waiters: grab lock briefly, notify_all, release.
        let _guard = self.synced_mutex.lock().unwrap();
        self.synced_condvar.notify_all();
        // guard drops here, unlocking before waiters re-check their condition.
    }

    /// Returns `true` if `synced_lsn` is at least `lsn`.
    #[inline]
    pub fn synced_at_least(&self, lsn: Lsn) -> bool {
        self.synced.load(Ordering::Acquire) >= lsn
    }

    /// Block the calling thread until `synced_lsn ≥ lsn` or `timeout` elapses.
    ///
    /// Returns `Ok(())` when the watermark is satisfied, or
    /// `Err(WaitTimeout { ... })` if the deadline passes first.
    ///
    /// # Usage
    ///
    /// Called only for `/push-sync` (PerEvent mode). The apply hot path must
    /// never call this — the apply thread must remain non-blocking.
    ///
    /// # Implementation
    ///
    /// Uses `Condvar::wait_timeout_while` which handles spurious wakeups
    /// internally: it loops while the condition is false.
    pub fn wait_for_synced(&self, lsn: Lsn, timeout: Duration) -> Result<(), WaitTimeout> {
        // Fast path: already satisfied without taking the lock.
        if self.synced_at_least(lsn) {
            return Ok(());
        }

        let guard = self.synced_mutex.lock().unwrap();
        // wait_timeout_while re-checks while the condition is true (i.e., while
        // NOT yet satisfied). It returns (guard, WaitTimeoutResult).
        let (_guard2, timed_out) = self
            .synced_condvar
            .wait_timeout_while(guard, timeout, |_| !self.synced_at_least(lsn))
            .unwrap();

        if timed_out.timed_out() {
            Err(WaitTimeout {
                requested: lsn,
                synced_at_timeout: self.synced(),
            })
        } else {
            Ok(())
        }
    }
}

impl Default for WalLsn {
    fn default() -> Self {
        Self::new()
    }
}
