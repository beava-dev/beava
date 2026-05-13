"""Decision-feed generator for beava.dev.

What this does
--------------
1. Connects to the beava-website-beava container at http://beava:8080.
2. Owns the FULL registry for the beava.dev instance and registers it
   on startup with force=true. Three verticals, two reflex signals each:

     AI agents
       AgentStep    -> AgentReflexes        (key=agent_id;   steps_30s)
       ToolCall     -> SessionReflexes      (key=session_id; risky_tools_10m)

     Marketplaces
       AddToCart    -> SkuMomentum          (key=sku;        carts_5m)
       ProductView  -> ShopperReflexes      (key=user_id;    avg_view_price_30m)

     B2B SaaS
       ApiError     -> UserActivation       (key=user_id;    errors_10m,
                                                             top_topic_10m)
       LimitHit     -> OrgExpansionSignals  (key=org_id;     limit_hits_24h)

     Plus PageView -> SiteMetrics for the site's own visitor analytics.

   Wholesale ownership matters: beava /register with force=true REPLACES
   the registry. Keeping all seven event+table pairs here means the
   generator owns the schema.

3. Pushes a synthetic event every PUSH_EVERY_S seconds, cycling through
   the six demo scenarios in balanced 1:1:1:1:1:1 order. Each push reads
   back the resulting feature row and composes an
   (event, entity, feature, decision) tuple for the ring buffer.
   PageView is fed by real visitors via /api/push/PageView; not
   synthesized here.

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
# Pipelines
# ─────────────────────────────────────────────────────────────────────────


# Real-visitor page-view counter. Fed by /api/push/PageView from
# project/js/track-pageview.js on every page load on beava.dev.
@bv.event
class PageView:
    session_id: str
    path: str
    dwell_ms: int


@bv.table  # no key= -> one row, site-wide (ADR-003)
def SiteMetrics(e: PageView):
    return e.agg(
        median_dwell_1h=bv.quantile("dwell_ms", q=0.5, window="1h"),
        page_views_24h=bv.count(window="24h"),
        top_page_1h=bv.top_k("path", k=1, window="1h"),
    )


# ── AI agents ────────────────────────────────────────────────────────────


@bv.event
class AgentStep:
    agent_id: str
    action: str


@bv.table(key="agent_id")
def AgentReflexes(e: AgentStep):
    return e.group_by("agent_id").agg(
        repeated_action_30s=bv.count(window="30s"),
    )


@bv.event
class ToolCall:
    session_id: str
    tool: str
    is_risky: bool


@bv.table(key="session_id")
def SessionReflexes(e: ToolCall):
    return e.group_by("session_id").agg(
        risky_tools_10m=bv.count(window="10m", where=bv.col("is_risky")),
    )


# ── Marketplaces ─────────────────────────────────────────────────────────


@bv.event
class AddToCart:
    user_id: str
    sku: str


@bv.table(key="sku")
def SkuMomentum(e: AddToCart):
    return e.group_by("sku").agg(
        carts_5m=bv.count(window="5m"),
    )


@bv.event
class ProductView:
    user_id: str
    sku: str
    price_usd: float


@bv.table(key="user_id")
def ShopperReflexes(e: ProductView):
    return e.group_by("user_id").agg(
        avg_view_price_30m=bv.mean("price_usd", window="30m"),
    )


# ── B2B SaaS ─────────────────────────────────────────────────────────────


@bv.event
class ApiError:
    user_id: str
    topic: str
    retries: int  # cumulative retry count for this user's session


@bv.table(key="user_id")
def UserActivation(e: ApiError):
    return e.group_by("user_id").agg(
        error_velocity_5m=bv.rate_of_change("retries", window="5m"),
    )


@bv.event
class LimitHit:
    org_id: str
    feature: str


@bv.table(key="org_id")
def OrgExpansionSignals(e: LimitHit):
    return e.group_by("org_id").agg(
        limit_hits_24h=bv.count(window="24h"),
    )


# ─────────────────────────────────────────────────────────────────────────
# Event generation
# ─────────────────────────────────────────────────────────────────────────


# Tight pools — small sets so each entity reliably accumulates enough
# events under the 1.4s push cadence to cross the dramatic thresholds.
_AGENT_POOL = ("agent_42", "agent_77", "agent_91")
_SESSION_POOL = ("session_12", "session_44", "session_81")
_SKU_POOL = ("sku_882", "sku_113", "sku_547", "sku_209")
_ORG_POOL = ("org_acme", "org_globex", "org_umbra", "org_soylent")
_AGENT_ACTIONS = ("http_get", "shell_exec", "code_run", "search", "file_read")
_TOOLS_RISKY = ("shell_exec", "external_send", "file_write")
_TOOLS_SAFE = ("http_get", "search", "code_read")
_ERROR_TOPICS = ("auth", "rate_limit", "schema", "permissions")
_LIMIT_FEATURES = ("seats", "api_calls", "storage_gb")


def _random_user() -> str:
    return f"user_{1000 + random.randint(0, 1499)}"


# Balanced 1:1:1:1:1:1 rotation across all six signals so the 3-row feed
# typically surfaces three different verticals instead of stacking one.
SCENARIOS = (
    "agent_steps",
    "tool_risk",
    "cart_momentum",
    "view_price",
    "user_errors",
    "limit_hits",
)


def _make_event(scenario: str) -> tuple[str, dict, str]:
    if scenario == "agent_steps":
        aid = random.choice(_AGENT_POOL)
        return "AgentStep", {"agent_id": aid, "action": random.choice(_AGENT_ACTIONS)}, aid
    if scenario == "tool_risk":
        sid = random.choice(_SESSION_POOL)
        is_risky = random.random() < 0.65  # bias risky so the count clears the approval gate
        tool = random.choice(_TOOLS_RISKY if is_risky else _TOOLS_SAFE)
        return "ToolCall", {"session_id": sid, "tool": tool, "is_risky": is_risky}, sid
    if scenario == "cart_momentum":
        sku = random.choice(_SKU_POOL)
        return "AddToCart", {"user_id": _random_user(), "sku": sku}, sku
    if scenario == "view_price":
        uid = _random_user()
        # Bias toward premium ($180–$280) so avg_view_price_30m sits in the
        # "premium picks" decision band.
        price = round(random.uniform(180.0, 280.0), 2)
        return (
            "ProductView",
            {"user_id": uid, "sku": random.choice(_SKU_POOL), "price_usd": price},
            uid,
        )
    if scenario == "user_errors":
        uid = _random_user()
        # Push monotonically-increasing retry count so rate_of_change reads as
        # a meaningful slope (≈ 0.6–2.8 /sec) under the 1.4s push cadence.
        retries = random.randint(3, 15)
        return (
            "ApiError",
            {"user_id": uid, "topic": random.choice(_ERROR_TOPICS), "retries": retries},
            uid,
        )
    if scenario == "limit_hits":
        oid = random.choice(_ORG_POOL)
        return "LimitHit", {"org_id": oid, "feature": random.choice(_LIMIT_FEATURES)}, oid
    raise KeyError(scenario)


def _decide(scenario: str, feature_value) -> str:
    if scenario == "agent_steps":
        return "pause agent loop" if (feature_value or 0) >= 6 else "continue monitoring"
    if scenario == "tool_risk":
        return "require approval" if (feature_value or 0) >= 2 else "allow"
    if scenario == "cart_momentum":
        return "boost trending item" if (feature_value or 0) >= 50 else "keep default ranking"
    if scenario == "view_price":
        return "sort toward premium picks" if (feature_value or 0) >= 200 else "keep value picks"
    if scenario == "user_errors":
        # error_velocity_5m is a slope (≈ 0.6–2.8 /sec). > 0.5 triggers rescue.
        return "launch setup rescue" if (feature_value or 0) >= 0.5 else "let user retry"
    if scenario == "limit_hits":
        return "show team upgrade path" if (feature_value or 0) >= 10 else "send usage tip"
    return ""


# Maps scenario -> (table_name, feature_key).
_SCENARIO_TARGETS = {
    "agent_steps":   ("AgentReflexes",        "repeated_action_30s"),
    "tool_risk":     ("SessionReflexes",      "risky_tools_10m"),
    "cart_momentum": ("SkuMomentum",          "carts_5m"),
    "view_price":    ("ShopperReflexes",      "avg_view_price_30m"),
    "user_errors":   ("UserActivation",       "error_velocity_5m"),
    "limit_hits":    ("OrgExpansionSignals",  "limit_hits_24h"),
}


def _format_value(scenario: str, row: dict | None, feature_value):
    """Compose the value string the homepage feed displays.

    Most scenarios just show the raw feature value. Two exceptions:
    - view_price: prepend a "$" so the row reads avg_view_price_30m = $240.
    - user_errors: render the rate_of_change slope as a signed number,
      matching the synthetic feed's `+1.2` display.
    """
    if feature_value is None:
        return feature_value
    if scenario == "view_price":
        return f"${int(feature_value)}"
    if scenario == "user_errors":
        try:
            return f"+{float(feature_value):.1f}"
        except (TypeError, ValueError):
            return feature_value
    return feature_value


# ─────────────────────────────────────────────────────────────────────────
# Ring buffer + pusher loop
# ─────────────────────────────────────────────────────────────────────────


_feed: deque = deque(maxlen=FEED_LEN)
_feed_lock = threading.Lock()
_latency_ms: float = 0.0


def _push_one(app: bv.App, scenario: str) -> None:
    global _latency_ms
    event_name, fields, entity = _make_event(scenario)
    table, feature_key = _SCENARIO_TARGETS[scenario]
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
        "feature": {"key": feature_key, "value": _format_value(scenario, row, value)},
        "decision": decision,
    }
    with _feed_lock:
        _feed.appendleft(item)


def warm_start(app: bv.App) -> None:
    """Seed the feed before the HTTP server starts serving so /feed is never empty.

    Pick one row per vertical so the first paint shows the spread.
    """
    for s in ("agent_steps", "cart_momentum", "user_errors"):
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
                PageView, SiteMetrics,
                AgentStep, AgentReflexes,
                ToolCall, SessionReflexes,
                AddToCart, SkuMomentum,
                ProductView, ShopperReflexes,
                ApiError, UserActivation,
                LimitHit, OrgExpansionSignals,
                force=True,
            )
            print("[generator] pipelines registered (1 site + 3 verticals × 2 signals)", file=sys.stderr)
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
