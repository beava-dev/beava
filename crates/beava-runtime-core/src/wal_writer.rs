//! Dedicated writer + fsync thread.
//!
//! # Role
//!
//! The writer thread is the **only** thread that calls `write(fd)` and
//! `fsync(fd)` on the WAL file. The apply thread is the only thread that
//! appends to the in-memory ring buffers. These two roles never overlap.
//!
//! # Loop invariant
//!
//! Every `tick_ms` milliseconds the writer thread:
//!
//! 1. Calls `ring.seal_active()` (forces a seal even if the buffer is not
//!    full, so writes are bounded by `tick_ms` latency).
//! 2. Drains all sealed buffers from `ring.pop_sealed()`.
//! 3. For each sealed buffer: `write(fd, bytes)` → `mark_written` →
//!    `fdatasync(fd)` → `mark_synced` + Condvar notify → `return_to_free`.
//!
//! Sequential write+fsync per buffer (no pipelining in v0). If perf gates
//! show fsync latency hides write throughput, split to two threads in a
//! follow-up.
//!
//! # WAL file
//!
//! Opened with `O_WRONLY | O_CREAT | O_APPEND` (append-mode is defense-in-depth;
//! single-writer architecture means atomicity isn't strictly needed but costs
//! zero).
//!
//! # Network FS guard
//!
//! `WalWriter::new` calls `is_network_fs` on the WAL directory. If true, it
//! returns an error — O_APPEND atomicity isn't reliable on NFS/SMB/FUSE.
//!
//! # Shutdown
//!
//! The writer thread runs until `shutdown` is called (sets a flag) or the
//! `JoinHandle` is dropped. On shutdown the loop drains the queue and does a
//! final fsync before exiting.

use crate::wal_buffer::WalBufferRing;
use crate::wal_lsn::WalLsn;
use std::fs::{File, OpenOptions};
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Errors from `WalWriter::new`.
#[derive(Debug)]
pub enum WalWriterError {
    /// The WAL directory is on a network filesystem (NFS/SMB/FUSE).
    NetworkFs { path: PathBuf },
    /// IO error opening or creating the WAL file.
    Io(std::io::Error),
}

impl std::fmt::Display for WalWriterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalWriterError::NetworkFs { path } => {
                write!(f, "WAL directory {:?} is on a network filesystem (NFS/SMB/FUSE); local block storage required", path)
            }
            WalWriterError::Io(e) => write!(f, "WAL IO error: {e}"),
        }
    }
}

impl std::error::Error for WalWriterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let WalWriterError::Io(e) = self {
            Some(e)
        } else {
            None
        }
    }
}

impl From<std::io::Error> for WalWriterError {
    fn from(e: std::io::Error) -> Self {
        WalWriterError::Io(e)
    }
}

/// Dedicated WAL writer + fsync thread.
///
/// Created with `WalWriter::new(dir, ring, lsn, tick_ms)`.
/// Start the background thread with `writer.spawn()`.
pub struct WalWriter {
    /// Open WAL segment file (O_APPEND | O_WRONLY | O_CREAT).
    file: File,
    /// Shared buffer ring — writer pops sealed buffers from here.
    ring: Arc<WalBufferRing>,
    /// Shared LSN watermarks — writer advances written + synced here.
    lsn: Arc<WalLsn>,
    /// How long to sleep between fsync ticks (milliseconds).
    tick_ms: u64,
    /// Set to `true` by `shutdown()` to ask the writer thread to drain and exit.
    shutdown: Arc<AtomicBool>,
}

impl WalWriter {
    /// Create a new `WalWriter`.
    ///
    /// Opens (or creates) the WAL file at `{dir}/wal-0000000000000000.wal`.
    /// Returns `Err(NetworkFs)` if `dir` is on a network filesystem.
    pub fn new(
        dir: &Path,
        ring: Arc<WalBufferRing>,
        lsn: Arc<WalLsn>,
        tick_ms: u64,
    ) -> Result<Self, WalWriterError> {
        std::fs::create_dir_all(dir)?;

        // Guard: refuse network filesystems.
        if is_network_fs(dir) {
            return Err(WalWriterError::NetworkFs {
                path: dir.to_owned(),
            });
        }

        let wal_path = dir.join("wal-0000000000000000.wal");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;

        Ok(Self {
            file,
            ring,
            lsn,
            tick_ms,
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Return a clone of the shutdown flag so the caller can request a drain.
    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutdown)
    }

    /// Start the writer + fsync loop in a dedicated `std::thread`.
    ///
    /// The returned `JoinHandle` can be awaited for clean shutdown.
    pub fn spawn(self) -> JoinHandle<()> {
        let WalWriter {
            mut file,
            ring,
            lsn,
            tick_ms,
            shutdown,
        } = self;
        let tick = Duration::from_millis(tick_ms);

        std::thread::Builder::new()
            .name("beava-wal-writer".to_owned())
            .spawn(move || {
                run_writer_loop(&mut file, &ring, &lsn, tick, &shutdown);
            })
            .expect("failed to spawn WAL writer thread")
    }
}

fn run_writer_loop(
    file: &mut File,
    ring: &WalBufferRing,
    lsn: &WalLsn,
    tick: Duration,
    shutdown: &AtomicBool,
) {
    loop {
        std::thread::sleep(tick);

        // 1. Force-seal the active buffer (even if not full) so pending
        //    records are bounded by tick latency.
        ring.seal_active();

        // 2. Drain all sealed buffers: write → fsync → free.
        flush_sealed_buffers(file, ring, lsn);

        // 3. Check shutdown after draining.
        if shutdown.load(Ordering::Acquire) {
            // Final drain + fsync on shutdown.
            ring.seal_active();
            flush_sealed_buffers(file, ring, lsn);
            break;
        }
    }
}

/// Drain all sealed buffers from the ring, writing and fsyncing each one.
fn flush_sealed_buffers(file: &mut File, ring: &WalBufferRing, lsn: &WalLsn) {
    while let Some(buf) = ring.pop_sealed() {
        let bytes = buf.written_bytes();
        if bytes.is_empty() {
            ring.return_to_free(buf);
            continue;
        }

        // write() to kernel page cache.
        if let Err(e) = file.write_all(bytes) {
            // Log and continue — apply thread detects a broken WAL via a
            // stalled synced watermark.
            tracing::error!("WAL write error: {e}");
            ring.return_to_free(buf);
            continue;
        }

        let hi = buf.lsn_hi();
        lsn.mark_written(hi);

        // fsync (or fdatasync where available).
        if let Err(e) = sync_file(file) {
            tracing::error!("WAL fsync error: {e}");
            ring.return_to_free(buf);
            continue;
        }

        // Advance synced watermark and wake PerEvent waiters.
        lsn.mark_synced(hi);

        // Return buffer to free pool.
        ring.return_to_free(buf);
    }
}

/// Sync the file to durable storage.
///
/// Uses `fdatasync` on Linux (cheaper: skips metadata update); falls back
/// to `fsync` on macOS and other platforms.
fn sync_file(file: &mut File) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        let ret = unsafe { libc::fdatasync(fd) };
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        file.sync_data()
    }
}

/// Return `true` if `path` is on a network filesystem (NFS, SMB, FUSE, etc.).
///
/// On macOS: uses `statfs` and checks `f_fstypename` for "nfs", "smbfs",
/// "fuse", "osxfuse", "macfuse", "webdavfs".
/// On Linux: checks `f_type` against known NFS/SMB/FUSE magic numbers.
/// On other platforms: always returns `false` (conservative: allow).
pub fn is_network_fs(path: &Path) -> bool {
    #[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(p) => p,
            Err(_) => return false,
        };

        // SAFETY: statfs is a well-known POSIX syscall; we pass a valid C string
        // and a zeroed-out statfs struct.
        let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::statfs(c_path.as_ptr(), &mut stat) };
        if ret != 0 {
            return false; // Can't determine — allow.
        }

        // f_fstypename is a fixed-size null-terminated C string.
        let name_bytes = stat.f_fstypename;
        let name_len = name_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_bytes.len());
        // SAFETY: f_fstypename contains ASCII bytes from the kernel.
        let name = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                name_bytes.as_ptr() as *const u8,
                name_len,
            ))
        };

        // Known network/virtual filesystem type names on macOS/BSD.
        matches!(
            name.to_lowercase().as_str(),
            "nfs"
                | "smbfs"
                | "cifs"
                | "fuse"
                | "osxfuse"
                | "macfuse"
                | "webdavfs"
                | "afpfs"
                | "fusefs"
        )
    }

    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(p) => p,
            Err(_) => return false,
        };

        let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::statfs(c_path.as_ptr(), &mut stat) };
        if ret != 0 {
            return false;
        }

        // Known Linux NFS/SMB/FUSE f_type magic numbers. `stat.f_type` is
        // `__fsword_t` (i64) on linux-gnu and `c_ulong` (u64) on linux-musl;
        // cast to u64 so the match is portable across both.
        const NFS_SUPER_MAGIC: u64 = 0x6969;
        const SMB_SUPER_MAGIC: u64 = 0x517B;
        const CIFS_MAGIC_NUMBER: u64 = 0xFF534D42;
        const FUSE_SUPER_MAGIC: u64 = 0x65735546;

        #[allow(clippy::unnecessary_cast)]
        let f_type = stat.f_type as u64;
        matches!(
            f_type,
            NFS_SUPER_MAGIC | SMB_SUPER_MAGIC | CIFS_MAGIC_NUMBER | FUSE_SUPER_MAGIC
        )
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "linux"
    )))]
    {
        let _ = path;
        false
    }
}
