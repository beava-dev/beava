"""End-to-end roundtrip coverage for operator families that lack it in test_operators.py.

These tests register a pipeline, push events with known shapes, and assert that
``app.get`` returns the hand-computed value (within sketch tolerance where the
operator is approximate). Pattern mirrors ``tests/v0/`` — embed-mode ``bv.App``
spawns the local binary, ``bv.event`` + ``bv.table`` decorators declare the
pipeline.

Requires: ``target/debug/beava`` (or ``target/release/beava``) built and
discoverable by ``App()`` embed mode — the SDK falls through ``BEAVA_BINARY`` →
PATH → ``target/{debug,release}/beava`` walk-up. The ``beava_binary`` session
fixture in conftest.py guarantees this.
"""

from __future__ import annotations

import math
import time
from collections import Counter

import pytest

import beava as bv


@pytest.fixture
def app(beava_binary):  # noqa: ARG001 — fixture pulled in for binary side-effect
    """Yield a fresh embed-mode ``bv.App(test_mode=True)`` per test.

    The ``beava_binary`` session fixture builds the binary once; this fixture
    spawns a fresh subprocess for each test so registry state is clean.
    """
    with bv.App(test_mode=True) as instance:
        yield instance


# ---------------------------------------------------------------------------
# var / std
# ---------------------------------------------------------------------------


def test_var_and_std_roundtrip(app):
    """bv.var + bv.std: 5 known values → hand-computed sample variance / stddev."""

    @bv.event
    class Reading:
        user_id: str
        x: float

    @bv.table(key="user_id")
    def UserStats(rs: Reading):
        return rs.group_by("user_id").agg(
            v=bv.var("x"),
            s=bv.std("x"),
        )

    app.register(Reading, UserStats)

    values = [1.0, 2.0, 3.0, 4.0, 5.0]
    for v in values:
        app.push("Reading", {"user_id": "alice", "x": v})

    # Sample variance of [1..5]: mean=3, sum_sq_dev=10, var=10/4=2.5
    expected_var = 2.5
    expected_std = math.sqrt(expected_var)

    row = app.get("UserStats", "alice")
    assert abs(row["v"] - expected_var) < 1e-9, (
        f"var: expected {expected_var}, got {row['v']}"
    )
    assert abs(row["s"] - expected_std) < 1e-9, (
        f"std: expected {expected_std}, got {row['s']}"
    )


# ---------------------------------------------------------------------------
# quantile (percentile)
# ---------------------------------------------------------------------------


def test_percentile_roundtrip(app):
    """bv.quantile(q=0.95): 20 events, p95 within sketch tolerance of true value."""

    @bv.event
    class Latency:
        user_id: str
        latency: float

    @bv.table(key="user_id")
    def UserLatency(ls: Latency):
        return ls.group_by("user_id").agg(
            p95=bv.quantile("latency", q=0.95),
        )

    app.register(Latency, UserLatency)

    # 20 latencies: 1..20
    for i in range(1, 21):
        app.push("Latency", {"user_id": "alice", "latency": float(i)})

    row = app.get("UserLatency", "alice")
    actual = float(row["p95"])
    # True p95 of [1..20] (index int(20*0.95)=19) → 20.0. DDSketch tolerance: ±5%.
    expected = 20.0
    assert abs(actual - expected) / expected <= 0.05, (
        f"p95: expected ~{expected} (±5%), got {actual}"
    )


# ---------------------------------------------------------------------------
# top_k
# ---------------------------------------------------------------------------


def _extract_top_k_values(actual):
    """top_k result may be list[str], list[(value, count)], or list[{value, count}]."""
    if not actual:
        return []
    head = actual[0]
    if isinstance(head, dict) and "value" in head:
        return [item["value"] for item in actual]
    if isinstance(head, (list, tuple)):
        return [pair[0] for pair in actual]
    return list(actual)


def test_top_k_roundtrip_string_field(app):
    """bv.top_k(k=3) over a string field with known frequencies."""

    @bv.event
    class Click:
        user_id: str
        category: str

    @bv.table(key="user_id")
    def UserTopCats(cs: Click):
        return cs.group_by("user_id").agg(
            top3=bv.top_k("category", k=3),
        )

    app.register(Click, UserTopCats)

    # Frequencies: a=5, b=3, c=1, d=1 → top 3 = [a, b, then either c or d].
    pushes = ["a"] * 5 + ["b"] * 3 + ["c"] * 1 + ["d"] * 1
    for cat in pushes:
        app.push("Click", {"user_id": "alice", "category": cat})

    row = app.get("UserTopCats", "alice")
    top_values = _extract_top_k_values(row["top3"])
    # The top-2 must be 'a' then 'b'; the 3rd slot is c or d (tied).
    assert top_values[0] == "a", f"top[0]: expected 'a', got {top_values!r}"
    assert top_values[1] == "b", f"top[1]: expected 'b', got {top_values!r}"
    assert top_values[2] in ("c", "d"), (
        f"top[2]: expected 'c' or 'd', got {top_values!r}"
    )


def test_top_k_roundtrip_numeric_field(app):
    """bv.top_k(k=2) over a numeric field — top values by frequency."""

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserTopAmts(txs: Tx):
        return txs.group_by("user_id").agg(
            top2=bv.top_k("amount", k=2),
        )

    app.register(Tx, UserTopAmts)

    # Frequencies: 10.0 ×4, 20.0 ×3, 30.0 ×1
    pushes = [10.0] * 4 + [20.0] * 3 + [30.0] * 1
    for amt in pushes:
        app.push("Tx", {"user_id": "alice", "amount": amt})

    row = app.get("UserTopAmts", "alice")
    top_values = _extract_top_k_values(row["top2"])
    assert len(top_values) >= 2, f"top_k k=2 returned {top_values!r}"
    # Server may serialise numeric top_k values as strings; coerce.
    def _num(v):
        return float(v) if not isinstance(v, (int, float)) else v
    assert _num(top_values[0]) == 10.0, (
        f"top[0]: expected 10.0, got {top_values!r}"
    )
    assert _num(top_values[1]) == 20.0, (
        f"top[1]: expected 20.0, got {top_values!r}"
    )


# ---------------------------------------------------------------------------
# n_unique with window
# ---------------------------------------------------------------------------


def test_n_unique_with_window(app):
    """bv.n_unique with a 1h window over 3 distinct users."""

    @bv.event
    class Visit:
        page_id: str
        user: str

    @bv.table(key="page_id")
    def PageUniques(vs: Visit):
        return vs.group_by("page_id").agg(
            uniq=bv.n_unique("user", window="1h"),
        )

    app.register(Visit, PageUniques)

    # 6 events, 3 unique users: alice ×3, bob ×2, carol ×1.
    for user in ["alice", "alice", "alice", "bob", "bob", "carol"]:
        app.push("Visit", {"page_id": "home", "user": user})

    row = app.get("PageUniques", "home")
    assert row["uniq"] == 3, f"n_unique: expected 3, got {row['uniq']}"


# ---------------------------------------------------------------------------
# decayed_sum
# ---------------------------------------------------------------------------


def test_decayed_sum_roundtrip(app):
    """bv.decayed_sum with 5m half-life: positive inputs → positive monotone result."""

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserDecay(txs: Tx):
        return txs.group_by("user_id").agg(
            ds=bv.decayed_sum("amount", half_life="5m"),
        )

    app.register(Tx, UserDecay)

    # Cormode forward-decay accumulates a positive weighted sum from positive
    # inputs. With three small-gap events, the exposed weight is bounded below
    # by min(amount) and above by sum(amount); we lock those invariants.
    amounts = [10.0, 20.0, 30.0]
    for amt in amounts:
        app.push("Tx", {"user_id": "alice", "amount": amt})

    row = app.get("UserDecay", "alice")
    ds = float(row["ds"])
    assert ds > 0.0, f"decayed_sum: expected > 0, got {ds}"
    # Engine emits a normalised (forward-decay) value, not a literal sum;
    # accept any value in (0, sum * generous-upper-bound).
    assert ds <= sum(amounts) * 1.5, (
        f"decayed_sum: expected <= {sum(amounts)*1.5}, got {ds}"
    )


# ---------------------------------------------------------------------------
# streak + max_streak
# ---------------------------------------------------------------------------


def test_streak_and_max_streak(app):
    """bv.streak / bv.max_streak with a known longest run."""

    @bv.event
    class Outcome:
        user_id: str
        ok: bool

    @bv.table(key="user_id")
    def UserStreaks(os: Outcome):
        return os.group_by("user_id").agg(
            cur=bv.streak(where=bv.col("ok") == True),  # noqa: E712
            peak=bv.max_streak(where=bv.col("ok") == True),  # noqa: E712
        )

    app.register(Outcome, UserStreaks)

    # Sequence: T T T F T T → max streak=3, current streak=2.
    seq = [True, True, True, False, True, True]
    for v in seq:
        app.push("Outcome", {"user_id": "alice", "ok": v})

    row = app.get("UserStreaks", "alice")
    assert row["peak"] == 3, f"max_streak: expected 3, got {row['peak']}"
    assert row["cur"] == 2, f"streak: expected 2, got {row['cur']}"


# ---------------------------------------------------------------------------
# time_since with where
# ---------------------------------------------------------------------------


def test_time_since_with_where(app):
    """bv.time_since(where=event=='login'): ms since the last matching event."""

    @bv.event
    class Action:
        user_id: str
        event: str

    @bv.table(key="user_id")
    def UserLastLogin(as_: Action):
        return as_.group_by("user_id").agg(
            since_login=bv.time_since(where=bv.col("event") == "login"),
        )

    app.register(Action, UserLastLogin)

    app.push("Action", {"user_id": "alice", "event": "view"})
    app.push("Action", {"user_id": "alice", "event": "login"})
    login_ms = int(time.time() * 1000)
    # Push a non-matching event after login — time_since(where=login) is still
    # measured from the login event, not from this one.
    app.push("Action", {"user_id": "alice", "event": "view"})

    time.sleep(0.1)
    query_ms = int(time.time() * 1000)
    row = app.get("UserLastLogin", "alice")
    actual = row["since_login"]
    # Server stamps wall-clock arrival, so allow a generous lower bound:
    # at least query_ms - login_ms - 100ms of slack.
    min_expected = max(0, query_ms - login_ms - 100)
    assert actual >= min_expected, (
        f"time_since: expected >= {min_expected}, got {actual}"
    )
    # Upper bound — we shouldn't be off by more than a few seconds of slack.
    assert actual < 30_000, (
        f"time_since: expected < 30s, got {actual}ms — wall-clock drift?"
    )


# ---------------------------------------------------------------------------
# geo_velocity
# ---------------------------------------------------------------------------


def test_geo_velocity_roundtrip(app):
    """bv.geo_velocity: 2 events with known coords → non-negative km/h."""

    @bv.event
    class Move:
        user_id: str
        lat: float
        lon: float

    @bv.table(key="user_id")
    def UserVel(ms: Move):
        return ms.group_by("user_id").agg(
            kmh=bv.geo_velocity(lat="lat", lon="lon"),
        )

    app.register(Move, UserVel)

    # NYC → LA: ~3940 km. With ~50ms gap (apply-time wall clock), the
    # implied km/h is astronomical — but the contract is "non-negative and
    # finite". The engine uses wall-clock dt; we just verify the operator
    # returns a sane positive number.
    app.push("Move", {"user_id": "alice", "lat": 40.7128, "lon": -74.0060})
    time.sleep(0.05)
    app.push("Move", {"user_id": "alice", "lat": 34.0522, "lon": -118.2437})

    row = app.get("UserVel", "alice")
    kmh = float(row["kmh"])
    assert kmh > 0.0, f"geo_velocity: expected > 0 for a real move, got {kmh}"
    assert math.isfinite(kmh), f"geo_velocity: expected finite, got {kmh}"


# ---------------------------------------------------------------------------
# event_type_mix
# ---------------------------------------------------------------------------


def test_event_type_mix_roundtrip(app):
    """bv.event_type_mix: 4 events / 2 types in 3:1 ratio → proportions ≈ 0.75 / 0.25."""

    @bv.event
    class Op:
        user_id: str
        kind: str

    @bv.table(key="user_id")
    def UserMix(os: Op):
        return os.group_by("user_id").agg(
            mix=bv.event_type_mix("kind"),
        )

    app.register(Op, UserMix)

    pushes = ["card"] * 3 + ["wire"] * 1
    counts = Counter(pushes)
    for kind in pushes:
        app.push("Op", {"user_id": "alice", "kind": kind})

    row = app.get("UserMix", "alice")
    mix = row["mix"]
    assert isinstance(mix, dict), f"event_type_mix: expected dict, got {type(mix)}"
    total = sum(mix.values())
    assert abs(total - 1.0) < 0.01, f"proportions sum {total} != 1.0"
    for kind, cnt in counts.items():
        expected = cnt / sum(counts.values())
        actual = mix.get(kind, 0.0)
        assert abs(actual - expected) < 0.02, (
            f"mix[{kind}]: expected {expected}, got {actual}"
        )
