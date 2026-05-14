"""Coverage for ``beava._wire`` decoder error paths and the
``parse_register_response`` error branch.

The decoder and ``read_frame`` happy paths are exercised by integration
tests over a real TCP connection; their *error* arms (short buffer,
underflow length, oversize length, unexpected op on the register
response) need to be poked directly. All branches under test are pure
bytes-in / exception-out so no embed-mode binary is required.
"""
from __future__ import annotations

import io
import json
import socket
import struct
from typing import cast

import pytest

from beava._errors import RegistrationError
from beava._wire import (
    CT_JSON,
    OP_ERROR_RESPONSE,
    OP_PING,
    OP_REGISTER,
    Frame,
    FrameTooLarge,
    IncompleteFrame,
    decode_frame,
    encode_frame,
    parse_register_response,
    read_frame,
)

# ---------------------------------------------------------------------------
# decode_frame — short / underflow / oversize / partial-payload error arms
# ---------------------------------------------------------------------------


def test_decode_frame_buffer_too_short_for_length_prefix() -> None:
    """A buffer < 4 bytes has no length prefix — must raise IncompleteFrame."""
    with pytest.raises(IncompleteFrame, match=r"need >=4 bytes"):
        decode_frame(b"\x00\x01")


def test_decode_frame_length_underflow_raises() -> None:
    """A declared length of < 3 cannot cover op(2) + content_type(1) — must
    raise ``IncompleteFrame`` mirroring the server's ``LengthUnderflow``."""
    # Length=2, then 2 bytes to make the buffer "complete-looking" — but the
    # underflow check fires before any payload read happens.
    buf = struct.pack(">I", 2) + b"\x00\x00"
    with pytest.raises(IncompleteFrame, match=r"< 3"):
        decode_frame(buf)


def test_decode_frame_oversize_raises_too_large() -> None:
    """A declared length above ``max_frame_bytes + 3`` must raise
    ``FrameTooLarge`` with the substring ``too_large`` in the message."""
    # max_frame_bytes=10 → limit=13. Declared 100 trips the cap.
    buf = struct.pack(">I", 100)
    with pytest.raises(FrameTooLarge, match="too_large"):
        decode_frame(buf, max_frame_bytes=10)


def test_decode_frame_incomplete_payload_raises() -> None:
    """If the buffer is long enough for the length prefix but doesn't contain
    the full declared payload, must raise ``IncompleteFrame``."""
    # length=10, but only deliver 4 bytes of body.
    buf = struct.pack(">I", 10) + b"\x00\x00\x01ab"
    with pytest.raises(IncompleteFrame, match=r"need 14 bytes"):
        decode_frame(buf)


def test_decode_frame_happy_path_round_trip() -> None:
    """Sanity that the error tests above didn't mask the success path."""
    frame_bytes = encode_frame(OP_PING, CT_JSON, b'{"hello":1}')
    frame = decode_frame(frame_bytes)
    assert frame.op == OP_PING
    assert frame.ct == CT_JSON
    assert frame.payload == b'{"hello":1}'


# ---------------------------------------------------------------------------
# read_frame — socket-shaped error arms (length underflow, oversize)
# ---------------------------------------------------------------------------


class _FakeSocket:
    """Minimal ``socket.recv``-shaped stub for read_frame tests.

    Exposes a single ``recv(n)`` that pulls from a pre-loaded bytes buffer
    in chunks, returning ``b""`` on exhaustion (the close signal that
    ``_recv_exactly`` translates to ``IncompleteFrame``).
    """

    def __init__(self, data: bytes) -> None:
        self._buf = io.BytesIO(data)

    def recv(self, n: int) -> bytes:
        return self._buf.read(n)


def test_read_frame_length_underflow_raises() -> None:
    """``read_frame`` mirrors ``decode_frame`` — declared length < 3 must
    raise ``IncompleteFrame`` before any payload bytes are consumed."""
    fake_sock = cast(socket.socket, _FakeSocket(struct.pack(">I", 1)))
    with pytest.raises(IncompleteFrame, match=r"< 3"):
        read_frame(fake_sock)


def test_read_frame_oversize_raises_too_large() -> None:
    """``read_frame`` enforces ``max_frame_bytes`` independently from
    ``decode_frame`` — declared length above the cap raises
    ``FrameTooLarge``."""
    fake_sock = cast(socket.socket, _FakeSocket(struct.pack(">I", 1000)))
    with pytest.raises(FrameTooLarge, match="too_large"):
        read_frame(fake_sock, max_frame_bytes=10)


def test_read_frame_socket_closed_mid_length_prefix() -> None:
    """Socket closed before delivering 4 length-prefix bytes — must raise
    ``IncompleteFrame`` from ``_recv_exactly``."""
    fake_sock = cast(socket.socket, _FakeSocket(b"\x00\x00"))
    with pytest.raises(IncompleteFrame, match="socket closed"):
        read_frame(fake_sock)


def test_read_frame_happy_path() -> None:
    """Sanity: a complete frame on the wire round-trips through read_frame."""
    payload = b'{"a":1}'
    frame_bytes = encode_frame(OP_REGISTER, CT_JSON, payload)
    fake_sock = cast(socket.socket, _FakeSocket(frame_bytes))
    frame = read_frame(fake_sock)
    assert frame.op == OP_REGISTER
    assert frame.payload == payload


# ---------------------------------------------------------------------------
# parse_register_response — error and unexpected-op arms
# ---------------------------------------------------------------------------


def test_parse_register_response_happy_path() -> None:
    """A successful register frame must round-trip to a dict."""
    body = {"status": "ok", "registry_version": 7}
    frame = Frame(op=OP_REGISTER, ct=CT_JSON, payload=json.dumps(body).encode())
    assert parse_register_response(frame) == body


def test_parse_register_response_error_frame_raises_registration_error() -> None:
    """An ``OP_ERROR_RESPONSE`` frame must lift into ``RegistrationError``
    carrying the server-supplied code/path/message."""
    body = {
        "error": {
            "code": "schema_mismatch",
            "path": "events.Click.fields.amount",
            "reason": "type changed",
        }
    }
    frame = Frame(op=OP_ERROR_RESPONSE, ct=CT_JSON, payload=json.dumps(body).encode())
    with pytest.raises(RegistrationError) as exc_info:
        parse_register_response(frame)
    assert exc_info.value.code == "schema_mismatch"


def test_parse_register_response_unexpected_op_raises() -> None:
    """A frame whose op is neither ``OP_REGISTER`` nor ``OP_ERROR_RESPONSE``
    must raise ``RegistrationError`` with code ``unexpected_frame``."""
    frame = Frame(op=0x1234, ct=CT_JSON, payload=b"{}")
    with pytest.raises(RegistrationError) as exc_info:
        parse_register_response(frame)
    assert exc_info.value.code == "unexpected_frame"
    assert "0x1234" in str(exc_info.value)
