"""``bv.col`` / ``bv.lit`` expression DSL.

Operator-overloaded AST. ``_Col`` / ``_Literal`` / ``_BinOp`` / ``_UnaryOp``
/ ``_CastOp`` are the node types; the public surface is :func:`col` and
:func:`lit`. ``_coerce`` wraps a Python literal that appears inside an
operator overload into a ``_Literal``, so::

    bv.col("amount") > 100
    bv.col("amount") > bv.lit(100)

are interchangeable (the explicit literal helper coexists with implicit
literal coercion).

The AST is inert at construction-time — all evaluation happens server-side.
``to_expr_string()`` renders to the wire-JSON expression string consumed
by the server's expression parser.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Union

# Allowed cast targets — symmetric with docs/sdk-api/python.md § Expression DSL.
_VALID_CAST_TARGETS = ("str", "int", "float", "bool")


class _Expr:
    """Base AST node. Operator overloads produce new ``_Expr`` nodes.

    Subclasses are frozen dataclasses, so the AST is immutable —
    re-using the same ``_Expr`` instance in multiple chains is safe.
    """

    def __add__(self, other: Any) -> "_Expr":
        return _BinOp("+", self, _coerce(other))

    def __radd__(self, other: Any) -> "_Expr":
        return _BinOp("+", _coerce(other), self)

    def __sub__(self, other: Any) -> "_Expr":
        return _BinOp("-", self, _coerce(other))

    def __rsub__(self, other: Any) -> "_Expr":
        return _BinOp("-", _coerce(other), self)

    def __mul__(self, other: Any) -> "_Expr":
        return _BinOp("*", self, _coerce(other))

    def __rmul__(self, other: Any) -> "_Expr":
        return _BinOp("*", _coerce(other), self)

    def __truediv__(self, other: Any) -> "_Expr":
        return _BinOp("/", self, _coerce(other))

    def __rtruediv__(self, other: Any) -> "_Expr":
        return _BinOp("/", _coerce(other), self)

    def __gt__(self, other: Any) -> "_Expr":
        return _BinOp(">", self, _coerce(other))

    def __ge__(self, other: Any) -> "_Expr":
        return _BinOp(">=", self, _coerce(other))

    def __lt__(self, other: Any) -> "_Expr":
        return _BinOp("<", self, _coerce(other))

    def __le__(self, other: Any) -> "_Expr":
        return _BinOp("<=", self, _coerce(other))

    def __eq__(self, other: Any) -> "_Expr":  # type: ignore[override]
        return _BinOp("==", self, _coerce(other))

    def __ne__(self, other: Any) -> "_Expr":  # type: ignore[override]
        return _BinOp("!=", self, _coerce(other))

    # Python forbids overloading `and` / `or`, so the SDK overloads `&` / `|`
    # as boolean combinators and serializes them as the server-grammar
    # keyword tokens `and` / `or` (rather than the bitwise `&` / `|` they
    # would normally emit). Surface ergonomics > bitwise fidelity here.
    def __and__(self, other: Any) -> "_Expr":
        return _BinOp("and", self, _coerce(other))

    def __rand__(self, other: Any) -> "_Expr":
        return _BinOp("and", _coerce(other), self)

    def __or__(self, other: Any) -> "_Expr":
        return _BinOp("or", self, _coerce(other))

    def __ror__(self, other: Any) -> "_Expr":
        return _BinOp("or", _coerce(other), self)

    def __invert__(self) -> "_Expr":
        return _UnaryOp("~", self)

    # `__eq__` is overridden to produce an `_Expr` (not a `bool`), which
    # makes instances unhashable by default. Each concrete subclass restores
    # `__hash__` explicitly so AST nodes can live in sets/dict keys.

    # Footgun guards. Python calls `__bool__` for `if`, `and`, `or`, `not`,
    # and ternary `a if c else b`. Without this guard a beava expression is
    # truthy by default, so `"yes" if (col > 0) else "no"` silently picks
    # `"yes"` — a silent-first-branch bug. PR 5's `@bv.expr` rewrites these
    # constructs at the source level *before* Python runs them, so the
    # guard never fires inside `@bv.expr`. The asymmetry is the whole point.
    def __bool__(self) -> bool:
        raise TypeError(
            "expression objects don't have a truth value — use "
            "`bv.if_else(cond, then, else)` for conditionals, or "
            "`&` / `|` to combine predicates"
        )

    def __iter__(self) -> Any:
        raise TypeError("expression objects are not iterable")

    def __len__(self) -> int:
        raise TypeError(
            "expression objects don't have a length — use "
            "`bv.length(x)` to take a string's length as a feature"
        )

    def isnull(self) -> "_Expr":
        return _UnaryOp("isnull", self)

    def cast(self, target: str) -> "_Expr":
        if target not in _VALID_CAST_TARGETS:
            raise ValueError(
                f"cast target must be one of {_VALID_CAST_TARGETS}; got {target!r}"
            )
        return _CastOp(self, target)

    def lower(self) -> "_Expr":
        return _Call("lower", (self,))

    def length(self) -> "_Expr":
        return _Call("length", (self,))

    def contains(self, s: Any) -> "_Expr":
        return _Call("contains", (self, _coerce(s)))

    def starts_with(self, s: Any) -> "_Expr":
        return _Call("starts_with", (self, _coerce(s)))

    def ends_with(self, s: Any) -> "_Expr":
        return _Call("ends_with", (self, _coerce(s)))

    def replace(self, old: Any, new: Any) -> "_Expr":
        return _Call("replace", (self, _coerce(old), _coerce(new)))

    def to_expr_string(self) -> str:
        """Render this AST node to wire JSON expression-string form."""
        raise NotImplementedError


@dataclass(frozen=True, eq=False)
class _Col(_Expr):
    name: str

    def to_expr_string(self) -> str:
        return self.name

    def __hash__(self) -> int:
        return hash(("_Col", self.name))


@dataclass(frozen=True, eq=False)
class _Literal(_Expr):
    value: Any

    def to_expr_string(self) -> str:
        if self.value is None:
            return "null"
        if isinstance(self.value, bool):
            return "true" if self.value else "false"
        if isinstance(self.value, str):
            # Single-quoted strings are required by the expression grammar.
            return repr(self.value)
        return repr(self.value)

    def __hash__(self) -> int:
        return hash(("_Literal", repr(self.value)))


@dataclass(frozen=True, eq=False)
class _BinOp(_Expr):
    op: str
    left: _Expr
    right: _Expr

    def to_expr_string(self) -> str:
        return f"({self.left.to_expr_string()} {self.op} {self.right.to_expr_string()})"

    def __hash__(self) -> int:
        return hash(("_BinOp", self.op, id(self.left), id(self.right)))


@dataclass(frozen=True, eq=False)
class _UnaryOp(_Expr):
    op: str
    operand: _Expr

    def to_expr_string(self) -> str:
        if self.op == "isnull":
            return f"({self.operand.to_expr_string()} == null)"
        if self.op == "~":
            # Server's where-parser (crates/beava-core/src/expr.rs:361-373 +
            # the `"not" => TokenKind::Not` keyword table) accepts `!=` for
            # not-equal and `not` (keyword) for logical negation, but
            # rejects bare unary `!` with `unexpected character '!'`. Emit
            # `(not …)` so SDK-built where predicates round-trip cleanly.
            return f"(not {self.operand.to_expr_string()})"
        raise ValueError(f"unknown unary op: {self.op!r}")

    def __hash__(self) -> int:
        return hash(("_UnaryOp", self.op, id(self.operand)))


@dataclass(frozen=True, eq=False)
class _CastOp(_Expr):
    operand: _Expr
    target: str

    def to_expr_string(self) -> str:
        return f"cast({self.operand.to_expr_string()}, {self.target})"

    def __hash__(self) -> int:
        return hash(("_CastOp", self.target, id(self.operand)))


@dataclass(frozen=True, eq=False)
class _Call(_Expr):
    """Generic function-call node. Renders ``name(a, b, ...)``.

    Every builtin sugar added in PR 3+ returns one of these — e.g.
    ``bv.log1p(x)`` is ``_Call("log1p", (x,))`` and ``bv.if_else(c, a, b)``
    is ``_Call("if_else", (c, a, b))``. ``_CastOp`` stays a special case
    because cast's second argument is a type tag, not a value.
    """

    name: str
    args: tuple[_Expr, ...]

    def to_expr_string(self) -> str:
        rendered = ", ".join(a.to_expr_string() for a in self.args)
        return f"{self.name}({rendered})"

    def __hash__(self) -> int:
        return hash(("_Call", self.name, tuple(id(a) for a in self.args)))


def _coerce(v: Any) -> _Expr:
    """Wrap a Python literal as a ``_Literal``; pass-through ``_Expr``."""
    return v if isinstance(v, _Expr) else _Literal(v)


def col(name: str) -> _Col:
    """Reference a schema field by name.

    Examples
    --------
    >>> import beava as bv
    >>> e = bv.col("amount") > 100
    >>> e.to_expr_string()
    '(amount > 100)'
    """
    return _Col(name)


def log1p(x: Any) -> _Expr:
    """Natural log of (x + 1). Numerically stable near zero. Returns F64."""
    return _Call("log1p", (_coerce(x),))


def clip(x: Any, lo: Any, hi: Any) -> _Expr:
    """Clamp x to the closed interval [lo, hi]. Preserves I64 vs F64."""
    return _Call("clip", (_coerce(x), _coerce(lo), _coerce(hi)))


def hour_of_day(dt: Any) -> _Expr:
    """Extract the UTC hour (0–23) from a datetime value."""
    return _Call("hour_of_day", (_coerce(dt),))


def hash_mod(x: Any, m: int) -> _Expr:
    """Hash x deterministically, then return the result modulo m.

    m must be a positive integer. Used to bucket high-cardinality fields
    into a fixed number of slots (e.g. ``hash_mod(user_id, 2)`` for A/B).
    """
    if not isinstance(m, int):
        raise TypeError(
            f"hash_mod: m must be a Python int (bucket count), got {type(m).__name__!r}"
        )
    return _Call("hash_mod", (_coerce(x), _coerce(m)))


def length(x: Any) -> _Expr:
    """Number of Unicode codepoints in string x. Matches Python's ``len()``."""
    return _Call("length", (_coerce(x),))


def if_else(cond: Any, then_: Any, else_: Any) -> _Expr:
    """Return ``then_`` when ``cond`` is true, otherwise ``else_``.

    Only the selected branch is evaluated server-side (short-circuit), so
    ``bv.if_else(denom != 0, num / denom, 0.0)`` is safe even when ``denom``
    is 0 — the division never runs. ``cond`` must be a boolean expression and
    the two branches must share a type at register time.
    """
    return _Call("if_else", (_coerce(cond), _coerce(then_), _coerce(else_)))


def lit(value: Union[int, float, str, bool, None]) -> _Literal:  # noqa: UP007
    """Construct an explicit literal expression.

    The implicit form ``bv.col("a") > 100`` and the explicit form
    ``bv.col("a") > bv.lit(100)`` are wire-equivalent. ``bv.lit`` is
    primarily useful when the literal stands on its own (constant column,
    numerator of a division, etc.) where Python's operator dispatch alone
    would not coerce.

    Examples
    --------
    >>> import beava as bv
    >>> bv.lit(42).value
    42
    >>> bv.lit("web").to_expr_string()
    "'web'"
    >>> bv.lit(None).to_expr_string()
    'null'
    """
    return _Literal(value)
