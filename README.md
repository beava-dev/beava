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
  <a href="https://discord.gg/J5trwbCYpS"><img src="https://img.shields.io/badge/chat-discord-7ca84a" alt="discord"/></a>
  <a href="https://github.com/beava-dev/beava/releases/latest"><img src="https://img.shields.io/github/v/release/beava-dev/beava?include_prereleases&label=release&color=d97757" alt="release"/></a>
</p>

<p align="center">
  <a href="https://render.com/deploy?repo=https://github.com/beava-dev/beava"><img src="https://img.shields.io/badge/deploy-Render-46e3b7?logo=render&logoColor=white" alt="Deploy to Render"/></a>
  <a href="https://railway.com/deploy/beava?referralCode=xkfMVJ&amp;utm_medium=integration&amp;utm_source=template&amp;utm_campaign=generic"><img src="https://img.shields.io/badge/deploy-Railway-0b0d0e?logo=railway&logoColor=white" alt="Deploy on Railway"/></a>
</p>

---

**Give your product live reflexes.**

beava turns live events into fresh decision features, so your app can pause runaway agents, reorder marketplaces, and rescue stuck users — no Kafka, no Flink, no feature store.

Push events over HTTP or TCP. The very next read reflects them. No batch lag, no broker, no stream worker in between.

```python
# agent_safety.py — live reflexes for an agent session.

import beava as bv

@bv.event
class AgentStep:
    session_id: str
    agent_id: str
    action: str       # "search" | "browse" | "tool_call" | "model_call"
    tool: str         # "browser" | "shell_exec" | "http_get" | "code_run"
    ok: bool
    risky: bool
    tokens: int
    latency_ms: int

@bv.table(key="session_id")
def SessionReflexes(e: AgentStep):
    return e.group_by("session_id").agg(
        failure_rate_5m  = bv.ratio(window="5m", where=~bv.col("ok")),
        top_tool_10m     = bv.top_k("tool", k=1, window="10m"),
        unique_tools_10m = bv.n_unique("tool", window="10m"),
        token_burn_1m    = bv.sum("tokens", window="1m"),
        p95_latency_5m   = bv.quantile("latency_ms", q=0.95, window="5m"),
        risky_streak     = bv.streak(where=bv.col("risky")),
        last_action      = bv.last("action"),
    )

app = bv.App("http://localhost:8080").register(AgentStep, SessionReflexes)

app.push("AgentStep", {
    "session_id": "session_44",
    "agent_id":   "agent_91",
    "action":     "tool_call",
    "tool":       "browser",
    "ok":         False,
    "risky":      True,
    "tokens":     4200,
    "latency_ms": 830,
})

features = app.get("SessionReflexes", "session_44")

if features["failure_rate_5m"] > 0.8:
    lock_tool_access()
if features["risky_streak"] >= 2:
    require_human_approval()
if features["token_burn_1m"] > 100_000:
    switch_to_cheaper_model()
```

That is the reflex loop: **event in → feature recomputed → decision served.** `app.push` writes the event straight to beava. The next `app.get` reflects it. Your product can act before the next request, next tool call, or next screen.

## Why beava

Most products already have events. The hard part is turning those events into fresh decision state.

Without beava, this usually becomes a pile of Redis counters, cron jobs, queue workers, Postgres triggers, stream processors, and drift-prone glue code.

beava gives you one declarative feature layer:

- define events in Python
- declare per-entity feature tables
- push live events
- read fresh features by key
- make the product act immediately

No Kafka. No Flink. No feature store. One Rust binary.

## Three pipelines. Six live signals.

| Pipeline | Live signal | Product reflex |
|---|---|---|
| **Agent runtime control** | `session_44.failure_rate_5m = 83%` | lock tool access |
| **Agent runtime control** | `session_44.risky_streak = 2` | require human approval |
| **Marketplace reranking** | `sku_882.cart_velocity_5m = 91` | boost trending item |
| **Marketplace reranking** | `user_1382.avg_view_price_30m = $211` | sort toward premium picks |
| **SaaS growth rescue** | `user_1271.top_error_topic_10m = "auth"` | launch setup rescue |
| **SaaS growth rescue** | `org_acme.limit_hits_24h = 12` | show team upgrade path |

These are not dashboards. They are decision features your app can read while the user, agent, or shopper is still active.

## What you can build

**Agent runtime control.** Catch agent loops before the next tool call. Track repeated actions, risky tools, failure rates, token burn, latency spikes, and approval triggers per session or agent. Pause loops, lock risky tools, require human approval, switch to a cheaper model, route around a slow provider.

**Marketplace reranking.** Reorder the marketplace while shoppers are still shopping. Track live price intent, cart velocity, product momentum, category spikes, and recommendation fatigue. Boost fast-moving SKUs, sort toward premium picks, show affordable alternatives, diversify stale recommendations, promote matching inventory.

**SaaS growth rescue.** Rescue stuck users before the session ends. Track error loops, docs spirals, setup attempts, usage limits, invite momentum, and expansion signals. Launch setup rescue, open guided onboarding, escalate to support, show an upgrade path, route expansion-ready accounts to sales.

## 60-second quickstart

Pick the install path that matches your environment. All three deliver the same `beava` binary.

```bash
# pip — installs the Python SDK and bundled Rust server binary
pip install beava

# brew — macOS and Linuxbrew
brew install beava-dev/beava/beava

# docker — zero host dependencies
docker run -p 8080:8080 -p 8081:8081 beavadev/beava:latest
```

Start the server:

```bash
beava --data-dir ./.beava/
```

Or run the in-process demo:

```bash
beava quickstart
```

Full walkthrough: [beava.dev/docs](https://beava.dev/docs).

## The primitives

beava has three core primitives:

```python
@bv.event
class ProductEvent:
    ...

@bv.table(key="user_id")
def UserReflexes(e: ProductEvent):
    return e.group_by("user_id").agg(...)

app.get("UserReflexes", "user_123")
```

The Python SDK includes operators for counters, windows, ratios, top-k, recency, sketches, decay, velocity, buffers, and geo signals:

```python
bv.count(window="10m")
bv.ratio(window="5m", where=...)
bv.top_k("tool", k=3, window="10m")
bv.n_unique("sku", window="30m")
bv.mean("price", window="30m")
bv.quantile("latency_ms", q=0.95, window="5m")
bv.streak(where=...)
bv.time_since(where=...)
bv.decayed_sum("tokens", half_life="10m")
```

## Performance and durability

beava is built for hot-path feature reads.

- Push and read are inline: the read after a push reflects that push.
- HTTP/JSON is available for debugging and integration.
- Framed TCP is available for the sub-millisecond hot path.
- WAL on every push plus periodic snapshots.
- Recovery rebuilds state from disk on boot.
- In-memory state only; size your box for your entity count and feature pack.

## When not to use beava

beava is intentionally small and direct. It is not a replacement for every streaming system. Do not use beava if:

- you need strict event-time semantics with watermarks
- you need cross-process sharding inside a single logical cluster
- your product can tolerate 5–30 seconds of feature staleness
- you want a managed service today
- you need long-term analytical storage or SQL exploration

Use beava when the product needs to act now.

## Wire surface

beava binds three listeners:

- **HTTP/JSON on `127.0.0.1:8080`** — curl-compatible debugging path.
- **Framed TCP on `127.0.0.1:8081`** — sub-millisecond fast-path. JSON or msgpack content.
- **Admin sidecar on `127.0.0.1:8090`** — `/health`, `/ready`, `/metrics`, `/registry`.

```bash
curl -X POST localhost:8080/register -d '{...schema...}'

curl -X POST localhost:8080/push -d '{
  "event": "AgentStep",
  "data": {
    "session_id": "session_44",
    "agent_id":   "agent_91",
    "action":     "tool_call",
    "tool":       "browser",
    "ok":         false,
    "risky":      true,
    "tokens":     4200,
    "latency_ms": 830
  }
}'

curl -X POST localhost:8080/get -d '{
  "table": "SessionReflexes",
  "key":   "session_44"
}'
```

## Learn more

- [beava.dev](https://beava.dev) — site, docs, guides, and roadmap
- [examples/](examples/) — vertical examples in Python
- [crates/beava-bench/README.md](crates/beava-bench/README.md) — benchmark harness

## Community and open source

The open-source project is the real system: something you can clone, run, test, operate, and trust as your use case grows. A managed beava service may remove operational burden later, but the open-source binary is the core product. Apache-2.0. No open-core lock-in.

- **Discussions:** [github.com/beava-dev/beava/discussions](https://github.com/beava-dev/beava/discussions)
- **Discord:** [discord.gg/J5trwbCYpS](https://discord.gg/J5trwbCYpS)
- **Security:** private disclosure to `hoang@beava.dev`

[Apache 2.0](LICENSE) · [CHANGELOG](CHANGELOG.md) · [SECURITY](SECURITY.md) · [CONTRIBUTING](CONTRIBUTING.md) · [CODE_OF_CONDUCT](CODE_OF_CONDUCT.md)
