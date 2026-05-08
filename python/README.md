# beava — Python SDK

Python SDK for [Beava](https://github.com/beava-dev/beava), the single-binary real-time feature server. Author pipelines with `@bv.event` / `@bv.table` decorators; push events and read features against a running `beava` server over HTTP or TCP.

## Install

```bash
pip install beava
```

The wheel ships the SDK **and** the Rust `beava` server binary (v0.4.0+, polars-style). After install, the `beava` shell command is on `PATH` and the SDK can run against it directly — including embed mode (`bv.App()` with no URL).

If you'd rather run the server in a container, Docker is the alternative:

```bash
docker run -p 8080:8080 -p 8081:8081 beavadev/beava:edge
```

## Quickstart

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

app = bv.App(url="http://localhost:8080")
app.register(Click, UserActivity)

app.push("Click", {"user_id": "alice", "page": "/home"})
app.push("Click", {"user_id": "alice", "page": "/products"})

app.get("UserActivity", "alice")
# => {"clicks_1h": 2, "unique_pages_1h": 2}
```

## Transports

The same SDK speaks three transports — pick by the URL you pass to `bv.App(...)`.

```python
# HTTP/JSON (curl-compatible debugging path)
app = bv.App(url="http://localhost:8080")

# Framed TCP (sub-millisecond fast-path; JSON or msgpack content)
app = bv.App(url="tcp://localhost:8081")

# Embed mode (no separate server — auto-spawns a local `beava` binary)
app = bv.App()
```

Embed mode finds the `beava` binary that ships with `pip install beava` automatically. Override with `BEAVA_BINARY=/path/to/beava` if you want to point at a different build.

## Surface

| Method | Wire | Notes |
|--------|------|-------|
| `app.register(*descriptors, force=False, dry_run=False)` | `POST /register` | Returns `{"registry_version": <n>}`. Pass `force=True` for destructive schema changes; `dry_run=True` returns a categorized diff without applying. |
| `app.push(event_name, fields)` | `POST /push` body `{event, data}` | Fire-and-forget. Returns server ack. |
| `app.get(table, key=None, features=None)` | `POST /get` body `{table, key}` | Returns a flat dict of feature values. `key=None` routes to the global-aggregation sentinel (ADR-003). `features=[...]` narrows to a subset. |
| `app.batch_get(requests)` | `POST /batch_get` body `{requests: [...]}` | `requests` is a list of `(table, key)` or `(table, key, features)` tuples. Returns a list of dicts in input order. |
| `app.reset()` | `POST /reset` (test_mode-only) | Wipes server state. Pass `test_mode=True` to `bv.App(...)` to enable. |
| `app.ping()` | `POST /ping` | Liveness; returns `{"pong": True, "registry_version": <n>}`. |
| `app.close()` | — | Releases the transport connection. |

## Pipeline DSL

`@bv.event` declares an event schema. Fields are typed Python class attributes; the SDK serializes them to JSON on push.

`@bv.table(key=...)` declares a feature table. The decorated function takes one event-typed parameter and returns `e.group_by(...).agg(...)` over it. The runtime maintains the table incrementally — every event updates exactly the affected key's row.

The `key=` kwarg accepts a string (single-column key) or a list of strings (composite key).

`bv.col("name")` references an event field inside a `where=` filter or arithmetic expression. Strings in `where=` are rejected — pass an `_Expr` (e.g. `bv.col("event_type") == "click"`).

`bv.lit(value)` wraps a Python literal so it can participate in the same expression grammar.

## Operator catalogue

50+ aggregation primitives are exported flat at the `bv.*` namespace. Inspect the live surface from Python:

```python
import beava as bv
print([x for x in dir(bv) if not x.startswith("_")])
```

Selection by family:

- **Core** — `count`, `sum`, `mean`, `min`, `max`, `var`, `std`, `n_unique`, `quantile`
- **Sketch** — `top_k`, `bloom_member`, `entropy`, `histogram`, `hour_of_day_histogram`, `dow_hour_histogram`
- **Recency** — `first`, `last`, `first_n`, `last_n`, `lag`, `first_seen`, `last_seen`, `age`, `time_since`
- **Decay** — `ewma`, `ema`, `ewvar`, `ew_zscore`, `decayed_sum`, `decayed_count`
- **Velocity** — `rate_of_change`, `inter_arrival_stats`, `burst_count`, `delta_from_prev`, `trend`, `z_score`
- **Buffer / geo** — `most_recent_n`, `reservoir_sample`, `geo_velocity`, `geo_distance`, `geo_spread`, `distance_from_home`

Deprecation aliases retained: `avg → mean`, `variance → var`, `stddev → std`, `count_distinct → n_unique`, `percentile → quantile`. Use the canonical form in new code.

## Demo data

`bv.demo` exposes three pre-built pipelines + dataset loaders for trying the SDK without writing your own pipeline:

```python
import beava as bv
adtech = bv.demo.adtech()      # ad-impression / click / conversion
fraud = bv.demo.fraud()        # transaction fraud
ecommerce = bv.demo.ecommerce()  # cart / view / purchase
```

Each returns a tuple of `(events, tables, dataset)` ready to register and push.

## Errors

Top-level error types: `bv.RegistrationError` (raised on register failures with structured `code` + `message`), `bv.ValidationError` (raised by client-side input validators), `bv.BinaryNotFoundError` (raised in embed mode if the `beava` binary isn't on `PATH`).

## Learn more

- [beava.dev](https://beava.dev) — site, guide, docs
- [Root README](https://github.com/beava-dev/beava#readme) — server install, wire surface, server CLI
- [Discord](https://discord.gg/Jnx89PN9) — questions and feedback
