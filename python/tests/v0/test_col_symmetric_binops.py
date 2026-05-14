"""Coverage for the right-hand operand variants and minor branches of ``beava._col``.

The forward arms (``__add__`` / ``__gt__`` / ...) are exercised heavily by
existing tests in ``test_lit.py`` and several integration suites. The
right-hand (``__radd__`` / ``__rsub__`` / ...) arms only fire when a plain
Python literal sits on the *left* of an operator — e.g. ``1 + bv.col("x")``
— and were previously uncovered. This file exercises:

* every binary right-arm (`+ - * /`)
* the boolean combinator right-arms (`& |`) and the unary `~`
* ``isnull()``
* ``cast()`` happy path and the invalid-target error branch
* ``_Expr.to_expr_string`` ``NotImplementedError`` on the abstract base
* ``__hash__`` implementations on ``_Col`` / ``_Literal`` / ``_BinOp`` /
  ``_UnaryOp`` / ``_CastOp`` (each restores hashability after the
  overridden ``__eq__``)
* ``_Literal.to_expr_string`` for ``None`` and ``bool`` values
* ``_UnaryOp.to_expr_string`` for the ``isnull`` / ``~`` paths and the
  ``ValueError`` raised for an unknown op tag

Pure-Python AST tests — no embed-mode binary needed, no skipif gate.
"""
from __future__ import annotations

from typing import Any, Callable

import pytest

import beava as bv
from beava._col import _BinOp, _CastOp, _Expr, _Literal, _UnaryOp

# ---------------------------------------------------------------------------
# Right-hand arithmetic arms — literal on the LEFT triggers __r*__
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    "op_func,left,col_name,want",
    [
        (lambda a, b: a + b, 1, "x", "(1 + x)"),
        (lambda a, b: a - b, 10, "x", "(10 - x)"),
        (lambda a, b: a * b, 2, "y", "(2 * y)"),
        (lambda a, b: a / b, 100, "z", "(100 / z)"),
    ],
)
def test_col_right_arithmetic_arms(
    op_func: Callable[[Any, Any], _Expr],
    left: Any,
    col_name: str,
    want: str,
) -> None:
    """Literal-on-left arithmetic must produce a ``_BinOp`` with left=literal."""
    result = op_func(left, bv.col(col_name))
    assert isinstance(result, _BinOp)
    assert result.to_expr_string() == want
    # The literal is wrapped via _coerce → _Literal; left side is NOT a _Col.
    assert isinstance(result.left, _Literal)
    assert result.left.value == left


def test_col_right_arms_match_forward_arm_wire_shape() -> None:
    """``5 - col("x")`` and ``bv.lit(5) - bv.col("x")`` are wire-identical."""
    a = 5 - bv.col("x")
    b = bv.lit(5) - bv.col("x")
    assert a.to_expr_string() == b.to_expr_string() == "(5 - x)"


# ---------------------------------------------------------------------------
# Right-hand boolean combinator arms — `&` / `|` with a literal on the left
# ---------------------------------------------------------------------------


def test_col_rand_arm_lit_left() -> None:
    """``True & bv.col('flag')`` triggers ``__rand__`` → ``and`` keyword.

    ``_Literal.to_expr_string`` lowercases bool values per the server grammar
    (``true``/``false`` keywords), so the rendered left operand is ``true``
    even though the Python literal is ``True``.
    """
    result = True & bv.col("flag")
    assert isinstance(result, _BinOp)
    assert result.op == "and"
    assert result.to_expr_string() == "(true and flag)"
    assert isinstance(result.left, _Literal)


def test_col_ror_arm_lit_left() -> None:
    """``False | bv.col('flag')`` triggers ``__ror__`` → ``or`` keyword."""
    result = False | bv.col("flag")
    assert isinstance(result, _BinOp)
    assert result.op == "or"
    assert result.to_expr_string() == "(false or flag)"
    assert isinstance(result.left, _Literal)


def test_col_or_arm_col_left() -> None:
    """``bv.col('a') | bv.col('b')`` triggers the forward ``__or__`` arm —
    serializes to the ``or`` keyword to match the boolean-combinator
    contract."""
    result = bv.col("a") | bv.col("b")
    assert isinstance(result, _BinOp)
    assert result.op == "or"
    assert result.to_expr_string() == "(a or b)"


# ---------------------------------------------------------------------------
# Unary ops
# ---------------------------------------------------------------------------


def test_col_invert_emits_not_keyword() -> None:
    """``~bv.col('x')`` must emit ``(not x)`` (server grammar — bare ``!`` is
    rejected by the where-parser)."""
    expr = ~bv.col("x")
    assert isinstance(expr, _UnaryOp)
    assert expr.op == "~"
    assert expr.to_expr_string() == "(not x)"


def test_col_isnull_emits_eq_null() -> None:
    """``bv.col('x').isnull()`` must emit ``(x == null)``."""
    expr = bv.col("x").isnull()
    assert isinstance(expr, _UnaryOp)
    assert expr.op == "isnull"
    assert expr.to_expr_string() == "(x == null)"


def test_unary_op_unknown_tag_raises_value_error() -> None:
    """Constructing a ``_UnaryOp`` with an unrecognized tag must raise on
    ``to_expr_string`` so accidental future additions can't silently
    serialize to garbage. The match-arms in ``_UnaryOp.to_expr_string``
    cover ``isnull`` / ``~`` only; everything else is a ValueError."""
    bogus = _UnaryOp("bogus_op", bv.col("x"))
    with pytest.raises(ValueError, match="unknown unary op"):
        bogus.to_expr_string()


# ---------------------------------------------------------------------------
# Cast op — happy path + invalid-target error
# ---------------------------------------------------------------------------


def test_col_cast_happy_path() -> None:
    """``bv.col('x').cast('float')`` → ``cast(x, float)``."""
    expr = bv.col("amount").cast("float")
    assert isinstance(expr, _CastOp)
    assert expr.target == "float"
    assert expr.to_expr_string() == "cast(amount, float)"


@pytest.mark.parametrize("target", ["str", "int", "float", "bool"])
def test_col_cast_accepts_all_valid_targets(target: str) -> None:
    """All four valid cast targets must compile and serialize."""
    expr = bv.col("x").cast(target)
    assert isinstance(expr, _CastOp)
    assert expr.target == target
    assert expr.to_expr_string() == f"cast(x, {target})"


def test_col_cast_rejects_invalid_target() -> None:
    """Any cast target not in the four allowed types raises ``ValueError``."""
    with pytest.raises(ValueError, match="cast target must be one of"):
        bv.col("x").cast("decimal")


# ---------------------------------------------------------------------------
# `_Literal.to_expr_string` — None / bool branches
# ---------------------------------------------------------------------------


def test_literal_none_serializes_to_null() -> None:
    """``bv.lit(None).to_expr_string() == 'null'`` per docstring example."""
    assert bv.lit(None).to_expr_string() == "null"


@pytest.mark.parametrize("value,want", [(True, "true"), (False, "false")])
def test_literal_bool_serializes_to_lowercase(value: bool, want: str) -> None:
    """``bv.lit(True/False)`` must emit lowercase keyword (server grammar)."""
    assert bv.lit(value).to_expr_string() == want


# ---------------------------------------------------------------------------
# `_Expr.to_expr_string` — NotImplementedError on abstract base
# ---------------------------------------------------------------------------


def test_expr_base_to_expr_string_not_implemented() -> None:
    """``_Expr`` itself is an abstract node — calling ``to_expr_string`` on
    a raw instance must raise ``NotImplementedError`` so misuse fails loud."""
    with pytest.raises(NotImplementedError):
        _Expr().to_expr_string()


# ---------------------------------------------------------------------------
# Hashability — every concrete node must be hashable (set/dict-key safe)
# ---------------------------------------------------------------------------


def test_col_is_hashable() -> None:
    """``_Col`` restores ``__hash__`` after overriding ``__eq__``."""
    c = bv.col("x")
    assert {c}  # constructing a set forces __hash__
    assert hash(c) == hash(bv.col("x"))  # value-equal cols hash the same


def test_literal_is_hashable() -> None:
    """``_Literal`` restores ``__hash__`` after overriding ``__eq__``."""
    lit = bv.lit(42)
    assert {lit}
    assert hash(lit) == hash(bv.lit(42))


def test_binop_is_hashable() -> None:
    """``_BinOp`` restores ``__hash__`` after overriding ``__eq__``."""
    expr = bv.col("a") + 1
    assert {expr}
    hash(expr)  # must not raise


def test_unary_op_is_hashable() -> None:
    """``_UnaryOp`` restores ``__hash__`` after overriding ``__eq__``."""
    expr = ~bv.col("a")
    assert {expr}
    hash(expr)


def test_cast_op_is_hashable() -> None:
    """``_CastOp`` restores ``__hash__`` after overriding ``__eq__``."""
    expr = bv.col("a").cast("int")
    assert {expr}
    hash(expr)
