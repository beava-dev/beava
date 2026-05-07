"""Schema-evolution diff matrix integration tests (D-03 MUST-FIX).

Covers SRV-REG-03-evolve from `.planning/phases/13.7.5-pre-oss-code-polish/COVERAGE-GAPS.md`
§ Python gaps with three subcases:

  - ``test_additive_succeeds`` — adding a new aggregation feature → 200 + version bump.
  - ``test_destructive_returns_409`` — removing an existing feature without
    ``force=True`` → 409 ``force_required``.
  - ``test_changed_field_type_returns_409`` — changing a feature's window →
    409 ``force_required``.

Server-side conflict detection is independently covered by
`crates/beava-server/tests/phase13_4_force_register.rs`; this file pins the
SDK error wrapping (per ``docs/error-codes.md`` §
``force_required``, which lists ``window_change`` and ``agg_removal`` among
the destructive classes).

Anti-pattern guard (Phase 13.5.1 D-05, USER-LOCKED): NO mock objects —
every test runs against a real engine via the shared ``app`` fixture
(``bv.App(test_mode=True)``).
"""
from __future__ import annotations

from typing import Any

import pytest

import beava as bv
from beava._errors import RegistrationError

from ._helpers import _engine_available

pytestmark = pytest.mark.skipif(
    not _engine_available(),
    reason="requires Phase 13.4 engine + Phase 13.5 SDK rewrite + Phase 13.5.1 transport-impl",
)


def test_additive_succeeds(app: Any) -> None:
    """SRV-REG-03 additive: adding a wholly new descriptor → 200 + version bump.

    Per ``docs/error-codes.md`` § ``force_required``, ``new_descriptor`` is
    in the additive list (no ``force=True`` required). The server returns
    ``status="ok"`` with a bumped ``registry_version`` integer.

    Note: the COVERAGE-GAPS row text says "adding a new aggregation feature".
    The cleanly-additive form supported by the v0 impl (and the form the
    documented diff envelope's ``new_descriptor`` covers) is adding a
    brand-new top-level descriptor — a new event source plus a new
    derivation. Adding a new agg column INSIDE an existing table descriptor
    currently routes through the legacy Phase-2 diff machinery and surfaces
    as ``registration_conflict`` (per docs/error-codes.md § force_required's
    note: "the dispatch order is force_required FIRST, legacy
    registration_conflict SECOND (additive-only path with a diff that still
    detects schema drift)"). That dual-machinery edge is a documentation /
    impl mismatch tracked in SKIPPED-COVERAGE-ROWS.md; this test asserts
    the cleanly-supported additive case.
    """

    @bv.event
    class Txn:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    result_a = app.register(Txn, UserTxn)
    assert result_a["status"] == "ok"
    version_a = result_a.get("registry_version")
    assert isinstance(version_a, int), (
        f"registry_version must be int; got {type(version_a).__name__}={version_a!r}"
    )

    # Additive: brand-new descriptor (event + derivation) layered on top.
    # The existing UserTxn descriptor is also re-sent unchanged (the engine's
    # ``already_present`` matcher silently no-ops re-registrations of an
    # unchanged descriptor).
    @bv.event
    class Click:
        user_id: str
        page: str

    @bv.table(key="user_id")
    def UserClicks(clicks: Click) -> Any:
        return clicks.group_by("user_id").agg(n=bv.count(window="forever"))

    result_b = app.register(Txn, UserTxn, Click, UserClicks)
    assert result_b["status"] == "ok", (
        f"additive (new descriptor) must succeed without force=; got {result_b!r}"
    )
    version_b = result_b.get("registry_version")
    assert isinstance(version_b, int)
    assert version_b > version_a, (
        f"additive register must bump registry_version; "
        f"version_a={version_a}, version_b={version_b}"
    )

    # Smoke: the new descriptor is now queryable end-to-end.
    for _ in range(3):
        app.push("Click", {"user_id": "alice", "page": "/home"})
    click_row = app.get("UserClicks", "alice")
    assert click_row.get("n") == 3, f"new descriptor 'n' column must read 3; got {click_row!r}"


def test_destructive_returns_409(app: Any) -> None:
    """SRV-REG-03 destructive (agg removal): without force=True → ``force_required``.

    Per ``docs/error-codes.md`` § ``force_required``, ``agg_removal`` is in
    the destructive list — re-registering with a previously-present
    aggregation removed must raise ``RegistrationError(code="force_required")``
    (HTTP 409) when ``force=True`` is not set.
    """

    @bv.event
    class Txn:
        user_id: str
        amount: float

    # Baseline: 2-agg table.
    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(
            c=bv.count(window="forever"),
            s=bv.sum("amount", window="forever"),
        )

    app.register(Txn, UserTxn)

    # Destructive: drop the ``s`` column. Without force=True, the server
    # rejects with force_required (409).
    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:  # noqa: F811
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    with pytest.raises(RegistrationError) as exc_info:
        app.register(Txn, UserTxn)
    assert exc_info.value.code == "force_required", (
        f"agg_removal without force= must raise force_required; "
        f"got code={exc_info.value.code!r} message={exc_info.value.message!r}"
    )


def test_changed_field_type_returns_409(app: Any) -> None:
    """SRV-REG-03 destructive (window change): without force=True → ``force_required``.

    Per ``docs/error-codes.md`` § ``force_required``, ``window_change`` is in
    the destructive list. Changing an existing aggregation's window kwarg
    (e.g. ``forever`` → ``count_60``) must raise ``RegistrationError`` with
    ``code="force_required"`` (HTTP 409) when ``force=True`` is not set.

    (The COVERAGE-GAPS row text says "changing a feature's window"; the
    test_function_name says ``test_changed_field_type_returns_409`` — kept
    verbatim per the gap row + reflective of the broader "destructive type
    or shape change" class.)
    """

    @bv.event
    class Txn:
        user_id: str
        amount: float

    # Baseline: 1-hour count window.
    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(c=bv.count(window="1h"))

    app.register(Txn, UserTxn)

    # Destructive: same agg, different window — 1h → 30m.
    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:  # noqa: F811
        return txn.group_by("user_id").agg(c=bv.count(window="30m"))

    with pytest.raises(RegistrationError) as exc_info:
        app.register(Txn, UserTxn)
    assert exc_info.value.code == "force_required", (
        f"window_change without force= must raise force_required; "
        f"got code={exc_info.value.code!r} message={exc_info.value.message!r}"
    )
