"""Pytest fixtures for beava Python SDK tests.

Session fixture `beava_binary` runs `cargo build --bin beava --quiet` once per test
session (cached across tests). Each test that uses `beava_server` pays ~150ms spawn
cost; the `beava_binary` session fixture caches the cargo build.

Port-override mechanism (verified from crates/beava-server/src/cli.rs and
crates/beava-core/src/config.rs):
  - The binary reads a YAML config via --config flag.
  - Env var overrides: BEAVA_LISTEN_ADDR=127.0.0.1:0 for HTTP (port 0 = OS-assigns);
    BEAVA_TCP_PORT=0 for TCP (port 0 = OS-assigns).
  - IMPORTANT: The binary writes JSON structured logs to STDOUT (not stderr).
    The bind log lines appear on stdout:
      {"kind":"server.http_bound","addr":"127.0.0.1:NNNN",...}
      {"kind":"server.tcp_bound","addr":"127.0.0.1:NNNN",...}
  - BEAVA_DEV_ENDPOINTS=1 mounts GET /registry (used by Plan 03-06 smoke).
"""

from __future__ import annotations

import json
import os
import subprocess
import threading
from pathlib import Path
from typing import Generator

import pytest


@pytest.fixture(scope="session")
def beava_binary(pytestconfig: pytest.Config) -> Path:
    """Build the beava binary once per test session via cargo.

    pytestconfig.rootpath is the pytest rootdir (python/); the repo root is one
    level up.  Returns the path to target/debug/beava.  Raises pytest.fail on
    build failure so the whole session fails fast with a clear message.
    """
    repo_root = Path(str(pytestconfig.rootpath)).parent
    result = subprocess.run(
        ["cargo", "build", "--bin", "beava", "--quiet"],
        cwd=repo_root,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        pytest.fail(
            f"cargo build --bin beava failed (exit {result.returncode}):\n"
            f"stdout: {result.stdout}\n"
            f"stderr: {result.stderr}"
        )
    binary = repo_root / "target" / "debug" / "beava"
    if not binary.is_file():
        pytest.fail(f"Binary not found at {binary} after successful build")
    return binary


@pytest.fixture
def beava_server(
    beava_binary: Path, tmp_path: Path
) -> Generator[tuple[str, str], None, None]:
    """Spawn a beava server on ephemeral HTTP and TCP ports.

    Parses stdout for server.http_bound + server.tcp_bound JSON log lines to
    discover the OS-assigned ports.  Yields (http_url, tcp_url).  Sends SIGTERM
    on teardown and waits up to 5s; SIGKILL if the process doesn't exit.

    Spawn uses env var overrides (not CLI flags) because the binary only accepts
    --config for a YAML file; there is no --http-port / --tcp-port CLI flag.
    Port overrides via env vars:
      BEAVA_LISTEN_ADDR=127.0.0.1:0  (HTTP listener — port 0 → OS assigns)
      BEAVA_TCP_PORT=0                (TCP listener — port 0 → OS assigns)

    WAL + snapshot isolation: ``BEAVA_WAL_DIR`` and ``BEAVA_SNAPSHOT_DIR``
    point at the per-test ``tmp_path`` so each test starts with a clean
    registry (registry_version=1 fresh-boot, no carried-over event
    descriptors). Without isolation, a test calling ``register("Foo")``
    leaves Foo in the default ``./.beava/`` and subsequent tests see it
    as ``already_present`` — caught while wiring the F2 /ping
    registry-version delta assertion.
    """
    wal_dir = tmp_path / "wal"
    snap_dir = tmp_path / "snapshots"
    wal_dir.mkdir()
    snap_dir.mkdir()
    env = {
        **os.environ,
        "BEAVA_LISTEN_ADDR": "127.0.0.1:0",
        "BEAVA_TCP_PORT": "0",
        "BEAVA_WAL_DIR": str(wal_dir),
        "BEAVA_SNAPSHOT_DIR": str(snap_dir),
        "BEAVA_DEV_ENDPOINTS": "1",  # expose GET /registry for Plan 03-06 smoke
    }

    # Pass --config /dev/null so the binary starts with all-defaults regardless of
    # the caller's CWD (default config path ./beava.yaml may not exist in python/).
    # All meaningful settings are overridden via env vars above.
    # NOTE: The beava binary writes JSON structured logs to STDOUT (not stderr).
    # We must pipe stdout to parse bind-address lines; stderr is discarded.
    proc = subprocess.Popen(
        [str(beava_binary), "--config", "/dev/null"],
        stdout=subprocess.PIPE,  # JSON log lines land on stdout
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
                continue
            kind = rec.get("kind", "")
            if kind == "server.http_bound":
                http_addr.append(rec["addr"])
            elif kind == "server.tcp_bound":
                tcp_addr.append(rec["addr"])
            if http_addr and tcp_addr:
                ready.set()

    t = threading.Thread(target=_reader, daemon=True)
    t.start()

    if not ready.wait(timeout=5.0):
        proc.kill()
        proc.wait()
        if proc.stdout:
            proc.stdout.close()  # unblocks the _reader thread; prevents fd leak on CI
        error_detail = f"http_addr={http_addr}, tcp_addr={tcp_addr}"
        pytest.fail(
            f"beava server did not emit both bind log lines within 5s: {error_detail}"
        )

    http_url = f"http://{http_addr[0]}"
    tcp_url = f"tcp://{tcp_addr[0]}"

    yield http_url, tcp_url

    proc.terminate()
    try:
        proc.wait(timeout=5.0)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
