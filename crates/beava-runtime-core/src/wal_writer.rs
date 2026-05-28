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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// Stable WAL path. The writer owns all truncate/compact operations.
    wal_path: PathBuf,
    /// Shared buffer ring — writer pops sealed buffers from here.
    ring: Arc<WalBufferRing>,
    /// Shared LSN watermarks — writer advances written + synced here.
    lsn: Arc<WalLsn>,
    /// How long to sleep between fsync ticks (milliseconds).
    tick_ms: u64,
    /// Set to `true` by `shutdown()` to ask the writer thread to drain and exit.
    shutdown: Arc<AtomicBool>,
    /// Snapshot-covered LSN requested by the snapshot task. The writer is the
    /// only thread that acts on this request because it owns the append file.
    reclaim: WalReclaimHandle,
}

/// Handle used by snapshot/checkpoint code to request hand-rolled WAL
/// compaction. Requests are monotone; the writer thread observes them after a
/// flush+fsync boundary and safely rewrites the WAL tail.
#[derive(Clone, Debug)]
pub struct WalReclaimHandle {
    requested_lsn: Arc<AtomicU64>,
}

impl WalReclaimHandle {
    fn new() -> Self {
        Self {
            requested_lsn: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Request reclamation of WAL records covered by `covered_lsn`.
    pub fn request_reclaim_up_to(&self, covered_lsn: u64) {
        self.requested_lsn.fetch_max(covered_lsn, Ordering::AcqRel);
    }

    fn requested_lsn(&self) -> u64 {
        self.requested_lsn.load(Ordering::Acquire)
    }
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
        repair_wal_file_tail(&wal_path)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;

        Ok(Self {
            file,
            wal_path,
            ring,
            lsn,
            tick_ms,
            shutdown: Arc::new(AtomicBool::new(false)),
            reclaim: WalReclaimHandle::new(),
        })
    }

    /// Return a clone of the shutdown flag so the caller can request a drain.
    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutdown)
    }

    /// Return a handle for snapshot/checkpoint code to request WAL
    /// reclamation. The writer performs the actual compaction on its own
    /// thread after flushing the ring.
    pub fn reclaim_handle(&self) -> WalReclaimHandle {
        self.reclaim.clone()
    }

    /// Start the writer + fsync loop in a dedicated `std::thread`.
    ///
    /// The returned `JoinHandle` can be awaited for clean shutdown.
    pub fn spawn(self) -> JoinHandle<()> {
        let WalWriter {
            mut file,
            wal_path,
            ring,
            lsn,
            tick_ms,
            shutdown,
            reclaim,
        } = self;
        let tick = Duration::from_millis(tick_ms);

        std::thread::Builder::new()
            .name("beava-wal-writer".to_owned())
            .spawn(move || {
                run_writer_loop(&mut file, &wal_path, &ring, &lsn, tick, &shutdown, &reclaim);
            })
            .expect("failed to spawn WAL writer thread")
    }
}

fn run_writer_loop(
    file: &mut File,
    wal_path: &Path,
    ring: &WalBufferRing,
    lsn: &WalLsn,
    tick: Duration,
    shutdown: &AtomicBool,
    reclaim: &WalReclaimHandle,
) {
    let mut reclaimed_lsn = 0u64;
    loop {
        std::thread::sleep(tick);

        // 1. Force-seal the active buffer (even if not full) so pending
        //    records are bounded by tick latency.
        ring.seal_active();

        // 2. Drain all sealed buffers: write → fsync → free.
        flush_sealed_buffers(file, ring, lsn);
        maybe_reclaim_wal_file(file, wal_path, lsn, reclaim, &mut reclaimed_lsn);

        // 3. Check shutdown after draining.
        if shutdown.load(Ordering::Acquire) {
            // Final drain + fsync on shutdown.
            ring.seal_active();
            flush_sealed_buffers(file, ring, lsn);
            maybe_reclaim_wal_file(file, wal_path, lsn, reclaim, &mut reclaimed_lsn);
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

fn maybe_reclaim_wal_file(
    file: &mut File,
    wal_path: &Path,
    lsn: &WalLsn,
    reclaim: &WalReclaimHandle,
    reclaimed_lsn: &mut u64,
) {
    let requested = reclaim.requested_lsn();
    if requested == 0 || requested <= *reclaimed_lsn {
        return;
    }
    let synced_lsn = lsn.synced();

    match compact_wal_file(file, wal_path, requested) {
        Ok(stats) => {
            *reclaimed_lsn = requested;
            tracing::info!(
                target: "beava.wal",
                kind = "wal.handrolled_reclaimed",
                covered_lsn = requested,
                data_plane_synced_lsn = synced_lsn,
                before_bytes = stats.before_bytes,
                after_bytes = stats.after_bytes,
                removed_records = stats.removed_records,
                retained_records = stats.retained_records,
                "hand-rolled WAL reclaimed after durable snapshot"
            );
        }
        Err(e) => {
            tracing::warn!(
                target: "beava.wal",
                kind = "wal.handrolled_reclaim_failed",
                covered_lsn = requested,
                error = %e,
                "hand-rolled WAL reclaim skipped"
            );
        }
    }
}

#[derive(Debug)]
struct WalCompactStats {
    before_bytes: u64,
    after_bytes: u64,
    removed_records: usize,
    retained_records: usize,
}

#[derive(Debug)]
struct ParsedWalRecord {
    lsn: u64,
    start: usize,
    end: usize,
    version: u8,
}

fn compact_wal_file(
    open_append_file: &mut File,
    wal_path: &Path,
    covered_lsn: u64,
) -> std::io::Result<WalCompactStats> {
    open_append_file.sync_all()?;

    let data = std::fs::read(wal_path)?;
    let before_bytes = data.len() as u64;
    if data.is_empty() {
        return Ok(WalCompactStats {
            before_bytes,
            after_bytes: 0,
            removed_records: 0,
            retained_records: 0,
        });
    }

    let parsed = parse_wal_record_bounds_with_prefix(&data)?;
    let records = parsed.records;
    let mut retained = Vec::new();
    let mut removed_records = 0usize;
    let mut retained_records = 0usize;
    for rec in records {
        if rec.lsn <= covered_lsn {
            removed_records += 1;
            continue;
        }
        if rec.version != 0x03 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "cannot compact retained legacy v2 WAL records without assigned LSNs",
            ));
        }
        retained.extend_from_slice(&data[rec.start..rec.end]);
        retained_records += 1;
    }

    if removed_records == 0 && parsed.valid_prefix_len == data.len() {
        sync_parent_dir(wal_path)?;
        return Ok(WalCompactStats {
            before_bytes,
            after_bytes: before_bytes,
            removed_records,
            retained_records,
        });
    }

    let tmp_path = wal_path.with_extension("wal.compact.tmp");
    match std::fs::remove_file(&tmp_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    {
        let mut tmp = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        tmp.write_all(&retained)?;
        tmp.sync_all()?;
    }

    // Open the replacement WAL before the rename. After rename succeeds there
    // must be no fallible reopen step, otherwise a transient fd/open failure
    // could leave the writer appending to the old unlinked inode.
    let new_append_file = OpenOptions::new().append(true).open(&tmp_path)?;

    std::fs::rename(&tmp_path, wal_path)?;

    *open_append_file = new_append_file;
    sync_parent_dir(wal_path)?;

    Ok(WalCompactStats {
        before_bytes,
        after_bytes: retained.len() as u64,
        removed_records,
        retained_records,
    })
}

#[cfg(test)]
fn parse_wal_record_bounds(data: &[u8]) -> std::io::Result<Vec<ParsedWalRecord>> {
    Ok(parse_wal_record_bounds_with_prefix(data)?.records)
}

#[derive(Debug)]
struct ParsedWalRecords {
    records: Vec<ParsedWalRecord>,
    valid_prefix_len: usize,
}

fn parse_wal_record_bounds_with_prefix(data: &[u8]) -> std::io::Result<ParsedWalRecords> {
    let mut records = Vec::new();
    let mut pos = 0usize;
    let mut valid_prefix_len = 0usize;

    while pos < data.len() {
        let start = pos;
        let version = data[pos];
        if version != 0x02 && version != 0x03 {
            break;
        }
        pos += 1;

        let assigned_lsn = if version == 0x03 {
            if pos + 8 > data.len() {
                break;
            }
            let lsn = u64::from_be_bytes(data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            Some(lsn)
        } else {
            None
        };

        if pos + 15 > data.len() {
            break;
        }
        pos += 1; // body_format
        pos += 4; // registry version
        pos += 8; // event time

        let name_len = u16::from_be_bytes(data[pos..pos + 2].try_into().unwrap()) as usize;
        pos += 2;

        if pos + name_len + 4 > data.len() {
            break;
        }
        if std::str::from_utf8(&data[pos..pos + name_len]).is_err() {
            break;
        }
        pos += name_len;

        let body_len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;

        if pos + body_len > data.len() {
            break;
        }
        pos += body_len;

        let end = pos;
        records.push(ParsedWalRecord {
            lsn: assigned_lsn.unwrap_or(end as u64),
            start,
            end,
            version,
        });
        valid_prefix_len = end;
    }

    Ok(ParsedWalRecords {
        records,
        valid_prefix_len,
    })
}

fn repair_wal_file_tail(wal_path: &Path) -> std::io::Result<()> {
    let data = match std::fs::read(wal_path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let parsed = parse_wal_record_bounds_with_prefix(&data)?;
    if parsed.valid_prefix_len == data.len() {
        return Ok(());
    }

    let file = OpenOptions::new().write(true).open(wal_path)?;
    file.set_len(parsed.valid_prefix_len as u64)?;
    file.sync_all()?;
    sync_parent_dir(wal_path)?;
    tracing::warn!(
        target: "beava.wal",
        kind = "wal.handrolled_tail_repaired",
        path = %wal_path.display(),
        before_bytes = data.len(),
        after_bytes = parsed.valid_prefix_len,
        "repaired hand-rolled WAL by truncating invalid tail before append"
    );
    Ok(())
}

fn sync_parent_dir(path: &Path) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent: {}", path.display()),
        )
    })?;
    File::open(parent)?.sync_all()
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

        // Known Linux NFS/SMB/FUSE f_type magic numbers.
        const NFS_SUPER_MAGIC: i64 = 0x6969;
        const SMB_SUPER_MAGIC: i64 = 0x517B;
        const CIFS_MAGIC_NUMBER: i64 = 0xFF534D42;
        const FUSE_SUPER_MAGIC: i64 = 0x65735546;

        matches!(
            stat.f_type,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "beava-wal-writer-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn encode_v3(buf: &mut Vec<u8>, lsn: u64, event_name: &str, body: &[u8]) {
        buf.push(0x03);
        buf.extend_from_slice(&lsn.to_be_bytes());
        buf.push(0x02); // JSON
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&123u64.to_be_bytes());
        let name_bytes = event_name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
        buf.extend_from_slice(name_bytes);
        buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
        buf.extend_from_slice(body);
    }

    #[test]
    fn compact_wal_file_removes_snapshot_covered_v3_records() {
        let dir = temp_dir("compact");
        let path = dir.join("wal-0000000000000000.wal");
        let mut bytes = Vec::new();
        encode_v3(&mut bytes, 10, "Txn", br#"{"user_id":"a"}"#);
        encode_v3(&mut bytes, 20, "Txn", br#"{"user_id":"b"}"#);
        encode_v3(&mut bytes, 30, "Txn", br#"{"user_id":"c"}"#);
        std::fs::write(&path, &bytes).unwrap();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let stats = compact_wal_file(&mut file, &path, 20).unwrap();
        let compacted = std::fs::read(&path).unwrap();
        let records = parse_wal_record_bounds(&compacted).unwrap();

        assert_eq!(stats.removed_records, 2);
        assert_eq!(stats.retained_records, 1);
        assert!(stats.after_bytes < stats.before_bytes);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].lsn, 30);

        file.write_all(b"tail").unwrap();
        file.sync_all().unwrap();
        assert!(
            std::fs::metadata(&path).unwrap().len() > stats.after_bytes,
            "writer must keep appending to the compacted WAL path"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn compact_wal_file_skips_when_no_records_are_covered() {
        let dir = temp_dir("noop");
        let path = dir.join("wal-0000000000000000.wal");
        let mut bytes = Vec::new();
        encode_v3(&mut bytes, 50, "Txn", br#"{"user_id":"a"}"#);
        std::fs::write(&path, &bytes).unwrap();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let stats = compact_wal_file(&mut file, &path, 20).unwrap();

        assert_eq!(stats.removed_records, 0);
        assert_eq!(stats.retained_records, 1);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn compact_wal_file_ignores_truncated_tail_like_recovery() {
        let dir = temp_dir("truncated-tail");
        let path = dir.join("wal-0000000000000000.wal");
        let mut bytes = Vec::new();
        encode_v3(&mut bytes, 10, "Txn", br#"{"user_id":"covered"}"#);
        encode_v3(&mut bytes, 30, "Txn", br#"{"user_id":"retained"}"#);
        bytes.extend_from_slice(&[0x03, 0, 0, 0]); // torn v3 header
        std::fs::write(&path, &bytes).unwrap();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let stats = compact_wal_file(&mut file, &path, 10).unwrap();
        let compacted = std::fs::read(&path).unwrap();
        let records = parse_wal_record_bounds(&compacted).unwrap();

        assert_eq!(stats.removed_records, 1);
        assert_eq!(stats.retained_records, 1);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].lsn, 30);

        file.write_all(b"tail").unwrap();
        file.sync_all().unwrap();
        assert!(
            std::fs::metadata(&path).unwrap().len() > stats.after_bytes,
            "writer must keep appending to the renamed compacted WAL"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn compact_wal_file_repairs_truncated_tail_even_without_covered_records() {
        let dir = temp_dir("truncated-tail-no-covered");
        let path = dir.join("wal-0000000000000000.wal");
        let mut bytes = Vec::new();
        encode_v3(&mut bytes, 30, "Txn", br#"{"user_id":"retained"}"#);
        let valid_len = bytes.len() as u64;
        bytes.extend_from_slice(&[0x03, 0, 0, 0]); // torn v3 header
        std::fs::write(&path, &bytes).unwrap();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let stats = compact_wal_file(&mut file, &path, 10).unwrap();
        let compacted = std::fs::read(&path).unwrap();
        let records = parse_wal_record_bounds(&compacted).unwrap();

        assert_eq!(stats.removed_records, 0);
        assert_eq!(stats.retained_records, 1);
        assert_eq!(stats.after_bytes, valid_len);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].lsn, 30);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn wal_writer_new_repairs_invalid_tail_before_append() {
        let dir = temp_dir("startup-repair");
        let path = dir.join("wal-0000000000000000.wal");
        let mut bytes = Vec::new();
        encode_v3(&mut bytes, 10, "Txn", br#"{"user_id":"covered"}"#);
        let valid_len = bytes.len();
        bytes.extend_from_slice(b"stale-garbage-tail");
        std::fs::write(&path, &bytes).unwrap();

        let ring = Arc::new(WalBufferRing::new(2, 4096, Arc::new(WalLsn::new())));
        let lsn = Arc::new(WalLsn::new());
        let mut writer = WalWriter::new(&dir, ring, lsn, 1).expect("WalWriter::new repairs tail");

        assert_eq!(std::fs::metadata(&path).unwrap().len(), valid_len as u64);

        let mut appended = Vec::new();
        encode_v3(&mut appended, 20, "Txn", br#"{"user_id":"retained"}"#);
        writer.file.write_all(&appended).unwrap();
        writer.file.sync_all().unwrap();

        let repaired = std::fs::read(&path).unwrap();
        let records = parse_wal_record_bounds(&repaired).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].lsn, 10);
        assert_eq!(records[1].lsn, 20);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn compact_wal_file_replaces_stale_tmp_from_prior_crash() {
        let dir = temp_dir("stale-tmp");
        let path = dir.join("wal-0000000000000000.wal");
        let mut bytes = Vec::new();
        encode_v3(&mut bytes, 10, "Txn", br#"{"user_id":"a"}"#);
        std::fs::write(&path, &bytes).unwrap();
        std::fs::write(path.with_extension("wal.compact.tmp"), b"stale").unwrap();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let stats = compact_wal_file(&mut file, &path, 10).unwrap();

        assert_eq!(stats.removed_records, 1);
        assert_eq!(stats.after_bytes, 0);
        assert_eq!(std::fs::read(&path).unwrap(), b"");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reclaim_request_can_exceed_data_plane_synced_lsn_for_registry_lsn_gaps() {
        let dir = temp_dir("registry-gap");
        let path = dir.join("wal-0000000000000000.wal");
        let mut bytes = Vec::new();
        encode_v3(&mut bytes, 10, "Txn", br#"{"user_id":"a"}"#);
        std::fs::write(&path, &bytes).unwrap();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let lsn = WalLsn::new_at(10);
        let reclaim = WalReclaimHandle::new();
        reclaim.request_reclaim_up_to(100);
        let mut reclaimed_lsn = 0;

        maybe_reclaim_wal_file(&mut file, &path, &lsn, &reclaim, &mut reclaimed_lsn);

        assert_eq!(reclaimed_lsn, 100);
        assert_eq!(std::fs::read(&path).unwrap(), b"");

        std::fs::remove_dir_all(dir).unwrap();
    }
}
