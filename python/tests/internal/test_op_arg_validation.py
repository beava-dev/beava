"""SDK-side operator argument validation coverage.

Locks every explicit bounds check in ``python/beava/_agg.py`` (and the
``cast`` target check in ``python/beava/_col.py``). Each test pins both
the rejection AND the accepted boundary so silent drift in either
direction surfaces as a failure.

Source map (validations under test):
- ``_validate_window`` (``_agg.py:32-43``)
- ``_validate_half_life`` (``_agg.py:46-48``)
- ``_enforce_field_str`` (``_agg.py:51-73``)
- ``quantile`` ``q`` bounds (``_agg.py:216-217``) — flagged by audit
- ``top_k`` ``k`` bounds (``_agg.py:237-238``)
- ``first_n`` / ``last_n`` / ``lag`` / ``most_recent_n`` /
  ``time_since_last_n`` ``n`` bound (``_agg.py:288, 301, 313, 343, 668``)
- ``reservoir_sample`` ``samples`` bound (``_agg.py:686``)
- ``event_type_mix`` ``max_categories`` + ``categories`` shape
  (``_agg.py:645-656``)
- ``histogram`` ``buckets`` shape (``_agg.py:581-594``)
- ``distance_from_home`` ``samples`` bound (``_agg.py:735-738``)
- ``_Expr.cast`` target whitelist (``_col.py:102-106``)

For factories with NO explicit bounds (``count``, ``has_seen``,
``streak``, ``first_seen``, ``last_seen``, ``age``, etc.) one test pins
that they accept anything documented — preventing drift in the
permissive direction.
"""
from __future__ import annotations

import math

import pytest

import beava as bv
from beava._errors import RegistrationError


# ── quantile q bounds (_agg.py:216-217) ─────────────────────────────────
@pytest.mark.parametrize(
    "q",
    [0.0, 1.0, -0.5, 2.0, -1e-9, 1.0 + 1e-9],
    ids=["zero", "one", "neg_half", "two", "below_zero", "above_one"],
)
def test_quantile_q_out_of_open_unit_interval_raises(q: float) -> None:
    with pytest.raises(ValueError, match=r"quantile q must be in \(0, 1\)") as ei:
        bv.quantile("amount", q=q, window="1h")
    assert str(q) in str(ei.value) or repr(q) in str(ei.value)


@pytest.mark.parametrize("q", [0.001, 0.5, 0.99, 0.999999])
def test_quantile_q_in_open_unit_interval_accepted(q: float) -> None:
    d = bv.quantile("amount", q=q, window="1h").to_dict()
    assert d["op"] == "quantile"
    assert d["q"] == q


def test_quantile_q_missing_kwarg_raises_typeerror() -> None:
    """``q`` is required-kwarg; Python's signature enforces presence."""
    with pytest.raises(TypeError, match="q"):
        bv.quantile("amount", window="1h")  # type: ignore[call-arg]


def test_quantile_q_none_raises() -> None:
    """``q=None`` slips past the kwarg-required check and currently raises
    ``TypeError`` from the ``0.0 < q < 1.0`` comparison rather than a
    typed ``ValueError`` — gap flagged in commit message.
    """
    with pytest.raises((TypeError, ValueError)):
        bv.quantile("amount", q=None, window="1h")  # type: ignore[arg-type]


# ── top_k k bounds (_agg.py:237-238) ────────────────────────────────────
@pytest.mark.parametrize("k", [0, -1, -100])
def test_top_k_below_one_raises(k: int) -> None:
    with pytest.raises(ValueError, match=r"top_k k must be >= 1") as ei:
        bv.top_k("merchant", k=k, window="1h")
    assert str(k) in str(ei.value)


@pytest.mark.parametrize("k", [1, 10, 1000])
def test_top_k_one_or_more_accepted(k: int) -> None:
    d = bv.top_k("merchant", k=k, window="1h").to_dict()
    assert d["op"] == "top_k"
    assert d["k"] == k


# ── n-bounded ops (>= 1) ────────────────────────────────────────────────
@pytest.mark.parametrize(
    "factory, op_name",
    [
        (lambda n: bv.first_n("x", n=n), "first_n"),
        (lambda n: bv.last_n("x", n=n), "last_n"),
        (lambda n: bv.lag("x", n=n), "lag"),
        (lambda n: bv.most_recent_n("x", n=n), "most_recent_n"),
        (lambda n: bv.time_since_last_n(n=n), "time_since_last_n"),
    ],
)
@pytest.mark.parametrize("n", [0, -1, -100])
def test_n_param_below_one_raises(factory, op_name: str, n: int) -> None:
    with pytest.raises(ValueError, match=rf"{op_name} n must be >= 1") as ei:
        factory(n)
    assert str(n) in str(ei.value)


@pytest.mark.parametrize(
    "factory",
    [
        lambda n: bv.first_n("x", n=n),
        lambda n: bv.last_n("x", n=n),
        lambda n: bv.lag("x", n=n),
        lambda n: bv.most_recent_n("x", n=n),
        lambda n: bv.time_since_last_n(n=n),
    ],
)
@pytest.mark.parametrize("n", [1, 5, 1000])
def test_n_param_one_or_more_accepted(factory, n: int) -> None:
    d = factory(n).to_dict()
    assert d["n"] == n


def test_lag_default_n_is_one() -> None:
    d = bv.lag("x").to_dict()
    assert d["n"] == 1


# ── reservoir_sample samples >= 1 (_agg.py:686) ─────────────────────────
@pytest.mark.parametrize("samples", [0, -1])
def test_reservoir_sample_below_one_raises(samples: int) -> None:
    with pytest.raises(ValueError, match=r"reservoir_sample samples must be >= 1") as ei:
        bv.reservoir_sample("x", samples=samples)
    assert str(samples) in str(ei.value)


def test_reservoir_sample_valid() -> None:
    d = bv.reservoir_sample("x", samples=128).to_dict()
    assert d["samples"] == 128


# ── distance_from_home samples >= 1 (_agg.py:735-738) ───────────────────
@pytest.mark.parametrize("samples", [0, -1])
def test_distance_from_home_below_one_raises(samples: int) -> None:
    with pytest.raises(ValueError, match=r"distance_from_home samples must be >= 1") as ei:
        bv.distance_from_home(lat="lat", lon="lon", samples=samples)
    assert str(samples) in str(ei.value)


def test_distance_from_home_default_samples() -> None:
    d = bv.distance_from_home(lat="lat", lon="lon").to_dict()
    assert d["samples"] == 100


# ── event_type_mix max_categories + categories (_agg.py:645-656) ────────
@pytest.mark.parametrize("mc", [0, -1])
def test_event_type_mix_max_categories_below_one_raises(mc: int) -> None:
    with pytest.raises(
        ValueError, match=r"event_type_mix max_categories must be >= 1"
    ) as ei:
        bv.event_type_mix("kind", max_categories=mc)
    assert str(mc) in str(ei.value)


def test_event_type_mix_categories_non_list_raises() -> None:
    with pytest.raises(ValueError, match=r"event_type_mix categories must be list\[str\]"):
        bv.event_type_mix("kind", categories="not_a_list")  # type: ignore[arg-type]


def test_event_type_mix_categories_non_string_entry_raises() -> None:
    with pytest.raises(ValueError, match=r"event_type_mix categories must be list\[str\]") as ei:
        bv.event_type_mix("kind", categories=["ok", 42])  # type: ignore[list-item]
    # bad value embedded in repr
    assert "42" in str(ei.value)


def test_event_type_mix_valid_with_allowlist() -> None:
    d = bv.event_type_mix("kind", categories=["a", "b", "c"]).to_dict()
    assert d["categories"] == ["a", "b", "c"]
    assert d["max_categories"] == 256


# ── histogram buckets (_agg.py:581-594) ─────────────────────────────────
def test_histogram_empty_buckets_raises() -> None:
    with pytest.raises(ValueError, match=r"histogram buckets must be a non-empty list"):
        bv.histogram("x", buckets=[])


def test_histogram_non_list_raises() -> None:
    with pytest.raises(ValueError, match=r"histogram buckets must be a non-empty list"):
        bv.histogram("x", buckets=(1.0, 2.0))  # type: ignore[arg-type]


def test_histogram_non_numeric_entry_raises() -> None:
    with pytest.raises(ValueError, match=r"histogram buckets entries must be numeric") as ei:
        bv.histogram("x", buckets=[1.0, "two", 3.0])  # type: ignore[list-item]
    assert "two" in str(ei.value)


@pytest.mark.parametrize(
    "buckets",
    [
        [1.0, 1.0, 2.0],     # equal neighbors
        [3.0, 2.0, 1.0],     # decreasing
        [1.0, 5.0, 2.0],     # non-monotonic
    ],
)
def test_histogram_non_strictly_increasing_raises(buckets) -> None:
    with pytest.raises(ValueError, match=r"histogram buckets must be strictly increasing"):
        bv.histogram("x", buckets=buckets)


def test_histogram_single_element_accepted() -> None:
    """Single-element list satisfies ``len >= 1`` and the strict-increase
    loop (which only runs from index 1). Pinned for stability."""
    d = bv.histogram("x", buckets=[42.0]).to_dict()
    assert d["buckets"] == [42.0]


def test_histogram_valid_strictly_increasing() -> None:
    d = bv.histogram("x", buckets=[0.0, 1.0, 10.0, 100.0]).to_dict()
    assert d["op"] == "histogram"
    assert d["buckets"] == [0.0, 1.0, 10.0, 100.0]


def test_histogram_buckets_with_nan_currently_accepted_GAP() -> None:
    """VALIDATION GAP: ``isinstance(nan, float)`` is True and
    ``nan <= 1.0`` is False, so a NaN bucket sneaks through the
    strict-increase check at ``_agg.py:590-594``. Pinning the current
    permissive behavior — when fixed, flip this test to ``raises``.
    """
    d = bv.histogram("x", buckets=[1.0, float("nan"), 3.0]).to_dict()
    assert any(isinstance(b, float) and math.isnan(b) for b in d["buckets"])


# ── window arg (_validate_window, _agg.py:32-43) ────────────────────────
@pytest.mark.parametrize(
    "bad",
    ["1x", "abc", "", "1", "h", "1.5h", "1 h", "FOREVER", "1H"],
)
def test_window_invalid_format_raises(bad: str) -> None:
    with pytest.raises(ValueError, match=r"invalid window") as ei:
        bv.sum("x", window=bad)
    assert repr(bad) in str(ei.value)


@pytest.mark.parametrize(
    "good",
    ["1ms", "1s", "30s", "10m", "1h", "24h", "7d", "100ms", "forever"],
)
def test_window_valid_format_accepted(good: str) -> None:
    d = bv.sum("x", window=good).to_dict()
    assert d["window"] == good


def test_window_none_optional_op_accepted() -> None:
    """``count``, ``sum``, ``mean``, ... mark window as optional."""
    d = bv.sum("x").to_dict()
    assert "window" not in d


@pytest.mark.parametrize(
    "factory",
    [
        lambda: bv.first_seen_in_window(window=None),  # type: ignore[arg-type]
        lambda: bv.twa("x", window=None),  # type: ignore[arg-type]
        lambda: bv.rate_of_change("x", window=None),  # type: ignore[arg-type]
        lambda: bv.inter_arrival_stats(window=None),  # type: ignore[arg-type]
        lambda: bv.trend("x", window=None),  # type: ignore[arg-type]
        lambda: bv.trend_residual("x", window=None),  # type: ignore[arg-type]
        lambda: bv.outlier_count("x", window=None),  # type: ignore[arg-type]
        lambda: bv.value_change_count("x", window=None),  # type: ignore[arg-type]
        lambda: bv.z_score("x", baseline_window=None),  # type: ignore[arg-type]
        lambda: bv.burst_count(window=None, sub_window="10s"),  # type: ignore[arg-type]
    ],
)
def test_window_required_ops_reject_none(factory) -> None:
    with pytest.raises(ValueError, match=r"requires a window"):
        factory()


def test_burst_count_sub_window_validated() -> None:
    with pytest.raises(ValueError, match=r"burst_count\.sub_window: invalid window"):
        bv.burst_count(window="1h", sub_window="bogus")


# ── half_life arg (_validate_half_life, _agg.py:46-48) ──────────────────
@pytest.mark.parametrize(
    "factory",
    [
        lambda hl: bv.ewma("x", half_life=hl),
        lambda hl: bv.ema("x", half_life=hl),
        lambda hl: bv.ewvar("x", half_life=hl),
        lambda hl: bv.ew_zscore("x", half_life=hl),
        lambda hl: bv.decayed_sum("x", half_life=hl),
        lambda hl: bv.decayed_count(half_life=hl),
    ],
)
@pytest.mark.parametrize("bad", ["1x", "abc", "", "1H", "1.5h"])
def test_half_life_invalid_format_raises(factory, bad: str) -> None:
    with pytest.raises(ValueError, match=r"invalid half_life") as ei:
        factory(bad)
    assert repr(bad) in str(ei.value)


@pytest.mark.parametrize("good", ["1ms", "30s", "5m", "1h", "7d"])
def test_half_life_valid_accepted(good: str) -> None:
    d = bv.ewma("x", half_life=good).to_dict()
    assert d["half_life"] == good


# ── field arg (_enforce_field_str, _agg.py:51-73) ───────────────────────
def test_field_expr_raises_RegistrationError() -> None:
    with pytest.raises(RegistrationError, match=r"schema_mismatch|expression"):
        bv.sum(bv.col("amount") * 2, window="1h")  # type: ignore[arg-type]


@pytest.mark.parametrize(
    "bad",
    [None, 42, 3.14, ["amount"], {"amount": 1}, True, b"amount"],
)
def test_field_non_string_raises_RegistrationError(bad) -> None:
    with pytest.raises(RegistrationError) as ei:
        bv.sum(bad, window="1h")  # type: ignore[arg-type]
    # Error mentions the offending Python type.
    assert type(bad).__name__ in str(ei.value.message)


def test_field_string_accepted() -> None:
    d = bv.sum("amount", window="1h").to_dict()
    assert d["field"] == "amount"


def test_field_empty_string_currently_accepted_GAP() -> None:
    """VALIDATION GAP: ``_enforce_field_str`` checks ``isinstance(_, str)``
    but never checks ``len > 0``. Empty-string field names sail through
    the SDK and only fail at the server. Pinning current behavior — when
    the SDK rejects empties, flip this to ``raises``.
    """
    d = bv.sum("", window="1h").to_dict()
    assert d["field"] == ""


# ── outlier_count sigma has no SDK validation — GAP ─────────────────────
def test_outlier_count_negative_sigma_currently_accepted_GAP() -> None:
    """VALIDATION GAP: ``outlier_count`` documents a ±sigma band but does
    not enforce ``sigma > 0`` at the SDK layer. Negative / zero sigma is
    silently forwarded to the server. Pinning current behavior."""
    d = bv.outlier_count("x", window="1h", sigma=-1.0).to_dict()
    assert d["sigma"] == -1.0
    d = bv.outlier_count("x", window="1h", sigma=0.0).to_dict()
    assert d["sigma"] == 0.0


# ── cast target whitelist (_col.py:102-106) ─────────────────────────────
@pytest.mark.parametrize("bad", ["blob", "bytes", "STR", "int64", "Float", "", "list"])
def test_cast_invalid_target_raises(bad: str) -> None:
    with pytest.raises(ValueError, match=r"cast target must be one of") as ei:
        bv.col("x").cast(bad)
    assert repr(bad) in str(ei.value)


@pytest.mark.parametrize("good", ["str", "int", "float", "bool"])
def test_cast_valid_target_accepted(good: str) -> None:
    expr = bv.col("x").cast(good)
    assert expr.to_expr_string() == f"cast(x, {good})"


# ── No-validation factories (contract-lock) ─────────────────────────────
# Locks the permissive contract — if a future change introduces validation
# for any of these, the test breaks deliberately so the contract change is
# visible.
def test_count_has_no_field_arg_and_window_optional() -> None:
    d = bv.count().to_dict()
    assert d == {"op": "count"}


def test_has_seen_no_validation() -> None:
    assert bv.has_seen().to_dict() == {"op": "has_seen"}


def test_streak_no_validation() -> None:
    assert bv.streak().to_dict() == {"op": "streak"}


def test_max_streak_no_validation() -> None:
    assert bv.max_streak().to_dict() == {"op": "max_streak"}


def test_negative_streak_no_validation() -> None:
    assert bv.negative_streak().to_dict() == {"op": "negative_streak"}


def test_first_seen_no_validation() -> None:
    assert bv.first_seen().to_dict() == {"op": "first_seen"}


def test_last_seen_no_validation() -> None:
    assert bv.last_seen().to_dict() == {"op": "last_seen"}


def test_age_no_validation() -> None:
    assert bv.age().to_dict() == {"op": "age"}


def test_time_since_no_validation() -> None:
    assert bv.time_since().to_dict() == {"op": "time_since"}


def test_hour_of_day_histogram_no_validation() -> None:
    assert bv.hour_of_day_histogram().to_dict() == {"op": "hour_of_day_histogram"}


def test_dow_hour_histogram_no_validation() -> None:
    assert bv.dow_hour_histogram().to_dict() == {"op": "dow_hour_histogram"}


def test_geo_velocity_no_lat_lon_validation() -> None:
    """``geo_*`` factories accept any string for lat/lon without checking
    schema membership — server validates at register-time."""
    d = bv.geo_velocity(lat="", lon="").to_dict()
    assert d["lat_field"] == ""
    assert d["lon_field"] == ""
