//! Group-commit fsync worker — owns a single `WalWriter` and broadcasts a
//! `durable_lsn` watermark every time a batch is fsynced. Coalesces appends
//! on a 1–5 ms timer or a 1 MiB byte threshold (whichever fires first); ACKs
//! `SyncMode::PerEvent` callers only after their LSN's batch has fsynced.

use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use crate::error::PersistError;
use crate::rotation;
use crate::writer::WalWriter;
use crate::{Lsn, RecordType, WalRecord};

/// Configuration for `WalSink::spawn`.
#[derive(Debug, Clone)]
pub struct WalSinkConfig {
    pub dir: PathBuf,
    pub initial_start_lsn: Lsn,
    pub initial_registry_version: u32,
    /// Max time to coalesce appends before fsync. Default 2 ms.
    pub fsync_interval_ms: u64,
    /// Max staged bytes (per segment) before forcing an fsync. Default 1 MiB.
    pub fsync_bytes: u64,
    /// Segment size before rotating to a new segment. Default 128 MiB.
    pub segment_bytes: u64,
    /// Default sync semantics for `append_event`. `Periodic` (default) ACKs
    /// after the in-memory append and defers fsync to the timer; `PerEvent`
    /// blocks each append on fsync.
    pub sync_mode: SyncMode,
}

impl Default for WalSinkConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("./beava-wal"),
            initial_start_lsn: 1,
            initial_registry_version: 1,
            fsync_interval_ms: 2,
            fsync_bytes: 1 << 20,
            segment_bytes: 128 << 20,
            sync_mode: SyncMode::Periodic,
        }
    }
}

/// Per-append fsync semantics.
///
/// `Periodic` (default) — `append_event_with_mode` resolves as soon as the
/// payload has been encoded + written to the in-memory `BufWriter` and an
/// LSN assigned. The background timer fsyncs on its own cadence
/// (`fsync_interval_ms`). On crash within that window the ACK'd event MAY
/// be lost — semantics match Kafka `acks=1`.
///
/// `PerEvent` — resolves only after the assigned LSN has been fsynced to
/// disk. Used by the strict `/push-sync` endpoint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncMode {
    #[default]
    Periodic,
    PerEvent,
}

struct AppendRequest {
    record_type: RecordType,
    payload: Vec<u8>,
    min_lsn: Lsn,
    mode: SyncMode,
    done: oneshot::Sender<Result<Lsn, PersistError>>,
}

enum ControlMsg {
    TruncateUpTo {
        covered_lsn: Lsn,
        ack: oneshot::Sender<Result<u32, PersistError>>,
    },
    Shutdown {
        ack: oneshot::Sender<Result<(), PersistError>>,
    },
}

/// Handle to the fsync worker. Cloneable; callers use `append_event` to
/// stage a record and await its durable LSN.
#[derive(Clone)]
pub struct WalSink {
    append_tx: mpsc::Sender<AppendRequest>,
    control_tx: mpsc::Sender<ControlMsg>,
    durable_rx: watch::Receiver<Lsn>,
    default_mode: SyncMode,
}

impl WalSink {
    /// Spawn a **no-op** WAL sink for `Persistence::Memory` mode. The returned
    /// worker drains append requests without writing to disk: every
    /// `append_event` / `append_record` call resolves with a fake
    /// monotonically-increasing LSN; `truncate_up_to` / `shutdown` succeed
    /// without touching the filesystem; `durable_lsn()` reflects the latest
    /// assigned LSN so watchers continue to make progress.
    ///
    /// Used by `ServerV18::bind_with_config` in memory mode to keep the
    /// `AppState { wal_sink: WalSink, .. }` shape unchanged while skipping
    /// all file I/O.
    pub fn spawn_no_op() -> (Self, JoinHandle<()>) {
        let (append_tx, mut append_rx) = mpsc::channel::<AppendRequest>(1024);
        let (control_tx, mut control_rx) = mpsc::channel::<ControlMsg>(16);
        let (durable_tx, durable_rx) = watch::channel::<Lsn>(0);

        let join = tokio::spawn(async move {
            let mut next_lsn: Lsn = 1;
            loop {
                tokio::select! {
                    biased;

                    ctrl = control_rx.recv() => {
                        match ctrl {
                            Some(ControlMsg::TruncateUpTo { covered_lsn: _, ack }) => {
                                // No-op: nothing on disk to truncate.
                                let _ = ack.send(Ok(0));
                            }
                            Some(ControlMsg::Shutdown { ack }) => {
                                let _ = ack.send(Ok(()));
                                return;
                            }
                            None => return,
                        }
                    }

                    req = append_rx.recv() => {
                        match req {
                            Some(req) => {
                                next_lsn = next_lsn.max(req.min_lsn);
                                let assigned = next_lsn;
                                next_lsn = next_lsn.saturating_add(1);
                                // Advance the durable watermark immediately —
                                // there is no fsync to defer.
                                let _ = durable_tx.send(assigned);
                                let _ = req.done.send(Ok(assigned));
                            }
                            None => return,
                        }
                    }
                }
            }
        });

        (
            Self {
                append_tx,
                control_tx,
                durable_rx,
                default_mode: SyncMode::Periodic,
            },
            join,
        )
    }

    /// Spawn the worker task. Returns the sink handle + the worker JoinHandle
    /// (callers should await the handle after `shutdown()` for clean exit).
    pub fn spawn(cfg: WalSinkConfig) -> Result<(Self, JoinHandle<()>), PersistError> {
        let writer = WalWriter::open(
            &cfg.dir,
            cfg.initial_start_lsn,
            cfg.initial_registry_version,
        )?;

        let (append_tx, append_rx) = mpsc::channel::<AppendRequest>(1024);
        let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(16);
        // durable_lsn starts below the first-to-be-assigned LSN.
        let (durable_tx, durable_rx) =
            watch::channel::<Lsn>(cfg.initial_start_lsn.saturating_sub(1));

        let default_mode = cfg.sync_mode;
        let join = tokio::spawn(worker_loop(cfg, writer, append_rx, control_rx, durable_tx));

        Ok((
            Self {
                append_tx,
                control_tx,
                durable_rx,
                default_mode,
            },
            join,
        ))
    }

    /// Enqueue a typed WAL record for durable append. Resolves only after
    /// the assigned LSN has been fsynced to disk — registry-version
    /// transitions must be durable before any future event can reference
    /// them, so this entry point is always strict regardless of the sink's
    /// default sync mode.
    pub async fn append_record(
        &self,
        record_type: RecordType,
        payload: Vec<u8>,
    ) -> Result<Lsn, PersistError> {
        self.append_record_with_mode_at_least(record_type, payload, 0, SyncMode::PerEvent)
            .await
    }

    /// Strict append that first raises the assigned LSN to at least
    /// `min_lsn`. Used when another WAL stream has already advanced the
    /// logical LSN namespace.
    pub async fn append_record_at_least(
        &self,
        record_type: RecordType,
        payload: Vec<u8>,
        min_lsn: Lsn,
    ) -> Result<Lsn, PersistError> {
        self.append_record_with_mode_at_least(record_type, payload, min_lsn, SyncMode::PerEvent)
            .await
    }

    /// Explicit-mode variant of `append_record`. `Periodic` resolves as soon
    /// as the in-memory append + LSN assignment has happened (the background
    /// timer fsyncs on its own schedule); `PerEvent` blocks until the
    /// assigned LSN has been fsynced.
    pub async fn append_record_with_mode(
        &self,
        record_type: RecordType,
        payload: Vec<u8>,
        mode: SyncMode,
    ) -> Result<Lsn, PersistError> {
        self.append_record_with_mode_at_least(record_type, payload, 0, mode)
            .await
    }

    async fn append_record_with_mode_at_least(
        &self,
        record_type: RecordType,
        payload: Vec<u8>,
        min_lsn: Lsn,
        mode: SyncMode,
    ) -> Result<Lsn, PersistError> {
        let (tx, rx) = oneshot::channel();
        self.append_tx
            .send(AppendRequest {
                record_type,
                payload,
                min_lsn,
                mode,
                done: tx,
            })
            .await
            .map_err(|_| {
                PersistError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "WAL sink worker closed",
                ))
            })?;
        rx.await.map_err(|_| {
            PersistError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "WAL sink worker dropped ack channel",
            ))
        })?
    }

    /// Enqueue an event payload for durable append using the sink's
    /// configured `default_mode` (set at `spawn` time from
    /// `WalSinkConfig.sync_mode`). The default is `Periodic` (Kafka
    /// `acks=1` semantics). Strict callers should use
    /// `append_event_with_mode(…, SyncMode::PerEvent)` or the `/push-sync`
    /// endpoint.
    pub async fn append_event(&self, payload: Vec<u8>) -> Result<Lsn, PersistError> {
        self.append_event_with_mode(payload, self.default_mode)
            .await
    }

    /// Explicit-mode variant of `append_event`.
    pub async fn append_event_with_mode(
        &self,
        payload: Vec<u8>,
        mode: SyncMode,
    ) -> Result<Lsn, PersistError> {
        self.append_record_with_mode(RecordType::Event, payload, mode)
            .await
    }

    /// Current highest-durable LSN (snapshot of the watch channel).
    pub fn durable_lsn(&self) -> Lsn {
        *self.durable_rx.borrow()
    }

    /// Delete closed segments whose last LSN < `covered_lsn`. Returns the
    /// number of segments removed.
    pub async fn truncate_up_to(&self, covered_lsn: Lsn) -> Result<u32, PersistError> {
        let (tx, rx) = oneshot::channel();
        self.control_tx
            .send(ControlMsg::TruncateUpTo {
                covered_lsn,
                ack: tx,
            })
            .await
            .map_err(|_| {
                PersistError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "WAL sink worker closed",
                ))
            })?;
        rx.await.map_err(|_| {
            PersistError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "WAL sink worker dropped ack channel",
            ))
        })?
    }

    /// Flush any pending batch, close the current segment, and stop the worker.
    pub async fn shutdown(self) -> Result<(), PersistError> {
        let (tx, rx) = oneshot::channel();
        self.control_tx
            .send(ControlMsg::Shutdown { ack: tx })
            .await
            .map_err(|_| {
                PersistError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "WAL sink worker already closed",
                ))
            })?;
        rx.await.map_err(|_| {
            PersistError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "WAL sink worker dropped ack channel",
            ))
        })?
    }
}

/// Estimate on-disk size of an encoded record. Must match `record::encode_record`
/// byte count: `4 (length) + 4 (crc) + 8 (lsn) + 1 (type) + payload.len()`.
fn encoded_size(payload_len: usize) -> u64 {
    (4 + 4 + 8 + 1 + payload_len) as u64
}

async fn worker_loop(
    cfg: WalSinkConfig,
    mut writer: WalWriter,
    mut append_rx: mpsc::Receiver<AppendRequest>,
    mut control_rx: mpsc::Receiver<ControlMsg>,
    durable_tx: watch::Sender<Lsn>,
) {
    // `pending` holds PerEvent requests + Periodic requests staged but
    // un-fsynced. Periodic requests have already had their `done` resolved
    // by the time they hit `pending` (via `stage_request`), so the fsync
    // batch only resolves PerEvent waiters.
    let mut pending: Vec<PendingFsync> = Vec::new();
    let mut staged_bytes: u64 = 0;
    let mut next_lsn: Lsn = cfg.initial_start_lsn;
    let mut current_start_lsn: Lsn = cfg.initial_start_lsn;
    let fsync_interval = Duration::from_millis(cfg.fsync_interval_ms);
    let mut timer = tokio::time::interval(fsync_interval);
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Drop the immediate first tick that `interval` emits so the first
    // real tick lands one `fsync_interval_ms` after spawn, not on entry.
    timer.tick().await;

    loop {
        let force_now = staged_bytes >= cfg.fsync_bytes;

        let flush_now = tokio::select! {
            biased;

            ctrl = control_rx.recv() => {
                match ctrl {
                    Some(ControlMsg::TruncateUpTo { covered_lsn, ack }) => {
                        let res = rotation::truncate_up_to(&cfg.dir, current_start_lsn, covered_lsn);
                        let _ = ack.send(res);
                        false
                    }
                    Some(ControlMsg::Shutdown { ack }) => {
                        let res = fsync_batch(
                            &cfg,
                            &mut writer,
                            &mut pending,
                            &mut staged_bytes,
                            &mut next_lsn,
                            &mut current_start_lsn,
                            &durable_tx,
                        ).await;
                        // Final close fsync also runs off-runtime so
                        // shutdown doesn't stall other tasks.
                        let close = blocking_sync_data(&mut writer).await;
                        let combined = res.and_then(|_| close.map_err(PersistError::Io));
                        let _ = ack.send(combined);
                        return;
                    }
                    None => {
                        let _ = fsync_batch(
                            &cfg,
                            &mut writer,
                            &mut pending,
                            &mut staged_bytes,
                            &mut next_lsn,
                            &mut current_start_lsn,
                            &durable_tx,
                        ).await;
                        let _ = blocking_sync_data(&mut writer).await;
                        return;
                    }
                }
            }

            req = append_rx.recv() => {
                match req {
                    Some(req) => {
                        let pending_fsync_opt = stage_request(
                            &mut writer,
                            req,
                            &mut next_lsn,
                            &mut staged_bytes,
                        );
                        if let Some(pf) = pending_fsync_opt {
                            pending.push(pf);
                        }
                        // Force a fsync now if the staged-byte threshold
                        // is hit, regardless of where we are in the timer
                        // cadence.
                        staged_bytes >= cfg.fsync_bytes
                    }
                    None => {
                        let _ = fsync_batch(
                            &cfg,
                            &mut writer,
                            &mut pending,
                            &mut staged_bytes,
                            &mut next_lsn,
                            &mut current_start_lsn,
                            &durable_tx,
                        ).await;
                        let _ = blocking_sync_data(&mut writer).await;
                        return;
                    }
                }
            }

            _ = timer.tick() => {
                // Timer ticks unconditionally; only flush when there is
                // something staged so an idle sink doesn't burn syscalls.
                staged_bytes > 0
            }
        };

        if flush_now || force_now {
            let _ = fsync_batch(
                &cfg,
                &mut writer,
                &mut pending,
                &mut staged_bytes,
                &mut next_lsn,
                &mut current_start_lsn,
                &durable_tx,
            )
            .await;
        }
    }
}

/// Flush the in-memory buffer on the runtime thread (cheap memcpy + write
/// to the kernel page cache) then issue the blocking `sync_data()` syscall
/// on a `spawn_blocking` thread via a cloned fd. The clone shares the same
/// kernel file description, so the fsync durably persists bytes flushed
/// from the original handle. Running fsync off-runtime is required because
/// macOS `F_FULLSYNC` blocks ~7 ms — long enough to stall every other task
/// on a `current_thread` tokio runtime if executed inline.
///
/// Falls back to inline fsync if the file handle can't be cloned (rare —
/// only on FD exhaustion); the fallback is correct, just temporarily blocks.
async fn blocking_sync_data(writer: &mut crate::writer::WalWriter) -> std::io::Result<()> {
    writer.flush_buffer()?;
    let cloned = match writer.try_clone_file() {
        Ok(f) => f,
        Err(_) => return writer.sync_data(),
    };
    tokio::task::spawn_blocking(move || cloned.sync_data())
        .await
        .map_err(|e| std::io::Error::other(format!("spawn_blocking join error: {e}")))?
}

/// A PerEvent request that's been written to the BufWriter but is awaiting
/// fsync before its `done` channel resolves.
struct PendingFsync {
    lsn: Lsn,
    done: oneshot::Sender<Result<Lsn, PersistError>>,
}

/// Stage a single request: assign LSN, encode + write to BufWriter. For
/// Periodic mode resolve `done` immediately; for PerEvent return a
/// `PendingFsync` so the caller pushes it onto the fsync queue.
fn stage_request(
    writer: &mut WalWriter,
    req: AppendRequest,
    next_lsn: &mut Lsn,
    staged_bytes: &mut u64,
) -> Option<PendingFsync> {
    *next_lsn = (*next_lsn).max(req.min_lsn);
    let lsn = *next_lsn;
    *next_lsn += 1;
    let payload_len = req.payload.len();
    let record = WalRecord {
        lsn,
        record_type: req.record_type,
        payload: req.payload,
    };
    if let Err(e) = writer.append(&record) {
        let _ = req.done.send(Err(e));
        return None;
    }
    *staged_bytes += encoded_size(payload_len);
    match req.mode {
        SyncMode::Periodic => {
            // ACK now — fsync happens on the timer (or shutdown).
            let _ = req.done.send(Ok(lsn));
            None
        }
        SyncMode::PerEvent => Some(PendingFsync {
            lsn,
            done: req.done,
        }),
    }
}

/// fsync the BufWriter, bump the durable watermark, resolve any PerEvent
/// waiters, and rotate if the segment has filled. The matching staging
/// step lives in `stage_request` on the recv path.
async fn fsync_batch(
    cfg: &WalSinkConfig,
    writer_ref: &mut WalWriter,
    pending: &mut Vec<PendingFsync>,
    staged_bytes: &mut u64,
    next_lsn: &mut Lsn,
    current_start_lsn: &mut Lsn,
    durable_tx: &watch::Sender<Lsn>,
) -> Result<(), PersistError> {
    if *staged_bytes == 0 && pending.is_empty() {
        return Ok(());
    }

    // The highest LSN to publish on the watermark. PerEvent waiters track
    // their own LSN; for Periodic-only batches we still need to advance
    // the watermark to `next_lsn - 1`.
    let highest = next_lsn.saturating_sub(1);

    let sync_result = blocking_sync_data(writer_ref)
        .await
        .map_err(PersistError::Io);

    if sync_result.is_ok() {
        let _ = durable_tx.send(highest);
        for pf in pending.drain(..) {
            let _ = pf.done.send(Ok(pf.lsn));
        }
    } else {
        let err = || PersistError::Io(std::io::Error::other("fsync"));
        for pf in pending.drain(..) {
            let _ = pf.done.send(Err(err()));
        }
        *staged_bytes = 0;
        return sync_result;
    }

    *staged_bytes = 0;

    if writer_ref.bytes_written() >= cfg.segment_bytes {
        let new_start = *next_lsn;
        rotation::rotate(
            writer_ref,
            &cfg.dir,
            new_start,
            cfg.initial_registry_version,
        )?;
        *current_start_lsn = new_start;
    }

    Ok(())
}
