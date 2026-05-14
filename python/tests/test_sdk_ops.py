"""SDK-OPS-09 unit tests: op methods on EventSource / EventDerivation.

Every test in this file is a unit test — no server required.

Tests verify:
  - Every op returns a NEW derivation (object identity differs; SDK-OPS-09).
  - No mutation of `self` (strict immutability).
  - Each op appends the correct dict to the ops list.
  - Chaining composes left-to-right.
  - Cast client-side validates target type.

Plan 12.7-06: Table-specific assertions removed per
`project_v0_events_only_scope` (locked 2026-04-30). v0 ships events-only;
the `@bv.table` decorator + TableSource / TableDerivation are stripped.
The events surface (filter / select / drop / rename / with_columns / map /
cast / fillna chaining) is intact.
"""

from __future__ import annotations

import pytest

import beava as bv
from beava._events import EventDerivation

# ---------------------------------------------------------------------------
# Shared descriptors (defined at module scope so they are not re-created per test)
# ---------------------------------------------------------------------------


@bv.event
class Transaction:
    amount: float
    kind: str
    ts: int


# Plan 12.7-06: UserProfile @bv.table fixture removed (v0 events-only).


# ---------------------------------------------------------------------------
# SDK-OPS-01: filter
# ---------------------------------------------------------------------------


def test_filter_returns_new_derivation() -> None:
    d1 = Transaction.filter(bv.col("amount") > 100)
    assert d1 is not Transaction
    assert isinstance(d1, EventDerivation)
    assert d1.upstream is Transaction
    assert len(d1.ops) == 1
    assert d1.ops[0] == {"op": "filter", "expr": "(amount > 100)"}


# ---------------------------------------------------------------------------
# SDK-OPS-02: select
# ---------------------------------------------------------------------------


def test_select_returns_new_derivation() -> None:
    d1 = Transaction.select("amount", "ts")
    assert d1 is not Transaction
    assert isinstance(d1, EventDerivation)
    assert d1._chain == [{"op": "select", "fields": ["amount", "ts"]}]


# ---------------------------------------------------------------------------
# SDK-OPS-03: drop
# ---------------------------------------------------------------------------


def test_drop_returns_new_derivation() -> None:
    d1 = Transaction.drop("amount")
    assert d1 is not Transaction
    assert isinstance(d1, EventDerivation)
    assert d1._chain == [{"op": "drop", "fields": ["amount"]}]


# ---------------------------------------------------------------------------
# SDK-OPS-04: rename
# ---------------------------------------------------------------------------


def test_rename_returns_new_derivation() -> None:
    d1 = Transaction.rename(amount="amt")
    assert d1 is not Transaction
    assert isinstance(d1, EventDerivation)
    assert d1.ops == [{"op": "rename", "mapping": {"amount": "amt"}}]


# ---------------------------------------------------------------------------
# SDK-OPS-05: with_columns
# ---------------------------------------------------------------------------


def test_with_columns_returns_new_derivation() -> None:
    d1 = Transaction.with_columns(is_big=bv.col("amount") > 500)
    assert d1 is not Transaction
    assert isinstance(d1, EventDerivation)
    assert d1.ops == [{"op": "with_columns", "exprs": {"is_big": "(amount > 500)"}}]


# ---------------------------------------------------------------------------
# SDK-OPS-06: map (alias; distinct op name on wire)
# ---------------------------------------------------------------------------


def test_map_is_alias_of_with_columns() -> None:
    d1 = Transaction.map(is_big=bv.col("amount") > 500)
    assert d1 is not Transaction
    assert isinstance(d1, EventDerivation)
    assert d1.ops[0]["op"] == "map"
    assert d1.ops[0]["exprs"] == {"is_big": "(amount > 500)"}


# ---------------------------------------------------------------------------
# SDK-OPS-07: cast
# ---------------------------------------------------------------------------


def test_cast_returns_new_derivation() -> None:
    d1 = Transaction.cast(amount="int")
    assert d1 is not Transaction
    assert isinstance(d1, EventDerivation)
    assert d1.ops == [{"op": "cast", "type_map": {"amount": "int"}}]


# ---------------------------------------------------------------------------
# SDK-OPS-08: fillna
# ---------------------------------------------------------------------------


def test_fillna_returns_new_derivation() -> None:
    d1 = Transaction.fillna(amount=0)
    assert d1 is not Transaction
    assert isinstance(d1, EventDerivation)
    assert d1.ops == [{"op": "fillna", "defaults": {"amount": 0}}]


# ---------------------------------------------------------------------------
# SDK-OPS-09/10: chaining + immutability
# ---------------------------------------------------------------------------


def test_chained_ops_append_to_ops_list() -> None:
    # Capture intermediate derivation BEFORE chaining further.
    filter_only = Transaction.filter(bv.col("amount") > 0)
    assert len(filter_only.ops) == 1  # sanity pre-condition

    d3 = filter_only.select("amount", "ts").with_columns(
        is_big=bv.col("amount") > 500
    )

    assert len(d3.ops) == 3
    assert d3.ops[0]["op"] == "filter"
    assert d3.ops[1]["op"] == "select"
    assert d3.ops[2]["op"] == "with_columns"

    # SDK-OPS-09: filter_only must NOT be mutated by further chaining.
    assert len(filter_only.ops) == 1, (
        "filter_only.ops was mutated by downstream chaining — SDK-OPS-09 violated"
    )


def test_source_reference_preserved_through_chain() -> None:
    d1 = Transaction.filter(bv.col("amount") > 0)
    d2 = d1.select("amount", "ts")
    d3 = d2.with_columns(is_big=bv.col("amount") > 500)
    # Can trace back to the original source through the upstream chain.
    assert d3.upstream is d2
    assert d2.upstream is d1
    assert d1.upstream is Transaction


# Plan 12.7-06: Table-specific tests removed per project_v0_events_only_scope.
# The drop-rejects-key and rename-cascades-key behaviors were table-only
# (events have no primary key). Return in v0.1+ if tables revive.


# ---------------------------------------------------------------------------
# Return-type alignment
# ---------------------------------------------------------------------------


def test_event_op_methods_available_on_event_derivation() -> None:
    d1 = Transaction.filter(bv.col("amount") > 0)
    assert isinstance(d1, EventDerivation)
    # d1 itself must expose the op methods (fluent chaining).
    d2 = d1.filter(bv.col("amount") < 1000)
    assert isinstance(d2, EventDerivation)
    assert len(d2.ops) == 2


def test_op_method_returns_type_matches_source() -> None:
    # EventSource -> EventDerivation
    ev_d = Transaction.filter(bv.col("amount") > 0)
    assert isinstance(ev_d, EventDerivation)
    # Plan 12.7-06: TableSource -> TableDerivation case removed (v0 events-only).


# ---------------------------------------------------------------------------
# Expression serialization
# ---------------------------------------------------------------------------


def test_filter_expression_serializes_to_canonical_form() -> None:
    d1 = Transaction.filter(
        (bv.col("amount") > 100) & (bv.col("kind") == "fraud")
    )
    assert d1.ops[0]["expr"] == "((amount > 100) and (kind == 'fraud'))"


def test_with_columns_expressions_serialize_to_canonical_form() -> None:
    d1 = Transaction.with_columns(
        is_big=bv.col("amount") > 500,
        is_small=bv.col("amount") < 10,
    )
    assert d1.ops[0]["exprs"] == {
        "is_big": "(amount > 500)",
        "is_small": "(amount < 10)",
    }


def test_fillna_default_serialization() -> None:
    d1 = Transaction.fillna(amount=0, kind="unknown")
    assert d1.ops[0]["defaults"] == {"amount": 0, "kind": "unknown"}


# ---------------------------------------------------------------------------
# SDK-OPS-07: cast client-side target validation
# ---------------------------------------------------------------------------


def test_cast_type_map_rejects_invalid_target_client_side() -> None:
    with pytest.raises(ValueError, match="blob"):
        Transaction.cast(amount="blob")
    # Valid targets do NOT raise.
    for valid in ("str", "int", "float", "bool"):
        Transaction.cast(amount=valid)  # must not raise
