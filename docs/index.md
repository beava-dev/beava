# beava Docs

> Real-time feature server for fraud, ad-tech, and behavioral analytics.

beava is a single-binary feature server. Push events in over HTTP, declare aggregations, query features by entity key. Per-instance: ≥3M events/sec/core for simple counters, ~6 KB per entity for the rich fraud-team shape, P99 batch-get under 10 ms (verified Phase 12.9 2026-05-03; v0 launch ship-pitch numbers).

## Quickstart

- [Quickstart](./quickstart.md) — `pip install "git+https://github.com/beava-dev/beava.git#subdirectory=python"` → first feature in 60 seconds

## Wire contract

- [docs/wire-spec.md](./wire-spec.md) — frame format + 6-opcode table + per-opcode JSON schemas
- [docs/http-api.md](./http-api.md) — verb-style POST routes for all 6 v0 endpoints + admin sidecar
- [examples/wire/schemas/](../examples/wire/schemas/) — 13 JSON Schema 2020-12 contract files
- [examples/wire/](../examples/wire/) — 20 worked-example fixtures (per opcode + global-table fixtures per ADR-003)

## SDK references

- [docs/sdk-api/shared.md](./sdk-api/shared.md) — cross-language semantic parity contract
- [docs/sdk-api/python.md](./sdk-api/python.md) — Python SDK (canonical implementation)
- [docs/sdk-api/typescript.md](./sdk-api/typescript.md) — TypeScript SDK
- [docs/sdk-api/go.md](./sdk-api/go.md) — Go SDK

## Pipeline DSL

- [docs/pipeline-dsl/overview.md](./pipeline-dsl/overview.md) — `@bv.event`, `@bv.table`, chain methods
- [docs/pipeline-dsl/expressions.md](./pipeline-dsl/expressions.md) — `bv.col` operator overloading + `bv.lit` literals
- [docs/pipeline-dsl/compilation-rules.md](./pipeline-dsl/compilation-rules.md) — Python source → JSON wire + ambiguity matrix

## Operator catalog

- [docs/operators/index.md](./operators/index.md) — full 54-op catalogue (53 unique kinds + ema alias inside ewma.md)
- [docs/operators/cost-class.md](./operators/cost-class.md) — per-op CPU tier metadata (Phase 19.2)
- Family overviews:
  - [Core (8)](./operators/core/index.md) — `count`, `sum`, `mean`, `min`, `max`, `var`, `std`, `ratio`
  - [Sketch (5)](./operators/sketch/index.md) — `n_unique`, `quantile`, `top_k`, `bloom_member`, `entropy`
  - [Point/ordinal (5)](./operators/point-ordinal/index.md) — `first`, `last`, `first_n`, `last_n`, `lag`
  - [Recency (10)](./operators/recency/index.md) — `streak`, `max_streak`, `negative_streak`, `first_seen`, `last_seen`, `age`, `has_seen`, `time_since`, `time_since_last_n`, `first_seen_in_window`
  - [Decay (6)](./operators/decay/index.md) — `ewma` (alias `ema`), `ewvar`, `ew_zscore`, `decayed_sum`, `decayed_count`, `twa`
  - [Velocity (9)](./operators/velocity/index.md) — `rate_of_change`, `inter_arrival_stats`, `burst_count`, `delta_from_prev`, `trend`, `trend_residual`, `outlier_count`, `value_change_count`, `z_score`
  - [Bounded buffers + Geo (11)](./operators/buffer-geo/index.md) — `histogram`, `hour_of_day_histogram`, `dow_hour_histogram`, `seasonal_deviation`, `event_type_mix`, `most_recent_n`, `reservoir_sample`, `geo_distance`, `geo_spread`, `geo_velocity`, `distance_from_home`

## Concepts

- [docs/concepts/events-vs-tables.md](./concepts/events-vs-tables.md) — `@bv.event` (immutable event source) vs `@bv.table` (aggregation output, per ADR-001)
- [docs/concepts/embed-mode.md](./concepts/embed-mode.md) — `bv.App()` no-URL embed mode (in-process for tests + small workloads)
- [docs/concepts/lifetime-aggregation.md](./concepts/lifetime-aggregation.md) — V0-MEM-GOV-02 register-time bound enforcement
- [docs/concepts/processing-time-only.md](./concepts/processing-time-only.md) — server-time only; no event-time, no joins, no watermarks
- [docs/concepts/global-aggregation.md](./concepts/global-aggregation.md) — global tables (no `key=`) for monitoring + dashboard use cases (per ADR-003)

## Architecture

- [docs/architecture/single-thread-apply.md](./architecture/single-thread-apply.md) — single-thread apply loop (per `project_no_sharded_apply`)
- [docs/architecture/mio-data-plane.md](./architecture/mio-data-plane.md) — mio is the sole data-plane runtime (per `project_phase18_no_dual_runtime`)
- [docs/architecture/wal-snapshot.md](./architecture/wal-snapshot.md) — WAL + snapshot durability
- [docs/architecture/memory-budget.md](./architecture/memory-budget.md) — verified ~6 KB / entity (post-Phase-12.9 boxing), 80 B AggOp size cap
- [docs/architecture/observability.md](./architecture/observability.md) — admin sidecar metrics (Prometheus on `/metrics`)

## Schema + errors

- [docs/schema-evolution.md](./schema-evolution.md) — additive default, `force=True` for destructive changes, `dry_run=True` flag
- [docs/error-codes.md](./error-codes.md) — alphabetical structured-code list + Python exception class hierarchy + HTTP status mapping

## Decisions (ADRs)

- [.planning/decisions/ADR-001-bv-table-partial-overturn.md](../.planning/decisions/ADR-001-bv-table-partial-overturn.md) — `@bv.table` aggregation-output revival (partial overturn of `project_v0_events_only_scope`)
- [.planning/decisions/ADR-002-polars-op-rename.md](../.planning/decisions/ADR-002-polars-op-rename.md) — Polars op renames (`avg→mean / variance→var / stddev→std / count_distinct→n_unique / percentile→quantile`)
- [.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md](../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md) — first-class global aggregation + public `bv.lit(value)` export

## Examples (runnable)

- Python: [examples/python/{adtech,fraud,ecommerce}.py](../examples/python/) (run with `python3`)
- TypeScript: [examples/typescript/{adtech,fraud,ecommerce}.ts](../examples/typescript/) (run with `npx tsx`)
- Go: [examples/go/{adtech,fraud,ecommerce}.go](../examples/go/) (run with `go run`)

Smoke test: `bash examples/test_examples.sh` — runs all 9 demos against language-local mocks (Python `MockApp` shim, TS+Go stubs).

Wire fixtures: [examples/wire/](../examples/wire/) — 20 JSON request + response + error fixtures per opcode (REGISTER / PUSH / GET / BATCH_GET / RESET / PING).

## Project commitments (locked)

- Single-thread data plane (per `project_no_sharded_apply`) — for higher throughput run multiple instances (Redis-cluster pattern)
- mio is the sole data-plane runtime; admin sidecar on tokio (per `project_phase18_no_dual_runtime`)
- Events-only with `@bv.table` aggregation-output exception (per `project_v0_events_only_scope` + ADR-001) — no `app.upsert/delete/retract` in v0
- Processing-time only; no event-time, no joins, no watermarks (per `project_redis_shaped_no_event_time_ever`)
- Memory governance: opt-in `cold_after=` TTL + lifetime aggregation contract (V0-MEM-GOV-01/02/03)
- 80 B `size_of::<AggOp>` cap (per Phase 12.9; CI tripwire enforced by `crates/beava-core/tests/per_entity_size_dump.rs::aggop_size_within_cap`)
- Polars op naming convention (per ADR-002)
- Global aggregation + `bv.lit` first-class (per ADR-003 — implementation in 13.4 + 13.5 + 13.6)

## Versioning

v0.0.0 ships from Phase 13.8 (packaging + GA tag) — currently in development. See [.planning/ROADMAP.md](../.planning/ROADMAP.md) for the full phase plan.

Active phase: **Phase 13.0 (design contract + spec docs) ✅ CLOSED 2026-05-03 (PASS)**. Next: 4-way parallel **Phase 13.4 (engine prep) + 13.5 (Python SDK + bench CLI) + 13.6 (TS + Go SDKs) + 13.7 (docs site)** → sequential **Phase 13.8 (packaging + GA tag)**.
