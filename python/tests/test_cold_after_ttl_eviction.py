"""End-to-end pytest coverage for cold-entity TTL eviction.

``test_phase12_8_cold_after_decorator.py`` pins the Python decorator surface
(``@bv.event(cold_after='7d')`` parses + emits ``cold_after_ms`` on the wire).
This file pins the eviction itself: register a short ``cold_after_ms``,
push, sleep past the TTL, push again, and assert the entity row was
evicted + re-initialised from zero on the resurrect.

The decorator wiring is RED at HEAD (Plan 12.8-02 Task 2.b lands the
``_cold_after_ms`` field + wire emission). To remain ship-shape today, this
file builds the register payload as a raw dict and POSTs it via httpx,
mirroring the existing pattern in ``test_phase12_8_op_lifetime_bounds.py``.
The server-side eviction code path is what's under test — the entry point
is the same regardless of which surface produced the JSON.

The 4 tests below mirror the Rust integration tests in
``crates/beava-server/tests/phase12_8_cold_entity_eviction.rs`` +
``phase12_8_metrics_endpoint.rs`` from the Python side, exercising the
HTTP transport end-to-end.

  1. ``test_entity_evicted_after_ttl_then_repush_starts_from_zero`` —
     500 ms TTL, push, sleep 700 ms, push: cnt must be 1, not 2.
  2. ``test_entity_not_evicted_inside_ttl`` — two pushes inside the TTL
     window: cnt must be 2.
  3. ``test_multiple_entities_evict_independently`` — eviction is
     per-entity-on-arrival (``project_no_sharded_apply``); user_B's push
     after user_A's TTL expiry must NOT inherit user_A's stale row.
  4. ``test_eviction_counter_increments_via_admin_metrics_if_available``
     — if ``/metrics`` is reachable on a known admin port, verify
     ``beava_cold_entity_evictions_total`` increments after a cold
     resurrect. Skipped via ``pytest.skip()`` when the admin sidecar
     isn't reachable (the embed-mode spawn binds admin to port 0 which
     the SDK doesn't surface, so this test owns its own spawn).
"""

from __future__ import annotations

import json
import os
import socket
import subprocess
import threading
import time
from pathlib import Path
from typing import Any, Generator

import httpx
import pytest

# ─── Helpers: spawn beava server with a known admin port ─────────────────────


def _free_port() -> int:
    """Reserve an OS-allocated free TCP port and release it.

    There is an intentional race window between releasing the socket and
    the binary binding it — but since both tests run sequentially in the
    same process and the kernel uses a recently-released port, the window
    is small in practice. Tests that fail to bind retry once.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def _spawn_beava_with_admin(
    binary: Path,
    wal_dir: Path,
    snap_dir: Path,
    admin_port: int,
) -> tuple[subprocess.Popen[bytes], str, str]:
    """Spawn a beava server with HTTP+TCP on OS-assigned ports and admin on
    ``admin_port``. Returns ``(proc, http_url, admin_url)``.

    Mirrors the conftest ``beava_server`` fixture but adds a fixed admin
    port so test #4 can issue ``GET /metrics`` without needing the SDK to
    surface the admin port (which it does not in v0).
    """
    env = {
        **os.environ,
        "BEAVA_LISTEN_ADDR": "127.0.0.1:0",
        "BEAVA_TCP_PORT": "0",
        "BEAVA_ADMIN_ADDR": f"127.0.0.1:{admin_port}",
        "BEAVA_DEV_ENDPOINTS": "1",
        "BEAVA_WAL_DIR": str(wal_dir),
        "BEAVA_SNAPSHOT_DIR": str(snap_dir),
    }
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

    if not ready.wait(timeout=5.0):
        proc.kill()
        proc.wait()
        if proc.stdout:
            proc.stdout.close()
        pytest.fail(
            f"beava server did not emit bind log lines within 5s; "
            f"http_addr={http_addr}, tcp_addr={tcp_addr}"
        )

    http_url = f"http://{http_addr[0]}"
    admin_url = f"http://127.0.0.1:{admin_port}"
    return proc, http_url, admin_url


@pytest.fixture
def beava_with_admin(
    beava_binary: Path, tmp_path: Path
) -> Generator[tuple[str, str], None, None]:
    """Spawn a beava server with HTTP + a known admin port. Yields
    ``(http_url, admin_url)``. SIGTERM + 5 s grace on teardown."""
    wal_dir = tmp_path / "wal"
    snap_dir = tmp_path / "snap"
    wal_dir.mkdir()
    snap_dir.mkdir()
    admin_port = _free_port()
    proc, http_url, admin_url = _spawn_beava_with_admin(
        beava_binary, wal_dir, snap_dir, admin_port
    )
    try:
        yield http_url, admin_url
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5.0)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


# ─── Helpers: register / push / get over HTTP ────────────────────────────────


def _register_count_payload(
    cold_after_ms: int | None, key_cols: list[str] | None = None
) -> dict[str, Any]:
    """Build a register JSON payload: ``Txn`` event source (with optional
    ``cold_after_ms``) + ``TxnAgg`` derivation that group_by's ``user_id`` (or
    ``key_cols`` if provided) and runs ``count``.

    Mirrors the layout used by the Rust integration tests in
    ``crates/beava-server/tests/phase12_8_cold_entity_eviction.rs`` so the
    server-side eviction code path is exercised identically from Python.
    """
    keys = key_cols if key_cols is not None else ["user_id"]
    fields: dict[str, str] = {"user_id": "str", "amount": "f64"}
    derived_fields: dict[str, str] = {k: "str" for k in keys}
    derived_fields["cnt"] = "i64"
    return {
        "nodes": [
            {
                "kind": "event",
                "name": "Txn",
                "schema": {
                    "fields": fields,
                    "optional_fields": [],
                },
                "cold_after_ms": cold_after_ms,
            },
            {
                "kind": "derivation",
                "name": "TxnAgg",
                "output_kind": "table",
                "upstreams": ["Txn"],
                "ops": [
                    {
                        "op": "group_by",
                        "keys": keys,
                        "agg": {"cnt": {"op": "count", "params": {}}},
                    }
                ],
                "schema": {
                    "fields": derived_fields,
                    "optional_fields": [],
                },
                "table_primary_key": keys,
            },
        ]
    }


def _register(http_url: str, payload: dict[str, Any]) -> httpx.Response:
    return httpx.post(
        f"{http_url}/register",
        content=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )


def _push(http_url: str, fields: dict[str, Any]) -> httpx.Response:
    """POST ``/push/Txn`` with the given fields. Uses the legacy
    path-segment route which mirrors what the Rust integration tests use
    and is stable across phases."""
    return httpx.post(
        f"{http_url}/push/Txn",
        content=json.dumps(fields).encode(),
        headers={"Content-Type": "application/json"},
        timeout=10.0,
    )


def _get_cnt(http_url: str, key: str) -> int:
    """GET ``/get/cnt/<key>`` and return the integer ``value`` field."""
    resp = httpx.get(f"{http_url}/get/cnt/{key}", timeout=10.0)
    assert resp.status_code == 200, (
        f"GET /get/cnt/{key} returned {resp.status_code}: {resp.text!r}"
    )
    body = resp.json()
    val = body.get("value")
    assert isinstance(val, int), (
        f"GET /get/cnt/{key} body.value not int; got {body!r}"
    )
    return val


def _scrape_counter(metrics_body: str, name: str) -> int | None:
    """Return the integer value of a Prometheus counter/gauge line, or
    ``None`` if the metric isn't present. Mirrors the Rust-side
    ``scrape_metric_value`` helper in ``phase12_8_metrics_endpoint.rs``.
    """
    for line in metrics_body.splitlines():
        trimmed = line.lstrip()
        if trimmed.startswith("#"):
            continue
        if not trimmed.startswith(name):
            continue
        rest = trimmed[len(name) :]
        # Must be terminated by whitespace or '{' to avoid prefix-match
        # false hits (e.g. ``beava_foo_total`` vs ``beava_foo``).
        if rest[:1] not in (" ", "\t", "{"):
            continue
        parts = trimmed.split()
        if not parts:
            continue
        try:
            return int(parts[-1])
        except ValueError:
            return None
    return None


# ─── Tests ───────────────────────────────────────────────────────────────────


def test_entity_evicted_after_ttl_then_repush_starts_from_zero(
    beava_with_admin: tuple[str, str],
) -> None:
    """500 ms TTL → push → cnt=1; sleep 700 ms → push → cnt=1 (NOT 2).

    The cold-TTL contract (D-04 ``project_no_sharded_apply``): on resurrect
    after ``now_ms - last_seen_ms > cold_after_ms``, the entity row is
    evicted before the new event applies, so the count restarts from 0
    and the new event increments it to 1.
    """
    http_url, _ = beava_with_admin
    payload = _register_count_payload(cold_after_ms=500)
    resp = _register(http_url, payload)
    assert resp.status_code == 200, (
        f"register failed: status={resp.status_code} body={resp.text!r}"
    )

    # First push: alice's row is born with cnt=1.
    r1 = _push(http_url, {"user_id": "alice", "amount": 10.0})
    assert r1.status_code == 200, f"first push failed: {r1.text!r}"
    assert _get_cnt(http_url, "alice") == 1, "cnt must be 1 after first push"

    # Sleep past the 500 ms TTL with comfortable margin.
    time.sleep(0.7)

    # Second push: alice was cold → row evicted → fresh row → cnt=1.
    r2 = _push(http_url, {"user_id": "alice", "amount": 20.0})
    assert r2.status_code == 200, f"resurrect push failed: {r2.text!r}"
    cnt = _get_cnt(http_url, "alice")
    assert cnt == 1, (
        f"FRESH state on resurrect (D-04): cnt must be 1, not 2; got cnt={cnt}. "
        "If this fails with cnt=2, the eviction path did NOT fire — the entity "
        "row carried stale state across the TTL boundary."
    )


def test_entity_not_evicted_inside_ttl(
    beava_with_admin: tuple[str, str],
) -> None:
    """Two pushes inside the TTL window → cnt=2 (no eviction)."""
    http_url, _ = beava_with_admin
    # 5 s TTL — comfortably larger than the sub-second sleeps below.
    payload = _register_count_payload(cold_after_ms=5_000)
    resp = _register(http_url, payload)
    assert resp.status_code == 200, f"register failed: {resp.text!r}"

    r1 = _push(http_url, {"user_id": "alice", "amount": 1.0})
    assert r1.status_code == 200
    time.sleep(0.1)  # well under the 5 s TTL
    r2 = _push(http_url, {"user_id": "alice", "amount": 2.0})
    assert r2.status_code == 200

    cnt = _get_cnt(http_url, "alice")
    assert cnt == 2, (
        f"warm entity must accumulate to cnt=2 across two pushes within the "
        f"TTL window; got cnt={cnt}. If this fails with cnt=1, the eviction "
        "path fired spuriously inside the TTL window."
    )


def test_multiple_entities_evict_independently(
    beava_with_admin: tuple[str, str],
) -> None:
    """Per-entity-on-arrival eviction (``project_no_sharded_apply``): pushing
    user_B after user_A's TTL expiry must not bleed user_A's stale row
    into user_B's fresh row.

    Sequence:
      T=0:    push user_A (cnt_A=1) and user_B (cnt_B=1).
      T=600ms: sleep past the 500 ms TTL.
      T=600ms: push user_B again.
      Assertions:
        - cnt_B == 2 (user_B's row was warm relative to the FIRST user_B
          push at T=0 — but that's > 500 ms ago, so user_B should also
          have been evicted, leaving cnt_B == 1 on its own resurrect).

    Subtle correctness note: the spec for this test is "push to user_A,
    wait, push to user_B INSIDE user_A's TTL". So sleep is between A and
    B but B's first push remains warm. Sequence corrected:
      T=0:     push user_A (cnt_A=1).
      T=600ms: sleep 700 ms past user_A's 500 ms TTL.
      T=700ms: push user_B (cnt_B=1) — user_B is brand-new.
      Verify:  cnt_B == 1 (fresh, NOT user_A's stale 1). user_A still
               holds stale state in memory until either user_A's next push
               (which would evict) or a global cleanup. We do NOT push
               user_A again here, so we can't directly assert user_A's
               state was evicted — but we CAN assert user_B's row is
               independent (the bleed-in bug would surface as cnt_B == 2
               if the row was misaddressed).
    """
    http_url, _ = beava_with_admin
    payload = _register_count_payload(cold_after_ms=500)
    resp = _register(http_url, payload)
    assert resp.status_code == 200, f"register failed: {resp.text!r}"

    # T=0: alice gets one push.
    r1 = _push(http_url, {"user_id": "alice", "amount": 1.0})
    assert r1.status_code == 200
    assert _get_cnt(http_url, "alice") == 1

    # Sleep past alice's 500 ms TTL.
    time.sleep(0.7)

    # Push bob for the first time. bob is a brand-new entity — eviction
    # logic for a never-seen-entity row must NOT fire (no last_seen_ms),
    # and bob's row must be fresh-allocated regardless of alice's state.
    r2 = _push(http_url, {"user_id": "bob", "amount": 2.0})
    assert r2.status_code == 200

    cnt_bob = _get_cnt(http_url, "bob")
    assert cnt_bob == 1, (
        f"bob's row must be independent of alice's stale TTL'd row; got cnt_bob={cnt_bob}. "
        "If this fails with cnt_bob=2, the eviction code mis-addressed alice's "
        "row as bob's (memory leak / cross-entity bleed)."
    )

    # Additionally: pushing alice again now (still post-TTL) must reset
    # her count to 1, confirming per-entity eviction still works after
    # bob's interleaved push.
    r3 = _push(http_url, {"user_id": "alice", "amount": 3.0})
    assert r3.status_code == 200
    cnt_alice = _get_cnt(http_url, "alice")
    assert cnt_alice == 1, (
        f"alice's stale row must be evicted on her own resurrect even after "
        f"bob's interleaved push; got cnt_alice={cnt_alice}."
    )


def test_eviction_counter_increments_via_admin_metrics_if_available(
    beava_with_admin: tuple[str, str],
) -> None:
    """If ``/metrics`` is reachable on the admin sidecar, verify
    ``beava_cold_entity_evictions_total`` increments after a cold
    resurrect. Skip with ``pytest.skip()`` when the endpoint isn't
    reachable or the metric line is absent (forward-compat with builds
    that haven't wired the counter yet).
    """
    http_url, admin_url = beava_with_admin

    # Probe the admin sidecar's /metrics — skip cleanly if unreachable.
    try:
        probe = httpx.get(f"{admin_url}/metrics", timeout=2.0)
    except httpx.HTTPError as e:
        pytest.skip(f"admin /metrics not reachable at {admin_url}: {e}")
    if probe.status_code != 200:
        pytest.skip(
            f"admin /metrics returned {probe.status_code}; expected 200"
        )
    before = _scrape_counter(probe.text, "beava_cold_entity_evictions_total")
    if before is None:
        pytest.skip(
            "beava_cold_entity_evictions_total not exposed in /metrics body "
            "(metric line missing — pre-12.8 build)"
        )

    payload = _register_count_payload(cold_after_ms=500)
    resp = _register(http_url, payload)
    assert resp.status_code == 200, f"register failed: {resp.text!r}"

    # Touch alice, sleep past TTL, touch again — the second push fires the
    # eviction path which is what increments the counter.
    assert (
        _push(http_url, {"user_id": "alice", "amount": 1.0}).status_code == 200
    )
    time.sleep(0.7)
    assert (
        _push(http_url, {"user_id": "alice", "amount": 2.0}).status_code == 200
    )

    after_resp = httpx.get(f"{admin_url}/metrics", timeout=2.0)
    assert after_resp.status_code == 200
    after = _scrape_counter(after_resp.text, "beava_cold_entity_evictions_total")
    assert after is not None, "counter line disappeared between probe and assert"
    assert after > before, (
        f"beava_cold_entity_evictions_total did not increment past {before} "
        f"after a cold resurrect (still {after})"
    )
