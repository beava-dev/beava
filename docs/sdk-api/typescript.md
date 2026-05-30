# beava TypeScript SDK

> **Communicate-only SDK.** This SDK pushes events, registers pre-compiled JSON descriptors, and reads features. Pipeline authoring (event sources, expression DSL, op helpers) lives in the **Python SDK only** — see [python.md](python.md). Use Python's `bv.App.register_json(...)` (or hand-write the JSON per [docs/wire-spec.md OP_REGISTER](../wire-spec.md#op_register-0x0001)) to produce descriptors, then ship that JSON to your TypeScript app.

> **Status:** Authoritative for v0. Documents the post-13.6 TS SDK shape (rescoped 2026-05-03 to communicate-only). Cross-language semantics live in [shared.md](shared.md); wire-level body shapes live in [docs/wire-spec.md](../wire-spec.md). Python is the canonical authoring reference.
>
> **Last reviewed:** 2026-05-03 (Phase 13.6).

## Overview

The Beava JavaScript SDK consists of two HTTP-based packages for interacting with a running Beava server. Both target Node.js 18+ and are **ESM-only** (`"type": "module"`).

- **Promise-based** — every wire-bound method returns a `Promise<T>`.
- **No DSL** — `Descriptor` is an opaque `Record<string, unknown>` JSON blob; the SDK never parses or compiles authoring expressions.
- **JSON wire bodies pass through verbatim** — no camelCase ↔ snake_case translation; what the wire spec says, the SDK sends.

### Packages

- **`@beava/node`** — for Node.js services and scripts. `npm install @beava/node` (or `pnpm add @beava/node` / `yarn add @beava/node`).
- **`@beava/client`** — browser-oriented package name that re-exports the same fetch-based API for frontend bundles. `npm install @beava/client`.

Both packages are Apache-2.0 licensed. Source lives at `github.com/beava-dev/beava/beava-js/`.

## Module structure

```
beava-js/
├── packages/
│   ├── beava-node/
│   │   ├── src/
│   │   │   ├── index.ts                # public exports: createBeavaClient, errors, types
│   │   │   ├── create-beava-client.ts  # factory function with all 6 wire methods
│   │   │   ├── beava-error.ts          # BeavaError, BeavaResponseValidationError
│   │   │   └── wire-schemas.ts         # Zod schemas for request/response validation
│   │   └── test/                       # vitest specs (unit + HTTP integration)
│   └── beava-client/
│       └── src/
│           └── index.ts                # re-exports @beava/node API
└── examples/
    └── node-basic.mjs                  # smoke-test example
```

There are deliberately **no** `events.ts` / `col.ts` / `agg.ts` / `table.ts` files — the TS SDK has no authoring layer. See [shared.md § Authoring vs communicate](shared.md#authoring-vs-communicate).

## createBeavaClient factory function

```typescript
import { createBeavaClient } from "@beava/node";

const beava = createBeavaClient({
  baseUrl: "http://127.0.0.1:8080",
  timeoutSeconds: 10,
  headers: { Authorization: "Bearer ..." },  // optional
});

// Wire methods (each returns a Promise<T>)
await beava.ping();                                                      // POST /ping
await beava.register({ nodes, force?, dry_run? });                      // POST /register
await beava.push({ event, data });                                       // POST /push
await beava.get({ table, key, features? });                              // POST /get
await beava.batchGet({ requests });                                      // POST /batch_get
await beava.reset();                                                     // POST /reset
```

Every method accepts an optional `AbortSignal` as its final argument:

```typescript
const controller = new AbortController();
await beava.get({ table: "UserSpend", key: "alice" }, controller.signal);
```

### `ping()`

Posts to `POST /ping`. Returns `PingResponse`:

```typescript
interface PingResponse { pong: true; registry_version: number; }
```

### `register({ nodes, force?, dry_run? })`

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

### `push({ event, data })`

Posts to `POST /push` with body `{ "event": "<name>", "data": {...} }`. Returns `PushResponse`:

```typescript
interface PushResponse {
  ack_lsn: number;
  idempotent_replay: boolean;
  registry_version: number;
}
```

Default semantics: `acks=1` (durable on this server).

### `get({ table, key, features? })`

Posts to `POST /get`. Wire body:

```json
{ "table": "<name>", "key": "<entity_id>" | ["a", 42, true], "features": ["col_a", "col_b"] }
```

`key` is either a `string` or a tuple `(string | number | boolean)[]` for composite keys. `features` is optional; if omitted, all columns are returned. Returns `Record<string, unknown>` (feature row). Cold-start (entity unknown) returns `{}` — never `null`, never an error.

### `batchGet({ requests })`

Posts to `POST /batch_get` with `{requests: [...]}`. Each request has `{ table, key, features? }`. Returns `{ results: JsonObject[] }` in request order. v0 has no partial success: any per-entry error rejects the whole batch and surfaces as a single `BeavaError`.

### `reset()`

Posts to `POST /reset`. The server returns `403` with `{error: {code: "reset_forbidden", ...}}` unless `test_mode` is enabled per Phase 13.4 D-03. The error surfaces verbatim as a `BeavaError`.

## Errors

```typescript
class BeavaError extends Error {
  status: number;       // HTTP status code
  code: string;         // structured error code, e.g. "unsupported_node_kind", "invalid_registration"
  path: string;         // JSON pointer-ish path, e.g. "nodes[0].kind" (empty string if not present)
  errors: unknown[];    // sub-errors (multi-error responses, empty array if not present)
}

class BeavaResponseValidationError extends Error {
  zodError: ZodError;   // validation error from successful response that doesn't match expected schema
}
```

`BeavaError` is thrown by every wire method on non-2xx responses. The `code` field maps to [docs/error-codes.md](../error-codes.md).

`BeavaResponseValidationError` is thrown when a successful (2xx) response doesn't match the expected Zod schema. This usually means the client and server versions disagree about the wire shape.

## TypeScript int64 caveat

JSON has no native int64. The SDK does not coerce numeric fields — `i64` features deserialize as `number` (which loses precision above `2^53`) by default. If you push `i64` values exceeding `Number.MAX_SAFE_INTEGER` and need exact round-tripping, treat the affected fields as strings on the wire.

## Versioning + compatibility

- v0 surface is **frozen** as documented above.
- Node.js 18+ LTS baseline (uses native `fetch` / `AbortController`).
- ESM-only output. No CJS dual publish in v0.
- Apache-2.0 license.

## Cross-references

- **Wire contract:** [docs/wire-spec.md](../wire-spec.md)
- **Cross-language semantics:** [shared.md](shared.md)
- **Authoring SDK (Python):** [python.md](python.md)
- **Go SDK (sister communicate-only client):** [go.md](go.md)
- **Phase 13.6 plan + summary:** `.planning/phases/13.6-typescript-go-sdks/`
