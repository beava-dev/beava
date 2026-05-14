"""SDK-side wire-shape tests for the ``~`` (invert) overload on ``bv.col``.

The server's where-parser (crates/beava-core/src/expr.rs:361-373) accepts
``!=`` for not-equal and ``not`` (keyword, expr.rs:463) for logical
negation, but rejects bare unary ``!`` with ``unexpected character '!'``.
The SDK's ``__invert__`` overload must therefore serialise to ``(not x)``,
not ``!(x)``. These are SDK-side unit tests — no server required — that
lock the wire-shape so the regression can be caught by ``pytest`` alone.

Companion to ``python/tests/test_col_overload_server_acceptance.py`` which
exercises the end-to-end roundtrip against a running server.
"""

from __future__ import annotations

import beava as bv


def test_invert_emits_not_keyword() -> None:
    """``~bv.col('ok')`` must serialise to ``(not ok)``."""
    assert (~bv.col("ok")).to_expr_string() == "(not ok)"


def test_invert_eq_emits_not_around_eq() -> None:
    """``~(bv.col('x') == 1)`` must serialise to ``(not (x == 1))``."""
    assert (~(bv.col("x") == 1)).to_expr_string() == "(not (x == 1))"


def test_invert_compound_and_emits_not_around_compound() -> None:
    """``~((col x ==1) & col y)`` must keep parenthesisation around the inner
    compound and emit a single leading ``(not …)``."""
    expr = ~((bv.col("x") == 1) & bv.col("y"))
    assert expr.to_expr_string() == "(not ((x == 1) and y))"


def test_double_invert_emits_two_nots() -> None:
    """``~~bv.col('x')`` must round-trip both negations rather than
    collapsing — the parser handles it; the SDK should not optimise here."""
    assert (~~bv.col("x")).to_expr_string() == "(not (not x))"
