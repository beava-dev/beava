# beava HTTP API

> **Status:** Authoritative for v0. Documents the **post-13.4 target** route
> table — verb-style POST + JSON body for all 6 data-plane operations.
> **Last reviewed:** 2026-05-03 (Phase 13.0).

## Overview

beava ships HTTP/1.1 + JSON as its primary data-plane transport so that any
HTTP-speaking client — `curl`, browser fetch, a Lua-scripted load balancer, a
WAF rewrite rule — can drive the server with no SDK. JSON is the only wire
content-type on HTTP in v0; MessagePack is reserved for the framed-TCP
fast-path only.

All 6 v0 data-plane operations are exposed as **verb-style POST routes** with a
JSON request body. There is no GET-with-query-string path for `/get` or any
other lookup; lookup arguments live in the request body. This convention
matches Polars, DuckDB, and other contemporary devex-first analytic tools where
the noun (`/get`, `/push`) names what is happening and the body carries the
structured arguments. It also keeps the HTTP route table identical to the TCP
opcode table — one transport reads as a literal translation of the other.

The full route table is:

| Method | Path | Wire opcode | Purpose |
|--------|------|-------------|---------|
| POST | `/register` | `OP_REGISTER` (`0x0001`) | Register descriptors. |
| POST | `/push` | `OP_PUSH` (`0x0010`) | Push one event. |
| POST | `/get` | `OP_GET` (`0x0020`) | Single-row feature read. |
| POST | `/batch_get` | `OP_BATCH_GET` (`0x0024`) | Heterogeneous batch read. |
| POST | `/reset` | `OP_RESET` (`0x0040`) | Wipe state + WAL (test fixture). |
| POST | `/ping` | `OP_PING` (`0x0000`) | Health probe / version discovery. |

The **admin sidecar** is a separate axum server on a separate port
(`cfg.admin_addr`) per the [Phase 12.6 mio-only invariant](../CLAUDE.md). It
exposes 4 `GET` endpoints — `/health`, `/ready`, `/metrics`, `/registry` — and
is the only place where tokio + axum touch the runtime. The data-plane port
itself is hand-rolled mio + non-blocking I/O. See
[Admin sidecar endpoints](#admin-sidecar-endpoints-separate-port-get-shaped)
below.

This doc is the **HTTP transport** spec; the body shapes (request and response
schemas, error envelope, opcode-level semantics) live in
[`docs/wire-spec.md`](wire-spec.md). Every route listed here is a verb-form of
an opcode in the wire spec, and each route description below cross-links to the
matching opcode section.

### A note on the post-13.4 target state

The current code (post-12.7) uses event-name-suffixed routes for push:
`POST /push/{event_name}`. This is a Phase 12.6 carry-over. Phase 13.4 (engine
prep) renames the routes mechanically to the verb-style form documented in this
doc — `POST /push` with `event_name` in the body. Phase 13.0 (this phase)
declares the **target** state so that downstream SDK ports (Phase 13.5 Python,
Phase 13.6 TS + Go) can author against a stable contract while the engine
rename ships in parallel. After Phase 13.4 lands, this doc and the engine match
exactly. See the [Note on event-name routing](#note-on-event-name-routing)
section at the bottom for the migration rationale.

## Authentication and headers

beava v0 ships **unauthenticated**. The OSS launch is intentionally an
"unauthenticated single-process server" — operators front it with whatever
auth proxy they use elsewhere (an internal LB, a Kong / Envoy / nginx in front,
a service mesh, etc.). v0.1+ may grow opinions on auth; v0 has none.

| Header | Required? | Notes |
|--------|-----------|-------|
| `Content-Type: application/json` | **Required** on all data-plane endpoints | Mismatch returns `415 Unsupported Media Type` with structured code `unsupported_content_type`. |
| `Accept: application/json` | Optional | If present, MUST include `application/json`; otherwise `406 Not Acceptable`. Default behaviour treats absent `Accept` as accept-anything per RFC 7231. |
| `X-Trace-Id` | Optional | Propagated to server logs. Useful for stitching distributed traces. v0 does NOT generate one server-side. |
| `X-Request-Id` | RESERVED | v0 is correlation-free per Redis-style strict-FIFO. Reserved for future async correlation in v0.1+. |
| `Host` | Required by HTTP/1.1 | Standard. v0 does not vhost. |

There are **no CORS headers** in v0. The data-plane is a single-origin server
inside a private network; cross-origin browser fetch is v0.1+ territory. If
your client does need CORS, terminate it at your reverse proxy.

## Data-plane endpoints

The 6 sections below document each verb-style route. Bodies, schemas, and
worked examples live in [`docs/wire-spec.md`](wire-spec.md); this doc covers
the HTTP-level surface (route + status codes + curl invocation).

### POST /register

| Field | Value |
|-------|-------|
| Method | POST |
| Path | `/register` |
| Wire opcode | `OP_REGISTER` (`0x0001`) — see [wire-spec § OP_REGISTER](wire-spec.md#op_register-0x0001) |
| Content-Type | `application/json` |
| Auth | None (v0) |

**Request body:** see the wire-spec
[`OP_REGISTER` request schema](wire-spec.md#op_register-0x0001). The full JSON
Schema lives at
[`examples/wire/schemas/register.request.schema.json`](../examples/wire/schemas/register.request.schema.json).

The body declares one or more **descriptors**, each disambiguated by a `kind`
discriminator (`event` | `table` | `derivation`). Per
[ADR-001](../.planning/decisions/ADR-001-bv-table-partial-overturn.md),
`kind: "table"` is the **aggregation-output** form — there is no
`app.upsert` / `app.delete` / `app.retract` path for these tables in v0; they
are populated only by upstream aggregation derivations. Per
[ADR-002](../.planning/decisions/ADR-002-polars-op-rename.md), aggregation op
names use the new Polars conventions (`mean`, `var`, `std`, `n_unique`,
`quantile`).

Top-level flags:

- `force=true` — accept destructive schema changes (e.g. type change on a
  field). Default is `false`; destructive changes are rejected with
  `registration_conflict`.
- `dry_run=true` — run the validator and compute the diff without applying.
  Response carries `added` / `removed` / `changed` arrays; `registry_version`
  is unchanged.

**Response body (success):** see the wire-spec
[`OP_REGISTER` response schema](wire-spec.md#op_register-0x0001). The full JSON
Schema lives at
[`examples/wire/schemas/register.response.schema.json`](../examples/wire/schemas/register.response.schema.json).

The success response carries `{status, registry_version, added, removed?, changed?, diff?}`.
`registry_version` is monotonic; clients use it as a cache key for
schema-dependent state.

**HTTP status codes:**

| Status | When |
|--------|------|
| 200 | Success — descriptors applied (or dry-run validated). |
| 400 | Validation error: `schema_invalid`, `unknown_op`, `cycle`, `missing_upstream`, `unsupported_node_kind`. |
| 404 | Path not found (rare — implies route table mismatch). |
| 409 | `registration_conflict` — destructive change without `force=true`. |
| 415 | Wrong `Content-Type` — must be `application/json`. |
| 500 | Server error during registration commit (rare; usually I/O on snapshot). |

**Curl example:**

```bash
curl -X POST http://localhost:8081/register \
  -H 'Content-Type: application/json' \
  -d @examples/wire/register-fraud-team.request.json
```

**Errors specific to this endpoint:**

| Code | When | Recovery |
|------|------|----------|
| `unsupported_node_kind` | Body has `kind="upsert"`, `kind="delete"`, etc. — pre-12.7 surface that is permanently killed per `project_v0_events_only_scope`. | Use `kind=event`, `kind=table` (aggregation-output only per ADR-001), or `kind=derivation`. |
| `registration_conflict` | Field type changed without `force=true`. | Re-issue with `force=true` if intentional (zeroes affected aggregations); otherwise revert the descriptor change. |
| `schema_invalid` | Descriptor missing required field, wrong type, or violates structural constraints. | Fix the descriptor against the JSON Schema. |
| `cycle` | Descriptor list forms a cycle through `upstreams`. | Break the cycle in the upstream graph. |
| `missing_upstream` | A `derivation` references an `upstream` not declared in this batch and not previously registered. | Add the missing upstream to the same register call, or register it first. |
| `unknown_op` | `agg.<feature>.op` references a name not in the operator catalogue. | Use one of the 53 catalogued ops; per ADR-002, prefer Polars names (`mean` not `avg`). |

See [docs/error-codes.md](error-codes.md) for the alphabetical structured-code
list with full HTTP status mapping (Plan 13.0-12 — forward reference).

### POST /push

| Field | Value |
|-------|-------|
| Method | POST |
| Path | `/push` |
| Wire opcode | `OP_PUSH` (`0x0010`) — see [wire-spec § OP_PUSH](wire-spec.md#op_push-0x0010) |
| Content-Type | `application/json` |
| Auth | None (v0) |

**Request body:** see the wire-spec
[`OP_PUSH` request schema](wire-spec.md#op_push-0x0010). The full JSON Schema
lives at
[`examples/wire/schemas/push.request.schema.json`](../examples/wire/schemas/push.request.schema.json).

The body shape is `{event_name, fields: {...}}` in the post-13.4 verb-style
form. The `event_name` field MUST match a registered `@bv.event` source; the
`fields` object MUST match its declared schema (same field names, compatible
types). Type coercion on the JSON boundary is allowed in v0 — string `"42"`
for an `i64` field is accepted (the JSON-to-Rust coercer handles common
mismatches). Strict-mode rejection is v0.1+.

> **Pre-13.4 form:** the current engine accepts `POST /push/{event_name}` with
> body `{fields: {...}}` (no `event_name` key — it is in the URL path). Phase
> 13.4 mechanically renames to the verb-style form documented above. SDKs ship
> against the post-13.4 form from day one (Plan 13.5 Python, Plan 13.6 TS + Go).

**Response body (success):** see the wire-spec
[`OP_PUSH` response schema](wire-spec.md#op_push-0x0010). The full JSON
Schema lives at
[`examples/wire/schemas/push.response.schema.json`](../examples/wire/schemas/push.response.schema.json).

The success response carries `{ack_lsn, registry_version}`. `ack_lsn` is the
server-assigned monotonic Log Sequence Number. v0 push is **`acks=1`** per
Phase 6.1 (`SyncMode::Periodic`) — the response returns after the WAL append
returns success, before the periodic fsync flushes to disk. This is the
intentional latency / durability tradeoff for v0; `acks=all` is reserved for
v0.1+ via the wire-level opcode `OP_PUSH_SYNC = 0x0011`.

**HTTP status codes:**

| Status | When |
|--------|------|
| 200 | Success — event accepted into the WAL. |
| 200 + `idempotent_replay: true` | A prior push with the same `dedupe_key` within the registered `dedupe_window` matched; response repeats the prior `ack_lsn`. |
| 400 | Validation error: `schema_mismatch`, `missing_field`, `validation_failed`. |
| 404 | `unknown_event` — the `event_name` is not registered. |
| 415 | Wrong `Content-Type`. |
| 500 | WAL I/O failure (rare; usually disk-full). |

**Curl example:**

```bash
curl -X POST http://localhost:8081/push \
  -H 'Content-Type: application/json' \
  -d '{"event_name": "Txn", "fields": '"$(cat examples/wire/push-success.request.json | jq .fields)"'}'
```

Or, when the `event_name` is already inlined in the fixture (post-13.4
fixtures bundle the field):

```bash
curl -X POST http://localhost:8081/push \
  -H 'Content-Type: application/json' \
  -d @examples/wire/push-success.request.json
```

> Note: `examples/wire/push-success.request.json` currently carries only the
> `fields` object (matching the wire-level body which routes `event_name`
> separately on TCP). The HTTP transport in the post-13.4 target form expects
> `event_name` inside the body; SDKs synthesise that from their `app.push("Txn", {...})`
> call. The two-line `jq` example above demonstrates the manual transform.

**Errors specific to this endpoint:**

| Code | When | Recovery |
|------|------|----------|
| `unknown_event` | `event_name` is not a registered `@bv.event` source. | Register the event source first (see `/register`); check spelling. |
| `schema_mismatch` | A field has the wrong type and cannot be coerced (e.g., string `"abc"` for an `f64` field). | Fix the field's type at the source. |
| `missing_field` | A required field is missing from `fields`. | Send all required fields per the registered schema. |
| `validation_failed` | A custom validator on the event source rejected the payload. | Read the `path` + `message` for the specific constraint. |

### POST /get

| Field | Value |
|-------|-------|
| Method | POST |
| Path | `/get` |
| Wire opcode | `OP_GET` (`0x0020`) — see [wire-spec § OP_GET](wire-spec.md#op_get-0x0020) |
| Content-Type | `application/json` |
| Auth | None (v0) |

**Request body:** see the wire-spec
[`OP_GET` request schema](wire-spec.md#op_get-0x0020). The full JSON Schema
lives at
[`examples/wire/schemas/get.request.schema.json`](../examples/wire/schemas/get.request.schema.json).

The body shape is `{table, key, features?}`:

- `table` is the table name registered with `OP_REGISTER`.
- `key` is either a string (single-key tables) or a homogeneous JSON array of
  `[string|number|boolean]` elements for composite-key tables. Composite-key
  arrays follow the same order as the table's declared `key` field.
- `features` is optional — limits the response to a subset of the table's
  features. Omitting it returns **all** features for the row.

**Response body (success):** see the wire-spec
[`OP_GET` response schema](wire-spec.md#op_get-0x0020). The full JSON Schema
lives at
[`examples/wire/schemas/get.response.schema.json`](../examples/wire/schemas/get.response.schema.json).

The success response is the **row-shape itself** — a flat JSON object with
feature names as keys. Cold-start (no events ever pushed for this key) returns
`{}` with HTTP `200` — empty result is **success**, not error. This matches
the Redis-shaped contract: a cold key is just a key with no data, not a
404-class condition.

**HTTP status codes:**

| Status | When |
|--------|------|
| 200 | Success — row returned. Cold-start returns `200` with body `{}`. |
| 400 | Validation error: `feature_not_in_table`, `key_shape_mismatch`, `invalid_key`. |
| 404 | `unknown_table` — `table` is not a registered table. |
| 415 | Wrong `Content-Type`. |
| 500 | Server error during read (rare). |

**Curl example:**

```bash
curl -X POST http://localhost:8081/get \
  -H 'Content-Type: application/json' \
  -d @examples/wire/get-found.request.json
```

**Errors specific to this endpoint:**

| Code | When | Recovery |
|------|------|----------|
| `unknown_table` | `table` is not a registered table name. | Check the table name against `/registry`. |
| `feature_not_in_table` | `features[i]` is not a feature of the named table. | Check the feature name against the table's declared `agg` map. |
| `key_shape_mismatch` | Composite key length / element types do not match the table's declared key. | Send `key` as the right shape (string for single-key, array of N for composite-N). |
| `invalid_key` | Key value violates the table's key constraints (e.g., empty string when not allowed). | Send a valid key. |

### POST /batch_get

| Field | Value |
|-------|-------|
| Method | POST |
| Path | `/batch_get` |
| Wire opcode | `OP_BATCH_GET` (`0x0024`) — see [wire-spec § OP_BATCH_GET](wire-spec.md#op_batch_get-0x0024) |
| Content-Type | `application/json` |
| Auth | None (v0) |

**Request body:** see the wire-spec
[`OP_BATCH_GET` request schema](wire-spec.md#op_batch_get-0x0024). The full
JSON Schema lives at
[`examples/wire/schemas/batch_get.request.schema.json`](../examples/wire/schemas/batch_get.request.schema.json).

The body shape is `{requests: [{table, key, features?}, ...]}`. The batch is
**heterogeneous** — different `table` values can appear within the same batch.
Each entry has the same per-entry semantics as a single `OP_GET`.

The server enforces a maximum entries-per-batch cap. The current cap is
**10000** (per `crates/beava-runtime-core/src/wire_request.rs` SRV-API-08); a
batch that exceeds the cap is rejected with `batch_too_large` (HTTP `400`).
Operators can lower this via configuration; raising it requires a server
recompile in v0.

**Response body (success):** see the wire-spec
[`OP_BATCH_GET` response schema](wire-spec.md#op_batch_get-0x0024). The full
JSON Schema lives at
[`examples/wire/schemas/batch_get.response.schema.json`](../examples/wire/schemas/batch_get.response.schema.json).

The success response is `{results: [row-shape, ...]}`. The order of `results`
matches the order of `requests`. Per-entry cold-start is `{}`; per-entry rows
are flat dicts of feature → value. There is **no partial success in v0** —
if any single request entry has an error (`unknown_table`, `key_shape_mismatch`,
etc.), the **entire frame** returns `OP_ERROR_RESPONSE` with the offending
request indexed in the `path` field (e.g., `requests[2].table`). Clients
re-issue with the bad request removed. Partial success is reserved for v0.1+.

**HTTP status codes:**

| Status | When |
|--------|------|
| 200 | Success — all per-entry reads completed (each result either populated or `{}` for cold-start). |
| 400 | Validation error on at least one request entry, or `batch_too_large`. |
| 404 | `unknown_table` on at least one entry — the offending entry is identified by `path` in the error envelope. |
| 415 | Wrong `Content-Type`. |
| 500 | Server error during batch processing (rare). |

**Curl example:**

```bash
curl -X POST http://localhost:8081/batch_get \
  -H 'Content-Type: application/json' \
  -d @examples/wire/batch_get-heterogeneous.request.json
```

**Errors specific to this endpoint:**

| Code | When | Recovery |
|------|------|----------|
| `batch_too_large` | More than 10000 entries in `requests`. | Split into multiple batches under the cap. |
| `unknown_table` | One entry's `table` is not registered. Path in error envelope identifies which (`requests[i].table`). | Check the table; remove the bad entry and re-issue. |
| `feature_not_in_table` | One entry's `features[j]` is not a feature of that table. | Same as `/get`; remove or fix. |
| `key_shape_mismatch` | One entry's `key` shape mismatches its table. | Same as `/get`; fix the entry. |

### POST /reset

| Field | Value |
|-------|-------|
| Method | POST |
| Path | `/reset` |
| Wire opcode | `OP_RESET` (`0x0040`) — see [wire-spec § OP_RESET](wire-spec.md#op_reset-0x0040) |
| Content-Type | `application/json` |
| Auth | None (v0) |

**Request body:** an empty JSON object `{}`. See the wire-spec
[`OP_RESET` request schema](wire-spec.md#op_reset-0x0040). The full JSON
Schema lives at
[`examples/wire/schemas/reset.request.schema.json`](../examples/wire/schemas/reset.request.schema.json).

**Response body (success):** `{"status": "ok"}` after WAL truncation completes.
The call is **synchronous** — the next event push to the same connection
observes the cleared state. See the wire-spec
[`OP_RESET` response schema](wire-spec.md#op_reset-0x0040). The full JSON
Schema lives at
[`examples/wire/schemas/reset.response.schema.json`](../examples/wire/schemas/reset.response.schema.json).

> **Destructive.** `OP_RESET` wipes all in-memory state and truncates the
> WAL. It is intended for **test fixtures** (`bv.test.fixture` and the
> `beavaTestServer` harness use it between tests). Production operators MUST
> NOT call `/reset` on an instance bound to live data. Operators concerned
> about misuse should set `enable_reset_op=false` in the server config; the
> route then returns `403 Forbidden` with `reset_disabled`.

**HTTP status codes:**

| Status | When |
|--------|------|
| 200 | Success — state wiped, WAL truncated. |
| 403 | `reset_disabled` — server config has `enable_reset_op=false`. |
| 415 | Wrong `Content-Type`. |
| 500 | `wal_truncate_failed` — I/O error during WAL truncation. Server state is undefined; restart recommended. |

**Curl example:**

```bash
curl -X POST http://localhost:8081/reset \
  -H 'Content-Type: application/json' \
  -d @examples/wire/reset-request.json
```

Or, since the body is empty:

```bash
curl -X POST http://localhost:8081/reset \
  -H 'Content-Type: application/json' \
  -d '{}'
```

**Errors specific to this endpoint:**

| Code | When | Recovery |
|------|------|----------|
| `reset_disabled` | Server config has `enable_reset_op=false`. | Set the config to `true` if appropriate; otherwise reset is intentionally forbidden on this instance. |
| `wal_truncate_failed` | I/O error during WAL truncation. | Restart the server; investigate disk health. |

### POST /ping

| Field | Value |
|-------|-------|
| Method | POST |
| Path | `/ping` |
| Wire opcode | `OP_PING` (`0x0000`) — see [wire-spec § OP_PING](wire-spec.md#op_ping-0x0000) |
| Content-Type | `application/json` |
| Auth | None (v0) |

**Request body:** an empty JSON object `{}` (or absent — the parser tolerates
empty body for `/ping`). See the wire-spec
[`OP_PING` request schema](wire-spec.md#op_ping-0x0000). The full JSON Schema
lives at
[`examples/wire/schemas/ping.request.schema.json`](../examples/wire/schemas/ping.request.schema.json).

**Response body (success):** `{"server_version": "<semver>", "registry_version": <integer>}`.
See the wire-spec
[`OP_PING` response schema](wire-spec.md#op_ping-0x0000). The full JSON Schema
lives at
[`examples/wire/schemas/ping.response.schema.json`](../examples/wire/schemas/ping.response.schema.json).

`server_version` is the beava server's semantic version (e.g., `"0.0.0"` for
the v0 launch). `registry_version` is a monotonic counter that increments on
every successful `OP_REGISTER`; clients use it as a cache key when caching
feature schemas.

**HTTP status codes:**

| Status | When |
|--------|------|
| 200 | Success — server is reachable. |
| 415 | Wrong `Content-Type` (if a body is sent). |
| 500 | Server error (extremely rare; implies internal panic). |

**Curl example:**

```bash
curl -X POST http://localhost:8081/ping \
  -H 'Content-Type: application/json' \
  -d @examples/wire/ping-request.json
```

**Errors specific to this endpoint:** none in the operational sense — `/ping`
does not validate any input.

## Admin sidecar endpoints (separate port, GET-shaped)

The admin sidecar binds on `cfg.admin_addr` — a **separate port** from the
data-plane HTTP listener. Per the
[Phase 12.6 mio-only Hot-Path Invariant](../CLAUDE.md), the data plane is the
hand-rolled mio + non-blocking I/O loop and the admin sidecar is the only
place tokio + axum run in the runtime. This separation is enforced by the
architectural test
`crates/beava-server/tests/phase12_6_mio_only_dataplane.rs` — adding a third
caller of `apply_event_to_aggregations` or introducing `axum::*` symbols
outside `http_admin.rs` fails CI.

The admin sidecar is **GET-shaped** (idempotent, cacheable, friendly to
operator tooling):

### GET /health

| Field | Value |
|-------|-------|
| Method | GET |
| Path | `/health` |
| Port | `cfg.admin_addr` |
| Auth | None (v0) |

**Response:** HTTP `200` with body `{"status": "ok"}` whenever the admin server
is up. This is a **cheap** liveness probe — it does NOT confirm that the
registry is loaded or that the data-plane has finished WAL recovery; for that,
use `/ready`.

Suitable for Kubernetes / Nomad / systemd-style liveness probes.

### GET /ready

| Field | Value |
|-------|-------|
| Method | GET |
| Path | `/ready` |
| Port | `cfg.admin_addr` |
| Auth | None (v0) |

**Response:** HTTP `200` with body `{"status": "ready"}` only after recovery
is complete (snapshot loaded + WAL replayed and the data-plane is dispatching
events). HTTP `503 Service Unavailable` while recovery is in progress.

Suitable for Kubernetes / Nomad / systemd-style readiness probes — gates the
service from receiving traffic until the cold-start replay finishes.

> **Back-compat note:** the data-plane port also exposes `GET /ready` and
> `GET /health` at `cfg.bind_addr` for back-compat with the ~20 test files
> that poll `ts.base_url()` for readiness. The canonical location is the
> admin sidecar; the data-plane mirroring is a Plan 12.6-01 carry-over and
> may be removed in v0.0.x.

### GET /metrics

| Field | Value |
|-------|-------|
| Method | GET |
| Path | `/metrics` |
| Port | `cfg.admin_addr` |
| Auth | None (v0) |

**Response:** Prometheus exposition format (`text/plain; version=0.0.4; charset=utf-8`).

The metric families exposed in v0 are:

- `beava_registry_version` (gauge) — monotonic.
- `beava_node_count` (gauge) — number of registered aggregation nodes.
- `beava_runtime_kind{runtime="mio"} 1` (gauge) — pins the data-plane runtime
  identity per `project_phase18_no_dual_runtime`.
- `beava_entropy_categories_capped_total` (counter) — entropy operator
  cap-hit total.
- `beava_cold_entity_evictions_total` (counter) — cold-TTL entity evictions
  (Plan 12.8-03).
- `beava_lifetime_op_cap_hit_total` (counter) — lifetime aggregation cap-hit
  total.
- `beava_entity_count_resident` (gauge) — current resident entity count.
- `beava_bucket_reclaim_total` (counter) — windowed-op trailing-bucket
  reclaims (AGG-CORE-09 64-bucket cap firings).
- `beava_bytes_per_entity_p99` (gauge) — static v0 estimate of per-entity
  memory footprint (~7000 bytes per Phase 12.9 verification).

See [`docs/architecture/observability.md`](architecture/observability.md)
(Plan 13.0-13 — forward reference) for the full metric-family catalogue, label
discipline, scrape interval recommendations, and Phase 12.8 memory-governance
counter semantics.

### GET /registry

| Field | Value |
|-------|-------|
| Method | GET |
| Path | `/registry` |
| Port | `cfg.admin_addr` |
| Auth | None (v0) |

**Response:** HTTP `200` with body `{"version": <integer>, "node_count": <integer>}`
(plus extended fields in v0.0.x; the v0 ship surface is the version + node
count). An optional `?version=N` query parameter requests a historical snapshot
for debugging — implementation lands in v0.0.x; the v0 ship surface is the
current snapshot only.

> **Back-compat note:** the data-plane port also exposes `GET /registry` for
> back-compat with phase4 / phase5 / phase11.5 tests that GET `/registry` to
> assert schema propagation. As with `/ready` + `/health`, the canonical
> location is the admin sidecar.

## Error envelope format

Every 4xx and 5xx response carries the standard error envelope per
[`examples/wire/schemas/error.schema.json`](../examples/wire/schemas/error.schema.json):

```json
{
  "code": "<structured-code-string>",
  "path": "<JSON-or-DAG-path>",
  "message": "<human-readable-string>"
}
```

- **`code`** is a structured machine-readable identifier. Stable across
  releases. The canonical alphabetised list with full HTTP status mapping
  lives at [`docs/error-codes.md`](error-codes.md) (Plan 13.0-12 — forward
  reference).
- **`path`** is an optional JSON path or DAG path locating the offending
  element. Examples: `descriptors[1].schema.amount` (during register
  validation), `fields.amount` (during push), `requests[2].table` (during
  batch_get).
- **`message`** is a human-readable explanation. **Forward-looking framing per
  Phase 12.7 D-02** — messages say "X is not supported in v0", **not** "X has
  been removed" or "X was deprecated". The framing avoids implying a
  previous-version reference for users who never saw older revisions of the
  product.

The error envelope is **identical** on both transports — TCP wraps it in a
frame with `op = 0xFFFF` (`OP_ERROR_RESPONSE`) and `content_type = 0x01`;
HTTP returns it as the response body with the appropriate status code.

Worked example:
[`examples/wire/register-conflict.error.json`](../examples/wire/register-conflict.error.json).

## Connection lifecycle

- **Single TCP connection per `App` instance.** Each SDK creates one HTTP/1.1
  connection (with keepalive) per `App`. Multi-connection pools inside one
  `App` are reserved for v0.1+. Concurrent calls on the same `App` from
  multiple threads serialise on the connection.
- **Auto-reconnect on drop.** If the server closes the connection (e.g.,
  during a graceful restart), the SDK's transport layer transparently
  reconnects on the next call. v0 keeps this behaviour conservative — there is
  no exponential backoff configuration; the SDK retries at most once per call,
  and the call surfaces the error if both attempts fail.
- **Keepalive cadence.** Server respects standard TCP keepalive; v0 does not
  pin a value. Reverse-proxies in front of beava (nginx, Envoy) typically
  enforce their own idle-timeout; aim to keep the SDK's idle-timeout below the
  proxy's.
- **Idempotency.** Per Phase 6.1, push idempotency is opt-in via the
  `dedupe_key` + `dedupe_window` fields registered on the event source.
  Duplicate pushes within the window return the prior `ack_lsn` plus
  `idempotent_replay: true`. See [`docs/wire-spec.md`](wire-spec.md) for the
  full dedupe semantics.

## Note on event-name routing

A short clarification on the route-rename:

| | Pre-13.4 (current code) | Post-13.4 (this doc's target) |
|--|--|--|
| Push route | `POST /push/{event_name}` | `POST /push` |
| `event_name` location | URL path segment | Top-level body key |
| Convention | Path-arg style (REST tradition) | Verb-style (Polars / DuckDB convention) |

Phase 13.4 ships the rename mechanically. The body shape changes from
`{fields: {...}}` to `{event_name: "...", fields: {...}}`. Other routes
(`/register`, `/get`, `/batch_get`, `/reset`, `/ping`) are already verb-style
in the current code; only `/push` (and its TCP analog) needs the rename.

The rename rationale aligns with `project_v2_devex_first` and the broader
Polars / DuckDB voice (`feedback_beava_website_voice`):

- **Verb-style is more REST-ful in the strict sense** — the URL names the
  operation; the body carries the structured arguments. Lookups, registers,
  pushes, and resets all read identically.
- **One-to-one match with the wire opcode table.** TCP carries `OP_PUSH` with
  the event name in the body; HTTP carries `POST /push` with the event name
  in the body. The two transports become a literal translation, which makes
  SDK porting trivial in 13.6 (the TCP framer is the only logic per opcode;
  the HTTP transformation is `frame.body → request.body`).
- **No path-segment escaping.** Event names with `/`, spaces, or non-ASCII
  characters require URL-encoding in the path-arg style; the body-arg style
  treats them as plain JSON strings.

SDKs (Plan 13.5 Python, Plan 13.6 TS + Go) ship against the post-13.4 form
from day one. The Python SDK does NOT carry a deprecation-alias path for the
old route — pre-13.0 dev users (small group, no production deploys yet) move
to the new form directly when they upgrade past v0.0.0. The deprecation
window for the route rename is "the gap between current code and v0.0.0
release"; once v0.0.0 ships, only the new form is supported.

## Plan-level traceability

This document is authored by Plan 13.0-03 (Wave 1). It declares the
post-13.4 target HTTP route table and is read by:

- **Plan 13.0-04** (`docs/sdk-api/{python,typescript,go}.md`) — per-language
  SDK API specs target the verb-style routes documented here.
- **Plan 13.0-12** (`docs/error-codes.md`) — alphabetised structured-code list
  referenced by the `code` field in this doc's error tables.
- **Plan 13.0-13** (`docs/architecture/observability.md`) — admin sidecar
  metric catalogue referenced by [GET /metrics](#get-metrics) above.
- **Phase 13.4** — engine implementation that mechanically renames push routes
  to the verb-style form documented here.
- **Phase 13.5 / 13.6** — Python / TS / Go SDKs that send requests against
  these routes.

For the full Phase 13.0 plan tree, see
[`.planning/phases/13.0-design-contract-spec-docs/13.0-PLAN.md`](../.planning/phases/13.0-design-contract-spec-docs/13.0-PLAN.md).
