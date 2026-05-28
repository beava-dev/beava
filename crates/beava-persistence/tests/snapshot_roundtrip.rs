//! Snapshot format round-trip + corruption probes.

use beava_persistence::{
    list_snapshots, prune_old_snapshots, PersistError, SnapshotError, SnapshotHeader,
    SnapshotReader, SnapshotWriter, SNAPSHOT_FORMAT_VERSION, SNAPSHOT_HEADER_SIZE, SNAPSHOT_MAGIC,
};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

/// Helper: build a SnapshotHeader instance with canonical test values.
fn make_header(snapshot_lsn: u64, body_crc: u32, body_len: u64) -> SnapshotHeader {
    SnapshotHeader {
        format_version: SNAPSHOT_FORMAT_VERSION,
        flags: 0,
        created_at_ms: 1_714_000_000_000,
        snapshot_lsn,
        registry_version: 7,
        body_len,
        body_crc32c: body_crc,
    }
}

#[test]
fn snapshot_header_roundtrip_bytes_exact() {
    let h = make_header(42, 0xDEADBEEF, 100);
    let bytes = h.encode();
    assert_eq!(bytes.len(), SNAPSHOT_HEADER_SIZE);
    assert_eq!(&bytes[..8], &SNAPSHOT_MAGIC);
    let decoded = SnapshotHeader::decode(&bytes).expect("decode ok");
    assert_eq!(decoded.format_version, h.format_version);
    assert_eq!(decoded.flags, h.flags);
    assert_eq!(decoded.created_at_ms, h.created_at_ms);
    assert_eq!(decoded.snapshot_lsn, h.snapshot_lsn);
    assert_eq!(decoded.registry_version, h.registry_version);
    assert_eq!(decoded.body_len, h.body_len);
    assert_eq!(decoded.body_crc32c, h.body_crc32c);
}

#[test]
fn snapshot_header_magic_corruption_rejected() {
    let h = make_header(1, 0, 0);
    let mut bytes = h.encode();
    bytes[0] ^= 0xFF;
    let err = SnapshotHeader::decode(&bytes).unwrap_err();
    assert!(matches!(err, SnapshotError::BadMagic { .. }));
}

#[test]
fn snapshot_header_version_future_rejected() {
    let h = make_header(1, 0, 0);
    let mut bytes = h.encode();
    // format_version is at bytes[8..10]
    bytes[8] = 99;
    bytes[9] = 0;
    // Also need to fix CRC so we hit UnsupportedVersion, not HeaderCrcMismatch.
    let new_crc = crc32c::crc32c(&bytes[..SNAPSHOT_HEADER_SIZE - 4]);
    bytes[SNAPSHOT_HEADER_SIZE - 4..].copy_from_slice(&new_crc.to_le_bytes());
    let err = SnapshotHeader::decode(&bytes).unwrap_err();
    assert!(matches!(err, SnapshotError::UnsupportedVersion(99)));
}

#[test]
fn snapshot_header_self_crc_corruption_rejected() {
    let h = make_header(1, 0, 0);
    let mut bytes = h.encode();
    // Flip a byte inside created_at_ms (offset 12..20) without updating CRC.
    bytes[15] ^= 0xAA;
    let err = SnapshotHeader::decode(&bytes).unwrap_err();
    assert!(matches!(err, SnapshotError::HeaderCrcMismatch { .. }));
}

#[test]
fn snapshot_write_then_read_roundtrip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let body = b"hello world payload".to_vec();
    let path = SnapshotWriter::write(tmp.path(), 1000, 3, &body).expect("write");
    assert!(path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .ends_with("snapshot-00000000000003e8.bvs"));

    let (hdr, body_read) = SnapshotReader::open(&path).expect("read");
    assert_eq!(hdr.snapshot_lsn, 1000);
    assert_eq!(hdr.registry_version, 3);
    assert_eq!(body_read, body);
}

#[test]
fn snapshot_write_with_stats_reports_bytes_and_fsync_timing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let body = b"instrumented payload".to_vec();
    let stats = SnapshotWriter::write_with_stats(tmp.path(), 1001, 3, &body).expect("write");

    assert!(stats.path.exists());
    assert_eq!(stats.bytes, SNAPSHOT_HEADER_SIZE as u64 + body.len() as u64);
    assert!(stats.total_fsync_duration() >= stats.file_fsync_duration);
    #[cfg(unix)]
    assert!(
        stats.dir_fsync_duration.is_some(),
        "unix snapshot writes should attempt parent-dir fsync"
    );
}

#[test]
fn snapshot_body_corruption_detected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let body = vec![0x42u8; 64];
    let path = SnapshotWriter::write(tmp.path(), 7, 1, &body).expect("write");
    // Flip a byte inside the body (offset SNAPSHOT_HEADER_SIZE + 3).
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    f.seek(SeekFrom::Start((SNAPSHOT_HEADER_SIZE + 3) as u64))
        .unwrap();
    let mut b = [0u8; 1];
    use std::io::Read;
    f.read_exact(&mut b).unwrap();
    b[0] ^= 0xFF;
    f.seek(SeekFrom::Start((SNAPSHOT_HEADER_SIZE + 3) as u64))
        .unwrap();
    f.write_all(&b).unwrap();
    f.sync_all().unwrap();
    drop(f);

    let err = SnapshotReader::open(&path).unwrap_err();
    match err {
        PersistError::Snapshot(SnapshotError::BodyCrcMismatch { .. }) => {}
        other => panic!("expected BodyCrcMismatch, got {other:?}"),
    }
}

#[test]
fn snapshot_truncated_body_detected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let body = vec![0xABu8; 100];
    let path = SnapshotWriter::write(tmp.path(), 11, 1, &body).expect("write");
    // Truncate file to header + 5 bytes (less than body_len=100).
    let f = OpenOptions::new().write(true).open(&path).unwrap();
    f.set_len((SNAPSHOT_HEADER_SIZE + 5) as u64).unwrap();
    f.sync_all().unwrap();
    drop(f);

    let err = SnapshotReader::open(&path).unwrap_err();
    match err {
        PersistError::Snapshot(SnapshotError::Truncated { .. }) => {}
        other => panic!("expected Truncated, got {other:?}"),
    }
}

#[test]
fn snapshot_list_returns_descending_by_lsn() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let body = b"x".to_vec();
    for lsn in [100u64, 300, 200, 50] {
        SnapshotWriter::write(tmp.path(), lsn, 1, &body).expect("write");
    }
    let got = list_snapshots(tmp.path()).expect("list");
    let lsns: Vec<u64> = got.into_iter().map(|(l, _)| l).collect();
    assert_eq!(lsns, vec![300u64, 200, 100, 50]);
}

#[test]
fn snapshot_prune_keeps_n_most_recent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let body = b"x".to_vec();
    for lsn in [100u64, 200, 300, 400, 500] {
        SnapshotWriter::write(tmp.path(), lsn, 1, &body).expect("write");
    }
    let removed = prune_old_snapshots(tmp.path(), 2).expect("prune");
    assert_eq!(removed, 3);
    let remaining: Vec<u64> = list_snapshots(tmp.path())
        .unwrap()
        .into_iter()
        .map(|(l, _)| l)
        .collect();
    assert_eq!(remaining, vec![500u64, 400]);
}

#[test]
fn snapshot_writer_leaves_no_tmp_on_success() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let body = b"ok".to_vec();
    SnapshotWriter::write(tmp.path(), 1, 1, &body).expect("write");
    let tmp_count = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s == "tmp")
                .unwrap_or(false)
        })
        .count();
    assert_eq!(tmp_count, 0);
}

#[test]
fn snapshot_writer_overwrites_orphan_tmp() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Drop an orphan .tmp from a simulated prior crash.
    let orphan = tmp.path().join("snapshot-0000000000000064.tmp");
    std::fs::write(&orphan, b"garbage").unwrap();
    // New write with same LSN (100 = 0x64) should succeed.
    let path = SnapshotWriter::write(tmp.path(), 100, 1, b"real").expect("write");
    let (_, body) = SnapshotReader::open(&path).expect("read");
    assert_eq!(body, b"real");
    // Tmp should not remain after rename.
    assert!(
        !orphan.exists(),
        "orphan tmp should have been overwritten + renamed away"
    );
}
