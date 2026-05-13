"""
SaaS growth rescue: catch stuck users before the session ends.

Tracks per-user friction (error loops, time since last success, decayed
docs-page reads) and per-org expansion signals (limit hits, invite
momentum) to fire setup-rescue / upgrade-path reflexes.

Run:

    beava --data-dir ./.beava/
    python3 examples/python/growth_rescue.py
"""
import beava as bv


@bv.event
class UsageEvent:
    user_id: str
    org_id: str
    surface: str       # "app" | "docs" | "cli" | "api"
    topic: str         # "auth" | "webhook" | "billing" | "schema" | "deploy"
    outcome: str       # "ok" | "error" | "limit_hit"
    docs_views: int


@bv.table(key="user_id")
def UserFriction(e: UsageEvent):
    return e.group_by("user_id").agg(
        errors_10m          = bv.count(window="10m", where=bv.col("outcome") == "error"),
        top_error_topic_10m = bv.top_k("topic", k=1, window="10m",
                                       where=bv.col("outcome") == "error"),
        docs_burn_10m       = bv.decayed_sum("docs_views", half_life="5m"),
        time_since_ok       = bv.time_since(where=bv.col("outcome") == "ok"),
        last_topic          = bv.last("topic"),
    )


@bv.table(key="org_id")
def OrgExpansion(e: UsageEvent):
    return e.group_by("org_id").agg(
        limit_hits_24h    = bv.count(window="24h", where=bv.col("outcome") == "limit_hit"),
        active_users_24h  = bv.n_unique("user_id", window="24h"),
        api_calls_1h      = bv.count(window="1h", where=bv.col("surface") == "api"),
    )


def main() -> int:
    app = bv.App("http://localhost:8080")
    app.register(UsageEvent, UserFriction, OrgExpansion, force=True)

    # user_1271 is stuck on auth; org_acme is hitting limits across users.
    flow = [
        ("user_1271", "org_acme",  "app",  "auth",    "error",     1),
        ("user_1271", "org_acme",  "docs", "auth",    "ok",        3),
        ("user_1271", "org_acme",  "app",  "auth",    "error",     2),
        ("user_1271", "org_acme",  "cli",  "auth",    "error",     1),
        ("user_1271", "org_acme",  "docs", "webhook", "ok",        5),
        ("user_4480", "org_acme",  "api",  "billing", "limit_hit", 0),
        ("user_4480", "org_acme",  "api",  "billing", "limit_hit", 0),
        ("user_4480", "org_acme",  "api",  "billing", "limit_hit", 0),
        ("user_5921", "org_acme",  "api",  "deploy",  "limit_hit", 0),
        ("user_5921", "org_acme",  "api",  "deploy",  "limit_hit", 0),
    ]
    for user, org, surface, topic, outcome, docs in flow:
        app.push("UsageEvent", {
            "user_id":  user,  "org_id":     org,
            "surface":  surface, "topic":    topic,
            "outcome":  outcome, "docs_views": docs,
        })

    user = app.get("UserFriction", "user_1271")
    org = app.get("OrgExpansion", "org_acme")
    print(f"user_1271 friction:  {user}")
    print(f"org_acme  expansion: {org}")

    if user.get("errors_10m", 0) >= 2:
        print("reflex: launch setup rescue for user_1271")
    if user.get("docs_burn_10m", 0) >= 5:
        print("reflex: open guided onboarding for user_1271")
    if org.get("limit_hits_24h", 0) >= 5:
        print("reflex: surface upgrade path to org_acme")

    print("OK -- growth_rescue.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
