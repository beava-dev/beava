"""Connection-drop / no-auto-retry contract tests (D-03 MUST-FIX).

Covers two MUST-FIX rows from `.planning/phases/13.7.5-pre-oss-code-polish/COVERAGE-GAPS.md`
§ Python gaps:

  - SDK-APP-RECONNECT       → ``test_transport_recovers_after_server_restart``
  - SDK-APP-MAX-RETRIES-0   → ``test_max_retries_zero_surfaces_immediately``

Per the COVERAGE-GAPS rationale, **v0's beava SDK does not implement
reconnect.** The older ``python/tally/_client.py`` had a reconnect path; the
new ``python/beava/_transport.py`` does not, and there is no ``max_retries``
parameter — the implicit default is fail-fast (no retries).

These tests therefore document the v0 contract:

  - A dropped connection raises a transport-layer exception on the next
    call, not silent recovery.
  - A transient transport error (here: connection refused at construction)
    surfaces immediately to the caller — there is no exponential-backoff
    loop chewing wall-clock time.

v0.1+ may add a ``max_retries=N`` parameter; if/when that lands, the
contract assertions below evolve. Each test docstring references this
COVERAGE-GAPS row so a future SDK author finds the v0 baseline before
introducing retries.

Anti-pattern guard (Phase 13.5.1 D-05, USER-LOCKED): NO mock objects —
the reconnect test spawns a real subprocess via
``beava._embed.spawn_embedded_server`` and then SIGKILLs it; the
max_retries test points at an unreachable port (no engine spawn needed,
which is the point — fail-fast must not depend on the engine being
contactable).
"""
from __future__ import annotations

import socket
import subprocess
import time

import pytest

import beava as bv
from beava._embed import spawn_embedded_server, teardown_process
from beava._errors import RegistrationError
from beava._wire import IncompleteFrame

from ._helpers import _engine_available

pytestmark = pytest.mark.skipif(
    not _engine_available(),
    reason="requires Phase 13.4 engine + Phase 13.5 SDK rewrite + Phase 13.5.1 transport-impl",
)


def _find_unused_port() -> int:
    """Return an OS-assigned ephemeral port that is currently free.

    The returned port is closed before return; a subsequent ``socket.connect``
    against ``127.0.0.1:<port>`` is overwhelmingly likely to surface
    ``ConnectionRefusedError`` immediately. (A race is theoretically
    possible if another process grabs the port in the microseconds between
    close and connect; in practice this is reliable on CI runners.)
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        port: int = s.getsockname()[1]
    return port


def test_transport_recovers_after_server_restart() -> None:
    """SDK-APP-RECONNECT: v0 SDK does NOT auto-reconnect — dropped connection raises.

    Per COVERAGE-GAPS.md SDK-APP-RECONNECT: the v0 ``python/beava``
    transport layer has no reconnect logic (the older ``python/tally``
    client had one; that path was deliberately not carried into the v0
    rewrite). The test asserts the v0 contract:

      1. Spawn a beava subprocess and connect a ``bv.App(url=tcp://...)``
         to it; verify a baseline call works (the connection is live).
      2. SIGKILL the subprocess so the TCP socket is dropped from the
         server side.
      3. Issue another call on the same App. The v0 contract is that this
         raises an exception (broken-pipe / connection-reset / equivalent
         transport-layer surface) — NOT that the SDK silently retries.

    If a future v0.1+ SDK adds reconnect, this test FAILS — that's the
    signal to update the contract: replace the raises-block with an
    expected-recovery assertion AND remove the SDK-APP-RECONNECT row from
    COVERAGE-GAPS.md.
    """
    proc: subprocess.Popen[bytes]
    proc, _http_url, tcp_url, _env = spawn_embedded_server(test_mode=True)

    try:
        with bv.App(url=tcp_url) as app:
            # Step 1: baseline ping confirms the connection is live.
            ping = app.ping()
            assert isinstance(ping, dict), (
                f"baseline ping must return a dict before kill; got {type(ping).__name__}"
            )

            # Step 2: SIGKILL the server. SIGTERM (proc.terminate) lets the
            # server drain cleanly; we want the harder kill so the socket
            # is dropped without a graceful FIN, surfacing as a transport
            # error on the next call.
            proc.kill()
            proc.wait()

            # Give the OS a tick to propagate the socket teardown — without
            # this, the next call sometimes blocks on the in-flight read
            # before raising.
            time.sleep(0.1)

            # Step 3: subsequent call must raise a transport-layer
            # exception. v0 does not auto-reconnect, so the call MUST NOT
            # silently succeed. The exact exception class depends on the OS
            # (macOS/Linux differ slightly) and the timing of the SIGKILL —
            # we accept the broad set that documents "transport broke":
            #   - ``IncompleteFrame`` (the SDK's framed-codec surface for
            #     "socket closed before a complete frame arrived" —
            #     ``beava._wire.IncompleteFrame``);
            #   - ``OSError`` / ``ConnectionError`` (kernel-level
            #     broken-pipe / connection-reset);
            #   - ``RegistrationError`` (the SDK's wrapped form for
            #     unexpected-frame replies);
            #   - ``EOFError`` (the alternative socket-closed surface on
            #     some Python builds).
            with pytest.raises((IncompleteFrame, OSError, RegistrationError, EOFError)):
                app.ping()
    finally:
        # ``proc`` may have been killed inside the try-block already;
        # teardown_process is idempotent (sends SIGTERM then SIGKILL with
        # bounded wait) so this is safe in either path.
        teardown_process(proc)


def test_max_retries_zero_surfaces_immediately() -> None:
    """SDK-APP-MAX-RETRIES-0: v0 SDK is fail-fast — transient error surfaces in <1s.

    Per COVERAGE-GAPS.md SDK-APP-MAX-RETRIES-0: the v0 SDK has no
    ``max_retries`` parameter; the implicit default is fail-fast (no
    retries, no exponential-backoff loop). The test asserts that pointing
    a ``bv.App`` at an unreachable URL surfaces the connection error
    immediately (< 1s wall-clock) — there is no silent retry loop chewing
    time before bubbling the failure to the caller.

    The 1-second budget is generous: a fail-fast SDK against an unreachable
    127.0.0.1 port resolves in milliseconds (kernel-level connect refused).
    A retry loop with even modest backoff would burn far more than 1s.
    """
    port = _find_unused_port()
    url = f"tcp://127.0.0.1:{port}"

    start = time.monotonic()
    raised: bool = False
    try:
        # Use a small explicit timeout so even if the SDK tried a
        # connect-with-long-timeout path (which it doesn't on 127.0.0.1
        # refused), we'd still fail fast. The default 30s timeout would
        # also resolve immediately on connection-refused, but pinning the
        # value documents that fail-fast does not depend on a low
        # timeout — it depends on no-retry-loop.
        with bv.App(url=url, timeout=0.5) as app:
            app.ping()
    except (IncompleteFrame, OSError, ConnectionError, RegistrationError, EOFError):
        raised = True
    elapsed = time.monotonic() - start

    assert raised, (
        f"v0 SDK against unreachable URL must raise a transport error; "
        f"none raised after {elapsed:.3f}s — auto-retry has been introduced?"
    )
    assert elapsed < 1.0, (
        f"v0 SDK must fail-fast (< 1s); got {elapsed:.3f}s — "
        f"a retry loop has been introduced without a corresponding "
        f"COVERAGE-GAPS update."
    )
