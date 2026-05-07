# Shared SDK Semantics

> **Status:** Authoritative for v0. The 3 v0 SDKs (Python, TypeScript, Go) MUST implement the **communicate** surfaces in this doc with semantic parity. The **authoring** layer (decorators, expression DSL, op helpers) is **Python-only** in v0 — see "Authoring vs communicate" below.
> **Last reviewed:** 2026-05-03 (Phase 13.6).

## Overview

beava's [JSON wire format](../wire-spec.md) is the **canonical contract**.
Per-language SDKs are thin compilers from idiomatic syntax to the wire — they
own the developer-experience translation, but they do not own semantics. Every
behavior visible to a user (cold-start returns `{}`, schema mismatch raises,
batch atomicity, window grammar) is observable directly through `curl` against
the wire spec, and every SDK MUST match.

This document is **normative for cross-language behavior**. Per-language
idioms (naming style, sync vs async, error patterns, decorator vs builder
syntax) are documented in the per-language docs:

- [Python SDK](python.md) — canonical authoring UX (`@bv.event`, `bv.col`, `bv.count(...)`).
- [TypeScript SDK](typescript.md) — npm package `@beava/sdk`; communicate-only (no DSL).
- [Go SDK](go.md) — module `github.com/beava-dev/beava/sdk/go`; context-aware methods + functional options; communicate-only (no DSL).

When Python prose and the cross-language semantics in this doc disagree, this
doc wins — Python is the **canonical implementation** but this is the
**canonical contract**.

## Authoring vs communicate

beava v0 splits its public SDK surface into two layers:

| Layer | Available in | Description |
|-------|--------------|-------------|
| **Authoring** | Python only | Decorators (`@bv.event`), expression DSL (`bv.col`, `bv.lit`), op helpers (`bv.count`, `bv.sum`, ...), pipeline chains (`events.filter().group_by().agg()`), demo loaders (`bv.demo("fraud")`). Compiles to JSON descriptors. |
| **Communicate** | All three SDKs (Python, TypeScript, Go) | `App` constructor + URL-scheme dispatch, `register(pre-compiled JSON)`, `push`, `pushSync`, `get`, `batchGet`, `reset`, `ping`, `close`. |

**Cross-language workflow:**

1. Author the pipeline in Python via the DSL.
2. Compile to JSON via `bv.App.register_json(...)` or by serializing descriptors.
3. Ship that JSON to a TypeScript / Go application — they pass it through verbatim to `app.register(...)` and never inspect it.

This split was locked 2026-05-03 (Phase 13.6 scope amendment) per the user directive: "pipeline sdk in python only; only communicate sdk are in all languages." The TS+Go SDKs deliberately ship NO DSL files (no `events.ts`/`col.ts`/`agg.ts`/`table.ts`; same for Go). Their `Descriptor` type is opaque pass-through (`Record<string, unknown>` in TS, `map[string]any` in Go).

This is not a permanent constraint — TS/Go authoring may land in v0.1+ if demand justifies it. The key insight is that **the wire spec is the cross-language contract**, not the authoring DSL: a TS app pushing events against a Python-authored pipeline is observably identical to a Python app doing the same.

## Wire transports

Three transports map to one URL scheme each. URL-scheme dispatch is part of
every SDK's contract — the user passes the URL, and the SDK selects the
transport.

| Scheme | Transport | Use case | Spec |
|--------|-----------|----------|------|
| `http://host:port` / `https://host:port` | HTTP/1.1 + JSON | curl reach, observability, LB / WAF integration | [docs/http-api.md](../http-api.md) |
| `tcp://host:port` | Custom-framed TCP, `[u32 length][u16 op][u8 ct][payload]` | Low-latency fast-path, fraud / ad-tech serving | [docs/wire-spec.md](../wire-spec.md) |
| (no URL) | Embed mode — spawn local `beava` binary on ephemeral ports | Tests, local dev, in-process default | [docs/concepts/embed-mode.md](../concepts/embed-mode.md) (forward-ref Plan 13.0-13) |

Embed mode is the default when the user constructs an `App` with no URL:

- Python: `bv.App()` with no argument.
- TypeScript: `new beavaApp()` with no argument.
- Go: `beava.NewApp(ctx, "")` with an empty URL string.

In embed mode the SDK locates the `beava` binary (via `$BEAVA_BINARY`,
`$PATH`, or a workspace `target/debug/beava` walk), spawns it on ephemeral
ports, and reads the bound addresses from the binary's stdout JSON log lines
(`server.http_bound` and `server.tcp_bound`). The transport then connects to
those addresses just like any other URL-mode call. Lifecycle is owned by the
`App` instance; closing the App terminates the embedded subprocess.

## Window grammar

Windows for streaming-aggregation operators use a single grammar across all
languages:

```
window := digit+ unit | "forever"
unit   := "ms" | "s" | "m" | "h" | "d"
```

Examples (parse-equivalent across all 3 SDKs): `100ms`, `30s`, `5m`, `1h`,
`24h`, `7d`, `forever`.

Validation rules:

- Leading digit MUST be `1-9` — `"0ms"` and `"0s"` are rejected.
- Sub-second resolutions are supported only via `ms` (e.g. `100ms`); `0.5s`
  is invalid.
- The literal `"forever"` is the **lifetime mode** sentinel — equivalent to
  omitting `window=` entirely. `forever` is REJECTED for decay operators
  (`ewma`, `ewvar`, `decayed_sum`, etc.) since exponential decay over an
  unbounded window is mathematically undefined.

All 3 SDKs MUST reject malformed windows at decorator / builder time
(client-side, before the wire call). Server-side validation re-checks for
defense-in-depth.

## Key shape

Entity keys (the `key` field on `OP_GET` / `OP_BATCH_GET` requests) come in
two shapes:

- **Single key:** a `string`.
- **Composite key:** an array of `[string | number | boolean]` items, in the
  same order as the table's declared `key` field.

Cross-language hash equality is achieved via stringification at the server
boundary (FxHash on the server-side `EntityKey` struct). Composite-key arrays
must use homogeneous element types per position — e.g., `["alice", 42, true]`
is fine, but the SDK must serialise integers as JSON numbers (not as JSON
strings) to preserve the type discriminator.

The SDK is responsible for translating idiomatic per-language types
(Python `int`, JS `number`, Go `int64`) into the appropriate JSON form on
the wire. v0 specifically:

- Python: `[str | int | float | bool]` items map to JSON `[string | number | boolean]`.
- TypeScript: `Array<string | number | boolean>` maps directly. JS `bigint`
  is REJECTED in v0 (clients should pre-convert to `number` if they fit).
- Go: `[]any` containing `string` / `int64` / `float64` / `bool` items.
  Other types in the slice raise an error before the wire call.

### Global-table sentinel (`key = ""`) — per ADR-003

Per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md), a register payload with `key: []` (empty array) declares a **global table** — single output dict, no per-entity dimension. The wire-level GET sentinel is `key: ""` (empty string).

| Language | Per-entity GET | Global GET |
|---|---|---|
| Python | `app.get("Table", "alice")` | `app.get("Table")` (1-arg overload) |
| TypeScript | `await app.get("Table", "alice")` | `await app.get("Table")` (overloaded signature) |
| Go | `app.Get(ctx, "Table", "alice")` | `app.GetGlobal(ctx, "Table")` (separate method) |

All three SDKs produce the same wire request when querying a global table (`{"table": "...", "key": ""}`). Per-language ergonomics differ to match each ecosystem's conventions:

- **Python** uses arity overloading — natural in dynamic typing.
- **TypeScript** uses overloaded signatures with type-level enforcement (compile-time arity check).
- **Go** uses a separate method — Go's static typing makes arity overloading awkward; explicit method names are idiomatic.

The wire shape is identical — only the per-language client surface differs. Global-table state is bounded by the per-table state size (single slot), independent of entity count. See [`docs/concepts/global-aggregation.md`](../concepts/global-aggregation.md) for the full conceptual treatment.

## Public expression literal (`bv.lit`) — Python-only in v0

Per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md), Python exposes a public literal factory:

```python
bv.lit(value: int | float | str | bool | None) -> Expr
```

**TS / Go do NOT expose `bv.lit`** — both SDKs are communicate-only in v0 (no expression layer). Cross-language workflow: Python authors expressions including literals → compiles to JSON → TS/Go ship the pre-compiled JSON.

If TS/Go authoring lands in v0.1+, they would expose mirror factories (`bv.lit(...)` / `beava.Lit(...)`); that's deferred per the Phase 13.6 communicate-only rescope.

## Field types

The 6-element field-type vocabulary is shared across all languages. Wire
representation (the string used in the registered schema) is the **wire
name** column; per-language SDKs use idiomatic types and translate at the
boundary.

| Wire name | Python | TypeScript | Go | Notes |
|-----------|--------|------------|-----|-------|
| `str` | `str` | `string` | `string` | UTF-8 strings; max length per server config. |
| `f64` | `float` | `number` | `float64` | IEEE 754 double precision. |
| `i64` | `int` | `number` | `int64` | TypeScript has no native 64-bit integer; values up to `Number.MAX_SAFE_INTEGER` (`2^53 - 1`) are safe. Larger integers MUST be sent as strings (subject to TS-13.6 design — see [typescript.md](typescript.md)). |
| `bool` | `bool` | `boolean` | `bool` | |
| `bytes` | `bytes` | `Uint8Array` | `[]byte` | Base64-encoded on the JSON wire; SDKs decode/encode transparently. |
| `datetime` | `datetime.datetime` | `Date` | `time.Time` | ISO 8601 (RFC 3339) on the JSON wire; SDKs parse / serialise. |

### Optional / nullable fields

`Optional[T]` semantics (across all 3 SDKs): the field MAY be absent from a
push payload, and the registered schema records it as nullable. The wire
form is the same Python `bv.Optional[T]` / TypeScript `T | null` / Go
`*T` — the SDK marks the field as `optional` in the
`schema.optional_fields` list inside the register descriptor.

A required field that is missing from a push payload returns
`missing_field` per [docs/error-codes.md](../error-codes.md) (forward-ref
Plan 13.0-12).

## FeatureResult shape

The result of `app.get(...)` is the **row-shape** — a flat dict / object /
map of feature name → value, exactly as defined in the wire spec for
`OP_GET` responses. No wrapper object, no envelope.

Cross-language surface:

- Python: `dict[str, Any]`.
- TypeScript: `Record<string, any>` (or generic `<T>` with a typed result —
  see [typescript.md](typescript.md)).
- Go: `map[string]any` (or strongly-typed result via codegen — v0.1+).

**Cold-start semantics:** if no events have ever been pushed for the
queried `(table, key)` pair, all 3 SDKs return an **empty dict / object /
map** (`{}`). This is **not** an error and **not** the same as the table
being absent — `unknown_table` is a separate error code per
[docs/wire-spec.md](../wire-spec.md#op_get-0x0020) `OP_GET` errors.

The motivation is the Redis-shaped contract: cold keys are just keys with
no data, not a 404-class condition. SDK ports MUST surface cold-start as
`{}` and not raise.

## ValidationError envelope

The cross-language schema for the validation-error envelope matches
`python/beava/_errors.py::ValidationError`:

```json
{
  "kind": "<one-of-9>",
  "path": "<DAG/JSON path>",
  "message": "<human-readable, forward-looking framing per Phase 12.7 D-02>"
}
```

The 9 `kind` values are **frozen for v0**; new kinds require an ADR:

| Kind | When |
|------|------|
| `cycle` | Descriptor list forms a cycle through `upstreams`. |
| `missing_upstream` | A `derivation` references an upstream not declared in this batch and not previously registered. |
| `schema_mismatch` | A push field has the wrong type and cannot be coerced; or `bv.sum` field arg is not a `string` per Q1 Path B. |
| `bad_return_type` | A function-form `@bv.event` returns the wrong descriptor shape. |
| `unknown_field_type` | Field type annotation is not in the supported vocabulary (str / f64 / i64 / bool / bytes / datetime). |
| `table_key_invalid` | Composite-key shape is malformed at register time. |
| `registration_conflict` | Destructive change (field type change, field removal) without `force=true`. |
| `duplicate_name` | Two descriptors in the same register call have the same name. |
| `unsupported_node_kind` | Body has `kind="upsert"`/`"delete"`/`"retract"` etc. — pre-12.7 surface that is permanently killed per `project_v0_events_only_scope`. |

`message` text follows the **forward-looking framing** locked in Phase 12.7
D-02: messages say "X is not supported in v0", **not** "X has been removed"
or "X was deprecated". This avoids implying a previous-version reference
for users who never saw older revisions.

## Schema evolution

The `force` and `dry_run` register-time flags are cross-language register
knobs (per [docs/wire-spec.md OP_REGISTER](../wire-spec.md#op_register-0x0001)):

| Flag | Type | Default | Behavior |
|------|------|---------|----------|
| `force` | bool | `false` | Permits destructive register (e.g., field type change, field removal). The server accepts the change and zeroes affected aggregations. Without `force`, destructive changes return `409 Conflict` with `registration_conflict`. |
| `dry_run` | bool | `false` | Returns the diff without applying. Response body: `{added, removed, changed, diff}`. `registry_version` is NOT bumped. |

Per-language idiom for these flags:

- Python: keyword-only — `app.register(*descs, force=False, dry_run=False)`.
- TypeScript: camelCase options object — `app.register(descs, { force: false, dryRun: false })`.
- Go: functional options — `app.Register(ctx, descs, beava.WithForce(), beava.WithDryRun())`.

The flags compose: `force=true` + `dry_run=true` returns the diff for the
destructive change without applying it, useful for migration tooling.

## Cross-language naming convention

Spec-level normative rules:

| Layer | Convention | Example |
|-------|-----------|---------|
| Wire JSON keys | `snake_case` | `event_name`, `registry_version`, `cold_after_ms` |
| Python public API | `snake_case` (PEP 8) | `app.batch_get(...)`, `bv.n_unique(...)` |
| TypeScript public API | `camelCase` | `app.batchGet(...)`, `bv.nUnique(...)` |
| Go public API | `PascalCase` | `App.BatchGet(...)`, `beava.NUnique(...)` |

All 3 languages serialize / deserialize automatically at the transport
layer; users write idiomatic per-language code, and the SDK translates
field names to wire `snake_case` on the way out and back to the
language's idiomatic shape on the way in.

The wire `snake_case` discipline is **frozen** — adding new wire fields
requires they use `snake_case` to preserve cross-language uniformity.

## Error semantics

Cross-language behavior for each error class:

| Class | Cold-start | Behavior |
|-------|-----------|----------|
| Cold-start (`{}` for unknown key) | NOT an error — returns empty row/object/map | All 3 SDKs return empty dict/object/map. |
| Schema mismatch on push | ERROR | Python raises `RegistrationError` (push variant: `ValidationError`). TS throws `RegistrationError`. Go returns `error`. |
| Unknown event/table on push or get | ERROR with `unknown_event` / `unknown_table` | Same as schema mismatch — language-idiomatic surfacing. |
| Validation error on register | ERROR — `RegistrationError` carrying ALL errors in `.errors` (Python: `errors: list[ValidationError]`; TS: `errors: ValidationError[]`; Go: `Errors []ValidationError`). |

**Batch atomicity (`OP_BATCH_GET`):** v0 has **no partial success**. If
any single per-entry request fails (e.g., one bad table), the entire
frame returns `OP_ERROR_RESPONSE` with the offending entry indexed in
the `path` field (e.g., `requests[2].table`). All 3 SDKs surface this
as a single language-idiomatic error — they do NOT return partial
results plus per-entry exceptions. Clients re-issue with the bad
request removed. Partial success is reserved for v0.1+ per
[docs/error-codes.md](../error-codes.md) `batch_too_large`.

## Lifetime aggregation rules (cross-language register-time validation)

Per [V0-MEM-GOV-02](../../.planning/REQUIREMENTS.md) (Phase 12.8):

> Lifetime aggregations (windowless mode — `window=` omitted or set to
> `"forever"`) MUST declare a finite per-entity memory ceiling at register-time.

Server-side enforcement: the JSON-prelude shim returns
`code: "unbounded_op_in_lifetime_mode"` if a register payload places an
unbounded operator in lifetime mode. The shim is default-on per Phase
12.8 Plan 06; the env-var `BEAVA_MEMORY_GOV_ENFORCE=0` disables it
(operators MUST NOT disable in production).

All 3 SDKs SHOULD validate this client-side (not strictly required, but
recommended) so the user gets fast feedback. The catalogue of bounded
vs unbounded ops in lifetime mode lives at
[docs/architecture/memory-budget.md](../architecture/memory-budget.md)
(forward-ref Plan 13.0-13). v0 enforces this on the server regardless
of SDK behavior.

## Cross-language API surface map

For quick reference, here is the canonical **communicate** surface that every SDK MUST implement:

| Wire opcode | Python | TypeScript | Go |
|-------------|--------|------------|-----|
| `OP_REGISTER` | `app.register(*descriptors, force=False, dry_run=False)` | `app.register(descriptors, { force, dry_run })` | `app.Register(ctx, descriptors, beava.WithForce(), beava.WithDryRun())` |
| `OP_PUSH` | `app.push(event_name, fields)` | `app.push(eventName, fields)` | `app.Push(ctx, eventName, fields)` |
| `OP_PUSH_SYNC` (reserved) | `app.push_sync(event_name, fields)` | `app.pushSync(eventName, fields)` | `app.PushSync(ctx, eventName, fields)` |
| `OP_GET` (per-entity) | `app.get(table, key)` | `app.get(table, key)` | `app.Get(ctx, table, key)` |
| `OP_GET` (global, ADR-003) | `app.get(table)` (1-arg) | `app.get(table)` (1-arg overload) | `app.GetGlobal(ctx, table)` (separate method) |
| `OP_BATCH_GET` | `app.batch_get(requests)` | `app.batchGet(requests)` | `app.BatchGet(ctx, requests)` |
| `OP_RESET` | `app.reset()` | `app.reset()` | `app.Reset(ctx)` |
| `OP_PING` | `app.ping()` | `app.ping()` | `app.Ping(ctx)` |
| (close lifecycle) | `app.close()` / context manager | `app.close()` | `app.Close(ctx)` / `defer` |

The **authoring** surface (`@bv.event`, `@bv.table`, expressions, op helpers, `bv.demo`) lives in Python only — see "Authoring vs communicate" above.

Each language doc fills in the per-language signature details with full type annotations, error semantics, and lifecycle expectations.

## Plan-level traceability

This document is authored by Plan 13.0-04 (Wave 1). Downstream consumers:

- [Python SDK](python.md), [TypeScript SDK](typescript.md), [Go SDK](go.md) — per-language docs in the same plan import the cross-language rules above.
- **Phase 13.5** — Python SDK rewrite implements the canonical surface.
- **Phase 13.6** — TypeScript + Go SDK ports implement the per-language docs.
- **Phase 13.4** — engine validates the wire contract that all 3 SDKs target.

For the full Phase 13.0 plan tree, see
[`.planning/phases/13.0-design-contract-spec-docs/13.0-PLAN.md`](../../.planning/phases/13.0-design-contract-spec-docs/13.0-PLAN.md).
