"""Wire-protocol codec for the binary-framed TCP transport.

Frame envelope::

    [u32 length BE][u16 op BE][u8 content_type][payload: length - 3 bytes]

``length`` is the byte count of ``op + content_type + payload`` (not
counting the length field itself); the minimum valid length is 3 (empty
payload). All multi-byte integers are big-endian (network byte order).

Opcode constants (client-initiated set):

    OP_PING           = 0x0000
    OP_REGISTER       = 0x0001
    OP_PUSH           = 0x0010
    OP_GET            = 0x0020
    OP_GET_RESPONSE   = 0x0023
    OP_BATCH_GET      = 0x0024
    OP_RESET          = 0x0040  (test-mode-gated)
    OP_ERROR_RESPONSE = 0xFFFF

Content types:

    CT_JSON    = 0x01
    CT_MSGPACK = 0x02  (read-path fast-path)
"""

from __future__ import annotations

import json
import socket
import struct
from dataclasses import dataclass
from typing import Any, cast

OP_PING: int = 0x0000
OP_REGISTER: int = 0x0001
OP_PUSH: int = 0x0010

OP_GET: int = 0x0020
OP_MGET: int = 0x0021
OP_GET_MULTI: int = 0x0022
OP_GET_RESPONSE: int = 0x0023
OP_BATCH_GET: int = 0x0024
OP_RESET: int = 0x0040

OP_ERROR_RESPONSE: int = 0xFFFF

CT_JSON: int = 0x01
CT_MSGPACK: int = 0x02

#: Default max payload size — matches the server's
#: ``DEFAULT_TCP_MAX_FRAME_BYTES`` (4 MiB).
MAX_FRAME_BYTES: int = 4 * 1024 * 1024


class FrameTooLarge(Exception):
    """Raised when a declared frame length exceeds ``MAX_FRAME_BYTES``.

    The message always contains ``'too_large'`` so callers (and the
    decoder's own tests) can assert on the substring.
    """


class IncompleteFrame(Exception):
    """Raised when the buffer does not contain a complete frame."""


@dataclass
class Frame:
    """A decoded TCP wire frame."""

    op: int
    ct: int
    payload: bytes


def encode_frame(op: int, ct: int, payload: bytes) -> bytes:
    """Encode a frame to bytes.

    Layout: [u32 length BE][u16 op BE][u8 ct][payload]
    where length = 2 (op) + 1 (ct) + len(payload).

    Args:
        op: Opcode (u16).
        ct: Content type (u8).
        payload: Raw payload bytes.

    Returns:
        Complete frame bytes ready for sendall().
    """
    length = 2 + 1 + len(payload)
    header = struct.pack(">IHB", length, op, ct)
    return header + payload


def decode_frame(buf: bytes, max_frame_bytes: int = MAX_FRAME_BYTES) -> Frame:
    """Decode a frame from a bytes buffer.

    The buffer must contain the full frame (header + payload). Use
    :func:`read_frame` to read incrementally from a socket.

    Args:
        buf: Bytes containing at least one complete frame (trailing data
            is permitted but ignored).
        max_frame_bytes: Maximum allowed payload size. Defaults to
            ``MAX_FRAME_BYTES`` (4 MiB).

    Returns:
        Decoded :class:`Frame`.

    Raises:
        IncompleteFrame: Buffer is too short to decode a complete frame.
        FrameTooLarge: Declared length exceeds ``max_frame_bytes``.
    """
    if len(buf) < 4:
        raise IncompleteFrame(f"need >=4 bytes for length prefix; got {len(buf)}")

    (length,) = struct.unpack(">I", buf[:4])

    # A length of less than 3 cannot cover op(2) + content_type(1) and is
    # always malformed; mirrors the server's FrameError::LengthUnderflow.
    if length < 3:
        raise IncompleteFrame(
            f"frame length {length} < 3: cannot cover op(2) + content_type(1)"
        )

    limit = max_frame_bytes + 3  # 3 = op(2) + ct(1)
    if length > limit:
        raise FrameTooLarge(
            f"frame too_large: declared_length={length} exceeds limit={limit} "
            f"(max_frame_bytes={max_frame_bytes})"
        )

    total_needed = 4 + length
    if len(buf) < total_needed:
        raise IncompleteFrame(
            f"need {total_needed} bytes; got {len(buf)} "
            f"(declared_length={length})"
        )

    op = struct.unpack(">H", buf[4:6])[0]
    ct = buf[6]
    payload = buf[7:total_needed]
    return Frame(op=op, ct=ct, payload=payload)


def _recv_exactly(sock: socket.socket, n: int) -> bytes:
    """Read exactly ``n`` bytes from ``sock``, looping until all arrive."""
    chunks: list[bytes] = []
    remaining = n
    while remaining > 0:
        chunk = sock.recv(remaining)
        if not chunk:
            raise IncompleteFrame(
                f"socket closed after {n - remaining}/{n} bytes"
            )
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_frame(sock: socket.socket, max_frame_bytes: int = MAX_FRAME_BYTES) -> Frame:
    """Read exactly one frame from a connected socket.

    Blocks until the complete frame is available; partial reads are handled
    by looping over ``socket.recv``.

    Args:
        sock: Connected TCP socket.
        max_frame_bytes: Maximum allowed payload size.

    Returns:
        Decoded :class:`Frame`.

    Raises:
        IncompleteFrame: Socket closed before a complete frame was received.
        FrameTooLarge: Declared length exceeds ``max_frame_bytes``.
    """
    len_bytes = _recv_exactly(sock, 4)
    (length,) = struct.unpack(">I", len_bytes)

    if length < 3:
        raise IncompleteFrame(
            f"frame length {length} < 3: cannot cover op(2) + content_type(1)"
        )

    limit = max_frame_bytes + 3
    if length > limit:
        raise FrameTooLarge(
            f"frame too_large: declared_length={length} exceeds limit={limit} "
            f"(max_frame_bytes={max_frame_bytes})"
        )

    rest = _recv_exactly(sock, length)
    op = struct.unpack(">H", rest[:2])[0]
    ct = rest[2]
    payload = rest[3:]
    return Frame(op=op, ct=ct, payload=payload)


def parse_register_response(frame: Frame) -> dict[str, Any]:
    """Parse a register response frame into a dict or raise.

    Args:
        frame: Response frame from the server.

    Returns:
        Parsed JSON body dict on success (``OP_REGISTER`` with
        ``status='ok'``).

    Raises:
        RegistrationError: Server returned ``OP_ERROR_RESPONSE`` or an
            unexpected op.
    """
    # Local import avoids the circular load order
    # _errors ← _wire ← _transport.
    from beava._errors import RegistrationError

    body: Any = json.loads(frame.payload.decode("utf-8"))

    if frame.op == OP_REGISTER:
        return cast(dict[str, Any], body)

    if frame.op == OP_ERROR_RESPONSE:
        error = body.get("error", {})
        raise RegistrationError(
            code=error.get("code", "unknown"),
            path=error.get("path", ""),
            message=error.get("reason") or error.get("message", ""),
            errors=[],
        )

    raise RegistrationError(
        code="unexpected_frame",
        message=f"expected OP_REGISTER (0x0001) or OP_ERROR_RESPONSE (0xFFFF); "
                f"got op={frame.op:#06x}",
    )
