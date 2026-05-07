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

Pre-release: install everything from main. PyPI / crates.io / Homebrew tap publish on the v0.0.0 cut.

```bash
# Server (Docker — published from main on every push)
docker run -p 8080:8080 -p 8081:8081 beavadev/beava:edge

# Or build from source
cargo install --git https://github.com/beava-dev/beava beava-server
beava --data-dir ./.beava/

# Python SDK (from main — PyPI 'beava' reserved for v0.0.0 GA)
pip install "git+https://github.com/beava-dev/beava.git#subdirectory=python"
```

```python
import beava as bv

@bv.event
class Click:
    user_id: str
    page: str

@bv.table(key="user_id")
def UserActivity(e: Click) -> bv.Table:
    return e.group_by("user_id").agg(
        clicks_1h=bv.count(window="1h"),
        unique_pages_1h=bv.count_distinct("page", window="1h"),
    )

app = bv.App(url="http://localhost:8080")
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

beava binds two listeners:

- **HTTP/JSON on `127.0.0.1:8080`** — curl-compatible debugging path.
- **Framed TCP on `127.0.0.1:8081`** — sub-millisecond fast-path. JSON or msgpack content.

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
beava [OPTIONS]

  --http-addr <ADDR>            default: 127.0.0.1:8080
  --tcp-addr <ADDR>             default: 127.0.0.1:8081
  --data-dir <PATH>             default: ./.beava/
  --memory-only                 ephemeral; no WAL/snapshot
  --test-mode                   enable POST /reset and OP_RESET
  --wal-flush-ms <MS>           default: 100
  --snapshot-interval-mins <M>  default: 30
  -h, --help
  -V, --version

env vars
  BEAVA_LOG=debug|info|warn     default: info
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
