"""Coverage for type mismatch at push time.

The audit flagged no test asserts what the server does when a push payload's
field types disagree with the declared schema. These tests pin the
contract: server-side validation failures **raise**
:exc:`RegistrationError` on the SDK side over TCP / TCP-embed.

# BUG FIXED (2026-05-14)

Previously the Python SDK's TCP transport
``_transport.py::TcpTransport.send_push`` did not check the response
frame's opcode. The server emits ``OP_ERROR_RESPONSE`` (0xFFFF) on push
validation failures, but the SDK happily parsed that as JSON and
returned ``{"error": {...}, "registry_version": N}`` to user code — no
exception, no signal that the push was rejected.

``send_push`` now mirrors ``send_get``: it checks
``frame.op != OP_PUSH`` and raises :exc:`RegistrationError` with the
parsed ``error.code`` whenever the server returns anything other than
an echoed ``OP_PUSH`` ack. Fire-and-forget callers that previously
silently dropped events on validation failures now surface those
failures as exceptions.

The HTTP transport path at ``_transport.py:156-192`` was already
correct (it raises on non-2xx).

# Contract pinned (verified by reading the impl)

1. Client-side validation: the Python SDK does **not** validate payload
   types in :py:meth:`bv.App.push`. ``_app.py:585-594`` forwards directly
   to ``transport.send_push``; ``_transport.py:443-490`` JSON-encodes
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
   "idempotent_replay": bool}`` (echoed back on ``OP_PUSH``).
   Error response (over TCP-embed): ``OP_ERROR_RESPONSE`` with body
   ``{"error": {"code": "..."}, "registry_version": int}`` — now
   raises :exc:`RegistrationError` with the matching ``code``.
"""

from __future__ import annotations

import json
import socket
import threading
from typing import Any

import pytest

import beava as bv
from beava._errors import RegistrationError
from beava._transport import TcpTransport
from beava._wire import (
    CT_JSON,
    OP_ERROR_RESPONSE,
    encode_frame,
    read_frame,
)


@pytest.fixture
def app(beava_binary):  # noqa: ARG001 — pulls the cargo-build side-effect
    """Fresh embed-mode ``bv.App(test_mode=True)`` per test."""
    with bv.App(test_mode=True) as instance:
        yield instance


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
    returns false → server emits ``invalid_event``. The SDK raises
    :exc:`RegistrationError` with that code.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    app.register(Tx)

    with pytest.raises(RegistrationError) as exc_info:
        app.push("Tx", {"user_id": "alice", "amount": "abc"})
    assert exc_info.value.code == "invalid_event"


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
    silent stringification ("42") happens. SDK raises
    :exc:`RegistrationError` with code ``invalid_event``.
    """

    @bv.event
    class Click:
        user_id: str
        page: str

    app.register(Click)

    with pytest.raises(RegistrationError) as exc_info:
        app.push("Click", {"user_id": 42, "page": "/home"})
    assert exc_info.value.code == "invalid_event"


# ---------------------------------------------------------------------------
# 4. Missing required field → rejected (invalid_event)
# ---------------------------------------------------------------------------


def test_push_with_missing_required_field(app):
    """Schema declares 3 fields, push 2 → ``invalid_event``.

    ``validate_row_against_descriptor`` walks ``descriptor.schema.fields``;
    the first missing required field (``row.get(field_name) == None``)
    returns false → server rejects. SDK raises :exc:`RegistrationError`.
    """

    @bv.event
    class Order:
        user_id: str
        item_id: str
        amount: float

    app.register(Order)

    # Missing `amount`.
    with pytest.raises(RegistrationError) as exc_info:
        app.push("Order", {"user_id": "alice", "item_id": "sku-1"})
    assert exc_info.value.code == "invalid_event"


# ---------------------------------------------------------------------------
# 5. Unknown field → rejected (unknown_field_v0)
# ---------------------------------------------------------------------------


def test_push_with_unknown_field(app):
    """Push includes a field not in schema → ``unknown_field_v0``.

    The strict-deny pass at ``apply_shard.rs:856-875`` runs before type
    validation, so the error code is ``unknown_field_v0`` (not
    ``invalid_event``). ``event_time`` / ``event_time_ms`` get the
    special-case ``unknown_field_event_time_v0`` code instead; this test
    pins the general case. SDK raises :exc:`RegistrationError`.
    """

    @bv.event
    class Ping:
        user_id: str

    app.register(Ping)

    with pytest.raises(RegistrationError) as exc_info:
        app.push("Ping", {"user_id": "alice", "extra_garbage": "lol"})
    assert exc_info.value.code == "unknown_field_v0"


# ---------------------------------------------------------------------------
# 6. Null against non-nullable → rejected (invalid_event)
# ---------------------------------------------------------------------------


def test_push_with_null_against_non_nullable(app):
    """``amount: float`` (required) + ``{"amount": None}`` → ``invalid_event``.

    Python ``None`` → JSON ``null`` → ``Value::Null``. The numeric arm of
    ``value_type_compatible`` is
    ``matches!(val, Value::I64(_) | Value::F64(_))`` — ``Value::Null`` is
    not in that arm → false → ``invalid_event``. SDK raises
    :exc:`RegistrationError`.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    app.register(Tx)

    with pytest.raises(RegistrationError) as exc_info:
        app.push("Tx", {"user_id": "alice", "amount": None})
    assert exc_info.value.code == "invalid_event"


# ---------------------------------------------------------------------------
# 7. Unexpected response opcode → SDK raises RegistrationError
# ---------------------------------------------------------------------------


def test_push_response_unexpected_opcode_raises():
    """Mock TCP server returns a non-OP_PUSH, non-error frame → raise.

    Mirrors ``send_get``'s unexpected-opcode behaviour: any frame whose
    op is neither the success op (``OP_PUSH``) nor a recognised error
    is surfaced as :exc:`RegistrationError`. Catches future server
    bugs that emit a stray opcode instead of silently returning a
    dict to user code.
    """

    # In-process TCP echo that replies with an unexpected opcode.
    bogus_op = 0x1234  # not OP_PUSH, not OP_ERROR_RESPONSE
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.bind(("127.0.0.1", 0))
    server.listen(1)
    host, port = server.getsockname()

    def serve():
        conn, _ = server.accept()
        try:
            # Read the inbound OP_PUSH frame and discard it.
            read_frame(conn, 4 * 1024 * 1024)
            body = json.dumps({"weird": "shape"}).encode("utf-8")
            conn.sendall(encode_frame(bogus_op, CT_JSON, body))
        finally:
            conn.close()
            server.close()

    t = threading.Thread(target=serve, daemon=True)
    t.start()
    try:
        transport = TcpTransport(host=host, port=port)
        try:
            with pytest.raises(RegistrationError) as exc_info:
                transport.send_push(event_name="X", fields={"a": 1})
            # No "error.code" in body → fallback code.
            assert exc_info.value.code == "unexpected_frame"
            # The diagnostic message embeds the actual opcode we sent.
            assert f"{bogus_op:#06x}" in str(exc_info.value)
        finally:
            transport.close()
    finally:
        t.join(timeout=2.0)


# ---------------------------------------------------------------------------
# 8. OP_ERROR_RESPONSE with malformed (non-JSON) body → fallback code
# ---------------------------------------------------------------------------


def test_push_error_response_with_unparseable_body_raises():
    """Server returns ``OP_ERROR_RESPONSE`` but the body isn't JSON.

    Belt-and-suspenders for the json.loads guard inside send_push: the
    SDK must still raise (never return) and must not crash on bad
    bytes. The fallback code path lands on ``unexpected_frame``.
    """

    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.bind(("127.0.0.1", 0))
    server.listen(1)
    host, port = server.getsockname()

    def serve():
        conn, _ = server.accept()
        try:
            read_frame(conn, 4 * 1024 * 1024)
            conn.sendall(encode_frame(OP_ERROR_RESPONSE, CT_JSON, b"\xff\xfe garbage"))
        finally:
            conn.close()
            server.close()

    t = threading.Thread(target=serve, daemon=True)
    t.start()
    try:
        transport = TcpTransport(host=host, port=port)
        try:
            with pytest.raises(RegistrationError) as exc_info:
                transport.send_push(event_name="X", fields={"a": 1})
            assert exc_info.value.code == "unparseable_error" or (
                exc_info.value.code == "unexpected_frame"
            )
        finally:
            transport.close()
    finally:
        t.join(timeout=2.0)

