"""Tests for beava._transport.HttpTransport.

These tests require a running beava server via the `beava_server` fixture.
They are expected to FAIL (ImportError) until python/beava/_transport.py is
created in Task 1.b.
"""

from __future__ import annotations

import pytest

from beava._errors import RegistrationError
from beava._transport import HttpTransport

# A minimal valid event registration payload (matches Phase 12.6-06 wire contract).
# Plan 12.6-06 D-03 hard rip: `event_time_field` and `tolerate_delay_ms` keys
# removed from EventDescriptor; sending them now raises a structured 400 with
# `unknown_field_event_time_v0` / `unknown_field_tolerate_delay_v0`.
VALID_REGISTER_PAYLOAD = (
    b'{"nodes":[{'
    b'"kind":"event",'
    b'"name":"TestEvent",'
    b'"schema":{"fields":{"event_time":"i64","amount":"f64"},"optional_fields":[]},'
    b'"dedupe_key":null,"dedupe_window_ms":null,'
    b'"keep_events_for_ms":null'
    b"}]}"
)

# Payload that uses a reserved _beava_ prefix — server returns invalid_registration.
INVALID_REGISTER_PAYLOAD = (
    b'{"nodes":[{'
    b'"kind":"event",'
    b'"name":"_beava_reserved",'
    b'"schema":{"fields":{"x":"f64"},"optional_fields":[]},'
    b'"dedupe_key":null,"dedupe_window_ms":null,'
    b'"keep_events_for_ms":null'
    b"}]}"
)


class TestHttpTransportRegister:
    def test_http_transport_register_success(self, beava_server: tuple[str, str]) -> None:
        """Successful registration returns dict with status='ok' and registry_version >= 1."""
        http_url, _ = beava_server
        with HttpTransport(http_url) as t:
            result = t.send_register(VALID_REGISTER_PAYLOAD)
        assert result["status"] == "ok"
        assert result["registry_version"] >= 1

    def test_http_transport_register_validation_error(
        self, beava_server: tuple[str, str]
    ) -> None:
        """Invalid payload raises RegistrationError with code='invalid_registration'."""
        http_url, _ = beava_server
        with HttpTransport(http_url) as t:
            with pytest.raises(RegistrationError) as exc_info:
                t.send_register(INVALID_REGISTER_PAYLOAD)
        assert exc_info.value.code == "invalid_registration"
        assert exc_info.value.path != "" or exc_info.value.message != ""

    def test_http_transport_register_unsupported_media_type(
        self, beava_server: tuple[str, str]
    ) -> None:
        """Posting with wrong Content-Type raises RegistrationError.

        Expected code: 'unsupported_media_type'.
        """
        import httpx

        http_url, _ = beava_server
        # Post with text/plain instead of application/json
        r = httpx.post(
            f"{http_url}/register",
            content=b"hello",
            headers={"Content-Type": "text/plain"},
        )
        assert r.status_code == 415
        body = r.json()
        assert body["error"]["code"] == "unsupported_media_type"

    def test_http_transport_ping_returns_pong_with_registry_version(
        self, beava_server: tuple[str, str]
    ) -> None:
        """HttpTransport.send_ping() returns {pong: True, registry_version: <n>}.

        Locked v0 wire surface: ``POST /ping`` is a verb-style liveness probe
        on the data plane, returning ``{"pong": true, "registry_version": <n>}``
        so SDK clients can use it for cheap registry-version invalidation
        (cache key, schema-evolution detection on long-lived connections).

        After a successful register, registry_version increases by 1 —
        verify the SDK propagates the bumped value. (Absolute pre-register
        value is implementation-detail: in-process TestServer starts at 0,
        subprocess spawn records an initial WAL bump and starts at 1; both
        are valid. The contract is the +1 delta on register.)
        """
        http_url, _ = beava_server
        with HttpTransport(http_url) as t:
            pre = t.send_ping()
            assert pre["pong"] is True, f"pre-register pong=True; got {pre}"
            pre_version = pre["registry_version"]
            assert isinstance(pre_version, int) and pre_version >= 0, (
                f"pre-register registry_version must be non-negative int; got {pre}"
            )

            t.send_register(VALID_REGISTER_PAYLOAD)
            post = t.send_ping()
            assert post["pong"] is True, f"post-register pong=True; got {post}"
            assert post["registry_version"] == pre_version + 1, (
                f"post-register registry_version must bump by 1; "
                f"pre={pre_version}, post={post['registry_version']}"
            )
