"""Aggregation op helpers.

Each helper returns an :class:`AggDescriptor` whose ``to_dict()`` emits
wire-shape JSON consumed by the server's register-time compiler. Names
follow Polars conventions (``mean`` / ``var`` / ``std`` / ``n_unique`` /
``quantile``); the SQL-prose names (``avg`` / ``variance`` / ``stddev`` /
``count_distinct`` / ``percentile``) are deprecation aliases that emit
``DeprecationWarning``.

Field-bearing ops require a string column name. Passing an ``_Expr`` (e.g.
``bv.col("x") * 2``) raises ``RegistrationError(code='schema_mismatch')``;
use a two-stage chain instead::

    events.with_columns(doubled=bv.col("x") * 2).group_by(...).agg(
        s=bv.sum("doubled", window="1h"),
    )
"""
from __future__ import annotations

import math
import re
import warnings
from dataclasses import dataclass
from dataclasses import field as _dc_field
from typing import Any

from beava._col import _Expr
from beava._errors import RegistrationError

_WINDOW_PATTERN = re.compile(r"^\d+(ms|s|m|h|d)$|^forever$")


def _validate_window(
    window: str | None, op: str, *, required: bool = True
) -> None:
    if window is None:
        if required:
            raise ValueError(f"{op} requires a window= kwarg")
        return
    if not _WINDOW_PATTERN.match(window):
        raise ValueError(
            f"{op}: invalid window {window!r} — must match \\d+(ms|s|m|h|d) "
            f"or 'forever'"
        )


def _validate_half_life(half_life: str, op: str) -> None:
    if not _WINDOW_PATTERN.match(half_life):
        raise ValueError(f"{op}: invalid half_life {half_life!r}")


def _enforce_field_str(field_arg: Any, op: str) -> str:
    """The ``field`` arg must be a string column name, not an ``_Expr``."""
    if isinstance(field_arg, _Expr):
        raise RegistrationError(
            code="schema_mismatch",
            path=op,
            message=(
                f"bv.{op}(field=...) requires a string column name, not an "
                f"expression. Use a two-stage chain: "
                f"events.with_columns(<derived>=<expr>).group_by(...).agg("
                f"{op}=bv.{op}('<derived>', ...)) — see "
                f"docs/sdk-api/python.md § bv.sum signature."
            ),
            errors=[],
        )
    if not isinstance(field_arg, str):
        raise RegistrationError(
            code="schema_mismatch",
            path=op,
            message=f"bv.{op}(field) must be a string; got {type(field_arg).__name__}",
            errors=[],
        )
    if len(field_arg) == 0:
        raise RegistrationError(
            code="schema_mismatch",
            path=op,
            message=f"bv.{op}(field) must be a non-empty string column name; got ''",
            errors=[],
        )
    return field_arg


def _serialize_where(where: Any) -> str | None:
    if where is None:
        return None
    if isinstance(where, _Expr):
        return where.to_expr_string()
    raise TypeError(
        f"where= must be an _Expr or None; got {type(where).__name__}"
    )


@dataclass(frozen=True)
class AggDescriptor:
    """The artifact returned by every op helper.

    ``to_dict()`` renders to wire-shape JSON. The ``extras`` dict carries
    op-specific kwargs (``q``, ``k``, ``n``, ``threshold``,
    ``baseline_window``, ``sub_window``, ``buckets``, ``lat_field``,
    ``lon_field``) so each helper need not subclass.
    """

    op: str
    field: str | None = None
    window: str | None = None
    half_life: str | None = None
    extras: dict[str, Any] = _dc_field(default_factory=dict)
    where: str | None = None

    def to_dict(self) -> dict[str, Any]:
        d: dict[str, Any] = {"op": self.op}
        if self.field is not None:
            d["field"] = self.field
        if self.window is not None:
            d["window"] = self.window
        if self.half_life is not None:
            d["half_life"] = self.half_life
        if self.where is not None:
            d["where"] = self.where
        d.update(self.extras)
        return d


def count(*, window: str | None = None, where: Any = None) -> AggDescriptor:
    """Count of events in window."""
    _validate_window(window, "count", required=False)
    return AggDescriptor(op="count", window=window, where=_serialize_where(where))


def sum(  # noqa: A001 — shadows builtin intentionally per docs/sdk-api/python.md
    field: Any, *, window: str | None = None, where: Any = None
) -> AggDescriptor:
    """Sum of ``field`` over window. Q1 Path B: ``field`` must be a column-name string."""
    _enforce_field_str(field, "sum")
    _validate_window(window, "sum", required=False)
    return AggDescriptor(
        op="sum", field=field, window=window, where=_serialize_where(where)
    )


def mean(
    field: Any, *, window: str | None = None, where: Any = None
) -> AggDescriptor:
    """Polars-style ``mean`` (ADR-002 rename of ``avg``)."""
    _enforce_field_str(field, "mean")
    _validate_window(window, "mean", required=False)
    return AggDescriptor(
        op="mean", field=field, window=window, where=_serialize_where(where)
    )


def min(  # noqa: A001
    field: Any, *, window: str | None = None, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "min")
    _validate_window(window, "min", required=False)
    return AggDescriptor(
        op="min", field=field, window=window, where=_serialize_where(where)
    )


def max(  # noqa: A001
    field: Any, *, window: str | None = None, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "max")
    _validate_window(window, "max", required=False)
    return AggDescriptor(
        op="max", field=field, window=window, where=_serialize_where(where)
    )


def var(
    field: Any, *, window: str | None = None, where: Any = None
) -> AggDescriptor:
    """Polars-style ``var`` (ADR-002 rename of ``variance``)."""
    _enforce_field_str(field, "var")
    _validate_window(window, "var", required=False)
    return AggDescriptor(
        op="var", field=field, window=window, where=_serialize_where(where)
    )


def std(
    field: Any, *, window: str | None = None, where: Any = None
) -> AggDescriptor:
    """Polars-style ``std`` (ADR-002 rename of ``stddev``)."""
    _enforce_field_str(field, "std")
    _validate_window(window, "std", required=False)
    return AggDescriptor(
        op="std", field=field, window=window, where=_serialize_where(where)
    )


def ratio(*, window: str | None = None, where: Any = None) -> AggDescriptor:
    """Server computes ratio = matched / total over window; ``where`` filters numerator."""
    _validate_window(window, "ratio", required=False)
    return AggDescriptor(op="ratio", window=window, where=_serialize_where(where))


def n_unique(
    field: Any, *, window: str | None = None, where: Any = None
) -> AggDescriptor:
    """Polars-style ``n_unique`` (ADR-002 rename of ``count_distinct``)."""
    _enforce_field_str(field, "n_unique")
    _validate_window(window, "n_unique", required=False)
    return AggDescriptor(
        op="n_unique", field=field, window=window, where=_serialize_where(where)
    )


def quantile(
    field: Any,
    *,
    q: float,
    window: str | None = None,
    where: Any = None,
) -> AggDescriptor:
    """Polars-style ``quantile`` (ADR-002 rename of ``percentile``).

    ``q`` is in the open interval ``(0, 1)``.
    """
    _enforce_field_str(field, "quantile")
    if q is None:
        raise ValueError("quantile q must be in (0, 1); got None")
    if not (0.0 < q < 1.0):
        raise ValueError(f"quantile q must be in (0, 1); got {q}")
    _validate_window(window, "quantile", required=False)
    return AggDescriptor(
        op="quantile",
        field=field,
        window=window,
        extras={"q": q},
        where=_serialize_where(where),
    )


def top_k(
    field: Any,
    *,
    k: int,
    window: str | None = None,
    where: Any = None,
) -> AggDescriptor:
    """Top-k most frequent values (server-side count-min sketch + heap)."""
    _enforce_field_str(field, "top_k")
    if k < 1:
        raise ValueError(f"top_k k must be >= 1; got {k}")
    _validate_window(window, "top_k", required=False)
    return AggDescriptor(
        op="top_k",
        field=field,
        window=window,
        extras={"k": k},
        where=_serialize_where(where),
    )


def bloom_member(
    field: Any, *, window: str | None = None, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "bloom_member")
    _validate_window(window, "bloom_member", required=False)
    return AggDescriptor(
        op="bloom_member",
        field=field,
        window=window,
        where=_serialize_where(where),
    )


def entropy(
    field: Any, *, window: str | None = None, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "entropy")
    _validate_window(window, "entropy", required=False)
    return AggDescriptor(
        op="entropy",
        field=field,
        window=window,
        where=_serialize_where(where),
    )


def first(field: Any) -> AggDescriptor:
    _enforce_field_str(field, "first")
    return AggDescriptor(op="first", field=field)


def last(field: Any) -> AggDescriptor:
    _enforce_field_str(field, "last")
    return AggDescriptor(op="last", field=field)


def first_n(field: Any, *, n: int, where: Any = None) -> AggDescriptor:
    """First N matching values in insertion order."""
    _enforce_field_str(field, "first_n")
    if n < 1:
        raise ValueError(f"first_n n must be >= 1; got {n}")
    return AggDescriptor(
        op="first_n",
        field=field,
        extras={"n": n},
        where=_serialize_where(where),
    )


def last_n(field: Any, *, n: int, where: Any = None) -> AggDescriptor:
    """Last N matching values, oldest-to-newest."""
    _enforce_field_str(field, "last_n")
    if n < 1:
        raise ValueError(f"last_n n must be >= 1; got {n}")
    return AggDescriptor(
        op="last_n",
        field=field,
        extras={"n": n},
        where=_serialize_where(where),
    )


def lag(field: Any, *, n: int = 1) -> AggDescriptor:
    _enforce_field_str(field, "lag")
    if n < 1:
        raise ValueError(f"lag n must be >= 1; got {n}")
    return AggDescriptor(op="lag", field=field, extras={"n": n})


def first_seen() -> AggDescriptor:
    return AggDescriptor(op="first_seen")


def last_seen() -> AggDescriptor:
    return AggDescriptor(op="last_seen")


def age() -> AggDescriptor:
    return AggDescriptor(op="age")


def has_seen(*, where: Any = None) -> AggDescriptor:
    """Boolean ever-matched flag (no field arg)."""
    return AggDescriptor(op="has_seen", where=_serialize_where(where))


def time_since(*, where: Any = None) -> AggDescriptor:
    """Query-time elapsed ms since the last matching event."""
    return AggDescriptor(op="time_since", where=_serialize_where(where))


def time_since_last_n(*, n: int, where: Any = None) -> AggDescriptor:
    """Silence relative to the nth-most-recent match."""
    if n < 1:
        raise ValueError(f"time_since_last_n n must be >= 1; got {n}")
    return AggDescriptor(
        op="time_since_last_n",
        extras={"n": n},
        where=_serialize_where(where),
    )


def streak(*, where: Any = None) -> AggDescriptor:
    """Current consecutive matching count (no field arg)."""
    return AggDescriptor(op="streak", where=_serialize_where(where))


def max_streak(*, where: Any = None) -> AggDescriptor:
    """All-time peak match streak (no field arg)."""
    return AggDescriptor(op="max_streak", where=_serialize_where(where))


def negative_streak(*, where: Any = None) -> AggDescriptor:
    """Consecutive non-matching count (no field arg)."""
    return AggDescriptor(op="negative_streak", where=_serialize_where(where))


def first_seen_in_window(*, window: str, where: Any = None) -> AggDescriptor:
    """Was the entity active within the past ``window``?"""
    _validate_window(window, "first_seen_in_window", required=True)
    return AggDescriptor(
        op="first_seen_in_window",
        window=window,
        where=_serialize_where(where),
    )


def ewma(
    field: Any, *, half_life: str, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "ewma")
    _validate_half_life(half_life, "ewma")
    return AggDescriptor(
        op="ewma",
        field=field,
        half_life=half_life,
        where=_serialize_where(where),
    )


def ema(
    field: Any, *, half_life: str, where: Any = None
) -> AggDescriptor:
    """Alias of :func:`ewma`."""
    return ewma(field, half_life=half_life, where=where)


def ewvar(
    field: Any, *, half_life: str, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "ewvar")
    _validate_half_life(half_life, "ewvar")
    return AggDescriptor(
        op="ewvar",
        field=field,
        half_life=half_life,
        where=_serialize_where(where),
    )


def ew_zscore(
    field: Any, *, half_life: str, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "ew_zscore")
    _validate_half_life(half_life, "ew_zscore")
    return AggDescriptor(
        op="ew_zscore",
        field=field,
        half_life=half_life,
        where=_serialize_where(where),
    )


def decayed_sum(
    field: Any, *, half_life: str, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "decayed_sum")
    _validate_half_life(half_life, "decayed_sum")
    return AggDescriptor(
        op="decayed_sum",
        field=field,
        half_life=half_life,
        where=_serialize_where(where),
    )


def decayed_count(*, half_life: str, where: Any = None) -> AggDescriptor:
    _validate_half_life(half_life, "decayed_count")
    return AggDescriptor(
        op="decayed_count",
        half_life=half_life,
        where=_serialize_where(where),
    )


def twa(field: Any, *, window: str, where: Any = None) -> AggDescriptor:
    _enforce_field_str(field, "twa")
    _validate_window(window, "twa", required=True)
    return AggDescriptor(
        op="twa",
        field=field,
        window=window,
        where=_serialize_where(where),
    )


def rate_of_change(
    field: Any, *, window: str, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "rate_of_change")
    _validate_window(window, "rate_of_change", required=True)
    return AggDescriptor(
        op="rate_of_change",
        field=field,
        window=window,
        where=_serialize_where(where),
    )


def inter_arrival_stats(*, window: str, where: Any = None) -> AggDescriptor:
    _validate_window(window, "inter_arrival_stats", required=True)
    return AggDescriptor(
        op="inter_arrival_stats",
        window=window,
        where=_serialize_where(where),
    )


def burst_count(
    *, window: str, sub_window: str, where: Any = None
) -> AggDescriptor:
    _validate_window(window, "burst_count", required=True)
    _validate_window(sub_window, "burst_count.sub_window", required=True)
    return AggDescriptor(
        op="burst_count",
        window=window,
        extras={"sub_window": sub_window},
        where=_serialize_where(where),
    )


def delta_from_prev(field: Any, *, where: Any = None) -> AggDescriptor:
    _enforce_field_str(field, "delta_from_prev")
    return AggDescriptor(
        op="delta_from_prev",
        field=field,
        where=_serialize_where(where),
    )


def trend(field: Any, *, window: str, where: Any = None) -> AggDescriptor:
    _enforce_field_str(field, "trend")
    _validate_window(window, "trend", required=True)
    return AggDescriptor(
        op="trend",
        field=field,
        window=window,
        where=_serialize_where(where),
    )


def trend_residual(
    field: Any, *, window: str, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "trend_residual")
    _validate_window(window, "trend_residual", required=True)
    return AggDescriptor(
        op="trend_residual",
        field=field,
        window=window,
        where=_serialize_where(where),
    )


def outlier_count(
    field: Any,
    *,
    window: str,
    sigma: float = 3.0,
    where: Any = None,
) -> AggDescriptor:
    """Count of events outside the ±sigma·stddev band."""
    if sigma <= 0:
        raise ValueError(
            f"outlier_count sigma must be > 0; got {sigma}"
        )
    _enforce_field_str(field, "outlier_count")
    _validate_window(window, "outlier_count", required=True)
    return AggDescriptor(
        op="outlier_count",
        field=field,
        window=window,
        extras={"sigma": sigma},
        where=_serialize_where(where),
    )


def value_change_count(
    field: Any, *, window: str, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "value_change_count")
    _validate_window(window, "value_change_count", required=True)
    return AggDescriptor(
        op="value_change_count",
        field=field,
        window=window,
        where=_serialize_where(where),
    )


def z_score(
    field: Any, *, baseline_window: str, where: Any = None
) -> AggDescriptor:
    _enforce_field_str(field, "z_score")
    _validate_window(baseline_window, "z_score.baseline_window", required=True)
    return AggDescriptor(
        op="z_score",
        field=field,
        extras={"baseline_window": baseline_window},
        where=_serialize_where(where),
    )


def histogram(
    field: Any,
    *,
    buckets: list[float],
    where: Any = None,
) -> AggDescriptor:
    """Lifetime-only fixed-bucket count histogram.

    ``buckets`` is a strictly-increasing list of split points; no ``window=``
    kwarg in v0 (the operator's bound is the required ``buckets`` kwarg
    itself, enforced by the memory-governance contract).
    """
    _enforce_field_str(field, "histogram")
    if not isinstance(buckets, list) or len(buckets) < 1:
        raise ValueError(
            f"histogram buckets must be a non-empty list[float]; got {buckets!r}"
        )
    for b in buckets:
        if not isinstance(b, (int, float)):
            raise ValueError(
                f"histogram buckets entries must be numeric; got {b!r}"
            )
        if isinstance(b, float) and math.isnan(b):
            raise ValueError(
                f"histogram buckets entries must not be NaN; got {buckets!r}"
            )
    for i in range(1, len(buckets)):
        if buckets[i] <= buckets[i - 1]:
            raise ValueError(
                f"histogram buckets must be strictly increasing; got {buckets!r}"
            )
    return AggDescriptor(
        op="histogram",
        field=field,
        extras={"buckets": list(buckets)},
        where=_serialize_where(where),
    )


def hour_of_day_histogram(*, where: Any = None) -> AggDescriptor:
    """Lifetime-only 24-bucket per-hour count."""
    return AggDescriptor(
        op="hour_of_day_histogram",
        where=_serialize_where(where),
    )


def dow_hour_histogram(*, where: Any = None) -> AggDescriptor:
    """Lifetime-only 168-bucket per-(day-of-week, hour) count."""
    return AggDescriptor(
        op="dow_hour_histogram",
        where=_serialize_where(where),
    )


def seasonal_deviation(
    field: Any, *, where: Any = None
) -> AggDescriptor:
    """Lifetime-only z-score versus hour-of-day baseline."""
    _enforce_field_str(field, "seasonal_deviation")
    return AggDescriptor(
        op="seasonal_deviation",
        field=field,
        where=_serialize_where(where),
    )


def event_type_mix(
    field: Any,
    *,
    categories: list[str] | None = None,
    max_categories: int = 256,
    where: Any = None,
) -> AggDescriptor:
    """Lifetime-only proportion-per-category sketch.

    Bounded by ``max_categories`` (default 256). When ``categories`` is
    explicitly set, the allowlist takes precedence and the cap-and-drop
    path is unreachable.
    """
    _enforce_field_str(field, "event_type_mix")
    if max_categories < 1:
        raise ValueError(
            f"event_type_mix max_categories must be >= 1; got {max_categories}"
        )
    extras: dict[str, Any] = {"max_categories": max_categories}
    if categories is not None:
        if not isinstance(categories, list) or not all(
            isinstance(c, str) for c in categories
        ):
            raise ValueError(
                f"event_type_mix categories must be list[str] or None; got {categories!r}"
            )
        extras["categories"] = list(categories)
    return AggDescriptor(
        op="event_type_mix",
        field=field,
        extras=extras,
        where=_serialize_where(where),
    )


def most_recent_n(field: Any, *, n: int, where: Any = None) -> AggDescriptor:
    _enforce_field_str(field, "most_recent_n")
    if n < 1:
        raise ValueError(f"most_recent_n n must be >= 1; got {n}")
    return AggDescriptor(
        op="most_recent_n",
        field=field,
        extras={"n": n},
        where=_serialize_where(where),
    )


def reservoir_sample(
    field: Any, *, samples: int, where: Any = None
) -> AggDescriptor:
    """Lifetime-only Vitter Algorithm R reservoir sample.

    ``samples`` is the required bound on retained values.
    """
    _enforce_field_str(field, "reservoir_sample")
    if samples < 1:
        raise ValueError(f"reservoir_sample samples must be >= 1; got {samples}")
    return AggDescriptor(
        op="reservoir_sample",
        field=field,
        extras={"samples": samples},
        where=_serialize_where(where),
    )


def geo_velocity(
    *, lat: str, lon: str, where: Any = None
) -> AggDescriptor:
    """Lifetime-only max km/h between consecutive matching events."""
    return AggDescriptor(
        op="geo_velocity",
        extras={"lat_field": lat, "lon_field": lon},
        where=_serialize_where(where),
    )


def geo_distance(
    *, lat: str, lon: str, where: Any = None
) -> AggDescriptor:
    """Lifetime-only cumulative haversine path length."""
    return AggDescriptor(
        op="geo_distance",
        extras={"lat_field": lat, "lon_field": lon},
        where=_serialize_where(where),
    )


def geo_spread(
    *, lat: str, lon: str, where: Any = None
) -> AggDescriptor:
    """Lifetime-only RMS dispersion around the running centroid."""
    return AggDescriptor(
        op="geo_spread",
        extras={"lat_field": lat, "lon_field": lon},
        where=_serialize_where(where),
    )


def distance_from_home(
    *, lat: str, lon: str, samples: int = 100, where: Any = None
) -> AggDescriptor:
    """Distance from the current point to the centroid of the last
    ``samples`` matching events.
    """
    if samples < 1:
        raise ValueError(
            f"distance_from_home samples must be >= 1; got {samples}"
        )
    return AggDescriptor(
        op="distance_from_home",
        extras={"lat_field": lat, "lon_field": lon, "samples": samples},
        where=_serialize_where(where),
    )


def avg(field: Any, **kw: Any) -> AggDescriptor:
    """Deprecated alias for :func:`mean`."""
    warnings.warn(
        "bv.avg is deprecated; use bv.mean",
        DeprecationWarning,
        stacklevel=2,
    )
    return mean(field, **kw)


def variance(field: Any, **kw: Any) -> AggDescriptor:
    """Deprecated alias for :func:`var`."""
    warnings.warn(
        "bv.variance is deprecated; use bv.var",
        DeprecationWarning,
        stacklevel=2,
    )
    return var(field, **kw)


def stddev(field: Any, **kw: Any) -> AggDescriptor:
    """Deprecated alias for :func:`std`."""
    warnings.warn(
        "bv.stddev is deprecated; use bv.std",
        DeprecationWarning,
        stacklevel=2,
    )
    return std(field, **kw)


def count_distinct(field: Any, **kw: Any) -> AggDescriptor:
    """Deprecated alias for :func:`n_unique`."""
    warnings.warn(
        "bv.count_distinct is deprecated; use bv.n_unique",
        DeprecationWarning,
        stacklevel=2,
    )
    return n_unique(field, **kw)


def percentile(field: Any, *, p: float, **kw: Any) -> AggDescriptor:
    """Deprecated alias for :func:`quantile` (use ``q=`` instead of ``p=``)."""
    warnings.warn(
        "bv.percentile is deprecated; use bv.quantile(q=...)",
        DeprecationWarning,
        stacklevel=2,
    )
    return quantile(field, q=p, **kw)
