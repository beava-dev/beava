"""Point/ordinal operator tests — first / last / first_n / last_n / lag.

5 tests, each pushing >=500 events per test (>=100 per entity across 5 entities)
with known sequencing. Order-sensitive ops; expected values computed by tracking
arrival order in Python.
"""
from __future__ import annotations

import random
from collections import deque

import pytest

import beava as bv

from ._helpers import ENTITIES, _engine_available, cold_start_equivalent

pytestmark = pytest.mark.skipif(
    not _engine_available(),
    reason="requires Phase 13.4 engine + Phase 13.5 SDK rewrite",
)


# ---------------------------------------------------------------------------
# Test 1: first
# ---------------------------------------------------------------------------


def test_first_per_user_high_volume(app):
    """bv.first: 500 events / 5 users; per-entity first-observed value sticky."""

    @bv.event
    class Login:
        user_id: str
        device_id: str

    @bv.table(key="user_id")
    def UserFirstDevice(logins: Login):
        return logins.group_by("user_id").agg(
            first_device=bv.first("device_id"),
        )

    app.register(Login, UserFirstDevice)

    rng = random.Random(60)
    first_seen: dict[str, str] = {}
    for _ in range(500):
        user = rng.choice(ENTITIES)
        device = f"device-{rng.randint(0, 19):02d}"
        if user not in first_seen:
            first_seen[user] = device
        app.push("Login", {"user_id": user, "device_id": device})

    for entity, exp in first_seen.items():
        result = app.get("UserFirstDevice", entity)
        assert result["first_device"] == exp, (
            f"{entity}: expected first_device={exp!r}, got {result['first_device']!r}"
        )

    assert cold_start_equivalent(app.get("UserFirstDevice", "unknown_first"))


# ---------------------------------------------------------------------------
# Test 2: last
# ---------------------------------------------------------------------------


def test_last_per_user_high_volume(app):
    """bv.last: 500 events / 5 users; per-entity last-observed value (overwrite)."""

    @bv.event
    class Beacon:
        user_id: str
        ip: str

    @bv.table(key="user_id")
    def UserLastIp(beacons: Beacon):
        return beacons.group_by("user_id").agg(
            last_ip=bv.last("ip"),
        )

    app.register(Beacon, UserLastIp)

    rng = random.Random(61)
    last_seen: dict[str, str] = {}
    for _ in range(500):
        user = rng.choice(ENTITIES)
        ip = f"10.0.{rng.randint(0, 255)}.{rng.randint(0, 255)}"
        last_seen[user] = ip  # overwrite — keep latest
        app.push("Beacon", {"user_id": user, "ip": ip})

    for entity, exp in last_seen.items():
        result = app.get("UserLastIp", entity)
        assert result["last_ip"] == exp, (
            f"{entity}: expected last_ip={exp!r}, got {result['last_ip']!r}"
        )

    assert cold_start_equivalent(app.get("UserLastIp", "unknown_last"))


# ---------------------------------------------------------------------------
# Test 3: first_n (n=5)
# ---------------------------------------------------------------------------


def test_first_n_per_user_high_volume(app):
    """bv.first_n (n=5): 500 events / 5 users; first 5 sticky values per entity."""

    @bv.event
    class Click:
        user_id: str
        target: str

    @bv.table(key="user_id")
    def UserFirstFive(clicks: Click):
        return clicks.group_by("user_id").agg(
            opens=bv.first_n("target", n=5),
        )

    app.register(Click, UserFirstFive)

    rng = random.Random(62)
    capture: dict[str, list[str]] = {entity: [] for entity in ENTITIES}
    for i in range(500):
        user = rng.choice(ENTITIES)
        target = f"target-{i}"  # unique per push so order is deterministic
        if len(capture[user]) < 5:
            capture[user].append(target)
        app.push("Click", {"user_id": user, "target": target})

    for entity, exp in capture.items():
        if not exp:
            continue
        result = app.get("UserFirstFive", entity)
        assert result["opens"] == exp, (
            f"{entity}: expected first_5={exp!r}, got {result['opens']!r}"
        )

    assert cold_start_equivalent(app.get("UserFirstFive", "unknown_n5"))


# ---------------------------------------------------------------------------
# Test 4: last_n (n=5)
# ---------------------------------------------------------------------------


def test_last_n_per_user_high_volume(app):
    """bv.last_n (n=5): 500 events / 5 users; trailing-5 ring buffer per entity."""

    @bv.event
    class Touch:
        user_id: str
        action: str

    @bv.table(key="user_id")
    def UserLastFive(touches: Touch):
        return touches.group_by("user_id").agg(
            recent=bv.last_n("action", n=5),
        )

    app.register(Touch, UserLastFive)

    rng = random.Random(63)
    capture: dict[str, deque[str]] = {entity: deque(maxlen=5) for entity in ENTITIES}
    for i in range(500):
        user = rng.choice(ENTITIES)
        action = f"action-{i}"
        capture[user].append(action)
        app.push("Touch", {"user_id": user, "action": action})

    for entity, dq in capture.items():
        if not dq:
            continue
        exp = list(dq)
        result = app.get("UserLastFive", entity)
        # last_n returns oldest-to-newest in insertion order (Vec<Value> per docs).
        assert result["recent"] == exp, (
            f"{entity}: expected last_5={exp!r}, got {result['recent']!r}"
        )

    assert cold_start_equivalent(app.get("UserLastFive", "unknown_n5l"))


# ---------------------------------------------------------------------------
# Test 5: lag (n=1 and n=3 in sub-cases)
# ---------------------------------------------------------------------------


def test_lag_per_user_high_volume(app):
    """bv.lag (n=1, n=3): 500 events / 5 users; previous-value lookup per entity."""

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserLagged(txs: Tx):
        return txs.group_by("user_id").agg(
            prev_amt=bv.lag("amount", n=1),
            prev_3_amt=bv.lag("amount", n=3),
        )

    app.register(Tx, UserLagged)

    rng = random.Random(64)
    history: dict[str, list[float]] = {entity: [] for entity in ENTITIES}
    for _ in range(500):
        user = rng.choice(ENTITIES)
        amount = rng.uniform(1.0, 1000.0)
        history[user].append(amount)
        app.push("Tx", {"user_id": user, "amount": amount})

    for entity, hist in history.items():
        if len(hist) < 4:
            # Need at least 4 events to populate both n=1 and n=3 lags.
            continue
        # n=1: value 1 event ago = hist[-2]; n=3: value 3 events ago = hist[-4].
        exp_n1 = hist[-2]
        exp_n3 = hist[-4]
        result = app.get("UserLagged", entity)
        assert abs(result["prev_amt"] - exp_n1) < 1e-9, (
            f"{entity}: expected lag(n=1)={exp_n1}, got {result['prev_amt']}"
        )
        assert abs(result["prev_3_amt"] - exp_n3) < 1e-9, (
            f"{entity}: expected lag(n=3)={exp_n3}, got {result['prev_3_amt']}"
        )

    assert cold_start_equivalent(app.get("UserLagged", "unknown_lag"))
