"""Reject multi-upstream ``@bv.event def`` at decoration time.

The engine only supports single-upstream event derivations in v0:
``_to_register_json`` (``python/beava/_app.py``) walks ``_parent`` up to
ONE EventSource root and silently drops any second upstream. The server
then rejects the registration with the unhelpful
``invalid_registration: missing field 'fields'``. The fix is to surface
the limitation at decoration time with a sharp ``TypeError`` — mirrors
the canonical "raw chain as parameter" rejection in
``_events._make_event_derivation``.

These tests pin the decoration-time contract.
"""
from __future__ import annotations

import pytest

import beava as bv


def test_two_event_params_raises_at_decoration_time() -> None:
    """``@bv.event def C(a: A, b: B)`` raises TypeError at decoration."""

    @bv.event
    class A:
        user_id: str

    @bv.event
    class B:
        user_id: str

    with pytest.raises(TypeError) as ei:

        @bv.event
        def C(a: A, b: B):  # type: ignore[valid-type]
            return a.filter(bv.col("user_id") == bv.lit("x"))

    msg = str(ei.value)
    assert "multi-upstream" in msg, f"missing 'multi-upstream' hint: {msg!r}"
    assert "C" in msg, f"hint must name the function: {msg!r}"
    assert "a" in msg and "b" in msg, (
        f"hint must list the offending param names: {msg!r}"
    )
    assert "not supported in v0" in msg, f"hint must call out v0 limit: {msg!r}"


def test_three_event_params_raises_at_decoration_time() -> None:
    """Three event-shaped params also rejected — not just exactly two."""

    @bv.event
    class A:
        user_id: str

    @bv.event
    class B:
        user_id: str

    @bv.event
    class D:
        user_id: str

    with pytest.raises(TypeError) as ei:

        @bv.event
        def Triple(a: A, b: B, d: D):  # type: ignore[valid-type]
            return a.filter(bv.col("user_id") == bv.lit("x"))

    msg = str(ei.value)
    assert "multi-upstream" in msg
    assert "Triple" in msg
    for name in ("a", "b", "d"):
        assert name in msg, f"hint must list param {name!r}: {msg!r}"


def test_one_event_param_with_unrelated_kwarg_still_works() -> None:
    """Control: single-upstream derivation still works (no over-rejection)."""

    @bv.event
    class A:
        user_id: str
        amount: float

    @bv.event
    def Big(a: A):  # type: ignore[valid-type]
        return a.filter(bv.col("amount") > bv.lit(100))

    # If we got here without a TypeError, the positive control passes.
    assert Big is not None
    assert getattr(Big, "_is_bv_event_function", False) is True


def test_two_upstreams_one_event_class_one_event_def_also_rejected() -> None:
    """Coverage: mix of ``@bv.event class`` + ``@bv.event def`` annotations."""

    @bv.event
    class A:
        user_id: str

    @bv.event
    class B:
        user_id: str

    @bv.event
    def BFiltered(b: B):  # type: ignore[valid-type]
        return b.filter(bv.col("user_id") == bv.lit("x"))

    with pytest.raises(TypeError) as ei:

        @bv.event
        def Mixed(a: A, b: BFiltered):  # type: ignore[valid-type]
            return a.filter(bv.col("user_id") == bv.lit("x"))

    assert "multi-upstream" in str(ei.value)
