//! fork()+COW snapshot path — Valkey BGSAVE pattern adapted for beava.
//!
//! The parent acquires `state_tables.lock()` only across the `fork()` syscall
//! (~µs), then immediately releases it. The child inherits a COW snapshot of
//! the parent's address space and writes the snapshot file from its own
//! frozen view of `state_tables`. Apply-thread blocking drops from
//! ~seconds to ~microseconds.
//!
//! Default ON on unix (linux/macos). Set `BEAVA_SNAPSHOT_FORK=0` (or
//! `false`/`no`) to opt back into the legacy in-process synchronous
//! snapshot in `snapshot_task::do_snapshot`. On non-unix targets the
//! fork path is unavailable and the in-process path is always used.
//!
//! ## Safety / fork-correctness notes
//!
//! The single `unsafe { libc::fork() }` call has these invariants:
//!
//! 1. **beava's tokio runtime is `new_current_thread`** (see
//!    `crates/beava-server/src/main.rs` + `quickstart.rs`). Total OS threads
//!    at fork time = the tokio main thread + the mio apply thread + the
//!    `beava-wal-writer-noop` tick thread + possibly a `spawn_blocking`
//!    worker. The forking thread is the tokio main thread (it runs
//!    `snapshot_task`). All other threads vanish in the child.
//!
//! 2. **Allocator is fork-safe.** beava uses the system allocator (glibc on
//!    Linux, libc on macOS). Both have `pthread_atfork` malloc handlers that
//!    take the malloc lock pre-fork and release it post-fork in both
//!    parent and child. `bincode::serialize` in the child therefore allocates
//!    safely.
//!
//! 3. **Locks held by vanished threads are irrelevant.** The parent captures
//!    the registry snapshot before `fork()`, so the child never takes the
//!    registry `RwLock` in its inherited address space. The child only
//!    touches: `app_state.dev_agg.state_tables` (read-only via the lock guard
//!    the forking thread holds), scalar counter copies captured pre-fork, and
//!    `std::fs` (writes the new snapshot file via its own fds). It does NOT
//!    touch WAL state, tokio runtime, the admin sidecar, or any
//!    `parking_lot::Mutex` it didn't already hold at fork time.
//!
//! 4. **Child never returns; calls `libc::_exit`.** `_exit` is async-signal-
//!    safe and skips Rust destructors / atexit handlers / tokio shutdown
//!    that could touch parent state. `std::process::exit` would run atexit
//!    handlers — unsafe in a forked child.
//!
//! 5. **Child error reporting via sidecar file.** Child writes
//!    `snapshot-<lsn>.error` on failure, then `_exit(1)`. Parent reads this
//!    after `waitpid`.

use crate::AppState;
use beava_core::snapshot_body::SnapshotBody;
use beava_persistence::PersistError;
use std::path::Path;
use std::sync::atomic::Ordering;

/// Result of a fork-snapshot. The parent uses `ChildExit::Success` to decide
/// whether to truncate the WAL.
#[derive(Debug)]
pub enum ChildExit {
    /// Child exited with status 0 — snapshot file is durable.
    Success,
    /// Child exited non-zero or with a signal. Snapshot file may be partial
    /// or absent.
    Failure { code: i32, message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotForkError {
    #[error("fork(2) failed: {0}")]
    ForkFailed(std::io::Error),
    #[error("child waitpid failed: {0}")]
    WaitFailed(std::io::Error),
    #[error("persistence: {0}")]
    Persist(#[from] PersistError),
}

/// Whether the fork path is enabled. Default ON on unix (linux/macos);
/// callers can opt out by setting `BEAVA_SNAPSHOT_FORK` to `0`, `false`,
/// `no`, or empty. Any other value (or unset) keeps the fork path on.
/// Reads the env on every call (cold path; cost is negligible vs. a
/// snapshot cycle). Always `false` on non-unix targets — fork(2) is
/// unavailable there.
pub fn fork_enabled() -> bool {
    if !cfg!(unix) {
        return false;
    }
    !matches!(
        std::env::var("BEAVA_SNAPSHOT_FORK").as_deref(),
        Ok("0") | Ok("false") | Ok("FALSE") | Ok("no") | Ok("")
    )
}

/// Perform a snapshot via `fork()` + COW. Returns the child's exit summary
/// so the caller can gate WAL truncation on success.
///
/// The caller is responsible for:
/// - Capturing `snapshot_lsn` BEFORE this call (so the snapshot covers a
///   well-defined LSN even if pushes land between this call and the fork).
/// - Truncating the WAL up to `snapshot_lsn` only on `ChildExit::Success`.
///
/// `snapshot_dir` must exist (the in-process path creates it lazily; the
/// child cannot afford a directory-creation failure mid-flight). Caller
/// should `std::fs::create_dir_all(snapshot_dir)` before this call.
#[cfg(unix)]
pub async fn do_snapshot_via_fork(
    snapshot_dir: &Path,
    snapshot_lsn: u64,
    app_state: &AppState,
) -> Result<ChildExit, SnapshotForkError> {
    use beava_persistence::SnapshotWriter;

    let next_event_id = app_state.dev_agg.next_event_id.load(Ordering::Relaxed);
    let query_time_ms = app_state.dev_agg.query_time_ms.load(Ordering::Relaxed) as i64;
    let registry_snap = app_state.dev_agg.registry.snapshot();

    // Ensure the snapshot dir exists in the parent (cheap; idempotent). The
    // child cannot afford a mkdir failure.
    std::fs::create_dir_all(snapshot_dir)
        .map_err(|e| SnapshotForkError::Persist(PersistError::Io(e)))?;

    let snapshot_dir_owned = snapshot_dir.to_path_buf();
    let app_state_arc = app_state.clone();

    // Briefly take the state_tables lock so the fork sees a quiescent state
    // snapshot. The lock-hold spans only the fork syscall (~µs).
    //
    // SAFETY:
    // - beava's tokio runtime is `new_current_thread`; the forking thread is
    //   the tokio main thread. All other OS threads (mio apply, wal-writer-
    //   noop, spawn_blocking workers) vanish in the child per POSIX.
    // - System malloc (glibc/libc) is fork-safe via pthread_atfork handlers,
    //   so `bincode::serialize` in the child allocates safely.
    // - Child uses the pre-captured registry snapshot and only reads the
    //   inherited state_tables snapshot from `app_state`; no registry RwLock,
    //   no tokio, no WAL, no admin sidecar.
    // - Child calls `libc::_exit` (async-signal-safe; skips at_exit
    //   handlers) rather than `std::process::exit`.
    let pid = {
        let _state_lock = app_state.dev_agg.state_tables.lock();
        unsafe { libc::fork() }
    };

    if pid < 0 {
        return Err(SnapshotForkError::ForkFailed(
            std::io::Error::last_os_error(),
        ));
    }

    if pid == 0 {
        // === CHILD ===
        // Build snapshot from our (now-frozen via COW) view of app_state.
        // The state_tables lock that the parent held at fork time is locked
        // in our address space too; since we're single-threaded, just read
        // through the Mutex.
        let tables = app_state_arc.dev_agg.state_tables.lock();
        let body = SnapshotBody::from_live(&registry_snap, &tables, next_event_id, query_time_ms);
        // Drop guard before encode — encoding allocates but doesn't need the
        // guard live; matches parent-path discipline.
        drop(tables);

        let encoded = match body.encode() {
            Ok(b) => b,
            Err(e) => child_fail(&snapshot_dir_owned, snapshot_lsn, &format!("encode: {e}")),
        };
        let registry_version = body.registry.version;

        match SnapshotWriter::write(
            &snapshot_dir_owned,
            snapshot_lsn,
            registry_version,
            &encoded,
        ) {
            Ok(_) => unsafe {
                libc::_exit(0);
            },
            Err(e) => child_fail(&snapshot_dir_owned, snapshot_lsn, &format!("write: {e}")),
        }
    }

    // === PARENT ===
    // Wait on the child without blocking the tokio runtime: spawn_blocking
    // a `waitpid` call. The lock is already released (the guard's scope
    // ended at the `}` above; we ran `fork()` inside the scope).
    let exit = tokio::task::spawn_blocking(move || -> Result<ChildExit, std::io::Error> {
        let mut status: libc::c_int = 0;
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        if waited < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::WIFEXITED(status) {
            let code = libc::WEXITSTATUS(status);
            if code == 0 {
                Ok(ChildExit::Success)
            } else {
                // Try to read the error sidecar the child wrote, best-effort.
                let err_path =
                    snapshot_dir_owned.join(format!("snapshot-{snapshot_lsn:016x}.error"));
                let message = std::fs::read_to_string(&err_path)
                    .unwrap_or_else(|_| format!("child exited with code {code}"));
                let _ = std::fs::remove_file(&err_path);
                Ok(ChildExit::Failure { code, message })
            }
        } else if libc::WIFSIGNALED(status) {
            let sig = libc::WTERMSIG(status);
            Ok(ChildExit::Failure {
                code: -1,
                message: format!("child killed by signal {sig}"),
            })
        } else {
            Ok(ChildExit::Failure {
                code: -1,
                message: format!("child stopped with status {status}"),
            })
        }
    })
    .await
    .map_err(|e| SnapshotForkError::WaitFailed(std::io::Error::other(format!("join: {e}"))))?
    .map_err(SnapshotForkError::WaitFailed)?;

    Ok(exit)
}

/// Non-unix stub — fork is Linux/macOS only. Beava ships on those platforms.
#[cfg(not(unix))]
pub async fn do_snapshot_via_fork(
    _snapshot_dir: &Path,
    _snapshot_lsn: u64,
    _app_state: &AppState,
) -> Result<ChildExit, SnapshotForkError> {
    Err(SnapshotForkError::ForkFailed(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "fork-snapshot is unix-only",
    )))
}

/// Child-side fatal: write the error message to a sidecar file and `_exit(1)`.
/// Never returns. Marked `-> !` so callers don't need to handle a return.
#[cfg(unix)]
fn child_fail(snapshot_dir: &Path, snapshot_lsn: u64, msg: &str) -> ! {
    let err_path = snapshot_dir.join(format!("snapshot-{snapshot_lsn:016x}.error"));
    // Best-effort: ignore write failure. The parent will fall back to a
    // generic "child exited non-zero" message.
    let _ = std::fs::write(&err_path, msg);
    unsafe { libc::_exit(1) }
}
