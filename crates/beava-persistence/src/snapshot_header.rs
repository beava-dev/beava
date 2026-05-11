//! Snapshot header — fixed-size 52-byte little-endian framed header with
//! magic + version + metadata + body-CRC + self-CRC.
//!
//! Layout (LE):
//! ```text
//! offset  size  field
//! 0       8     magic (b"BEAVASNP")
//! 8       2     format_version: u16
//! 10      2     flags: u16
//! 12      8     created_at_ms: i64
//! 20      8     snapshot_lsn: u64
//! 28      8     registry_version: u64
//! 36      8     body_len: u64
//! 44      4     body_crc32c: u32     (crc32c of the body_len bytes that follow)
//! 48      4     header_crc32c: u32   (crc32c of bytes [0..48])
//! ```

use crate::error::SnapshotError;
use crate::Lsn;

/// Magic bytes at the start of every snapshot header.
pub const SNAPSHOT_MAGIC: [u8; 8] = *b"BEAVASNP";
/// Snapshot format version emitted by this build.
///
/// v0 ships at version=1 uniformly across WAL / snapshot / wire (events-only
/// invariant — see CLAUDE.md §"Events-Only Invariant"). Pre-v0 dev snapshots
/// carrying `v=2` fail with `UnsupportedVersion(2)` on open; there is no
/// migration shim — operators must clear `.beava/snapshots` before booting
/// the new binary.
pub const SNAPSHOT_FORMAT_VERSION: u16 = 1;
/// Fixed serialized size of `SnapshotHeader`.
pub const SNAPSHOT_HEADER_SIZE: usize = 52;
/// File extension used for committed snapshots.
pub const SNAPSHOT_EXT: &str = "bvs";

/// Snapshot header (in-memory form).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotHeader {
    pub format_version: u16,
    pub flags: u16,
    pub created_at_ms: i64,
    pub snapshot_lsn: Lsn,
    pub registry_version: u64,
    pub body_len: u64,
    pub body_crc32c: u32,
}

impl SnapshotHeader {
    /// Encode the header into its 52-byte canonical form.
    ///
    /// Caller is responsible for `body_crc32c` being the crc32c of the body
    /// that will be written immediately after these 52 bytes.
    pub fn encode(&self) -> [u8; SNAPSHOT_HEADER_SIZE] {
        let mut buf = [0u8; SNAPSHOT_HEADER_SIZE];
        buf[0..8].copy_from_slice(&SNAPSHOT_MAGIC);
        buf[8..10].copy_from_slice(&self.format_version.to_le_bytes());
        buf[10..12].copy_from_slice(&self.flags.to_le_bytes());
        buf[12..20].copy_from_slice(&self.created_at_ms.to_le_bytes());
        buf[20..28].copy_from_slice(&self.snapshot_lsn.to_le_bytes());
        buf[28..36].copy_from_slice(&self.registry_version.to_le_bytes());
        buf[36..44].copy_from_slice(&self.body_len.to_le_bytes());
        buf[44..48].copy_from_slice(&self.body_crc32c.to_le_bytes());
        let header_crc = crc32c::crc32c(&buf[0..48]);
        buf[48..52].copy_from_slice(&header_crc.to_le_bytes());
        buf
    }

    /// Decode a header from its canonical 52-byte form. Verifies magic,
    /// format_version, and the self-CRC (which covers the first 48 bytes).
    pub fn decode(bytes: &[u8; SNAPSHOT_HEADER_SIZE]) -> Result<Self, SnapshotError> {
        let mut magic = [0u8; 8];
        magic.copy_from_slice(&bytes[0..8]);
        if magic != SNAPSHOT_MAGIC {
            return Err(SnapshotError::BadMagic { got: magic });
        }
        let format_version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if format_version != SNAPSHOT_FORMAT_VERSION {
            return Err(SnapshotError::UnsupportedVersion(format_version));
        }
        // Verify self-CRC before trusting the rest.
        let stored_crc = u32::from_le_bytes([bytes[48], bytes[49], bytes[50], bytes[51]]);
        let computed_crc = crc32c::crc32c(&bytes[0..48]);
        if stored_crc != computed_crc {
            return Err(SnapshotError::HeaderCrcMismatch {
                expected: stored_crc,
                got: computed_crc,
            });
        }
        let flags = u16::from_le_bytes([bytes[10], bytes[11]]);
        let created_at_ms = i64::from_le_bytes(
            bytes[12..20]
                .try_into()
                .expect("snapshot header: created_at_ms"),
        );
        let snapshot_lsn = u64::from_le_bytes(
            bytes[20..28]
                .try_into()
                .expect("snapshot header: snapshot_lsn"),
        );
        let registry_version = u64::from_le_bytes(
            bytes[28..36]
                .try_into()
                .expect("snapshot header: registry_version"),
        );
        let body_len =
            u64::from_le_bytes(bytes[36..44].try_into().expect("snapshot header: body_len"));
        let body_crc32c = u32::from_le_bytes(
            bytes[44..48]
                .try_into()
                .expect("snapshot header: body_crc32c"),
        );
        Ok(SnapshotHeader {
            format_version,
            flags,
            created_at_ms,
            snapshot_lsn,
            registry_version,
            body_len,
            body_crc32c,
        })
    }
}
