"""Coverage for type mismatch at push time.

The audit flagged no test asserts what the server does when a push payload's
field types disagree with the declared schema. These tests lock down the
**current observed contract** so any future change to it is visible.

# REAL BUG SURFACED (2026-05-14)

The Python SDK's TCP-embed transport ``_transport.py::TcpTransport.send_push``
(lines 443-465) does **not** check the response frame's opcode. The server
emits ``OP_ERROR_RESPONSE`` with a JSON body of
``{"error": {"code": "..."}, "registry_version": N}`` on push validation
failures, but the SDK happily parses that as JSON and returns the dict to
user code — there is no ``RegistrationError`` raised, no exception, no
signal that the push was rejected.

Compare to ``send_get`` (lines 467-506) which correctly checks
``frame.op != OP_GET_RESPONSE`` and raises. ``send_push`` is missing the
analogous opcode check (``frame.op != OP_PUSH_RESPONSE`` or the equivalent).

Practical impact: user code that does ``app.push(...)`` and ignores the
return value (the documented "fire-and-forget" idiom) will silently drop
events on validation failures. There is no exception, no log, no
``ack_lsn``. The user sees a successful return.

The HTTP transport path at ``_transport.py:156-192`` is correctly written
(it raises on non-2xx), but the embed-mode default is TCP, so the bug
fires on the docs' canonical setup.

These tests lock down the bug as it stands today by asserting the **shape
of the returned dict** (``"error"`` vs ``"ack_lsn"``). When the SDK is
fixed to raise, these tests will flip and must be rewritten to assert
``pytest.raises(RegistrationError)``.

# Contract pinned (verified by reading the impl)

1. Client-side validation: the Python SDK does **not** validate payload
   types in :py:meth:`bv.App.push`. ``_app.py:585-594`` forwards directly
   to ``transport.send_push``; ``_transport.py:443-465`` JSON-encodes
   ``{"event": name, "body": fields}`` and ships it on the wire as-is.

2. Server-side validation lives in
   ``crates/beava-server/src/apply_shard.rs::dispatch_push_sync``:

   - Unknown field names → ``unknown_field_v0``
     (``unknown_field_event_time_v0`` for legacy ``event_time`` /
     ``event_time_ms`` names).
   - Type mismatch / missing required / null-against-required →
     ``invalid_event``.

   Compatibility table (``value_type_compatible``):

     * ``FieldType::I64`` | ``FieldType::F64`` ↔ ``Value::I64`` |
       ``Value::F64`` — numeric types are **bidirectionally interchangeable**.
       Float against int field is **silently accepted** with the float
       preserved as-is (no truncation).
     * ``FieldType::Str`` ↔ ``Value::Str`` only.
     * ``FieldType::Bool`` ↔ ``Value::Bool`` only.
     * ``FieldType::Bytes`` | ``Datetime`` | ``Json`` ↔ any non-null.

3. Success response: ``{"ack_lsn": int, "registry_version": int,
   "idempotent_replay": bool}``.
   Error response (over TCP-embed): ``{"error": {"code": "..."},
   "registry_version": int}`` — currently returned to user code instead
   of raising.
"""

from __future__ import annotations

from typing import Any

import pytest

import beava as bv


@pytest.fixture
def app(beava_binary):  # noqa: ARG001 — pulls the cargo-build side-effect
    """Fresh embed-mode ``bv.App(test_mode=True)`` per test."""
    with bv.App(test_mode=True) as instance:
        yield instance


def _assert_push_error(result: dict[str, Any], expected_code: str) -> None:
    """Assert that a push result is the (silent) error-body shape.

    Locks down the BUG documented in the module docstring: the TCP-embed
    transport does not raise on server-side push errors. When the SDK is
    fixed, these assertions will fail and must be rewritten to
    ``pytest.raises(RegistrationError) as exc_info; assert
    exc_info.value.code == expected_code``.
    """
    assert "error" in result, (
        f"BUG-FIX REGRESSION: expected error-body shape "
        f"(SDK currently does NOT raise on push errors over TCP-embed); "
        f"got {result!r}. If the SDK was fixed to raise, rewrite this "
        f"test to use pytest.raises(RegistrationError)."
    )
    assert "ack_lsn" not in result, (
        f"expected error-body without ack_lsn, got {result!r}"
    )
    code = result["error"].get("code")
    assert code == expected_code, (
        f"expected error code {expected_code!r}, got {code!r} "
        f"(full body: {result!r})"
    )


def _assert_push_ok(result: dict[str, Any]) -> None:
    """Assert that a push result is the success shape (ack_lsn present)."""
    assert "ack_lsn" in result, f"expected ack_lsn in result, got {result!r}"
    assert "error" not in result, (
        f"expected no error in success result, got {result!r}"
    )


# ---------------------------------------------------------------------------
# 1. String against float field → server rejects with invalid_event
# ---------------------------------------------------------------------------


def test_push_string_against_float_field(app):
    """``amount: float`` + ``{"amount": "abc"}`` → ``invalid_event``.

    ``"abc"`` parses to ``Value::Str``; ``value_type_compatible(Str, F64)``
    returns false → server emits ``invalid_event``. The SDK does NOT
    raise (see module-docstring bug note) — the error body comes back as
    the return value.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    app.register(Tx)

    result = app.push("Tx", {"user_id": "alice", "amount": "abc"})
    _assert_push_error(result, "invalid_event")


# ---------------------------------------------------------------------------
# 2. Float against int field → silently accepted (numeric coercion)
# ---------------------------------------------------------------------------


def test_push_float_against_int_field(app):
    """``count: int`` + ``{"count": 1.5}`` → **accepted** as F64.

    Lock-down: numeric types (I64 ↔ F64) are bidirectionally compatible
    per ``value_type_compatible``. The 1.5 payload is stored as
    ``Value::F64(1.5)`` and the apply path consumes it as-is — no
    truncation, no rounding, no rejection.

    This is the sharpest contract point in the file: it's the only
    type-mismatch case the server **intentionally** accepts. If a future
    refactor tightens it (e.g. "int field must receive int"), the
    aggregation total will change from 5.0 to 4 (1+1+2) and this test
    will fail loudly.
    """

    @bv.event
    class Counter:
        user_id: str
        count: int

    @bv.table(key="user_id")
    def CounterStats(c: Counter):
        return c.group_by("user_id").agg(
            total=bv.sum("count"),
        )

    app.register(Counter, CounterStats)

    # Push three events: one int, two floats. All should succeed.
    r1 = app.push("Counter", {"user_id": "alice", "count": 1})
    _assert_push_ok(r1)
    r2 = app.push("Counter", {"user_id": "alice", "count": 1.5})
    _assert_push_ok(r2)
    r3 = app.push("Counter", {"user_id": "alice", "count": 2.5})
    _assert_push_ok(r3)

    # Aggregation must reflect the float values un-truncated:
    # 1 + 1.5 + 2.5 = 5.0. If the wire truncated, total would be 4.
    row = app.get("CounterStats", "alice")
    assert "total" in row, f"expected total in row, got {row!r}"
    assert abs(float(row["total"]) - 5.0) < 1e-9, (
        f"expected total=5.0 (no truncation), got {row['total']!r} — "
        f"if this changed, either the server tightened the int-vs-float "
        f"check or it started truncating floats in int fields"
    )


# ---------------------------------------------------------------------------
# 3. Int against str field → rejected (invalid_event)
# ---------------------------------------------------------------------------


def test_push_int_against_str_field(app):
    """``user_id: str`` + ``{"user_id": 42}`` → ``invalid_event``.

    JSON ``42`` parses to ``Value::I64(42)``;
    ``value_type_compatible(I64, Str)`` is false → server rejects. No
    silent stringification ("42") happens. SDK does NOT raise — error
    body is returned (see module bug note).
    """

    @bv.event
    class Click:
        user_id: str
        page: str

    app.register(Click)

    result = app.push("Click", {"user_id": 42, "page": "/home"})
    _assert_push_error(result, "invalid_event")


# ---------------------------------------------------------------------------
# 4. Missing required field → rejected (invalid_event)
# ---------------------------------------------------------------------------


def test_push_with_missing_required_field(app):
    """Schema declares 3 fields, push 2 → ``invalid_event``.

    ``validate_row_against_descriptor`` walks ``descriptor.schema.fields``;
    the first missing required field (``row.get(field_name) == None``)
    returns false → server rejects. SDK does NOT raise.
    """

    @bv.event
    class Order:
        user_id: str
        item_id: str
        amount: float

    app.register(Order)

    # Missing `amount`.
    result = app.push("Order", {"user_id": "alice", "item_id": "sku-1"})
    _assert_push_error(result, "invalid_event")


# ---------------------------------------------------------------------------
# 5. Unknown field → rejected (unknown_field_v0)
# ---------------------------------------------------------------------------


def test_push_with_unknown_field(app):
    """Push includes a field not in schema → ``unknown_field_v0``.

    The strict-deny pass at ``apply_shard.rs:856-875`` runs before type
    validation, so the error code is ``unknown_field_v0`` (not
    ``invalid_event``). ``event_time`` / ``event_time_ms`` get the
    special-case ``unknown_field_event_time_v0`` code instead; this test
    pins the general case. SDK does NOT raise.
    """

    @bv.event
    class Ping:
        user_id: str

    app.register(Ping)

    result = app.push("Ping", {"user_id": "alice", "extra_garbage": "lol"})
    _assert_push_error(result, "unknown_field_v0")


# ---------------------------------------------------------------------------
# 6. Null against non-nullable → rejected (invalid_event)
# ---------------------------------------------------------------------------


def test_push_with_null_against_non_nullable(app):
    """``amount: float`` (required) + ``{"amount": None}`` → ``invalid_event``.

    Python ``None`` → JSON ``null`` → ``Value::Null``. The numeric arm of
    ``value_type_compatible`` is
    ``matches!(val, Value::I64(_) | Value::F64(_))`` — ``Value::Null`` is
    not in that arm → false → ``invalid_event``. SDK does NOT raise.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    app.register(Tx)

    result = app.push("Tx", {"user_id": "alice", "amount": None})
    _assert_push_error(result, "invalid_event")
