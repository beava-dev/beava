"""Plan 13.5.1-07a — operator-catalogue signature reconciliation.

Per-family unit-test contract for the 16 helpers in `python/beava/_agg.py`
that drift from the v0 acceptance test call shapes (Plan 05 deficit
Category 1) and from `docs/operators/<family>/<op>.md`. This suite is the
RED→GREEN gate per CLAUDE.md §Conventions §TDD Discipline item #1; the
v0 acceptance integration smoke remains the additive engine-dependent gate
per item #4.

Family layout mirrors the per-family table in 13.5.1-07a-PLAN.md:

- ``test_buffer_family``        — histogram, hour_of_day_histogram,
                                  dow_hour_histogram, seasonal_deviation,
                                  event_type_mix, reservoir_sample
- ``test_geo_family``           — geo_velocity, geo_distance, geo_spread,
                                  distance_from_home
- ``test_point_ordinal_family`` — first_n, last_n (where= parity)
- ``test_recency_family``       — streak, max_streak, negative_streak,
                                  has_seen, first_seen_in_window,
                                  time_since, time_since_last_n
- ``test_velocity_family``      — outlier_count (sigma rename),
                                  value_change_count (regression guard)
"""
from __future__ import annotations

import beava as bv

# ---------------------------------------------------------------------------
# Family A — Buffer
# ---------------------------------------------------------------------------


def test_buffer_family() -> None:
    """6 buffer ops match docs/operators/buffer-geo/<op>.md signature blocks."""

    # bv.histogram — buckets is list[float], no window=, lifetime-only
    h = bv.histogram("amount", buckets=[10.0, 50.0, 100.0, 500.0])
    d = h.to_dict()
    assert d["op"] == "histogram"
    assert d["field"] == "amount"
    assert d["buckets"] == [10.0, 50.0, 100.0, 500.0]
    assert "window" not in d, f"histogram is lifetime-only; got window={d.get('window')!r}"

    # bv.hour_of_day_histogram — no window=
    h = bv.hour_of_day_histogram()
    d = h.to_dict()
    assert d["op"] == "hour_of_day_histogram"
    assert "window" not in d

    # bv.dow_hour_histogram — no window=
    h = bv.dow_hour_histogram()
    d = h.to_dict()
    assert d["op"] == "dow_hour_histogram"
    assert "window" not in d

    # bv.seasonal_deviation — no window=
    h = bv.seasonal_deviation("amount")
    d = h.to_dict()
    assert d["op"] == "seasonal_deviation"
    assert d["field"] == "amount"
    assert "window" not in d

    # bv.event_type_mix — no window=, accepts categories + max_categories
    h = bv.event_type_mix("kind")
    d = h.to_dict()
    assert d["op"] == "event_type_mix"
    assert d["field"] == "kind"
    assert "window" not in d

    h = bv.event_type_mix("kind", categories=["p2p", "card"], max_categories=10)
    d = h.to_dict()
    assert d["categories"] == ["p2p", "card"]
    assert d["max_categories"] == 10

    # bv.reservoir_sample — samples= (NOT k=)
    h = bv.reservoir_sample("amount", samples=100)
    d = h.to_dict()
    assert d["op"] == "reservoir_sample"
    assert d["field"] == "amount"
    assert d["samples"] == 100, f"reservoir_sample expects samples=, got d={d!r}"
    assert "k" not in d

    # bv.reservoir_sample — accepts where= per docs parity
    h = bv.reservoir_sample("amount", samples=100, where=bv.col("kind") == "p2p")
    assert h.where is not None


# ---------------------------------------------------------------------------
# Family B — Geo
# ---------------------------------------------------------------------------


def test_geo_family() -> None:
    """4 geo ops match docs/operators/buffer-geo/geo_*.md signature blocks."""

    # bv.geo_velocity — kw-only lat=/lon=, no window=
    h = bv.geo_velocity(lat="latitude", lon="longitude")
    d = h.to_dict()
    assert d["op"] == "geo_velocity"
    assert d["lat_field"] == "latitude"
    assert d["lon_field"] == "longitude"
    assert "window" not in d

    # bv.geo_distance — kw-only lat=/lon=, no window=
    h = bv.geo_distance(lat="lat", lon="lon")
    d = h.to_dict()
    assert d["op"] == "geo_distance"
    assert d["lat_field"] == "lat"
    assert d["lon_field"] == "lon"
    assert "window" not in d

    # bv.geo_spread — kw-only lat=/lon=, no window=
    h = bv.geo_spread(lat="lat", lon="lon")
    d = h.to_dict()
    assert d["op"] == "geo_spread"
    assert d["lat_field"] == "lat"
    assert d["lon_field"] == "lon"
    assert "window" not in d

    # bv.distance_from_home — kw-only lat=/lon=/samples=, no window=
    h = bv.distance_from_home(lat="lat", lon="lon", samples=50)
    d = h.to_dict()
    assert d["op"] == "distance_from_home"
    assert d["lat_field"] == "lat"
    assert d["lon_field"] == "lon"
    assert d["samples"] == 50
    assert "window" not in d

    # bv.distance_from_home — samples defaults to 100
    h = bv.distance_from_home(lat="lat", lon="lon")
    d = h.to_dict()
    assert d["samples"] == 100


# ---------------------------------------------------------------------------
# Family C — Point/ordinal (where= parity)
# ---------------------------------------------------------------------------


def test_point_ordinal_family() -> None:
    """first_n / last_n: signature already matches; add where= per docs parity."""

    # Existing call shape — regression guard
    h = bv.first_n("target", n=5)
    d = h.to_dict()
    assert d["op"] == "first_n"
    assert d["field"] == "target"
    assert d["n"] == 5

    h = bv.last_n("action", n=5)
    d = h.to_dict()
    assert d["op"] == "last_n"
    assert d["n"] == 5

    # New: where= parity
    h = bv.first_n("target", n=5, where=bv.col("kind") == "click")
    assert h.where is not None

    h = bv.last_n("action", n=5, where=bv.col("kind") == "click")
    assert h.where is not None


# ---------------------------------------------------------------------------
# Family D — Recency
# ---------------------------------------------------------------------------


def test_recency_family() -> None:
    """7 recency ops match docs/operators/recency/<op>.md signature blocks."""

    # bv.streak — no field, accepts where=
    h = bv.streak(where=bv.col("outcome") == "win")
    d = h.to_dict()
    assert d["op"] == "streak"
    assert "field" not in d
    assert d["where"] is not None

    # bv.max_streak — no field, accepts where=
    h = bv.max_streak(where=bv.col("outcome") == "win")
    d = h.to_dict()
    assert d["op"] == "max_streak"
    assert "field" not in d
    assert d["where"] is not None

    # bv.negative_streak — no field, accepts where=
    h = bv.negative_streak(where=bv.col("result") == "fail")
    d = h.to_dict()
    assert d["op"] == "negative_streak"
    assert "field" not in d
    assert d["where"] is not None

    # bv.has_seen — no field, accepts where=
    h = bv.has_seen(where=bv.col("status") == "failed")
    d = h.to_dict()
    assert d["op"] == "has_seen"
    assert "field" not in d
    assert d["where"] is not None

    # bv.first_seen_in_window — no field, requires window=, accepts where=
    h = bv.first_seen_in_window(window="1h")
    d = h.to_dict()
    assert d["op"] == "first_seen_in_window"
    assert "field" not in d
    assert d["window"] == "1h"

    h = bv.first_seen_in_window(window="1h", where=bv.col("kind") == "click")
    assert h.where is not None

    # bv.time_since — accepts where= parity
    h = bv.time_since(where=bv.col("kind") == "alive")
    assert h.where is not None

    # bv.time_since_last_n — accepts where= parity
    h = bv.time_since_last_n(n=3, where=bv.col("kind") == "p")
    d = h.to_dict()
    assert d["n"] == 3
    assert h.where is not None


# ---------------------------------------------------------------------------
# Family E — Velocity
# ---------------------------------------------------------------------------


def test_velocity_family() -> None:
    """outlier_count.sigma rename + value_change_count regression guard."""

    # bv.outlier_count — sigma= (NOT threshold=)
    h = bv.outlier_count("x", window="1h", sigma=3.0)
    d = h.to_dict()
    assert d["op"] == "outlier_count"
    assert d["field"] == "x"
    assert d["window"] == "1h"
    assert d["sigma"] == 3.0, f"outlier_count expects sigma=, got d={d!r}"
    assert "threshold" not in d

    # default sigma=3.0
    h = bv.outlier_count("x", window="1h")
    d = h.to_dict()
    assert d["sigma"] == 3.0

    # bv.value_change_count — regression guard; signature already matches
    h = bv.value_change_count("state", window="1h")
    d = h.to_dict()
    assert d["op"] == "value_change_count"
    assert d["field"] == "state"
    assert d["window"] == "1h"
