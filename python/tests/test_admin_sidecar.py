"""Pytest coverage for the admin sidecar HTTP endpoints.

These tests close the audit gap: ``/health``, ``/ready``, ``/metrics``, and
``/registry`` exist in Rust (``crates/beava-server/src/http_admin.rs``) but had
no Python-side coverage before this file. Each test spawns a real ``beava``
binary with ``BEAVA_ADMIN_ADDR=127.0.0.1:<port>`` so the admin sidecar is
reachable on a known port (the binary does not emit a ``server.admin_bound``
JSON log line, so we cannot rely on parsing stdout the way ``http_bound`` and
``tcp_bound`` work).
"""
from __future__ import annotations

import json
import os
import socket
import subprocess
import threading
import time
from pathlib import Path
from typing import Generator

import httpx
import pytest

_BIND_TIMEOUT_S = 10.0
_READY_TIMEOUT_S = 10.0


def _free_port() -> int:
    """Reserve an ephemeral port and return it after closing the socket.

    There is an inherent race between close() and the beava binary's bind,
    but on Linux/macOS the port is held briefly in TIME_WAIT only after a
    connection actually transits; an unused listener freed this way is
    immediately reusable.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def _spawn_with_admin(
    binary: Path,
    wal_dir: Path,
    snap_dir: Path,
    admin_port: int,
    extra_env: dict[str, str] | None = None,
) -> tuple[subprocess.Popen[bytes], str, str, threading.Thread]:
    """Spawn ``beava`` bound to OS-assigned HTTP/TCP ports and a fixed admin
    port. Returns ``(proc, http_url, admin_url, reader_thread)``.
    """
    env = {
        **os.environ,
        "BEAVA_LISTEN_ADDR": "127.0.0.1:0",
        "BEAVA_TCP_PORT": "0",
        "BEAVA_ADMIN_ADDR": f"127.0.0.1:{admin_port}",
        "BEAVA_WAL_DIR": str(wal_dir),
        "BEAVA_SNAPSHOT_DIR": str(snap_dir),
        "BEAVA_DEV_ENDPOINTS": "1",
    }
    if extra_env:
        env.update(extra_env)

    proc = subprocess.Popen(
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

    if not ready.wait(timeout=_BIND_TIMEOUT_S):
        proc.kill()
        proc.wait()
        if proc.stdout is not None:
            proc.stdout.close()
        pytest.fail(
            f"beava server did not bind within {_BIND_TIMEOUT_S}s; "
            f"http_addr={http_addr}, tcp_addr={tcp_addr}"
        )

    http_url = f"http://{http_addr[0]}"
    admin_url = f"http://127.0.0.1:{admin_port}"
    return proc, http_url, admin_url, t


def _wait_for_admin(admin_url: str, *, timeout: float = _READY_TIMEOUT_S) -> None:
    """Poll the admin ``/health`` endpoint until 200 or timeout.

    The admin axum task spawns inside ``ServerV18::bind`` and the
    listener accepts almost immediately, but on a cold cargo build the
    process may still be initialising tokio when the HTTP listener
    binds.  Poll briefly to remove the flake.
    """
    deadline = time.monotonic() + timeout
    last_err: Exception | None = None
    while time.monotonic() < deadline:
        try:
            r = httpx.get(f"{admin_url}/health", timeout=1.0)
            if r.status_code == 200:
                return
            last_err = RuntimeError(f"/health returned {r.status_code}")
        except Exception as e:  # noqa: BLE001
            last_err = e
        time.sleep(0.05)
    pytest.fail(f"admin endpoint never reached READY at {admin_url}: {last_err!r}")


@pytest.fixture
def admin_server(
    beava_binary: Path, tmp_path: Path
) -> Generator[tuple[str, str, int], None, None]:
    """Yield ``(http_url, admin_url, admin_port)`` for an admin-bound boot."""
    wal_dir = tmp_path / "wal"
    snap_dir = tmp_path / "snap"
    wal_dir.mkdir()
    snap_dir.mkdir()
    admin_port = _free_port()
    proc, http_url, admin_url, _t = _spawn_with_admin(
        beava_binary, wal_dir, snap_dir, admin_port
    )
    try:
        _wait_for_admin(admin_url)
        yield http_url, admin_url, admin_port
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5.0)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


# ─── tests ────────────────────────────────────────────────────────────────────


def test_health_endpoint_returns_200_json(
    admin_server: tuple[str, str, int],
) -> None:
    """``/health`` returns 200 with a JSON body containing ``status: ok``."""
    _http_url, admin_url, _port = admin_server
    r = httpx.get(f"{admin_url}/health", timeout=2.0)
    assert r.status_code == 200, f"unexpected status: {r.status_code}; body={r.text!r}"
    body = r.json()
    assert isinstance(body, dict), f"/health body must be a JSON object: {body!r}"
    assert body.get("status") == "ok", f"/health body missing status=ok: {body!r}"
    # Per http_admin.rs the admin runtime tags every response with X-Runtime: tokio.
    assert r.headers.get("x-runtime") == "tokio", (
        f"admin response must carry X-Runtime: tokio header; got headers={dict(r.headers)!r}"
    )


def test_ready_endpoint_returns_200_json(
    admin_server: tuple[str, str, int],
) -> None:
    """``/ready`` returns 200 with a JSON body indicating readiness."""
    _http_url, admin_url, _port = admin_server
    r = httpx.get(f"{admin_url}/ready", timeout=2.0)
    assert r.status_code == 200, f"unexpected status: {r.status_code}; body={r.text!r}"
    body = r.json()
    assert isinstance(body, dict), f"/ready body must be a JSON object: {body!r}"
    assert body.get("status") == "ready", f"/ready body missing status=ready: {body!r}"


def test_metrics_endpoint_exposes_prometheus_text(
    admin_server: tuple[str, str, int],
) -> None:
    """``/metrics`` returns Prometheus text exposition with ``beava_`` metrics."""
    _http_url, admin_url, _port = admin_server
    r = httpx.get(f"{admin_url}/metrics", timeout=2.0)
    assert r.status_code == 200, f"unexpected status: {r.status_code}; body={r.text!r}"
    # http_admin.rs emits `text/plain; version=0.0.4; charset=utf-8`.
    ctype = r.headers.get("content-type", "")
    assert ctype.startswith("text/plain"), (
        f"/metrics Content-Type must be text/plain (Prometheus exposition), got {ctype!r}"
    )
    assert "version=0.0.4" in ctype, (
        f"/metrics Content-Type must include Prometheus version=0.0.4, got {ctype!r}"
    )
    body = r.text
    for metric in (
        "beava_registry_version",
        "beava_node_count",
        "beava_runtime_kind",
        "beava_entity_count_resident",
    ):
        assert metric in body, f"/metrics body missing {metric!r}; body={body!r}"
    # HELP/TYPE lines must accompany at least one metric (sanity-check on the
    # exposition shape, not on every metric).
    assert "# HELP beava_registry_version" in body, (
        f"/metrics body missing HELP comment; body={body!r}"
    )
    assert "# TYPE beava_registry_version gauge" in body, (
        f"/metrics body missing TYPE comment; body={body!r}"
    )


def test_registry_endpoint_returns_current_descriptors(
    admin_server: tuple[str, str, int],
) -> None:
    """``/registry`` returns the registry snapshot JSON shape.

    Pins the contract surfaced by ``http_admin::registry_handler``: a JSON
    object with ``version`` and ``node_count`` integer fields.  See
    :func:`test_registry_endpoint_reflects_registrations_xfail` for the
    post-register update behaviour.
    """
    _http_url, admin_url, _port = admin_server

    r = httpx.get(f"{admin_url}/registry", timeout=2.0)
    assert r.status_code == 200, f"unexpected status: {r.status_code}; body={r.text!r}"
    body = r.json()
    assert isinstance(body, dict), f"/registry must return a JSON object: {body!r}"
    assert "version" in body, f"/registry missing version: {body!r}"
    assert "node_count" in body, f"/registry missing node_count: {body!r}"
    assert isinstance(body["version"], int), (
        f"/registry version must be an int: {body!r}"
    )
    assert isinstance(body["node_count"], int), (
        f"/registry node_count must be an int: {body!r}"
    )


@pytest.mark.xfail(
    strict=False,
    reason=(
        "Audit gap surfaced 2026-05-14: the admin sidecar's "
        "SharedRegistrySnapshot is constructed with default() in "
        "ServerV18::bind and never updated by the register / apply path, "
        "so /registry permanently reports version=0 / node_count=0 even "
        "after a successful POST /register. Fix is to plumb the snapshot "
        "into the registry-update site in apply_shard.rs / register.rs."
    ),
)
def test_registry_endpoint_reflects_registrations_xfail(
    admin_server: tuple[str, str, int],
) -> None:
    """``/registry`` SHOULD advance ``version`` + ``node_count`` after a
    register; today it does not (xfail until the snapshot is wired up)."""
    http_url, admin_url, _port = admin_server

    cold = httpx.get(f"{admin_url}/registry", timeout=2.0).json()
    cold_version = int(cold["version"])
    cold_nodes = int(cold["node_count"])

    payload = {
        "nodes": [
            {
                "kind": "event",
                "name": "AdminProbe",
                "schema": {
                    "fields": {"user_id": "str", "amount": "f64"},
                    "optional_fields": [],
                },
            },
            {
                "kind": "derivation",
                "name": "AdminProbeAgg",
                "output_kind": "table",
                "upstreams": ["AdminProbe"],
                "ops": [
                    {
                        "op": "group_by",
                        "keys": ["user_id"],
                        "agg": {"cnt": {"op": "count", "params": {}}},
                    }
                ],
                "schema": {
                    "fields": {"user_id": "str", "cnt": "i64"},
                    "optional_fields": [],
                },
                "table_primary_key": ["user_id"],
            },
        ]
    }
    reg = httpx.post(
        f"{http_url}/register",
        content=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )
    assert reg.status_code == 200, f"/register failed: {reg.status_code} {reg.text!r}"

    warm = httpx.get(f"{admin_url}/registry", timeout=2.0).json()
    assert int(warm["version"]) > cold_version, (
        f"/registry version did not advance after /register: "
        f"cold={cold_version}, warm={warm!r}"
    )
    assert int(warm["node_count"]) > cold_nodes, (
        f"/registry node_count did not grow after /register: "
        f"cold={cold_nodes}, warm={warm!r}"
    )


def test_admin_addr_env_var_binds_admin_to_custom_port(
    beava_binary: Path, tmp_path: Path
) -> None:
    """``BEAVA_ADMIN_ADDR`` env var binds the admin sidecar to the given port."""
    wal_dir = tmp_path / "wal"
    snap_dir = tmp_path / "snap"
    wal_dir.mkdir()
    snap_dir.mkdir()
    custom_port = _free_port()
    proc, _http_url, admin_url, _t = _spawn_with_admin(
        beava_binary, wal_dir, snap_dir, custom_port
    )
    try:
        _wait_for_admin(admin_url)
        # The admin URL must actually be the env-supplied port — assert by
        # reading /health on http://127.0.0.1:<custom_port> directly.
        r = httpx.get(f"http://127.0.0.1:{custom_port}/health", timeout=2.0)
        assert r.status_code == 200, (
            f"admin /health on env-supplied port {custom_port} returned "
            f"{r.status_code}; body={r.text!r}"
        )
        assert r.json().get("status") == "ok"
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5.0)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
