"""Lock down which ``bv.col`` overload serializations the server accepts.

The server's where-predicate parser (crates/beava-core/src/expr.rs:340-470)
only accepts: ``==``, ``!=``, ``<``, ``>``, ``<=``, ``>=``, ``and``, ``or``,
``not`` (keyword), parentheses, identifiers, literals. A bare ``!`` is
rejected with ``unexpected character '!'``.

Covers each overload that produces a where-predicate, end-to-end:
``==`` / ``!=`` (literal comparison), ``&`` / ``|`` (compound predicates),
and ``~`` (logical NOT — serialises to the ``not`` keyword the parser
accepts, not the bare ``!`` that it rejects).

Requires: ``target/debug/beava`` (or release) discoverable via embed mode.
"""

from __future__ import annotations

import pytest

import beava as bv


@pytest.fixture
def app(beava_binary):  # noqa: ARG001 — fixture pulled in for binary side-effect
    """Yield a fresh embed-mode ``bv.App(test_mode=True)`` per test."""
    with bv.App(test_mode=True) as instance:
        yield instance


# ---------------------------------------------------------------------------
# Accepted overloads
# ---------------------------------------------------------------------------


def test_eq_serializes_and_accepted(app):
    """``bv.col('ok') == True`` → ``(ok == true)`` → register OK."""

    @bv.event
    class Outcome:
        user_id: str
        ok: bool

    @bv.table(key="user_id")
    def UserOks(os: Outcome):
        return os.group_by("user_id").agg(
            n_ok=bv.count(where=bv.col("ok") == True),  # noqa: E712
        )

    resp = app.register(Outcome, UserOks)
    assert resp.get("status") == "ok", f"register failed: {resp!r}"


def test_ne_serializes_and_accepted(app):
    """``bv.col('status') != 'active'`` → register OK."""

    @bv.event
    class User:
        user_id: str
        status: str

    @bv.table(key="user_id")
    def InactiveCounts(us: User):
        return us.group_by("user_id").agg(
            n_not_active=bv.count(where=bv.col("status") != "active"),
        )

    resp = app.register(User, InactiveCounts)
    assert resp.get("status") == "ok", f"register failed: {resp!r}"


def test_and_or_serialize_and_accepted(app):
    """Compound ``(col(x) == 1) & (col(y) > 0)`` and ``| ...`` register cleanly."""

    @bv.event
    class Tx:
        user_id: str
        x: int
        y: int

    @bv.table(key="user_id")
    def AndOrCounts(txs: Tx):
        return txs.group_by("user_id").agg(
            both=bv.count(where=(bv.col("x") == 1) & (bv.col("y") > 0)),
            either=bv.count(where=(bv.col("x") == 1) | (bv.col("y") > 0)),
        )

    resp = app.register(Tx, AndOrCounts)
    assert resp.get("status") == "ok", f"register failed: {resp!r}"


# ---------------------------------------------------------------------------
# `~` (invert) overload — now emits `(not …)` and is accepted by the server
# ---------------------------------------------------------------------------


def test_invert_serializes_and_accepted(app):
    """``~bv.col('ok')`` → ``(not ok)`` → register OK.

    Previously the SDK emitted ``!(ok)`` which the server's where-parser
    rejected at ``expr.rs:370`` with ``unexpected character '!'``. The
    parser accepts ``not`` as a keyword (``expr.rs:463``) so the fix is
    purely SDK-side: emit ``(not x)`` instead of ``!(x)``.
    """

    @bv.event
    class Outcome:
        user_id: str
        ok: bool

    # Wire-shape: SDK must emit the keyword form the server accepts.
    assert (~bv.col("ok")).to_expr_string() == "(not ok)"

    @bv.table(key="user_id")
    def UserNotOks(os: Outcome):
        return os.group_by("user_id").agg(
            n_not_ok=bv.count(where=~bv.col("ok")),
        )

    resp = app.register(Outcome, UserNotOks)
    assert resp.get("status") == "ok", f"register failed: {resp!r}"


def test_invert_compound_with_and_or_accepted(app):
    """``~(col(x) == 1) & col(y)`` — nested invert inside a compound predicate
    must still serialise + parse cleanly. Locks down that the keyword
    form composes with ``and`` / ``or``.
    """

    @bv.event
    class Ev:
        user_id: str
        x: int
        y: bool

    @bv.table(key="user_id")
    def Counts(es: Ev):
        return es.group_by("user_id").agg(
            n=bv.count(where=(~(bv.col("x") == 1)) & bv.col("y")),
        )

    resp = app.register(Ev, Counts)
    assert resp.get("status") == "ok", f"register failed: {resp!r}"
