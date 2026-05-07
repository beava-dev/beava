# Embed Mode

`bv.App()` with no URL spawns a local `beava` binary as a subprocess on
ephemeral ports. This is **embed mode** — beava-the-server runs in-process
under your Python interpreter for the lifetime of the `App`. There is no
remote infrastructure, no port allocation, and no setup. You go from
`pip install "git+https://github.com/beava-dev/beava.git#subdirectory=python"` to a registered, queryable feature server in one line.

Embed mode exists for notebooks, scripts, pytest fixtures, and the
`bv.demo()` quickstart. It is not how you ship to production.

## How it works

```python
import beava as bv

app = bv.App()  # no URL → embed mode
app.register(...)
app.push(...)
features = app.get(...)
```

Behind the scenes the SDK:

1. Discovers a `beava` binary on the system (see § Binary discovery).
2. Spawns it as a child `subprocess.Popen` on two ephemeral ports
   (TCP for the data-plane fast path, HTTP for the admin sidecar).
3. Waits up to 5 seconds for the binary to log its bound addresses.
4. Wires the SDK's transport at those addresses.
5. Tears the subprocess down on `app.close()` or context-manager exit.

Subprocess `stdout` / `stderr` are captured via background reader threads
and surfaced through Python `logging` at `INFO` / `WARN` for debugging.

## Binary discovery

`python/beava/_embed.py::discover_binary()` searches in this fixed order:

1. **`$BEAVA_BINARY` env var.** If set, the path MUST exist and be
   executable; otherwise raises `BinaryNotFoundError` immediately. There
   is no fallthrough — explicit override means explicit override.
2. **`beava` on `$PATH`** via `shutil.which`. Standard search.
3. **`./target/debug/beava`** walked upward from the current working
   directory. This is the dev-loop convenience: build the server with
   `cargo build`, then `python my_script.py` from anywhere inside the
   repo.
4. **Raise `BinaryNotFoundError`** with install guidance:

   ```text
   beava binary not found. Install with one of:
     brew install beava
     pip install "git+https://github.com/beava-dev/beava.git#subdirectory=python"
     docker pull beava/beava
   Or set BEAVA_BINARY=/path/to/beava.
   ```


The 4-step order is fixed — see
[`python/beava/_embed.py`](../../python/beava/_embed.py) for the
implementation. T-03-04-03 (security): the SDK only spawns paths from this
discovery order; no shell interpolation; no arbitrary-command execution.

## Lifecycle

- **Spawn** happens in `App.__init__` when no URL is given. The
  subprocess inherits the parent's stdin (closed) and gets piped
  stdout/stderr.
- **Ready signal** is the binary logging both `tcp_addr` and `admin_addr`
  to its stdout as JSON-shaped log lines. The SDK parses these and uses
  them as the transport endpoint. Default startup timeout: 5 seconds.
- **Teardown** runs on `app.close()` or `__exit__`. The SDK sends SIGTERM,
  waits up to 2 seconds for graceful exit, then SIGKILL.
- **Crash recovery** is not handled — embed mode is process-local. If the
  child crashes, subsequent SDK calls raise transport errors. Restart the
  Python process.

## When to use embed mode

- **Quickstart and `bv.demo()`** — `pip install "git+https://github.com/beava-dev/beava.git#subdirectory=python"` and have a working
  feature server in one line. No installer, no `docker run`, no port
  conflict.
- **Pytest fixtures.** `bv.test.fixture(reset_each=True)` spawns a fresh
  embed per test, gives each test an isolated server, tears down on
  teardown. Per-test isolation without the test author managing
  processes.
- **Notebooks and scripts.** Run an experimental pipeline against a real
  beava instance without provisioning anything. Restart the kernel and
  you get a fresh server.
- **Local development** for SDK / pipeline / aggregation work where you'd
  otherwise be running `cargo run` in another terminal.

## When NOT to use embed mode

- **Production.** Embed mode is process-local. State dies when the Python
  process exits. For production use a remote `beava` server and connect
  with `bv.App("tcp://host:port")` or `bv.App("http://host:port")`.
- **Multi-process workloads.** Each Python process spawns its own embed
  with its own state. No state sharing between workers. If you want
  multiple Python processes hitting the same beava state, run beava
  remotely.
- **Persistent state across restarts.** Embed mode resets on shutdown by
  default. For persistence pass `bv.App(persist_dir="/path/to/state")` —
  beava writes WAL + snapshots there and replays them on the next embed
  start. (Persist-dir support lands fully in Phase 13.4; v0 partial.)
- **High-throughput benchmarking.** Embed adds subprocess + transport
  setup cost on `App.__init__`. Use `crates/beava-bench` against a
  long-running server for accurate throughput numbers.

## Worked example

```python
import beava as bv

@bv.event
class Click:
    user_id: str
    ad_id: str

@bv.table(key="user_id")
def UserClickCount(click) -> bv.Table:
    return click.group_by("user_id").agg(clicks=bv.count())

with bv.App() as app:                         # spawn embed
    app.register(Click, UserClickCount)
    for i in range(10):
        app.push(Click, {"user_id": "u_1", "ad_id": f"ad_{i}"})
    print(app.get(UserClickCount, "u_1"))      # {"clicks": 10}
# app.close() runs here; subprocess is reaped
```

The whole flow — discover binary, spawn, register, push, query, tear down
— happens in <500 ms on a warm machine.

## Cross-references

- [`python/beava/_embed.py`](../../python/beava/_embed.py) — binary
  discovery + spawn / teardown implementation.
- [sdk-api/python.md](../sdk-api/python.md) — `App.__init__` signature,
  `App.close()`, `App.__enter__` / `__exit__`.
- [sdk-api/shared.md](../sdk-api/shared.md) — transport selection (URL
  scheme `tcp://` vs `http://` vs no-URL = embed).
- [error-codes.md](../error-codes.md) — `BinaryNotFoundError` error
  envelope.
- `docs/quickstart.md` (forthcoming, Plan 13.0-14) — `bv.demo()` flow
  that uses embed mode.
