//! Regression tests for the `WAL header-only orphan segment` deploy crashloop.
//!
//! Background: a previous boot called `WalWriter::open(dir, start_lsn=N, ...)`
//! which writes a 24-byte segment header. If the container is killed (SIGKILL,
//! crash) before the writer appends any record, the segment file at `start_lsn=N`
//! is left on disk with exactly `HEADER_SIZE` bytes.
//!
//! On the next boot, recovery reads the WAL up through `last_lsn = N - 1`
//! (the orphan contributes no records, so it can't advance recovery's
//! cursor), determines `initial_start_lsn = N`, and asks the writer to open
//! `wal-{N:016x}.log` again. The pre-fix writer used `OpenOptions::create_new(true)`,
//! which fails with `ErrorKind::AlreadyExists` (`EEXIST`, errno 17) — the
//! same error visible in production logs:
//!
//! ```text
//! Error: bind ServerV18 listeners
//! Caused by:
//!     failed to spawn WAL sink: io: File exists (os error 17)
//! ```
//!
//! With `restart: unless-stopped`, the container then crashloops indefinitely;
//! whoever clears the WAL dir to break the loop also clears all per-entity
//! state. This is exactly the "state got reset once in a while" symptom
//! observed on the beava.dev Hetzner deploy on 2026-05-07.
//!
//! These tests pin the post-fix contract:
//!   1. A header-only orphan at `start_lsn=N` with matching `(start_lsn,
//!      registry_version)` MUST be reused — the next `open` succeeds, and
//!      subsequent appends round-trip through the reader.
//!   2. A non-empty segment at the requested `start_lsn` MUST refuse with a
//!      structured error — we will NOT silently overwrite committed data.
//!   3. A header-only orphan with a `(start_lsn, registry_version)` mismatch
//!      MUST refuse — the recovery state has diverged from the orphan and
//!      blindly reusing would corrupt.

use beava_persistence::{PersistError, RecordType, WalReader, WalRecord, WalWriter};
use std::io::Write as _;

const HEADER_SIZE: u64 = 24;

fn segment_path(dir: &std::path::Path, start_lsn: u64) -> std::path::PathBuf {
    dir.join(format!("wal-{start_lsn:016x}.log"))
}

fn sample_record(lsn: u64) -> WalRecord {
    WalRecord {
        lsn,
        record_type: RecordType::Event,
        payload: format!("{{\"lsn\":{lsn}}}").into_bytes(),
    }
}

/// Reproduces the production crashloop: a previous boot left a header-only
/// segment at `start_lsn=N`. The next boot's `WalWriter::open(dir, N, R)`
/// MUST succeed (reuse the orphan), not return `AlreadyExists`.
#[test]
fn open_reuses_header_only_orphan_segment() {
    let dir = tempfile::tempdir().unwrap();
    let start_lsn: u64 = 7;
    let registry_version: u32 = 3;

    // Boot 1: open the writer, drop without appending — the file ends up
    // containing exactly the 24-byte header.
    {
        let _w = WalWriter::open(dir.path(), start_lsn, registry_version)
            .expect("first open should succeed");
        // Drop here flushes the BufWriter so the header lands on disk.
    }

    let path = segment_path(dir.path(), start_lsn);
    let len = std::fs::metadata(&path).unwrap().len();
    assert_eq!(
        len, HEADER_SIZE,
        "precondition: file should be exactly the segment header"
    );

    // Boot 2: simulate recovery determining the same `initial_start_lsn`
    // and asking the writer to open at `start_lsn=7` again.
    let mut w = WalWriter::open(dir.path(), start_lsn, registry_version)
        .expect("reopen on a header-only orphan must succeed (this is the fix)");

    // Append a record to confirm the writer is functional after reuse.
    let rec = sample_record(start_lsn);
    w.append(&rec).expect("append after orphan reuse");
    w.sync_data().expect("sync after orphan reuse");
    drop(w);

    // The reader should produce: a single record at LSN=7. The header must
    // remain intact (start_lsn=7, registry_version=3).
    let r = WalReader::open(&path).expect("reader open");
    assert_eq!(r.start_lsn(), start_lsn);
    assert_eq!(r.registry_version(), registry_version);
    let recs: Vec<_> = r.collect::<Result<_, _>>().expect("read records");
    assert_eq!(recs.len(), 1, "exactly one record after reuse");
    assert_eq!(recs[0].lsn, start_lsn);
    assert_eq!(
        recs[0].payload,
        format!("{{\"lsn\":{start_lsn}}}").as_bytes()
    );
}

/// Reuse must NOT corrupt existing records. If a segment at `start_lsn=N`
/// already contains records past the header, `open` MUST refuse rather
/// than overwrite.
#[test]
fn open_refuses_non_empty_segment_collision() {
    let dir = tempfile::tempdir().unwrap();
    let start_lsn: u64 = 12;
    let registry_version: u32 = 1;

    // Boot 1: open + append + flush. Segment is now `HEADER_SIZE + record bytes`.
    {
        let mut w = WalWriter::open(dir.path(), start_lsn, registry_version).unwrap();
        w.append(&sample_record(start_lsn)).unwrap();
        w.sync_data().unwrap();
    }

    let path = segment_path(dir.path(), start_lsn);
    let len_before = std::fs::metadata(&path).unwrap().len();
    assert!(
        len_before > HEADER_SIZE,
        "precondition: file should contain a record"
    );

    // Boot 2: a buggy recovery (or an attacker) hands the writer a `start_lsn`
    // that already has committed records on disk. Refuse.
    match WalWriter::open(dir.path(), start_lsn, registry_version) {
        Ok(_) => panic!("must refuse to overwrite a non-empty segment"),
        Err(PersistError::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(other) => panic!("expected AlreadyExists, got {other:?}"),
    }

    // Sanity: the existing segment is untouched.
    let len_after = std::fs::metadata(&path).unwrap().len();
    assert_eq!(len_before, len_after, "segment must not be overwritten");
}

/// Reuse must validate the orphan header. If the on-disk header records
/// `(start_lsn, registry_version)` that don't match what `open` was called
/// with, reuse would conflate two different recovery histories.
#[test]
fn open_refuses_orphan_with_mismatched_header() {
    let dir = tempfile::tempdir().unwrap();
    let start_lsn: u64 = 42;

    // Boot 1: open with registry_version=2, drop. Header records (42, 2).
    {
        let _w = WalWriter::open(dir.path(), start_lsn, 2).unwrap();
    }
    let path = segment_path(dir.path(), start_lsn);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), HEADER_SIZE);

    // Boot 2: try to reopen the orphan with a different registry_version.
    // Either AlreadyExists (pre-fix) or any error variant the post-fix code
    // chose for the mismatch — but it MUST NOT silently succeed.
    if WalWriter::open(dir.path(), start_lsn, /* mismatched */ 99).is_ok() {
        panic!("orphan reuse must validate registry_version");
    }

    // Sanity: the file is still there with its original size.
    assert_eq!(std::fs::metadata(&path).unwrap().len(), HEADER_SIZE);
}

/// A 'half-header' (file shorter than HEADER_SIZE) is unreusable too —
/// likely a torn write or filesystem corruption. Refuse, don't overwrite.
#[test]
fn open_refuses_truncated_pre_header_file() {
    let dir = tempfile::tempdir().unwrap();
    let start_lsn: u64 = 5;
    let path = segment_path(dir.path(), start_lsn);

    // Place a 10-byte file at the segment path — shorter than HEADER_SIZE.
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(&[0u8; 10]).unwrap();
    drop(f);

    if WalWriter::open(dir.path(), start_lsn, 1).is_ok() {
        panic!("must refuse to reuse a truncated pre-header file");
    }

    // The mystery file is left in place for human investigation; the writer
    // refused to clobber it.
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 10);
}
