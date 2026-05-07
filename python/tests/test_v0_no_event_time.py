"""Plan 12.6-08: Python SDK strict-deny on event_time / tolerate_delay / joins / unions.

Per the no-event-time architectural pivot (locked 2026-04-30, see
`project_redis_shaped_no_event_time_ever`), the public Python SDK API has:

  - No event_time field on @bv.event class form (TypeError at decorator time)
  - No tolerate_delay parameter on @bv.event (TypeError at decorator time)
  - No event_time_field parameter on @bv.event (TypeError at decorator time)
  - No bv.join helper (AttributeError on bv namespace)
  - No bv.union helper (AttributeError on bv namespace)

These tests pin the contract. They run RED before Task 1.b strips the surface
and lift to GREEN once Task 1.b lands.
"""

from __future__ import annotations

import datetime

import pytest

import beava as bv

# ---------------------------------------------------------------------------
# Decorator-time strict-deny: event_time field
# ---------------------------------------------------------------------------


def test_event_class_with_event_time_int_field_raises_type_error() -> None:
    """A class declaring `event_time: int` must error at decorator time."""
    with pytest.raises(
        TypeError, match=r"event_time.*no.event-time.pivot|removed|not supported"
    ):

        @bv.event
        class Tx:
            user_id: str
            amount: float
            event_time: int  # post-pivot: this must error at decorator time

        _ = Tx


def test_event_class_with_event_time_datetime_field_raises_type_error() -> None:
    """A class declaring `event_time: datetime` must also error."""
    with pytest.raises(
        TypeError, match=r"event_time.*no.event-time.pivot|removed|not supported"
    ):

        @bv.event
        class Tx:
            user_id: str
            event_time: datetime.datetime

        _ = Tx


# ---------------------------------------------------------------------------
# Decorator-time strict-deny: tolerate_delay parameter
# ---------------------------------------------------------------------------


def test_bv_event_with_tolerate_delay_param_raises_type_error() -> None:
    """`@bv.event(tolerate_delay=...)` must error at decorator time."""
    with pytest.raises(
        TypeError, match=r"tolerate_delay.*removed|not supported|no.event-time.pivot"
    ):

        @bv.event(tolerate_delay="5s")  # type: ignore[call-arg]
        class Tx2:
            user_id: str
            amount: float

        _ = Tx2


def test_bv_event_with_event_time_field_param_raises_type_error() -> None:
    """`@bv.event(event_time_field=...)` must error at decorator time."""
    with pytest.raises(
        TypeError, match=r"event_time_field.*removed|not supported|no.event-time.pivot"
    ):

        @bv.event(event_time_field="ts")  # type: ignore[call-arg]
        class Tx3:
            user_id: str
            ts: int

        _ = Tx3


# ---------------------------------------------------------------------------
# bv namespace: no join / union helpers
# ---------------------------------------------------------------------------


def test_bv_namespace_has_no_join() -> None:
    """`bv.join` must not exist post-pivot (joins removed v0)."""
    assert not hasattr(bv, "join"), "bv.join must be removed per no-event-time pivot"


def test_bv_namespace_has_no_union() -> None:
    """`bv.union` must not exist post-pivot (union deferred v0)."""
    assert not hasattr(bv, "union"), "bv.union must be deferred per no-event-time pivot"


# ---------------------------------------------------------------------------
# to_register_dict serialization: no event-time keys
# ---------------------------------------------------------------------------


def test_register_json_omits_event_time_field_and_tolerate_delay_ms() -> None:
    """EventSource._to_register_json must not emit legacy event-time keys."""

    @bv.event
    class CleanTx:
        user_id: str
        amount: float

    j = CleanTx._to_register_json()
    assert "event_time_field" not in j, (
        "to_register_json must not emit event_time_field key per D-03"
    )
    assert "tolerate_delay_ms" not in j, (
        "to_register_json must not emit tolerate_delay_ms key per D-03"
    )
