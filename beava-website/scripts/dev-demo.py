"""Hook the homepage's Live decision feed up to a real beava server.

What this does
--------------
1. Connects to a beava server (you start it separately: see the comment block
   at the bottom). Registers three pipelines that match the homepage tabs —
   fraud (LoginAttempt → UserSignals), recommendations (ProductClick →
   UserAffinity), and guardrails (LLMRequest → OrgBudget).
2. Spawns a pusher thread that picks a random scenario every ~1.4s, pushes
   the event, queries the resulting feature row, and emits a decision.
3. Serves the website's static files from ./beava-website/project AND a
   small JSON endpoint at /feed that the homepage polls. One origin, no
   CORS hassle.

Run
---
    cargo run --release --bin beava -- --http-addr 127.0.0.1:6400 \\
        --memory-only --test-mode &
    python3 beava-website/scripts/dev-demo.py

Then open http://127.0.0.1:8889/ — the hero feed is now real beava state.
"""
from __future__ import annotations

import http.server
import json
import os
import random
import socketserver
import sys
import threading
import time
from collections import deque
from pathlib import Path

# Make the in-tree Python SDK importable without `pip install -e`.
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "python"))

import beava as bv  # noqa: E402

BEAVA_URL = os.environ.get("BEAVA_URL", "http://127.0.0.1:6400")
SITE_DIR = ROOT / "beava-website" / "project"
WEB_PORT = int(os.environ.get("WEB_PORT", "8889"))
PUSH_EVERY_S = float(os.environ.get("PUSH_EVERY_S", "1.4"))
FEED_LEN = 3

# ─────────────────────────────────────────────────────────────────────────
# Pipelines — one per decision category shown on the homepage tabs.
# Keeping them small so the demo registers in <1s.
# ─────────────────────────────────────────────────────────────────────────


@bv.event
class LoginAttempt:
    user_id: str
    success: bool


@bv.table(key="user_id")
def UserSignals(e: LoginAttempt):
    return e.group_by("user_id").agg(
        failed_logins_10m=bv.count(window="10m", where=bv.col("success") == False),  # noqa: E712
        attempts_1h=bv.count(window="1h"),
    )


@bv.event
class ProductClick:
    user_id: str
    product_id: str
    category: str


@bv.table(key="user_id")
def UserAffinity(e: ProductClick):
    return e.group_by("user_id").agg(
        recent_clicks_30m=bv.count(window="30m"),
        top_categories_1h=bv.top_k("category", k=3, window="1h"),
    )


@bv.event
class LLMRequest:
    org_id: str
    tokens: int
    model: str


@bv.table(key="org_id")
def OrgBudget(e: LLMRequest):
    return e.group_by("org_id").agg(
        tokens_used_24h=bv.sum("tokens", window="24h"),
        requests_1m=bv.count(window="1m"),
    )


# ─────────────────────────────────────────────────────────────────────────
# Pusher — turns the homepage's six "scenarios" into real beava events
# ─────────────────────────────────────────────────────────────────────────


def _random_user() -> str:
    return f"user_{1000 + random.randint(0, 899)}"


def _random_org() -> str:
    return f"org_{random.choice(['acme', 'globex', 'umbra', 'soylent'])}"


SCENARIOS = [
    # One scenario per registered pipeline tab. Same order, same feature key
    # the homepage code block declares. Don't add a scenario here without a
    # matching tab on the page — the live feed would surface a feature key
    # the user can't find in any visible pipeline.
    "login_failed",
    "login_failed",  # bias toward fraud — clearest read for a new visitor
    "product_clicked",
    "llm_request",
]


def _make_event(name: str) -> tuple[str, dict, str]:
    """Return (event_name, fields, entity_key)."""
    if name == "login_failed":
        uid = _random_user()
        return "LoginAttempt", {"user_id": uid, "success": False}, uid
    if name == "product_clicked":
        uid = _random_user()
        cat = random.choice(["shoes", "books", "kitchen", "tools", "garden"])
        return "ProductClick", {
            "user_id": uid,
            "product_id": f"p_{random.randint(100, 999)}",
            "category": cat,
        }, uid
    if name == "llm_request":
        oid = _random_org()
        return "LLMRequest", {
            "org_id": oid,
            "tokens": random.randint(120, 4500),
            "model": random.choice(["gpt-5", "haiku-4-5", "sonnet-4-6"]),
        }, oid
    raise KeyError(name)


def _decide(scenario: str, feature_value) -> str:
    """Pick a decision label given the current feature value.

    Each scenario has at least one **passive** branch ("continue
    monitoring", "allow", "keep …") for the case where the feature
    hasn't crossed an action threshold. The homepage feed renders
    those in green so the reader can scan the panel and see which
    rows fired and which didn't — every-row-orange reads as
    every-row-fired, which is wrong.
    """
    if scenario == "login_failed":
        n = feature_value or 0
        if n >= 5:
            return "require verification"
        if n >= 2:
            return "increase risk score"
        return "continue monitoring"
    if scenario == "product_clicked":
        n = feature_value or 0
        if n >= 5:
            return "refresh recommendations"
        return "keep default recommendations"
    if scenario == "llm_request":
        kilo = (feature_value or 0) / 1000.0
        if kilo >= 90:
            return "throttle expensive model"
        if kilo >= 30:
            return "route to cheap model"
        return "allow"
    return ""


def _format_value(scenario: str, value):
    if scenario == "llm_request" and value is not None:
        return f"{int((value or 0) / 1000)}k"
    return value


# Ring buffer of recent decisions, newest-first. Thread-safe via the GIL on
# list operations + an explicit lock around list/deque swaps.
_feed: deque = deque(maxlen=FEED_LEN)
_feed_lock = threading.Lock()
_latency_ms: float = 0.0


def warm_start(app: bv.App) -> None:
    """Synchronously seed the feed before the HTTP server starts serving.

    Why synchronously: if the steady-state pusher runs in a daemon thread
    AND the HTTP server starts at the same time, /feed can serve an empty
    ring buffer for the first few hundred ms. The homepage flips to
    synthetic mode on an empty response, which is the wrong default for
    a freshly-booted demo.
    """
    for s in ("login_failed", "product_clicked", "llm_request"):
        _push_one(app, s)


def pusher_loop(app: bv.App) -> None:
    while True:
        scenario = random.choice(SCENARIOS)
        try:
            _push_one(app, scenario)
        except Exception as exc:  # surface but don't die
            sys.stderr.write(f"[dev-demo] push error: {exc!r}\n")
        time.sleep(PUSH_EVERY_S)


def _push_one(app: bv.App, scenario: str) -> None:
    global _latency_ms
    event_name, fields, entity = _make_event(scenario)
    table = {
        "LoginAttempt": "UserSignals",
        "ProductClick": "UserAffinity",
        "LLMRequest": "OrgBudget",
    }[event_name]
    feature_key = {
        "login_failed": "failed_logins_10m",
        "product_clicked": "recent_clicks_30m",
        "llm_request": "tokens_used_24h",
    }[scenario]
    t0 = time.perf_counter()
    app.push(event_name, fields)
    row = app.get(table, key=entity)
    elapsed = (time.perf_counter() - t0) * 1000
    _latency_ms = elapsed
    value = row.get(feature_key) if isinstance(row, dict) else None
    decision = _decide(scenario, value)
    item = {
        "id": f"{int(time.time() * 1000)}-{random.randint(0, 9999)}",
        "ts": int(time.time() * 1000),
        "event": event_name,  # @bv.event class name; matches the code block in the tab below
        "entity": entity,
        "feature": {"key": feature_key, "value": _format_value(scenario, value)},
        "decision": decision,
    }
    with _feed_lock:
        _feed.appendleft(item)


# ─────────────────────────────────────────────────────────────────────────
# HTTP server — static files + /feed
# ─────────────────────────────────────────────────────────────────────────


class _Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(SITE_DIR), **kwargs)

    def log_message(self, fmt, *args):  # quiet the access log
        try:
            msg = fmt % args
        except Exception:
            msg = fmt
        if "/api/feed" in msg:
            return
        sys.stderr.write("[dev-demo] " + msg + "\n")

    def do_GET(self):  # noqa: N802
        if self.path.startswith("/api/feed"):
            with _feed_lock:
                payload = {
                    "rows": list(_feed),
                    "latency_ms": round(_latency_ms, 1),
                }
            body = json.dumps(payload).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.write(body)
            return
        return super().do_GET()

    def send_error(self, code, message=None, explain=None):
        # Cloudflare Pages serves /404.html on any unmatched URL. Mirror that
        # locally so devs see the real 404 page, not Python's default plaintext.
        if code == 404:
            page = SITE_DIR / "404.html"
            if page.is_file():
                body = page.read_bytes()
                self.send_response(404)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                if self.command != "HEAD":
                    self.wfile.write(body)
                return
        super().send_error(code, message, explain)


class _ThreadedHTTP(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def main() -> None:
    print(f"[dev-demo] connecting to beava at {BEAVA_URL}", file=sys.stderr)
    app = bv.App(BEAVA_URL)
    app.__enter__()
    try:
        print("[dev-demo] registering pipelines", file=sys.stderr)
        app.register(
            LoginAttempt, UserSignals,
            ProductClick, UserAffinity,
            LLMRequest, OrgBudget,
            force=True,
        )
    except Exception as exc:
        print(f"[dev-demo] register failed: {exc!r}", file=sys.stderr)
        sys.exit(1)

    t = threading.Thread(target=pusher_loop, args=(app,), daemon=True)
    t.start()

    print(f"[dev-demo] serving {SITE_DIR} on http://127.0.0.1:{WEB_PORT}", file=sys.stderr)
    print(f"[dev-demo] feed at      http://127.0.0.1:{WEB_PORT}/feed", file=sys.stderr)
    server = _ThreadedHTTP(("127.0.0.1", WEB_PORT), _Handler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("[dev-demo] shutting down", file=sys.stderr)
        server.shutdown()
        app.close()


if __name__ == "__main__":
    main()
