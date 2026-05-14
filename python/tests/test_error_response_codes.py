"""Wire-level error-response shape tests across HTTP and TCP.

Coverage scope (8 tests, most parametrised across HTTP + TCP):

  1. ``test_invalid_registration_missing_field`` — server rejects malformed
     register payload (top-level ``nodes`` key missing) with structured
     ``{error:{code:"invalid_registration", path, reason}}`` on both wires.
  2. ``test_unknown_op_returns_error_response_with_correct_code`` — TCP-only:
     unknown opcode (0x00AA) surfaces as ``OP_ERROR_RESPONSE`` with
     ``code="unknown_op"`` (TcpError variant). HTTP has no analogue (routing
     is path-based, not opcode-based) — documented in the test body.
  3. ``test_unsupported_content_type_on_op_register`` — TCP-only: ``OP_REGISTER``
     framed with ``CT_MSGPACK`` is rejected with ``code="unsupported_content_type"``.
  4. ``test_aggregation_invalid_where_returns_parse_error`` — register payload
     with a ``where=`` clause referencing an unknown field surfaces as
     ``code="aggregation_invalid_where"`` on both wires.
  5. ``test_feature_not_found_via_get_with_unknown_feature_name`` —
     **documents HTTP-vs-TCP asymmetry** from PR #117: HTTP returns status 500
     with ``{error:{code:"internal_error", reason:"feature_not_found: ..."}}``;
     TCP collapses to the catch-all ``OP_ERROR_RESPONSE`` body
     ``{"code":"unsupported"}`` (no ``"error"`` wrapper, no nested fields).
     Test locks both shapes in-place so an attempt to fix the asymmetry will
     trip this test and force a deliberate update.
  6. ``test_push_to_unregistered_event_type`` — push for an event that was
     never registered surfaces as ``code="event_not_found"`` on both wires
     (HTTP returns 404, TCP frames ``OP_ERROR_RESPONSE``).
  7. ``test_get_on_unregistered_table`` — get on an unregistered table
     surfaces as ``code="unknown_table"`` on both wires (HTTP 404, TCP
     ``OP_ERROR_RESPONSE``).
  8. ``test_register_force_with_conflicting_field_types`` — re-registering an
     event with a different field type (destructive diff) without
     ``force=true`` surfaces as ``code="force_required"`` on both wires
     (HTTP 409, TCP ``OP_ERROR_RESPONSE``).

For each parametrised test, the TCP arm also verifies the connection stays
usable after the error (a follow-up ``OP_PING`` succeeds) — strict-FIFO with
no half-broken connections is part of the wire contract.
"""

from __future__ import annotations

import json
import socket
from typing import Callable

import httpx
import pytest

from beava._errors import RegistrationError
from beava._transport import HttpTransport, TcpTransport
from beava._wire import (
    CT_JSON,
    CT_MSGPACK,
    OP_ERROR_RESPONSE,
    OP_GET,
    OP_PING,
    OP_PUSH,
    OP_REGISTER,
    encode_frame,
    read_frame,
)

# ---------------------------------------------------------------------------
# Wire-payload fixtures (raw JSON bytes — not constructed via the SDK so the
# tests can exercise malformed shapes without the SDK builder rejecting them
# locally).
# ---------------------------------------------------------------------------

# Minimal valid register payload — one event named ``OrderEvent`` with two
# fields. Used by tests that need a baseline registration before triggering
# an error.
VALID_REGISTER_ORDER = json.dumps({
    "nodes": [{
        "kind": "event",
        "name": "OrderEvent",
        "schema": {"fields": {"user_id": "str", "amount": "f64"}, "optional_fields": []},
        "dedupe_key": None,
        "dedupe_window_ms": None,
        "keep_events_for_ms": None,
    }]
}).encode("utf-8")

# Register payload missing the required top-level ``nodes`` key — serde
# strict mode rejects unknown / missing fields with a structured
# ``invalid_registration`` error.
MALFORMED_REGISTER_MISSING_NODES = b'{"not_nodes": []}'

# Register payload with a where-clause that references a field absent from
# the event schema — exercises ``ErrorCode::AggregationInvalidWhere`` ->
# wire string ``aggregation_invalid_where``. v0 ships events-only at the
# raw-JSON layer; tables come back as ``kind: "derivation"`` with
# ``output_kind: "table"`` (per ADR-001 partial overturn).
REGISTER_INVALID_WHERE = json.dumps({
    "nodes": [
        {
            "kind": "event",
            "name": "WhereEvent",
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
            "name": "WhereTable",
            "output_kind": "table",
            "upstreams": ["WhereEvent"],
            "ops": [{
                "op": "group_by",
                "keys": ["user_id"],
                "agg": {
                    # ``no_such_field`` is not in WhereEvent's schema; the
                    # compile pass rejects the predicate with
                    # AggregationInvalidWhere.
                    "c": {
                        "op": "count",
                        "params": {"where": "(no_such_field == 'ok')"},
                    },
                },
            }],
            "schema": {"fields": {"user_id": "str", "c": "i64"}, "optional_fields": []},
            "table_primary_key": ["user_id"],
        },
    ]
}).encode("utf-8")


def _tcp_socket(tcp_url: str, *, timeout: float = 10.0) -> socket.socket:
    """Open a TCP connection to ``tcp://host:port`` and return the socket."""
    host_port = tcp_url[len("tcp://"):]
    host, port_str = host_port.rsplit(":", 1)
    return socket.create_connection((host, int(port_str)), timeout=timeout)


def _make_tcp_transport(tcp_url: str) -> TcpTransport:
    host_port = tcp_url[len("tcp://"):]
    host, port_str = host_port.rsplit(":", 1)
    return TcpTransport(host=host, port=int(port_str))


def _http_post(
    http_url: str,
    path: str,
    body: bytes,
    content_type: str = "application/json",
) -> httpx.Response:
    """Helper: post raw bytes to ``http_url + path``; do NOT raise on non-2xx."""
    return httpx.post(
        f"{http_url}{path}",
        content=body,
        headers={"Content-Type": content_type},
        timeout=10.0,
    )


def _tcp_round_trip(sock: socket.socket, op: int, ct: int, payload: bytes):
    """Send one frame, read one frame; return the decoded :class:`Frame`."""
    sock.sendall(encode_frame(op, ct, payload))
    return read_frame(sock)


def _assert_tcp_connection_still_usable(sock: socket.socket) -> None:
    """Send OP_PING; assert the server still responds — i.e. the prior error
    didn't poison the connection. Strict-FIFO + connection-resumable is part
    of the wire contract for all non-fatal errors (frame_too_large is fatal;
    everything else keeps the connection open).
    """
    pong = _tcp_round_trip(sock, OP_PING, CT_JSON, b"{}")
    assert pong.op == OP_PING, (
        f"OP_PING after error frame must return OP_PING; got op={pong.op:#06x}"
    )


# ---------------------------------------------------------------------------
# Test 1 — invalid_registration on a missing-top-level-field payload.
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("transport_kind", ["http", "tcp"])
def test_invalid_registration_missing_field(
    beava_server: tuple[str, str],
    transport_kind: str,
) -> None:
    """Malformed register body surfaces as ``code='invalid_registration'``.

    The serde-strict path rejects the missing top-level ``nodes`` key with a
    structured error envelope — same code on both wires; HTTP returns 400,
    TCP frames ``OP_ERROR_RESPONSE``.
    """
    http_url, tcp_url = beava_server

    if transport_kind == "http":
        r = _http_post(http_url, "/register", MALFORMED_REGISTER_MISSING_NODES)
        assert r.status_code == 400, f"expected 400; got {r.status_code} body={r.text!r}"
        body = r.json()
        assert "error" in body, f"missing 'error' envelope; body={body!r}"
        assert body["error"]["code"] == "invalid_registration", (
            f"expected code=invalid_registration; got {body['error'].get('code')!r}"
        )
        # The serde path produces a non-empty reason/path so callers can
        # surface something actionable rather than a bare code string.
        assert (
            body["error"].get("path", "") or body["error"].get("reason", "")
        ), f"invalid_registration must carry path or reason; body={body!r}"
    else:
        sock = _tcp_socket(tcp_url)
        try:
            frame = _tcp_round_trip(
                sock, OP_REGISTER, CT_JSON, MALFORMED_REGISTER_MISSING_NODES
            )
            assert frame.op == OP_ERROR_RESPONSE, (
                f"expected OP_ERROR_RESPONSE (0xFFFF); got op={frame.op:#06x}"
            )
            body = json.loads(frame.payload.decode("utf-8"))
            assert "error" in body, f"TCP body missing 'error' envelope: {body!r}"
            assert body["error"]["code"] == "invalid_registration", (
                f"TCP expected code=invalid_registration; got {body['error'].get('code')!r}"
            )
            _assert_tcp_connection_still_usable(sock)
        finally:
            sock.close()


# ---------------------------------------------------------------------------
# Test 2 — unknown_op (TCP-only — HTTP routing is path-based, not opcode-based).
# ---------------------------------------------------------------------------


def test_unknown_op_returns_error_response_with_correct_code(
    beava_server: tuple[str, str],
) -> None:
    """Sending an unrecognised TCP opcode (0x00AA) → ``code='unknown_op'``.

    The TcpError path emits a rich frame with ``message`` and ``extras.op``
    set, and KEEPS THE CONNECTION OPEN — only ``frame_too_large`` is fatal.

    No HTTP analogue: HTTP routing is path-based and unrecognised paths
    surface as ``not_found`` (HTTP 404), exercised in Test 7. The "opcode"
    concept is wire-specific to TCP.
    """
    _, tcp_url = beava_server
    sock = _tcp_socket(tcp_url)
    try:
        # 0x00AA — not in the known opcode set (PING/REGISTER/PUSH/GET/...);
        # known-but-deferred opcodes get the ``op_not_implemented`` code
        # path, truly unknown ones get ``unknown_op``.
        UNKNOWN_OP = 0x00AA
        frame = _tcp_round_trip(sock, UNKNOWN_OP, CT_JSON, b"{}")
        assert frame.op == OP_ERROR_RESPONSE, (
            f"expected OP_ERROR_RESPONSE; got op={frame.op:#06x}"
        )
        body = json.loads(frame.payload.decode("utf-8"))
        assert "error" in body, f"TCP error body missing 'error' envelope: {body!r}"
        assert body["error"]["code"] == "unknown_op", (
            f"expected code=unknown_op; got {body['error'].get('code')!r}"
        )
        # The rich TcpError frame carries a human-readable message and the
        # offending opcode in extras.op.
        assert "message" in body["error"], (
            f"TcpError frame must carry 'message'; body={body!r}"
        )
        assert body["error"].get("op") == UNKNOWN_OP, (
            f"TcpError extras must carry op={UNKNOWN_OP:#06x}; body={body!r}"
        )
        _assert_tcp_connection_still_usable(sock)
    finally:
        sock.close()


# ---------------------------------------------------------------------------
# Test 3 — unsupported_content_type on OP_REGISTER (TCP-only).
# ---------------------------------------------------------------------------


def test_unsupported_content_type_on_op_register(
    beava_server: tuple[str, str],
) -> None:
    """OP_REGISTER framed with CT_MSGPACK is rejected.

    Per ``tcp_listener.rs`` the register path only accepts ``CT_JSON``.
    Other content-type bytes surface as a ParseError with the
    ``unsupported content_type for register: ...`` prefix, which the
    apply_shard dispatcher classifies as a TcpError with
    ``code='unsupported_content_type'``. The connection stays open.

    HTTP has no parallel: the HTTP register path checks for
    ``Content-Type: application/json`` and rejects others with
    ``unsupported_media_type`` (HTTP 415) — already covered by the
    existing transport tests.
    """
    _, tcp_url = beava_server
    sock = _tcp_socket(tcp_url)
    try:
        # Payload is irrelevant — the listener short-circuits on the
        # content-type byte before reading the body.
        frame = _tcp_round_trip(sock, OP_REGISTER, CT_MSGPACK, b"\x80")
        assert frame.op == OP_ERROR_RESPONSE, (
            f"expected OP_ERROR_RESPONSE; got op={frame.op:#06x}"
        )
        body = json.loads(frame.payload.decode("utf-8"))
        assert "error" in body, f"missing 'error' envelope; body={body!r}"
        assert body["error"]["code"] == "unsupported_content_type", (
            f"expected code=unsupported_content_type; "
            f"got {body['error'].get('code')!r} body={body!r}"
        )
        _assert_tcp_connection_still_usable(sock)
    finally:
        sock.close()


# ---------------------------------------------------------------------------
# Test 4 — aggregation_invalid_where on a where= clause with an unknown field.
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("transport_kind", ["http", "tcp"])
def test_aggregation_invalid_where_returns_parse_error(
    beava_server: tuple[str, str],
    transport_kind: str,
) -> None:
    """A ``where=`` clause referencing an unknown field surfaces as
    ``code='aggregation_invalid_where'`` on both wires (HTTP 400, TCP error
    frame).

    Per ``register.rs::error_code_to_wire_str``, ``ErrorCode::AggregationInvalidWhere``
    maps to the wire string ``aggregation_invalid_where`` — distinct from the
    generic ``invalid_registration`` so callers can pinpoint the
    where-clause as the cause.
    """
    http_url, tcp_url = beava_server

    if transport_kind == "http":
        r = _http_post(http_url, "/register", REGISTER_INVALID_WHERE)
        assert r.status_code == 400, f"expected 400; got {r.status_code} body={r.text!r}"
        body = r.json()
        assert body["error"]["code"] == "aggregation_invalid_where", (
            f"expected aggregation_invalid_where; got {body['error'].get('code')!r}"
        )
    else:
        sock = _tcp_socket(tcp_url)
        try:
            frame = _tcp_round_trip(
                sock, OP_REGISTER, CT_JSON, REGISTER_INVALID_WHERE
            )
            assert frame.op == OP_ERROR_RESPONSE, (
                f"expected OP_ERROR_RESPONSE; got op={frame.op:#06x}"
            )
            body = json.loads(frame.payload.decode("utf-8"))
            assert body["error"]["code"] == "aggregation_invalid_where", (
                f"TCP expected aggregation_invalid_where; "
                f"got {body['error'].get('code')!r} body={body!r}"
            )
            _assert_tcp_connection_still_usable(sock)
        finally:
            sock.close()


# ---------------------------------------------------------------------------
# Test 5 — feature_not_found via /get with an unknown feature name.
# Documents the HTTP-vs-TCP asymmetry surfaced by PR #117.
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("transport_kind", ["http", "tcp"])
def test_feature_not_found_via_get_with_unknown_feature_name(
    beava_server: tuple[str, str],
    transport_kind: str,
) -> None:
    """Get with an unknown feature in ``features=[...]`` surfaces a server
    error — but the wire shape DIFFERS across transports (PR #117).

    Server path (``runtime_core_glue::dispatch_get_single_verb_style_sync``):
    when any name in ``features`` is absent from the registered descriptor,
    the call returns ``GlueResponse::InternalError`` with reason
    ``"feature_not_found: missing=[...] table=..."``. The two encoders
    handle that variant differently:

      HTTP (``encode_glue_response_http``):
        status=500, body=``{"error":{"code":"internal_error", "reason":<msg>}}``
        — the ``reason`` carries the ``feature_not_found`` substring and the
        offending feature names.

      TCP (``encode_glue_response_tcp``):
        falls through to the catch-all arm — body=``{"code":"unsupported"}``
        (NOT nested under ``"error"``). Both the structured nesting AND the
        ``feature_not_found`` reason text are dropped on the TCP wire.

    This asymmetry is a real bug — the TCP encoder ought to mirror the HTTP
    encoder's structured shape — but it's the current observed behaviour
    and PR #117 explicitly chose to lock it in place rather than fix it,
    pending a separate alignment plan. The test asserts only what is true
    today AND the cross-transport invariant that the SDK surfaces an
    error (it does NOT fabricate a row).

    Once the encoder is fixed, this test MUST be updated — the asymmetry
    block below will start failing; that is the intended signal to revisit
    the catch-all arm in ``server.rs::encode_glue_response_tcp``.
    """
    http_url, tcp_url = beava_server

    # Bootstrap: register a real event + table so the table exists; the
    # unknown name in the projection is the only thing the server should
    # complain about.
    register_payload = json.dumps({
        "nodes": [
            {
                "kind": "event",
                "name": "FeatEvent",
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
                "name": "FeatTable",
                "output_kind": "table",
                "upstreams": ["FeatEvent"],
                "ops": [{
                    "op": "group_by",
                    "keys": ["user_id"],
                    "agg": {
                        "c": {"op": "count", "params": {}},
                    },
                }],
                "schema": {
                    "fields": {"user_id": "str", "c": "i64"},
                    "optional_fields": [],
                },
                "table_primary_key": ["user_id"],
            },
        ]
    }).encode("utf-8")
    r = _http_post(http_url, "/register", register_payload)
    assert r.status_code == 200, f"register bootstrap failed: {r.status_code} {r.text!r}"

    bad_get_body = json.dumps({
        "table": "FeatTable",
        "key": "alice",
        "features": ["definitely_not_a_real_feature"],
    }).encode("utf-8")

    if transport_kind == "http":
        r = _http_post(http_url, "/get", bad_get_body)
        # PR #117 lock-down: HTTP returns 500 + internal_error code.
        assert r.status_code == 500, (
            f"expected 500 for HTTP feature_not_found path; got {r.status_code} "
            f"body={r.text!r}"
        )
        body = r.json()
        assert "error" in body, f"HTTP body missing 'error' envelope: {body!r}"
        assert body["error"]["code"] == "internal_error", (
            f"HTTP feature_not_found currently surfaces as internal_error per "
            f"server.rs::encode_glue_response_http; got {body['error'].get('code')!r}"
        )
        # The reason text carries the actual ``feature_not_found`` substring
        # and the offending name — this part is preserved on HTTP.
        reason = body["error"].get("reason", "")
        assert "feature_not_found" in reason, (
            f"HTTP error.reason must mention feature_not_found; got reason={reason!r}"
        )
        assert "definitely_not_a_real_feature" in reason, (
            f"HTTP error.reason must mention offending name; got reason={reason!r}"
        )
    else:
        sock = _tcp_socket(tcp_url)
        try:
            frame = _tcp_round_trip(sock, OP_GET, CT_JSON, bad_get_body)
            assert frame.op == OP_ERROR_RESPONSE, (
                f"expected OP_ERROR_RESPONSE on TCP; got op={frame.op:#06x}"
            )
            # PR #117 lock-down: TCP collapses to the catch-all arm — body
            # is ``{"code":"unsupported"}`` (no nested ``"error"`` envelope,
            # no ``feature_not_found`` reason, no offending name). This is
            # the asymmetry; documented here, NOT fixed in this PR.
            body = json.loads(frame.payload.decode("utf-8"))
            assert body == {"code": "unsupported"}, (
                f"TCP feature_not_found currently collapses to the catch-all "
                f"{{'code': 'unsupported'}} body per "
                f"server.rs::encode_glue_response_tcp; got {body!r}. "
                f"If this asymmetry has been fixed, update this test and the "
                f"corresponding HTTP arm above."
            )
            _assert_tcp_connection_still_usable(sock)
        finally:
            sock.close()


# ---------------------------------------------------------------------------
# Test 6 — push to an unregistered event surfaces as ``event_not_found``.
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("transport_kind", ["http", "tcp"])
def test_push_to_unregistered_event_type(
    beava_server: tuple[str, str],
    transport_kind: str,
) -> None:
    """Push for an event name that was never registered → ``code='event_not_found'``.

    Per ``apply_shard.rs::dispatch_push_sync``, the descriptor lookup miss
    emits ``GlueResponse::PushError {code: "event_not_found", ...}``. The
    HTTP encoder maps ``event_not_found`` specifically to status 404 (other
    PushError codes route to 400); the TCP encoder frames
    ``OP_ERROR_RESPONSE`` with the structured body. The code string is the
    same across transports.
    """
    http_url, tcp_url = beava_server

    push_body_http = json.dumps({
        "event": "GhostEvent",
        "data": {"user_id": "alice"},
    }).encode("utf-8")

    push_body_tcp = json.dumps({
        "event": "GhostEvent",
        "body": {"user_id": "alice"},
    }).encode("utf-8")

    if transport_kind == "http":
        r = _http_post(http_url, "/push", push_body_http)
        assert r.status_code == 404, (
            f"event_not_found must surface as HTTP 404; got {r.status_code} "
            f"body={r.text!r}"
        )
        body = r.json()
        assert body["error"]["code"] == "event_not_found", (
            f"HTTP body code mismatch; body={body!r}"
        )
    else:
        sock = _tcp_socket(tcp_url)
        try:
            frame = _tcp_round_trip(sock, OP_PUSH, CT_JSON, push_body_tcp)
            assert frame.op == OP_ERROR_RESPONSE, (
                f"expected OP_ERROR_RESPONSE; got op={frame.op:#06x}"
            )
            body = json.loads(frame.payload.decode("utf-8"))
            assert body["error"]["code"] == "event_not_found", (
                f"TCP body code mismatch; body={body!r}"
            )
            _assert_tcp_connection_still_usable(sock)
        finally:
            sock.close()


# ---------------------------------------------------------------------------
# Test 7 — get on an unregistered table surfaces as ``unknown_table``.
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("transport_kind", ["http", "tcp"])
def test_get_on_unregistered_table(
    beava_server: tuple[str, str],
    transport_kind: str,
) -> None:
    """GET for a table name that was never registered → ``code='unknown_table'``.

    Per ``runtime_core_glue::dispatch_get_single_verb_style_sync``, the
    descriptor lookup miss returns ``GlueResponse::QueryNotFound
    {code: "unknown_table"}``. The HTTP encoder maps QueryNotFound to status
    404 with ``{"error":{"code":"unknown_table"}}``; the TCP encoder frames
    ``OP_ERROR_RESPONSE`` with the same body. Code string is identical
    across transports.
    """
    http_url, tcp_url = beava_server

    get_body = json.dumps({
        "table": "NeverRegisteredTable",
        "key": "alice",
    }).encode("utf-8")

    if transport_kind == "http":
        r = _http_post(http_url, "/get", get_body)
        assert r.status_code == 404, (
            f"unknown_table must surface as HTTP 404; got {r.status_code} "
            f"body={r.text!r}"
        )
        body = r.json()
        assert body["error"]["code"] == "unknown_table", (
            f"HTTP body code mismatch; body={body!r}"
        )
    else:
        sock = _tcp_socket(tcp_url)
        try:
            frame = _tcp_round_trip(sock, OP_GET, CT_JSON, get_body)
            assert frame.op == OP_ERROR_RESPONSE, (
                f"expected OP_ERROR_RESPONSE; got op={frame.op:#06x}"
            )
            body = json.loads(frame.payload.decode("utf-8"))
            assert body["error"]["code"] == "unknown_table", (
                f"TCP body code mismatch; body={body!r}"
            )
            _assert_tcp_connection_still_usable(sock)
        finally:
            sock.close()


# ---------------------------------------------------------------------------
# Test 8 — destructive register without force=true → ``force_required``.
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("transport_kind", ["http", "tcp"])
def test_register_force_with_conflicting_field_types(
    beava_server: tuple[str, str],
    transport_kind: str,
) -> None:
    """Re-register an existing event with a different field type → ``force_required``.

    Per ``register_check_force_required`` + ``apply_shard.rs::dispatch_one``,
    a destructive diff entry without ``force=true`` surfaces with
    ``code='force_required'``. HTTP returns 409 with
    ``{"error":{"code":"force_required", "reason": ..., "diff": {...}}}``;
    TCP frames ``OP_ERROR_RESPONSE`` with the same body. Code string is
    identical across transports.
    """
    http_url, tcp_url = beava_server

    initial_payload = json.dumps({
        "nodes": [{
            "kind": "event",
            "name": "ConflictEvent",
            "schema": {
                "fields": {"user_id": "str", "amount": "f64"},
                "optional_fields": [],
            },
            "dedupe_key": None,
            "dedupe_window_ms": None,
            "keep_events_for_ms": None,
        }]
    }).encode("utf-8")

    # Same event name, but ``amount`` changes type from f64 → i64. This is a
    # destructive schema change (field type swap).
    conflicting_payload = json.dumps({
        "nodes": [{
            "kind": "event",
            "name": "ConflictEvent",
            "schema": {
                "fields": {"user_id": "str", "amount": "i64"},
                "optional_fields": [],
            },
            "dedupe_key": None,
            "dedupe_window_ms": None,
            "keep_events_for_ms": None,
        }]
    }).encode("utf-8")

    # Bootstrap the initial registration — runs over HTTP for both arms so
    # the test is deterministic regardless of which transport we then exercise.
    r = _http_post(http_url, "/register", initial_payload)
    assert r.status_code == 200, (
        f"bootstrap register failed: {r.status_code} {r.text!r}"
    )

    if transport_kind == "http":
        r = _http_post(http_url, "/register", conflicting_payload)
        assert r.status_code == 409, (
            f"destructive re-register without force must surface as HTTP 409; "
            f"got {r.status_code} body={r.text!r}"
        )
        body = r.json()
        assert body["error"]["code"] == "force_required", (
            f"HTTP body code mismatch; body={body!r}"
        )
    else:
        sock = _tcp_socket(tcp_url)
        try:
            frame = _tcp_round_trip(
                sock, OP_REGISTER, CT_JSON, conflicting_payload
            )
            assert frame.op == OP_ERROR_RESPONSE, (
                f"expected OP_ERROR_RESPONSE; got op={frame.op:#06x}"
            )
            body = json.loads(frame.payload.decode("utf-8"))
            assert body["error"]["code"] == "force_required", (
                f"TCP body code mismatch; body={body!r}"
            )
            _assert_tcp_connection_still_usable(sock)
        finally:
            sock.close()


# Keep `_make_tcp_transport` exported as a future utility — silences
# unused-import diagnostics without affecting test discovery.
_ = (HttpTransport, RegistrationError, Callable, _make_tcp_transport)
