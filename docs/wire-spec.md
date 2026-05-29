# beava Wire Spec

> **Status:** Authoritative for v0. Engine and all SDKs (Python, TypeScript, Go) MUST conform to this spec.
> **JSON Schema dialect:** [Draft 2020-12](https://json-schema.org/draft/2020-12/schema).
> **Last reviewed:** 2026-05-03 (Phase 13.0).

## Overview

beava speaks two transports — **HTTP/1.1 + JSON** for compatibility with curl, load balancers, WAFs, and any HTTP client; and a **custom-framed TCP** path for low-latency fast-path traffic. Both transports carry the **same logical opcode set** and the **same JSON body shapes**. Choosing one transport over the other is an operational decision (HTTP for reach + observability, TCP for tail latency); it is never a contract decision.

Correlation on the TCP transport follows **Redis-style strict-FIFO**: the order of responses on a connection matches the order of requests, and there is no `request_id` or `correlation_id` field anywhere in the wire format. This keeps the protocol simple, eliminates an entire class of header-bookkeeping bugs in client implementations, and makes the framed envelope as small as possible.

This document is **authoritative**. Where prose and JSON Schema disagree, the JSON Schema in [`examples/wire/schemas/`](../examples/wire/schemas/) wins — schemas are machine-validatable contracts, prose is explanatory. Phase 13.4 ships a CI test (`crates/beava-server/tests/wire_spec_validates.rs`) that loads every schema and asserts every fixture under [`examples/wire/`](../examples/wire/) validates against its corresponding schema. SDK ports in 13.5 (Python) and 13.6 (TypeScript + Go) consume the same fixtures via language-native validators.

The opcode-discovery question — "which family of body shape do I parse?" — is answered by the **opcode** in the TCP frame header (or the URL path on HTTP). Within a body, polymorphic shapes are disambiguated by a JSON **`kind`** discriminator. Specifically, `OP_REGISTER` carries a `kind=event|table|derivation` discriminator that selects between three sub-shapes; all other opcodes have a single body shape per direction.

## Frame Format

The TCP transport frames every request and every response identically:

```
+---------------+---------------+----------------------+--------------------------------+
| length (u32)  | op (u16)      | content_type (u8)    | payload                        |
| big-endian    | big-endian    |                      | length - 3 bytes               |
| 4 bytes       | 2 bytes       | 1 byte               | 0..(length-3) bytes            |
+---------------+---------------+----------------------+--------------------------------+
```

Notes:

- **`length`** is the size in bytes of `op + content_type + payload`. It does **not** include itself. The smallest legal frame is therefore `0x00000003` (length=3, empty payload), which carries `[length=3 op=XX content_type=YY]` with no payload bytes.
- **`length` is big-endian** (network byte order). Same for `op`. There is no little-endian variant of the wire format anywhere in the v0 protocol.
- **`content_type`** is a single byte selecting the payload encoding:
  - `0x01` (`CT_JSON`) — JSON. The only encoding required in v0.
  - `0x02` (`CT_MSGPACK`) — MessagePack. Reserved on the TCP transport for v0.1+; servers MAY accept it but clients MUST NOT rely on it being available.
- The **HTTP transport always uses JSON** for v0; the `content_type` byte is implicit (it lives in the HTTP `Content-Type: application/json` header).
- **Maximum frame size** is configurable; the server default is 4 MiB (`DEFAULT_TCP_MAX_FRAME_BYTES = 4 * 1024 * 1024`). Frames that declare `length > max` are rejected with `OP_ERROR_RESPONSE` carrying code `frame_too_large`, and the connection is closed.
- **Correlation:** Redis-style strict-FIFO on a connection. Clients send N requests, then read N responses in the same order. There is **no `request_id`** field in either request or response bodies. Pipelining is supported (multiple in-flight requests on one connection).
- **Errors** use the dedicated opcode `OP_ERROR_RESPONSE = 0xFFFF`. The payload is a JSON object matching [`error.schema.json`](../examples/wire/schemas/error.schema.json). The connection stays open after a single-frame error (only that frame is rejected); fatal protocol errors close the connection.

The HTTP transport mirrors this opcode set: each opcode has a corresponding verb-style POST route (e.g., `POST /push/<event_name>` for `OP_PUSH`). The full HTTP route table lives in [`docs/http-api.md`](http-api.md) (Plan 13.0-03).

## Opcode Table

| Opcode | Name | Direction | Body shape (JSON) | Notes |
|--------|------|-----------|-------------------|-------|
| `0x0000` | `OP_PING` | client → server | `{}` | Health probe. Response carries `{server_version, registry_version}`. |
| `0x0001` | `OP_REGISTER` | client → server | DAG payload | Discriminated union on `kind`: `event` \| `table` \| `derivation`. |
| `0x0010` | `OP_PUSH` | client → server | `{fields: object}` | Sync push. Default ack is `acks=1` (Kafka-style: durable on this server). The event name comes from the URL path (HTTP) or routing prefix (TCP). |
| `0x0011` | `OP_PUSH_SYNC` | RESERVED | — | Reserved for `acks=all` (multi-replica) push in v0.1+. v0 servers reply with `op_not_implemented`. |
| `0x0012` | `OP_PUSH_MANY` | RESERVED | — | Reserved for batch push in v0.1+. v0 servers reply with `op_not_implemented`. |
| `0x0020` | `OP_GET` | client → server | `{table, key, features?}` | Single-row read. Returns row-shape (dict of feature → value). Cold-start returns `{}`. |
| `0x0023` | `OP_GET_RESPONSE` | server → client | row-shape body | Response opcode for `OP_GET` and `OP_BATCH_GET`. |
| `0x0024` | `OP_BATCH_GET` | client → server | `{requests: [{table, key, features?}, ...]}` | Heterogeneous batch lookup. Response order matches request order. NEW in v0 (post-12.7) per ROADMAP §13.4. |
| `0x0030` – `0x003F` | (reserved) | — | — | Reserved range for future direct-feature-write opcodes (`set` / `mset` / similar). v0 servers reply with `op_not_implemented`. |
| `0x0040` | `OP_RESET` | client → server | `{}` | Wipes all in-memory state and truncates WAL. Useful for tests and `bv.test.fixture`. Destructive — only call on a beava instance bound to test data. Per Phase 13.0 Q7. |
| `0xFFFF` | `OP_ERROR_RESPONSE` | server → client | error envelope | Universal error reply. Payload schema: [`error.schema.json`](../examples/wire/schemas/error.schema.json). |

The 6 v0 client-initiated opcodes (`OP_PING`, `OP_REGISTER`, `OP_PUSH`, `OP_GET`, `OP_BATCH_GET`, `OP_RESET`) are documented per-opcode in the sections below.

## Content Types

Two content type bytes are defined for the TCP transport:

| Byte | Constant | Encoding | Status |
|------|----------|----------|--------|
| `0x01` | `CT_JSON` | UTF-8 JSON | **Implemented in v0.** Required. Both transports. |
| `0x02` | `CT_MSGPACK` | MessagePack | **Reserved for v0.1+** on the TCP transport. v0 servers MAY accept it (Phase 18-09 wired the codec); clients MUST NOT depend on it. |

The HTTP transport in v0 accepts only `application/json` request bodies and emits `application/json` responses. `Content-Type` other than JSON is rejected with `unsupported_content_type`.

Frames with an unknown `content_type` byte (anything other than `0x01` or `0x02`) are rejected with the structured error code `unsupported_content_type`. The connection stays open — only the offending frame is rejected.

## Per-opcode body shapes

Each section below declares the request and response body shape for a v0 opcode, cross-links to the JSON Schema, and points to a worked example fixture.

### OP_PING (0x0000)

Health probe. Useful for liveness checks, transport-level keepalive, and version discovery.

**Request body shape:**

```json
{}
```

JSON Schema: [`examples/wire/schemas/ping.request.schema.json`](../examples/wire/schemas/ping.request.schema.json)

Worked example: [`examples/wire/ping-request.json`](../examples/wire/ping-request.json)

**Response body shape (success):**

```json
{
  "server_version": "<semver>",
  "registry_version": <integer>
}
```

JSON Schema: [`examples/wire/schemas/ping.response.schema.json`](../examples/wire/schemas/ping.response.schema.json)

Worked example: [`examples/wire/ping-response.json`](../examples/wire/ping-response.json)

`server_version` is the beava server's semantic version (e.g., `"0.0.0"` for the v0 launch). `registry_version` is a monotonic counter that increments on every successful `OP_REGISTER`; clients use it as a cache key when caching feature schemas.

**Errors:** `OP_PING` does not validate any input, so it has no per-opcode error codes. Connection-level errors (e.g., framing) still apply.

### OP_REGISTER (0x0001)

Register one or more **descriptors** with the beava server. A descriptor is one of three kinds, disambiguated by the JSON `kind` field:

- **`kind: "event"`** — declares a `@bv.event` source (an event type with a typed schema; events of this name push fields matching the schema).
- **`kind: "table"`** — declares a `@bv.table(key=...)` aggregation-output node, per [ADR-001](../.planning/decisions/ADR-001-bv-table-partial-overturn.md). The table receives rows materialised by upstream aggregation derivations; it has **no** `app.upsert` / `app.delete` / `app.retract` paths in v0 (those stay killed by `project_v0_events_only_scope`).
- **`kind: "derivation"`** — declares a derived event or table node (filter / select / with_columns / group_by / agg chains). The `output_kind` field disambiguates whether the derivation emits events (push-shaped) or a table (key-shaped row materialisation).

Per ADR-002, op names inside aggregation specs use the **new Polars conventions** (`mean`, `var`, `std`, `n_unique`, `quantile`) — not the old SQL-prose names (`avg`, `variance`, `stddev`, `count_distinct`, `percentile`).

**Request body shape:**

```json
{
  "descriptors": [
    {"kind": "event", "name": "Txn", "schema": {...}, ...},
    {"kind": "table", "name": "UserFeatures", "key": ["user_id"], "upstreams": ["Txn"], "agg": {...}},
    {"kind": "derivation", "name": "...", "upstreams": [...], "ops": [...], "output_kind": "event|table"}
  ],
  "force": false,
  "dry_run": false
}
```

- `force=true` allows destructive schema changes (e.g., changing a field's type); the server accepts the change and zeroes affected aggregations. Default is `false` — destructive changes are rejected with `registration_conflict`.
- `dry_run=true` runs the validator and computes the diff without applying anything. The response carries the diff (`added`, `removed`, `changed`) but no state is mutated; `registry_version` is unchanged.

JSON Schema: [`examples/wire/schemas/register.request.schema.json`](../examples/wire/schemas/register.request.schema.json)

Worked examples:
- [`examples/wire/register-fraud-team.request.json`](../examples/wire/register-fraud-team.request.json) — fraud-team-style payload using NEW op names (`mean`, `n_unique`, `quantile`).
- [`examples/wire/register-dry-run.request.json`](../examples/wire/register-dry-run.request.json) — `dry_run=true`.
- [`examples/wire/register-force.request.json`](../examples/wire/register-force.request.json) — `force=true` for destructive change.

**Response body shape (success):**

```json
{
  "status": "ok",
  "registry_version": 1,
  "added": ["Txn", "UserTxnFeatures"],
  "removed": [],
  "changed": []
}
```

JSON Schema: [`examples/wire/schemas/register.response.schema.json`](../examples/wire/schemas/register.response.schema.json)

Worked example: [`examples/wire/register-fraud-team.response.json`](../examples/wire/register-fraud-team.response.json)

**Errors:**

| Code | When | HTTP status |
|------|------|-------------|
| `unsupported_node_kind` | Body has `kind="table"` (pre-12.7 form) or `kind="upsert"`/`"delete"`/`"retract"` etc. — handled at the JSON-prelude validator. | 400 |
| `registration_conflict` | A descriptor changes a field type or removes a field without `force=true`. | 409 |
| `schema_invalid` | Descriptor structure does not conform to its schema (missing required field, wrong type). | 400 |
| `unknown_op` | `agg.<feature>.op` references an op name not in the operator catalogue. | 400 |

Worked example: [`examples/wire/register-conflict.error.json`](../examples/wire/register-conflict.error.json)

#### Global tables (`key = []`) — per ADR-003

A register payload with `key: []` (empty array) declares a **global table** — a single output row, no per-entity dimension. The sentinel `key = ""` (empty string) routes global state on the GET wire. Per [ADR-003](../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md), every operator works in both per-entity and global modes — semantics are identical, only the state-keying dimension differs.

Use cases: monitoring dashboards (total throughput, current entity count, global p95), anomaly detection on global rates ("is the GLOBAL signup rate spiking?"), top-K-globally features ("top 10 hottest pages on the platform"), cross-entity aggregations ("total spend across all users").

**Wire-level register payload (table descriptor):**

```json
{
  "kind": "table",
  "name": "GlobalCounter",
  "key": [],
  "upstreams": ["Click"],
  "agg": {
    "click_count": {"op": "count", "params": {"window": "forever"}}
  }
}
```

**Wire-level GET request for a global table:**

```json
{ "table": "GlobalCounter", "key": "" }
```

**Cold-start GET response (no events landed yet):**

```json
{}
```

**GET response after events land (flat row-shape per `get.response.schema.json`):**

```json
{ "click_count": 12345 }
```

See [`examples/wire/register-global-counter.request.json`](../examples/wire/register-global-counter.request.json), [`examples/wire/get-global.request.json`](../examples/wire/get-global.request.json), and [`examples/wire/get-global.response.json`](../examples/wire/get-global.response.json) for the full fixture set. The `examples/wire/schemas/register.request.schema.json` JSON Schema accepts `key: []` (the `minItems: 1` constraint is relaxed to `minItems: 0` per ADR-003); `examples/wire/schemas/get.request.schema.json` accepts the empty-string `key: ""` sentinel for global GET.

**Validation contract:** `key` MUST be either non-empty (per-entity table) or empty array (global table) — never null. The server rejects null `key` at the JSON-prelude validator with `schema_invalid`. The corresponding GET MUST send `key: ""` for a global table; sending a non-empty key against a global table raises `KeyError`-style rejection (or returns `{}` cold-start, depending on the SDK convention; per Phase 13.5 the Python SDK raises). Symmetric: sending `key: ""` against a per-entity table is a misuse and the server returns `{}` cold-start (no error — the empty entity simply has no state).

`OP_BATCH_GET` accepts mixed per-entity + global lookups in the same batch (heterogeneous batches can include both shapes — global lookups simply set `key` to `""`). See `examples/wire/batch_get-heterogeneous.request.json` for the per-entity variant; the global variant inside the same batch is `{"table": "GlobalCounter", "key": ""}`.

**Implementation deferred** to Phase 13.4 (engine sentinel routing — ~30 LOC; the existing `&str` key path handles `""` natively, so this is mostly the absence of a special-case rejection) + Phase 13.5 (Python SDK no-key form: `@bv.table` no `key=`, `events.group_by()` empty, `events.agg(**aggs)` shorthand, `App.get(table_name)` 1-arg overload) + Phase 13.6 (TS + Go SDK overloads). Acceptance gate: `python/tests/v0/test_global.py` (Plan 13.0-16, 8 tests gated by `_engine_available()` SKIP until 13.4 + 13.5 land together).

### OP_PUSH (0x0010)

Push a single event into a registered event source. Default ack semantics is `acks=1` — the server returns success after the event is durably written to the local WAL (per the active sync mode; default is periodic fsync per Phase 6.1 `SyncMode::Periodic`).

The **event name** comes from the URL path on HTTP (`POST /push/<event_name>`) or from a routing prefix on the TCP transport. The wire body itself carries only the **fields** dict.

**Request body shape:**

```json
{
  "fields": {
    "user_id": "alice",
    "card_id": "card_001",
    "amount": 42.50,
    "merchant": "amazon",
    "ip": "203.0.113.42"
  }
}
```

The `fields` object MUST match the registered event's schema — same field names, compatible types. Type-coercion is allowed on the boundary (string `"42"` for an `i64` field is accepted in v0 if the source is HTTP/JSON; TCP/JSON-encoded payloads follow the same rule).

ADR-002 op-rename note: pushed events are **events**, not aggregations, so op renames have no effect on push body shapes.

JSON Schema: [`examples/wire/schemas/push.request.schema.json`](../examples/wire/schemas/push.request.schema.json)

Worked example: [`examples/wire/push-success.request.json`](../examples/wire/push-success.request.json)

**Response body shape (success):**

```json
{
  "ack_lsn": 12345,
  "registry_version": 1
}
```

`ack_lsn` is the server-assigned monotonic Log Sequence Number for this event; clients can persist it as an idempotency anchor (re-pushing an event with the same fields and an idempotent dedupe key returns the same `ack_lsn` plus `idempotent_replay: true`).

JSON Schema: [`examples/wire/schemas/push.response.schema.json`](../examples/wire/schemas/push.response.schema.json)

Worked example: [`examples/wire/push-success.response.json`](../examples/wire/push-success.response.json)

**Errors:**

| Code | When | HTTP status |
|------|------|-------------|
| `schema_mismatch` | A field has the wrong type and cannot be coerced (e.g., string `"abc"` for a `f64` field). | 400 |
| `missing_field` | A required field is missing from `fields`. | 400 |
| `control_character_in_string` | One or more string fields in the event contain control characters (bytes < 0x20 except tab, newline, carriage return). Control characters are rejected to prevent corruption of the write-ahead log. | 400 |
| `unknown_event` | The event name (URL path or TCP routing prefix) is not registered. | 404 |
| `dedupe_replay` | A dedupe key matched a recent push within the dedupe window — server returns the prior `ack_lsn` with `idempotent_replay: true` (this is **not** an error in the operational sense; documented here for completeness). | 200 |

Worked example: [`examples/wire/push-validation-error.error.json`](../examples/wire/push-validation-error.error.json)

### OP_GET (0x0020)

Single-row feature read. Returns the row-shape — a flat dict of feature name → value — for the requested `(table, key)` pair. Cold-start (no events have ever been pushed for that key) returns `{}` — **not** an error.

**Request body shape:**

```json
{
  "table": "UserTxnFeatures",
  "key": "alice",
  "features": ["tx_count_1h", "tx_sum_1h"]
}
```

- `table` is the table name registered with `OP_REGISTER`.
- `key` is either a string (single-key tables) or a homogeneous array of `[string|number|boolean]` for composite-key tables. Composite keys are rendered into the array in the same order as the table's `key` field.
- `features` (optional) — limits the response to a subset of the table's features. Omitting it returns **all** features for the row.

JSON Schema: [`examples/wire/schemas/get.request.schema.json`](../examples/wire/schemas/get.request.schema.json)

Worked example: [`examples/wire/get-found.request.json`](../examples/wire/get-found.request.json)

**Response body shape (success):**

The response is the row-shape itself — a JSON object with feature names as keys.

```json
{
  "tx_count_1h": 7,
  "tx_sum_1h": 312.45,
  "tx_mean_1h": 44.64,
  "tx_p99_1h": 89.99,
  "tx_unique_merchants_1h": 3
}
```

Cold-start returns `{}`. The TCP-transport response opcode is `OP_GET_RESPONSE = 0x0023`; HTTP returns the same body with status `200`.

JSON Schema: [`examples/wire/schemas/get.response.schema.json`](../examples/wire/schemas/get.response.schema.json)

Worked examples:
- [`examples/wire/get-found.response.json`](../examples/wire/get-found.response.json) — populated row.
- [`examples/wire/get-not-found.response.json`](../examples/wire/get-not-found.response.json) — cold-start (`{}`).

**Errors:**

| Code | When | HTTP status |
|------|------|-------------|
| `unknown_table` | `table` is not a registered table name. | 404 |
| `feature_not_in_table` | `features[i]` is not a feature of the named table. | 400 |
| `key_shape_mismatch` | Composite key length / element types do not match the table's declared key. | 400 |

### OP_BATCH_GET (0x0024)

Heterogeneous batch lookup. NEW in v0 (post-12.7) per ROADMAP §13.4. Equivalent to N parallel `OP_GET` calls in a single round-trip; the server processes them in order, and the response array preserves request-order.

Different `table` values can appear within the same batch. This is what makes the opcode **heterogeneous** — it is not a same-table-different-keys batch; it is a fully general batch.

**Request body shape:**

```json
{
  "requests": [
    { "table": "UserTxnFeatures", "key": "alice" },
    { "table": "UserTxnFeatures", "key": "bob" },
    { "table": "CardTxnFeatures", "key": "card_001", "features": ["tx_count_1h"] }
  ]
}
```

JSON Schema: [`examples/wire/schemas/batch_get.request.schema.json`](../examples/wire/schemas/batch_get.request.schema.json)

Worked example: [`examples/wire/batch_get-heterogeneous.request.json`](../examples/wire/batch_get-heterogeneous.request.json)

**Response body shape (success):**

```json
{
  "results": [
    { "tx_count_1h": 7, "tx_sum_1h": 312.45 },
    {},
    { "tx_count_1h": 3 }
  ]
}
```

`results[i]` corresponds to `requests[i]`. Per-entry cold-start is `{}`, **not** an error. Per-entry errors (e.g., one bad key in an otherwise valid batch) DO turn the whole frame into `OP_ERROR_RESPONSE` — there is no partial success in v0; clients re-issue with the bad request removed.

JSON Schema: [`examples/wire/schemas/batch_get.response.schema.json`](../examples/wire/schemas/batch_get.response.schema.json)

Worked example: [`examples/wire/batch_get-heterogeneous.response.json`](../examples/wire/batch_get-heterogeneous.response.json)

**Errors:** Same set as `OP_GET` (`unknown_table`, `feature_not_in_table`, `key_shape_mismatch`), with the per-entry `path` field in the error envelope identifying which request entry tripped (e.g., `"requests[2].table"`).

### OP_RESET (0x0040)

Wipe all in-memory state and truncate the WAL. **Destructive.** Per Phase 13.0 Q7 the value is `0x0040`, leaving `0x0030`–`0x003F` reserved for future direct-feature-write opcodes (`set`, `mset`, etc.).

Use case: testing fixtures. `bv.test.fixture` and the `beavaTestServer` harness reset between tests so the next test sees a clean slate. Production operators MUST NOT call `OP_RESET` on a beava instance bound to live data.

**Request body shape:**

```json
{}
```

JSON Schema: [`examples/wire/schemas/reset.request.schema.json`](../examples/wire/schemas/reset.request.schema.json)

Worked example: [`examples/wire/reset-request.json`](../examples/wire/reset-request.json)

**Response body shape (success):**

```json
{ "status": "ok" }
```

The server replies after the WAL truncation completes — the call is **synchronous**. The next event push to the same connection observes the cleared state.

JSON Schema: [`examples/wire/schemas/reset.response.schema.json`](../examples/wire/schemas/reset.response.schema.json)

Worked example: [`examples/wire/reset-response.json`](../examples/wire/reset-response.json)

**Errors:**

| Code | When | HTTP status |
|------|------|-------------|
| `reset_disabled` | Server config has `enable_reset_op=false` (production operators set this to forbid resets). | 403 |
| `wal_truncate_failed` | I/O error during WAL truncation. The server's state is undefined after this; restart recommended. | 500 |

## Error Envelope

Every `OP_ERROR_RESPONSE` (and every HTTP non-2xx response) carries a JSON body conforming to [`error.schema.json`](../examples/wire/schemas/error.schema.json):

```json
{
  "code": "<structured-code-string>",
  "path": "<JSON-path-or-DAG-path>",
  "message": "<human-readable-string>"
}
```

- **`code`** is a structured machine-readable identifier. Stable across releases. The canonical alphabetised list lives at [`docs/error-codes.md`](error-codes.md) (Plan 13.0-12).
- **`path`** is an optional JSON path or DAG path locating the offending element. Examples: `"descriptors[1].schema.amount"` (during register validation), `"fields.amount"` (during push), `"requests[2].table"` (during batch_get). Optional.
- **`message`** is a human-readable explanation. **Forward-looking framing per Phase 12.7 D-02** — messages say "X is not supported in v0", **not** "X has been removed" or "X was deprecated". The framing avoids implying a previous-version reference for users who never saw older revisions.

The error envelope is the SAME on both transports — TCP wraps it in a frame with `op = 0x000A...` no actually `op = 0xFFFF` (`OP_ERROR_RESPONSE`) and `content_type = 0x01`; HTTP returns it as the response body with the appropriate status code.

Worked example: [`examples/wire/register-conflict.error.json`](../examples/wire/register-conflict.error.json)

## ADR cross-references

This wire spec is shaped by the following Architecture Decision Records:

- **[ADR-001](../.planning/decisions/ADR-001-bv-table-partial-overturn.md)** — `@bv.table` aggregation-output revival (partial overturn of v0 events-only scope). The wire-spec uses `kind=table` in register payloads per ADR-001. Mutation paths (`upsert` / `delete` / `retract`) and MVCC remain killed.

- **[ADR-002](../.planning/decisions/ADR-002-polars-op-rename.md)** — Polars op renames. Register payloads use the **new** op-string names (`mean`, `var`, `std`, `n_unique`, `quantile`). The Rust engine's internal `AggKind` enum variant names (`AggKind::Avg`, `AggKind::Variance`, etc.) are unchanged — only the public string mapping changes. SDKs in 13.5 (Python deprecation aliases) and 13.6 (TS + Go, no aliases) implement the full rename.

## Validation harness (Phase 13.4)

The schemas under [`examples/wire/schemas/`](../examples/wire/schemas/) and the worked examples under [`examples/wire/`](../examples/wire/) are validated by a CI test that ships in Phase 13.4. Specifically:

- **Engine-side (Rust):** `crates/beava-server/tests/wire_spec_validates.rs` (lands in 13.4) loads every `examples/wire/schemas/*.schema.json` and asserts every `examples/wire/*.json` (excluding the `schemas/` subdirectory) validates against its corresponding schema. The Rust validator crate is **`boon`** — chosen because it has full Draft 2020-12 support; the older `jsonschema` Rust crate has only partial 2020-12 coverage.

- **Python SDK (Phase 13.5):** the SDK test suite runs the same fixtures through Python's [`jsonschema`](https://pypi.org/project/jsonschema/) library (`Draft202012Validator`) as part of its unit tests. The harness lives at [`examples/wire/_validate_examples.py`](../examples/wire/_validate_examples.py) and is the authoritative cross-language validation reference.

- **TypeScript SDK (Phase 13.6):** uses [Ajv](https://ajv.js.org/) v8+ via `import Ajv2020 from "ajv/dist/2020"` (Ajv splits Draft 2020-12 into a separate import to avoid bundling bloat).

- **Go SDK (Phase 13.6):** uses [`santhosh-tekuri/jsonschema/v6`](https://github.com/santhosh-tekuri/jsonschema), which supports Draft 2020-12.

Phase 13.0 (this phase) ships the schemas + examples + Python validator. The Rust engine harness ships in 13.4. The TS + Go validators ship in 13.6 alongside the SDK ports themselves.

## Stable contract guarantees

- **Frame layout** is locked. Adding a request_id, changing endianness, or reordering header bytes is a breaking wire-format change requiring a `FORMAT_VERSION` bump; the v0 commitment is `FORMAT_VERSION = 1`.
- **Opcode values** are locked. Opcodes assigned in this spec (PING, REGISTER, PUSH, GET, BATCH_GET, RESET, GET_RESPONSE, ERROR_RESPONSE) keep their values across all v0 minor releases.
- **Body field names** within a given opcode are locked once shipped. Adding optional fields is non-breaking; removing fields or changing their types is breaking.
- **Error codes** in [`docs/error-codes.md`](error-codes.md) are stable identifiers. Renaming a code (e.g., `schema_mismatch` → `field_type_mismatch`) is a breaking change.
- **JSON Schema dialect** is `draft/2020-12` for v0. Migrating to a future dialect requires explicit ADR.

What is **not** part of the stable contract:

- Internal wire details below the application layer (TCP keepalive cadence, HTTP/1.1 header set, connection-pool sizing).
- The exact HTTP status code for every error code beyond the broad 4xx-vs-5xx distinction (the structured `code` field is the contract; HTTP status is a hint).
- The `server_version` returned by `OP_PING` (semver discipline applies once v0 ships, but the value itself is informational, not contractual).

## Plan-level traceability

This document is authored by Plan 13.0-02 (Wave 1). Downstream plans consume it:

- **Plan 13.0-03** (`docs/http-api.md`) writes the verb-style HTTP route table that mirrors this opcode set.
- **Plan 13.0-04** (`docs/sdk-api/*.md`) writes per-language SDK API specs that target this wire format.
- **Plan 13.0-12** (`docs/error-codes.md`) writes the alphabetised structured-code list referenced by the `code` field above.
- **Plan 13.0-14** (vertical examples) reuses the fixtures here as mock-backend response data.
- **Phase 13.4** ships the engine and the Rust validator that asserts every fixture validates.
- **Phase 13.5 / 13.6** ship the SDKs that send / receive frames matching this spec.

For the full Phase 13.0 plan tree, see [`.planning/phases/13.0-design-contract-spec-docs/13.0-PLAN.md`](../.planning/phases/13.0-design-contract-spec-docs/13.0-PLAN.md).
