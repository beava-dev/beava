"""Smoke coverage for the public ``beava.test`` helpers.

``beava.test`` is the bundled test-helper surface that users import to
write tests against beava — fixtures, mock app, assertion helpers, and
the ``replay`` driver. This file locks the four public symbols
re-exported from ``beava.test.__init__`` plus their negative-path
contracts (mismatched dicts, missing ``_event`` key, etc.).

Most tests in this file are pure-Python (use ``MockApp`` rather than a
real embed server); only the ``fixture`` generator test needs the binary
and is skipped when it isn't available.
"""
from __future__ import annotations

import os
import shutil
from pathlib import Path

import pytest

import beava as bv
from beava.test import MockApp, assert_features_eq, fixture, replay

# ---------------------------------------------------------------------------
# __init__ re-export contract — all four public symbols are importable
# ---------------------------------------------------------------------------


def test_test_module_reexports_four_public_symbols() -> None:
    """``beava.test`` re-exports exactly the four documented helpers."""
    import beava.test as bt

    assert set(bt.__all__) == {"fixture", "replay", "assert_features_eq", "MockApp"}
    for name in bt.__all__:
        assert hasattr(bt, name), f"beava.test must expose {name}"


# ---------------------------------------------------------------------------
# assert_features_eq
# ---------------------------------------------------------------------------


def test_assert_features_eq_happy_path_exact() -> None:
    """Identical dicts must pass — no exception."""
    assert_features_eq({"a": 1, "b": 2.0}, {"a": 1, "b": 2.0})


def test_assert_features_eq_happy_path_floats_close() -> None:
    """Floats within ``rel_tol``/``abs_tol`` must compare equal."""
    # Default tolerance is rel_tol=1e-9, abs_tol=1e-12.
    assert_features_eq({"x": 1.0}, {"x": 1.0 + 1e-15})


def test_assert_features_eq_raises_on_key_mismatch() -> None:
    """Different key sets must raise with a message naming the diff."""
    with pytest.raises(AssertionError, match="missing keys|extra keys"):
        assert_features_eq({"a": 1}, {"a": 1, "b": 2})


def test_assert_features_eq_raises_on_float_mismatch() -> None:
    """Floats outside tolerance must raise."""
    with pytest.raises(AssertionError, match="feature 'x' differ"):
        assert_features_eq({"x": 1.0}, {"x": 2.0})


def test_assert_features_eq_raises_on_non_float_mismatch() -> None:
    """Non-float scalar mismatches must raise."""
    with pytest.raises(AssertionError, match="feature 'name' differ"):
        assert_features_eq({"name": "alice"}, {"name": "bob"})


def test_assert_features_eq_mixed_float_int_within_tolerance() -> None:
    """``got=1`` vs ``want=1.0`` (one side float) must pass via isclose."""
    assert_features_eq({"x": 1}, {"x": 1.0})


# ---------------------------------------------------------------------------
# MockApp — the in-memory test double
# ---------------------------------------------------------------------------


def test_mock_app_is_context_manager() -> None:
    """``MockApp`` supports ``with`` and marks itself closed on exit."""
    with MockApp() as app:
        assert isinstance(app, MockApp)
        assert app._closed is False
    assert app._closed is True


def test_mock_app_register_records_call_and_descriptors() -> None:
    """``register`` appends to ``_calls`` and stores descriptors."""
    app = MockApp()

    @bv.event
    class Click:
        user_id: str

    resp = app.register(Click, force=True)
    assert resp["status"] == "ok"
    assert resp["registry_version"] == 1
    assert app._calls[0][0] == "register"
    assert Click in app._registered


def test_mock_app_push_returns_monotonic_ack_lsn() -> None:
    """``push`` returns an ``ack_lsn`` that grows with each call."""
    app = MockApp()
    r1 = app.push("Click", {"user_id": "u1"})
    r2 = app.push("Click", {"user_id": "u2"})
    assert r1["ack_lsn"] < r2["ack_lsn"]
    assert ("push", "Click", {"user_id": "u1"}) in app._calls


def test_mock_app_get_returns_canned_response_or_empty() -> None:
    """``_set_get_response`` seeds a response; unseeded keys return ``{}``."""
    app = MockApp()
    app._set_get_response("UserTable", "u1", {"count": 5})
    assert app.get("UserTable", "u1") == {"count": 5}
    assert app.get("UserTable", "missing") == {}


def test_mock_app_get_handles_list_key() -> None:
    """``get`` with a list key normalises to tuple before lookup."""
    app = MockApp()
    app._set_get_response("Table", ["a", "b"], {"v": 1})
    assert app.get("Table", ["a", "b"]) == {"v": 1}


def test_mock_app_get_none_key_falls_back_to_empty_string() -> None:
    """``app.get('Table')`` with no key (None) is normalised to ``""`` so
    keyless tables (global aggregations) share a single lookup slot."""
    app = MockApp()
    app._set_get_response("Global", "", {"total": 7})
    assert app.get("Global") == {"total": 7}
    # And the unconfigured key-less variant is the empty-dict cold-start.
    other = MockApp()
    assert other.get("Global") == {}


def test_mock_app_batch_get_returns_one_response_per_request() -> None:
    """``batch_get`` returns a list aligned 1:1 with the input."""
    app = MockApp()
    app._set_get_response("T", "a", {"v": 1})
    out = app.batch_get([("T", "a"), ("T", "missing")])
    assert out == [{"v": 1}, {}]


def test_mock_app_reset_clears_canned_responses() -> None:
    """``reset`` clears stored responses but keeps the call log."""
    app = MockApp()
    app._set_get_response("T", "a", {"v": 1})
    app.reset()
    assert app.get("T", "a") == {}
    assert ("reset",) in app._calls


def test_mock_app_ping_reports_mock_version_and_registry_version() -> None:
    """``ping`` returns the mock version sentinel and current registry count."""
    app = MockApp()
    r = app.ping()
    assert r["server_version"] == "0.0.0-mock"
    assert r["registry_version"] == 0


# ---------------------------------------------------------------------------
# replay — drive a list of event dicts through ``app.push``
# ---------------------------------------------------------------------------


def test_replay_drives_push_in_order() -> None:
    """``replay`` calls ``app.push`` once per event in list order, stripping
    the ``_event`` key from the fields payload."""
    app = MockApp()
    events = [
        {"_event": "Click", "user_id": "u1", "page": "/home"},
        {"_event": "Click", "user_id": "u2", "page": "/about"},
    ]
    replay(app, events)
    push_calls = [c for c in app._calls if c[0] == "push"]
    assert len(push_calls) == 2
    assert push_calls[0] == ("push", "Click", {"user_id": "u1", "page": "/home"})
    assert push_calls[1] == ("push", "Click", {"user_id": "u2", "page": "/about"})


def test_replay_missing_event_key_raises() -> None:
    """Each event dict must carry an ``_event`` key — missing is fatal."""
    app = MockApp()
    with pytest.raises(ValueError, match="missing '_event' key"):
        replay(app, [{"user_id": "u1"}])


def test_replay_empty_event_list_no_op() -> None:
    """An empty event list must not call push at all."""
    app = MockApp()
    replay(app, [])
    assert app._calls == []


# ---------------------------------------------------------------------------
# fixture — yields a real bv.App when the binary is available
# ---------------------------------------------------------------------------


def _binary_available() -> bool:
    if os.environ.get("BEAVA_BINARY"):
        p = Path(os.environ["BEAVA_BINARY"])
        return p.is_file() and os.access(p, os.X_OK)
    if shutil.which("beava") is not None:
        return True
    for parent in [Path.cwd(), *Path.cwd().parents]:
        cand = parent / "target" / "debug" / "beava"
        if cand.is_file() and os.access(cand, os.X_OK):
            return True
    return False


@pytest.mark.skipif(
    not _binary_available(),
    reason="bv.App embed mode needs the beava binary on disk",
)
def test_fixture_yields_an_entered_app() -> None:
    """``fixture(reset_each=True)`` must yield an entered ``bv.App`` instance
    that is callable. We drive the generator manually since this *is* the
    fixture under test (we don't want to invoke it as a real pytest fixture
    inside another test)."""
    gen = fixture(reset_each=True, test_mode=True, timeout=30.0)
    app = next(gen)
    try:
        assert isinstance(app, bv.App)
        # The fixture entered the context manager → ping should round-trip.
        ping = app.ping()
        assert "server_version" in ping
    finally:
        # Drain the generator so the bv.App context manager exits cleanly.
        with pytest.raises(StopIteration):
            next(gen)


@pytest.mark.skipif(
    not _binary_available(),
    reason="bv.App embed mode needs the beava binary on disk",
)
def test_fixture_swallows_reset_failure_when_not_test_mode() -> None:
    """A non-test-mode App rejects ``app.reset()`` with a RuntimeError; the
    fixture must swallow that and still yield the app. Exercises the
    ``except (RuntimeError, Exception)`` arm in ``_fixtures.fixture``."""
    gen = fixture(reset_each=True, test_mode=False, timeout=30.0)
    app = next(gen)
    try:
        assert isinstance(app, bv.App)
        # If reset had bubbled up, we'd never reach this line.
        assert app.ping()["server_version"]
    finally:
        with pytest.raises(StopIteration):
            next(gen)
