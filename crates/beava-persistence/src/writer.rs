//! Append-only WAL segment writer.
//!
//! `append()` writes to the in-memory `BufWriter` only; durability is the
//! group-commit fsync worker's responsibility (`fsync_worker.rs`).

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::PersistError;
use crate::record::encode_record;
use crate::segment;
use crate::{Lsn, WalRecord};

/// An append-only WAL segment writer.
///
/// `open` creates a NEW segment file at `dir/wal-{start_lsn:016x}.log`.
/// If the file already exists, `open` will REUSE it iff the file is
/// exactly the segment header (no records yet) and the on-disk header
/// matches the requested `(start_lsn, registry_version)`. This handles
/// the deploy-time crashloop case where a previous boot wrote the
/// header but was killed before appending any record (see
/// `tests/writer_orphan_segment.rs`).
///
/// Any other on-disk state — non-empty segment, truncated pre-header
/// file, mismatched header, garbage magic, unsupported format version —
/// is refused. Refusals surface as `PersistError::Io(AlreadyExists)`
/// with a diagnostic message. We never silently overwrite segments
/// that may carry committed data, and we never propagate raw
/// `BadMagic` / `UnsupportedVersion` from the reuse path so the
/// boot-time error contract stays uniform regardless of corruption
/// shape.
///
/// **Single-writer invariant:** the WAL dir must be opened by only ONE
/// `WalWriter` process at a time. This holds in normal beava deploys
/// (single container, `restart: unless-stopped`, `--force-recreate`
/// serializing on container lifecycle) but is NOT enforced by the OS
/// — the reuse path opens with `read+write` and takes no `flock`. Two
/// concurrent processes pointed at the same WAL dir can both pass the
/// size==`HEADER_SIZE` check and start writing at offset 24,
/// interleaving record bytes and producing CRC corruption on read. If
/// you ever multi-process this, add an `fcntl(F_SETLK)` here first.
///
/// Subsequent segments are opened by `rotation::rotate`.
pub struct WalWriter {
    file: BufWriter<File>,
    path: PathBuf,
    bytes_since_header: u64,
}

impl WalWriter {
    pub fn open(dir: &Path, start_lsn: Lsn, registry_version: u32) -> Result<Self, PersistError> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(segment::segment_filename(start_lsn));

        // Happy path: segment doesn't exist yet — create + write header.
        match Self::create_new(&path, start_lsn, registry_version) {
            Ok(writer) => Ok(writer),
            // Recovery path: a previous boot wrote the header but was killed
            // before appending any record (e.g. SIGKILL during
            // `docker compose up --force-recreate`). The orphan segment
            // sits at the same `start_lsn` recovery determined this boot,
            // so `create_new` returns AlreadyExists. Try to reuse it.
            Err(PersistError::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Self::reuse_orphan(&path, start_lsn, registry_version)
            }
            Err(other) => Err(other),
        }
    }

    fn create_new(
        path: &Path,
        start_lsn: Lsn,
        registry_version: u32,
    ) -> Result<Self, PersistError> {
        let file = OpenOptions::new().create_new(true).write(true).open(path)?;
        let mut bw = BufWriter::new(file);
        segment::write_header(&mut bw, start_lsn, registry_version)?;
        // Flush the header before any append so a reader opened concurrently
        // with the first record sees a complete segment header.
        bw.flush()?;
        Ok(Self {
            file: bw,
            path: path.to_owned(),
            bytes_since_header: 0,
        })
    }

    /// Reuse an existing segment file iff it is exactly the header (no
    /// records yet) and the header matches the requested
    /// `(start_lsn, registry_version)`. Anything else surfaces a
    /// structured error — we will NOT overwrite a segment whose
    /// contents we can't verify.
    fn reuse_orphan(
        path: &Path,
        start_lsn: Lsn,
        registry_version: u32,
    ) -> Result<Self, PersistError> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let len = file.metadata()?.len();
        if len != segment::HEADER_SIZE {
            // Either committed records (len > HEADER_SIZE) or a torn
            // pre-header file (len < HEADER_SIZE). Both are out of scope
            // for orphan-reuse — refuse so the operator can investigate.
            // We surface AlreadyExists (the original create_new error)
            // because the file existed and we declined to clobber it;
            // adding a dedicated variant is a separate concern.
            return Err(PersistError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "WAL segment {} exists with {len} bytes (expected exactly the {} byte header — \
                     the orphan-segment recovery path only handles header-only files; this segment \
                     either contains committed records or is a torn pre-header write. Refusing to \
                     overwrite.)",
                    path.display(),
                    segment::HEADER_SIZE,
                ),
            )));
        }

        // Header-sized file: validate it before reuse. If magic / version /
        // start_lsn / registry_version don't match the request, refuse.
        // We wrap `BadMagic` / `UnsupportedVersion` from `read_header` into
        // an `AlreadyExists` Io error so the boot-time error contract is
        // uniform: ANY orphan-segment refusal (size mismatch, header
        // mismatch, garbage bytes, future format version) surfaces as
        // `WAL segment exists but can't be reused, refusing to overwrite`.
        // The original error variant is included in the message for
        // operator forensics.
        file.seek(SeekFrom::Start(0))?;
        let (existing_start_lsn, existing_registry_version) = match segment::read_header(&mut file)
        {
            Ok(parsed) => parsed,
            Err(header_err) => {
                return Err(PersistError::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "WAL segment {} exists but its header is unparseable ({header_err}). \
                         Refusing to overwrite — investigate filesystem corruption or a \
                         foreign file at this path before manually clearing.",
                        path.display(),
                    ),
                )));
            }
        };
        if existing_start_lsn != start_lsn || existing_registry_version != registry_version {
            return Err(PersistError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "WAL segment {} header records (start_lsn={existing_start_lsn}, \
                     registry_version={existing_registry_version}) but boot is asking for \
                     (start_lsn={start_lsn}, registry_version={registry_version}). Recovery state \
                     has diverged from the orphan; refusing to reuse.",
                    path.display(),
                ),
            )));
        }

        // Validated header-only orphan. Position at end-of-header for
        // the first append; bytes_since_header starts at 0 (matches the
        // semantics of a freshly-created segment).
        file.seek(SeekFrom::End(0))?;
        Ok(Self {
            file: BufWriter::new(file),
            path: path.to_owned(),
            bytes_since_header: 0,
        })
    }

    /// Append a record. No fsync. Caller (the group-commit fsync worker)
    /// invokes `sync_data` to make the write durable.
    pub fn append(&mut self, record: &WalRecord) -> Result<(), PersistError> {
        let mut buf = Vec::with_capacity(16 + record.payload.len());
        encode_record(record, &mut buf);
        self.file.write_all(&buf)?;
        self.bytes_since_header += buf.len() as u64;
        Ok(())
    }

    /// Bytes appended since the segment was opened (excludes header).
    pub fn bytes_written(&self) -> u64 {
        self.bytes_since_header
    }

    pub fn current_path(&self) -> &Path {
        &self.path
    }

    /// Flush the buffered writer AND `sync_data()` the underlying file.
    pub fn sync_data(&mut self) -> std::io::Result<()> {
        self.file.flush()?;
        self.file.get_mut().sync_data()
    }

    /// Flush the in-memory buffer to the OS page cache without invoking the
    /// blocking `sync_data()` syscall. Pair with [`Self::try_clone_file`] +
    /// `spawn_blocking` to run the durability syscall off the runtime.
    pub fn flush_buffer(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }

    /// Produce an OS-level clone of the underlying file handle suitable for
    /// being moved into `tokio::task::spawn_blocking` to call `sync_data()`
    /// off the runtime thread. The clone shares the same kernel file
    /// description, so a fsync on the clone durably persists bytes
    /// previously flushed via [`Self::flush_buffer`].
    pub fn try_clone_file(&self) -> std::io::Result<File> {
        self.file.get_ref().try_clone()
    }
}

impl Drop for WalWriter {
    fn drop(&mut self) {
        // Best-effort flush on drop so read-after-write tests don't need
        // to explicitly sync. Does NOT sync_data — durability is the fsync
        // worker's responsibility.
        let _ = self.file.flush();
    }
}
