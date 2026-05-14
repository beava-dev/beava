"""TCP multi-connection consistency tests for the binary-framed wire.

Closes an audit gap: single-connection TCP push/get had coverage, but no
test asserted (a) strict-FIFO across pipelined writes on ONE connection,
(b) cross-connection visibility — a push on conn-A immediately seen by a
get on conn-B (the server's single-threaded apply loop guarantees this),
or (c) connection survival after an OP_ERROR_RESPONSE.

All tests register a tiny ``Txn``/``TxnAgg`` schema over HTTP (the simpler
control plane) then exercise the data plane purely over TCP. Each test
uses the per-test ``beava_server`` fixture so the registry/WAL is fresh.
"""

from __future__ import annotations

import json
import socket
from typing import Any

import httpx
import pytest

from beava._transport import TcpTransport
from beava._wire import (
    CT_JSON,
    OP_ERROR_RESPONSE,
    OP_GET,
    OP_GET_RESPONSE,
    OP_PUSH,
    encode_frame,
    read_frame,
)

# ---------------------------------------------------------------------------
# Fixtures / helpers
# ---------------------------------------------------------------------------


# Wire-shape register payload — a single ``Txn`` event source and a
# windowed ``TxnAgg`` aggregation keyed by ``user_id``. Hand-built JSON
# matching the server's register-deserializer contract so the test does
# not depend on SDK decorator behaviour.
_REGISTER_PAYLOAD: dict[str, Any] = {
    "nodes": [
        {
            "kind": "event",
            "name": "Txn",
            "schema": {
                "fields": {"user_id": "str", "amount": "f64"},
                "optional_fields": [],
            },
            "dedupe_key": None,
            "dedupe_window_ms": None,
            "keep_events_for_ms": None,
        },
        {
            "kind": "derivation",
            "name": "TxnAgg",
            "output_kind": "table",
            "upstreams": ["Txn"],
            "ops": [
                {
                    "op": "group_by",
                    "keys": ["user_id"],
                    "agg": {
                        "cnt": {"op": "count", "params": {}},
                        "total": {
                            "op": "sum",
                            "params": {"field": "amount"},
                        },
                    },
                }
            ],
            "schema": {
                "fields": {"user_id": "str", "cnt": "i64", "total": "f64"},
                "optional_fields": [],
            },
            "table_primary_key": ["user_id"],
        },
    ]
}


def _parse_tcp_url(tcp_url: str) -> tuple[str, int]:
    """``tcp://host:port`` → ``(host, port)``."""
    assert tcp_url.startswith("tcp://"), tcp_url
    host_port = tcp_url[len("tcp://") :]
    host, port_str = host_port.rsplit(":", 1)
    return host, int(port_str)


def _register(http_url: str) -> None:
    """POST the canonical Txn / TxnAgg register payload."""
    resp = httpx.post(
        f"{http_url}/register",
        content=json.dumps(_REGISTER_PAYLOAD).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )
    assert resp.status_code == 200, f"register failed: {resp.status_code} {resp.text}"


def _encode_push(event: str, body: dict[str, Any]) -> bytes:
    """Build an OP_PUSH JSON frame ready for sendall()."""
    envelope = json.dumps({"event": event, "body": body}).encode("utf-8")
    return encode_frame(OP_PUSH, CT_JSON, envelope)


def _encode_get(table: str, key: str) -> bytes:
    """Build an OP_GET JSON frame ready for sendall()."""
    body = json.dumps({"table": table, "key": key}).encode("utf-8")
    return encode_frame(OP_GET, CT_JSON, body)


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


class TestSingleConnectionPushGet:
    def test_single_conn_push_then_get_returns_pushed_event(
        self, beava_server: tuple[str, str]
    ) -> None:
        """Baseline: one push, one get on the SAME TCP connection — get sees the push."""
        http_url, tcp_url = beava_server
        _register(http_url)
        host, port = _parse_tcp_url(tcp_url)

        with TcpTransport(host=host, port=port) as t:
            ack = t.send_push(event_name="Txn", fields={"user_id": "alice", "amount": 10.0})
            assert "ack_lsn" in ack, f"unexpected push ack: {ack!r}"
            row = t.send_get(table="TxnAgg", key="alice")

        assert row.get("cnt") == 1, f"expected cnt=1 after one push, got {row!r}"
        assert row.get("total") == pytest.approx(10.0), f"unexpected total: {row!r}"

    def test_single_conn_pipelined_pushes_then_get_are_strict_fifo(
        self, beava_server: tuple[str, str]
    ) -> None:
        """Pipeline 50 pushes back-to-back over ONE socket, then read 50 acks,
        then a get on the same socket — final state must reflect all 50 pushes.

        Strict-FIFO contract: the server processes frames on a single
        connection in arrival order. The 51st frame (get) MUST be applied
        after the 50 pushes, so cnt == 50 and total == sum(1..=50).
        """
        http_url, tcp_url = beava_server
        _register(http_url)
        host, port = _parse_tcp_url(tcp_url)

        N = 50
        sock = socket.create_connection((host, port), timeout=10.0)
        try:
            # Pipeline: concatenate N push frames + 1 get frame in one sendall.
            pushes = b"".join(
                _encode_push("Txn", {"user_id": "alice", "amount": float(i + 1)})
                for i in range(N)
            )
            get = _encode_get("TxnAgg", "alice")
            sock.sendall(pushes + get)

            # Drain N push acks in order.
            for i in range(N):
                frame = read_frame(sock)
                assert frame.op == OP_PUSH, (
                    f"push #{i}: expected OP_PUSH ack ({OP_PUSH:#06x}), "
                    f"got op={frame.op:#06x}"
                )
                ack = json.loads(frame.payload.decode("utf-8"))
                assert "ack_lsn" in ack, f"push #{i}: ack missing ack_lsn: {ack!r}"

            # The final response is the get.
            get_frame = read_frame(sock)
            assert get_frame.op == OP_GET_RESPONSE, (
                f"expected OP_GET_RESPONSE ({OP_GET_RESPONSE:#06x}), "
                f"got op={get_frame.op:#06x}"
            )
            row = json.loads(get_frame.payload.decode("utf-8"))
        finally:
            sock.close()

        # cnt counts events; total sums amounts 1..=N.
        expected_total = float(N * (N + 1) // 2)
        assert row.get("cnt") == N, f"FIFO violation: cnt={row.get('cnt')!r}, expected {N}"
        assert row.get("total") == pytest.approx(expected_total), (
            f"FIFO violation: total={row.get('total')!r}, expected {expected_total}"
        )


class TestTwoConnectionVisibility:
    def test_two_conns_push_a_get_b_sees_it(
        self, beava_server: tuple[str, str]
    ) -> None:
        """Push on conn-A; get on conn-B sees the push.

        The server's single-threaded apply loop serialises pushes; once
        conn-A receives an ack the state mutation is committed and any
        subsequent get on ANY connection must observe it.
        """
        http_url, tcp_url = beava_server
        _register(http_url)
        host, port = _parse_tcp_url(tcp_url)

        with (
            TcpTransport(host=host, port=port) as conn_a,
            TcpTransport(host=host, port=port) as conn_b,
        ):
            ack = conn_a.send_push(
                event_name="Txn", fields={"user_id": "bob", "amount": 7.5}
            )
            assert "ack_lsn" in ack, f"unexpected ack: {ack!r}"
            # Sanity: two distinct sockets.
            assert conn_a._socket is not conn_b._socket
            row = conn_b.send_get(table="TxnAgg", key="bob")

        assert row.get("cnt") == 1, f"conn-B did not see conn-A's push: {row!r}"
        assert row.get("total") == pytest.approx(7.5), f"unexpected total: {row!r}"

    def test_two_conns_interleaved_pushes_atomic_state(
        self, beava_server: tuple[str, str]
    ) -> None:
        """Alternate pushes A, B, A, B, ... 20 each; final get sees cnt == 40.

        Each push is fully round-tripped (send + read ack) before the next,
        so this is a strict interleaving — but the test still proves that
        the server's apply loop merges writes from two connections into a
        single atomic state without loss or double-counting.
        """
        http_url, tcp_url = beava_server
        _register(http_url)
        host, port = _parse_tcp_url(tcp_url)

        N_PER_CONN = 20
        with (
            TcpTransport(host=host, port=port) as conn_a,
            TcpTransport(host=host, port=port) as conn_b,
        ):
            for _ in range(N_PER_CONN):
                conn_a.send_push(
                    event_name="Txn",
                    fields={"user_id": "carol", "amount": 1.0},
                )
                conn_b.send_push(
                    event_name="Txn",
                    fields={"user_id": "carol", "amount": 1.0},
                )
            # Read final state from either connection.
            row = conn_a.send_get(table="TxnAgg", key="carol")

        total_pushes = 2 * N_PER_CONN
        assert row.get("cnt") == total_pushes, (
            f"interleaved pushes lost or duplicated: cnt={row.get('cnt')!r}, "
            f"expected {total_pushes}"
        )
        assert row.get("total") == pytest.approx(float(total_pushes)), (
            f"unexpected total: {row!r}"
        )


class TestConnectionSurvivesError:
    def test_connection_survives_after_error_response(
        self, beava_server: tuple[str, str]
    ) -> None:
        """Push an unknown event (OP_ERROR_RESPONSE, code=event_not_found),
        then push a valid event on the SAME connection — the second succeeds.

        The wire docs guarantee the server keeps the connection open after
        an error frame; this test pins that contract from the client side.
        """
        http_url, tcp_url = beava_server
        _register(http_url)
        host, port = _parse_tcp_url(tcp_url)

        sock = socket.create_connection((host, port), timeout=10.0)
        try:
            # Frame 1: push for an unregistered event — server must reply
            # OP_ERROR_RESPONSE without closing the socket.
            sock.sendall(_encode_push("NoSuchEvent", {"x": 1}))
            err = read_frame(sock)
            assert err.op == OP_ERROR_RESPONSE, (
                f"expected OP_ERROR_RESPONSE ({OP_ERROR_RESPONSE:#06x}), "
                f"got op={err.op:#06x}"
            )
            err_body = json.loads(err.payload.decode("utf-8"))
            assert err_body.get("error", {}).get("code") == "event_not_found", (
                f"unexpected error body: {err_body!r}"
            )

            # Frame 2: valid push on the SAME socket — must succeed.
            sock.sendall(
                _encode_push("Txn", {"user_id": "dave", "amount": 3.0})
            )
            ack_frame = read_frame(sock)
            assert ack_frame.op == OP_PUSH, (
                f"connection died after error: expected OP_PUSH ack, "
                f"got op={ack_frame.op:#06x}"
            )
            ack = json.loads(ack_frame.payload.decode("utf-8"))
            assert "ack_lsn" in ack, f"valid push ack missing ack_lsn: {ack!r}"

            # Frame 3: get on the same socket — confirms the second push
            # actually mutated state (i.e. it wasn't silently swallowed
            # after the error frame).
            sock.sendall(_encode_get("TxnAgg", "dave"))
            get_frame = read_frame(sock)
            assert get_frame.op == OP_GET_RESPONSE, (
                f"expected OP_GET_RESPONSE, got op={get_frame.op:#06x}"
            )
            row = json.loads(get_frame.payload.decode("utf-8"))
        finally:
            sock.close()

        assert row.get("cnt") == 1, f"second push lost after error: {row!r}"
        assert row.get("total") == pytest.approx(3.0), f"unexpected total: {row!r}"
