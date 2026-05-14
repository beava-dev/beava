"""End-to-end roundtrip coverage for aggregations on SYNTHETIC fields.

A "synthetic field" is one that doesn't exist on the source event but is
introduced by an upstream derivation — ``with_columns`` / ``cast`` /
``rename``. The downstream ``@bv.table`` then aggregates against that
synthetic field, exercising the agg-local ``field_idx`` → union
``field_idx_into_event_extracted`` resolver. The engine-layer
``agg_combinations_matrix.rs`` exercises operator combos well, but it does
NOT push fields through the SDK's chain-derivation surface; if the SDK
ever drifts in how it threads synthetic fields through the schema, the
engine-layer tests can't catch it.

This was the bug class PR #106 lived in (``fix(windowed-op): use caller's
pre_val instead of re-extracting from extracted[field_idx]``):
``WindowedOp::update_at`` re-extracted ``pre_val`` locally using the
agg-local ``field_idx`` against the union-indexed ``extracted`` array. The
two coincide only when an agg's ``field_names`` list happens to match the
source union order — which our test corpus always did, which is how the
bug shipped. Test 5 below directly retests that shape (mean + window +
non-source field) through the SDK.

Pattern mirrors ``tests/test_op_roundtrips_extended.py`` — embed-mode
``bv.App(test_mode=True)`` spawns the local binary, ``@bv.event`` /
``@bv.table`` decorators declare the pipeline, ``app.push`` then
``app.get`` round-trips against the running server. Requires the
``beava_binary`` session fixture to build ``target/debug/beava`` once.
"""

from __future__ import annotations

import pytest

import beava as bv


@pytest.fixture
def app(beava_binary):  # noqa: ARG001 — fixture pulled in for binary side-effect
    """Yield a fresh embed-mode ``bv.App(test_mode=True)`` per test."""
    with bv.App(test_mode=True) as instance:
        yield instance


# ---------------------------------------------------------------------------
# Test 1: mean on a with_columns-synthesised field
# ---------------------------------------------------------------------------


def test_mean_on_with_columns_synthetic_field(app):
    """bv.mean over a ``rate = amount / count`` synthetic field."""

    @bv.event
    class Tx:
        user_id: str
        amount: float
        count: int

    @bv.event
    def Rated(tx: Tx):
        return tx.with_columns(rate=bv.col("amount") / bv.col("count"))

    @bv.table(key="user_id")
    def RateStats(rated: Rated):
        return rated.group_by("user_id").agg(mean_rate=bv.mean("rate"))

    app.register(Tx, Rated, RateStats)

    # rates: 100/4=25, 200/4=50, 300/4=75 → mean = 50.
    pushes = [(100.0, 4), (200.0, 4), (300.0, 4)]
    for amount, count in pushes:
        app.push("Tx", {"user_id": "alice", "amount": amount, "count": count})

    row = app.get("RateStats", "alice")
    expected = sum(a / c for a, c in pushes) / len(pushes)
    actual = float(row["mean_rate"])
    assert abs(actual - expected) < 1e-9, (
        f"mean_rate on synthetic field: expected {expected}, got {actual}"
    )


# ---------------------------------------------------------------------------
# Test 2: sum on a renamed-only field
# ---------------------------------------------------------------------------


def test_sum_on_renamed_field(app):
    """bv.sum over a field that exists only after ``rename(amount='value')``.

    The source event has ``amount`` but the downstream table aggregates by
    ``value`` — the rename remap must thread through schema propagation so
    the agg's ``field_idx`` lookup hits the renamed column.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.event
    def Renamed(tx: Tx):
        return tx.rename(amount="value")

    @bv.table(key="user_id")
    def SumStats(rn: Renamed):
        return rn.group_by("user_id").agg(total=bv.sum("value"))

    app.register(Tx, Renamed, SumStats)

    amounts = [10.0, 20.0, 30.0, 40.0]
    for a in amounts:
        app.push("Tx", {"user_id": "alice", "amount": a})

    row = app.get("SumStats", "alice")
    expected = sum(amounts)
    actual = float(row["total"])
    assert abs(actual - expected) < 1e-9, (
        f"sum on renamed field: expected {expected}, got {actual}"
    )


# ---------------------------------------------------------------------------
# Test 3: count(where=…) over a with_columns predicate column
# ---------------------------------------------------------------------------


def test_count_with_where_on_with_columns_predicate(app):
    """bv.count(where=col('is_big')==True) where ``is_big`` is synthesised upstream.

    The predicate references a column that doesn't exist on ``Tx``; it
    comes from ``with_columns(is_big=col('amount') > 100)``. If the SDK
    drifts the schema lift through ``with_columns``, the where-predicate
    eval will see an unknown column and the count will be 0 (or the
    register will fail).
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.event
    def Tagged(tx: Tx):
        return tx.with_columns(is_big=bv.col("amount") > 100)

    @bv.table(key="user_id")
    def Stats(tg: Tagged):
        return tg.group_by("user_id").agg(
            big_count=bv.count(where=bv.col("is_big") == True),  # noqa: E712
        )

    app.register(Tx, Tagged, Stats)

    # amounts > 100 → 3 rows (150, 200, 300); the other two (50, 75) miss.
    amounts = [50.0, 150.0, 200.0, 75.0, 300.0]
    expected_big = sum(1 for a in amounts if a > 100)
    for a in amounts:
        app.push("Tx", {"user_id": "alice", "amount": a})

    row = app.get("Stats", "alice")
    assert row["big_count"] == expected_big, (
        f"big_count over synthetic predicate column: "
        f"expected {expected_big}, got {row['big_count']}"
    )


# ---------------------------------------------------------------------------
# Test 4: quantile on a cast field
# ---------------------------------------------------------------------------


def test_quantile_on_cast_field(app):
    """bv.quantile over a field whose dtype is rewritten by ``cast``.

    Source ``amount`` is ``int``; ``cast(amount='float')`` rewrites the
    column's dtype. The aggregation then samples through the
    DDSketch op, which is dtype-sensitive — if cast doesn't propagate, the
    sketch sees the wrong shape and ``p95`` either returns ``None`` or a
    nonsense value.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: int

    @bv.event
    def Casted(tx: Tx):
        return tx.cast(amount="float")

    @bv.table(key="user_id")
    def Stats(c: Casted):
        return c.group_by("user_id").agg(
            p95=bv.quantile("amount", q=0.95, window="1h"),
        )

    app.register(Tx, Casted, Stats)

    # 20 integer amounts 1..20 → true p95 ≈ 20.0 (index int(20*0.95)=19).
    for i in range(1, 21):
        app.push("Tx", {"user_id": "alice", "amount": i})

    row = app.get("Stats", "alice")
    actual = float(row["p95"])
    expected = 20.0
    # DDSketch relative-error tolerance: ±5%.
    assert abs(actual - expected) / expected <= 0.05, (
        f"p95 on cast field: expected ~{expected} (±5%), got {actual}"
    )


# ---------------------------------------------------------------------------
# Test 5: windowed mean over a synthetic field — the EXACT PR #106 shape
# ---------------------------------------------------------------------------


def test_synthetic_field_with_windowed_op(app):
    """bv.mean('rate', window='30m') over a synthetic ``rate`` field.

    This is the precise shape of the PR #106 bug class:
    ``WindowedOp::update_at`` re-extracted ``pre_val`` locally using the
    agg-local ``field_idx`` against the union-indexed ``extracted`` array.
    Pre-fix, this returned ``Null`` because the agg-local index didn't
    line up with the union-indexed extracted-field array — the synthetic
    ``rate`` column lives at a different position in the agg's
    ``field_names`` than in the upstream ``Rated`` event's union schema.

    Replays the same pipeline as test 1 but uses a windowed aggregation
    instead of a lifetime one — exercising the ``WindowedOp`` path that
    the engine-layer unit tests cover for built-in fields but the SDK
    surface does not exercise for synthetic ones.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float
        count: int

    @bv.event
    def Rated(tx: Tx):
        return tx.with_columns(rate=bv.col("amount") / bv.col("count"))

    @bv.table(key="user_id")
    def RateStats(rated: Rated):
        return rated.group_by("user_id").agg(
            windowed_mean_rate=bv.mean("rate", window="30m"),
        )

    app.register(Tx, Rated, RateStats)

    # rates: 25, 50, 75 within a 30m window → mean = 50.
    pushes = [(100.0, 4), (200.0, 4), (300.0, 4)]
    for amount, count in pushes:
        app.push("Tx", {"user_id": "alice", "amount": amount, "count": count})

    row = app.get("RateStats", "alice")
    # PR #106 pre-fix: this returned None / Null because WindowedOp's
    # update_at re-extracted from the wrong index space. Assert a real
    # numeric value first, then assert the value matches the ground truth.
    assert "windowed_mean_rate" in row, (
        f"windowed_mean_rate missing from row {row!r} — "
        f"PR #106 regression: WindowedOp may have re-extracted a null pre_val"
    )
    actual = row["windowed_mean_rate"]
    assert actual is not None, (
        "windowed_mean_rate is None — PR #106 regression: "
        "WindowedOp::update_at lost the synthetic field's pre_val"
    )
    expected = sum(a / c for a, c in pushes) / len(pushes)
    actual = float(actual)
    assert abs(actual - expected) < 1e-9, (
        f"windowed mean_rate on synthetic field: expected {expected}, got {actual}"
    )
