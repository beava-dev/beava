# Changelog

All notable changes to Beava are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
