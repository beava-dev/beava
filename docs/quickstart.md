# beava Quickstart

> Run the server. Install the SDK. Push your first event and query a feature — in five minutes.

beava is a real-time feature server. You declare aggregations, push events
over HTTP, and query computed features by entity key — sub-millisecond —
with `curl` alone or any HTTP client.

![beava quickstart](./_assets/quickstart-demo.svg)

## 1. Install beava

```bash
pip install beava
```

As of v0.4.0, `pip install beava` ships **both** the Python SDK and the Rust
server binary in one wheel (~14 MB). The `beava` shell command lands on PATH
automatically; no separate Cargo or Docker install required for SDK users.

## 2. Run the server

```bash
beava
```

That's it — beava boots on built-in defaults: HTTP `:8080` (data plane), admin
sidecar `:8090` (`/health`, `/ready`, `/metrics`, `/registry`), in-memory state
with WAL + snapshot under `/data`. No config file required (a missing
`./beava.yaml` falls through to built-in defaults + `BEAVA_*` env-var
overrides). Drop a `beava.yaml` next to the binary later if you want to
pin durability paths or change ports.

**Docker alternative:**

```bash
docker run --rm -p 8080:8080 -p 8090:8090 beavadev/beava:edge
```

The `:edge` tag is rebuilt from `main` on every push. Use Docker if you want to
run the server without installing Python, or to isolate the server in a
container. Same ports, same defaults.

**Verify either path:**

```bash
curl http://localhost:8090/health
# {"status":"ok"}
```

## 3. Declare a feature

```python
import beava as bv

@bv.event
class Impression:
    campaign_id: str
    bid: float

@bv.table(key="campaign_id")
def CampaignStats(imp: Impression):
    return imp.group_by("campaign_id").agg(
        impressions_1h=bv.count(window="1h"),
        bid_sum_1h=bv.sum("bid", window="1h"),
        bid_mean_1h=bv.mean("bid", window="1h"),
    )
```

## 4. Push events and query

Point the SDK at the server you started in step 2:

```python
with bv.App("http://localhost:8080") as app:
    app.register(Impression, CampaignStats)

    for camp_id, bid in [("c1", 0.50), ("c1", 0.75), ("c2", 0.40)]:
        app.push("Impression", {"campaign_id": camp_id, "bid": bid})

    print(app.get("CampaignStats", "c1"))
    # -> {"impressions_1h": 2, "bid_sum_1h": 1.25, "bid_mean_1h": 0.625}
```

Or with `curl`:

```bash
curl -X POST http://localhost:8080/push \
  -H 'content-type: application/json' \
  -d '{"event":"Impression","body":{"campaign_id":"c1","bid":0.50}}'

curl 'http://localhost:8080/get?table=CampaignStats&key=c1'
# {"impressions_1h":2,"bid_sum_1h":1.25,"bid_mean_1h":0.625}
```

## Embed mode (no separate server)

If you'd rather not run a server at all — for tests, notebooks, or a quick
sanity check — `bv.App()` with no URL spawns a local beava on ephemeral
ports automatically:

```python
with bv.App() as app:
    app.register(Impression, CampaignStats)
    app.push("Impression", {"campaign_id": "c1", "bid": 0.5})
    print(app.get("CampaignStats", "c1"))
```

Same wire protocol; everything you build in embed mode runs unchanged
against the real server in step 2. See
[concepts/embed-mode](./concepts/embed-mode.md).

## Global aggregation

Need a feature that spans every entity — total throughput, top-K globally?
Drop the `key=` kwarg:

```python
@bv.table
def TotalImpressions(imp: Impression):
    return imp.agg(total=bv.count(window="forever"))

# Per-entity query (2 args):
print(app.get("CampaignStats", "c1"))     # -> {"impressions_1h": 2, ...}
# Global query (1 arg):
print(app.get("TotalImpressions"))        # -> {"total": 3}
```

Per [ADR-003](../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md),
all 54 operators work with both per-entity and global aggregation. See
[concepts/global-aggregation](./concepts/global-aggregation.md) for when to
pick which.

## bv.demo()

A self-contained tour with realistic-shape data:

```python
import beava as bv

bv.demo("adtech")     # ad-impression / click-rate aggregations
bv.demo("fraud")      # high-cardinality velocity + sketch
bv.demo("ecommerce")  # purchase / basket aggregations
```

Each demo registers descriptors, pushes ~10 events, and queries the
resulting features. Source:
[examples/python/adtech.py](../examples/python/adtech.py),
[examples/python/fraud.py](../examples/python/fraud.py),
[examples/python/ecommerce.py](../examples/python/ecommerce.py).

> **Cross-language note:** Pipeline authoring is **Python-only** in v0. The
> [TypeScript](./sdk-api/typescript.md) and [Go](./sdk-api/go.md) SDKs push
> events, register pre-compiled JSON descriptors (authored from Python),
> and read features. Use Python to design the pipeline; TS/Go services
> push events + read features against the same registered pipeline.

## Next steps

- **API reference:** [docs/sdk-api/python.md](./sdk-api/python.md) — full
  Python SDK surface (App, decorators, expressions, op helpers)
- **Operator catalog:** [docs/operators/index.md](./operators/index.md) —
  all 54 op pages (`count`, `sum`, `mean`, `n_unique`, `quantile`, `ewma`,
  …)
- **Wire contract:** [docs/wire-spec.md](./wire-spec.md) — frame format +
  JSON Schema 2020-12 contracts (for porting to other languages)
- **Pipeline DSL:** [docs/pipeline-dsl/overview.md](./pipeline-dsl/overview.md) —
  `@bv.event`, `@bv.table`, chain methods, expressions
- **Architecture:** [docs/architecture/](./architecture/) — single-thread
  apply + mio data plane + WAL/snapshot durability + memory budget
