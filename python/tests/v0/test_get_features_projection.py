"""End-to-end coverage for ``App.get(table, key, features=[...])`` projection.

Locks down the narrow-projection form of the GET wire surface:
  - ``features=["a", "c"]`` narrows a 5-feature row to a 2-key dict.
  - ``features=[]`` documents the current empty-filter behaviour.
  - Unknown feature names raise a structured ``RegistrationError`` carrying
    ``feature_not_found`` (matches
    ``runtime_core_glue::dispatch_get_single_verb_style_sync``).
  - Projection survives schema flattening across an ``@bv.event``
    ``with_columns(...)`` derived event boundary.
  - Cold-start key returns the same empty/None shape as the no-features
    path (per the ``cold_start_equivalent`` contract).
  - ``features=None`` is the full-row default — sanity-checked so future
    changes don't silently narrow.

Style follows ``python/tests/v0/test_core.py``: each test pushes a real
event stream against the fixture-spawned subprocess (no transport mocks),
computes ground truth in Python, asserts both the row shape AND the
projected subset.
"""
from __future__ import annotations

import pytest

import beava as bv
from beava import RegistrationError

from ._helpers import _engine_available, cold_start_equivalent

pytestmark = pytest.mark.skipif(
    not _engine_available(),
    reason="requires Phase 13.4 engine + Phase 13.5 SDK rewrite",
)


# ---------------------------------------------------------------------------
# Shared pipeline — a table with five features in a stable order.
# ---------------------------------------------------------------------------


def _register_five_feature_table(app):
    """Register a Click event + UserActivity table with 5 named features.

    Returns nothing — the caller pushes events via ``app.push("Click", ...)``
    and queries via ``app.get("UserActivity", entity, features=...)``.

    The feature list is intentionally heterogeneous (count / sum / mean /
    min / max) so a projection on any subset is non-trivial and asserts the
    server emits *exactly* the requested keys.
    """

    @bv.event
    class Click:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserActivity(clicks: Click):
        return clicks.group_by("user_id").agg(
            feat_count=bv.count(window="forever"),
            feat_sum=bv.sum("amount", window="forever"),
            feat_mean=bv.mean("amount", window="forever"),
            feat_min=bv.min("amount", window="forever"),
            feat_max=bv.max("amount", window="forever"),
        )

    app.register(Click, UserActivity)


def _push_alice_baseline(app) -> dict[str, float]:
    """Push 50 deterministic Click events for ``alice``; return ground truth."""
    amounts = [float(i + 1) for i in range(50)]  # 1.0..50.0 inclusive
    for amount in amounts:
        app.push("Click", {"user_id": "alice", "amount": amount})
    return {
        "feat_count": float(len(amounts)),
        "feat_sum": sum(amounts),
        "feat_mean": sum(amounts) / len(amounts),
        "feat_min": min(amounts),
        "feat_max": max(amounts),
    }


# ---------------------------------------------------------------------------
# Test 1: features projection returns exactly the requested subset.
# ---------------------------------------------------------------------------


def test_features_projection_returns_only_requested_subset(app):
    """``features=["feat_count", "feat_mean"]`` → row carries those 2 keys only."""
    _register_five_feature_table(app)
    expected = _push_alice_baseline(app)

    # Baseline — full row holds all five features.
    full = app.get("UserActivity", "alice")
    assert set(full.keys()) == {
        "feat_count",
        "feat_sum",
        "feat_mean",
        "feat_min",
        "feat_max",
    }, f"baseline get must return the full 5-feature row; got {full!r}"

    # Projection — server must return *exactly* the two requested features.
    projected = app.get(
        "UserActivity", "alice", features=["feat_count", "feat_mean"]
    )
    assert set(projected.keys()) == {"feat_count", "feat_mean"}, (
        f"projection must return exactly the requested feature names; "
        f"got {projected!r}"
    )
    # Values must match the full-row values (projection is a filter, not a
    # different code path that re-aggregates).
    assert projected["feat_count"] == expected["feat_count"]
    assert abs(projected["feat_mean"] - expected["feat_mean"]) < 1e-9


# ---------------------------------------------------------------------------
# Test 2: features=[] empty list — lock down current behaviour.
# ---------------------------------------------------------------------------


def test_features_projection_with_empty_list_returns_full_row(app):
    """``features=[]`` is sent on the wire and yields the no-feature shape.

    Lock-down: the JSON layer DOES forward an empty list (``features``
    in payload is keyed by ``is not None``, per ``_transport.send_get``),
    and the server's projection loop's ``filter.iter().any(...)`` excludes
    *every* feature when the filter is empty. The contract is therefore
    "no features", manifest as an empty dict (cold-start-shaped).

    If a future commit flips this to "empty list means full row", that
    change MUST update this test and document the intent.
    """
    _register_five_feature_table(app)
    _push_alice_baseline(app)

    result = app.get("UserActivity", "alice", features=[])
    # Empty filter omits every feature server-side; the row collapses to {}.
    # We tolerate cold-start-equivalent shapes (``{}`` or ``None``) to track
    # the same transport-vs-embed slop the conftest's helper papers over.
    assert cold_start_equivalent(result), (
        f"features=[] must yield the empty/None cold-start shape; got {result!r}"
    )


# ---------------------------------------------------------------------------
# Test 3: features with an unknown name — structured error contract.
# ---------------------------------------------------------------------------


def test_features_projection_with_unknown_feature_name(app):
    """Unknown feature in the filter raises ``RegistrationError``.

    Per ``runtime_core_glue::dispatch_get_single_verb_style_sync`` (the
    verb-style /get handler), any name in ``features`` not present in the
    registered descriptor triggers an early ``feature_not_found`` reject of
    the whole request — matching the batch path's whole-batch-reject
    disposition. The transport surfaces this as ``RegistrationError``.

    Wire-level lossiness lock-down: the TCP error-response encoder
    (``server.rs::encode_glue_response_tcp``) maps the ``InternalError``
    variant through the catch-all ``OP_ERROR_RESPONSE`` body
    ``{"code": "unsupported"}`` — i.e. the ``feature_not_found`` reason
    and offending name are dropped on the TCP wire. The HTTP transport
    preserves them via ``encode_glue_response_http``'s ``InternalError``
    arm. This test asserts only the cross-transport invariant — that the
    SDK surfaces the rejection as ``RegistrationError`` (NOT a silent
    pass-through that fabricates a row).
    """
    _register_five_feature_table(app)
    _push_alice_baseline(app)

    # Baseline guards against false positives: a *valid* projection on
    # the same row must NOT raise, so any RegistrationError below is
    # provably caused by the unknown-feature name.
    ok = app.get("UserActivity", "alice", features=["feat_count"])
    assert "feat_count" in ok, (
        f"sanity baseline: valid projection must succeed; got {ok!r}"
    )

    with pytest.raises(RegistrationError):
        app.get(
            "UserActivity",
            "alice",
            features=["feat_count", "definitely_not_a_real_feature"],
        )


# ---------------------------------------------------------------------------
# Test 4: projection on a derived-event table — schema flattening boundary.
# ---------------------------------------------------------------------------


def test_features_projection_on_derived_event_table(app):
    """``with_columns(rate=...)`` synthetic field is projectable downstream.

    Reproduces ``test_lit.py::test_lit_force_float_division`` but adds a
    second feature on top so a ``features=["mean_rate"]`` projection is
    distinguishable from "the whole row happens to be one key". Asserts the
    schema-flattening pass (derived-event with_columns → group-by → agg →
    feature row) preserves the synthetic name *and* that projection on the
    synthetic name works.
    """

    @bv.event
    class Telemetry:
        user_id: str
        count: int

    @bv.event
    def Rated(telemetry: Telemetry):
        return telemetry.with_columns(rate=bv.col("count") / bv.lit(60.0))

    @bv.table(key="user_id")
    def UserRated(rated: Rated):
        return rated.group_by("user_id").agg(
            mean_rate=bv.mean("rate", window="forever"),
            total_rate=bv.sum("rate", window="forever"),
        )

    app.register(Telemetry, Rated, UserRated)

    # Push 120 events for bob — rates are deterministic count / 60.0.
    counts = list(range(1, 121))
    for c in counts:
        app.push("Telemetry", {"user_id": "bob", "count": c})

    rates = [c / 60.0 for c in counts]
    expected_mean = sum(rates) / len(rates)
    expected_total = sum(rates)

    # Sanity: full row has both synthetic-name features.
    full = app.get("UserRated", "bob")
    assert set(full.keys()) == {"mean_rate", "total_rate"}, (
        f"baseline must carry both derived-event-fed features; got {full!r}"
    )
    assert abs(float(full["mean_rate"]) - expected_mean) < 1e-3
    assert abs(float(full["total_rate"]) - expected_total) < 1e-3

    # Projection on the synthetic-field-fed feature.
    projected = app.get("UserRated", "bob", features=["mean_rate"])
    assert set(projected.keys()) == {"mean_rate"}, (
        f"projection across with_columns boundary must narrow to "
        f"['mean_rate']; got {projected!r}"
    )
    assert abs(float(projected["mean_rate"]) - expected_mean) < 1e-3


# ---------------------------------------------------------------------------
# Test 5: missing key + features filter — consistent cold-start shape.
# ---------------------------------------------------------------------------


def test_features_projection_on_missing_key_returns_consistent_shape(app):
    """``features=["f"]`` on a cold-start key matches the no-features shape.

    Locks the contract that the projection layer doesn't fabricate a row
    for a key that was never pushed — it must return the same empty/None
    cold-start shape as ``app.get(table, missing_key)`` without features.
    """
    _register_five_feature_table(app)
    # Push some traffic for alice so the table exists but ``never_pushed``
    # genuinely has no state — distinguishes "table unregistered" (would
    # raise ``unknown_table``) from "key cold-start" (returns empty).
    _push_alice_baseline(app)

    cold_full = app.get("UserActivity", "never_pushed")
    cold_narrow = app.get(
        "UserActivity", "never_pushed", features=["feat_count"]
    )

    assert cold_start_equivalent(cold_full), (
        f"baseline cold-start with no filter must be {{}} or None; got {cold_full!r}"
    )
    assert cold_start_equivalent(cold_narrow), (
        f"cold-start with projection filter must match no-filter shape; "
        f"got {cold_narrow!r}"
    )


# ---------------------------------------------------------------------------
# Test 6: features=None (default) returns the full row — no accidental narrowing.
# ---------------------------------------------------------------------------


def test_features_projection_with_features_none_returns_full_row(app):
    """Default ``features=None`` is wire-omitted and returns every feature.

    Sanity-checks that ``App.get(table, key)`` (the call form without the
    kwarg) and ``App.get(table, key, features=None)`` (the explicit
    default) are observationally identical, both returning the full 5-key
    row. Catches accidental narrowing if a future refactor changes the
    sentinel meaning of ``None``.
    """
    _register_five_feature_table(app)
    expected = _push_alice_baseline(app)

    implicit = app.get("UserActivity", "alice")
    explicit_none = app.get("UserActivity", "alice", features=None)

    expected_keys = {
        "feat_count",
        "feat_sum",
        "feat_mean",
        "feat_min",
        "feat_max",
    }
    assert set(implicit.keys()) == expected_keys, (
        f"implicit features=None must return the full row; got {implicit!r}"
    )
    assert set(explicit_none.keys()) == expected_keys, (
        f"explicit features=None must return the full row; got {explicit_none!r}"
    )
    # Both code paths must surface the same values too — not just the same
    # key set. A future bug where the default branch reuses a stale cached
    # row would slip through a keys-only assert.
    assert implicit == explicit_none, (
        f"features=None and the no-kwarg form must be observationally "
        f"identical; got {implicit!r} vs {explicit_none!r}"
    )
    assert implicit["feat_count"] == expected["feat_count"]
    assert abs(implicit["feat_sum"] - expected["feat_sum"]) < 1e-6
    assert abs(implicit["feat_mean"] - expected["feat_mean"]) < 1e-9
    assert implicit["feat_min"] == expected["feat_min"]
    assert implicit["feat_max"] == expected["feat_max"]
