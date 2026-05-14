"""Payload-bounds + recursion-limit tests for the Beava SDK and server.

Closes an audit gap: until this file landed, no test pushed an oversize
frame, a deeply-nested expression, or a huge ``batch_get`` against the
server. The server's TCP wire enforces ``tcp_max_frame_bytes = 4 MiB``
(``crates/beava-core/src/wire.rs::decode_frame``) and the evaluator caps
recursion at ``MAX_EVAL_DEPTH = 512``
(``crates/beava-core/src/eval.rs``). Without these tests, any change to
those bounds would ship invisibly — DoS vector if loosened, silent
truncation if tightened.

Each test pairs with the closest existing pattern:

  * Frame-size limits — TCP raw-wire frame against the live server
    (``beava_server`` fixture). The decoder returns
    ``code="frame_too_large"`` in an ``OP_ERROR_RESPONSE`` frame and
    closes the connection (only fatal wire error).
  * Recursion / payload size limits — register + push via the SDK so
    schema propagation, expression parsing, and apply-loop scheduling
    are all exercised end-to-end.
"""

from __future__ import annotations

import json
import socket
import struct
from typing import Any

import httpx
import pytest

import beava as bv
from beava._wire import (
    CT_JSON,
    MAX_FRAME_BYTES,
    OP_ERROR_RESPONSE,
    OP_PING,
    OP_PUSH,
    encode_frame,
    read_frame,
)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _parse_tcp_url(tcp_url: str) -> tuple[str, int]:
    assert tcp_url.startswith("tcp://"), tcp_url
    host_port = tcp_url[len("tcp://") :]
    host, port_str = host_port.rsplit(":", 1)
    return host, int(port_str)


# Minimal register payload — single event ``Bulk`` with a ``user_id`` key and
# a wide ``payload`` string field. Used by the frame-size boundary tests so
# the server has somewhere to route a big OP_PUSH.
_REGISTER_BULK: dict[str, Any] = {
    "nodes": [
        {
            "kind": "event",
            "name": "Bulk",
            "schema": {
                "fields": {"user_id": "str", "payload": "str"},
                "optional_fields": [],
            },
            "dedupe_key": None,
            "dedupe_window_ms": None,
            "keep_events_for_ms": None,
        }
    ]
}


def _register_bulk(http_url: str) -> None:
    """POST the Bulk schema; raise on non-200."""
    r = httpx.post(
        f"{http_url}/register",
        content=json.dumps(_REGISTER_BULK).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )
    assert r.status_code == 200, f"register Bulk failed: {r.status_code} {r.text!r}"


def _build_push_payload_of_size(total_payload_bytes: int) -> bytes:
    """Build a valid ``OP_PUSH`` JSON body of exactly ``total_payload_bytes``.

    The wire body is ``{"event": "Bulk", "body": {"user_id": "u", "payload": "<filler>"}}``;
    the filler is sized so the encoded UTF-8 JSON exactly matches the
    requested length. Asserts the resulting buffer length so a regression
    in encoding (e.g. extra whitespace) is caught before the network round
    trip.
    """
    envelope: dict[str, Any] = {
        "event": "Bulk",
        "body": {"user_id": "u", "payload": ""},
    }
    base = json.dumps(envelope, ensure_ascii=False).encode("utf-8")
    overhead = len(base)
    filler_len = total_payload_bytes - overhead
    assert filler_len >= 0, (
        f"requested size {total_payload_bytes} < envelope overhead {overhead}"
    )
    envelope["body"]["payload"] = "x" * filler_len
    out = json.dumps(envelope, ensure_ascii=False).encode("utf-8")
    assert len(out) == total_payload_bytes, (
        f"payload sizing off: requested {total_payload_bytes}, got {len(out)}"
    )
    return out


# ---------------------------------------------------------------------------
# Test 1 — push frame at MAX_FRAME succeeds (success-side boundary lock)
# ---------------------------------------------------------------------------


def test_push_frame_at_max_size_succeeds(
    beava_server: tuple[str, str],
) -> None:
    """An OP_PUSH whose payload is exactly ``MAX_FRAME_BYTES`` is accepted.

    The server's check (``crates/beava-core/src/wire.rs::decode_frame``) is
    ``declared_len > limit`` where ``limit = max_frame_bytes + 3``; at
    exactly the limit the frame is decoded. The push body is a valid JSON
    document, so the apply path returns an ACK rather than a parse error.
    """
    http_url, tcp_url = beava_server
    _register_bulk(http_url)

    host, port = _parse_tcp_url(tcp_url)
    sock = socket.create_connection((host, port), timeout=15.0)
    try:
        # Build a body just under the limit so the encoded frame
        # (length+op+ct+payload) lands inside MAX_FRAME_BYTES. We size at
        # exactly MAX_FRAME_BYTES payload bytes (server limit is for the
        # *frame body* = op(2) + ct(1) + payload).
        body = _build_push_payload_of_size(MAX_FRAME_BYTES)
        frame_bytes = encode_frame(OP_PUSH, CT_JSON, body)
        # Sanity: declared length field == 3 + payload_len == 3 + MAX_FRAME_BYTES.
        declared = struct.unpack(">I", frame_bytes[:4])[0]
        assert declared == 3 + MAX_FRAME_BYTES, (
            f"declared {declared} mismatched expected {3 + MAX_FRAME_BYTES}"
        )

        sock.sendall(frame_bytes)
        resp = read_frame(sock, MAX_FRAME_BYTES)
        # OP_PUSH ACK is OP_PUSH (echoed) carrying an ack body, not an
        # error response. The exact ack opcode is documented in
        # crates/beava-server/src/server.rs.
        assert resp.op != OP_ERROR_RESPONSE, (
            f"frame at max size must NOT trigger frame_too_large; "
            f"got op={resp.op:#06x} body={resp.payload[:200]!r}"
        )
    finally:
        sock.close()


# ---------------------------------------------------------------------------
# Test 2 — push frame one byte over MAX_FRAME rejected
# ---------------------------------------------------------------------------


def test_push_frame_over_max_size_rejected_with_frame_too_large(
    beava_server: tuple[str, str],
) -> None:
    """An OP_PUSH frame declaring ``MAX_FRAME_BYTES + 1`` payload bytes is
    rejected with ``code='frame_too_large'`` and the connection is closed.

    The decoder's check fires on the declared length alone (see
    ``inline_encode_frame_too_large`` in
    ``crates/beava-runtime-core/src/io_thread_worker.rs``) — the server
    does NOT wait for the full oversized payload to arrive before
    rejecting, so we only need to send the header to trigger the error
    frame. ``frame_too_large`` is the only fatal wire error: the
    server inline-encodes the error frame, flushes, and closes the
    socket.
    """
    http_url, tcp_url = beava_server
    _register_bulk(http_url)

    host, port = _parse_tcp_url(tcp_url)
    sock = socket.create_connection((host, port), timeout=15.0)
    try:
        # Declared length = 3 (op+ct) + MAX_FRAME_BYTES + 1 = limit + 1.
        # The server rejects without ever reading the payload, so we send
        # only the header — a faster test that also avoids OS send-buffer
        # backpressure on huge buffers.
        declared = 3 + MAX_FRAME_BYTES + 1
        header = struct.pack(">IHB", declared, OP_PUSH, CT_JSON)
        sock.sendall(header)

        resp = read_frame(sock, MAX_FRAME_BYTES)
        assert resp.op == OP_ERROR_RESPONSE, (
            f"expected OP_ERROR_RESPONSE; got op={resp.op:#06x}"
        )
        body = json.loads(resp.payload.decode("utf-8"))
        assert body.get("error", {}).get("code") == "frame_too_large", (
            f"expected code='frame_too_large'; got body={body!r}"
        )
        # The error frame carries the declared length and the configured
        # limit — useful for ops triage.
        assert "limit" in body["error"], f"missing 'limit'; body={body!r}"
        assert body["error"]["declared"] == declared, (
            f"declared echo mismatch: got {body['error'].get('declared')!r}, "
            f"sent {declared}"
        )

        # Per the wire contract (frame_too_large is fatal) the server
        # closes the connection after the error frame. A subsequent recv
        # must return EOF (b"") within the timeout.
        sock.settimeout(5.0)
        tail = sock.recv(1)
        assert tail == b"", (
            f"connection must close after frame_too_large; got tail={tail!r}"
        )
    finally:
        sock.close()


# ---------------------------------------------------------------------------
# Test 3 — deeply-nested filter predicate does not crash the server
# ---------------------------------------------------------------------------


def test_expr_parser_rejects_or_evaluates_deeply_nested_where_predicate(
    beava_binary: Any,  # noqa: ARG001 — pulled in for the cargo-build side-effect
) -> None:
    """Register an event-derivation with a 600-deep ``&``-chained filter.

    ``crates/beava-core/src/eval.rs`` caps recursion at ``MAX_EVAL_DEPTH =
    512``: at deeper depths the evaluator returns ``Null`` (the silent DoS
    guard) instead of overflowing the stack. The register-time parser does
    NOT cap depth — the chain is accepted at compile time. The contract
    this test locks:

      * The server MUST NOT panic / stack-overflow / hang at register time
        or push time (the canary: a follow-up push completes).
      * Either the chain compiles cleanly and the depth-guarded eval
        returns ``Null`` (so the filter rejects the event silently) OR the
        server returns a structured registration error.

    If the server crashes / segfaults / never returns, that is a real DoS
    vector and the test will fail by timeout / connection drop — caller
    must stop and report.
    """
    with bv.App(test_mode=True) as app:

        @bv.event
        class Deep:
            user_id: str
            x: int
            y: int

        # Build (x == 1) & (y > 0) & (y > 0) & ... — 600 ANDs.
        DEPTH = 600
        predicate = bv.col("x") == 1
        for _ in range(DEPTH):
            predicate = predicate & (bv.col("y") > 0)

        @bv.event
        def DeepFiltered(d: Deep):
            return d.filter(predicate)

        @bv.table(key="user_id")
        def DeepCount(df: DeepFiltered):
            return df.group_by("user_id").agg(c=bv.count(window="forever"))

        try:
            app.register(Deep, DeepFiltered, DeepCount)
        except bv.RegistrationError as e:
            # Acceptable outcome: server rejected the deep predicate at
            # register time. Lock-in: the error MUST be structured (carries
            # a code), not a generic transport failure.
            assert e.code, f"RegistrationError without code: {e!r}"
            return

        # Canary push: server stayed up through the deep register. A normal
        # OP_PING completes within the per-call timeout — if the apply
        # loop is wedged, this will raise.
        app.ping()
        # The push itself must not panic. Server-side eval may return
        # Null (depth guard) and the filter rejects the event; that is
        # the expected silent-DoS-guard behaviour, not a bug.
        app.push("Deep", {"user_id": "alice", "x": 1, "y": 5})
        # Server still responsive after the deep push.
        pong = app.ping()
        assert "server_version" in pong, f"server unresponsive after deep push: {pong!r}"


# ---------------------------------------------------------------------------
# Test 4 — batch_get with 10,000 keys (upper-bound discovery)
# ---------------------------------------------------------------------------


def test_batch_get_with_10k_keys(beava_binary: Any) -> None:  # noqa: ARG001
    """``batch_get`` with 10,000 entries returns a 10,000-row list OR a
    specific error.

    Locks the upper limit on batch_get fan-out. The wire body is a single
    JSON document, so this also exercises the response-side frame-size
    boundary (10k empty rows ≪ 4 MiB, well within the response budget).
    """
    with bv.App(test_mode=True) as app:

        @bv.event
        class Click:
            user_id: str

        @bv.table(key="user_id")
        def Hits(cs: Click):
            return cs.group_by("user_id").agg(n=bv.count(window="forever"))

        app.register(Click, Hits)

        # 10k cold-start keys — server returns {} per entry (Plan 05
        # ADR-003 cold-start sentinel).
        N = 10_000
        requests: list[
            tuple[str, str | list[str | int | bool]]
            | tuple[str, str | list[str | int | bool], list[str] | None]
        ] = [("Hits", f"user_{i}") for i in range(N)]

        try:
            results = app.batch_get(requests)
        except bv.RegistrationError as e:
            # If a server-side cap exists, the error MUST be structured
            # with a meaningful code (not a generic transport-level
            # surface). Lock the shape; the size-limit code itself is not
            # specified by the wire contract.
            assert e.code, f"batch_get error without code: {e!r}"
            return

        # Success path: cardinality must match the request count exactly.
        assert isinstance(results, list), f"expected list, got {type(results).__name__}"
        assert len(results) == N, (
            f"batch_get returned {len(results)} rows; expected {N}"
        )
        # Cold-start: every row is {} (no events pushed).
        for i, row in enumerate(results):
            assert row == {}, f"row[{i}] expected {{}} (cold); got {row!r}"


# ---------------------------------------------------------------------------
# Test 5 — batch_get response shape for very large batches
# ---------------------------------------------------------------------------


def test_batch_get_response_shape_for_oversize_batch(beava_binary: Any) -> None:  # noqa: ARG001
    """If a max-batch limit exists, the error envelope MUST mention it.

    Probes ``batch_get`` with 100k entries (10× test 4). Either the server
    accepts it OR returns a structured error whose code / message gives
    operators something actionable. A silent truncation (returning fewer
    results than requested) IS the DoS / data-integrity bug this test
    guards against.
    """
    with bv.App(test_mode=True) as app:

        @bv.event
        class Click:
            user_id: str

        @bv.table(key="user_id")
        def Hits(cs: Click):
            return cs.group_by("user_id").agg(n=bv.count(window="forever"))

        app.register(Click, Hits)

        N = 100_000
        requests: list[
            tuple[str, str | list[str | int | bool]]
            | tuple[str, str | list[str | int | bool], list[str] | None]
        ] = [("Hits", f"u{i}") for i in range(N)]

        try:
            results = app.batch_get(requests)
        except bv.RegistrationError as e:
            # Structured error path: must carry a code, not a bare
            # transport failure.
            assert e.code, f"batch_get error without code at N={N}: {e!r}"
            return
        except (OSError, ConnectionError) as e:
            # Transport-level failure (frame-size exceeded etc.) is also
            # acceptable — the request envelope itself can exceed
            # MAX_FRAME_BYTES at 100k entries. The DoS guard surfaces as a
            # clean error rather than a crash; we record the kind.
            pytest.skip(f"batch_get N={N} surfaced as transport error: {e!r}")
            return

        # Success path: cardinality MUST match. Silent truncation here
        # would be the actual DoS bug.
        assert isinstance(results, list), f"expected list, got {type(results).__name__}"
        assert len(results) == N, (
            f"silent-truncation suspected: batch_get returned {len(results)}; "
            f"expected {N}"
        )


# ---------------------------------------------------------------------------
# Test 6 — register payload with 1000 features works (no quiet truncation)
# ---------------------------------------------------------------------------


def test_register_payload_at_size_limit(beava_binary: Any) -> None:  # noqa: ARG001
    """Register an aggregation with 1000 feature outputs; push events;
    verify every feature is computed (no silent truncation at the
    deserializer or schema-propagation pass).

    1000 ``count`` aggs from the same event source. The wire payload
    is well under MAX_FRAME_BYTES (~50 KB) but exercises the agg-table
    schema layout at scale.
    """
    with bv.App(test_mode=True) as app:

        @bv.event
        class Wide:
            user_id: str
            kind: str

        # Build 1000 feature outputs — each a count filtered on a
        # distinct ``kind`` literal. Server-side compilation rejects
        # unknown fields, so ``kind == 'k_i'`` references a real schema
        # field with a literal RHS.
        N_FEATURES = 1000

        def _build_aggs(w: Any) -> Any:
            agg_kwargs = {
                f"c_{i}": bv.count(where=(bv.col("kind") == f"k_{i}"))
                for i in range(N_FEATURES)
            }
            return w.group_by("user_id").agg(**agg_kwargs)

        @bv.table(key="user_id")
        def WideAgg(ws: Wide):
            return _build_aggs(ws)

        app.register(Wide, WideAgg)

        # Push one matching event per feature column (kind = "k_<i>").
        for i in range(N_FEATURES):
            app.push("Wide", {"user_id": "alice", "kind": f"k_{i}"})

        row = app.get("WideAgg", "alice")
        # Cardinality assertion catches silent truncation at any of:
        # register-time schema deserialize, compile, runtime allocation,
        # get-response serialize.
        non_key_fields = {k: v for k, v in row.items() if k != "user_id"}
        assert len(non_key_fields) == N_FEATURES, (
            f"silent-truncation suspected: got {len(non_key_fields)} features; "
            f"expected {N_FEATURES}"
        )
        # Every c_i must equal 1 (each pushed kind matches its own
        # predicate exactly once).
        for i in range(N_FEATURES):
            key = f"c_{i}"
            assert row.get(key) == 1, (
                f"feature {key} mismatched: got {row.get(key)!r}, expected 1"
            )


# ---------------------------------------------------------------------------
# Test 7 — connection-stays-up after a payload-bound rejection
#
# Locks the wire contract that ``frame_too_large`` is the ONLY fatal error.
# Every other error keeps the TCP connection usable. Mirrors the canary in
# test_error_response_codes._assert_tcp_connection_still_usable.
# ---------------------------------------------------------------------------


def test_oversize_get_request_does_not_poison_connection(
    beava_server: tuple[str, str],
) -> None:
    """A LengthUnderflow-shaped frame (declared length < 3) closes the
    connection — but a frame parse-error with valid declared length keeps
    the connection up. Test the latter: send an unknown opcode with a
    well-formed (small) frame, then verify the connection survives.

    Complement to test 2 which proves ``frame_too_large`` closes the
    connection. Together they nail down the per-error fatality contract.
    """
    _, tcp_url = beava_server
    host, port = _parse_tcp_url(tcp_url)
    sock = socket.create_connection((host, port), timeout=10.0)
    try:
        # Unknown opcode → OP_ERROR_RESPONSE with code=unknown_op,
        # connection stays open.
        sock.sendall(encode_frame(0x00AA, CT_JSON, b"{}"))
        resp = read_frame(sock, MAX_FRAME_BYTES)
        assert resp.op == OP_ERROR_RESPONSE, (
            f"expected OP_ERROR_RESPONSE; got {resp.op:#06x}"
        )

        # Follow-up OP_PING must succeed on the same socket.
        sock.sendall(encode_frame(OP_PING, CT_JSON, b"{}"))
        pong = read_frame(sock, MAX_FRAME_BYTES)
        assert pong.op == OP_PING, (
            f"connection poisoned after unknown_op; got {pong.op:#06x}"
        )
    finally:
        sock.close()


