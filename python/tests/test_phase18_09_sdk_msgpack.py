"""Phase 18 Plan 09 — Python SDK msgpack TCP push tests.

Task 9.8: TcpTransport.send_push(event_name, body_dict, wire_format='msgpack')
sends a CT_MSGPACK framed envelope over the TCP transport.

RED: TcpTransport has no send_push method yet — tests fail with AttributeError.
"""

from __future__ import annotations

import json
import struct
import threading
from socket import AF_INET, SOCK_STREAM, socket

import pytest

# ─── Codec helpers (mirrors _wire.py) ────────────────────────────────────────

OP_PUSH: int = 0x0002
CT_JSON: int = 0x01
CT_MSGPACK: int = 0x02


def encode_frame(op: int, ct: int, payload: bytes) -> bytes:
    length = 2 + 1 + len(payload)
    return struct.pack(">IHB", length, op, ct) + payload


def _recv_frame(sock: socket) -> tuple[int, int, bytes]:
    """Read one frame from sock; return (op, ct, payload)."""
    header = b""
    while len(header) < 4:
        chunk = sock.recv(4 - len(header))
        if not chunk:
            raise RuntimeError("socket closed")
        header += chunk
    (length,) = struct.unpack(">I", header)
    rest = b""
    while len(rest) < length:
        chunk = sock.recv(length - len(rest))
        if not chunk:
            raise RuntimeError("socket closed")
        rest += chunk
    op = struct.unpack(">H", rest[:2])[0]
    ct = rest[2]
    payload = rest[3:]
    return op, ct, payload


# ─── Test: encoding layer (no server needed) ─────────────────────────────────

class TestSendPushEncoding:
    """Unit-tests the send_push encoding without a real server.

    A minimal echo server in a thread receives the frame, records op/ct/payload,
    and sends back a synthetic OP_PUSH ACK.
    """

    def _run_echo_server(
        self,
        server_sock: socket,
        received: list[tuple[int, int, bytes]],
    ) -> None:
        conn, _ = server_sock.accept()
        op, ct, payload = _recv_frame(conn)
        received.append((op, ct, payload))
        # Send a synthetic OP_PUSH ACK
        ack = encode_frame(OP_PUSH, CT_JSON, b'{"ack_lsn":1}')
        conn.sendall(ack)
        conn.close()

    def _start_server(self) -> tuple[socket, int, list[tuple[int, int, bytes]]]:
        srv = socket(AF_INET, SOCK_STREAM)
        srv.bind(("127.0.0.1", 0))
        srv.listen(1)
        port = srv.getsockname()[1]
        received: list[tuple[int, int, bytes]] = []
        t = threading.Thread(target=self._run_echo_server, args=(srv, received), daemon=True)
        t.start()
        return srv, port, received

    def test_send_push_json_encoding(self) -> None:
        """send_push with wire_format='json' sends CT_JSON frame."""
        from beava._transport import TcpTransport

        srv, port, received = self._start_server()
        with TcpTransport(host="127.0.0.1", port=port) as t:
            t.send_push("TxnEvent", {"user_id": "u1", "amount": 42.0}, wire_format="json")
        srv.close()

        assert len(received) == 1, "expected exactly one frame"
        op, ct, payload = received[0]
        assert op == OP_PUSH, f"expected OP_PUSH ({OP_PUSH:#06x}), got {op:#06x}"
        assert ct == CT_JSON, f"expected CT_JSON ({CT_JSON:#04x}), got {ct:#04x}"
        # Payload must be a valid JSON envelope {"event": ..., "body": ...}
        env = json.loads(payload)
        assert env["event"] == "TxnEvent"
        assert env["body"]["user_id"] == "u1"
        assert env["body"]["amount"] == pytest.approx(42.0)

    def test_send_push_msgpack_encoding(self) -> None:
        """send_push with wire_format='msgpack' sends CT_MSGPACK frame."""
        from beava._transport import TcpTransport

        srv, port, received = self._start_server()
        with TcpTransport(host="127.0.0.1", port=port) as t:
            t.send_push("TxnEvent", {"user_id": "u2", "amount": 99.0}, wire_format="msgpack")
        srv.close()

        assert len(received) == 1, "expected exactly one frame"
        op, ct, payload = received[0]
        assert op == OP_PUSH, f"expected OP_PUSH ({OP_PUSH:#06x}), got {op:#06x}"
        assert ct == CT_MSGPACK, f"expected CT_MSGPACK ({CT_MSGPACK:#04x}), got {ct:#04x}"

        # Payload must be a valid msgpack envelope {event: ..., body: ...}
        try:
            import msgpack  # type: ignore[import-untyped]
        except ImportError:
            pytest.skip("msgpack not installed")
        env = msgpack.unpackb(payload, raw=False)
        assert env["event"] == "TxnEvent"
        assert env["body"]["user_id"] == "u2"
        assert abs(env["body"]["amount"] - 99.0) < 1e-9

    def test_send_push_default_is_json(self) -> None:
        """send_push with no wire_format defaults to CT_JSON."""
        from beava._transport import TcpTransport

        srv, port, received = self._start_server()
        with TcpTransport(host="127.0.0.1", port=port) as t:
            t.send_push("Ev", {"x": 1})
        srv.close()

        assert len(received) == 1
        op, ct, payload = received[0]
        assert ct == CT_JSON, f"default should be CT_JSON, got {ct:#04x}"

    def test_send_push_invalid_wire_format_raises(self) -> None:
        """send_push with unknown wire_format raises ValueError."""
        from beava._transport import TcpTransport

        srv, port, _ = self._start_server()
        with TcpTransport(host="127.0.0.1", port=port) as t:
            with pytest.raises(ValueError, match="wire_format"):
                t.send_push("Ev", {"x": 1}, wire_format="avro")
        srv.close()
