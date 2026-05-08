# Quickstart

> One command, ~5 seconds, end-to-end. Then install however you want and
> open the Python file Beava just dropped on disk.

Beava is a real-time feature server: declare aggregations in Python,
push events over HTTP, query computed features in sub-millisecond.
Single binary. No Kafka.

This page walks you through your first feature in three steps.

## 1. `beava quickstart` — see it work in 5 seconds

Once you have the binary on `PATH`, one command runs an in-process
demo against an ephemeral port and prints a 4-step walkthrough.

```bash
$ beava quickstart
```

You'll see the four steps stream by — register a `PageView` event +
a global `SiteMetrics` table, push 5 events, query the row, and a
parting note pointing at `beava_quickstart.py` (a Python file Beava
just wrote next to your shell):

```
beava quickstart · v0.1.0
═══════════════════════════════════════════════════════════════

  Spinning up an in-process beava server on 127.0.0.1:54321…
  ✓ ready in 0.18s

[1/4] Define a feature
───────────────────────
  @bv.event
  class PageView:
      session_id: str
      path: str
      dwell_ms: int

  @bv.table # no key= → one row, site-wide
  def SiteMetrics(e: PageView):
      return e.agg(
          median_dwell_1h = bv.quantile("dwell_ms", q=0.5, window="1h"),
          page_views_today = bv.count(window="24h"),
          top_page_1h = bv.top_k("path", k=1, window="1h"),
      )

  POST /register → 201 (registry_version=1)

[2/4] Push 5 events
───────────────────
  POST /push {event:"PageView", data:{session_id:"s_1", …}} → ack_lsn=…
  …

[3/4] Query the global row
──────────────────────────
  POST /get {table:"SiteMetrics", key:""}
  → {
      "median_dwell_1h": 2110.0,
      "page_views_today": 5,
      "top_page_1h": [{"value": "/", "count": 2}]
    }

[4/4] Now run it for real
─────────────────────────
  Wrote ./beava_quickstart.py — same pipeline, talks to a real server.
  …
```

`--no-file` skips the drop-file step (handy in CI / `docker exec`).

## 2. Install however you want

If `beava quickstart` is the first time you've heard of the binary,
pick a path:

**Homebrew (macOS / Linuxbrew — recommended):**

```bash
brew tap beava-dev/beava
brew install beava
```

**pip (Python users):**

```bash
pip install beava
```

The pip wheel ships the SDK + a `beava` console-script shim that
locates the server binary on `PATH` and execs into it. The wheel
does **not** bundle the Rust binary itself — pair it with brew or
docker.

**Docker (zero deps on the host):**

```bash
docker run --rm -p 8080:8080 -p 8090:8090 beavadev/beava:edge
```

Boots Beava on built-in defaults: HTTP `:8080` (data plane), admin
sidecar `:8090` (`/health`, `/ready`, `/metrics`, `/registry`),
in-memory state with WAL + snapshot under `/data` inside the
container. The `:edge` tag rebuilds from `main` on every push.

**Verify any path:**

```bash
curl http://localhost:8090/health
# {"status":"ok"}
```

## 3. Edit `beava_quickstart.py` and run it for real

Step 1 dropped a Python file in your shell's working directory:

```python
# beava_quickstart.py — same pipeline as `beava quickstart`.
# Run a real server in another terminal (`beava`) and run this file:
# $ python beava_quickstart.py

import beava as bv


@bv.event
class PageView:
    session_id: str
    path: str
    dwell_ms: int


@bv.table # no key= → one row, site-wide
def SiteMetrics(e: PageView):
    return e.agg(
        median_dwell_1h = bv.quantile("dwell_ms", q=0.5, window="1h"),
        page_views_today = bv.count(window="24h"),
        top_page_1h = bv.top_k("path", k=1, window="1h"),
    )


app = bv.App("127.0.0.1:8080")
app.register(PageView, SiteMetrics)

for sid, path, dwell in [
    ("s_1", "/", 1240),
    ("s_2", "/pricing", 3380),
    ("s_3", "/docs", 890),
    ("s_4", "/", 2110),
    ("s_5", "/docs", 5620),
]:
    app.push("PageView", {"session_id": sid, "path": path, "dwell_ms": dwell})

print(app.get("SiteMetrics"))
```

To run it against a real server:

```bash
# Terminal 1 — start the server
$ beava

# Terminal 2 — install the SDK if you haven't, then run
$ pip install beava
$ python beava_quickstart.py
{'median_dwell_1h': 2110.0, 'page_views_today': 5, 'top_page_1h': [{'value': '/', 'count': 2}]}
```

You're now editing `beava_quickstart.py` against a Beava server.
Beava re-runs the pipeline on every push you send. Re-running the
file is idempotent — `app.register` accepts the same descriptors
without churn. Calling `bv.App` with `force=True` lets you change
the schema between runs.

If you'd rather not run a separate server at all — tests, notebooks,
quick prototypes — `bv.App()` with no URL spawns a local Beava on
ephemeral ports automatically:

```python
with bv.App() as app:
    app.register(PageView, SiteMetrics)
    app.push("PageView", {"session_id": "s_1", "path": "/", "dwell_ms": 1240})
    print(app.get("SiteMetrics"))
```

Same wire protocol; everything you build in embed mode runs
unchanged against a real server. See
[concepts/embed-mode](./concepts/embed-mode.md).

## With `curl` alone

If you don't want to touch Python:

```bash
# Register
curl -X POST http://localhost:8080/register \
  -H 'content-type: application/json' \
  -d @schema.json

# Push
curl -X POST http://localhost:8080/push \
  -H 'content-type: application/json' \
  -d '{"event":"PageView","data":{"session_id":"s_1","path":"/","dwell_ms":1240}}'

# Get the global row
curl -X POST http://localhost:8080/get \
  -H 'content-type: application/json' \
  -d '{"table":"SiteMetrics","key":""}'
```

The full HTTP surface is documented at [docs/http-api.md](./http-api.md).

## Per-entity vs global

`SiteMetrics` above is **global** — `@bv.table` with no `key=` gives
you one row, site-wide. Pass `key=` to get per-entity aggregation:

```python
@bv.table(key="campaign_id")
def CampaignStats(imp: Impression):
    return imp.group_by("campaign_id").agg(
        impressions_1h=bv.count(window="1h"),
        bid_sum_1h=bv.sum("bid", window="1h"),
        bid_mean_1h=bv.mean("bid", window="1h"),
    )

print(app.get("CampaignStats", "c1")) # -> {"impressions_1h": 2, ...}
```

Per [ADR-003](../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md),
all 54 operators work with both shapes. See
[concepts/global-aggregation](./concepts/global-aggregation.md) for
when to pick which.

## `bv.demo()` — the longer tour

A self-contained tour with realistic-shape data:

```python
import beava as bv

bv.demo("adtech") # ad-impression / click-rate aggregations
bv.demo("fraud") # high-cardinality velocity + sketch
bv.demo("ecommerce") # purchase / basket aggregations
```

Each demo registers descriptors, pushes ~10 events, and queries the
resulting features. Source:
[examples/python/adtech.py](../examples/python/adtech.py),
[examples/python/fraud.py](../examples/python/fraud.py),
[examples/python/ecommerce.py](../examples/python/ecommerce.py).

> **Cross-language note:** Pipeline authoring is **Python-only** in v0.
> The [TypeScript](./sdk-api/typescript.md) and [Go](./sdk-api/go.md)
> SDKs push events, register pre-compiled JSON descriptors (authored
> from Python), and read features. Use Python to design the pipeline;
> TS/Go services push events + read features against the same
> registered pipeline.

## Next steps

- **API reference:** [docs/sdk-api/python.md](./sdk-api/python.md) —
  full Python SDK surface (App, decorators, expressions, op helpers)
- **Operator catalog:** [docs/operators/index.md](./operators/index.md)
  — all 54 op pages (`count`, `sum`, `mean`, `n_unique`, `quantile`,
  `ewma`, …)
- **Wire contract:** [docs/wire-spec.md](./wire-spec.md) — frame
  format + JSON Schema 2020-12 contracts (for porting to other
  languages)
- **Pipeline DSL:** [docs/pipeline-dsl/overview.md](./pipeline-dsl/overview.md)
  — `@bv.event`, `@bv.table`, chain methods, expressions
- **Architecture:** [docs/architecture/](./architecture/) —
  single-thread apply + mio data plane + WAL/snapshot durability +
  memory budget
