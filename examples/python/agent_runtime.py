"""
Agent runtime control: catch agent loops before the next tool call.

Tracks per-session features over an agent's tool calls — failure rate,
top tool, unique tools, token burn, p95 latency, risky streak, last
action — and prints which reflex would fire (pause loop, require human
approval, switch to a cheaper model).

Run:

    # start the server in another terminal
    beava --data-dir ./.beava/

    # then
    python3 examples/python/agent_runtime.py

Or swap the URL to ``None`` and wrap in ``with`` to use embed mode.
"""
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


def main() -> int:
    app = bv.App("http://localhost:8080")
    app.register(AgentStep, SessionReflexes, force=True)

    # Six tool calls. Three fail, two are risky, token burn climbs.
    steps = [
        ("search",    "browser",    True,  False,  120,  90),
        ("tool_call", "browser",    False, True,  2400, 510),
        ("tool_call", "shell_exec", False, True,  3100, 720),
        ("model_call","http_get",   True,  False, 1800, 240),
        ("tool_call", "browser",    False, False, 4200, 830),
        ("tool_call", "code_run",   True,  False, 5000, 410),
    ]
    for action, tool, ok, risky, tokens, latency in steps:
        app.push("AgentStep", {
            "session_id": "session_44",
            "agent_id":   "agent_91",
            "action":     action,
            "tool":       tool,
            "ok":         ok,
            "risky":      risky,
            "tokens":     tokens,
            "latency_ms": latency,
        })

    features = app.get("SessionReflexes", "session_44")
    print(f"session_44 reflexes: {features}")

    if features.get("failure_rate_5m", 0) > 0.4:
        print("reflex: lock tool access")
    if features.get("risky_streak", 0) >= 2:
        print("reflex: require human approval")
    if features.get("token_burn_1m", 0) > 10_000:
        print("reflex: switch to a cheaper model")

    print("OK -- agent_runtime.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
