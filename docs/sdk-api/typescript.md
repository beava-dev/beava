# beava TypeScript SDK

> **Communicate-only SDK.** This SDK pushes events, registers pre-compiled JSON descriptors, and reads features. Pipeline authoring (event sources, expression DSL, op helpers) lives in the **Python SDK only** — see [python.md](python.md). Use Python's `bv.App.register_json(...)` (or hand-write the JSON per [docs/wire-spec.md OP_REGISTER](../wire-spec.md#op_register-0x0001)) to produce descriptors, then ship that JSON to your TypeScript app.

> **Status:** Authoritative for v0. Documents the post-13.6 TS SDK shape (rescoped 2026-05-03 to communicate-only). Cross-language semantics live in [shared.md](shared.md); wire-level body shapes live in [docs/wire-spec.md](../wire-spec.md). Python is the canonical authoring reference.
>
> **Last reviewed:** 2026-05-03 (Phase 13.6).

## Overview

`@beava/sdk` is a wire-thin TypeScript client for the beava real-time feature server. It targets Node.js 18+ (LTS) and is **ESM-only** (`"type": "module"`).

- **Promise-based** — every wire-bound method returns a `Promise<T>`.
- **No DSL** — `Descriptor` is an opaque `Record<string, unknown>` JSON blob; the SDK never parses or compiles authoring expressions.
- **JSON wire bodies pass through verbatim** — no camelCase ↔ snake_case translation; what the wire spec says, the SDK sends.

> **npm:** `npm install @beava/sdk` (or `pnpm add @beava/sdk` / `yarn add @beava/sdk`). Apache-2.0 license. Source lives at `github.com/beava-dev/beava/sdk/typescript/`.

## Module structure

```
sdk/typescript/
├── src/
│   ├── index.ts             # public exports: beavaApp, errors, types, wire, transports
│   ├── app.ts               # beavaApp class with all 8 wire methods
│   ├── wire.ts              # frame codec + opcode constants (CT_JSON only in v0)
│   ├── transport.ts         # HttpTransport (uses global fetch)
│   ├── transport-tcp.ts     # TcpTransport (node:net, Redis-style FIFO)
│   ├── embed.ts             # spawnEmbeddedServer, teardownServer, discoverBinary
│   ├── errors.ts            # RegistrationError, BinaryNotFoundError
│   └── types.ts             # Descriptor (opaque), result interfaces
└── test/                    # vitest specs
```

There are deliberately **no** `events.ts` / `col.ts` / `agg.ts` / `table.ts` files — the TS SDK has no authoring layer. See [shared.md § Authoring vs communicate](shared.md#authoring-vs-communicate).

## beavaApp class

```typescript
import { beavaApp } from "@beava/sdk";

class beavaApp {
  constructor(url?: string, options?: { timeout?: number; test_mode?: boolean; binary_path?: string });

  // Wire methods (each returns a Promise<T>)
  register(descriptors: Descriptor[], opts?: { force?: boolean; dry_run?: boolean }): Promise<RegisterResult>;
  push(eventName: string, fields: Record<string, unknown>): Promise<PushResult>;
  pushSync(eventName: string, fields: Record<string, unknown>): Promise<PushResult>;
  get(table: string): Promise<FeatureRow>;                                          // 1-arg = global table per ADR-003
  get(table: string, key: string | (string | number | boolean)[]): Promise<FeatureRow>; // 2-arg = per-entity
  batchGet(requests: GetRequest[]): Promise<FeatureRow[]>;
  reset(): Promise<void>;                                                            // server gates on test_mode
  ping(): Promise<PingResult>;
  close(): Promise<void>;                                                            // idempotent
}
```

### Constructor + URL-scheme dispatch

The first constructor argument selects the transport:

| URL form | Transport | Notes |
|----------|-----------|-------|
| `http://host:port` / `https://host:port` | HTTP/JSON via `fetch` | Default for production. |
| `tcp://host:port` | Custom-framed TCP (`[u32 length][u16 op][u8 ct][payload]`) | Lowest-latency; Redis-style strict-FIFO correlation. |
| `undefined` (no URL) | Embed mode | Spawns local `beava` binary on first call; auto-reaped on `close()`. |

`options`:

- `timeout` — per-request I/O timeout in ms (default `30000`).
- `test_mode` — passes `BEAVA_TEST_MODE=1` to the embed-mode subprocess (mirrors Python `bv.App(test_mode=True)` per Phase 13.5 D-05). Ignored in network mode.
- `binary_path` — overrides the embed-mode binary discovery path.

### `register(descriptors, opts?)`

Submit a list of pre-compiled register node JSON blobs to `POST /register`. Wire body:

```json
{ "nodes": [<descriptor>, ...], "force": false, "dry_run": false }
```

`Descriptor` is `Record<string, unknown>` — the SDK does not validate or compile descriptors. Authoring (event sources, expressions, op helpers) lives in the Python SDK; users either:

1. Author in Python, compile to JSON via `bv.App.register_json(...)`, ship the JSON to a TypeScript runtime.
2. Hand-write the JSON per `docs/wire-spec.md OP_REGISTER`.

Returns `RegisterResult`:

```typescript
interface RegisterResult {
  status: string;          // "ok" on success
  registry_version: number;
  added?: string[];
  removed?: string[];
  changed?: string[];
}
```

`opts.force`: pass-through to wire `force` flag (allows destructive schema changes per Phase 13.4 D-01).
`opts.dry_run`: pass-through to wire `dry_run` flag (validate without applying).

### `push(eventName, fields)` and `pushSync(eventName, fields)`

`push` posts to `POST /push/<eventName>` with `{fields: {...}}`. Default semantics: `acks=1` (durable on this server).

`pushSync` is reserved for `acks=all` (multi-replica) durable push in v0.1+ per [docs/wire-spec.md OP_PUSH_SYNC](../wire-spec.md). v0 implementations delegate to `push` so users can write `acks=all`-shaped code today and switch wire opcodes when the server lands the multi-replica path.

### `get(table)` and `get(table, key)`

Two overloaded signatures:

- `get(table, key)` — per-entity lookup. `key` is either a `string` or a tuple `(string | number | boolean)[]` for composite keys.
- `get(table)` — global aggregation (no entity dimension) per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md). Wire body uses `key: ""` (empty-string sentinel).

Wire body:

```json
{ "table": "<name>", "key": "<entity_id>" | ["a", 42, true] | "" }
```

Returns `FeatureRow = Record<string, unknown>`. Cold-start (entity unknown) returns `{}` — never `null`, never an error.

### `batchGet(requests)`

Posts to `POST /batch-get` with `{requests: [...]}`. Returns `FeatureRow[]` in request order. v0 has no partial success: any per-entry error rejects the whole batch and surfaces as a single `RegistrationError`.

### `reset()`

Posts to `POST /reset`. The server returns `403` with `{error: {code: "reset_forbidden", ...}}` unless `test_mode` is enabled per Phase 13.4 D-03. The error surfaces verbatim as a `RegistrationError`.

### `ping()`

Calls `GET /health`. Returns `PingResult`:

```typescript
interface PingResult { server_version: string; registry_version: number; }
```

### `close()`

Idempotent. Closes the underlying transport. In embed mode, sends `SIGTERM` to the spawned subprocess and `SIGKILL`s after 5s if it didn't exit; cleans up the per-instance temp CWD.

## Errors

```typescript
class RegistrationError extends Error {
  code: string;        // structured error code, e.g. "unsupported_node_kind", "invalid_registration"
  path?: string;       // JSON pointer-ish path, e.g. "nodes[0].kind"
  errors?: unknown[];  // sub-errors (multi-error responses)
}

class BinaryNotFoundError extends Error {
  searched: string[];  // paths that were probed during 4-step discovery
}
```

`RegistrationError` is thrown by every wire method on non-2xx responses (HTTP transport) or on `OP_ERROR_RESPONSE` frames (TCP transport). The `code` field maps to [docs/error-codes.md](../error-codes.md).

`BinaryNotFoundError` is thrown by `discoverBinary()` when the 4-step search fails (see [shared.md § Embed mode](shared.md)).

## TypeScript int64 caveat

JSON has no native int64. The SDK does not coerce numeric fields — `i64` features deserialize as `number` (which loses precision above `2^53`) by default. If you push `i64` values exceeding `Number.MAX_SAFE_INTEGER` and need exact round-tripping, treat the affected fields as strings on the wire.

## Test fixtures

The SDK exports test helpers under `@beava/sdk` (no separate sub-export needed):

- `spawnEmbeddedServer({startupTimeoutMs?, testMode?})` — spawn a local beava server on ephemeral ports; returns `{proc, httpUrl, tcpUrl, tmpDir}`.
- `teardownServer(handle)` / `teardownProcess(proc)` — graceful shutdown.
- `discoverBinary()` — 4-step binary discovery (mirrors `python/beava/_embed.py`).

Use `beavaApp(undefined, { test_mode: true })` to spawn an embed-mode server in tests — it auto-spawns on the first wire call and auto-reaps on `close()`.

## Versioning + compatibility

- v0 surface is **frozen** as documented above.
- Node.js 18+ LTS baseline (uses native `fetch` / `AbortController` / `child_process` / `node:net`).
- ESM-only output. No CJS dual publish in v0.
- Apache-2.0 license.

## Cross-references

- **Wire contract:** [docs/wire-spec.md](../wire-spec.md)
- **Cross-language semantics:** [shared.md](shared.md)
- **Authoring SDK (Python):** [python.md](python.md)
- **Go SDK (sister communicate-only client):** [go.md](go.md)
- **Phase 13.6 plan + summary:** `.planning/phases/13.6-typescript-go-sdks/`
