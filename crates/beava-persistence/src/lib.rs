//! Beava v2 persistence crate — WAL + snapshot (syscall-bearing, outside beava-core).
//!
//! Preserves the beava-core WASM-portability invariant: all filesystem/fsync
//! logic lives here so core stays syscall-free.

mod error;
mod fsync_worker;
mod reader;
mod record;
mod rotation;
mod segment;
mod snapshot;
mod snapshot_header;
mod writer;

/// Log Sequence Number — monotonic event identifier assigned by the WAL sink.
pub type Lsn = u64;

/// WAL record type discriminant.
///
/// v0 ships events-only (locked architectural commitment, see
/// CLAUDE.md §"Events-Only Invariant"). Surface = pushed events (0x01) +
/// registry version bumps (0x02). Bytes 0x03 / 0x04 / 0x05 (formerly
/// table / retract record types) fall through to `UnknownRecordType` in
/// `record::RecordType::from_u8`; they may return in v0.1+ if/when
/// table aggregation lands.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    Event = 0x01,
    RegistryBump = 0x02,
}

/// A single WAL record as stored on disk.
#[derive(Debug, Clone)]
pub struct WalRecord {
    pub lsn: Lsn,
    pub record_type: RecordType,
    pub payload: Vec<u8>,
}

/// Persistence mode discriminator.
///
/// `Memory` skips the WAL writer thread, snapshot writer, and boot
/// recovery — state lives in RAM only and is lost on restart. The
/// embed-mode use case (`bv.App()` with no URL) is single-process; recovery
/// has no value across a binary restart, so memory mode lets tests and
/// notebook callers avoid the on-disk overhead.
///
/// `Disk { .. }` is the production path — WAL + snapshot + recovery on boot.
#[derive(Debug, Clone)]
pub enum Persistence {
    /// In-RAM only — no WAL, no snapshot, no recovery.
    Memory,
    /// On-disk persistence — WAL + snapshot + recovery on boot.
    Disk {
        wal_dir: std::path::PathBuf,
        snapshot_dir: std::path::PathBuf,
        sync_mode: fsync_worker::SyncMode,
    },
}

impl Default for Persistence {
    fn default() -> Self {
        Persistence::Disk {
            wal_dir: std::path::PathBuf::from(".beava/wal"),
            snapshot_dir: std::path::PathBuf::from(".beava/snapshots"),
            sync_mode: fsync_worker::SyncMode::default(),
        }
    }
}

pub use error::{PersistError, SnapshotError};
pub use fsync_worker::{SyncMode, WalSink, WalSinkConfig};
pub use reader::WalReader;
pub use record::FORMAT_VERSION;
pub use snapshot::{
    list_snapshots, prune_old_snapshots, SnapshotReader, SnapshotWriteStats, SnapshotWriter,
};
pub use snapshot_header::{
    SnapshotHeader, SNAPSHOT_EXT, SNAPSHOT_FORMAT_VERSION, SNAPSHOT_HEADER_SIZE, SNAPSHOT_MAGIC,
};
pub use writer::WalWriter;
