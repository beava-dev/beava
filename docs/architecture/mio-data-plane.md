# mio Data Plane

The data plane runs on a hand-rolled mio event loop. Every push, get,
batch_get, register, and ping flows through a single mio reactor on a
single OS thread. There is no tokio in the data path; tokio is restricted
to the admin sidecar on a separate port.

This is locked architecture, not provisional. The memory commitment is
`project_phase18_no_dual_runtime`, locked 2026-04-23 with v0-01.
The companion locks (`project_redis_shaped_no_event_time_ever`,
`project_no_sharded_apply`) reinforce the single-runtime, single-thread,
single-keyspace shape. Reviving a second data-plane runtime requires an
explicit user override and a new ADR.

## Why mio (not tokio for the data plane)

Three reasons it's mio, not tokio:

1. **Hand-rolled control over the apply path.** Every push goes through
   `apply_shard.rs::dispatch_one`. The mio reactor explicitly controls
   when bytes are read from the socket, when they're parsed, when the
   apply thread runs, and when the reply goes back. There is no
   scheduler picking when futures run; the loop is the scheduler.
2. **No async/await, no `'static` lifetime gymnastics on the hot path.**
   The hot path is synchronous. State references are short-lived and
   live in stack frames. No `Arc<Mutex<T>>` to satisfy a `Future`'s
   `'static` bound; no boxed-future indirection.
3. **Predictable performance.** mio gives raw `epoll` / `kqueue` semantics.
   Latency is bounded by the syscall + parse + apply path; there's no
   tokio task wakeup, no scheduler queue, no work-stealing.

v0 deleted the legacy axum data plane (~7,475 LOC). The
architectural test
[`crates/beava-server/tests/phase12_6_mio_only_dataplane.rs`](../../crates/beava-server/tests/phase12_6_mio_only_dataplane.rs)
walks the entire workspace at test runtime and fails CI if any forbidden
axum symbol (`axum::Router`, `axum::Json`, `axum::Extension`,
`axum::extract`, `axum::routing`, `axum::http`, `axum::body`,
`axum::response`, `axum::middleware`) appears outside
`crates/beava-server/src/http_admin.rs`. The companion test
`phase12_6_legacy_axum_killed.rs` asserts the deleted axum-data-plane
files stay deleted.

## What mio does

The reactor lives in
[`crates/beava-server/src/server.rs`](../../crates/beava-server/src/server.rs)
(the `ServerV18` data plane) and the `IoPool` worker
(`server.rs::read_and_parse_client`). On each tick:

1. **Poll** the registered TCP listener + HTTP listener for ready I/O.
2. **distribute_reads** — read available bytes off ready sockets into
   per-client buffers; wake any client with a complete frame.
3. **join** — wait for IoPool worker threads to finish parsing
   push-frame bodies (an internal plan.8 fast path; saves ~190 ns/push at
   parallel=4 by overlapping parse with read).
4. **apply** — for each parsed `WireRequest`, call
   `apply_shard.rs::dispatch_one` synchronously. The dispatch matches
   on the request variant (Push / Get / BatchGet / Register / Ping /
   Reset) and runs the corresponding handler against `Arc<AppState>`.
5. **distribute_writes** — push reply frames into per-client write
   buffers; wake any client whose socket is writable.
6. **join** — wait for the WAL writer to ack the LSN watermark for any
   outbound push reply (acks=1 default).

The serve loop in `serve_with_dirs` runs this tick continuously until
shutdown. The whole thing is one OS thread.

## The single dispatch entry point

All data-plane requests funnel through one function:

[`crates/beava-server/src/apply_shard.rs::dispatch_one`](../../crates/beava-server/src/apply_shard.rs)

It matches on `WireRequest` variants and routes to the per-op handler.
There is no other entry point on the data plane. The architectural test
asserts this — the only other legitimate caller of
`apply_event_to_aggregations` is `recovery.rs` (cold-path WAL replay on
boot), and it's allowlisted by name.

```rust
// from apply_shard.rs::dispatch_one
fn dispatch_one(&self, req: WireRequest, pre_parsed_row: Option<Row>) -> GlueResponse {
    match req {
        WireRequest::Ping => GlueResponse::Pong { ... },
        WireRequest::Register(...) => self.dispatch_register_sync(...),
        WireRequest::HttpPush(...) | WireRequest::TcpPush(...) => {
            self.dispatch_push_sync(req, pre_parsed_row)
        }
        WireRequest::HttpGet(...) | WireRequest::TcpGet(...) => self.dispatch_get_sync(...),
        WireRequest::HttpBatchGet(...) | WireRequest::TcpBatchGet(...) => {
            self.dispatch_batch_get_sync(...)
        }
        // ... reset, etc.
    }
}
```

If a future plan attempts to add a new data-plane caller of
`apply_event_to_aggregations` outside `apply_shard.rs` or `recovery.rs`,
the `phase12_6_mio_only_dataplane` test will fail in CI.

## Admin sidecar (tokio + axum)

The admin sidecar is the **only** place axum lives in the workspace.
It serves four endpoints on a separate port (`cfg.admin_addr`,
typically the data-plane port + 1):

- `GET /health` — cheap liveness probe. 200 when the server is up.
- `GET /ready` — readiness probe. 200 when recovery has completed; 503
  during recovery.
- `GET /metrics` — Prometheus exposition format.
- `GET /registry` — current registry version + node count.

Implementation:
[`crates/beava-server/src/http_admin.rs`](../../crates/beava-server/src/http_admin.rs).

The sidecar is bound on its own port so admin probes don't interfere
with the data plane (and vice versa). It reads through an
`Arc<RwLock<RegistrySnapshot>>` — read-only view of registry metadata,
no write-back path. Updates flow from the apply thread to the snapshot
on every successful register.

A small middleware tags every admin response with `X-Runtime: tokio` so
operators can confirm at the wire which runtime served a given response.
Data-plane responses get `X-Runtime: hand-rolled`.

## How enforced

Two CI tests lock this architecture in place:

1. **[`crates/beava-server/tests/phase12_6_mio_only_dataplane.rs`](../../crates/beava-server/tests/phase12_6_mio_only_dataplane.rs)** —
   walks the workspace at test runtime; fails if any forbidden axum
   symbol appears outside `http_admin.rs`, OR if any new caller of
   `apply_event_to_aggregations` appears outside `apply_shard.rs` or
   `recovery.rs`.
2. **[`crates/beava-server/tests/phase12_6_legacy_axum_killed.rs`](../../crates/beava-server/tests/phase12_6_legacy_axum_killed.rs)** —
   asserts the legacy axum data-plane files (deleted in v0)
   stay deleted; new files matching the legacy paths fail CI.

Both tests run on every PR via `cargo test --workspace`.

## Implications for contributors

- **New data-plane endpoints** go through `apply_shard.rs::dispatch_one`,
  not through axum. Add a new `WireRequest` variant; add a new dispatch
  arm; thread the handler.
- **New admin endpoints** can stay on axum in `http_admin.rs`. Don't
  add them to the data-plane router.
- **No `axum::*` imports** outside `http_admin.rs`. CI catches this.
- **No new caller** of `apply_event_to_aggregations` outside the two
  allowlisted call sites. CI catches this too.

## Cross-references

- [`CLAUDE.md` § mio-only Hot-Path Invariant (locked v0)](../../CLAUDE.md)
  — the canonical invariant block.
- `~/.claude/projects/-Users-petrpan26-work-tally/memory/project_phase18_no_dual_runtime.md`
  — the locked architectural commitment.
- [single-thread-apply.md](./single-thread-apply.md) — the single OS
  thread that runs the apply loop.
- [observability.md](./observability.md) — admin sidecar endpoints in
  detail.
- [`crates/beava-server/src/apply_shard.rs`](../../crates/beava-server/src/apply_shard.rs)
  — single dispatch entry point.
- [`crates/beava-server/src/http_admin.rs`](../../crates/beava-server/src/http_admin.rs)
  — admin sidecar implementation.
- [../wire-spec.md](../wire-spec.md) — TCP frame format + opcode table
  (the data plane's wire contract).
