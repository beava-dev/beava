---
title: Cross-language parity
description: The wire-level contract every beava SDK implements — transports, window grammar, key shape, error envelope, naming.
sidebarTitle: Shared
---

# Cross-language parity

The [JSON wire format](../wire-spec.md) is the canonical contract. Every
SDK is a thin compiler from idiomatic syntax to the wire — they own the
DX translation, but not semantics. Behaviour visible to users (cold-start
returns `{}`, schema mismatch raises, batch atomicity, window grammar) is
observable directly through `curl` and every SDK MUST match.

This page is normative for cross-language behaviour. Per-language idioms
live in:

- [Python SDK](python.md) — canonical authoring UX (`@bv.event`, `bv.col`, op helpers).
- [TypeScript SDK](typescript.md) — npm `@beava/sdk`; communicate-only.
- [Go SDK](go.md) — `github.com/beava-dev/beava/sdk/go`; communicate-only.

## Authoring vs communicate

The SDK surface splits into two layers:

| Layer | Available in | What it does |
|---|---|---|
| **Authoring** | Python only | Decorators (`@bv.event`, `@bv.table`), expression DSL (`bv.col`, `bv.lit`), op helpers, pipeline chains, demo loaders. Compiles to JSON descriptors. |
| **Communicate** | Python, TypeScript, Go | `App` constructor, `register(pre-compiled JSON)`, `push`, `pushSync`, `get`, `batchGet`, `reset`, `ping`, `close`. |

Cross-language workflow: author the pipeline in Python → compile to JSON →
ship that JSON to a TS / Go app, which passes it through `app.register(...)`
verbatim. The wire spec is the cross-language contract — a TS app pushing
events against a Python-authored pipeline is observably identical to a
Python app doing the same.

TS / Go authoring may land in v0.1+ if demand justifies it.

## Wire transports

Three transports map to one URL scheme each. URL-scheme dispatch is part of
every SDK's contract.

| Scheme | Transport | Use case |
|---|---|---|
| `http://` / `https://` | HTTP/1.1 + JSON | curl reach, observability, LB / WAF integration |
| `tcp://` | Custom-framed TCP, `[u32 len][u16 op][u8 ct][payload]` | Low-latency fast-path |
| (no URL) | Embed — spawn local `beava` binary on ephemeral ports | Tests, local dev |

In embed mode the SDK locates the binary (via `$BEAVA_BINARY`, `$PATH`,
or a workspace `target/debug/beava` walk), spawns it, and reads the bound
addresses from the binary's stdout JSON log lines (`server.http_bound`,
`server.tcp_bound`). The transport then connects exactly like any other
URL-mode call. Lifecycle is owned by the `App`; closing the App
terminates the embedded subprocess.

Constructor entry points:

| Language | Embed | URL |
|---|---|---|
| Python | `bv.App()` | `bv.App("http://...")` |
| TypeScript | `new BeavaApp()` | `new BeavaApp("http://...")` |
| Go | `beava.NewApp(ctx, "")` | `beava.NewApp(ctx, "http://...")` |

## Window grammar

```
window := digit+ unit | "forever"
unit   := "ms" | "s" | "m" | "h" | "d"
```

Examples: `100ms`, `30s`, `5m`, `1h`, `24h`, `7d`, `forever`.

- Leading digit MUST be `1-9` — `"0ms"` and `"0s"` are rejected.
- Sub-second resolution only via `ms` (e.g. `100ms`); `0.5s` is invalid.
- `"forever"` is the lifetime-mode sentinel (equivalent to omitting
  `window=`). REJECTED for decay operators — exponential decay over an
  unbounded window is mathematically undefined.

All SDKs MUST reject malformed windows client-side. Server-side validation
re-checks for defense-in-depth.

## Key shape

Entity keys come in two shapes:

- **Single key:** a `string`.
- **Composite key:** an array of `[string | number | boolean]`, in the
  same order as the table's declared `key` field.

Composite arrays must use homogeneous element types per position —
`["alice", 42, true]` is fine. SDKs serialise integers as JSON numbers
(not strings) to preserve the type discriminator. Per-language types map
to JSON like this:

| Language | Single | Composite |
|---|---|---|
| Python | `str` | `list[str \| int \| float \| bool]` |
| TypeScript | `string` | `Array<string \| number \| boolean>` (`bigint` rejected — pre-convert if it fits) |
| Go | `string` | `[]any` containing `string` / `int64` / `float64` / `bool` |

### Global tables (`key = ""` sentinel)

A register payload with `key: []` (empty array) declares a **global
table** — single output dict, no per-entity dimension. The wire-level GET
sentinel is `key: ""`.

| Language | Per-entity GET | Global GET |
|---|---|---|
| Python | `app.get("Table", "alice")` | `app.get("Table")` (1-arg) |
| TypeScript | `await app.get("Table", "alice")` | `await app.get("Table")` (overloaded signature) |
| Go | `app.Get(ctx, "Table", "alice")` | `app.GetGlobal(ctx, "Table")` (separate method) |

All three produce the same wire request (`{"table": "...", "key": ""}`).
Per-language ergonomics differ: dynamic-typed Python uses arity overloading;
TS uses overloaded signatures with compile-time arity check; Go uses an
explicit method (idiomatic for Go's static typing).

## Field types

The 6-element vocabulary, mapped from wire to language:

| Wire | Python | TypeScript | Go |
|---|---|---|---|
| `str` | `str` | `string` | `string` |
| `f64` | `float` | `number` | `float64` |
| `i64` | `int` | `number` (safe up to `2^53 - 1`) | `int64` |
| `bool` | `bool` | `boolean` | `bool` |
| `bytes` | `bytes` | `Uint8Array` | `[]byte` |
| `datetime` | `datetime.datetime` | `Date` | `time.Time` |

### Optional / nullable

The field MAY be absent from a push payload; the registered schema
records it as nullable. Wire form is `bv.Optional[T]` / `T | null` /
`*T`; the SDK marks the field in `schema.optional_fields`. A required
field missing from a push returns `missing_field`.

## FeatureResult — `app.get(...)` return shape

Flat dict / object / map of `feature_name -> value` — no wrapper, no
envelope:

| Language | Type |
|---|---|
| Python | `dict[str, Any]` |
| TypeScript | `Record<string, any>` (or generic `<T>`) |
| Go | `map[string]any` (or strongly-typed via codegen — v0.1+) |

<Tip>
**Cold-start returns `{}`** in all three SDKs — empty dict, not error,
not 404. A cold key is a key with no data. `unknown_table` IS an error.
</Tip>

## ValidationError envelope

```json
{
  "kind": "<one-of-9>",
  "path": "<DAG/JSON path>",
  "message": "<human-readable, forward-looking framing>"
}
```

The 9 frozen `kind` values:

| Kind | When |
|---|---|
| `cycle` | Descriptor list forms a cycle through `upstreams`. |
| `missing_upstream` | A `derivation` references an upstream not declared in this batch and not previously registered. |
| `schema_mismatch` | A push field has the wrong type and cannot be coerced; or `bv.sum` field arg is not a `string`. |
| `bad_return_type` | A function-form `@bv.event` returns the wrong descriptor shape. |
| `unknown_field_type` | Field type annotation outside the supported vocabulary. |
| `table_key_invalid` | Composite-key shape is malformed at register time. |
| `registration_conflict` | Destructive change without `force=true`. |
| `duplicate_name` | Two descriptors in the same register call have the same name. |
| `unsupported_node_kind` | Body has `kind="upsert"`/`"delete"`/`"retract"` etc. — pre-12.7 surface that's permanently killed. |

Adding new kinds requires an ADR. Message text follows a **forward-looking
framing** — "X is not supported in v0", not "X has been removed". This
avoids implying a previous-version reference.

## Schema evolution flags

| Flag | Type | Default | Behaviour |
|---|---|---|---|
| `force` | bool | `false` | Permits destructive register (field type change, removal). Server accepts and zeroes affected aggregations. Without it, returns `409` + `registration_conflict`. |
| `dry_run` | bool | `false` | Returns the diff without applying. Body: `{added, removed, changed, diff}`. `registry_version` not bumped. |

Per-language idioms:

| Language | Shape |
|---|---|
| Python | keyword-only — `app.register(*descs, force=False, dry_run=False)` |
| TypeScript | options object — `app.register(descs, { force: false, dryRun: false })` |
| Go | functional options — `app.Register(ctx, descs, beava.WithForce(), beava.WithDryRun())` |

The flags compose: `force=true` + `dry_run=true` returns the diff for the
destructive change without applying it. Useful for migration tooling.

## Naming convention

| Layer | Convention | Example |
|---|---|---|
| Wire JSON keys | `snake_case` (frozen) | `event_name`, `registry_version`, `cold_after_ms` |
| Python public API | `snake_case` (PEP 8) | `app.batch_get(...)`, `bv.n_unique(...)` |
| TypeScript public API | `camelCase` | `app.batchGet(...)`, `bv.nUnique(...)` |
| Go public API | `PascalCase` | `App.BatchGet(...)`, `beava.NUnique(...)` |

SDKs translate field names automatically at the transport layer — users
write idiomatic per-language code; the SDK serialises to wire `snake_case`
on the way out and back to the language idiom on the way in.

## Error semantics

| Class | Cold-start | Behaviour |
|---|---|---|
| Cold-start (`{}` for unknown key) | NOT an error | All SDKs return empty dict / object / map. |
| Schema mismatch on push | ERROR | Python raises `RegistrationError` (push variant: `ValidationError`). TS throws `RegistrationError`. Go returns `error`. |
| Unknown event/table | ERROR with `unknown_event` / `unknown_table` | Language-idiomatic surfacing. |
| Validation error on register | ERROR | `RegistrationError` carries ALL errors in `.errors` (Python: `errors: list[ValidationError]`; TS: `errors: ValidationError[]`; Go: `Errors []ValidationError`). |

<Warning>
**No partial success on `OP_BATCH_GET`.** If any per-entry request fails
(e.g., one bad table), the entire frame returns `OP_ERROR_RESPONSE` with
the offending entry indexed in the `path` field (e.g., `requests[2].table`).
SDKs surface this as a single language-idiomatic error — they do NOT
return partial results plus per-entry exceptions. Re-issue with the bad
request removed. Partial success is reserved for v0.1+.
</Warning>

## Cross-language API surface map

The communicate surface every SDK MUST implement:

| Wire opcode | Python | TypeScript | Go |
|---|---|---|---|
| `OP_REGISTER` | `app.register(*descriptors, force=False, dry_run=False)` | `app.register(descriptors, { force, dry_run })` | `app.Register(ctx, descriptors, beava.WithForce(), beava.WithDryRun())` |
| `OP_PUSH` | `app.push(event_name, fields)` | `app.push(eventName, fields)` | `app.Push(ctx, eventName, fields)` |
| `OP_PUSH_SYNC` | `app.push_sync(event_name, fields)` | `app.pushSync(eventName, fields)` | `app.PushSync(ctx, eventName, fields)` |
| `OP_GET` (per-entity) | `app.get(table, key)` | `app.get(table, key)` | `app.Get(ctx, table, key)` |
| `OP_GET` (global) | `app.get(table)` (1-arg) | `app.get(table)` (overloaded) | `app.GetGlobal(ctx, table)` |
| `OP_BATCH_GET` | `app.batch_get(requests)` | `app.batchGet(requests)` | `app.BatchGet(ctx, requests)` |
| `OP_RESET` | `app.reset()` | `app.reset()` | `app.Reset(ctx)` |
| `OP_PING` | `app.ping()` | `app.ping()` | `app.Ping(ctx)` |
| close | `app.close()` / context manager | `app.close()` | `app.Close(ctx)` / `defer` |

The authoring surface (`@bv.event`, `@bv.table`, expressions, op helpers,
`bv.demo`) lives in [Python](python.md) only.
