//! Snapshot writer + reader + retention helpers.
//!
//! Atomic write protocol:
//! 1. Open `snapshot-{lsn:016x}.tmp` (truncate-create).
//! 2. Compute body crc32c, build SnapshotHeader, write header bytes + body bytes.
//! 3. `file.sync_all()` (fsync).
//! 4. `fs::rename(tmp, final)` where final = `snapshot-{lsn:016x}.bvs`.
//! 5. (unix) Open parent dir + `sync_all()` for rename durability.
//!
//! Crash between 1-3 leaves the prior snapshot intact (tmp is overwritten
//! on next attempt). Crash between 4-5 may leave the rename unflushed in
//! the directory entry on some filesystems; recovery tolerates a
//! missing-but-partially-renamed entry.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::{PersistError, SnapshotError};
use crate::snapshot_header::{
    SnapshotHeader, SNAPSHOT_EXT, SNAPSHOT_FORMAT_VERSION, SNAPSHOT_HEADER_SIZE,
};
use crate::Lsn;

/// Atomic snapshot writer.
///
/// `SnapshotWriter::write` is the production path. The unit-struct shape
/// also supports memory-mode callers via [`SnapshotWriter::no_op`] +
/// [`SnapshotWriter::commit_no_op`], which round-trip `Ok(())` without any
/// file I/O. In memory mode the snapshot scheduler is not spawned at all
/// (see the persistence branch in `ServerV18::bind_with_config`); these
/// methods exist as an explicit affordance for tests and embed callers
/// that still hold a writer handle.
pub struct SnapshotWriter;

/// Best-effort instrumentation returned by [`SnapshotWriter::write_with_stats`].
#[derive(Debug, Clone)]
pub struct SnapshotWriteStats {
    pub path: PathBuf,
    /// Header bytes plus body bytes written to the committed snapshot file.
    pub bytes: u64,
    /// Required file fsync duration.
    pub file_fsync_duration: Duration,
    /// Best-effort parent-directory fsync duration. `None` when unsupported or
    /// when opening the directory failed.
    pub dir_fsync_duration: Option<Duration>,
}

impl SnapshotWriteStats {
    pub fn total_fsync_duration(&self) -> Duration {
        self.file_fsync_duration + self.dir_fsync_duration.unwrap_or_default()
    }
}

impl SnapshotWriter {
    /// Construct a no-op snapshot writer for `Persistence::Memory` mode.
    pub fn no_op() -> Self {
        SnapshotWriter
    }

    /// No-op commit for memory-mode callers — returns `Ok(())` immediately,
    /// performs zero file I/O.
    pub fn commit_no_op(&self) -> Result<(), PersistError> {
        Ok(())
    }

    /// Atomically write a snapshot file and return its final path. See module
    /// doc for the protocol.
    pub fn write(
        dir: &Path,
        snapshot_lsn: Lsn,
        registry_version: u64,
        body: &[u8],
    ) -> Result<PathBuf, PersistError> {
        Self::write_with_stats(dir, snapshot_lsn, registry_version, body).map(|stats| stats.path)
    }

    /// Atomically write a snapshot file and return write/fsync stats.
    pub fn write_with_stats(
        dir: &Path,
        snapshot_lsn: Lsn,
        registry_version: u64,
        body: &[u8],
    ) -> Result<SnapshotWriteStats, PersistError> {
        std::fs::create_dir_all(dir).map_err(PersistError::Io)?;
        let tmp_path = dir.join(format!("snapshot-{snapshot_lsn:016x}.tmp"));
        let final_path = dir.join(format!("snapshot-{snapshot_lsn:016x}.{SNAPSHOT_EXT}"));

        let body_crc32c = crc32c::crc32c(body);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let header = SnapshotHeader {
            format_version: SNAPSHOT_FORMAT_VERSION,
            flags: 0,
            created_at_ms: now_ms,
            snapshot_lsn,
            registry_version,
            body_len: body.len() as u64,
            body_crc32c,
        };
        let header_bytes = header.encode();
        let bytes = header_bytes.len() as u64 + body.len() as u64;

        let mut f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)
            .map_err(PersistError::Io)?;
        f.write_all(&header_bytes).map_err(PersistError::Io)?;
        f.write_all(body).map_err(PersistError::Io)?;

        let file_fsync_started = Instant::now();
        f.sync_all().map_err(PersistError::Io)?;
        let file_fsync_duration = file_fsync_started.elapsed();
        drop(f);

        std::fs::rename(&tmp_path, &final_path).map_err(PersistError::Io)?;

        #[cfg(unix)]
        let mut dir_fsync_duration = None;
        #[cfg(not(unix))]
        let dir_fsync_duration = None;
        // Best-effort parent-directory fsync makes the rename durable on
        // filesystems that don't journal directory entries with the file.
        #[cfg(unix)]
        {
            if let Ok(d) = File::open(dir) {
                let dir_fsync_started = Instant::now();
                let _ = d.sync_all();
                dir_fsync_duration = Some(dir_fsync_started.elapsed());
            }
        }

        Ok(SnapshotWriteStats {
            path: final_path,
            bytes,
            file_fsync_duration,
            dir_fsync_duration,
        })
    }
}

/// Snapshot reader — verifies magic, version, header CRC, body length, body CRC.
pub struct SnapshotReader;

impl SnapshotReader {
    /// Open + fully verify + return (header, body bytes).
    pub fn open(path: &Path) -> Result<(SnapshotHeader, Vec<u8>), PersistError> {
        let mut f = File::open(path).map_err(PersistError::Io)?;

        let mut header_bytes = [0u8; SNAPSHOT_HEADER_SIZE];
        f.read_exact(&mut header_bytes).map_err(PersistError::Io)?;
        let header = SnapshotHeader::decode(&header_bytes)?;

        // Hand-rolled read loop so a short read surfaces as `Truncated`
        // (with the actual got vs expected counts) rather than a generic
        // `UnexpectedEof`.
        let mut body = vec![0u8; header.body_len as usize];
        let mut read_total = 0usize;
        while read_total < body.len() {
            match f.read(&mut body[read_total..]) {
                Ok(0) => {
                    return Err(PersistError::Snapshot(SnapshotError::Truncated {
                        expected: header.body_len,
                        got: read_total as u64,
                    }));
                }
                Ok(n) => read_total += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(PersistError::Io(e)),
            }
        }

        let computed = crc32c::crc32c(&body);
        if computed != header.body_crc32c {
            return Err(PersistError::Snapshot(SnapshotError::BodyCrcMismatch {
                expected: header.body_crc32c,
                got: computed,
            }));
        }

        Ok((header, body))
    }
}

/// List committed snapshots in `dir`, sorted by snapshot_lsn DESCENDING.
///
/// Filename pattern: `snapshot-{lsn:016x}.bvs`. Files that don't match the
/// pattern are silently skipped. A missing directory returns an empty list.
pub fn list_snapshots(dir: &Path) -> Result<Vec<(Lsn, PathBuf)>, PersistError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out: Vec<(Lsn, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(PersistError::Io)? {
        let entry = entry.map_err(PersistError::Io)?;
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        if ext != SNAPSHOT_EXT {
            continue;
        }
        let Some(hex) = stem.strip_prefix("snapshot-") else {
            continue;
        };
        let Ok(lsn) = u64::from_str_radix(hex, 16) else {
            continue;
        };
        out.push((lsn, path));
    }
    out.sort_by_key(|(lsn, _)| std::cmp::Reverse(*lsn));
    Ok(out)
}

/// Delete all but the `keep` highest-LSN snapshots in `dir`. Returns the
/// number of files removed.
pub fn prune_old_snapshots(dir: &Path, keep: usize) -> Result<u32, PersistError> {
    let snaps = list_snapshots(dir)?;
    let mut removed = 0u32;
    for (_, path) in snaps.into_iter().skip(keep) {
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(PersistError::Io(e)),
        }
    }
    Ok(removed)
}
