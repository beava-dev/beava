# Changelog

All notable changes to Beava are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.6] - 2026-05-29

Resolves a SEV-1 reported by a downstream consumer: HTTP serving (including
`POST /ping`) stalled for 60–90 s during every snapshot write, tripping
docker healthchecks into a restart loop.

### Fixed

- **Snapshot writes no longer block the data plane.** Snapshots now run in a
  forked child via copy-on-write (Valkey BGSAVE pattern, default on unix); the
  apply thread holds the state lock only for the `fork()` syscall, so `/ping`
  and all HTTP/TCP requests stay responsive while a multi-hundred-MB snapshot
  is written. Set `BEAVA_SNAPSHOT_FORK=0` to fall back to the in-process path.
- **WAL truncation reclaims on-disk bytes.** Compaction rewrites the segment to
  drop snapshot-covered records instead of logging "truncated" while the file
  kept growing.
- **`POST /push` rejects control characters in string fields** (HTTP 400,
  `control_character_in_string`) so corrupt bytes can't poison the WAL and
  recur as a decode WARN on every restart.
- Hardened WAL recovery: applied-watermark replay, forced re-register on
  schema gaps, bounded fork-child reaping, and EINTR-safe fork wait.

### Added

- **Configurable snapshot cadence** — `BEAVA_SNAPSHOT_INTERVAL_MS`,
  `BEAVA_SNAPSHOT_MIN_EVENTS` (Redis-style conditional snapshot), and
  `BEAVA_SNAPSHOT_FORK`.
- Snapshot metrics on the admin `/metrics` endpoint: last duration, bytes,
  and fsync time.

### Docs

- Install docs feature `pip` / `brew` / `docker` as the three primary paths.
- Corrected homepage, quickstart, and field-guide snippets to match the
  shipped SDK and HTTP API; fixed homepage mobile layout overflow.

## [0.0.4] - 2026-05-13

First release with behavioural fixes since v0.0.0. Three operator bugs that
mis-routed data through the engine + SDK are gone; the rest of the release
is regression coverage so the bug class can't return.

### Fixed

- **`WindowedOp::update_at` honours the dispatcher's `pre_val`.** Windowed
  field-bearing ops (`mean`/`avg`/`sum`/`top_k`/`quantile` with `window=`)
  were re-extracting from `extracted[field_idx]` instead of using the
  `pre_val` the outer apply-loop had already resolved via the agg-local →
  union-index remap. End-to-end symptoms: `mean("price", window="…")`
  returned `Null` and `top_k("category", window="…")` returned a different
  field's distribution whenever a prior windowed op in the same agg
  referenced a different field. Fix routes `pre_val` through the windowed
  arm.
- **`EventTypeMixState::update_at` honours the dispatcher's `pre_val`.**
  Same bug class as the windowed fix; the second `update_at` arm that
  discarded `pre_val` and re-extracted from the wrong index space.
- **SDK `~bv.col(x)` emits `(not x)`, not `!(x)`.** The server's
  where-parser rejects bare unary `!` (`unexpected character '!'`) but
  accepts the `not` keyword. Every register that used the idiomatic
  `~bv.col(...)` inside `where=` had been failing with
  `aggregation_invalid_where`.

### Tests added (33 regression tests across five files)

- `crates/beava-core/tests/windowed_op_uses_caller_pre_val.rs` — 4 tests.
- `crates/beava-core/tests/field_ordering_invariant.rs` — 4 tests covering
  multi-agg reversed field order, non-overlapping subsets,
  top_k-after-mean stacking, geo lat/lon position swaps.
- `crates/beava-core/tests/agg_combinations_matrix.rs` — 12 tests covering
  (where × window × field) 2^3 matrix and 3+ mixed windowed ops in one
  agg.
- `crates/beava-server/tests/wire_roundtrip_parity.rs` — 3 tests asserting
  HTTP-JSON and TCP-msgpack produce identical state for a complex schema.
- `python/tests/test_op_roundtrips_extended.py` +
  `test_col_overload_server_acceptance.py` + `internal/test_col_invert.py`
  — 18 Python tests covering var / std / percentile / top_k / decay / geo
  / event_type_mix end-to-end plus `bv.col` overload server-acceptance.

### Examples

- `examples/python/agent_runtime.py` reverts the `bv.col("ok") == False`
  workaround back to idiomatic `~bv.col("ok")`.
- `marketplace_rerank.py` no longer returns `None` for
  `avg_view_price_30m` and surfaces `'watches'` for `top_category_30m`
  (it had been a corrupted price distribution pre-fix).

## [0.0.3] - 2026-05-12

Syncs the Rust workspace version with the Python package version so
`beava --version` reports the same string as `pip show beava`. Otherwise
source-identical to v0.0.2.

## [0.0.2] - 2026-05-12

Homebrew install path. Wires per-platform tarballs into the release pipeline
so the `homebrew-bump.yml` workflow can bump `Formula/beava.rb` cleanly.
Also syncs the Rust crate version with the Python package version so
`beava --version` matches the pip metadata. Otherwise source-identical to
v0.0.1.

## [0.0.1] - 2026-05-12

First release published to PyPI. Source-identical to v0.0.0; the version bump
exists so the inaugural PyPI publish can claim the `beava` project name
under the Trusted-Publisher OIDC flow enabled in this release cycle.

## [0.0.0] - 2026-05-12

Initial public release. Beava is a single-binary real-time feature server for fraud,
ad-tech, and behavioral analytics — declare a feature, push events, query it, in
under ten minutes, with `curl` alone.

### Added

- Single-binary real-time feature server with HTTP/1.1 + framed-TCP wire
  transports; in-memory state, WAL + periodic snapshot durability, sub-millisecond
  batch-get.
- 54 aggregation operators across 7 families: core, sketch, point-ordinal, recency,
  decay, velocity, and buffer-geo.
- Python SDK with declarative `@bv.event` decorator and pipeline DSL —
  `events.with_columns(...).group_by(K).agg(...)`.
- `@bv.table` derivation outputs for aggregation results (output-only; no
  upsert / delete / retract).
- TypeScript SDK (`@beava/sdk`) — communicate-only: push, register, get, batch-get.
- Go SDK (`github.com/beava-dev/beava/sdk/go`) — communicate-only.
- Schema evolution flags: `force=True` for destructive register with a structured
  diff payload, and `dry_run=True` for preview without applying.
- Three demo workloads bundled with the Python SDK: `bv.demo("adtech")`,
  `bv.demo("fraud")`, `bv.demo("ecommerce")`.
- `beava bench` CLI with four modes: throughput, mixed, memory, fsync.
- Global aggregations (no `key=` form) and `bv.lit(value)` literal expression.
- Cold-entity TTL — opt-in lazy eviction via
  `@bv.event(cold_after='<duration>')`.
- Memory governance — every lifetime aggregation operator declares a finite
  per-entity ceiling at register time; unbounded ops are rejected at register.
- In-memory persistence backend for ephemeral and test workloads.
- `OP_RESET` (0x0040) opcode and `POST /reset` route, gated on test-mode
  (environment, config, or SDK kwarg).
- Documentation site at [beava.dev](https://beava.dev) — quickstart, operator
  catalogue, wire spec, Python / TypeScript / Go SDK reference, and ADR archive.

### Architecture

- Hand-rolled mio-only single-threaded data plane (Redis-shaped); horizontal
  scaling via multi-instance entity-key sharding.
- Apache-2.0 OSS license.

### Removed

- Tables (upsert / delete / retract API), MVCC temporal store, joins, unions,
  session windows, and event-time / watermark semantics — descoped to v0.1+.
- `event_time_ms` field on the wire — Beava is processing-time only in v0.

### Security

- OWASP Top-10 and LLM Top-10 review against the v0 attack surface.
- ASVS L1 threat model — single-tenant; operator owns network isolation; no
  auth in v0.
- `cargo audit` + `cargo deny` policy enforced at workspace root.

### Performance

- ≥3M events/sec/core sustained on representative fraud workloads (single-thread
  mio data plane).
- P99 batch-get latency under 10 ms warm.
- Per-entity memory budget under 7 KB for a typical 30-feature pack.

[Unreleased]: https://github.com/beava-dev/beava/compare/v0.0.0...HEAD
[0.0.0]: https://github.com/beava-dev/beava/releases/tag/v0.0.0
