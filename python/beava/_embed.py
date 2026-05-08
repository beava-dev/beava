"""Embed-mode subprocess launcher.

Binary discovery order (locked):
  1. ``$BEAVA_BINARY`` env var — if set, the path MUST exist and be
     executable; otherwise :class:`BinaryNotFoundError` is raised
     immediately (no fallthrough — silent fallback would mask user
     misconfiguration).
  2. ``beava`` on PATH via ``shutil.which``, BUT shebang scripts
     (``#!``) are skipped — the ``[project.scripts] beava`` entry
     installs a Python shim of that name, and execing into it would
     re-enter ``discover_binary``, find itself, and loop. Only native
     binaries (ELF / Mach-O / etc.) qualify.
  3. Walk CWD upward looking for ``target/debug/beava`` (dev-loop
     convenience).
  4. Raise :class:`BinaryNotFoundError` with install guidance.

Security: only paths produced by the discovery order above are executed;
no shell interpolation; no arbitrary-command execution.
"""

from __future__ import annotations

import atexit
import json
import logging
import os
import shutil
import subprocess
import tempfile
import threading
import time
from pathlib import Path

from beava._errors import BinaryNotFoundError

_log = logging.getLogger("beava.embed")


def _is_shebang_script(path: str) -> bool:
    """True if ``path`` is a text file starting with ``#!`` (interpreter).

    The Python ``[project.scripts]`` entry installs a ``beava`` console
    script as a shebang-headed Python file (e.g. ``#!/usr/bin/env
    python3``). Treating it as the server binary causes an exec loop:
    spawn shim → shim's main runs ``discover_binary`` → finds itself
    → execs into itself → ...

    Native server binaries are ELF / Mach-O / PE — none start with
    ``#!``. A read failure (permission denied, broken symlink) is
    treated as "not a shebang" so we don't reject candidates we
    couldn't classify; the subsequent ``Popen`` will surface any
    real exec error.
    """
    try:
        with open(path, "rb") as f:
            head = f.read(2)
    except OSError:
        return False
    return head == b"#!"


def discover_binary() -> Path:
    """Locate the beava binary using the 4-step discovery order.

    Returns:
        Path to an executable beava binary.

    Raises:
        BinaryNotFoundError: Binary cannot be found using any of the 4 steps.
    """
    # An explicit BEAVA_BINARY override must be valid if set — silent
    # fallback to PATH lookup would mask the misconfiguration.
    env_val = os.environ.get("BEAVA_BINARY")
    if env_val is not None:
        p = Path(env_val)
        if p.is_file() and os.access(p, os.X_OK):
            return p
        raise BinaryNotFoundError(
            f"BEAVA_BINARY={env_val!r} is set but the path is not an executable file. "
            f"Unset BEAVA_BINARY or fix the path."
        )

    # Scan every PATH directory rather than just the first match. A user
    # with both `pip install beava` (shim in e.g. ~/.local/bin) and
    # `cargo install beava-server` (binary in ~/.cargo/bin) on PATH must
    # find the native binary regardless of which directory comes first.
    # `shutil.which(name, path=dir)` enforces both file-existence + the
    # executable bit and handles platform quirks (PATHEXT on Windows).
    for path_dir in os.environ.get("PATH", "").split(os.pathsep):
        if not path_dir:
            continue
        resolved = shutil.which("beava", path=path_dir)
        if resolved is None:
            continue
        if _is_shebang_script(resolved):
            # Python console_script shim from `[project.scripts]` —
            # execing into it would loop. Try the next PATH entry.
            continue
        return Path(resolved)

    for parent in [Path.cwd(), *Path.cwd().parents]:
        candidate = parent / "target" / "debug" / "beava"
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate

    raise BinaryNotFoundError(
        "beava binary not found. Install with one of:\n"
        "  brew install beava\n"
        "  pip install beava[server]\n"
        "  docker pull beava/beava\n"
        "Or set BEAVA_BINARY=/path/to/beava."
    )


def spawn_embedded_server(
    startup_timeout: float = 5.0,
    *,
    test_mode: bool = False,
) -> tuple[subprocess.Popen[bytes], str, str, dict[str, str]]:
    """Spawn a local beava server on ephemeral ports and wait until it is ready.

    Reads stderr line-by-line (in a background thread) until both
    ``{"kind":"server.http_bound","addr":"..."}`` and
    ``{"kind":"server.tcp_bound","addr":"..."}`` appear.

    After both bind events are received, remaining stderr lines are forwarded
    to the ``beava.embed`` logger at DEBUG level.

    **Disk lifecycle.** Each spawn allocates a unique tmpdir at
    ``$TMPDIR/beava-embed-<pid>-<unix-ms>-<hex>/`` holding the WAL
    (``./wal``) and snapshot (``./snapshots``) sub-dirs. The path is
    registered with ``atexit.register(shutil.rmtree, ...,
    ignore_errors=True)`` so the dir is reaped at Python interpreter
    shutdown. SIGKILL'd processes leave the dir for the OS tmpfs reaper
    (typical reapers handle ``$TMPDIR`` aging within days).

    Args:
        startup_timeout: Seconds to wait for both bind log lines.

    Returns:
        ``(proc, http_url, tcp_url)`` where ``http_url = "http://127.0.0.1:PORT"``
        and ``tcp_url = "tcp://127.0.0.1:PORT"``.

    Raises:
        TimeoutError: Neither/one bind event arrived within ``startup_timeout``.
        BinaryNotFoundError: Binary discovery failed.
    """
    binary = discover_binary()

    # Port overrides go via env vars (the binary's CLI has no
    # --http-port/--tcp-port flags). Pass a minimal *copy* of the parent
    # env (not a bare dict) so DYLD_LIBRARY_PATH / locale / etc. propagate;
    # only the keys we explicitly care about are overridden.
    #
    # WAL/snapshot dirs are pinned per-spawn under $TMPDIR so parallel test
    # spawns can't collide on the default ./beava-wal/ location (the binary
    # refuses to boot when a prior process leaves a 0-byte WAL file
    # behind).
    unique = f"{os.getpid()}-{int(time.time() * 1000)}-{os.urandom(4).hex()}"
    spawn_root = Path(tempfile.gettempdir()) / f"beava-embed-{unique}"
    wal_dir = spawn_root / "wal"
    snapshot_dir = spawn_root / "snapshots"
    wal_dir.mkdir(parents=True, exist_ok=True)
    snapshot_dir.mkdir(parents=True, exist_ok=True)

    # Reap per-spawn tmpdirs at interpreter shutdown so a long-running test
    # process doesn't accumulate disk. ``ignore_errors=True`` because the
    # binary may still be holding a handle at shutdown; the OS tmpfs reaper
    # backs up the no-cleanup case (e.g. SIGKILL).
    atexit.register(shutil.rmtree, str(spawn_root), ignore_errors=True)

    env = {
        **os.environ,
        "BEAVA_LISTEN_ADDR": "127.0.0.1:0",
        "BEAVA_TCP_PORT": "0",
        # The admin sidecar must also bind to an OS-allocated ephemeral
        # port; otherwise parallel test spawns (or a stale beava on
        # the default port) fail boot with "Address already in use".
        "BEAVA_ADMIN_ADDR": "127.0.0.1:0",
        "BEAVA_DEV_ENDPOINTS": "1",
        "BEAVA_WAL_DIR": str(wal_dir),
        "BEAVA_SNAPSHOT_DIR": str(snapshot_dir),
    }
    if test_mode:
        env["BEAVA_TEST_MODE"] = "1"

    # `--config /dev/null` makes the binary boot with all-defaults regardless
    # of the caller's CWD; all meaningful settings come from env vars above.
    # The binary writes its JSON structured logs (including the
    # bind-address records) to STDOUT, not stderr — capture stdout, drop
    # stderr.
    proc: subprocess.Popen[bytes] = subprocess.Popen(
        [str(binary), "--config", "/dev/null"],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        env=env,
    )

    http_addr: list[str] = []
    tcp_addr: list[str] = []
    ready = threading.Event()

    def _reader() -> None:
        assert proc.stdout is not None
        for raw in proc.stdout:
            line = raw.decode("utf-8", errors="replace").rstrip()
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                # Non-JSON line (e.g. the human-readable banner) — log at
                # DEBUG once startup is confirmed; skip silently before that.
                if ready.is_set():
                    _log.debug("non-json stdout: %s", line)
                continue

            kind = rec.get("kind", "")
            if kind == "server.http_bound":
                http_addr.append(rec.get("addr", ""))
            elif kind == "server.tcp_bound":
                tcp_addr.append(rec.get("addr", ""))

            if http_addr and tcp_addr:
                if not ready.is_set():
                    ready.set()
                else:
                    _log.debug("%s", line)
        # Stdout pipe closed (process exited) — signal readiness so callers
        # don't wait the full startup_timeout when teardown raced startup.
        ready.set()

    t = threading.Thread(target=_reader, daemon=True)
    t.start()

    ready.wait(timeout=startup_timeout)

    # `ready` may have been set by the EOF sentinel (process exited early)
    # instead of by both bind events; verify before claiming success.
    if not (http_addr and tcp_addr):
        proc.kill()
        proc.wait()
        if proc.stdout:
            proc.stdout.close()  # unblocks the _reader thread
        raise TimeoutError(
            f"embed-mode server did not bind within {startup_timeout}s "
            f"(http_addr={http_addr}, tcp_addr={tcp_addr}). "
            f"Check that the beava binary starts correctly."
        )

    return proc, f"http://{http_addr[0]}", f"tcp://{tcp_addr[0]}", env


def teardown_process(proc: subprocess.Popen[bytes], timeout: float = 5.0) -> None:
    """Send SIGTERM; wait for ``timeout`` seconds; SIGKILL if still running.

    Args:
        proc: The subprocess to terminate.
        timeout: Seconds to wait for graceful shutdown before SIGKILL.
    """
    proc.terminate()
    try:
        proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
