"""Pytest coverage for ``register(force=True)`` interaction matrix.

``python/tests/v0/test_register_flags.py`` covers two happy paths
(``force=True`` survives a destructive diff; ``dry_run=True`` returns the
diff without committing). This file fills the audit gap by exercising
combinations and edge cases that single-shape happy-path tests miss:

  1. Force-register that PRESERVES an unchanged event source while
     swapping a downstream table.
  2. Force-register that ADDS a new table while keeping an existing
     unchanged one (and the existing table's state must survive).
  3. Force-register that REMOVES a table while keeping another (and the
     removed table must subsequently surface as ``unknown_table``).
  4. Force-register with a CONFLICTING FIELD TYPE for an existing event
     field (destructive: ``f64`` -> ``i64``).
  5. The matching no-force baseline: same destructive change without
     ``force=True`` MUST raise ``force_required``.
  6. ``force=True`` AND ``dry_run=True`` together — per
     ``docs/http/register.mdx``: "dry_run wins so a 'what would this do?'
     probe never escalates to a real mutation."

Anti-pattern guard (matches Phase 13.5.1 D-05): NO mock objects — every
test runs against a real spawned subprocess via the local ``app`` fixture
which composes the session-scoped ``beava_binary`` fixture from
``python/tests/conftest.py``.
"""
from __future__ import annotations

from typing import Any, Generator

import pytest

import beava as bv
from beava._errors import RegistrationError

# ---------------------------------------------------------------------------
# Skip guard — mirrors python/tests/v0/conftest._engine_available so tests
# in this file are collected cleanly even when the engine + SDK aren't
# fully wired (e.g. mid-refactor branches).
# ---------------------------------------------------------------------------


def _engine_available() -> bool:
    required = ("mean", "var", "std", "n_unique", "quantile", "table", "sum", "count")
    return all(hasattr(bv, name) for name in required)


pytestmark = pytest.mark.skipif(
    not _engine_available(),
    reason="requires Phase 13.4 engine + Phase 13.5 SDK (bv.table + Polars helpers)",
)


@pytest.fixture
def app(beava_binary: Any) -> Generator[Any, None, None]:  # noqa: ARG001
    """Yield a fresh embed-mode ``bv.App(test_mode=True)`` per test.

    ``beava_binary`` is the session-scoped fixture from
    ``python/tests/conftest.py`` which builds ``target/debug/beava`` once
    per session. Pulled in for the build side-effect; the App spawns the
    subprocess itself via the embed transport.
    """
    with bv.App(test_mode=True) as instance:
        yield instance


# ---------------------------------------------------------------------------
# 1. Force-register with event source unchanged + table replaced.
# ---------------------------------------------------------------------------


def test_force_register_keeps_unchanged_event_source_only_tables_changed(
    app: Any,
) -> None:
    """Re-register the same event ``Txn`` with a structurally different
    ``UserTxn`` table (count -> sum). The event source is byte-identical
    so it must land in ``already_present``; the destructive table swap
    requires ``force=True``. Pushes after the swap reflect the new
    aggregation (sum), confirming no stale event-source routing.
    """

    @bv.event
    class Txn:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    first = app.register(Txn, UserTxn)
    assert first["status"] == "ok"

    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:  # noqa: F811 — intentional rebind
        return txn.group_by("user_id").agg(s=bv.sum("amount", window="forever"))

    second = app.register(Txn, UserTxn, force=True)
    assert second["status"] == "ok"

    # Event source was byte-equivalent; the server should surface that
    # via the already_present list when present (some response shapes
    # omit it). When present it MUST contain ``Txn`` and MUST NOT contain
    # ``UserTxn`` (the table was destructively replaced).
    if "already_present" in second:
        ap = second["already_present"]
        assert "Txn" in ap, (
            f"unchanged event source Txn must surface in already_present; "
            f"got {ap!r}"
        )
        assert "UserTxn" not in ap, (
            f"destructively-replaced UserTxn must NOT be already_present; "
            f"got {ap!r}"
        )

    # Push + get: new (sum) aggregation must be active.
    for amount in (2.0, 3.0, 5.0):
        app.push("Txn", {"user_id": "alice", "amount": amount})
    row = app.get("UserTxn", "alice")
    assert "s" in row and "c" not in row, (
        f"post-force row must reflect new aggregation only; got {row!r}"
    )
    assert abs(row["s"] - 10.0) < 1e-9, f"sum(amount) must equal 10.0; got {row!r}"


# ---------------------------------------------------------------------------
# 2. Force-register adds a new table while keeping an existing one.
# ---------------------------------------------------------------------------


def test_force_register_adds_new_aggs_keeping_existing(app: Any) -> None:
    """Register ``Txn + UserCount``. Push 3 events. Force-register
    ``Txn + UserCount + UserSum`` (UserSum is new, additive only).

    Contract: ``UserCount`` is byte-equivalent so its accumulated state
    survives the re-register. ``UserSum`` starts fresh (no historical
    events replayed against newly-added aggregations).
    """

    @bv.event
    class Txn:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserCount(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    app.register(Txn, UserCount)

    for amount in (1.0, 2.0, 4.0):
        app.push("Txn", {"user_id": "alice", "amount": amount})

    pre_row = app.get("UserCount", "alice")
    assert pre_row == {"c": 3}, f"baseline count must be 3; got {pre_row!r}"

    @bv.table(key="user_id")
    def UserSum(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(s=bv.sum("amount", window="forever"))

    # Pure-additive register; force=True is not strictly required but
    # asking for it must not corrupt the additive case.
    result = app.register(Txn, UserCount, UserSum, force=True)
    assert result["status"] == "ok"

    # UserCount state must survive — count is still 3, no replay
    # double-counting.
    post_count_row = app.get("UserCount", "alice")
    assert post_count_row == {"c": 3}, (
        f"force-register additive change must NOT replay events into existing "
        f"aggregations; pre={pre_row!r}, post={post_count_row!r}"
    )

    # UserSum is freshly added — cold-start until new events arrive.
    cold = app.get("UserSum", "alice")
    assert cold == {} or cold is None, (
        f"newly-added UserSum must be cold-start before any post-add events; "
        f"got {cold!r}"
    )

    # Push one event AFTER the additive register; both tables advance.
    app.push("Txn", {"user_id": "alice", "amount": 10.0})
    final_count = app.get("UserCount", "alice")
    final_sum = app.get("UserSum", "alice")
    assert final_count == {"c": 4}, f"count must advance to 4; got {final_count!r}"
    assert "s" in final_sum and abs(final_sum["s"] - 10.0) < 1e-9, (
        f"UserSum sees only post-add events; got {final_sum!r}"
    )


# ---------------------------------------------------------------------------
# 3. Force-register removes a table while keeping another.
# ---------------------------------------------------------------------------


def test_force_register_removes_aggs_keeping_others(app: Any) -> None:
    """Register S + T1 + T2. Push events. Force-register S + T1 only —
    T2 must be gone (``unknown_table`` on get) and T1 must keep
    advancing for new events.
    """

    @bv.event
    class Txn:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def T1(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    @bv.table(key="user_id")
    def T2(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(s=bv.sum("amount", window="forever"))

    app.register(Txn, T1, T2)
    for amount in (1.0, 2.0, 3.0):
        app.push("Txn", {"user_id": "alice", "amount": amount})

    # Sanity-baseline: both tables answer.
    assert app.get("T1", "alice") == {"c": 3}
    assert abs(app.get("T2", "alice")["s"] - 6.0) < 1e-9

    # Drop T2 via force-register (T2 absent from payload = destructive
    # removal of an entire derivation; requires force=True).
    result = app.register(Txn, T1, force=True)
    assert result["status"] == "ok"

    # T1 still answers and its state survives the re-register (it was
    # byte-equivalent across the two register calls).
    post_t1 = app.get("T1", "alice")
    assert post_t1 == {"c": 3}, (
        f"T1 was unchanged across re-register; state must survive; got {post_t1!r}"
    )

    # T2 is fully gone; the SDK surfaces it as RegistrationError(unknown_table).
    with pytest.raises(RegistrationError) as exc_info:
        app.get("T2", "alice")
    assert exc_info.value.code == "unknown_table", (
        f"removed table must surface as unknown_table; got code="
        f"{exc_info.value.code!r} message={exc_info.value.message!r}"
    )

    # T1 continues to track new events post-removal.
    app.push("Txn", {"user_id": "alice", "amount": 99.0})
    assert app.get("T1", "alice") == {"c": 4}


# ---------------------------------------------------------------------------
# 4. Force-register with conflicting field types for the same field.
# ---------------------------------------------------------------------------


def test_force_register_with_conflicting_field_types_for_same_field(
    app: Any,
) -> None:
    """Register ``Txn`` with ``amount: float``. Force-register the same
    event name with ``amount: int``. Lock the observed behaviour.

    Observed 2026-05-14 (post-13.7.5 SDK against post-13.4 engine):
      - The server ACCEPTS the destructive ``f64 -> i64`` type-change
        when ``force=True`` is set — no ``force_required`` raised.
      - The pre-swap accumulated state SURVIVES the swap: a baseline of
        ``s=1.0+2.0=3.0`` (f64) reads back unchanged immediately after
        the re-register.
      - Subsequent int pushes are ADDED to the existing f64 accumulator
        (server-side coercion): pushing ``7`` then ``3`` lifts the
        result to ``s=13.0``.

    Documented divergence: ``docs/http/register.mdx`` states "Destructive
    changes ... drop the affected descriptor's accumulated state when
    applied." The observed behaviour preserves state and coerces. This
    test locks the OBSERVED behaviour so a future fix that aligns
    server with docs (drop state on type-change) MUST update this
    assertion explicitly rather than silently regress.
    """

    @bv.event
    class Txn:
        user_id: str
        amount: float  # f64

    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(s=bv.sum("amount", window="forever"))

    app.register(Txn, UserTxn)
    for amount in (1.0, 2.0):
        app.push("Txn", {"user_id": "alice", "amount": amount})
    baseline = app.get("UserTxn", "alice")
    assert "s" in baseline and abs(baseline["s"] - 3.0) < 1e-9, (
        f"baseline f64 sum must equal 3.0; got {baseline!r}"
    )

    # Re-define Txn with amount as int (i64) — destructive type-change.
    @bv.event
    class Txn:  # noqa: F811 — intentional rebind
        user_id: str
        amount: int

    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:  # noqa: F811
        return txn.group_by("user_id").agg(s=bv.sum("amount", window="forever"))

    # Observed behaviour: force=True accepts the type-change cleanly; no
    # RegistrationError surfaces.
    result = app.register(Txn, UserTxn, force=True)
    assert result["status"] == "ok"

    # Observed behaviour: prior f64 state is PRESERVED across the
    # destructive type-change (divergence from docs/http/register.mdx
    # which claims destructive changes drop state).
    post_swap = app.get("UserTxn", "alice")
    assert "s" in post_swap and abs(post_swap["s"] - 3.0) < 1e-9, (
        f"prior f64 accumulator must survive the destructive type swap "
        f"(observed-locked behaviour as of 2026-05-14); got {post_swap!r}"
    )

    # Observed behaviour: subsequent int pushes are coerced and added on
    # top of the surviving f64 accumulator. 3.0 + 7 + 3 = 13.0.
    app.push("Txn", {"user_id": "alice", "amount": 7})
    app.push("Txn", {"user_id": "alice", "amount": 3})
    final = app.get("UserTxn", "alice")
    assert "s" in final, f"post-swap aggregation must respond; got {final!r}"
    assert abs(float(final["s"]) - 13.0) < 1e-9, (
        f"post-swap sum must equal 13.0 (3.0 surviving f64 baseline + 7 + 3 "
        f"coerced int pushes); got {final!r}"
    )


# ---------------------------------------------------------------------------
# 5. Destructive change WITHOUT force= must raise force_required.
# ---------------------------------------------------------------------------


def test_force_required_for_destructive_change_without_force(app: Any) -> None:
    """Register S + T1. Re-register S + T2 (T1 dropped, T2 added) without
    ``force=True``. The server MUST raise ``RegistrationError`` with
    ``code="force_required"`` — destructive removal of an entire
    derivation requires explicit confirmation.

    Mirrors ``test_register_force_with_conflicting_field_types`` from
    ``test_error_response_codes.py`` at the SDK surface (that test
    exercises raw-JSON HTTP / TCP shapes; this one exercises the Python
    SDK boundary).
    """

    @bv.event
    class Txn:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def T1(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    app.register(Txn, T1)

    @bv.table(key="user_id")
    def T2(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(s=bv.sum("amount", window="forever"))

    # Re-register with T1 dropped (destructive) and T2 added (additive).
    # Without force=True the destructive entry triggers force_required.
    with pytest.raises(RegistrationError) as exc_info:
        app.register(Txn, T2)
    assert exc_info.value.code == "force_required", (
        f"destructive re-register without force= must raise force_required; "
        f"got code={exc_info.value.code!r} message={exc_info.value.message!r}"
    )


# ---------------------------------------------------------------------------
# 6. force=True AND dry_run=True together — dry_run dominates.
# ---------------------------------------------------------------------------


def test_force_and_dry_run_together(app: Any) -> None:
    """Per ``docs/http/register.mdx``::

        dry_run=true, force=true still resolves to dry-run — dry-run
        wins so a 'what would this do?' probe never escalates to a real
        mutation.

    Verify: register pipeline-A, push events, then re-register a
    destructive pipeline-B with BOTH ``force=True`` AND ``dry_run=True``.
    The call must succeed (no exception, no ``force_required``) AND it
    must NOT commit — a subsequent ``app.get`` reads the pipeline-A
    baseline row.
    """

    @bv.event
    class Txn:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:
        return txn.group_by("user_id").agg(c=bv.count(window="forever"))

    app.register(Txn, UserTxn)
    for _ in range(5):
        app.push("Txn", {"user_id": "alice", "amount": 1.0})
    baseline = app.get("UserTxn", "alice")
    assert baseline == {"c": 5}, f"baseline must be {{'c': 5}}; got {baseline!r}"

    # Destructive re-register payload — count -> sum is destructive (agg
    # removal + new agg with different output type).
    @bv.table(key="user_id")
    def UserTxn(txn: Txn) -> Any:  # noqa: F811
        return txn.group_by("user_id").agg(s=bv.sum("amount", window="forever"))

    # force=True + dry_run=True: must NOT raise force_required (force is
    # set) AND must NOT commit (dry_run dominates).
    result = app.register(Txn, UserTxn, force=True, dry_run=True)
    assert isinstance(result, dict), (
        f"force=True + dry_run=True must return a dict envelope; "
        f"got {type(result).__name__}"
    )

    # State is unchanged — the destructive swap did NOT land. The
    # baseline column ``c`` is still present with value 5.
    post = app.get("UserTxn", "alice")
    assert post == baseline, (
        f"dry_run must dominate when combined with force=True; "
        f"baseline={baseline!r}, post-call={post!r}"
    )
