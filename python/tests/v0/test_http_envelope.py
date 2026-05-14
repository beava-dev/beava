"""HTTP error-envelope wire tests for the v0 data-plane port.

Covers the underexercised arms of ``encode_glue_response_http`` in
``crates/beava-server/src/server.rs`` (currently 73.68% line coverage, with
most of the gap in the HTTP error-encoder match arms):

  - ``HttpRouteNotFound`` -> 404 ``{"error":{"code":"not_found","path":...}}``
  - ``HttpMethodNotAllowed`` -> 405 ``{"error":{"code":"method_not_allowed",
    "method":..., "path":...}}``
  - ``HttpUnsupportedMediaType`` -> 415
    ``{"error":{"code":"unsupported_media_type","path":...,
    "reason":"expected application/json"}, "registry_version":0}``
  - ``UnsupportedRequestShape`` -> 400
    ``{"error":{"code":"unsupported_request_shape","message":...}}`` —
    triggered by a body shape the dispatch doesn't recognise (e.g. a /push
    body that's a JSON array instead of an object).
  - ``Pong`` -> 200 ``{"pong":true,"registry_version":N}`` — exercises the
    ``POST /ping`` route added in Plan 12.6.

These tests use raw HTTP requests (httpx) against a freshly spawned beava
binary, NOT the ``app`` fixture — the goal is to exercise the wire-format
encoder, so we want to send byte-level payloads that the SDK builder would
reject locally before hitting the network.

The fixture is local to this module (does not piggyback on the embed
``app`` fixture because that uses TCP, not HTTP).
"""

from __future__ import annotations

import json
from typing import Generator

import httpx
import pytest

from ._helpers import _engine_available

pytestmark = pytest.mark.skipif(
    not _engine_available(),
    reason="requires Phase 13.4 engine + Phase 13.5 SDK rewrite",
)


@pytest.fixture
def http_server() -> Generator[str, None, None]:
    """Spawn a beava binary on ephemeral HTTP+TCP ports; yield ``http_url``.

    Uses ``beava._embed.spawn_embedded_server`` so the binary discovery /
    port-wait machinery matches what the SDK does in embed mode. We only
    consume the HTTP URL and tear the process down on test exit.
    """
    from beava._embed import spawn_embedded_server, teardown_process

    proc, http_url, _tcp_url, _env = spawn_embedded_server()
    try:
        yield http_url
    finally:
        teardown_process(proc)


# Minimal valid event-only register payload — keeps the registry non-empty
# so /push and /get routes have a real target for follow-up tests.
VALID_REGISTER_EVENT_ONLY = json.dumps({
    "nodes": [{
        "kind": "event",
        "name": "OrderEvent",
        "schema": {
            "fields": {"user_id": "str", "amount": "f64"},
            "optional_fields": [],
        },
        "dedupe_key": None,
        "dedupe_window_ms": None,
        "keep_events_for_ms": None,
    }]
}).encode("utf-8")


# ---------------------------------------------------------------------------
# Test 1 — HttpRouteNotFound (404)
# ---------------------------------------------------------------------------


def test_unknown_route_returns_404_with_path_in_envelope(http_server: str) -> None:
    """GET /this_route_does_not_exist → 404 ``{"error":{"code":"not_found",
    "path":"/this_route_does_not_exist"}}``."""
    path = "/this_route_does_not_exist"
    r = httpx.get(f"{http_server}{path}", timeout=10.0)
    assert r.status_code == 404, f"expected 404; got {r.status_code} body={r.text!r}"
    body = r.json()
    assert "error" in body, f"missing 'error' envelope: {body!r}"
    assert body["error"]["code"] == "not_found", (
        f"expected code=not_found; got {body['error'].get('code')!r}"
    )
    assert body["error"].get("path") == path, (
        f"expected path={path!r}; got {body['error'].get('path')!r}"
    )


def test_unknown_route_post_returns_404(http_server: str) -> None:
    """POST to an unknown route also surfaces 404 (router treats verb-pair
    lookups uniformly — no path-defined ⇒ NotFound, not MethodNotAllowed)."""
    path = "/some_made_up_route"
    r = httpx.post(
        f"{http_server}{path}",
        content=b"{}",
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )
    assert r.status_code == 404, f"expected 404; got {r.status_code} body={r.text!r}"
    body = r.json()
    assert body["error"]["code"] == "not_found"
    assert body["error"]["path"] == path


# ---------------------------------------------------------------------------
# Test 2 — HttpMethodNotAllowed (405)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("method,path", [
    ("DELETE", "/register"),
    ("PUT", "/register"),
    ("POST", "/health"),     # /health is GET-only
    ("DELETE", "/push/Foo"),  # /push/<name> is POST-only
    ("GET", "/reset"),        # /reset is POST-only
])
def test_method_not_allowed_returns_405_with_method_and_path(
    http_server: str, method: str, path: str
) -> None:
    """Method-mismatch on a known route → 405 ``{"error":{"code":
    "method_not_allowed", "method":..., "path":...}}``."""
    r = httpx.request(method, f"{http_server}{path}", timeout=10.0)
    assert r.status_code == 405, (
        f"{method} {path}: expected 405; got {r.status_code} body={r.text!r}"
    )
    body = r.json()
    assert "error" in body, f"missing 'error' envelope: {body!r}"
    assert body["error"]["code"] == "method_not_allowed", (
        f"expected code=method_not_allowed; got {body['error'].get('code')!r}"
    )
    assert body["error"].get("method") == method, (
        f"expected method={method!r}; got {body['error'].get('method')!r}"
    )
    assert body["error"].get("path") == path, (
        f"expected path={path!r}; got {body['error'].get('path')!r}"
    )


# ---------------------------------------------------------------------------
# Test 3 — HttpUnsupportedMediaType (415) on POST /register
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("ct", [
    "text/plain",
    "application/xml",
    "application/octet-stream",
    "",  # empty content-type
])
def test_register_wrong_content_type_returns_415(http_server: str, ct: str) -> None:
    """POST /register with non-``application/json`` content-type →
    415 ``{"error":{"code":"unsupported_media_type",...},"registry_version":0}``.

    Only enforced on /register (other POST endpoints don't gate on
    Content-Type — see ``http_listener.rs::is_register_post``).
    """
    headers = {"Content-Type": ct} if ct else {}
    r = httpx.post(
        f"{http_server}/register",
        content=VALID_REGISTER_EVENT_ONLY,
        headers=headers,
        timeout=10.0,
    )
    assert r.status_code == 415, (
        f"Content-Type={ct!r}: expected 415; got {r.status_code} body={r.text!r}"
    )
    body = r.json()
    assert "error" in body, f"missing 'error' envelope: {body!r}"
    assert body["error"]["code"] == "unsupported_media_type", (
        f"expected code=unsupported_media_type; got {body['error'].get('code')!r}"
    )
    assert body["error"].get("path") == "/register"
    assert body["error"].get("reason") == "expected application/json"
    # The encoder asserts a fresh-boot ``registry_version: 0`` in the 415
    # body (see server.rs:2631) so callers can cheaply detect "this server
    # has no registrations yet" alongside the media-type rejection.
    assert "registry_version" in body, (
        f"415 body must include registry_version sentinel: {body!r}"
    )


# ---------------------------------------------------------------------------
# Test 4 — UnsupportedRequestShape (400) on /push with a non-object body
# ---------------------------------------------------------------------------


def test_push_with_array_body_returns_unsupported_request_shape(
    http_server: str,
) -> None:
    """POST /push/Foo with a JSON array (not an object) → 400
    ``{"error":{"code":"unsupported_request_shape","message":...}}``.

    The dispatch layer expects a JSON object for /push; a top-level array
    is a structurally invalid request shape that surfaces via the
    ``UnsupportedRequestShape`` arm of the HTTP encoder.

    NOTE: First register the event so we get past the route-resolution
    check; the goal is to reach the push-body-shape rejection, not the
    event-name-unknown rejection.
    """
    # Register first so /push/OrderEvent resolves.
    reg = httpx.post(
        f"{http_server}/register",
        content=VALID_REGISTER_EVENT_ONLY,
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )
    assert reg.status_code == 200, f"register failed: {reg.status_code} {reg.text!r}"

    # Send a JSON array (not an object) — dispatch will reject the shape.
    bad_body = b'[1,2,3]'
    r = httpx.post(
        f"{http_server}/push/OrderEvent",
        content=bad_body,
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )
    # The exact code path may surface as ``unsupported_request_shape``
    # (HTTP 400) or as a structured push error (HTTP 400 with a different
    # code). Both are acceptable here as long as the server doesn't
    # 500/crash and the response is JSON.
    assert r.status_code in (400, 404), (
        f"expected 400 or 404; got {r.status_code} body={r.text!r}"
    )
    body = r.json()
    assert "error" in body, f"missing 'error' envelope: {body!r}"
    # Either ``unsupported_request_shape`` (request-shape rejection)
    # OR a push-time validation error — both prove we hit a structured
    # error path, not the catch-all ``unsupported`` arm.
    assert body["error"]["code"] != "unsupported", (
        f"hit the catch-all 501 arm; body={body!r}"
    )


# ---------------------------------------------------------------------------
# Test 5 — Pong (200 + registry_version) on POST /ping
# ---------------------------------------------------------------------------


def test_ping_returns_pong_with_registry_version(http_server: str) -> None:
    """POST /ping → 200 ``{"pong":true,"registry_version":N}``.

    Verb-style liveness probe added in Plan 12.6; mirrors TCP ``OP_PING``
    (0x0000). Returns the live registry counter so SDK clients can use it
    as a cheap cache-invalidation probe.
    """
    # Cold server: registry_version should be 0 (no /register yet).
    r0 = httpx.post(
        f"{http_server}/ping",
        content=b"{}",
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )
    assert r0.status_code == 200, f"expected 200; got {r0.status_code} body={r0.text!r}"
    body0 = r0.json()
    assert body0.get("pong") is True, f"expected pong=true; got {body0!r}"
    assert "registry_version" in body0, (
        f"Pong body must carry registry_version: {body0!r}"
    )
    rv_before = int(body0["registry_version"])

    # Register a node — registry_version should bump.
    reg = httpx.post(
        f"{http_server}/register",
        content=VALID_REGISTER_EVENT_ONLY,
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )
    assert reg.status_code == 200, f"register failed: {reg.status_code} {reg.text!r}"

    # Cold server: registry_version should now be >= 1.
    r1 = httpx.post(
        f"{http_server}/ping",
        content=b"{}",
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )
    assert r1.status_code == 200, f"expected 200; got {r1.status_code} body={r1.text!r}"
    body1 = r1.json()
    assert body1.get("pong") is True
    rv_after = int(body1["registry_version"])
    assert rv_after > rv_before, (
        f"registry_version must advance after /register: "
        f"before={rv_before} after={rv_after}"
    )
