"""Phase 13.5 Plan 02: bv.App lifecycle tests.

Validates the 7 wire-mapped methods + context manager invariants per
docs/sdk-api/python.md § App class.

Phase 13.5.1 Plan 05 D-05 (USER-LOCKED): 6 of 10 tests now exercise the
real ``app`` fixture (``bv.App(test_mode=True)`` against a spawned
embed-mode subprocess) — replacing the prior MagicMock-against-Transport
anti-pattern that masked the 0/68 v0 acceptance-test deficit at Phase
13.5 Plan 11 close. The 4 remaining tests use MagicMock for pure
construction / url-mode / context-manager guard scenarios where there
are no transport calls (CONTEXT D-05 permitted MagicMock uses).
"""
from __future__ import annotations

from typing import Any, Generator
from unittest.mock import MagicMock, patch

import pytest

import beava as bv

# ─── Real-engine fixture (Phase 13.5.1 D-05 USER-LOCKED replacement) ─────────


@pytest.fixture
def app() -> Generator[Any, None, None]:
    """Real-engine fixture per Phase 13.5.1 D-05 (no MagicMock against Transport).

    Spawns a fresh embed-mode subprocess per test with test_mode=True so
    app.reset() is callable if needed. Subprocess teardown handled by
    bv.App's __exit__.
    """
    with bv.App(test_mode=True) as instance:
        yield instance


# ─── Pure-construction tests (no transport calls — KEEP AS-IS) ───────────────


def test_app_construct_no_url_uses_embed_mode() -> None:
    app = bv.App()
    assert app._transport_kind == "embed"


def test_app_construct_http_url() -> None:
    app = bv.App(url="http://localhost:7777")
    assert app._transport_kind == "http"


def test_app_construct_tcp_url() -> None:
    app = bv.App(url="tcp://localhost:7778")
    assert app._transport_kind == "tcp"


# ─── Real-engine tests (Plan 13.5.1 D-05 — replaces 6 MagicMock sites) ───────


def test_register_calls_transport_with_force_dry_run_kwargs(app: Any) -> None:
    """Plan 13.5.1 D-05: real engine, real wire payload (replaces MagicMock).

    Registers an event source with force=True, dry_run=True; asserts the
    server returns the categorized-diff payload and ``would_apply=False``
    (dry-run was honored — nothing was committed).
    """
    @bv.event
    class Txn:
        user_id: str
        amount: float

    result = app.register(Txn, force=True, dry_run=True)
    # Phase 13.4 Plan 06 D-01: dry_run returns the categorized-diff payload
    # ({"diff": {"additive": [...], "destructive": [...]}, "would_apply": false}).
    # The pre-13.4 shape ({"status": "ok", "registry_version": N}) is the
    # commit-path response — dry_run never commits, so it never returns it.
    assert result.get("would_apply") is False, f"dry_run must not commit; got {result!r}"
    assert "diff" in result, f"dry_run must return diff payload; got {result!r}"


def test_push_signature_event_name_and_fields(app: Any) -> None:
    """Plan 13.5.1 D-05: real engine push (replaces MagicMock).

    Registers an event + a count aggregation, pushes one event, asserts
    the response carries an integer ack_lsn.
    """
    @bv.event
    class Txn:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserTxn(txn: Txn):
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    app.register(Txn, UserTxn)
    result = app.push("Txn", {"user_id": "alice", "amount": 1.0})
    assert isinstance(result.get("ack_lsn"), int)


def test_get_returns_row_shape_dict(app: Any) -> None:
    """Plan 13.5.1 D-05: real engine get (replaces MagicMock).

    Push 3 events for one entity, query the row, expect {"c": 3}.
    """
    @bv.event
    class Txn:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserTxn(txn: Txn):
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    app.register(Txn, UserTxn)
    for _ in range(3):
        app.push("Txn", {"user_id": "alice", "amount": 1.0})

    r = app.get("UserTxn", "alice")
    assert r == {"c": 3}


def test_batch_get_returns_list_in_request_order(app: Any) -> None:
    """Plan 13.5.1 D-05: real engine batch_get (replaces MagicMock).

    Push events for two entities, batch-get both + a cold-start key,
    expect 3 results in order.
    """
    @bv.event
    class Txn:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserTxn(txn: Txn):
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    app.register(Txn, UserTxn)
    for user, n in (("alice", 2), ("bob", 5)):
        for _ in range(n):
            app.push("Txn", {"user_id": user, "amount": 1.0})

    r = app.batch_get(
        [("UserTxn", "alice"), ("UserTxn", "bob"), ("UserTxn", "cold")]
    )
    assert len(r) == 3
    assert r[0] == {"c": 2}
    assert r[1] == {"c": 5}
    # cold-start: server returns either {} or a missing-row marker
    assert r[2] in ({}, {"c": 0}) or r[2] is None or "c" not in r[2]


def test_reset_calls_transport(app: Any) -> None:
    """Plan 13.5.1 D-05: real engine reset (replaces MagicMock).

    Register + push + get, then reset, then assert post-reset state is
    cold-start-equivalent OR the table no longer exists (registry wiped).
    """
    from beava._errors import RegistrationError  # noqa: PLC0415

    @bv.event
    class Txn:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserTxn(txn: Txn):
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    app.register(Txn, UserTxn)
    app.push("Txn", {"user_id": "alice", "amount": 1.0})
    assert app.get("UserTxn", "alice") == {"c": 1}

    app.reset()

    # Post-reset: state wiped (and on the v0 default path, registry too —
    # see test_transport_equivalence::test_reset_equivalent for the
    # USER-LOCKED behavior).
    try:
        post = app.get("UserTxn", "alice")
        assert post in ({}, {"c": 0}) or post is None
    except RegistrationError as exc:
        assert exc.code == "unknown_table"


def test_ping_returns_server_version_and_registry_version(app: Any) -> None:
    """Plan 13.5.1 D-05: real engine ping (replaces MagicMock).

    Pings a freshly-spawned server; expects server_version + registry_version
    in the response.
    """
    r = app.ping()
    assert "server_version" in r
    assert "registry_version" in r


# ─── Url-mode / context-manager guards (no transport calls — KEEP AS-IS) ─────


def test_close_is_idempotent() -> None:
    """url-mode close is idempotent — uses MagicMock for the transport
    factory (CONTEXT D-05 permitted: no transport methods are called)."""
    with patch("beava._app.make_transport") as mk:
        mk.return_value = MagicMock()
        a = bv.App(url="http://localhost:7777")
        a.close()
        a.close()  # second call must not raise


def test_embed_mode_requires_context_manager() -> None:
    """Embed-mode App.register() outside `with` raises RuntimeError (docs/sdk-api/python.md)."""
    a = bv.App()  # embed mode, no `with`
    with pytest.raises(RuntimeError, match="context manager"):
        a.register()
