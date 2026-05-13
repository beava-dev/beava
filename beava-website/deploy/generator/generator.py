"""Decision-feed generator for beava.dev.

What this does
--------------
1. Connects to the beava-website-beava container at http://beava:8080.
2. Owns the FULL registry for the beava.dev instance and registers it
   on startup with force=true:
     - PageView    -> SiteMetrics       (real visitor page-view counter)
     - AgentStep   -> AgentGuardrails   (repeated_action_30s)
     - ModelCall   -> OrgBudget         (token_burn_rate_1m)
     - UserIntent  -> UserAffinity      (intent_now)

   Wholesale ownership matters: beava /register with force=true REPLACES
   the registry. If two services both register with force=true but
   different sets, the last one wins and the other's pipelines vanish.
   Keeping all four definitions here means the generator owns the
   schema; the deploy workflow no longer needs a separate register step.

3. Pushes a synthetic event every PUSH_EVERY_S seconds for the three
   demo pipelines, queries the resulting feature, composes a
   (event, entity, feature, decision) row, and keeps the most recent
   FEED_LEN rows in an in-memory ring buffer. PageView is fed by real
   visitors via /api/push/PageView; we don't synthesize page-views here.

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
# Pipelines — three "live product reflex" categories: agent safety,
# model-cost routing, and user-intent personalization. Plus PageView for
# the site's own visitor analytics. Keep all four here; the generator's
# register call is the registry's single source of truth.
# ─────────────────────────────────────────────────────────────────────────


# Real-visitor page-view counter. Fed by /api/push/PageView from
# project/js/track-pageview.js on every page load on beava.dev.
@bv.event
class PageView:
    session_id: str
    path: str
    dwell_ms: int  # set when the visitor leaves the page


@bv.table  # no key= -> one row, site-wide (ADR-003)
def SiteMetrics(e: PageView):
    return e.agg(
        median_dwell_1h=bv.quantile("dwell_ms", q=0.5, window="1h"),
        page_views_24h=bv.count(window="24h"),
        top_page_1h=bv.top_k("path", k=1, window="1h"),
    )


# Agent safety reflex — an AgentStep is one tool call / one action the
# agent took. AgentGuardrails surfaces "how busy this agent has been in
# the last 30s" as `repeated_action_30s`; the app pauses the agent when
# the count crosses the threshold.
@bv.event
class AgentStep:
    agent_id: str
    action: str


@bv.table(key="agent_id")
def AgentGuardrails(e: AgentStep):
    return e.group_by("agent_id").agg(
        repeated_action_30s=bv.count(window="30s"),
    )


# Model-cost reflex — every model API call is a ModelCall with a token
# count. OrgBudget surfaces `token_burn_rate_1m` (sum of tokens per
# minute per org); when it crosses 100k/min, the app routes to a
# cheaper model.
@bv.event
class ModelCall:
    org_id: str
    tokens: int
    model: str


@bv.table(key="org_id")
def OrgBudget(e: ModelCall):
    return e.group_by("org_id").agg(
        token_burn_rate_1m=bv.sum("tokens", window="1m"),
    )


# Personalization reflex — UserIntent is an inferred-or-declared intent
# for the current session. UserAffinity exposes `intent_now` (last seen
# intent) so the app can render the right next screen.
@bv.event
class UserIntent:
    user_id: str
    intent: str


@bv.table(key="user_id")
def UserAffinity(e: UserIntent):
    return e.group_by("user_id").agg(
        intent_now=bv.last("intent"),
    )


# ─────────────────────────────────────────────────────────────────────────
# Event generation
# ─────────────────────────────────────────────────────────────────────────


# Tight agent pool — 2 ids so each agent reliably accumulates enough
# repeated_action_30s under the 1.4s push cadence even with balanced
# 1:1:1 scenario rotation. Smaller pool than the org/user pools below
# because agent_loop is the only scenario whose decision threshold
# depends on accumulation; cost and personalization don't need
# concentration.
_AGENT_POOL = ("agent_42", "agent_77")
_ORG_POOL = ("org_acme", "org_globex", "org_umbra", "org_soylent")
_AGENT_ACTIONS = ("http_get", "shell_exec", "code_run", "search", "file_read")
_INTENTS = ("compare plans", "browse docs", "see api reference", "ask sales")


def _random_user() -> str:
    return f"user_{1000 + random.randint(0, 899)}"


# Balanced 1:1:1 rotation so the 3-row feed surfaces all three reflex
# categories instead of stacking agent rows. With FEED_LEN=3, equal
# weights mean visitors typically see one agent / one cost / one
# personalization row at any moment.
SCENARIOS = [
    "agent_loop",
    "model_cost",
    "user_intent",
]


def _make_event(scenario: str) -> tuple[str, dict, str]:
    if scenario == "agent_loop":
        aid = random.choice(_AGENT_POOL)
        return "AgentStep", {"agent_id": aid, "action": random.choice(_AGENT_ACTIONS)}, aid
    if scenario == "model_cost":
        oid = random.choice(_ORG_POOL)
        # Per-call tokens chosen so token_burn_rate_1m (sum over 1m) tends
        # to clear the 100k threshold for the routing decision.
        return "ModelCall", {
            "org_id": oid,
            "tokens": random.randint(18_000, 35_000),
            "model": random.choice(["gpt-5", "claude-opus-4-7", "sonnet-4-6"]),
        }, oid
    if scenario == "user_intent":
        uid = _random_user()
        return "UserIntent", {"user_id": uid, "intent": random.choice(_INTENTS)}, uid
    raise KeyError(scenario)


def _decide(scenario: str, feature_value) -> str:
    if scenario == "agent_loop":
        return "pause runaway agent" if (feature_value or 0) >= 5 else "continue monitoring"
    if scenario == "model_cost":
        kilo = (feature_value or 0) / 1000.0
        return "switch to cheaper model" if kilo >= 100 else "route to default model"
    if scenario == "user_intent":
        if feature_value == "compare plans":
            return "show pricing assistant"
        if feature_value == "ask sales":
            return "escalate to human"
        return "personalize next screen"
    return ""


def _format_value(scenario: str, value):
    if scenario == "model_cost" and value is not None:
        # Sum is in raw tokens; surface a per-minute rate (same value,
        # since the window is exactly 1m) with a /min suffix.
        return f"{int(value / 1000)}k/min"
    if scenario == "user_intent" and value is not None:
        # Quoted string so the row reads naturally:
        # `user_1094.intent_now = "compare plans"`
        return f'"{value}"'
    return value


# ─────────────────────────────────────────────────────────────────────────
# Ring buffer + pusher loop
# ─────────────────────────────────────────────────────────────────────────


_feed: deque = deque(maxlen=FEED_LEN)
_feed_lock = threading.Lock()
_latency_ms: float = 0.0


# Maps scenario -> (table_name, feature_key) used by /get + display.
_SCENARIO_TARGETS = {
    "agent_loop":  ("AgentGuardrails", "repeated_action_30s"),
    "model_cost":  ("OrgBudget",       "token_burn_rate_1m"),
    "user_intent": ("UserAffinity",    "intent_now"),
}


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
        "feature": {"key": feature_key, "value": _format_value(scenario, value)},
        "decision": decision,
    }
    with _feed_lock:
        _feed.appendleft(item)


def warm_start(app: bv.App) -> None:
    """Seed the feed before the HTTP server starts serving so /feed is never empty."""
    for s in ("agent_loop", "model_cost", "user_intent"):
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
                AgentStep, AgentGuardrails,
                ModelCall, OrgBudget,
                UserIntent, UserAffinity,
                force=True,
            )
            print("[generator] pipelines registered (4: SiteMetrics + 3 reflex demos)", file=sys.stderr)
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
