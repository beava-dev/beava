"""``bv.App`` — synchronous client.

Implements the seven wire-mapped lifecycle methods (``register`` / ``push``
/ ``get`` / ``batch_get`` / ``reset`` / ``ping`` / ``close``). Transport
is selected from the URL scheme:

  ``http(s)://`` → :class:`HttpTransport`
  ``tcp://``     → :class:`TcpTransport`
  ``None``       → :class:`EmbedTransport` (spawns the local binary)

The ``test_mode=True`` kwarg propagates ``BEAVA_TEST_MODE=1`` to the spawned
binary in embed mode (gating ``OP_RESET`` and other test-only opcodes). In
network mode the kwarg is ignored with a ``UserWarning`` — server-side
configuration controls test mode for shared servers.
"""
from __future__ import annotations

import json
import warnings
from typing import Any
from urllib.parse import urlparse

from beava._transport import Transport, make_transport


def _to_register_json(
    descriptors: tuple[Any, ...],
    *,
    force: bool = False,
    dry_run: bool = False,
) -> bytes:
    """Convert ``@bv.event`` / ``@bv.table`` descriptors into a UTF-8 JSON
    payload matching the wire-spec ``{"nodes": [...]}`` shape.

    Module-level (not a method) so tests can introspect the wire payload
    independently of an actual transport.
    """
    nodes: list[dict[str, Any]] = []
    for d in descriptors:
        node = _descriptor_to_node(d)
        if node is None:
            continue
        nodes.append(node)
    payload: dict[str, Any] = {"nodes": nodes}
    if force:
        payload["force"] = True
    if dry_run:
        payload["dry_run"] = True
    return json.dumps(payload, ensure_ascii=False).encode("utf-8")


def _descriptor_to_node(d: Any) -> dict[str, Any] | None:
    """Best-effort serialization of a single descriptor to wire-shape JSON.

    Recognised inputs:
      - ``EventSource`` (class with ``_kind == "event_source"``)
      - ``EventDerivation``
      - ``TableDescriptor``
      - Plain dict (passthrough — for tests that hand-build payloads)
    """
    if isinstance(d, dict):
        return d
    kind = getattr(d, "_kind", None)
    if kind in ("event_source", None) and hasattr(d, "_schema"):
        name = getattr(d, "_name", getattr(d, "__name__", None))
        if not name:
            return None
        schema_dict = getattr(d, "_schema", {}) or {}
        fields_obj: dict[str, str] = {}
        for fname, ftype in schema_dict.items():
            fields_obj[fname] = _python_type_to_wire(ftype)
        return {
            "kind": "event",
            "name": name,
            "schema": {"fields": fields_obj, "optional_fields": []},
        }
    if kind == "table":
        chain = getattr(d, "_chain", []) or []
        key_cols = getattr(d, "_key_cols", []) or []
        parent = getattr(d, "_parent", None)
        # SDK chain-flatten: when an @bv.table's parent chain contains an
        # @bv.event def output (marker `_is_bv_event_function=True`), the
        # upstream must be the ROOT EventSource. The server's apply-time
        # routing index keys aggregations by their direct upstream name —
        # events arriving at the root only trigger aggregations whose
        # declared upstream IS the root, not intermediate derivations. The
        # intermediate ops are already prepended into `chain` by
        # `_make_derivation`, so flattening upstream to the root produces a
        # wire payload that routes correctly without losing chain steps.
        has_bv_event_def_ancestor = False
        ancestor: Any = parent
        while ancestor is not None:
            if getattr(ancestor, "_is_bv_event_function", False):
                has_bv_event_def_ancestor = True
                break
            ancestor = getattr(ancestor, "_parent", None)
        if has_bv_event_def_ancestor and parent is not None:
            root: Any = parent
            while True:
                p = getattr(root, "_parent", None)
                if p is None:
                    break
                root = p
            upstreams = [getattr(root, "_name", "")]
        elif parent is not None:
            parent_kind = getattr(parent, "_kind", None)
            parent_name = getattr(parent, "_name", "")
            # Named derivations (via `.named(...)`) keep their stable name
            # as the upstream; auto-named intermediate steps
            # (`Foo__derived_N`) are not addressable on the wire and must
            # resolve to the root EventSource instead.
            if parent_kind == "event_source" or "__derived_" not in parent_name:
                upstreams = [parent_name] if parent_name else []
            else:
                root2: Any = parent
                while True:
                    p = getattr(root2, "_parent", None)
                    if p is None:
                        break
                    root2 = p
                upstreams = [getattr(root2, "_name", "")]
        else:
            upstreams = []
        ops = _chain_to_ops(chain)
        # The server's DerivationDescriptor deserializer requires a populated
        # `schema` field; compute it from key cols + chain agg outputs.
        parent_schema = _parent_wire_schema(d)
        fields = _infer_derivation_schema(chain, parent_schema, list(key_cols))
        return {
            "kind": "derivation",
            "name": getattr(d, "_name", ""),
            "output_kind": "table",
            "upstreams": upstreams,
            "ops": ops,
            "schema": {"fields": fields, "optional_fields": []},
            "table_primary_key": list(key_cols),
        }
    if kind in ("event_derivation", "aggregation"):
        chain = getattr(d, "_chain", []) or []
        # Walk back through the parent chain to find the root EventSource —
        # the only node with kind=event_source. Each EventDerivation's
        # _parent is the previous derivation in the chain.
        ev_root: Any = d
        while True:
            p = getattr(ev_root, "_parent", None)
            if p is None:
                break
            ev_root = p
        upstreams = (
            [getattr(ev_root, "_name", "")] if ev_root is not d else []
        )
        ops = _chain_to_ops(chain)
        # `output_kind = "table"` iff the chain terminates in an `agg` step
        # (event → table boundary); `"event"` for pure event-to-event
        # transforms (filter/select/with_columns/...).
        has_agg = any(step.get("op") == "agg" for step in chain)
        output_kind = "table" if has_agg else "event"
        # When the chain ends in `group_by(...).agg(...)`, the result is a
        # table, and every table needs to know which columns are its key (the
        # ones we grouped by). Grab those key columns here so they show up both
        # in the field list and in `table_primary_key`, which the server needs
        # for any table it stores.
        key_cols = getattr(d, "_key_cols", []) or []
        parent_schema = _parent_wire_schema_from_root(ev_root)
        if has_agg:
            fields = _infer_derivation_schema(chain, parent_schema, list(key_cols))
        else:
            fields = _infer_event_derivation_schema(chain, parent_schema)
        node: dict[str, Any] = {
            "kind": "derivation",
            "name": getattr(d, "_name", ""),
            "output_kind": output_kind,
            "upstreams": upstreams,
            "ops": ops,
            "schema": {"fields": fields, "optional_fields": []},
        }
        if output_kind == "table":
            node["table_primary_key"] = list(key_cols)
        return node
    return None


def _python_type_to_wire(t: Any) -> str:
    """Map Python type / annotation to wire-format type string."""
    if t in (str,) or getattr(t, "__name__", "") == "str":
        return "str"
    if t in (int,) or getattr(t, "__name__", "") == "int":
        return "i64"
    if t in (float,) or getattr(t, "__name__", "") == "float":
        return "f64"
    if t in (bool,) or getattr(t, "__name__", "") == "bool":
        return "bool"
    return "str"


# Agg-op output type mapping — mirrors Rust ``agg_op::output_type_for``.
# Used by ``_infer_derivation_schema`` to populate the ``schema.fields`` map
# of a derivation node so the server's ``DerivationDescriptor``
# deserializer (which requires ``schema``) accepts the payload. The
# Python-side inference is best-effort — the server is the authoritative
# source — but it must produce a non-empty schema dict to clear the
# deserializer's invariant.
_FIXED_OP_OUTPUT_TYPE: dict[str, str] = {
    "count": "i64",
    "sum": "f64",
    "mean": "f64",
    "avg": "f64",
    "var": "f64",
    "variance": "f64",
    "std": "f64",
    "stddev": "f64",
    "ratio": "f64",
    "n_unique": "i64",
    "count_distinct": "i64",
    "quantile": "f64",
    "percentile": "f64",
    "top_k": "str",
    "bloom_member": "bool",
    "entropy": "f64",
    "first_seen": "i64",
    "last_seen": "i64",
    "age": "i64",
    "time_since": "i64",
    "time_since_last_n": "i64",
    "has_seen": "bool",
    "first_seen_in_window": "bool",
    "streak": "i64",
    "max_streak": "i64",
    "negative_streak": "i64",
    "ewma": "f64",
    "ema": "f64",
    "ewvar": "f64",
    "ew_zscore": "f64",
    "decayed_sum": "f64",
    "decayed_count": "f64",
    "twa": "f64",
    "rate_of_change": "f64",
    "inter_arrival_stats": "f64",
    "trend": "f64",
    "trend_residual": "f64",
    "z_score": "f64",
    "burst_count": "i64",
    "outlier_count": "i64",
    "value_change_count": "i64",
    "first_n": "str",
    "last_n": "str",
    "histogram": "str",
    "hour_of_day_histogram": "str",
    "dow_hour_histogram": "str",
    "event_type_mix": "str",
    "most_recent_n": "str",
    "reservoir_sample": "str",
    "seasonal_deviation": "f64",
    "geo_velocity": "f64",
    "geo_distance": "f64",
    "geo_spread": "f64",
    "distance_from_home": "f64",
}

# Ops whose output type inherits from the named upstream field.
_FIELD_INHERITING_OPS: frozenset[str] = frozenset(
    {"min", "max", "first", "last", "lag", "delta_from_prev"}
)


def _agg_output_type(
    op_name: str, field_name: str | None, upstream_schema: dict[str, str]
) -> str:
    """Return the wire-type string for an agg op's output column.

    Mirrors the Rust authoritative mapping. Falls back to ``str`` for
    unknown ops; an incorrect inference surfaces server-side as
    ``invalid_registration`` with a useful message.
    """
    if op_name in _FIXED_OP_OUTPUT_TYPE:
        return _FIXED_OP_OUTPUT_TYPE[op_name]
    if op_name in _FIELD_INHERITING_OPS:
        if field_name is not None and field_name in upstream_schema:
            return upstream_schema[field_name]
        return "str"
    return "str"


def _infer_derivation_schema(
    chain: list[dict[str, Any]],
    parent_schema_wire: dict[str, str],
    key_cols: list[str],
) -> dict[str, str]:
    """Compute a derivation node's ``schema.fields`` map from its chain.

    Walks the chain looking for the (single) ``agg`` step (always present
    as the terminal step in a v0 ``@bv.table`` function). Combines key
    columns (typed via ``parent_schema_wire`` lookup; ``str`` fallback)
    with each agg's output column type via ``_agg_output_type``.

    Args:
        chain: The list of chain steps (raw, pre-``_chain_to_ops`` form).
        parent_schema_wire: Wire-format ``{field: type-str}`` map of the
            upstream event source's schema.
        key_cols: Table key columns (post-``@bv.table(key=...)``).

    Returns:
        ``{column-name: wire-type-str}`` ordered with key cols first, then
        agg output columns. Used as the ``fields`` value of the emitted
        ``schema`` JSON object.
    """
    fields: dict[str, str] = {}
    for k in key_cols:
        fields[k] = parent_schema_wire.get(k, "str")
    for step in chain:
        if step.get("op") != "agg":
            continue
        aggs = step.get("aggs", {})
        for out_name, spec in aggs.items():
            if isinstance(spec, dict):
                op_name = spec.get("op", "count")
                field_name = spec.get("field")
            else:
                op_name = "count"
                field_name = None
            fields[out_name] = _agg_output_type(
                op_name, field_name, parent_schema_wire
            )
    return fields


def _parent_wire_schema(d: Any) -> dict[str, str]:
    """Best-effort extraction of the upstream event source's wire schema.

    Returns ``{field-name: wire-type-str}`` derived from the parent's
    ``_schema`` attribute (a ``{name: python-type}`` map populated by
    ``@bv.event``). Empty dict if the parent or schema is missing.

    Walks the parent chain to find the root event source (the only one
    with ``_kind == "event_source"`` and a populated ``_schema`` map).
    """
    cur = getattr(d, "_parent", None)
    while cur is not None:
        kind = getattr(cur, "_kind", None)
        schema_dict = getattr(cur, "_schema", None)
        if kind == "event_source" and schema_dict:
            out: dict[str, str] = {}
            for fname, ftype in schema_dict.items():
                out[fname] = _python_type_to_wire(ftype)
            return out
        cur = getattr(cur, "_parent", None)
    return {}


def _parent_wire_schema_from_root(root: Any) -> dict[str, str]:
    """Variant of ``_parent_wire_schema`` that takes the root EventSource directly."""
    if root is None:
        return {}
    schema_dict = getattr(root, "_schema", {}) or {}
    out: dict[str, str] = {}
    for fname, ftype in schema_dict.items():
        out[fname] = _python_type_to_wire(ftype)
    return out


def _infer_event_derivation_schema(
    chain: list[dict[str, Any]],
    parent_schema_wire: dict[str, str],
) -> dict[str, str]:
    """Compute ``schema.fields`` for an event-derivation (no agg) chain.

    Starts from the upstream event-source's schema and applies
    ``with_columns`` / ``rename`` / ``drop`` / ``select`` / ``cast`` /
    ``fillna`` / ``filter`` / ``rename_self`` step semantics. Best-effort —
    the server's own schema propagation is authoritative; this just
    produces a schema dict with ``len > 0`` to clear the registration
    deserializer's invariant.
    """
    fields: dict[str, str] = dict(parent_schema_wire)
    for step in chain:
        op = step.get("op")
        if op == "with_columns" or op == "map":
            for col in step.get("exprs", {}):
                if col not in fields:
                    fields[col] = "str"
        elif op == "rename":
            mapping = step.get("mapping", {})
            for old, new in mapping.items():
                if old in fields:
                    fields[new] = fields.pop(old)
        elif op == "drop":
            for col in step.get("cols", []):
                fields.pop(col, None)
        elif op == "select":
            cols = step.get("cols", [])
            fields = {c: fields.get(c, "str") for c in cols}
        elif op == "cast":
            type_map = step.get("type_map", {})
            for col, ttype in type_map.items():
                if col in fields:
                    fields[col] = {
                        "str": "str",
                        "int": "i64",
                        "float": "f64",
                        "bool": "bool",
                    }.get(ttype, "str")
        elif op in ("fillna", "filter", "rename_self"):
            # No schema change.
            pass
    # Ensure at least one field so the deserializer accepts the payload.
    if not fields and parent_schema_wire:
        first_field = next(iter(parent_schema_wire))
        fields[first_field] = parent_schema_wire[first_field]
    return fields


def _chain_to_ops(chain: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Convert chain steps to wire-shape ops.

    The aggregation step is rewritten as ``{"op": "group_by", "keys": [...],
    "agg": {name: {"op": <op>, "params": {...}}}}``. The ``rename_self``
    step is a Python-side chain-noop introduced by ``.named(...)`` — it
    tags the derivation with a stable name but is not a server-recognized
    op variant, so it is stripped from the wire payload (the name is
    emitted via the node's ``name`` field instead).
    """
    out: list[dict[str, Any]] = []
    for step in chain:
        op = step.get("op")
        if op == "rename_self":
            continue
        if op == "agg":
            keys = step.get("keys", [])
            aggs = step.get("aggs", {})
            agg_obj: dict[str, Any] = {}
            for name, spec in aggs.items():
                if isinstance(spec, dict):
                    op_name = spec.get("op", "count")
                    params = {k: v for k, v in spec.items() if k != "op"}
                    agg_obj[name] = {"op": op_name, "params": params}
                else:
                    agg_obj[name] = {"op": "count", "params": {}}
            out.append({"op": "group_by", "keys": list(keys), "agg": agg_obj})
        else:
            out.append(dict(step))
    return out


class App:
    """Beava synchronous client — 7 wire-mapped methods + context manager.

    Embed mode (``url=None``) REQUIRES use as a context manager so the
    spawned binary is torn down on exit. Network mode may be used either
    way.

    Args:
        url: Server URL. ``http://...`` / ``https://...`` for HTTP transport,
            ``tcp://...`` for the custom-framed TCP fast-path. ``None``
            (default) for embed mode (local binary spawn).
        timeout: Socket / HTTP timeout in seconds. Defaults to 30.0.
        test_mode: When True + embed mode, sets ``BEAVA_TEST_MODE=1`` in the
            spawned binary's env so test-only opcodes (``OP_RESET``, …)
            are accepted. When True + network mode, emits a UserWarning
            and proceeds without effect.
    """

    def __init__(
        self,
        url: str | None = None,
        *,
        timeout: float = 30.0,
        test_mode: bool = False,
    ) -> None:
        self._url = url
        self._timeout = timeout
        self._test_mode = test_mode
        self._transport: Transport | None = None
        self._closed = False
        self._entered = False

        if url is None:
            self._transport_kind = "embed"
        else:
            scheme = urlparse(url).scheme
            if scheme in ("http", "https"):
                self._transport_kind = "http"
                if test_mode:
                    warnings.warn(
                        "test_mode kwarg ignored for network mode (url is set); "
                        "server controls test mode in network mode.",
                        UserWarning,
                        stacklevel=2,
                    )
            elif scheme == "tcp":
                self._transport_kind = "tcp"
                if test_mode:
                    warnings.warn(
                        "test_mode kwarg ignored for network mode (url is set); "
                        "server controls test mode in network mode.",
                        UserWarning,
                        stacklevel=2,
                    )
            else:
                raise ValueError(f"unsupported URL scheme: {scheme!r}")

    def __enter__(self) -> "App":
        self._entered = True
        if self._transport is None:
            self._transport = make_transport(
                url=self._url, timeout=self._timeout, test_mode=self._test_mode
            )
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:
            pass

    def close(self) -> None:
        if self._closed:
            return
        if self._transport is not None:
            try:
                self._transport.close()
            except Exception:
                pass
        self._closed = True

    def _require_transport(self) -> Transport:
        if self._closed:
            raise RuntimeError("App is closed")
        if self._transport_kind == "embed" and not self._entered:
            raise RuntimeError(
                "Embed mode requires use as a context manager: "
                "`with bv.App() as app:`"
            )
        if self._transport is None:
            self._transport = make_transport(
                url=self._url, timeout=self._timeout, test_mode=self._test_mode
            )
        return self._transport

    def register(
        self,
        *descriptors: Any,
        force: bool = False,
        dry_run: bool = False,
    ) -> dict[str, Any]:
        """Register one or more event/table descriptors with the server.

        Args:
            *descriptors: Descriptor objects produced by the ``@bv.event``
                or ``@bv.table`` decorators.
            force: If True, the server replaces any existing pipeline of
                the same name.
            dry_run: If True, the server validates and returns a categorized
                diff payload but does not commit.

        Raises:
            RegistrationError: A descriptor is an ``EventDerivation``
                instance (a raw chain expression). Chain expressions must
                be wrapped in ``@bv.event def F(...)`` before being
                registered — that wrapper is what carries the stable name
                the server's apply-time routing index keys on.
        """
        # Reject raw EventDerivation instances at register-time with a sharp
        # client-side error pointing at the canonical `@bv.event def`
        # rewrite. Local imports avoid a circular import at module load
        # (_app ← _events ← _col).
        from beava._errors import RegistrationError
        from beava._events import EventDerivation

        for i, d in enumerate(descriptors):
            if isinstance(d, EventDerivation) and not getattr(
                d, "_is_bv_event_function", False
            ):
                raise RegistrationError(
                    code="invalid_descriptor",
                    path=f"descriptors[{i}]",
                    message=(
                        f"argument {i} is an EventDerivation instance (a raw chain). "
                        f"Wrap the chain in @bv.event:\n"
                        f"    @bv.event\n"
                        f"    def Foo(click: Click):\n"
                        f"        return click.with_columns(...)"
                    ),
                    errors=[],
                )

        t = self._require_transport()
        payload = _to_register_json(descriptors, force=force, dry_run=dry_run)
        result: dict[str, Any] = t.send_register(payload)
        return result

    def push(self, event_name: str, fields: dict[str, Any]) -> dict[str, Any]:
        """Push a single event to the server.

        Args:
            event_name: Name of the registered event type.
            fields: Event fields as a plain Python dict.
        """
        t = self._require_transport()
        result: dict[str, Any] = t.send_push(event_name=event_name, fields=fields)
        return result

    def get(
        self,
        table: str,
        key: str | list[str | int | bool] | None = None,
        features: list[str] | None = None,
    ) -> dict[str, Any]:
        """Get a single feature row by entity key.

        Calling ``get(table)`` without a key (or with ``key=None``) routes
        to the global-aggregation sentinel (empty-string entity id) per
        ADR-003.

        When ``features`` is provided the server narrows the returned row
        to the named subset; the default (``None``) returns the full row.
        """
        t = self._require_transport()
        effective_key: str | list[Any] = "" if key is None else key
        result: dict[str, Any] = t.send_get(
            table=table, key=effective_key, features=features
        )
        return result

    def batch_get(
        self,
        requests: list[
            tuple[str, str | list[str | int | bool]]
            | tuple[str, str | list[str | int | bool], list[str] | None]
        ],
    ) -> list[dict[str, Any]]:
        """Batch GET — N requests, returns a list of dicts in the same order.

        Args:
            requests: A list of per-entry tuples. Each entry is either a
                ``(table, key)`` 2-tuple or a ``(table, key, features)``
                3-tuple where ``features`` is an optional ``list[str]``
                filter (same shape as :meth:`get`'s ``features`` kwarg,
                applied per entry). The 2-tuple form returns the full row;
                the 3-tuple form narrows to the named features. Both
                shapes can be mixed within the same call. Cold-start
                per-entry result is ``{}``.
        """
        t = self._require_transport()
        coerced: list[
            tuple[str, str | list[Any]]
            | tuple[str, str | list[Any], list[str] | None]
        ] = []
        for entry in requests:
            # The wire contract accepts the tuple shape only. Dict entries
            # raise TypeError here; the transport-equivalence test suite
            # asserts both code paths.
            if not isinstance(entry, tuple):
                raise TypeError(
                    f"batch_get request entry must be a tuple "
                    f"(table, key) or (table, key, features); "
                    f"got {type(entry).__name__}"
                )
            if len(entry) == 2:
                tbl, k = entry
                coerced.append((tbl, k))
            elif len(entry) == 3:
                tbl, k, feats = entry
                coerced.append((tbl, k, feats))
            else:
                raise TypeError(
                    f"batch_get request entry must be a 2- or 3-tuple "
                    f"(table, key) or (table, key, features); "
                    f"got {len(entry)}-tuple"
                )
        result: list[dict[str, Any]] = t.send_batch_get(requests=coerced)
        return result

    def reset(self) -> None:
        """Reset all server state. Test-mode-gated.

        Raises:
            RuntimeError: The server is not in test mode (error code
                ``reset_disabled_in_production``).
        """
        t = self._require_transport()
        t.send_reset()

    def ping(self) -> dict[str, Any]:
        """Server liveness check; returns ``server_version`` + ``registry_version``."""
        t = self._require_transport()
        result: dict[str, Any] = t.send_ping()
        return result
