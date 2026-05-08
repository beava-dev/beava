# Observability

The admin sidecar exposes four endpoints on a separate port
(`cfg.admin_addr`) for ops monitoring. The sidecar is the only place
tokio + axum live in the workspace; the data plane stays on
hand-rolled mio. Endpoints are read-only over the registry snapshot;
they cannot modify state.

This page documents the four endpoints, the Prometheus metric
families exposed at `/metrics`, the structured-log shape, and the
trace-id propagation contract.

## Overview

| Endpoint | Method | Purpose |
| ------------ | ------ | ---------------------------------------------------- |
| `/health` | GET | Liveness probe. 200 if the server process is up. |
| `/ready` | GET | Readiness probe. 200 only after recovery completes. |
| `/metrics` | GET | Prometheus exposition format. All families in one scrape. |
| `/registry` | GET | Current registry version + node count + (debug) snapshot. |

All four endpoints respond with `X-Runtime: tokio` so operators can
verify which runtime served the response (data-plane responses get
`X-Runtime: hand-rolled`).

Implementation: [`crates/beava-server/src/http_admin.rs`](../../crates/beava-server/src/http_admin.rs).

## `/health` — liveness

Cheap liveness probe. Returns `200 OK` with body `{"status": "ok"}`
when the admin sidecar is running. Does NOT confirm registry is loaded;
does NOT confirm WAL replay is complete.

Use this for:
- Kubernetes `livenessProbe` (restart on failure).
- Load-balancer node-up checks (route traffic away if 5xx).
- Smoke tests in CI.

Do NOT use this for routing real traffic — `/health` returns 200 even
during recovery, when the data plane isn't accepting pushes yet. Use
`/ready` for that.

## `/ready` — readiness

Returns `200 OK` with body `{"status": "ready"}` once `ServerV18::bind`
returns (recovery complete; WAL replayed; apply thread polling). Returns
`503 Service Unavailable` during recovery.

Use this for:
- Kubernetes `readinessProbe` (route traffic only when ready).
- Load-balancer ready-for-traffic checks.

The sidecar is bound and serving `/health` + `/ready` immediately on
process start, but `/ready` doesn't flip to 200 until recovery completes.
This gives orchestrators a clean signal for "the process is up but not
yet handling requests."

## `/metrics` — Prometheus exposition

Prometheus exposition format (`text/plain; version=0.0.4`). All metric
families in one scrape. Counters monotonically increase; gauges sample
live; histograms expose `_bucket`, `_sum`, `_count`.

### v0-01 metric families

(Land in v0 alongside the verb-route rename.)

- `beava_register_total` (counter) — count of register operations
  (success + fail; labeled by status).
- `beava_push_total` (counter) — count of push operations
  (success + reject; labeled by event source + status).
- `beava_push_latency_seconds` (histogram) — push apply-path latency
  buckets.
- `beava_get_latency_seconds` (histogram) — get / batch_get latency
  buckets.

### memory-governance metric families (5)

Land at v0 an internal plan. All five are aggregate (no per-source
labels in v0; per-source labels deferred to v0.0.x per an internal plan
PLANNER-SURFACED CONCERN 3):

- **`beava_cold_entity_evictions_total`** (counter) — V0-MEM-GOV-01
  cold-TTL evictions fired. Increments per entity evicted via
  `cold_after=` lazy expiry.
- **`beava_lifetime_op_cap_hit_total`** (counter) — V0-MEM-GOV-02
  cap-hit events: entropy categories capped, plus future top_k /
  histogram cap-hits as those are wired.
- **`beava_entity_count_resident`** (gauge) — resident entity count
  snapshot. Sampled (not real-time) per v0 an internal plan to avoid
  O(N_tables) read on every `/metrics` scrape.
- **`beava_bucket_reclaim_total`** (counter) — V0-MEM-GOV-03 per-event
  bucket reclaims. Increments on `WindowedOp::evict_oldest_bucket`
  firings.
- **`beava_bytes_per_entity_p99`** (gauge) — currently a static 7000
  placeholder per v0 an internal plan PLANNER-SURFACED CONCERN.
  Dynamic sampling (~30 LOC in `agg_state.rs::EntityCountResidentSnapshot`)
  is deferred to v0 / v0.0.x. The post-Phase-12.9 actual
  fraud-team weighted-avg is ~6 KB so the static value is no longer
  misleading, just not informative.

See [memory-budget.md](./memory-budget.md) for the V0-MEM-GOV
invariants the metrics observe.

### WAL / snapshot metric families

- `beava_wal_append_latency_seconds` (histogram) — WAL append
  latency on the apply thread.
- `beava_snapshot_latency_seconds` (histogram) — snapshot writer
  serialization latency (off the apply thread).
- `beava_wal_synced_lsn` / `beava_wal_committed_lsn` /
  `beava_wal_acked_lsn` (gauges) — four-watermark LSN values per
  v0 WAL design.

### Runtime identity

- `beava_runtime_kind` (gauge, value=1 with label
  `kind="hand-rolled"` or `"tokio"`) — an internal plan.6 Task 4.6.5;
  identifies which runtime is serving the data plane.

### Op-specific metric families

- `beava_entropy_categories_capped_total` (counter) — v0
  D-05a; entropy op cap-hit events.
- (More land per phase; see `metrics_handler` in
  [`crates/beava-server/src/http_admin.rs`](../../crates/beava-server/src/http_admin.rs)
  for the live list.)

## `/registry` — registry snapshot

Returns the current registry version + node count as JSON:

```json
{
  "version": 42,
  "node_count": 14
}
```

Optional `?version=N` for historical version inspection (debugging).

Use this for:
- Confirming a register call landed (version bumps on success).
- Diffing registry state between two beava instances during a rollout.
- Debugging "did my last register actually take effect?" questions.

This endpoint reads through the same `SharedRegistrySnapshot` the
data-plane apply thread updates on every successful register; reads are
non-blocking via `RwLock`.

## Logs

Structured JSON log lines at `INFO` / `WARN` / `ERROR`. Standard fields:

- `timestamp` (ISO 8601, microsecond resolution)
- `level` (`INFO` / `WARN` / `ERROR`)
- `target` (Rust log target, typically the module path)
- `message` (human-readable summary)
- Per-event fields (event source, registry version, LSN, etc.)

Logs go to stdout by default; the `subprocess.Popen` in
[embed mode](../concepts/embed-mode.md) captures them and surfaces them
through Python `logging`.

## `X-Trace-Id` propagation

All HTTP responses (admin + data-plane) propagate the inbound
`X-Trace-Id` header back in the response. Logs emitted during the
request include the trace id when present. For TCP frames, the trace
id can be carried in the optional metadata field (see
[../wire-spec.md](../wire-spec.md)) but is not required.

This gives operators end-to-end correlation: trace id stamped at the
edge → admin endpoint trace id matches → log lines all carry the same
trace id → easy grep across services.

## Implementation notes

The 5 v0 metric families use **process-static atomic counters**
(`AtomicU64` / `AtomicUsize` with `inc()` / `count()` shape) instead of
plumbing through `AdminState`. This was a an internal plan Rule 3 auto-fix —
process-static avoids the 10-file cascade that explicit plumbing would
have required. The pattern follows v0 D-05a's
`EntropyStateWrap::categories_capped_count` precedent.

The middleware that stamps `X-Runtime: tokio` is in
`http_admin.rs::stamp_tokio_header` (axum middleware function). The
data-plane equivalent (stamping `hand-rolled`) lives in the mio reply
path in `apply_shard.rs`.

## Cross-references

- [`crates/beava-server/src/http_admin.rs`](../../crates/beava-server/src/http_admin.rs)
  — admin sidecar implementation (admin_router, handlers, middleware).
- [mio-data-plane.md](./mio-data-plane.md) — the admin sidecar's
  separation from the data plane.
- [memory-budget.md](./memory-budget.md) — the V0-MEM-GOV invariants
  the 5 v0 metrics observe.
- [wal-snapshot.md](./wal-snapshot.md) — the WAL / snapshot metrics
  context (four-watermark LSNs).
- [`CLAUDE.md` § Memory Governance Invariant](../../CLAUDE.md) — the
  V0-MEM-GOV contract the metrics surface.
- [`.planning/REQUIREMENTS.md`](../../.planning/REQUIREMENTS.md)
  V0-MEM-GOV-01 / 02 / 03 — the requirements the metrics fulfill.
