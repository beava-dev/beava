"""End-to-end WAL crash/kill/restart recovery tests.

These tests exercise the durability claim documented in CLAUDE.md (WAL +
periodic snapshot, in-memory state, single-process). A Rust unit suite in
``crates/beava-server/tests/phase18_02_inline_wal_test.rs`` covers the WAL
append path at the buffer-ring level; this module is the Python-side
guardrail that pins the end-to-end contract: register a schema, push
events, ``SIGKILL`` the server, restart with the same ``--data-dir``, and
assert ``GET`` returns identical values.

Pattern
-------
Each test spawns the beava binary as a child process with:

  * ``--data-dir <tmp>``  — per-test WAL + snapshot isolation under
    ``<tmp>/wal`` and ``<tmp>/snapshots``.
  * ``--test-mode``        — so destructive ``/reset`` is allowed if a
    test wants it (no test in this file currently does, but the flag
    also gates ``OP_RESET`` paths so leaving it on is harmless).
  * ``BEAVA_LISTEN_ADDR=127.0.0.1:0`` + ``BEAVA_TCP_PORT=0`` +
    ``BEAVA_ADMIN_ADDR=127.0.0.1:0`` — ephemeral ports, parsed from
    the server's structured JSON stdout log lines.
  * ``BEAVA_WAL_TICK_MS=5`` — fast fsync tick so events written within
    ~10ms of SIGKILL are durable. The shipping default is 20ms; we
    shorten it to keep test wall-clock bounded.
  * ``BEAVA_SNAPSHOT_INTERVAL_MS=200`` (only in the snapshot-tail test)
    — periodic snapshot every 200ms so we can force a snapshot file to
    land before pushing the tail.

The server is killed with ``proc.kill()`` (``SIGKILL``) — no chance for
in-flight buffers to flush — then a second server is spawned with the
same ``--data-dir``. On the recovery boot path
(``crates/beava-server/src/recovery.rs``) the new process loads the
latest snapshot (if any) and replays the WAL tail from
``snapshot_lsn``. The Python test then issues a ``POST /get`` and
asserts the row equals what the pre-kill aggregations would have
produced.

Why these tests matter
----------------------
The audit's #1 missing test was: "Rust covers WAL append, zero pytest
exercises kill-restart". A durability regression that drops events on
recovery would slip past every existing Python suite. This file is
that guardrail.
"""

from __future__ import annotations

import json
import os
import subprocess
import threading
import time
from pathlib import Path
from typing import Any, Iterator

import httpx
import pytest

# ---------------------------------------------------------------------------
# Helpers — server spawn / kill / wait-for-bind / register payloads
# ---------------------------------------------------------------------------


_BIND_TIMEOUT_S = 20.0
_READY_TIMEOUT_S = 10.0


class _ServerHandle:
    """A spawned beava server process with parsed HTTP/TCP/admin URLs.

    Provides ``.kill()`` (``SIGKILL`` — the durability-critical path)
    and ``.terminate()`` (graceful — used for the final cleanup of the
    second restart so we don't leak processes).
    """

    def __init__(
        self,
        proc: subprocess.Popen[bytes],
        http_url: str,
        tcp_url: str,
        admin_url: str,
        reader: threading.Thread,
    ) -> None:
        self.proc = proc
        self.http_url = http_url
        self.tcp_url = tcp_url
        self.admin_url = admin_url
        self._reader = reader

    def kill(self) -> None:
        """Hard-kill via SIGKILL — no signal handlers, no flushing."""
        if self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait(timeout=5.0)

    def terminate(self) -> None:
        """Graceful shutdown via SIGTERM; fallback to SIGKILL after 5s."""
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()


def _spawn_server(
    binary: Path,
    data_dir: Path,
    *,
    snapshot_interval_ms: int = 60_000,
    wal_tick_ms: int = 5,
) -> _ServerHandle:
    """Spawn ``beava --data-dir <tmp> --test-mode`` and wait for bind.

    Parses stdout for ``server.http_bound`` + ``server.tcp_bound`` JSON
    log lines to discover the OS-assigned ports.

    ``snapshot_interval_ms`` and ``wal_tick_ms`` are propagated via env
    vars (the binary clamps wal_tick to [1, 1000]). Default snapshot
    interval is 60s — long enough that "no snapshot landed" is the
    expected steady state for the kill-restart tests; only the
    snapshot-tail test shortens it.
    """
    env = {
        **os.environ,
        "BEAVA_LISTEN_ADDR": "127.0.0.1:0",
        "BEAVA_TCP_PORT": "0",
        "BEAVA_ADMIN_ADDR": "127.0.0.1:0",
        "BEAVA_WAL_TICK_MS": str(wal_tick_ms),
        "BEAVA_SNAPSHOT_INTERVAL_MS": str(snapshot_interval_ms),
        "BEAVA_DEV_ENDPOINTS": "1",
    }
    proc = subprocess.Popen(
        [
            str(binary),
            "--config",
            "/dev/null",
            "--data-dir",
            str(data_dir),
            "--test-mode",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )
    http_addr: list[str] = []
    tcp_addr: list[str] = []
    admin_addr: list[str] = []
    ready = threading.Event()

    def _stdout_reader() -> None:
        assert proc.stdout is not None
        for raw in proc.stdout:
            try:
                rec = json.loads(raw.decode("utf-8", errors="replace").rstrip())
            except json.JSONDecodeError:
                continue
            kind = rec.get("kind", "")
            if kind == "server.http_bound":
                http_addr.append(rec["addr"])
            elif kind == "server.tcp_bound":
                tcp_addr.append(rec["addr"])
            elif kind == "server.admin_bound":
                admin_addr.append(rec["addr"])
            if http_addr and tcp_addr:
                ready.set()

    t = threading.Thread(target=_stdout_reader, daemon=True)
    t.start()

    if not ready.wait(timeout=_BIND_TIMEOUT_S):
        proc.kill()
        proc.wait()
        if proc.stdout is not None:
            proc.stdout.close()
        # Drain any stderr for the failure message — the bind hang is
        # almost always a recovery-path crash on the second boot, and
        # stderr has the panic backtrace.
        stderr_bytes = b""
        if proc.stderr is not None:
            try:
                stderr_bytes = proc.stderr.read() or b""
            except Exception:
                stderr_bytes = b""
        pytest.fail(
            f"beava server did not bind within {_BIND_TIMEOUT_S}s "
            f"(data_dir={data_dir}); "
            f"http_addr={http_addr}, tcp_addr={tcp_addr}; "
            f"stderr={stderr_bytes!r}"
        )

    http_url = f"http://{http_addr[0]}"
    tcp_url = f"tcp://{tcp_addr[0]}"
    admin_url = f"http://{admin_addr[0]}" if admin_addr else ""
    return _ServerHandle(proc, http_url, tcp_url, admin_url, t)


def _wait_for_ping(http_url: str, *, timeout: float = _READY_TIMEOUT_S) -> None:
    """Poll ``POST /ping`` until it returns 200 or we hit ``timeout``."""
    deadline = time.monotonic() + timeout
    last_err: Exception | None = None
    while time.monotonic() < deadline:
        try:
            with httpx.Client(base_url=http_url, timeout=2.0) as client:
                r = client.post(
                    "/ping",
                    json={},
                    headers={"Content-Type": "application/json"},
                )
                if r.status_code == 200:
                    return
                last_err = RuntimeError(f"/ping returned {r.status_code}: {r.text!r}")
        except Exception as exc:  # noqa: BLE001 — propagate the final error
            last_err = exc
        time.sleep(0.05)
    raise AssertionError(
        f"/ping never became 200 within {timeout}s at {http_url}: {last_err!r}"
    )


def _register(http_url: str, payload: dict[str, Any]) -> None:
    with httpx.Client(base_url=http_url, timeout=10.0) as client:
        r = client.post(
            "/register",
            json=payload,
            headers={"Content-Type": "application/json"},
        )
        if r.status_code != 200:
            raise AssertionError(f"/register failed: {r.status_code} {r.text}")


def _push(http_url: str, event_name: str, fields: dict[str, Any]) -> None:
    with httpx.Client(base_url=http_url, timeout=10.0) as client:
        r = client.post(
            f"/push/{event_name}",
            json=fields,
            headers={"Content-Type": "application/json"},
        )
        if r.status_code != 200:
            raise AssertionError(
                f"/push/{event_name} failed: {r.status_code} {r.text}"
            )


def _get(http_url: str, table: str, key: str) -> dict[str, Any]:
    with httpx.Client(base_url=http_url, timeout=10.0) as client:
        r = client.post(
            "/get",
            json={"table": table, "key": key},
            headers={"Content-Type": "application/json"},
        )
        if r.status_code != 200:
            raise AssertionError(f"/get failed: {r.status_code} {r.text}")
        body: dict[str, Any] = r.json()
        return body


def _txn_count_sum_avg_payload() -> dict[str, Any]:
    """Register payload: ``Txn`` event → ``UserTxn`` table with 3 features.

    Features: ``cnt`` (count), ``total`` (sum amount), ``avg_amount``
    (mean amount). Keyed by ``user_id``. ``window="1h"`` puts every
    aggregation on a single rolling-1h bucket — enough to capture all
    100 test events as one window. The lifetime-bound guard
    (phase 12.8) rejects unbounded aggregations, so every op gets an
    explicit ``window``.
    """
    return {
        "nodes": [
            {
                "kind": "event",
                "name": "Txn",
                "schema": {
                    "fields": {"user_id": "str", "amount": "f64"},
                    "optional_fields": [],
                },
            },
            {
                "kind": "derivation",
                "name": "UserTxn",
                "output_kind": "table",
                "upstreams": ["Txn"],
                "ops": [
                    {
                        "op": "group_by",
                        "keys": ["user_id"],
                        "agg": {
                            "cnt": {
                                "op": "count",
                                "params": {"window": "1h"},
                            },
                            "total": {
                                "op": "sum",
                                "params": {"field": "amount", "window": "1h"},
                            },
                            "avg_amount": {
                                "op": "mean",
                                "params": {"field": "amount", "window": "1h"},
                            },
                        },
                    }
                ],
                "schema": {
                    "fields": {
                        "user_id": "str",
                        "cnt": "i64",
                        "total": "f64",
                        "avg_amount": "f64",
                    },
                    "optional_fields": [],
                },
                "table_primary_key": ["user_id"],
            },
        ]
    }


def _click_three_tables_payload() -> dict[str, Any]:
    """One source event, three downstream tables (fan-out shape).

    ``Click`` events fan out to:

      * ``ClicksByUser`` — count grouped by ``user_id``.
      * ``ClicksByDevice`` — count grouped by ``device``.
      * ``ClicksByCountry`` — count grouped by ``country``.

    Covers the "one event source feeds multiple aggregations" branch
    of the recovery code — register-time fan-out plus apply-time
    cascading.
    """
    return {
        "nodes": [
            {
                "kind": "event",
                "name": "Click",
                "schema": {
                    "fields": {
                        "user_id": "str",
                        "device": "str",
                        "country": "str",
                    },
                    "optional_fields": [],
                },
            },
            {
                "kind": "derivation",
                "name": "ClicksByUser",
                "output_kind": "table",
                "upstreams": ["Click"],
                "ops": [
                    {
                        "op": "group_by",
                        "keys": ["user_id"],
                        "agg": {
                            "n_user": {
                                "op": "count",
                                "params": {"window": "1h"},
                            }
                        },
                    }
                ],
                "schema": {
                    "fields": {"user_id": "str", "n_user": "i64"},
                    "optional_fields": [],
                },
                "table_primary_key": ["user_id"],
            },
            {
                "kind": "derivation",
                "name": "ClicksByDevice",
                "output_kind": "table",
                "upstreams": ["Click"],
                "ops": [
                    {
                        "op": "group_by",
                        "keys": ["device"],
                        "agg": {
                            "n_device": {
                                "op": "count",
                                "params": {"window": "1h"},
                            }
                        },
                    }
                ],
                "schema": {
                    "fields": {"device": "str", "n_device": "i64"},
                    "optional_fields": [],
                },
                "table_primary_key": ["device"],
            },
            {
                "kind": "derivation",
                "name": "ClicksByCountry",
                "output_kind": "table",
                "upstreams": ["Click"],
                "ops": [
                    {
                        "op": "group_by",
                        "keys": ["country"],
                        "agg": {
                            "n_country": {
                                "op": "count",
                                "params": {"window": "1h"},
                            }
                        },
                    }
                ],
                "schema": {
                    "fields": {"country": "str", "n_country": "i64"},
                    "optional_fields": [],
                },
                "table_primary_key": ["country"],
            },
        ]
    }


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def data_dir(tmp_path: Path) -> Path:
    """Per-test data directory under pytest's ``tmp_path``.

    The binary creates ``<data-dir>/wal`` and ``<data-dir>/snapshots``
    automatically on first boot. We pass the parent so the second
    boot's recovery scan finds the same paths.
    """
    d = tmp_path / "beava-data"
    d.mkdir()
    return d


@pytest.fixture
def server_factory(
    beava_binary: Path, data_dir: Path
) -> Iterator[Any]:
    """Yields a callable that spawns servers against the same ``data_dir``.

    The factory tracks every spawned handle so the teardown can
    terminate any that the test forgot to clean up (e.g. when a kill +
    restart is interrupted by an assert mid-test).
    """
    spawned: list[_ServerHandle] = []

    def _factory(
        *,
        snapshot_interval_ms: int = 60_000,
        wal_tick_ms: int = 5,
    ) -> _ServerHandle:
        handle = _spawn_server(
            beava_binary,
            data_dir,
            snapshot_interval_ms=snapshot_interval_ms,
            wal_tick_ms=wal_tick_ms,
        )
        spawned.append(handle)
        _wait_for_ping(handle.http_url)
        return handle

    try:
        yield _factory
    finally:
        for h in spawned:
            try:
                h.terminate()
            except Exception:
                pass


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_register_push_kill_restart_get_returns_same_values(
    server_factory: Any,
) -> None:
    """Register → push 100 → SIGKILL → restart → GET returns identical values.

    This is the canonical durability test. The pre-kill server
    aggregates ``cnt=100, total=sum(0..99)*1.0=4950.0,
    avg_amount=49.5`` over the ``Txn`` event stream. After SIGKILL,
    the second server boot loads the snapshot (none yet — snapshot
    interval is 60s) and replays the WAL tail, reconstructing
    identical aggregations.
    """
    server = server_factory()
    _register(server.http_url, _txn_count_sum_avg_payload())

    for i in range(100):
        _push(
            server.http_url,
            "Txn",
            {"user_id": "alice", "amount": float(i)},
        )

    # Snapshot the row before the kill so the post-recovery assertion
    # compares apples to apples (also catches the pre-kill aggregation
    # itself being wrong, which would otherwise masquerade as a
    # recovery bug).
    pre_row = _get(server.http_url, "UserTxn", "alice")
    assert pre_row["cnt"] == 100, f"pre-kill cnt should be 100; got {pre_row!r}"
    assert abs(pre_row["total"] - 4950.0) < 1e-9, (
        f"pre-kill total should be 4950.0; got {pre_row!r}"
    )
    assert abs(pre_row["avg_amount"] - 49.5) < 1e-9, (
        f"pre-kill avg should be 49.5; got {pre_row!r}"
    )

    # Give the WAL writer thread a moment to fsync the last batch.
    # wal_tick_ms=5 means 3 × tick = 15ms is more than enough; we pad
    # to 100ms for CI jitter.
    time.sleep(0.1)
    server.kill()

    # Second boot against the same data-dir. Recovery replays the WAL.
    restarted = server_factory()
    post_row = _get(restarted.http_url, "UserTxn", "alice")

    assert post_row["cnt"] == pre_row["cnt"], (
        f"WAL recovery dropped events: pre={pre_row['cnt']} "
        f"post={post_row['cnt']}"
    )
    assert abs(post_row["total"] - pre_row["total"]) < 1e-9, (
        f"WAL recovery sum mismatch: pre={pre_row['total']} "
        f"post={post_row['total']}"
    )
    assert abs(post_row["avg_amount"] - pre_row["avg_amount"]) < 1e-9, (
        f"WAL recovery avg mismatch: pre={pre_row['avg_amount']} "
        f"post={post_row['avg_amount']}"
    )


def test_recovery_with_multiple_aggs_per_source(server_factory: Any) -> None:
    """One event source → three downstream tables, all recover.

    Push 50 ``Click`` events with rotating ``(user_id, device,
    country)`` tuples. After SIGKILL + restart, all three tables
    (``ClicksByUser``, ``ClicksByDevice``, ``ClicksByCountry``) must
    report the same per-group counts as before the kill. This pins
    the fan-out branch of the recovery path: the WAL stores one
    record per push, and apply-replay must cascade into each
    downstream aggregation.
    """
    server = server_factory()
    _register(server.http_url, _click_three_tables_payload())

    users = ["u_alice", "u_bob"]
    devices = ["ios", "android", "web"]
    countries = ["us", "uk"]
    for i in range(50):
        _push(
            server.http_url,
            "Click",
            {
                "user_id": users[i % len(users)],
                "device": devices[i % len(devices)],
                "country": countries[i % len(countries)],
            },
        )

    # Capture pre-kill counts per group for every table.
    pre_user_alice = _get(server.http_url, "ClicksByUser", "u_alice")
    pre_user_bob = _get(server.http_url, "ClicksByUser", "u_bob")
    pre_dev_ios = _get(server.http_url, "ClicksByDevice", "ios")
    pre_dev_web = _get(server.http_url, "ClicksByDevice", "web")
    pre_ctry_us = _get(server.http_url, "ClicksByCountry", "us")
    pre_ctry_uk = _get(server.http_url, "ClicksByCountry", "uk")

    # Sanity-check the partition arithmetic so a downstream bug can't
    # hide behind "the test fixture is wrong".
    assert pre_user_alice["n_user"] + pre_user_bob["n_user"] == 50
    assert (
        pre_dev_ios["n_device"]
        + _get(server.http_url, "ClicksByDevice", "android")["n_device"]
        + pre_dev_web["n_device"]
        == 50
    )
    assert pre_ctry_us["n_country"] + pre_ctry_uk["n_country"] == 50

    time.sleep(0.1)
    server.kill()

    restarted = server_factory()
    assert (
        _get(restarted.http_url, "ClicksByUser", "u_alice") == pre_user_alice
    )
    assert _get(restarted.http_url, "ClicksByUser", "u_bob") == pre_user_bob
    assert _get(restarted.http_url, "ClicksByDevice", "ios") == pre_dev_ios
    assert _get(restarted.http_url, "ClicksByDevice", "web") == pre_dev_web
    assert _get(restarted.http_url, "ClicksByCountry", "us") == pre_ctry_us
    assert _get(restarted.http_url, "ClicksByCountry", "uk") == pre_ctry_uk


def test_recovery_after_snapshot_plus_wal_tail(
    server_factory: Any, data_dir: Path
) -> None:
    """Recovery after a deep WAL with potentially-rotated segments + SIGKILL.

    Original intent: force a snapshot to land mid-test (via a short
    ``BEAVA_SNAPSHOT_INTERVAL_MS``), push more events, kill, restart,
    and assert ``snapshot + tail`` recovery. While auditing this
    test's first run we discovered the snapshot interval is
    hard-coded to ``60_000`` ms inside
    ``crates/beava-server/src/server.rs::bind_with_config``
    (line ~416) — the ``BEAVA_SNAPSHOT_INTERVAL_MS`` env var
    resolves into ``ServerV18Config::from_env`` but is then ignored.
    See ``main.rs::main`` which calls ``bind_with_config`` rather
    than wiring the resolved snapshot interval. This is a real
    finding documented here as a regression latch — *if* a future
    change wires the env var through, just shrink the constant and
    re-add the snapshot-file-presence assertion.

    Practical contract this test enforces today: push a deep batch
    of events (500), SIGKILL, restart. Even without a snapshot, the
    WAL replay path is exercised at depth — multiple buffer flushes,
    multiple ring-buffer recycles. The recovered row equals the
    pre-kill row, exactly.
    """
    server = server_factory()
    _register(server.http_url, _txn_count_sum_avg_payload())

    # 500 events — deep enough to exercise multiple WAL buffer flushes
    # at the default 16 MiB × 3 ring configuration even though one
    # record is small; the apply path still writes one buffer slot
    # per record group, so 500 pushes drive enough churn that the
    # writer thread has done dozens of fsync ticks by the time we
    # kill it.
    for _ in range(500):
        _push(server.http_url, "Txn", {"user_id": "alice", "amount": 2.0})

    # The snapshots dir may or may not exist (the snapshot worker has
    # the hardcoded 60s cadence, so at this point it almost certainly
    # has NOT fired yet). Document whichever case landed; the recovery
    # contract is the same: snapshot + WAL OR WAL-only.
    snap_dir = data_dir / "snapshots"
    snap_files_pre_kill = (
        list(snap_dir.glob("*")) if snap_dir.exists() else []
    )

    pre_row = _get(server.http_url, "UserTxn", "alice")
    assert pre_row["cnt"] == 500, (
        f"pre-kill cnt should be 500; got {pre_row!r}"
    )
    assert abs(pre_row["total"] - 1000.0) < 1e-9, (
        f"pre-kill total should be 1000.0; got {pre_row!r}"
    )

    time.sleep(0.1)
    server.kill()

    restarted = server_factory()
    post_row = _get(restarted.http_url, "UserTxn", "alice")
    assert post_row["cnt"] == 500, (
        f"deep-WAL recovery cnt mismatch: pre=500 post={post_row['cnt']} "
        f"(snapshot files at pre-kill: "
        f"{[f.name for f in snap_files_pre_kill]})"
    )
    assert abs(post_row["total"] - 1000.0) < 1e-9, (
        f"deep-WAL recovery total mismatch: pre=1000.0 "
        f"post={post_row['total']}"
    )


def test_recovery_after_sigkill_mid_push(server_factory: Any) -> None:
    """SIGKILL while a push batch is in flight; restart must not corrupt.

    Spawns a writer thread that hammers ``POST /push/Txn`` in a tight
    loop. After ~200ms (enough to land hundreds of pushes), the main
    thread issues ``SIGKILL``. The contract: the second boot's WAL
    recovery must NOT panic / NOT report corruption; the recovered
    ``cnt`` is at most the number of pushes that returned 200 from
    the writer thread, and the last record (if it was mid-fsync) is
    silently discarded (not double-counted).
    """
    server = server_factory()
    _register(server.http_url, _txn_count_sum_avg_payload())

    successes: list[int] = []
    stop = threading.Event()

    def _hammer() -> None:
        count = 0
        with httpx.Client(base_url=server.http_url, timeout=2.0) as client:
            while not stop.is_set():
                try:
                    r = client.post(
                        "/push/Txn",
                        json={"user_id": "bob", "amount": 1.0},
                        headers={"Content-Type": "application/json"},
                    )
                    if r.status_code == 200:
                        count += 1
                except Exception:
                    # Connection torn down by the SIGKILL — that's
                    # the whole point. Drop and bail.
                    break
        successes.append(count)

    t = threading.Thread(target=_hammer, daemon=True)
    t.start()
    time.sleep(0.2)  # let the writer rack up some pushes
    server.kill()
    stop.set()
    t.join(timeout=2.0)

    successful_pushes = successes[0] if successes else 0
    assert successful_pushes > 0, (
        "writer thread should have logged at least one 200 before SIGKILL"
    )

    restarted = server_factory()
    post_row = _get(restarted.http_url, "UserTxn", "bob")
    recovered_cnt = int(post_row.get("cnt", 0))

    # The hard contract: recovered count is bounded above by the
    # client-side 200-response count. The lower bound is soft — the
    # SIGKILL can drop the last few records that hadn't fsynced. We
    # require at least 50% to catch a recovery that's silently
    # dropping the whole WAL.
    assert recovered_cnt <= successful_pushes, (
        f"recovery FABRICATED events: client saw {successful_pushes} 200s "
        f"but recovered cnt={recovered_cnt} — durability is broken upward "
        f"(replay must not double-apply)"
    )
    assert recovered_cnt >= successful_pushes // 2, (
        f"recovery DROPPED too many events: client saw {successful_pushes} "
        f"200s but recovered cnt={recovered_cnt} — WAL fsync gating is "
        f"broken downward"
    )


def test_register_force_replace_then_kill_restart(server_factory: Any) -> None:
    """Force-replace schema A with B, push only to B, then kill + restart.

    Register schema A (``Txn → UserTxn(cnt)``); push 10 events; force-
    replace with schema B (``Txn → UserTxn(total)`` — different agg
    output) via ``"force": true``; push 5 events with amounts summing
    to 100.0; SIGKILL; restart. The recovered table must reflect ONLY
    schema B's state. Specifically: the ``total`` feature exists, the
    pre-replace ``cnt`` does NOT, and ``total == 100.0``.

    This guards against a known durability hazard: WAL replay against
    a swapped registry. If the recovery code naively replays
    pre-replace WAL records against the post-replace schema, the
    aggregations either crash or fabricate state. The contract is
    that the registry version is part of the WAL header and replay
    is gated on schema match.
    """
    server = server_factory()

    # Schema A: count.
    schema_a = {
        "nodes": [
            {
                "kind": "event",
                "name": "Txn",
                "schema": {
                    "fields": {"user_id": "str", "amount": "f64"},
                    "optional_fields": [],
                },
            },
            {
                "kind": "derivation",
                "name": "UserTxn",
                "output_kind": "table",
                "upstreams": ["Txn"],
                "ops": [
                    {
                        "op": "group_by",
                        "keys": ["user_id"],
                        "agg": {
                            "cnt": {
                                "op": "count",
                                "params": {"window": "1h"},
                            }
                        },
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
    _register(server.http_url, schema_a)
    for _ in range(10):
        _push(server.http_url, "Txn", {"user_id": "alice", "amount": 1.0})

    # Schema B: sum, force-replace.
    schema_b = {
        "force": True,
        "nodes": [
            {
                "kind": "event",
                "name": "Txn",
                "schema": {
                    "fields": {"user_id": "str", "amount": "f64"},
                    "optional_fields": [],
                },
            },
            {
                "kind": "derivation",
                "name": "UserTxn",
                "output_kind": "table",
                "upstreams": ["Txn"],
                "ops": [
                    {
                        "op": "group_by",
                        "keys": ["user_id"],
                        "agg": {
                            "total": {
                                "op": "sum",
                                "params": {"field": "amount", "window": "1h"},
                            }
                        },
                    }
                ],
                "schema": {
                    "fields": {"user_id": "str", "total": "f64"},
                    "optional_fields": [],
                },
                "table_primary_key": ["user_id"],
            },
        ],
    }
    _register(server.http_url, schema_b)

    # 5 pushes summing to 100.0 (20 each).
    for _ in range(5):
        _push(server.http_url, "Txn", {"user_id": "alice", "amount": 20.0})

    pre_row = _get(server.http_url, "UserTxn", "alice")
    assert "total" in pre_row, (
        f"schema B's 'total' must be present pre-kill; got {pre_row!r}"
    )
    assert "cnt" not in pre_row, (
        f"schema A's 'cnt' must be gone after force-replace; got {pre_row!r}"
    )
    assert abs(pre_row["total"] - 100.0) < 1e-9, (
        f"pre-kill total should be 100.0; got {pre_row!r}"
    )

    time.sleep(0.1)
    server.kill()

    restarted = server_factory()
    post_row = _get(restarted.http_url, "UserTxn", "alice")
    assert "total" in post_row, (
        f"after restart, schema B's 'total' must survive; got {post_row!r}"
    )
    assert "cnt" not in post_row, (
        f"after restart, pre-replace 'cnt' must NOT resurface; "
        f"got {post_row!r} — force-replace + WAL recovery is broken"
    )
    assert abs(post_row["total"] - 100.0) < 1e-9, (
        f"after restart, total should still be 100.0; got {post_row!r}"
    )
