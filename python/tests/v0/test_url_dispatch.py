"""URL-scheme dispatch integration tests (D-03 MUST-FIX).

Covers SDK-WIRE-03 from `.planning/phases/13.7.5-pre-oss-code-polish/COVERAGE-GAPS.md`
§ Python gaps with four subcases:

  - ``test_http_url_picks_http_transport``      — ``http://...`` → ``HttpTransport``
  - ``test_tcp_url_picks_tcp_transport``        — ``tcp://...``  → ``TcpTransport``
  - ``test_no_url_picks_embed_transport``       — ``url=None``   → ``EmbedTransport``
  - ``test_unknown_scheme_raises_value_error``  — unsupported scheme → ``ValueError``

`test_transport_equivalence.py` parametrizes embed/http/tcp but does not
assert the transport-selection mechanism itself; this file pins the
``urlparse(url).scheme`` dispatch rule documented in ``python/beava/_app.py``
+ ``python/beava/_transport.py::make_transport``.

Anti-pattern guard (Phase 13.5.1 D-05, USER-LOCKED): NO mock objects —
the http and tcp tests spawn a real subprocess via
``beava._embed.spawn_embedded_server`` and connect a real ``bv.App(url=...)``
to it. The unknown-scheme test is the only one without an engine — it
asserts the constructor raises before any transport is materialized, so
spawning a subprocess would be wasteful (no transport call is made).
"""
from __future__ import annotations

import subprocess
from typing import Any, Generator

import pytest

import beava as bv
from beava._embed import spawn_embedded_server, teardown_process
from beava._transport import EmbedTransport, HttpTransport, TcpTransport

from ._helpers import _engine_available

pytestmark = pytest.mark.skipif(
    not _engine_available(),
    reason="requires Phase 13.4 engine + Phase 13.5 SDK rewrite + Phase 13.5.1 transport-impl",
)


@pytest.fixture
def embedded_server() -> Generator[tuple[str, str], None, None]:
    """Spawn a real beava subprocess and yield ``(http_url, tcp_url)``.

    Used by ``test_http_url_picks_http_transport`` and
    ``test_tcp_url_picks_tcp_transport`` so they connect a real ``bv.App``
    to a real engine, then assert the transport-selection result before the
    fixture tears the subprocess down.
    """
    proc: subprocess.Popen[bytes]
    proc, http_url, tcp_url, _env = spawn_embedded_server(test_mode=True)
    try:
        yield http_url, tcp_url
    finally:
        teardown_process(proc)


def test_http_url_picks_http_transport(embedded_server: tuple[str, str]) -> None:
    """SDK-WIRE-03: ``http://...`` URL routes to :class:`HttpTransport`.

    Constructs a ``bv.App(url="http://...")``, enters the context manager
    (which materializes the transport), then asserts ``app._transport`` is
    an :class:`HttpTransport` instance — verifying ``urlparse(url).scheme``
    dispatch in ``make_transport``.
    """
    http_url, _tcp_url = embedded_server
    with bv.App(url=http_url) as app:
        assert isinstance(app._transport, HttpTransport), (
            f"http:// URL must select HttpTransport; "
            f"got {type(app._transport).__name__}"
        )
        # Smoke: also verify the transport actually talks to the engine —
        # protects against a no-op transport that satisfies the type
        # assertion but is non-functional. Post-F2 the HTTP transport's
        # /ping returns {"pong": true, "registry_version": <n>}; pre-F2
        # binaries in the discover_binary search path may still raise
        # NotImplementedError. Either path proves the transport works.
        ping_or_register: Any
        try:
            ping_or_register = app.ping()
        except NotImplementedError:
            @bv.event
            class _Probe:
                user_id: str

            ping_or_register = app.register(_Probe)
        assert ping_or_register is not None


def test_tcp_url_picks_tcp_transport(embedded_server: tuple[str, str]) -> None:
    """SDK-WIRE-03: ``tcp://...`` URL routes to :class:`TcpTransport`.

    Constructs a ``bv.App(url="tcp://...")``, enters the context manager,
    then asserts ``app._transport`` is a :class:`TcpTransport` instance.
    """
    _http_url, tcp_url = embedded_server
    with bv.App(url=tcp_url) as app:
        assert isinstance(app._transport, TcpTransport), (
            f"tcp:// URL must select TcpTransport; "
            f"got {type(app._transport).__name__}"
        )
        # Smoke: ping round-trip confirms the TCP connection is live.
        result = app.ping()
        assert isinstance(result, dict), (
            f"ping must return a dict; got {type(result).__name__}"
        )


def test_no_url_picks_embed_transport() -> None:
    """SDK-WIRE-03: ``bv.App()`` with no URL routes to :class:`EmbedTransport`.

    Embed mode spawns a local subprocess and connects via TCP; the
    returned transport is :class:`EmbedTransport` (which wraps a
    :class:`TcpTransport` plus the subprocess handle).
    """
    with bv.App(test_mode=True) as app:
        assert isinstance(app._transport, EmbedTransport), (
            f"no-URL bv.App() must select EmbedTransport; "
            f"got {type(app._transport).__name__}"
        )


def test_unknown_scheme_raises_value_error() -> None:
    """SDK-WIRE-03: unsupported URL scheme raises :class:`ValueError`.

    Per ``python/beava/_app.py::App.__init__``: schemes other than
    ``http`` / ``https`` / ``tcp`` (and the ``url=None`` embed-mode
    sentinel) raise ``ValueError`` at construction with a message
    enumerating the supported schemes. The check happens BEFORE any
    transport is materialized, so the test does not need an engine to
    pass.

    The COVERAGE-GAPS row text says "ws://...", but ``redis://`` /
    ``ftp://`` / etc. would all trip the same path; ``ws://`` is the
    canonical "unsupported but plausible" example used here.
    """
    with pytest.raises(ValueError) as exc_info:
        bv.App(url="ws://localhost:7379")
    msg = str(exc_info.value)
    assert "ws" in msg, (
        f"unknown-scheme ValueError must mention the offending scheme; got {msg!r}"
    )
    # Per python/beava/_app.py the message is shaped
    # ``unsupported URL scheme: 'ws'``; we also accept any error mentioning
    # the canonical supported set so a future error-message rewrite that
    # keeps the contract still passes the test.
    assert "scheme" in msg.lower() or "http" in msg or "tcp" in msg, (
        f"unknown-scheme ValueError must mention 'scheme' or list supported schemes; "
        f"got {msg!r}"
    )
