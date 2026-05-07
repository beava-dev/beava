# Single-Threaded Apply

beava processes every event on a single OS thread — the apply loop. There
is no in-process apply sharding, no work-stealing scheduler, no per-core
fan-out. Auxiliary threads exist (the WAL writer, the HTTP listener, the
admin sidecar, the snapshot writer) but they sit beside the apply thread,
not inside it. State mutations only happen on the apply thread.

This is a permanent architectural commitment: the locked memory file is
`project_no_sharded_apply`, last reaffirmed 2026-04-26 when Phase 13.3's
`RefCell + LocalSet` lockless-apply proposal was REJECTED.

## Why single-thread

The single-thread model is correctness-by-construction:

- **No locks on the hot path.** The apply thread owns the per-entity
  state map. No `Mutex<HashMap>`, no `RwLock`, no compare-and-swap on
  per-entity counters. The hot path is `entity_lookup → AggOp::update →
  return`, all on one thread.
- **Atomicity is free.** A push that updates 30 aggregations on one
  entity is atomic — no observer sees a half-updated entity, because the
  next read serializes against the same thread that did the write.
- **No coordination overhead.** Cross-thread synchronization is
  expensive. A modern x86 atomic compare-and-swap is ~10-25 ns; a
  contended cache line is 100+ ns. beava's hottest path runs at ~300-400
  ns/event, so coordination overhead would dominate.
- **Predictable latency.** No scheduler jitter, no work-stealing
  starvation, no priority inversion across threads. P99 latency is a
  function of the slowest single op, not of contention.

For higher per-instance throughput, beava expects users to run multiple
instances sharded at the entity-key level — the **Redis-cluster pattern**.
Each instance owns a slice of the key space; the SDK or a thin proxy
routes pushes/gets to the right instance. Phase 13.3's lockless-apply
proposal was rejected in favor of this — see
`project_no_sharded_apply` for the full reasoning.

Per-instance ceiling is workload-dependent but consistently in the 100k -
1M EPS range on Apple-M4 / Linux Xeon for fraud-shape workloads. The
fraud-team primary tuning bench measures **109,895 EPS** post-Phase-12.9
on M4 (median of 3 runs) — see
[`.planning/throughput-baselines.md`](../../.planning/throughput-baselines.md)
for the running history.

## Apply loop responsibilities

The apply thread runs a mio event loop that polls the TCP listener and
the HTTP listener (both data-plane). On each ready event:

1. **Receive frame.** mio reads the inbound frame from the socket.
2. **Parse.** TCP frames are `[u32 length][u16 op][u8 content_type][payload]`;
   HTTP requests are dispatched by route. The IoPool worker thread
   (Plan 18-04.8) eagerly deserialises push payloads into `Row` while
   the bytes are hot in L1, then hands the parsed row to the apply
   thread via the `MioClient.parsed_rows` side-channel — saves ~190 ns
   per push at parallel=4.
3. **Validate.** The row is checked against the registered schema for
   that event source.
4. **Apply.** For each derivation that indexes this event source, the
   matching `AggOp::update` runs on the per-entity state. All in
   process, all on this thread.
5. **WAL append.** The serialized record is appended to the in-memory
   WAL ring buffer. Lock-free memcpy + atomic position bump on the apply
   thread; the WAL writer thread fsyncs in the background.
6. **Reply.** The push acks once the WAL append has been acknowledged
   (acks=1 default). For HTTP/JSON pushes the reply is a JSON envelope;
   for TCP pushes it's an op-coded reply frame.

The dispatch entry point is
[`crates/beava-server/src/apply_shard.rs`](../../crates/beava-server/src/apply_shard.rs)::`dispatch_one`
(synchronous, no `.await`, no tokio dependency on the hot path).

## What the apply loop does NOT do

- **No async / no tokio in the data plane.** Per
  `project_phase18_no_dual_runtime` (locked Phase 12.6), the apply path
  is synchronous and runs on a hand-rolled mio event loop. The legacy
  axum data-plane was deleted in Phase 12.6 (~7,475 LOC removed). See
  [mio-data-plane.md](./mio-data-plane.md) for the full enforcement
  story.
- **No locks on hot path.** Per-entity state lives in a `HashMap` owned
  by the apply thread. Admin sidecar reads through an `Arc<AppState>` +
  uncontended `Mutex` (lock+unlock cost ~10-20 ns on macOS/Linux); reads
  are infrequent enough that the contention is negligible.
- **No cross-entity coordination.** Each push touches one entity per
  derivation. Cross-entity ops (`co_occurrence_count`, `graph_degree`,
  stream-stream joins on non-matching shard keys) are out of scope per
  the constraints in [`.planning/PROJECT.md`](../../.planning/PROJECT.md)
  § Out of Scope.
- **No fsync on the apply thread.** The WAL writer thread owns fsync.
  The apply thread only memcpy's into a ring buffer + bumps an atomic
  position. fsync latency does not block the apply loop.
- **No snapshot serialization on the apply thread.** The snapshot writer
  thread takes a copy-on-write reference to state and serializes
  off-path. See [wal-snapshot.md](./wal-snapshot.md).

## Auxiliary threads

The apply thread is the only thread that mutates state. These run beside
it:

| Thread             | Role                                                                | Lives in                              |
| ------------------ | ------------------------------------------------------------------- | ------------------------------------- |
| **Apply**          | mio event loop; reads, validates, dispatches, applies                | `apply_shard.rs::dispatch_one`        |
| **IoPool worker**  | Eagerly parses push frames into `Row` while bytes are L1-hot         | `server.rs::read_and_parse_client`    |
| **WAL writer**     | Drains the WAL ring buffer; fsyncs in background; advances LSN watermarks | `wal_writer.rs`                  |
| **Snapshot**       | Periodic snapshot serialization (default 30s)                        | `snapshot_writer.rs`                  |
| **Admin sidecar**  | tokio runtime serving `/health`, `/ready`, `/metrics`, `/registry`   | `http_admin.rs`                       |
| **Recovery (boot)**| Loads latest snapshot + replays WAL; runs once before serving begins | `recovery.rs`                         |

`recovery.rs` is the only other legitimate caller of
`apply_event_to_aggregations` besides the apply thread — see
[`CLAUDE.md` § mio-only Hot-Path Invariant](../../CLAUDE.md). All other
callers are forbidden by an architectural test (see
[mio-data-plane.md](./mio-data-plane.md)).

## Implications for users

- **Per-instance throughput is single-thread-bounded.** A 16-core box
  does not give you 16x apply throughput — it gives you the same apply
  throughput plus 15 cores you can use to run other things (or 16
  instances on different ports, sharded).
- **Sub-millisecond P99.** Single-thread + in-memory + no-coordination
  consistently delivers sub-millisecond P99 for batch-get on warm cache.
  See [`.planning/perf-baselines.md`](../../.planning/perf-baselines.md)
  for measurement.
- **Horizontal scale = more processes.** Run N beava instances; shard
  the entity key space across them; route pushes/gets via consistent
  hashing. Each instance is independent; no cross-process state.
- **Failure isolation = per-instance.** A crashed instance loses only
  its slice of state until WAL replay completes. Recovery (RTO target <
  30s on 10 GB state on NVMe) is per-instance.

## Cross-references

- [`CLAUDE.md` § mio-only Hot-Path Invariant](../../CLAUDE.md) — locked
  Phase 12.6 commitment; the architectural test that enforces it at CI.
- `~/.claude/projects/-Users-petrpan26-work-tally/memory/project_no_sharded_apply.md`
  — the locked single-thread commitment + the rejected Phase 13.3
  proposal.
- [mio-data-plane.md](./mio-data-plane.md) — mio runtime details +
  admin sidecar separation.
- [wal-snapshot.md](./wal-snapshot.md) — WAL writer thread + snapshot
  thread lifecycles.
- [memory-budget.md](./memory-budget.md) — per-instance memory ceiling
  + capacity math.
- [observability.md](./observability.md) — `/metrics` exposes per-loop
  counters: pushes, latency histograms, WAL fsync latency.
- [`.planning/throughput-baselines.md`](../../.planning/throughput-baselines.md)
  — running per-instance EPS history.
