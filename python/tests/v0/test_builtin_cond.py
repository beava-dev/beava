"""TDD red — if_else builtin Python sugar (PR 4).

What this file checks: ``bv.if_else(cond, then_, else_)`` produces the
correct wire string in all argument combinations.

Why if_else is a top-level function, not a method: ``col.if_else(...)``
reads as if the column is the subject, but the condition is. ``bv.if_else``
is the single conditional surface in the SDK.

Status: fails until Step 9 of PR 4 adds ``bv.if_else`` to the module.
"""
from __future__ import annotations

import beava as bv
from beava._col import _Call


# ── basic wire output ────────────────────────────────────────────────────────


def test_if_else_three_columns() -> None:
    """All three args as column references. Pins the wire format:
    ``if_else(cond, a, b)`` with no extra parens around plain field names."""
    expr = bv.if_else(bv.col("flag"), bv.col("score"), bv.col("default"))
    assert expr.to_expr_string() == "if_else(flag, score, default)"


def test_if_else_produces_call_node() -> None:
    """Result is a ``_Call`` node, same shape as every other builtin."""
    expr = bv.if_else(bv.col("c"), bv.col("a"), bv.col("b"))
    assert isinstance(expr, _Call)
    assert expr.name == "if_else"
    assert len(expr.args) == 3


def test_if_else_literal_args_are_coerced() -> None:
    """Plain Python literals in any position get auto-wrapped.
    Numbers render bare; strings get single quotes."""
    expr = bv.if_else(bv.col("flag"), 1, 0)
    assert expr.to_expr_string() == "if_else(flag, 1, 0)"

    expr_str = bv.if_else(bv.col("flag"), "yes", "no")
    assert expr_str.to_expr_string() == "if_else(flag, 'yes', 'no')"


def test_if_else_condition_is_expression() -> None:
    """The condition can itself be a compound expression. The wire format
    wraps binary ops in parens, which is the standard ``_BinOp`` output."""
    expr = bv.if_else(bv.col("amount") > 100, bv.col("amount"), 0)
    assert expr.to_expr_string() == "if_else((amount > 100), amount, 0)"


def test_if_else_nested() -> None:
    """if_else can be nested — the outer takes the result of the inner as
    one of its branches. This is the lowering target for elif chains."""
    inner = bv.if_else(bv.col("b"), bv.col("x"), bv.col("y"))
    outer = bv.if_else(bv.col("a"), bv.col("z"), inner)
    assert outer.to_expr_string() == "if_else(a, z, if_else(b, x, y))"


# ── composition with PR 3 builtins ───────────────────────────────────────────


def test_if_else_composes_with_string_builtins() -> None:
    """if_else and string methods compose: use the lowercased email when it
    contains @, otherwise use a literal placeholder."""
    expr = bv.if_else(
        bv.col("email").contains("@"),
        bv.col("email").lower(),
        bv.lit("unknown"),
    )
    assert (
        expr.to_expr_string()
        == "if_else(contains(email, '@'), lower(email), 'unknown')"
    )
