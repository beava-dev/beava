"""Decision-feed generator for beava.dev.

What this does
--------------
1. Connects to the beava-website-beava container at http://beava:8080.
2. Registers three demo pipelines on top of the existing SiteMetrics
   pipeline that backs the site's own page-view tracking:
     - LoginAttempt   -> UserSignals(failed_logins_10m, attempts_1h)
     - ProductClick   -> UserAffinity(recent_clicks_30m, top_categories_1h)
     - LLMRequest     -> OrgBudget(tokens_used_24h, requests_1m)
3. Pushes a synthetic event every PUSH_EVERY_S seconds, queries the
   resulting feature, composes a (event, entity, feature, decision) row,
   and keeps the most recent FEED_LEN rows in an in-memory ring buffer.
4. Serves `GET /feed` on FEED_PORT with the ring buffer + a measured
   round-trip latency for the last push+get. Caddy proxies
   /api/feed -> generator:FEED_PORT.

The events are synthetic; the FEATURE VALUES are real beava state.
Each /feed row corresponds to an actual /push + /get round trip against
the production beava on this box.
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

import beava as bv

BEAVA_URL = os.environ.get("BEAVA_URL", "http://beava:8080")
FEED_PORT = int(os.environ.get("FEED_PORT", "8090"))
PUSH_EVERY_S = float(os.environ.get("PUSH_EVERY_S", "1.4"))
FEED_LEN = int(os.environ.get("FEED_LEN", "3"))


# ─────────────────────────────────────────────────────────────────────────
# Pipelines — same shape as the three homepage tabs.
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
# Event generation
# ─────────────────────────────────────────────────────────────────────────


def _random_user() -> str:
    return f"user_{1000 + random.randint(0, 899)}"


def _random_org() -> str:
    return f"org_{random.choice(['acme', 'globex', 'umbra', 'soylent'])}"


# One scenario per pipeline tab — keep this in sync with the homepage's
# FEED_TEMPLATES order. Bias toward fraud (clearest read for new visitors).
SCENARIOS = [
    "login_failed",
    "login_failed",
    "product_clicked",
    "llm_request",
]


def _make_event(scenario: str) -> tuple[str, dict, str]:
    if scenario == "login_failed":
        uid = _random_user()
        return "LoginAttempt", {"user_id": uid, "success": False}, uid
    if scenario == "product_clicked":
        uid = _random_user()
        return "ProductClick", {
            "user_id": uid,
            "product_id": f"p_{random.randint(100, 999)}",
            "category": random.choice(["shoes", "books", "kitchen", "tools", "garden"]),
        }, uid
    if scenario == "llm_request":
        oid = _random_org()
        return "LLMRequest", {
            "org_id": oid,
            "tokens": random.randint(120, 4500),
            "model": random.choice(["gpt-5", "haiku-4-5", "sonnet-4-6"]),
        }, oid
    raise KeyError(scenario)


def _decide(scenario: str, feature_value) -> str:
    if scenario == "login_failed":
        return "require verification" if (feature_value or 0) >= 5 else "increase risk score"
    if scenario == "product_clicked":
        return "refresh recommendations"
    if scenario == "llm_request":
        kilo = (feature_value or 0) / 1000.0
        return "throttle expensive model" if kilo >= 90 else "route to cheap model"
    return ""


def _format_value(scenario: str, value):
    if scenario == "llm_request" and value is not None:
        return f"{int((value or 0) / 1000)}k"
    return value


# ─────────────────────────────────────────────────────────────────────────
# Ring buffer + pusher loop
# ─────────────────────────────────────────────────────────────────────────


_feed: deque = deque(maxlen=FEED_LEN)
_feed_lock = threading.Lock()
_latency_ms: float = 0.0


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
    _latency_ms = (time.perf_counter() - t0) * 1000
    value = row.get(feature_key) if isinstance(row, dict) else None
    decision = _decide(scenario, value)
    item = {
        "id": f"{int(time.time() * 1000)}-{random.randint(0, 9999)}",
        "ts": int(time.time() * 1000),
        "event": event_name,  # the @bv.event class name; matches the homepage code block
        "entity": entity,
        "feature": {"key": feature_key, "value": _format_value(scenario, value)},
        "decision": decision,
    }
    with _feed_lock:
        _feed.appendleft(item)


def warm_start(app: bv.App) -> None:
    """Seed the feed before the HTTP server starts serving so /feed is never empty."""
    for s in ("login_failed", "product_clicked", "llm_request"):
        _push_one(app, s)


def pusher_loop(app: bv.App) -> None:
    while True:
        try:
            _push_one(app, random.choice(SCENARIOS))
        except Exception as exc:
            sys.stderr.write(f"[generator] push error: {exc!r}\n")
        time.sleep(PUSH_EVERY_S)


# ─────────────────────────────────────────────────────────────────────────
# HTTP server — GET /feed returns the ring buffer as JSON
# ─────────────────────────────────────────────────────────────────────────


class _FeedHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):  # quiet
        if "/feed" in (args[0] if args else ""):
            return
        sys.stderr.write("[generator] " + (fmt % args) + "\n")

    def do_GET(self):  # noqa: N802
        if self.path == "/feed" or self.path == "/health":
            if self.path == "/health":
                body = b'{"status":"ok"}'
            else:
                with _feed_lock:
                    payload = {"rows": list(_feed), "latency_ms": round(_latency_ms, 1)}
                body = json.dumps(payload).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)
            return
        self.send_response(404)
        self.end_headers()


class _ThreadedHTTP(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def main() -> None:
    print(f"[generator] connecting to beava at {BEAVA_URL}", file=sys.stderr)
    # Retry registration — beava container may not be ready on first try.
    app = bv.App(BEAVA_URL)
    app.__enter__()
    for attempt in range(60):
        try:
            app.register(
                LoginAttempt, UserSignals,
                ProductClick, UserAffinity,
                LLMRequest, OrgBudget,
                force=True,
            )
            print("[generator] pipelines registered", file=sys.stderr)
            break
        except Exception as exc:
            sys.stderr.write(f"[generator] register attempt {attempt + 1} failed: {exc!r}\n")
            time.sleep(2)
    else:
        sys.exit("[generator] could not register pipelines after 60 attempts")

    print("[generator] warm-starting feed...", file=sys.stderr)
    warm_start(app)

    t = threading.Thread(target=pusher_loop, args=(app,), daemon=True)
    t.start()

    print(f"[generator] serving /feed on 0.0.0.0:{FEED_PORT}", file=sys.stderr)
    server = _ThreadedHTTP(("0.0.0.0", FEED_PORT), _FeedHandler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.shutdown()
        app.close()


if __name__ == "__main__":
    main()
