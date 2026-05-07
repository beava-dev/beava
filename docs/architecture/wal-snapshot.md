# WAL + Snapshot

beava's durability model is the Redis RDB+AOF pattern: a write-ahead log
(WAL) for crash recovery + periodic snapshots for fast restart. Both
files live on local disk; there is no replication, no remote storage, no
quorum write. State recovery on boot is a snapshot load + WAL replay
from the snapshot's LSN to present.

This page covers the on-disk wire format, the recovery flow, the
sync-mode trade-offs, and what got reset in the Phase 12.7 schema strip.

## Overview

| File         | Role                                              | Lifetime                                              |
| ------------ | ------------------------------------------------- | ----------------------------------------------------- |
| WAL segment  | Per-event durability log; append-only              | Truncated when superseded by a complete snapshot      |
| Snapshot     | Full in-memory state + registry serialized to disk | Periodic (default 30 s); newest snapshot is canonical |

On boot, beava loads the latest snapshot, replays WAL records from that
snapshot's LSN to the present, then begins serving. RTO target is < 30 s
at 10 GB state on NVMe.

## WAL format (post-12.7 RESET)

Plan 12.7-05 RESET both `FORMAT_VERSION` (WAL) and
`SNAPSHOT_FORMAT_VERSION` / `SNAPSHOT_BODY_FORMAT_VERSION` back to **1**
when the entire `@bv.table` surface (TableUpsert / TableDelete /
TableRetract record types + temporal MVCC) was deleted. v0 launches at
version=1 across WAL/snapshot/wire — not a forward bump because v0 is
unreleased and there is nothing to be backward-compatible with.

**Pre-12.7 dev WALs / snapshots** (which carried `v=2`) fail at the
version-byte check on recovery with `SchemaVersionMismatch`. There is no
migration shim — operators clear `.beava/wal` + `.beava/snapshots`
before booting the new binary. (Locked decision per Phase 12.7 D-01:
"hard rip RESET, no migration.")

### WAL segment layout

The WAL is a sequence of **segments**, each prefixed by an 8-byte magic
header:

```text
File:   [MAGIC = b"BEAVAWAL"][record][record][record]...
```

Each record is a self-describing frame:

```text
Record: [u32 length][u32 crc32c][u64 lsn][u8 record_type][payload]
        ↑       ↑       ↑       ↑       ↑
        |       |       |       |       └── raw bytes (record-type specific)
        |       |       |       └── 0x01 Event | 0x02 RegistryBump
        |       |       └── monotonically increasing log sequence number
        |       └── CRC32C over [lsn || record_type || payload]
        └── covers [crc || lsn || record_type || payload]
```

Implementation: [`crates/beava-persistence/src/record.rs`](../../crates/beava-persistence/src/record.rs).

### RecordType variants (post-12.7)

| Discriminant | Variant         | Payload                                                  |
| ------------ | --------------- | -------------------------------------------------------- |
| `0x01`       | `Event`         | Serialized event row (one push)                          |
| `0x02`       | `RegistryBump`  | Serialized registry diff for a successful register call  |
| `0x03`       | (deleted)       | Was `TableUpsert`; pre-12.7 only — surfaces as `UnknownRecordType` on recovery |
| `0x04`       | (deleted)       | Was `TableDelete`; pre-12.7 only                         |
| `0x05`       | (deleted)       | Was `Retract`; pre-12.7 only                             |

Bytes `0x03` / `0x04` / `0x05` fall through to the existing
`UnknownRecordType` arm — pre-12.7 dev WALs that carried these bytes
surface the standard "unknown record_type" error on recovery, which is
the operator's signal to clear the WAL.

## Snapshot format (post-12.7 RESET)

Snapshot files have the same MAGIC + version pattern. Two version
constants gate the format:

- `SNAPSHOT_FORMAT_VERSION = 1` — header version
  (`crates/beava-persistence/src/snapshot_header.rs`).
- `SNAPSHOT_BODY_FORMAT_VERSION = 1` — body version
  (`crates/beava-core/src/snapshot_body.rs`).

The header carries the version, the LSN at which the snapshot was taken,
and a CRC over the body. The body is the serialized in-memory state +
registry, written via `serde` + bincode.

Snapshots run on a dedicated writer thread (off the apply path) at a
periodic cadence (default 30 s, configurable). The writer takes a
copy-on-write reference to state, serializes off-path, and atomically
renames the temp file into place when complete. The apply thread is
never blocked on snapshot serialization.

## Recovery on boot

Recovery happens in [`crates/beava-server/src/recovery.rs`](../../crates/beava-server/src/recovery.rs)
before `ServerV18::bind` returns. Sequence:

1. **Discover snapshots** under `<persist_dir>/snapshots/`.
2. **Load latest snapshot.** Verify magic, version, header CRC, body CRC.
   Deserialize state + registry. Note the snapshot's LSN N.
3. **Discover WAL segments** under `<persist_dir>/wal/`.
4. **Replay WAL from LSN N+1 to present.** Each record is decoded;
   events apply through `apply_event_to_aggregations` (recovery is one
   of the two allowlisted callers — see
   [mio-data-plane.md](./mio-data-plane.md)); RegistryBump records
   reapply to the registry.
5. **Mark ready.** `/ready` admin probe begins returning 200; the apply
   thread starts polling for new connections.

RTO target: **< 30 s on 10 GB state on NVMe**. Recovery runs single-threaded
on the apply thread (mirrors the production hot path).

## Sync modes

The WAL writer has two sync modes (Phase 6.1):

### `SyncMode::Periodic` (default)

Group-commit fsync every **1-5 ms or 1 MB**, whichever comes first.
Acks the push as soon as the record is appended to the in-memory ring
buffer (acks=1 Kafka-style). This is what gives the **~15× EPS lift**
over per-event fsync. Default mode for `OP_PUSH`.

Trade: under crash, you can lose up to one fsync interval of pushes
(typically 1-5 ms worth, ~5k-25k events at 1 M EPS). The events are
acked; the operator may have downstream consumers that observed them.
For most fraud / ad-tech workloads this is acceptable — the real-time
feature is what matters; a few seconds of post-crash gap is recovered
by upstream replay.

### `SyncMode::PerEvent` (`OP_PUSH_SYNC`)

Acks=all: the push is held until the WAL record's LSN has been fsynced
to disk. Survives crash without loss. Cost: P99 latency rises to fsync
latency (~1-10 ms on NVMe).

`OP_PUSH_SYNC` is wired through Phase 6.1 but the SDK opcode is v0.1+.
See [`.planning/ideas/v0.1-deferrals.md`](../../.planning/ideas/v0.1-deferrals.md).

## Four-watermark LSN discipline

The WAL writer maintains four watermarks per Phase 18 design (memory
file `project_phase18_wal_architecture`):

| Watermark   | Meaning                                                 |
| ----------- | ------------------------------------------------------- |
| `committed` | Apply thread acknowledged the in-memory append          |
| `written`   | WAL writer wrote the bytes to the kernel                |
| `synced`    | `fsync` returned                                        |
| `acked`     | Reply sent to the client                                |

For `acks=1` (default): client acks after `committed`. For
`acks=all` (`OP_PUSH_SYNC`): client acks after `synced`.

The 3-buffer state machine (active / sealed / flushing / free) gives
the apply thread always-available append capacity even while the
writer is fsyncing. Buffer size: 16 MiB × 3 buffers (Phase 18 lock).

## Snapshot truncation

A successful snapshot at LSN N truncates the WAL — segments whose
records are entirely ≤ N can be removed. The snapshot writer signals
the WAL writer once the snapshot is durable + atomically renamed; the
WAL writer truncates on its next tick.

**Crash during truncation:** the snapshot is durable before any WAL
removal happens, so a crash mid-truncate just leaves more WAL than
needed — recovery still works (it'll replay from the snapshot's LSN).

## Implications for users

- **Pick a `persist_dir` on local fast disk.** WAL throughput is fsync-
  bound on the writer thread; NVMe ≥ SATA SSD ≥ network FS. The Phase
  18 WAL refuses to bind to network filesystems by default.
- **Default `SyncMode::Periodic` is what you want.** It's the
  Kafka-default trade. Use `PerEvent` only if your downstream
  guarantees demand it.
- **Snapshot interval is configurable.** Default 30 s. Smaller →
  faster recovery, higher steady-state I/O. Larger → slower recovery,
  lower steady-state I/O.
- **Pre-12.7 WALs / snapshots are not readable by post-12.7 binaries.**
  Clear `<persist_dir>/wal` + `<persist_dir>/snapshots` before
  upgrading from a pre-12.7 dev binary. There is no migration tool.

## Cross-references

- [`CLAUDE.md` § Events-Only Invariant (locked Phase 12.7)](../../CLAUDE.md)
  — the schema-reset commitment + version=1 reset rationale.
- [`CLAUDE.md` § Constraints](../../CLAUDE.md) — "WAL + periodic snapshot
  for durability" + "No external storage dependencies."
- `~/.claude/projects/-Users-petrpan26-work-tally/memory/project_phase18_wal_architecture.md`
  — locked Phase 18 WAL architecture (lock-free apply, 3-buffer state
  machine, four-watermark discipline, refuse-on-network-FS).
- [`crates/beava-persistence/src/record.rs`](../../crates/beava-persistence/src/record.rs)
  — `FORMAT_VERSION`, `MAGIC`, frame encoding/decoding.
- [`crates/beava-persistence/src/snapshot_header.rs`](../../crates/beava-persistence/src/snapshot_header.rs)
  — `SNAPSHOT_FORMAT_VERSION`, snapshot header layout.
- [`crates/beava-server/src/recovery.rs`](../../crates/beava-server/src/recovery.rs)
  — boot recovery flow.
- [single-thread-apply.md](./single-thread-apply.md) — apply thread vs
  WAL writer thread separation.
- [memory-budget.md](./memory-budget.md) — in-memory state sizing
  (snapshots write the same).
- [observability.md](./observability.md) — `beava_wal_append_latency_seconds`,
  `beava_snapshot_latency_seconds` metrics.
- [../wire-spec.md](../wire-spec.md) — `OP_PUSH` ack semantics +
  `ack_lsn` reply field.
