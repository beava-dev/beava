use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static SNAPSHOT_LAST_DURATION_US: AtomicU64 = AtomicU64::new(0);
static SNAPSHOT_LAST_BYTES: AtomicU64 = AtomicU64::new(0);
static SNAPSHOT_LAST_FSYNC_US: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SnapshotMetricsSnapshot {
    pub last_duration_us: u64,
    pub last_bytes: u64,
    pub last_fsync_us: u64,
}

pub(crate) fn record_snapshot_success(duration: Duration, bytes: u64, fsync_duration: Duration) {
    SNAPSHOT_LAST_DURATION_US.store(duration_micros(duration), Ordering::Relaxed);
    SNAPSHOT_LAST_BYTES.store(bytes, Ordering::Relaxed);
    SNAPSHOT_LAST_FSYNC_US.store(duration_micros(fsync_duration), Ordering::Relaxed);
}

pub(crate) fn snapshot() -> SnapshotMetricsSnapshot {
    SnapshotMetricsSnapshot {
        last_duration_us: SNAPSHOT_LAST_DURATION_US.load(Ordering::Relaxed),
        last_bytes: SNAPSHOT_LAST_BYTES.load(Ordering::Relaxed),
        last_fsync_us: SNAPSHOT_LAST_FSYNC_US.load(Ordering::Relaxed),
    }
}

fn duration_micros(duration: Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}
