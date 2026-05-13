# Deploy and Host Beava on Railway

Beava is a real-time feature server. Push events over HTTP or TCP and the next read reflects them. Define windowed aggregates in ~15 lines of Python (`@bv.event`, `@bv.table`), then query per-entity state via HTTP. Built for fraud detection, recommendations, LLM guardrails, and in-product analytics. Single Rust binary — no Kafka, no Flink, no feature store.

## About Hosting Beava

Beava runs as a single Rust binary with an in-memory state engine backed by a write-ahead log and periodic snapshots on disk. There is no external broker, stream worker, or feature-store dependency — events go straight to Beava and reads reflect the latest state immediately. This template provisions Beava from the official `beavadev/beava:latest` image with a 1 GB persistent volume mounted at `/data` for WAL + snapshots. Port 8080 is exposed publicly for the HTTP data plane (`/push`, `/get`, `/register`, `/health`). The lower-latency framed TCP fast-path on port 8081 is opt-in: flip one env var and add a Railway TCP Proxy on that port to enable it.

## Common Use Cases

- **Fraud detection** — windowed counters (failed logins, transaction velocity, distinct devices per user) keyed by `user_id`, queried at decision time to block the 5th try.
- **Recommendation features** — recent clicks, category affinities, view recency per user, updated as the user browses and read back to refresh the feed.
- **LLM guardrails and agent control** — token-usage budgets, requests-per-minute, and per-org spend keyed by `org_id`, queried before each model call to throttle or deny.

## Dependencies for Beava Hosting

- A 1 GB persistent volume mounted at `/data` (holds WAL and snapshot files for durability across redeploys).
- Public HTTP port `8080` for the data plane.
- *Optional:* a Railway TCP Proxy on port `8081` if your application uses the Python SDK's TCP fast-path transport.

### Deployment Dependencies

- Docker image: [beavadev/beava on Docker Hub](https://hub.docker.com/r/beavadev/beava)
- Source code: [beava-dev/beava on GitHub](https://github.com/beava-dev/beava)
- Python SDK: [beava on PyPI](https://pypi.org/project/beava/)
- Documentation: [beava.dev/docs](https://beava.dev/docs) — see [Deploy](https://beava.dev/docs/get-started/deploy/) for the full setup walkthrough and env-var reference.

### Implementation Details

Once the service is up, smoke-test it from your laptop:

```bash
# Liveness probe (returns 200 once the listener is up)
curl https://your-beava.up.railway.app/health

# Register a pipeline + push an event + query the result, all in Python
pip install beava
python - <<'PY'
import beava as bv

@bv.event
class LoginAttempt:
    user_id: str
    success: bool

@bv.table(key="user_id")
def UserSignals(e: LoginAttempt):
    return e.group_by("user_id").agg(
        failed_logins_10m=bv.count(window="10m", where=~bv.col("success")),
        attempts_1h=bv.count(window="1h"),
    )

app = bv.App("https://your-beava.up.railway.app").register(LoginAttempt, UserSignals)
app.push("LoginAttempt", {"user_id": "alice", "success": False})
app.push("LoginAttempt", {"user_id": "alice", "success": False})
print(app.get("UserSignals", "alice"))
# => {"failed_logins_10m": 2, "attempts_1h": 2}
PY
```

To enable the TCP fast-path: set `BEAVA_TCP_ENABLED=true` and `BEAVA_TCP_HOST=0.0.0.0` in the service environment, then in Service → Networking add a TCP Proxy on port `8081`. Point the Python SDK at the resulting `proxy.rlwy.net:NNNNN` endpoint via `bv.App(tcp_url="tcp://proxy.rlwy.net:NNNNN")`.

## Why Deploy Beava on Railway?

Railway is a singular platform to deploy your infrastructure stack. Railway will host your infrastructure so you don't have to deal with configuration, while allowing you to vertically and horizontally scale it.

By deploying Beava on Railway, you are one step closer to supporting a complete full-stack application with minimal burden. Host your servers, databases, AI agents, and more on Railway.
