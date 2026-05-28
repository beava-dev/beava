//! fork()+COW snapshot path — Valkey BGSAVE pattern adapted for beava.
//!
//! The parent acquires `state_tables.lock()` across the `fork()` syscall, then
//! immediately releases it in the parent. The child inherits the held guard and
//! reads through that guard without taking any new `parking_lot` locks. Apply-
//! thread blocking drops from ~seconds to the fork syscall window.
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
use beava_persistence::{PersistError, SnapshotWriteStats};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
#[cfg(unix)]
use std::time::{Duration, Instant};

/// Result of a fork-snapshot. The parent uses `ChildExit::Success` to decide
/// whether to truncate the WAL.
#[derive(Debug)]
pub enum ChildExit {
    /// Child exited with status 0 — snapshot file is durable.
    Success {
        snapshot_lsn: u64,
        write_stats: Option<SnapshotWriteStats>,
    },
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
/// so the caller can gate WAL reclamation on success.
///
/// The caller is responsible for:
/// - Passing the legacy `WalSink` LSN. This function combines it with the
///   applied data-plane watermark while holding `state_tables.lock()`, so the
///   snapshot LSN matches the state snapshot the child inherits.
/// - Truncating the WAL up to `snapshot_lsn` only on `ChildExit::Success`.
///
/// `snapshot_dir` must exist (the in-process path creates it lazily; the
/// child cannot afford a directory-creation failure mid-flight). Caller
/// should `std::fs::create_dir_all(snapshot_dir)` before this call.
#[cfg(unix)]
pub async fn do_snapshot_via_fork(
    snapshot_dir: &Path,
    legacy_snapshot_lsn: u64,
    app_state: &AppState,
) -> Result<ChildExit, SnapshotForkError> {
    do_snapshot_via_fork_with_wait_timeout(
        snapshot_dir,
        legacy_snapshot_lsn,
        app_state,
        snapshot_fork_wait_timeout(),
    )
    .await
}

#[cfg(unix)]
pub async fn do_snapshot_via_fork_with_wait_timeout(
    snapshot_dir: &Path,
    legacy_snapshot_lsn: u64,
    app_state: &AppState,
    wait_timeout: Duration,
) -> Result<ChildExit, SnapshotForkError> {
    use beava_persistence::SnapshotWriter;

    // Ensure the snapshot dir exists in the parent (cheap; idempotent). The
    // child cannot afford a mkdir failure.
    std::fs::create_dir_all(snapshot_dir)
        .map_err(|e| SnapshotForkError::Persist(PersistError::Io(e)))?;

    let snapshot_dir_owned = snapshot_dir.to_path_buf();
    let registry_snap = app_state.dev_agg.registry.snapshot();

    let (state_lock, next_event_id, query_time_ms, snapshot_lsn) = loop {
        let candidate_next_event_id = app_state.dev_agg.next_event_id.load(Ordering::Acquire);
        let candidate_snapshot_lsn = legacy_snapshot_lsn.max(candidate_next_event_id);
        let _ = std::fs::remove_file(snapshot_stats_sidecar_path(
            snapshot_dir,
            candidate_snapshot_lsn,
        ));
        let _ = std::fs::remove_file(snapshot_error_sidecar_path(
            snapshot_dir,
            candidate_snapshot_lsn,
        ));

        let state_lock = app_state.dev_agg.state_tables.lock();
        let next_event_id = app_state.dev_agg.next_event_id.load(Ordering::Acquire);
        let query_time_ms = app_state.dev_agg.query_time_ms.load(Ordering::Acquire) as i64;
        let snapshot_lsn = legacy_snapshot_lsn.max(next_event_id);
        if snapshot_lsn == candidate_snapshot_lsn {
            break (state_lock, next_event_id, query_time_ms, snapshot_lsn);
        }
        drop(state_lock);
    };

    // Briefly take the state_tables lock so the fork sees a quiescent state
    // snapshot. The lock-hold spans two scalar loads plus the fork syscall
    // (~µs); path setup, registry capture, and sidecar cleanup happened above.
    //
    // SAFETY:
    // - beava's tokio runtime is `new_current_thread`; the forking thread is
    //   the tokio main thread. All other OS threads (mio apply, wal-writer-
    //   noop, spawn_blocking workers) vanish in the child per POSIX.
    // - System malloc (glibc/libc) is fork-safe via pthread_atfork handlers,
    //   so `bincode::serialize` in the child allocates safely.
    // - Child uses the pre-captured registry snapshot and the inherited
    //   `state_lock` guard; it takes no registry RwLock, no parking_lot Mutex,
    //   no tokio, no WAL, no admin sidecar.
    // - Child calls `libc::_exit` (async-signal-safe; skips at_exit
    //   handlers) rather than `std::process::exit`.
    let pid = unsafe { libc::fork() };

    if pid < 0 {
        return Err(SnapshotForkError::ForkFailed(
            std::io::Error::last_os_error(),
        ));
    }

    if pid == 0 {
        // === CHILD ===
        // Build snapshot from our (now-frozen via COW) view of app_state.
        // Do not lock or unlock `state_tables` in the child. The forking
        // thread already held this guard, and `_exit` skips its destructor.
        let body =
            SnapshotBody::from_live(&registry_snap, &state_lock, next_event_id, query_time_ms);

        let encoded = match body.encode() {
            Ok(b) => b,
            Err(e) => child_fail(&snapshot_dir_owned, snapshot_lsn, &format!("encode: {e}")),
        };
        let registry_version = body.registry.version;

        match SnapshotWriter::write_with_stats(
            &snapshot_dir_owned,
            snapshot_lsn,
            registry_version,
            &encoded,
        ) {
            Ok(stats) => {
                if let Err(e) = write_stats_sidecar(&snapshot_dir_owned, snapshot_lsn, &stats) {
                    child_fail(&snapshot_dir_owned, snapshot_lsn, &format!("stats: {e}"));
                }
                unsafe {
                    libc::_exit(0);
                }
            }
            Err(e) => child_fail(&snapshot_dir_owned, snapshot_lsn, &format!("write: {e}")),
        }
    }

    // === PARENT ===
    drop(state_lock);

    // Wait on the child without blocking the tokio runtime. The blocking side
    // polls with WNOHANG so a wedged fork child can be killed and reaped instead
    // of leaving an uncancelable waitpid task behind.
    let exit = tokio::task::spawn_blocking(move || {
        wait_for_snapshot_child(pid, snapshot_dir_owned, snapshot_lsn, wait_timeout)
    })
    .await
    .map_err(|e| SnapshotForkError::WaitFailed(std::io::Error::other(format!("join: {e}"))))?
    .map_err(SnapshotForkError::WaitFailed)?;

    Ok(exit)
}

#[cfg(unix)]
const DEFAULT_SNAPSHOT_FORK_WAIT_TIMEOUT_SECS: u64 = 600;

#[cfg(unix)]
fn snapshot_fork_wait_timeout() -> Duration {
    std::env::var("BEAVA_SNAPSHOT_FORK_WAIT_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_SNAPSHOT_FORK_WAIT_TIMEOUT_SECS))
}

#[cfg(unix)]
fn wait_for_snapshot_child(
    pid: libc::pid_t,
    snapshot_dir: PathBuf,
    snapshot_lsn: u64,
    timeout: Duration,
) -> Result<ChildExit, std::io::Error> {
    let deadline = Instant::now() + timeout;
    loop {
        let mut status: libc::c_int = 0;
        let waited = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if waited == pid {
            return Ok(child_exit_from_status(status, &snapshot_dir, snapshot_lsn));
        }
        if waited < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                if Instant::now() >= deadline {
                    return terminate_timed_out_child(pid, timeout);
                }
                continue;
            }
            return Err(err);
        }

        if Instant::now() >= deadline {
            return terminate_timed_out_child(pid, timeout);
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn terminate_timed_out_child(
    pid: libc::pid_t,
    timeout: Duration,
) -> Result<ChildExit, std::io::Error> {
    let kill_error = if unsafe { libc::kill(pid, libc::SIGKILL) } == 0 {
        None
    } else {
        Some(std::io::Error::last_os_error())
    };
    let reap_grace = Duration::from_secs(1);
    let reap_deadline = Instant::now() + reap_grace;
    let reaped = reap_child_until(pid, reap_deadline)?;

    let mut message = format!(
        "child exceeded fork snapshot wait timeout of {}s and was killed",
        timeout.as_secs()
    );
    if let Some(err) = kill_error {
        message.push_str(&format!("; SIGKILL failed: {err}"));
    }

    match reaped {
        Some(status) => {
            if libc::WIFSIGNALED(status) {
                message.push_str(&format!("; reaped signal {}", libc::WTERMSIG(status)));
            } else if libc::WIFEXITED(status) {
                message.push_str(&format!(
                    "; reaped exit status {}",
                    libc::WEXITSTATUS(status)
                ));
            } else {
                message.push_str(&format!("; reaped status {status}"));
            }
            Ok(ChildExit::Failure { code: -1, message })
        }
        None => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!(
                "child exceeded fork snapshot wait timeout of {}s, but was not reaped within {}ms",
                timeout.as_secs(),
                reap_grace.as_millis()
            ),
        )),
    }
}

#[cfg(unix)]
fn reap_child_until(
    pid: libc::pid_t,
    deadline: Instant,
) -> Result<Option<libc::c_int>, std::io::Error> {
    loop {
        let mut status: libc::c_int = 0;
        let waited = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if waited == pid {
            return Ok(Some(status));
        }
        if waited < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                continue;
            }
            return Err(err);
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn child_exit_from_status(
    status: libc::c_int,
    snapshot_dir: &Path,
    snapshot_lsn: u64,
) -> ChildExit {
    if libc::WIFEXITED(status) {
        let code = libc::WEXITSTATUS(status);
        if code == 0 {
            match read_stats_sidecar(snapshot_dir, snapshot_lsn) {
                Some(write_stats) => ChildExit::Success {
                    snapshot_lsn,
                    write_stats: Some(write_stats),
                },
                None => ChildExit::Failure {
                    code,
                    message: format!(
                        "child exited successfully but stats sidecar was missing or corrupt for snapshot LSN {snapshot_lsn}"
                    ),
                },
            }
        } else {
            let err_path = snapshot_error_sidecar_path(snapshot_dir, snapshot_lsn);
            let message = std::fs::read_to_string(&err_path)
                .unwrap_or_else(|_| format!("child exited with code {code}"));
            let _ = std::fs::remove_file(&err_path);
            ChildExit::Failure { code, message }
        }
    } else if libc::WIFSIGNALED(status) {
        let sig = libc::WTERMSIG(status);
        ChildExit::Failure {
            code: -1,
            message: format!("child killed by signal {sig}"),
        }
    } else {
        ChildExit::Failure {
            code: -1,
            message: format!("child stopped with status {status}"),
        }
    }
}

/// Non-unix stub — fork is Linux/macOS only. Beava ships on those platforms.
#[cfg(not(unix))]
pub async fn do_snapshot_via_fork(
    _snapshot_dir: &Path,
    _legacy_snapshot_lsn: u64,
    _app_state: &AppState,
) -> Result<ChildExit, SnapshotForkError> {
    Err(SnapshotForkError::ForkFailed(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "fork-snapshot is unix-only",
    )))
}

#[cfg(not(unix))]
pub async fn do_snapshot_via_fork_with_wait_timeout(
    _snapshot_dir: &Path,
    _legacy_snapshot_lsn: u64,
    _app_state: &AppState,
    _wait_timeout: std::time::Duration,
) -> Result<ChildExit, SnapshotForkError> {
    Err(SnapshotForkError::ForkFailed(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "fork-snapshot is unix-only",
    )))
}

#[cfg(unix)]
fn snapshot_stats_sidecar_path(snapshot_dir: &Path, snapshot_lsn: u64) -> std::path::PathBuf {
    snapshot_dir.join(format!("snapshot-{snapshot_lsn:016x}.stats"))
}

#[cfg(unix)]
fn snapshot_error_sidecar_path(snapshot_dir: &Path, snapshot_lsn: u64) -> std::path::PathBuf {
    snapshot_dir.join(format!("snapshot-{snapshot_lsn:016x}.error"))
}

#[cfg(unix)]
fn snapshot_file_path(snapshot_dir: &Path, snapshot_lsn: u64) -> std::path::PathBuf {
    snapshot_dir.join(format!(
        "snapshot-{snapshot_lsn:016x}.{}",
        beava_persistence::SNAPSHOT_EXT
    ))
}

#[cfg(unix)]
fn write_stats_sidecar(
    snapshot_dir: &Path,
    snapshot_lsn: u64,
    stats: &SnapshotWriteStats,
) -> std::io::Result<()> {
    let dir_fsync_us = stats
        .dir_fsync_duration
        .map(duration_micros)
        .map(|us| us.to_string())
        .unwrap_or_else(|| "none".to_string());
    let body = format!(
        "bytes={}\nfile_fsync_us={}\ndir_fsync_us={}\n",
        stats.bytes,
        duration_micros(stats.file_fsync_duration),
        dir_fsync_us
    );
    std::fs::write(
        snapshot_stats_sidecar_path(snapshot_dir, snapshot_lsn),
        body,
    )
}

#[cfg(unix)]
fn read_stats_sidecar(snapshot_dir: &Path, snapshot_lsn: u64) -> Option<SnapshotWriteStats> {
    let path = snapshot_stats_sidecar_path(snapshot_dir, snapshot_lsn);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            return None;
        }
    };
    let _ = std::fs::remove_file(&path);

    let bytes = parse_stats_field(&raw, "bytes")?.parse::<u64>().ok()?;
    let file_fsync_us = parse_stats_field(&raw, "file_fsync_us")?
        .parse::<u64>()
        .ok()?;
    let dir_fsync_duration = match parse_stats_field(&raw, "dir_fsync_us")? {
        "none" => None,
        value => Some(Duration::from_micros(value.parse::<u64>().ok()?)),
    };

    Some(SnapshotWriteStats {
        path: snapshot_file_path(snapshot_dir, snapshot_lsn),
        bytes,
        file_fsync_duration: Duration::from_micros(file_fsync_us),
        dir_fsync_duration,
    })
}

#[cfg(unix)]
fn parse_stats_field<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    raw.lines()
        .find_map(|line| line.strip_prefix(&prefix).map(str::trim))
}

#[cfg(unix)]
fn duration_micros(duration: Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}

/// Child-side fatal: write the error message to a sidecar file and `_exit(1)`.
/// Never returns. Marked `-> !` so callers don't need to handle a return.
#[cfg(unix)]
fn child_fail(snapshot_dir: &Path, snapshot_lsn: u64, msg: &str) -> ! {
    let err_path = snapshot_error_sidecar_path(snapshot_dir, snapshot_lsn);
    // Best-effort: ignore write failure. The parent will fall back to a
    // generic "child exited non-zero" message.
    let _ = std::fs::write(&err_path, msg);
    unsafe { libc::_exit(1) }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_snapshot_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "beava-snapshot-fork-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn wait_for_snapshot_child_timeout_kills_and_reaps_child() {
        let dir = temp_snapshot_dir("timeout-reap");
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed: {}", std::io::Error::last_os_error());

        if pid == 0 {
            loop {
                unsafe {
                    libc::pause();
                }
            }
        }

        let started = Instant::now();
        let exit = wait_for_snapshot_child(pid, dir.clone(), 42, Duration::from_millis(20))
            .expect("timeout path must return a bounded failure");
        let elapsed = started.elapsed();

        match exit {
            ChildExit::Failure { code, message } => {
                assert_eq!(code, -1);
                assert!(
                    message.contains("exceeded fork snapshot wait timeout"),
                    "{message}"
                );
                assert!(message.contains("reaped signal"), "{message}");
            }
            other => panic!("expected timeout failure, got {other:?}"),
        }
        assert!(
            elapsed < Duration::from_secs(2),
            "timeout/kill/reap path took {elapsed:?}"
        );

        let mut status: libc::c_int = 0;
        let waited = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        assert_eq!(waited, -1, "child should already be reaped");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stats_sidecar_roundtrip_reports_fsync_and_removes_file() {
        let dir = temp_snapshot_dir("stats-sidecar");
        let snapshot_lsn = 77;
        let stats = SnapshotWriteStats {
            path: snapshot_file_path(&dir, snapshot_lsn),
            bytes: 1234,
            file_fsync_duration: Duration::from_micros(456),
            dir_fsync_duration: Some(Duration::from_micros(789)),
        };

        write_stats_sidecar(&dir, snapshot_lsn, &stats).expect("write stats sidecar");
        let roundtrip = read_stats_sidecar(&dir, snapshot_lsn).expect("read stats sidecar");

        assert_eq!(roundtrip.bytes, 1234);
        assert_eq!(roundtrip.file_fsync_duration, Duration::from_micros(456));
        assert_eq!(
            roundtrip.dir_fsync_duration,
            Some(Duration::from_micros(789))
        );
        assert!(
            !snapshot_stats_sidecar_path(&dir, snapshot_lsn).exists(),
            "parent must remove stats sidecar after reading it"
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
