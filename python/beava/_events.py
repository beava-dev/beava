"""``@bv.event`` decorator and chain-building classes.

``@bv.event`` has two forms:

- **Class form:** declares an event source with a typed schema. Each
  annotated field becomes a schema column. Per-source kwargs
  (``keep_events_for``, ``cold_after``, ``dedupe_key``, ``dedupe_window``)
  are accepted via the decorator-factory shape ``@bv.event(...)``.

- **Function form:** declares an event derivation — a pipeline that chains
  on one or more upstream event sources. The function body builds a chain
  via the ``filter / select / drop / rename / with_columns / cast / fillna
  / group_by / agg`` chain methods.

``events.group_by()`` (no args) returns a *global* :class:`GroupBy` whose
subsequent ``.agg(...)`` emits a chain step with ``keys=[]``;
``events.agg(**aggs)`` is a direct shorthand for
``events.group_by().agg(**aggs)``.

Per the locked ``project_redis_shaped_no_event_time_ever`` commitment, v0
is processing-time only: the decorator rejects ``event_time`` schema fields
and the ``tolerate_delay`` / ``event_time_field`` decorator kwargs with
``TypeError`` at decorator time. Event-time semantics are roadmapped for
post-v0.
"""
from __future__ import annotations

import inspect
from typing import Any, Callable, get_type_hints

from beava._col import _Expr

_FORBIDDEN_FIELD_NAMES = ("event_time",)
_FORBIDDEN_DECORATOR_KWARGS = ("tolerate_delay", "event_time_field")
_VALID_CAST_TARGETS = ("str", "int", "float", "bool")


class _ChainMixin:
    """Common chain methods for ``EventSource`` and ``EventDerivation``."""

    _chain: list[dict[str, Any]]
    _name: str
    _schema: dict[str, type] | None

    def filter(self, expr: _Expr) -> "EventDerivation":
        return _make_derivation(self, {"op": "filter", "expr": expr.to_expr_string()})

    def select(self, *cols: str) -> "EventDerivation":
        # Server-side OpNode::Select / OpNode::Drop deserialise from
        # `"fields"` (crates/beava-core/src/op_node.rs:43,46); emitting
        # `"cols"` caused the register call to fail with
        # `invalid_registration: missing field 'fields'`. Tests survived
        # because no SDK test rode the chain into a real server.
        return _make_derivation(self, {"op": "select", "fields": list(cols)})

    def drop(self, *cols: str) -> "EventDerivation":
        return _make_derivation(self, {"op": "drop", "fields": list(cols)})

    def rename(self, **mapping: str) -> "EventDerivation":
        return _make_derivation(
            self, {"op": "rename", "mapping": dict(mapping)}
        )

    def with_columns(self, **exprs: Any) -> "EventDerivation":
        return _make_derivation(
            self,
            {
                "op": "with_columns",
                "exprs": {
                    k: (v.to_expr_string() if isinstance(v, _Expr) else v)
                    for k, v in exprs.items()
                },
            },
        )

    def map(self, **exprs: Any) -> "EventDerivation":
        return self.with_columns(**exprs)

    def cast(self, **type_map: str) -> "EventDerivation":
        for k, v in type_map.items():
            if v not in _VALID_CAST_TARGETS:
                raise ValueError(
                    f"cast target {v!r} for {k!r} not in {_VALID_CAST_TARGETS}"
                )
        return _make_derivation(
            self, {"op": "cast", "type_map": dict(type_map)}
        )

    def fillna(self, **defaults: Any) -> "EventDerivation":
        return _make_derivation(
            self, {"op": "fillna", "defaults": dict(defaults)}
        )

    def group_by(self, *keys: str) -> "GroupBy":
        # Empty *keys is allowed and produces a global GroupBy.
        return GroupBy(parent=self, keys=tuple(keys))

    def agg(self, **named: Any) -> "EventDerivation":
        """Shorthand for ``events.group_by().agg(...)`` (global aggregation)."""
        return self.group_by().agg(**named)

    def named(self, name: str) -> "EventDerivation":
        """Tag a derivation with a stable, registration-visible name.

        Implemented as a chain-noop ``rename_self`` step so the chain
        survives serialization unchanged while still exposing a
        deterministic ``_name`` for downstream registration.
        """
        d = _make_derivation(self, {"op": "rename_self", "name": name})
        d._name = name
        return d


class EventSource(_ChainMixin):
    """An event source declared via ``@bv.event class Foo: ...``."""

    def __init__(
        self, name: str, schema: dict[str, type], **kwargs: Any
    ) -> None:
        self._name = name
        self._schema = schema
        self._chain: list[dict[str, Any]] = []
        self._keep_events_for = kwargs.get("keep_events_for")
        self._cold_after = kwargs.get("cold_after")
        self._dedupe_key = kwargs.get("dedupe_key")
        self._dedupe_window = kwargs.get("dedupe_window")
        self._kind = "event_source"


class EventDerivation(_ChainMixin):
    """An event derivation chained on top of an upstream source/derivation."""

    def __init__(
        self,
        name: str,
        parent: Any,
        chain: list[dict[str, Any]],
    ) -> None:
        self._name = name
        self._parent = parent
        self._chain = chain
        self._schema: dict[str, type] | None = None  # propagated at register-time
        self._kind = "event_derivation"
        self._key_cols: list[str] | None = None
        # Marker set by ``_make_event_derivation`` — True iff this
        # EventDerivation is the output of an ``@bv.event def F(...)``
        # decoration. Raw chain expressions (e.g.
        # ``Click.with_columns(...).named("X")``) keep it False so
        # ``App.register`` and ``_resolve_upstream_proxies`` can reject
        # them with a sharp error pointing at the canonical rewrite.
        self._is_bv_event_function: bool = False


def _make_derivation(parent: Any, step: dict[str, Any]) -> EventDerivation:
    new_chain = list(parent._chain) + [step]
    name = f"{parent._name}__derived_{len(new_chain)}"
    return EventDerivation(name=name, parent=parent, chain=new_chain)


class GroupBy:
    """Intermediate group-by object — call ``.agg(**aggs)`` to materialize."""

    def __init__(self, parent: Any, keys: tuple[str, ...]) -> None:
        self._parent = parent
        self._keys = keys

    def agg(self, **named: Any) -> EventDerivation:
        """Apply named aggregations.

        Each value is either an :class:`AggDescriptor` (exposing
        ``to_dict()``) or a primitive that is serialized directly.
        """
        new_chain = list(self._parent._chain) + [
            {
                "op": "agg",
                "keys": list(self._keys),
                "aggs": {
                    name: (agg.to_dict() if hasattr(agg, "to_dict") else agg)
                    for name, agg in named.items()
                },
            }
        ]
        d = EventDerivation(
            name=f"{self._parent._name}__agg",
            parent=self._parent,
            chain=new_chain,
        )
        d._kind = "aggregation"
        d._key_cols = list(self._keys)
        return d


def _validate_class_event(cls: type, kwargs: dict[str, Any]) -> None:
    # Per `project_redis_shaped_no_event_time_ever` (locked 2026-04-30), v0
    # is processing-time only — the server stamps wall-clock time on every
    # push. Reject the event-time-shaped surface at decorator time so users
    # never silently accumulate event-time semantics they can't actually use.
    for forbidden_kw in _FORBIDDEN_DECORATOR_KWARGS:
        if forbidden_kw in kwargs:
            raise TypeError(
                f"@bv.event {forbidden_kw!r} kwarg is not supported in v0; "
                f"the server stamps wall-clock processing time on every push."
            )
    hints = get_type_hints(cls, include_extras=True)
    for forbidden_field in _FORBIDDEN_FIELD_NAMES:
        if forbidden_field in hints:
            raise TypeError(
                f"@bv.event class field {forbidden_field!r} is not supported in v0; "
                f"the server stamps wall-clock processing time on every push."
            )


def _make_event_source(cls: type, kwargs: dict[str, Any]) -> type:
    _validate_class_event(cls, kwargs)
    hints = get_type_hints(cls, include_extras=True)
    src = EventSource(name=cls.__name__, schema=hints, **kwargs)
    # Mirror EventSource state onto the class so users can access it as
    # static attributes (`MyEvent._schema`, `MyEvent._chain`, ...).
    for attr in (
        "_name",
        "_schema",
        "_chain",
        "_keep_events_for",
        "_cold_after",
        "_dedupe_key",
        "_dedupe_window",
        "_kind",
    ):
        setattr(cls, attr, getattr(src, attr))
    # Attach the chain methods as staticmethods so user code can call
    # `Cls.filter(...)` directly without instantiating the EventSource.
    for method_name in (
        "filter",
        "select",
        "drop",
        "rename",
        "with_columns",
        "map",
        "cast",
        "fillna",
        "group_by",
        "agg",
        "named",
    ):
        bound = getattr(src, method_name)
        setattr(cls, method_name, staticmethod(bound))
    return cls


_EVENTS_MODULE_FILE = __file__


def _collect_closure_cells_for_events(fn: Callable[..., Any]) -> dict[str, Any]:
    """Return a ``{name: value}`` map of ``fn``'s lexical closure cells.

    Mirror of :func:`_table._collect_closure_cells` for resolution parity
    with ``@bv.table`` — see that module for the rationale.
    """
    cells: dict[str, Any] = {}
    code = getattr(fn, "__code__", None)
    closure = getattr(fn, "__closure__", None)
    if code is None or closure is None:
        return cells
    freevars = getattr(code, "co_freevars", ())
    for name, cell in zip(freevars, closure, strict=False):
        try:
            cells[name] = cell.cell_contents
        except ValueError:
            continue
    return cells


def _collect_caller_frame_locals_for_events() -> dict[str, Any]:
    """Return merged ``f_locals`` from user-code frames above ``_events.py``.

    Mirror of :func:`_table._collect_caller_frame_locals` — boundary
    detection is by file identity (``_EVENTS_MODULE_FILE``) so the resolver
    stays robust under decorator-stack wrappers. Captures the
    function-local ``@bv.event class Foo: ...`` pattern (e.g. inside a
    pytest test function body).
    """
    merged: dict[str, Any] = {}
    frame = inspect.currentframe()
    try:
        if frame is None:
            return merged
        frame = frame.f_back  # skip this helper
        depth = 0
        while frame is not None and depth < 32:
            code = frame.f_code
            if getattr(code, "co_filename", None) != _EVENTS_MODULE_FILE:
                for name, val in frame.f_locals.items():
                    if name not in merged:
                        merged[name] = val
            frame = frame.f_back
            depth += 1
        return merged
    finally:
        del frame


def _is_event_upstream(ann: Any) -> bool:
    """Return True if ``ann`` looks like a Beava event-source / event-derivation.

    Two shapes count:
      * ``@bv.event class Foo: ...`` — :func:`_make_event_source` mirrors
        ``_kind = "event_source"`` onto the class itself.
      * ``@bv.event def Bar(...): ...`` — :class:`EventDerivation` instance
        with ``_is_bv_event_function = True``.
    """
    if isinstance(ann, EventDerivation) and getattr(
        ann, "_is_bv_event_function", False
    ):
        return True
    if getattr(ann, "_kind", None) == "event_source":
        return True
    return False


def _make_event_derivation(fn: Callable[..., Any]) -> EventDerivation:
    sig = inspect.signature(fn)
    params = list(sig.parameters.values())
    if not params:
        raise TypeError(
            f"@bv.event function {fn.__name__!r} must take at least one parameter"
        )
    # Mirror `_table._resolve_upstream_proxies` so function-local
    # @bv.event class definitions (e.g. inside a pytest test fn) resolve
    # correctly. Under `from __future__ import annotations`, `def
    # Tagged(click: Click)` where `Click` is function-local would otherwise
    # leave the annotation as the string `'Click'` and cause an
    # AttributeError when invoked. Resolution sources are checked in this
    # priority order: globals, then closure cells, then caller-frame locals.
    closure_cells = _collect_closure_cells_for_events(fn)
    caller_locals = _collect_caller_frame_locals_for_events()
    localns: dict[str, Any] = dict(caller_locals)
    localns.update(closure_cells)  # closure cells outrank caller-frame locals
    try:
        resolved = get_type_hints(fn, globalns=fn.__globals__, localns=localns)
    except Exception:
        resolved = {}
    upstream_proxies: list[Any] = []
    resolved_anns: list[tuple[str, Any]] = []
    for p in params:
        ann = resolved.get(p.name, p.annotation)
        if isinstance(ann, str):
            ann = fn.__globals__.get(
                ann,
                closure_cells.get(ann, caller_locals.get(ann, ann)),
            )
        # Reject raw EventDerivation instances. Legitimate @bv.event def
        # outputs carry the `_is_bv_event_function` marker set at the
        # bottom of this function; raw chain expressions don't.
        if isinstance(ann, EventDerivation) and not getattr(
            ann, "_is_bv_event_function", False
        ):
            raise TypeError(
                f"@bv.event function {fn.__name__!r} parameter {p.name!r} "
                f"annotation resolves to an EventDerivation instance (a raw "
                f"chain). Annotate with an @bv.event-decorated class or function "
                f"instead — e.g.\n"
                f"    @bv.event\n"
                f"    def Tagged(click: Click): return click.with_columns(...)\n"
                f"    @bv.event\n"
                f"    def {fn.__name__}({p.name}: Tagged): return {p.name}.<chain>"
            )
        upstream_proxies.append(ann)
        resolved_anns.append((p.name, ann))
    # Reject multi-upstream derivations at decoration time. The engine
    # only supports a single-upstream chain in v0 — `_to_register_json`
    # walks `_parent` up to ONE EventSource and silently drops any second
    # upstream, surfacing later as the unhelpful server-side error
    # `invalid_registration: missing field 'fields'`. Sharp TypeError here
    # tells the user what's actually unsupported. Mirrors the
    # raw-chain-as-annotation rejection above.
    event_params = [
        name for name, ann in resolved_anns if _is_event_upstream(ann)
    ]
    if len(event_params) > 1:
        raise TypeError(
            f"@bv.event def {fn.__name__}(...) accepts multiple upstream "
            f"event parameters ({event_params}), but multi-upstream "
            f"derivations are not supported in v0. Either:\n"
            f"  1. Pick a single upstream and combine state at the table layer.\n"
            f"  2. File a feature request if you need this pattern."
        )
    result = fn(*upstream_proxies)
    if not isinstance(result, EventDerivation):
        raise TypeError(
            f"@bv.event function {fn.__name__!r} must return a chain "
            f"(filter/select/group_by/agg/...); got {type(result).__name__}"
        )
    result._name = fn.__name__
    # Tag this EventDerivation as a legitimate @bv.event def output so
    # `App.register` and downstream decorator resolvers accept it; raw
    # chain expressions (e.g. `Click.with_columns(...).named("X")`) lack
    # this marker and are rejected with a sharp error.
    result._is_bv_event_function = True
    return result


def event(cls_or_fn: Any = None, /, **kwargs: Any) -> Any:
    """``@bv.event`` decorator — class form OR function form.

    Class form::

        @bv.event
        class Click:
            user_id: str
            page: str

    Function form::

        @bv.event
        def BigClick(click: Click):
            return click.filter(bv.col("page") == "/checkout")

    Decorator-factory form (per-source kwargs)::

        @bv.event(keep_events_for="30d", dedupe_key="id", dedupe_window="5m")
        class Login:
            id: str

    Forbidden kwargs (raise ``TypeError`` at decorator-time):
        - ``event_time_field`` / ``tolerate_delay`` (no event-time in v0)

    Forbidden class fields (raise ``TypeError``): ``event_time``.
    """
    if cls_or_fn is None:
        # Decorator factory: @bv.event(cold_after="1d") class Foo: ...
        def _wrap(target: Any) -> Any:
            if inspect.isclass(target):
                return _make_event_source(target, kwargs)
            return _make_event_derivation(target)

        return _wrap
    if inspect.isclass(cls_or_fn):
        return _make_event_source(cls_or_fn, {})
    return _make_event_derivation(cls_or_fn)
