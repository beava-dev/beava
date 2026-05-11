<p align="center">
  <a href="https://beava.dev">
    <img src="beava-design-system/project/assets/readme-banner.png" alt="beava" width="100%"/>
  </a>
</p>

<p align="center">
  <a href="https://github.com/beava-dev/beava/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/beava-dev/beava/ci.yml?branch=main&label=build" alt="build"/></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-7ca84a" alt="license Apache-2.0"/></a>
  <a href="https://beava.dev"><img src="https://img.shields.io/badge/site-beava.dev-3a6a8a" alt="site beava.dev"/></a>
  <a href="https://beava.dev/docs"><img src="https://img.shields.io/badge/docs-beava.dev%2Fdocs-3a6a8a" alt="docs"/></a>
  <a href="https://discord.gg/Jnx89PN9"><img src="https://img.shields.io/badge/chat-discord-7ca84a" alt="discord"/></a>
  <a href="https://github.com/beava-dev/beava/releases/latest"><img src="https://img.shields.io/github/v/release/beava-dev/beava?include_prereleases&label=release&color=d97757" alt="release"/></a>
</p>

---

A real-time feature server. Push events over HTTP or TCP, declare features in Python, query them at sub-millisecond latency.

beava is a single-binary feature server for fraud detection, ad-tech, and behavioral analytics. Push events in over HTTP or TCP; beava tracks per-entity features (counters, velocities, distances, rates, distributions) updated atomically on every event; your application queries them at sub-millisecond latency to power live scoring rules.

Think **Redis for stateful streaming features**, with 50+ purpose-built aggregation primitives instead of do-it-yourself Lua scripts.

## 60-second quickstart

Pick whichever install path matches your box. All three deliver the same `beava` binary.

```bash
# curl  — fetches the platform wheel from the latest GitHub Release
#         (~14 MB, ships SDK + Rust server binary together;
#          polars / ruff / uv pattern). `beava` lands on PATH.
#          Pin a version with BEAVA_VERSION=v0.0.0.
curl -fsSL https://raw.githubusercontent.com/beava-dev/beava/main/scripts/install.sh \
  | sh

# docker — zero deps on the host
docker run -p 8080:8080 -p 8081:8081 beavadev/beava:edge

# cargo  — from source, for Rust-toolchain users
cargo install --git https://github.com/beava-dev/beava beava-server
```

Then start the server:

```bash
beava --data-dir ./.beava/
```

Or kick the tyres without writing anything to disk:

```bash
beava quickstart   # 4-step in-process demo, ~10s, drops a beava_quickstart.py file
beava --memory-only   # ephemeral server, no WAL, no recovery
```

```python
import beava as bv

@bv.event
class Click:
    user_id: str
    page: str

@bv.table(key="user_id")
def UserActivity(e: Click):
    return e.group_by("user_id").agg(
        clicks_1h=bv.count(window="1h"),
        unique_pages_1h=bv.n_unique("page", window="1h"),
    )

app = bv.App(url="http://localhost:8080")    # or bv.App() to spawn an embed-mode server
app.register(Click, UserActivity)

app.push("Click", {"user_id": "alice", "page": "/home"})
app.push("Click", {"user_id": "alice", "page": "/products"})

app.get("UserActivity", "alice")
# => {"clicks_1h": 2, "unique_pages_1h": 2}
```

That's it. **No broker, no ETL, no schema registry, no separate stream / batch path.** One binary, one Python decorator, real-time features.

Full walkthrough: [beava.dev/docs](https://beava.dev/docs).

## Why beava

Replaces Postgres triggers + Redis counters + the cron job that heals drift. Same pipeline from laptop to production.

**Performance:** 684,812 sustained events/sec on a single Apple-M4 core[^1] — simple-fraud pipeline, TCP transport, msgpack wire, parallel=16, 60s sustained run. Run multiple beava instances for higher throughput (Redis-cluster style; no in-process sharding).

**Memory:** ~7 KB per entity for a rich 30-feature pack → ~700 GB for 100M entities. Size your box; in-memory only — no SSD overflow.

**Durability:** WAL on every push + periodic snapshot. Boot recovers state in seconds. Refuse-on-network-FS so you don't accidentally fsync over NFS.

[^1]: Reproduce: `cargo run -p beava-bench --release -- throughput --pipeline small --transport tcp --wire-format msgpack --parallel 16 --duration-secs 60 --pipeline-depth 1024`. Numbers vary by hardware; dedicated x86 server-class boxes typically clear 1M+ EPS sustained. See [crates/beava-bench/README.md](crates/beava-bench/README.md) for the harness.

## Wire surface

beava binds three listeners:

- **HTTP/JSON on `127.0.0.1:8080`** - curl-compatible debugging path.
- **Framed TCP on `127.0.0.1:8081`** - sub-millisecond fast-path. JSON or msgpack content.
- **Admin sidecar on `127.0.0.1:8090`** - observability endpoints for `/health`, `/ready`, `/metrics`, and `/registry`. Override with `BEAVA_ADMIN_ADDR`.

### HTTP

```bash
curl -X POST localhost:8080/register -d '{...schema...}'
curl -X POST localhost:8080/push     -d '{"event":"Click","data":{"user_id":"alice","page":"/home"}}'
curl -X POST localhost:8080/get      -d '{"table":"UserActivity","key":"alice"}'
curl -X POST localhost:8080/batch_get -d '{"requests":[{"table":"UserActivity","key":"alice"}]}'
curl -X POST localhost:8080/ping
```

### TCP frame

```text
[u32 length BE][u16 op BE][u8 content_type][payload: length - 3 bytes]
```

`length` counts the bytes after itself. Multi-byte integers are big-endian. **Strict FIFO per connection** (Redis RESP style) — frame order correlates requests to responses; no `request_id` field.

| Opcode | Name | Body |
|--------|------|------|
| `0x0010` | `push` | `{event, data}` |
| `0x0020` | `get` | `{table, key}` |
| `0x0024` | `batch_get` | `{requests: [...]}` |
| `0x0030` | `register` | full schema |
| `0x0040` | `reset` | `{}` (test_mode-only) |
| `0xFFFF` | `error_response` | `{error: {code, message}}` |

| Content-type | Format |
|--------------|--------|
| `0x01` | JSON |
| `0x02` | msgpack |

Unknown opcodes return `error_response` with code `unknown_op` and the connection stays open.

## Server CLI

```text
beava [OPTIONS] [SUBCOMMAND]

  -c, --config <CONFIG>     YAML config file (full surface; optional)
      --http-addr <ADDR>    default: 127.0.0.1:8080
      --tcp-addr <ADDR>     default: 127.0.0.1:8081
      --data-dir <PATH>     default: ./.beava/  (WAL → <DIR>/wal,
                                                 snapshots → <DIR>/snapshots)
      --memory-only         ephemeral; no WAL/snapshot
      --test-mode           enable POST /reset and OP_RESET
  -h, --help
  -V, --version

subcommands
  quickstart [--no-file]    in-process 4-step first-touch demo

env vars
  BEAVA_LOG_LEVEL=debug|info|warn     default: info
  BEAVA_TEST_MODE=1                   alias for --test-mode
  BEAVA_ADMIN_ADDR                    admin sidecar address; default: 127.0.0.1:8090
  BEAVA_WAL_DIR / BEAVA_SNAPSHOT_DIR  per-dir overrides (use --data-dir
                                      for a single-root convenience flag)
  BEAVA_LISTEN_ADDR                   alias for --http-addr
  BEAVA_TCP_HOST / BEAVA_TCP_PORT     per-listener overrides
                                      (use --tcp-addr for the canonical form)

WAL fsync interval and snapshot interval ride along inside YAML config;
promotion to first-class CLI flags (`--wal-flush-ms`, `--snapshot-interval-mins`)
is a v0.0.x followup. Most operators don't tune these.
```

No TLS in v0 — terminate at nginx, Envoy, or Cloudflare if you need it. No auth in v0 — bind to a private network.

## Learn more

- [beava.dev](https://beava.dev) — site, docs, guides, RFCs, dev calls
- [examples/](examples/) — vertical demos in Python
- [crates/beava-bench/README.md](crates/beava-bench/README.md) — benchmark harness, reproduce the numbers

## Community & open-source commitment

The open-source project is the real system — something you can clone, run, test, operate, and trust as your use case grows. A managed beava service can remove operational burden later, but the open-source binary is the real product. TiDB-style commitment to open source. Apache-2.0, no open-core lock-in.

- **Discussions:** [github.com/beava-dev/beava/discussions](https://github.com/beava-dev/beava/discussions)
- **Discord:** [discord.gg/Jnx89PN9](https://discord.gg/Jnx89PN9)
- **Security:** private disclosure to `hoang@beava.dev` (see [SECURITY.md](SECURITY.md))

[Apache 2.0](LICENSE) · [CHANGELOG](CHANGELOG.md) · [SECURITY](SECURITY.md) · [CONTRIBUTING](CONTRIBUTING.md) · [GOVERNANCE](GOVERNANCE.md) · [MAINTAINERS](MAINTAINERS.md) · [CODE_OF_CONDUCT](CODE_OF_CONDUCT.md)
