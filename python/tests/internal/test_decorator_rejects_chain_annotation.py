"""Phase 13.5.2 D-02 — RED contract test.

Both decorator resolvers (`@bv.table` via `_resolve_upstream_proxies` and
`@bv.event def` via `_make_event_derivation`) MUST reject parameter
annotations that resolve to an `EventDerivation` instance (a raw chain
expression like `Click.with_columns(...).named('Tagged')`).

The rejection fires at DECORATION time (before `app.register` is ever
called), with `TypeError` whose message includes the canonical
`@bv.event def Tagged(click: Click): ...` rewrite hint.

These tests MUST FAIL at HEAD before Plan 13.5.2-04 (GREEN) lands.
"""
from __future__ import annotations

import pytest

import beava as bv
from beava._events import EventDerivation

# ---------------------------------------------------------------------------
# @bv.table function form
# ---------------------------------------------------------------------------


def test_bv_table_rejects_chain_annotation_with_columns() -> None:
    """@bv.table parameter annotated with a `with_columns` chain instance → TypeError."""

    @bv.event
    class Click:
        user_id: str

    Tagged = Click.with_columns(source=bv.lit("web")).named("Tagged")
    assert isinstance(Tagged, EventDerivation), (
        "test precondition: `.named()` returns EventDerivation"
    )

    with pytest.raises(TypeError) as ei:

        @bv.table(key="user_id")
        def UserClicks(tagged: Tagged):  # type: ignore[valid-type]
            return tagged.group_by("user_id").agg(c=bv.count(window="forever"))

    msg = str(ei.value)
    assert "EventDerivation" in msg, f"hint missing 'EventDerivation': {msg!r}"
    assert "@bv.event" in msg, f"hint missing @bv.event rewrite: {msg!r}"
    assert "tagged" in msg, f"hint must name the offending param: {msg!r}"
    assert "UserClicks" in msg, f"hint must name the function: {msg!r}"


def test_bv_table_rejects_chain_annotation_filter() -> None:
    """@bv.table parameter annotated with a `filter` chain instance → TypeError."""

    @bv.event
    class Tx:
        user_id: str
        amount: float

    Big = Tx.filter(bv.col("amount") > 100).named("Big")
    assert isinstance(Big, EventDerivation)

    with pytest.raises(TypeError) as ei:

        @bv.table(key="user_id")
        def CountBig(big: Big):  # type: ignore[valid-type]
            return big.group_by("user_id").agg(n=bv.count(window="forever"))

    assert "EventDerivation" in str(ei.value)


# ---------------------------------------------------------------------------
# @bv.event def function form
# ---------------------------------------------------------------------------


def test_bv_event_def_rejects_chain_annotation_with_columns() -> None:
    """@bv.event def parameter annotated with a chain instance → TypeError."""

    @bv.event
    class Click:
        user_id: str

    Tagged = Click.with_columns(source=bv.lit("web")).named("Tagged")

    with pytest.raises(TypeError) as ei:

        @bv.event
        def Filtered(tagged: Tagged):  # type: ignore[valid-type]
            return tagged.filter(bv.col("source") == bv.lit("web"))

    msg = str(ei.value)
    assert "EventDerivation" in msg, f"hint missing 'EventDerivation': {msg!r}"
    assert "@bv.event" in msg, f"hint missing @bv.event rewrite: {msg!r}"
    assert "Filtered" in msg, f"hint must name the function: {msg!r}"


def test_bv_event_def_rejects_chain_annotation_filter() -> None:
    """@bv.event def parameter annotated with a filter-chain instance → TypeError."""

    @bv.event
    class Tx:
        user_id: str
        amount: float

    Big = Tx.filter(bv.col("amount") > 100).named("Big")

    with pytest.raises(TypeError) as ei:

        @bv.event
        def DoubleBig(big: Big):  # type: ignore[valid-type]
            return big.filter(bv.col("amount") > 200)

    assert "EventDerivation" in str(ei.value)


# ---------------------------------------------------------------------------
# Positive control — class & @bv.event-decorated function annotations work
# ---------------------------------------------------------------------------


def test_decorators_accept_event_class_and_event_def_positive_control() -> None:
    """Positive control: class & @bv.event-decorated function annotations remain accepted.

    Guards against accidentally over-rejecting in the GREEN impl.
    """

    @bv.event
    class Click:
        user_id: str
        page: str

    # Wrap the chain in @bv.event def — the canonical form.
    @bv.event
    def Tagged(click: Click):
        return click.with_columns(source=bv.lit("web"))

    # Both class annotation (`tagged: Click`) and @bv.event-decorated function
    # annotation (`tagged: Tagged`) must NOT raise — they're the public-surface
    # valid forms per CONTEXT D-02.
    @bv.table(key="user_id")
    def UserClicksRaw(tagged: Click):
        return tagged.group_by("user_id").agg(c=bv.count(window="forever"))

    @bv.table(key="user_id")
    def UserClicksTagged(tagged: Tagged):  # type: ignore[valid-type]
        return tagged.group_by("user_id").agg(c=bv.count(window="forever"))

    # If we got here without TypeError, the positive control passes.
    assert UserClicksRaw is not None
    assert UserClicksTagged is not None
