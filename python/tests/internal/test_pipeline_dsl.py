"""Phase 13.5 Plan 03 red tests: pipeline DSL chain methods.

Validates the chain-method surface documented in docs/sdk-api/python.md §
Pipeline DSL. Tests focus on AST/JSON shape — no engine round-trip
(Plan 11 covers integration).
"""
from __future__ import annotations

import pytest

import beava as bv


@bv.event
class _Txn:
    user_id: str
    amount: float
    merchant: str


def test_filter_returns_derivation_with_chain_node() -> None:
    d = _Txn.filter(bv.col("amount") > 100)
    assert d is not _Txn  # filter creates a new derivation
    assert hasattr(d, "_chain")
    assert d._chain[-1]["op"] == "filter"


def test_select_drops_unselected() -> None:
    d = _Txn.select("user_id", "amount")
    assert d._chain[-1]["op"] == "select"
    assert d._chain[-1]["cols"] == ["user_id", "amount"]


def test_drop_removes_named_cols() -> None:
    d = _Txn.drop("merchant")
    assert d._chain[-1]["op"] == "drop"
    assert d._chain[-1]["cols"] == ["merchant"]


def test_rename_kwargs() -> None:
    d = _Txn.rename(amount="amt")
    assert d._chain[-1]["op"] == "rename"
    assert d._chain[-1]["mapping"] == {"amount": "amt"}


def test_with_columns_kwargs_emit_expressions() -> None:
    d = _Txn.with_columns(big=bv.col("amount") > 100)
    assert d._chain[-1]["op"] == "with_columns"
    assert "big" in d._chain[-1]["exprs"]


def test_cast_validates_target_types() -> None:
    d = _Txn.cast(amount="int")
    assert d._chain[-1]["op"] == "cast"
    with pytest.raises(ValueError):
        _Txn.cast(amount="invalid_type")


def test_fillna_kwargs() -> None:
    d = _Txn.fillna(merchant="unknown")
    assert d._chain[-1]["op"] == "fillna"


def test_group_by_returns_GroupBy_instance() -> None:
    g = _Txn.group_by("user_id")
    # GroupBy class — its identity is shape-checked here
    assert hasattr(g, "agg")
    assert g._keys == ("user_id",)


def test_group_by_empty_returns_global_GroupBy() -> None:
    """ADR-003: events.group_by() with no args is allowed and means global aggregation."""
    g = _Txn.group_by()
    assert hasattr(g, "agg")
    assert g._keys == ()


def test_filter_chain_combines_via_AND() -> None:
    d = _Txn.filter((bv.col("amount") > 100) & (bv.col("user_id") == "alice"))
    assert d._chain[-1]["op"] == "filter"


def test_event_class_collects_schema_from_annotations() -> None:
    """@bv.event class form: schema fields populated from type annotations."""
    assert hasattr(_Txn, "_schema")
    schema = _Txn._schema
    assert "user_id" in schema
    assert "amount" in schema
    assert "merchant" in schema


def test_event_function_form_creates_derivation() -> None:
    @bv.event
    def BigTxn(txn: _Txn):
        return txn.filter(bv.col("amount") > 100)

    assert hasattr(BigTxn, "_chain")
    assert BigTxn._name == "BigTxn"


def test_event_with_event_time_field_raises_TypeError() -> None:
    """Declaring event_time field raises TypeError, no event-time in v0 (docs/sdk-api/python.md)."""
    with pytest.raises(TypeError, match="event_time"):
        @bv.event
        class HasEventTime:
            event_time: int


def test_event_with_tolerate_delay_kwarg_raises_TypeError() -> None:
    with pytest.raises(TypeError, match="tolerate_delay"):
        @bv.event(tolerate_delay="5s")
        class HasTolerate:
            x: int


def test_per_source_kwargs_keep_events_for_and_dedupe() -> None:
    @bv.event(keep_events_for="30d", dedupe_key="x", dedupe_window="5m")
    class Login:
        x: str
        y: str

    assert Login._keep_events_for == "30d"
    assert Login._dedupe_key == "x"
    assert Login._dedupe_window == "5m"
