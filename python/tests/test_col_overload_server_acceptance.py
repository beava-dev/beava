"""Lock down which ``bv.col`` overload serializations the server accepts.

The server's where-predicate parser (crates/beava-core/src/expr.rs:340-470)
only accepts: ``==``, ``!=``, ``<``, ``>``, ``<=``, ``>=``, ``and``, ``or``,
``not`` (keyword), parentheses, identifiers, literals. A bare ``!`` is
rejected with ``unexpected character '!'``.

The SDK's ``~bv.col(...)`` overload (python/beava/_col.py: _UnaryOp.to_expr_string)
currently serialises as ``!(x)`` — which the server REJECTS. This file locks
down that mismatch so an eventual fix (serialising to ``(not x)`` instead)
will surface as a green test that previously was red.

Requires: ``target/debug/beava`` (or release) discoverable via embed mode.
"""

from __future__ import annotations

import pytest

import beava as bv
from beava._errors import RegistrationError


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
# Currently-rejected overload — the `~` invert bug
# ---------------------------------------------------------------------------


def test_invert_currently_rejected_by_server(app):
    """``~bv.col('ok')`` → ``!(ok)`` is rejected by the server's expr parser.

    This is a KNOWN SDK bug: the ``__invert__`` overload should emit
    ``(not ok)`` (a keyword the parser accepts) instead of ``!(ok)`` (a
    bare ``!`` that the parser rejects at expr.rs:370 with ``unexpected
    character '!'``).

    When the SDK is fixed, this test will start passing the register call —
    flip ``pytest.raises`` to ``assert success`` at that point.
    """

    @bv.event
    class Outcome:
        user_id: str
        ok: bool

    # Sanity: confirm the SDK still emits the bug-shape on the wire.
    assert (~bv.col("ok")).to_expr_string() == "!(ok)", (
        "Expected the SDK's __invert__ to emit '!(ok)' so this regression "
        "lock-down catches a future fix. If the SDK changed, update this "
        "test."
    )

    @bv.table(key="user_id")
    def UserNotOks(os: Outcome):
        return os.group_by("user_id").agg(
            n_not_ok=bv.count(where=~bv.col("ok")),
        )

    with pytest.raises(RegistrationError) as exc_info:
        app.register(Outcome, UserNotOks)

    err = exc_info.value
    assert err.code == "aggregation_invalid_where", (
        f"expected aggregation_invalid_where, got code={err.code!r} "
        f"message={err.message!r}"
    )
    # The parse error message must mention the bare '!' the parser
    # tripped on, anchoring this test to the actual server-side cause.
    assert "!" in err.message, (
        f"expected parse error to mention '!'; got message={err.message!r}"
    )
