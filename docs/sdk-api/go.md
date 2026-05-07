# beava Go SDK

> **Communicate-only SDK.** This SDK pushes events, registers pre-compiled JSON descriptors, and reads features. Pipeline authoring (event sources, expression DSL, op helpers) lives in the **Python SDK only** — see [python.md](python.md). Use Python's `bv.App.register_json(...)` to produce descriptors, then ship that JSON to your Go service. Or hand-write the JSON per [docs/wire-spec.md OP_REGISTER](../wire-spec.md#op_register-0x0001).

> **Status:** Authoritative for v0. Documents the post-13.6 Go SDK shape (rescoped 2026-05-03 to communicate-only). Cross-language semantics live in [shared.md](shared.md); wire-level body shapes live in [docs/wire-spec.md](../wire-spec.md). Python is the canonical authoring reference.
>
> **Last reviewed:** 2026-05-03 (Phase 13.6).

## Overview

The beava Go SDK ships as `github.com/beava-dev/beava/sdk/go`. It is a wire-thin client targeting Go 1.22+, idiomatic Go patterns:

- **`context.Context`-aware** — every wire-bound method takes `ctx context.Context` as its first argument and respects `ctx.Done()` for cancellation.
- **Functional options** — `App` and `Register` use options structs (`WithTimeout`, `WithTestMode`, `WithBinaryPath`, `WithForce`, `WithDryRun`).
- **PascalCase exported identifiers + json snake_case tags** — Go convention; the wire layer maps PascalCase Go field names to wire `snake_case` via `json:"..."` struct tags.
- **No DSL** — `Descriptor` is `map[string]any`; the SDK does not parse or compile authoring expressions.

> **Module:** `github.com/beava-dev/beava/sdk/go`. Install via
> `go get github.com/beava-dev/beava/sdk/go`. Import as
> `import beava "github.com/beava-dev/beava/sdk/go"`. The SDK targets Go 1.22+.
>
> Source layout: monorepo at `github.com/beava-dev/beava/`, Go SDK lives at `sdk/go/` subdirectory (per Phase 13.6 D-02).

## Module structure

```
sdk/go/
├── go.mod
├── beava.go               # App struct, NewApp, URL-scheme dispatch, transport interface, Close
├── app.go                 # method receivers: Register/Push/PushSync/Get/GetGlobal/BatchGet/Reset/Ping
├── wire.go                # frame codec + opcode constants (CT_JSON only in v0)
├── transport_http.go      # httpTransport (net/http; structured error envelope decoding)
├── transport_tcp.go       # tcpTransport (net.Conn, Redis-style FIFO queue)
├── embed.go               # SpawnEmbeddedServer + Teardown + discoverBinary
├── types.go               # Descriptor, FeatureResult, RegisterResult, PushResult, PingResult, GetRequest
├── errors.go              # RegistrationError, ValidationError, BinaryNotFoundError
└── *_test.go              # standard testing + httptest
```

There are deliberately **no** `events.go` / `col.go` / `agg.go` / `table.go` files — the Go SDK has no authoring layer. See [shared.md § Authoring vs communicate](shared.md#authoring-vs-communicate).

## App struct

```go
package beava

type App struct { /* ... */ }

func NewApp(ctx context.Context, url string, opts ...AppOption) (*App, error)

// Wire methods
func (a *App) Register(ctx context.Context, descriptors []Descriptor, opts ...RegisterOption) (*RegisterResult, error)
func (a *App) Push(ctx context.Context, eventName string, fields map[string]any) (*PushResult, error)
func (a *App) PushSync(ctx context.Context, eventName string, fields map[string]any) (*PushResult, error)
func (a *App) Get(ctx context.Context, table string, key any) (FeatureResult, error)        // per-entity
func (a *App) GetGlobal(ctx context.Context, table string) (FeatureResult, error)           // global table per ADR-003
func (a *App) BatchGet(ctx context.Context, requests []GetRequest) ([]FeatureResult, error)
func (a *App) Reset(ctx context.Context) error
func (a *App) Ping(ctx context.Context) (*PingResult, error)
func (a *App) Close(ctx context.Context) error                                              // idempotent
```

### `NewApp(ctx, url, opts...)` + URL-scheme dispatch

The `url` argument selects the transport:

| URL form | Transport | Notes |
|----------|-----------|-------|
| `http://host:port` / `https://host:port` | HTTP/JSON via `net/http` | Default. |
| `tcp://host:port` | Custom-framed TCP (`[u32 length][u16 op][u8 ct][payload]`) | Lowest-latency; Redis-style FIFO. |
| `""` | Embed mode | Spawns local `beava` binary on first call; auto-reaped on `Close()`. |

App options:

- `WithTimeout(d time.Duration)` — per-request I/O timeout (default `30s`).
- `WithTestMode()` — passes `BEAVA_TEST_MODE=1` to the embed-mode subprocess (mirrors Python `bv.App(test_mode=True)`).
- `WithBinaryPath(p string)` — overrides the embed-mode binary discovery path.

### `Register(ctx, descriptors, opts...)`

Submit a list of pre-compiled register node JSON blobs to `POST /register`. Wire body:

```json
{ "nodes": [<descriptor>, ...], "force": false, "dry_run": false }
```

`Descriptor = map[string]any` — opaque; the SDK does not validate or compile.

Register options:

- `WithForce()` — set wire `force=true` (allows destructive schema changes).
- `WithDryRun()` — set wire `dry_run=true` (validate without applying).

Returns `*RegisterResult`:

```go
type RegisterResult struct {
    Status          string   `json:"status"`
    RegistryVersion int64    `json:"registry_version"`
    Added           []string `json:"added,omitempty"`
    Removed         []string `json:"removed,omitempty"`
    Changed         []string `json:"changed,omitempty"`
}
```

### `Push(ctx, eventName, fields)` and `PushSync(ctx, ...)`

`Push` posts to `POST /push/<eventName>` with `{fields: {...}}`. Default `acks=1`.

`PushSync` is reserved for `acks=all` (multi-replica) durable push in v0.1+ per [docs/wire-spec.md OP_PUSH_SYNC](../wire-spec.md). v0 delegates to `Push`.

### `Get(ctx, table, key)` and `GetGlobal(ctx, table)`

Per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md), Go uses **separate methods** rather than arity overloading (which Go doesn't support):

- `Get(ctx, table, key)` — per-entity. `key` is `any` — accepts `string` (single-column) or `[]any` (composite).
- `GetGlobal(ctx, table)` — global aggregation. Wire body uses `key: ""` (the empty-string sentinel).

Wire body (both methods):

```json
{ "table": "<name>", "key": "<entity_id>" | ["a", 42, true] | "" }
```

Returns `FeatureResult = map[string]any`. Cold-start (entity unknown) returns an empty (non-nil) map, never an error.

### `BatchGet(ctx, requests)`

Posts to `POST /batch-get` with `{requests: [...]}`. Returns `[]FeatureResult` in request order. v0 has no partial success: any per-entry error rejects the whole batch and surfaces as `*RegistrationError`.

### `Reset(ctx)`

Posts to `POST /reset`. The server returns `403` with `{error: {code: "reset_forbidden", ...}}` unless `test_mode` is enabled per Phase 13.4 D-03. Surfaces as `*RegistrationError`.

### `Ping(ctx)`

Calls `GET /health`. Returns:

```go
type PingResult struct {
    ServerVersion   string `json:"server_version"`
    RegistryVersion int64  `json:"registry_version"`
}
```

### `Close(ctx)`

Idempotent. Closes the underlying transport. In embed mode, sends `SIGTERM` and `SIGKILL`s after timeout; removes the per-instance temp CWD.

## Errors

```go
type RegistrationError struct {
    Code    string            `json:"code"`     // e.g., "unsupported_node_kind"
    Path    string            `json:"path,omitempty"`
    Message string            `json:"message"`
    Errors  []ValidationError `json:"errors,omitempty"`
}

func (e *RegistrationError) Error() string

type ValidationError struct {
    Kind    string `json:"kind"`
    Path    string `json:"path"`
    Message string `json:"message"`
}

type BinaryNotFoundError struct {
    Searched []string
    Reason   string
}

func (e *BinaryNotFoundError) Error() string
```

`*RegistrationError` is returned by every wire method on non-2xx HTTP responses or `OpErrorResponse` TCP frames. Use `errors.As(err, &regErr)` to inspect:

```go
var regErr *beava.RegistrationError
if errors.As(err, &regErr) {
    if regErr.Code == "unsupported_node_kind" {
        // ...
    }
}
```

## Embed mode

```go
import beava "github.com/beava-dev/beava/sdk/go"

ctx := context.Background()
app, err := beava.NewApp(ctx, "", beava.WithTestMode()) // empty URL = embed
if err != nil {
    log.Fatal(err)
}
defer app.Close(ctx)
```

The first call to a wire method spawns the local `beava` binary on ephemeral ports in a fresh temp CWD. Discovery: `BEAVA_BINARY` env / `exec.LookPath("beava")` / walk parents for `target/debug/beava` / fail with `*BinaryNotFoundError`.

## Test fixtures

For tests, use `App` with embed mode and `WithTestMode()`:

```go
func TestThing(t *testing.T) {
    ctx := context.Background()
    app, err := beava.NewApp(ctx, "", beava.WithTestMode())
    if err != nil { t.Fatal(err) }
    defer app.Close(ctx)
    // ...
}
```

For mock-server tests against a fake HTTP backend, use the standard `net/http/httptest` package:

```go
srv := httptest.NewServer(http.HandlerFunc(...))
defer srv.Close()
app, _ := beava.NewApp(ctx, srv.URL)
defer app.Close(ctx)
```

## Versioning + compatibility

- v0 surface is **frozen** as documented above.
- Go 1.22+ baseline.
- Apache-2.0 license.
- Module path `github.com/beava-dev/beava/sdk/go` is permanent for v0; if the monorepo splits in v0.1+, a `go get` redirect at `github.com/beava-dev/beava-go` is the migration path (deferred until a real signal is needed).

## Cross-references

- **Wire contract:** [docs/wire-spec.md](../wire-spec.md)
- **Cross-language semantics:** [shared.md](shared.md)
- **Authoring SDK (Python):** [python.md](python.md)
- **TS SDK (sister communicate-only client):** [typescript.md](typescript.md)
- **Phase 13.6 plan + summary:** `.planning/phases/13.6-typescript-go-sdks/`
