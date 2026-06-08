"""group_by().agg() must serialize its key columns as the table's primary key.

Regression for the bug fixed in ``_app.py::_descriptor_to_node``. An
aggregation (``group_by(...).agg(...)``) produces a *table* output, and the
server requires every table to declare its key columns via
``table_primary_key``. The serializer was dropping the group-by columns —
it emitted a table node with **no** ``table_primary_key`` and also left the
key column out of the table schema — so the server rejected the
registration with ``DerivationOutputKindTableMissingPrimaryKey``.

These tests assert the emitted wire node directly, so they need no running
server: the bug lived entirely in the Python serialization step, and the
existing coverage (``test_phase5_smoke.py::test_sc1_groupby_agg_produces_table``)
both needs the binary and sits outside the gated v0 suite.
"""
from __future__ import annotations

import json

import beava as bv
from beava._app import _to_register_json


def _only_table_node(*descriptors: object) -> dict:
    """Serialize the descriptors and return the single table node."""
    payload = json.loads(_to_register_json(descriptors))
    tables = [n for n in payload["nodes"] if n.get("output_kind") == "table"]
    assert len(tables) == 1, f"expected exactly one table node, got {payload['nodes']}"
    return tables[0]


@bv.event
class Click:
    user_id: str
    amount_usd: float


@bv.event
def ClickFeatures(e: Click):
    return e.group_by("user_id").agg(
        clicks_24h=bv.count(window="24h"),
        avg_amount_24h=bv.mean("amount_usd", window="24h"),
    )


def test_single_key_agg_serializes_primary_key() -> None:
    """group_by('user_id').agg(...): the column you grouped by must be sent
    as the table's key, and also kept as a column in the table schema — not
    dropped in favour of only the aggregate columns."""
    node = _only_table_node(Click, ClickFeatures)
    # The group-by column lands on the node as the table's key...
    assert node["table_primary_key"] == ["user_id"]
    # ...and also as a column in the table schema (not just the aggregates).
    fields = node["schema"]["fields"]
    assert "user_id" in fields
    assert "clicks_24h" in fields
    assert "avg_amount_24h" in fields


@bv.event
class Tx:
    user_id: str
    merchant: str
    amount: float


@bv.event
def TxFeatures(e: Tx):
    return e.group_by("user_id", "merchant").agg(cnt=bv.count(window="1h"))


def test_compound_key_agg_serializes_all_key_cols_in_order() -> None:
    """group_by('user_id', 'merchant'): when you group by more than one
    column, every column must be kept, in the order you wrote them. The key
    is an ordered list, not a set, so order matters for how rows are looked
    up later."""
    node = _only_table_node(Tx, TxFeatures)
    # Order is preserved — the key is `(user_id, merchant)`, not a set.
    assert node["table_primary_key"] == ["user_id", "merchant"]
    fields = node["schema"]["fields"]
    assert "user_id" in fields
    assert "merchant" in fields


@bv.event
def ClickFiltered(e: Click):
    return e.filter(bv.col("amount_usd") > 0.0)


def test_event_derivation_has_no_primary_key() -> None:
    """A plain filter (no group_by) stays an event, not a table, so it must
    NOT get a primary key. This is the other side of the contract: this checks 
    that it leaves ordinary derivations alone."""
    payload = json.loads(_to_register_json((Click, ClickFiltered)))
    deriv = next(n for n in payload["nodes"] if n.get("name") == "ClickFiltered")
    assert deriv["output_kind"] == "event"
    assert "table_primary_key" not in deriv
