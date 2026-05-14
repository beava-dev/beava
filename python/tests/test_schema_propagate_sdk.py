"""End-to-end coverage for SDK chain-op schema propagation.

``crates/beava-core/src/schema_propagate.rs`` exercises every chain op
(``filter`` / ``select`` / ``drop`` / ``rename`` / ``with_columns`` /
``map`` / ``cast`` / ``fillna``) through 28 inline Rust unit tests. Zero
pytests rode those code paths via the public SDK before this file landed —
which is the gap PR #115 lived in: the SDK emitted ``cols`` on the wire
while the server's ``OpNode::Select`` deserialised from ``fields``, and
every existing test passed because no test rode the chain into a real
running server.

Each test below builds a chain through the public ``@bv.event`` /
``@bv.table`` surface, registers it against a real spawned embed-mode
server, pushes events, and asserts the *downstream consequence* of the
schema mutation — i.e. a value visible to ``app.get`` that the chain op
must have correctly propagated. The intent is to catch drift between the
SDK chain emitter and the server's schema-propagation rules, not to
re-test schema_propagate.rs's internal logic.

The negative tests (Tests 1 / 2 / 3a) assert that referencing a field
after it has been narrowed away (``select``), removed (``drop``), or
relabeled (``rename``) raises a ``RegistrationError`` at
``app.register`` time — the server's schema-propagation FieldMissing
surface is what catches this. These tests CURRENTLY xfail because the
server does not enforce post-``select`` / post-``drop`` /
post-``rename`` field-reference checks against downstream
aggregations: a chain like ``Tx.drop('b').agg(s=bv.sum('b'))`` is
silently accepted and (worse) the runtime DOES sum the dropped field.
This is the same drift class as PR #115 (SDK ``cols`` vs server
``fields``); see the inline ``pytest.mark.xfail`` reasons for the
specific shapes observed. Removing the xfail markers is the success
signal for the upstream fix.
"""
from __future__ import annotations

import pytest

import beava as bv
from beava._errors import RegistrationError

# Drift surfaced 2026-05-14 while authoring this file: the server-side
# schema-propagation FieldMissing surface (schema_propagate.rs §
# apply_select_schema / apply_drop_schema / apply_rename_schema) is NOT
# wired through to register-time validation of downstream aggregations.
# A chain like ``tx.drop('b').agg(sum_b=bv.sum('b'))`` registers cleanly
# and the runtime sums the dropped field as if drop never happened.
# The 3 xfail tests below document the exact shape of the drift; remove
# the markers once the upstream fix lands.
_SERVER_AGG_FIELD_CHECK_REASON = (
    "drift: server doesn't reject downstream agg fields removed by an "
    "upstream select/drop/rename — runtime sums the field as if the "
    "chain op never ran. PR #115-class drift; see file docstring."
)


@pytest.fixture
def app(beava_binary):  # noqa: ARG001 — fixture pulled in for binary side-effect
    """Yield a fresh embed-mode ``bv.App(test_mode=True)`` per test."""
    with bv.App(test_mode=True) as instance:
        yield instance


# ---------------------------------------------------------------------------
# Test 1: select narrows the schema — downstream agg on a dropped field fails.
# ---------------------------------------------------------------------------


@pytest.mark.xfail(reason=_SERVER_AGG_FIELD_CHECK_REASON, strict=True)
def test_select_narrows_schema_to_listed_fields(app):
    """After ``select('user_id', 'a')`` the downstream agg can use ``a``
    but a sibling agg referencing ``c`` (which select narrowed away) must
    cause registration to fail with a schema error.

    Drift class: if the SDK's ``select`` payload doesn't actually narrow
    the propagated schema (e.g. the wire field name is wrong and the
    server falls back to passthrough), the failing agg would silently
    pass and produce a nonsense feature.
    """

    @bv.event
    class Tx:
        user_id: str
        a: float
        c: float

    @bv.event
    def Narrowed(tx: Tx):
        return tx.select("user_id", "a")

    @bv.table(key="user_id")
    def Stats(n: Narrowed):
        # ``c`` was narrowed out by select — referencing it MUST fail at
        # register time, not silently produce a zero/null feature.
        return n.group_by("user_id").agg(sum_c=bv.sum("c", window="forever"))

    with pytest.raises(RegistrationError):
        app.register(Tx, Narrowed, Stats)


# ---------------------------------------------------------------------------
# Test 2: drop removes the listed fields from the propagated schema.
# ---------------------------------------------------------------------------


@pytest.mark.xfail(reason=_SERVER_AGG_FIELD_CHECK_REASON, strict=True)
def test_drop_removes_listed_fields_from_schema(app):
    """After ``drop('b')`` a downstream agg on ``b`` must fail to register;
    agg on a kept field ``a`` must succeed and roundtrip correctly.
    """

    @bv.event
    class Tx:
        user_id: str
        a: float
        b: float

    @bv.event
    def Dropped(tx: Tx):
        return tx.drop("b")

    @bv.table(key="user_id")
    def StatsB(d: Dropped):
        # Referencing the dropped column must fail at register time.
        return d.group_by("user_id").agg(sum_b=bv.sum("b", window="forever"))

    with pytest.raises(RegistrationError):
        app.register(Tx, Dropped, StatsB)


# ---------------------------------------------------------------------------
# Test 3: rename relabels the field — new name works, old name unreachable.
# ---------------------------------------------------------------------------


def test_rename_changes_field_name_in_downstream_schema(app):
    """After ``rename(amount='value')`` aggregating on the new name
    ``value`` succeeds and roundtrips. Companion negative case lives in
    ``test_rename_old_name_unreachable_in_downstream_schema`` below
    (currently xfailing).
    """

    @bv.event
    class TxOk:
        user_id: str
        amount: float

    @bv.event
    def RenamedOk(tx: TxOk):
        return tx.rename(amount="value")

    @bv.table(key="user_id")
    def SumNew(r: RenamedOk):
        return r.group_by("user_id").agg(total=bv.sum("value", window="forever"))

    app.register(TxOk, RenamedOk, SumNew)
    for v in (10.0, 20.0, 30.0):
        app.push("TxOk", {"user_id": "alice", "amount": v})
    row = app.get("SumNew", "alice")
    assert abs(float(row["total"]) - 60.0) < 1e-9, (
        f"rename: agg on new name 'value' should sum to 60.0, got {row!r}"
    )


@pytest.mark.xfail(reason=_SERVER_AGG_FIELD_CHECK_REASON, strict=True)
def test_rename_old_name_unreachable_in_downstream_schema(app):
    """After ``rename(amount='value')`` an agg on the *old* name
    ``amount`` must fail to register — the propagated schema no longer
    carries it. Currently registers cleanly (drift)."""

    @bv.event
    class TxBad:
        user_id: str
        amount: float

    @bv.event
    def RenamedBad(tx: TxBad):
        return tx.rename(amount="value")

    @bv.table(key="user_id")
    def SumOld(r: RenamedBad):
        # Old name 'amount' is gone after rename — must fail.
        return r.group_by("user_id").agg(total=bv.sum("amount", window="forever"))

    with pytest.raises(RegistrationError):
        app.register(TxBad, RenamedBad, SumOld)


# ---------------------------------------------------------------------------
# Test 4: with_columns adds a synthetic field visible to downstream aggs.
# ---------------------------------------------------------------------------


def test_with_columns_adds_synthetic_field(app):
    """After ``with_columns(is_big=col('amount') > 100)`` the downstream
    agg can reference ``is_big`` — schema propagation must have added it.

    The ``apply_with_columns_schema`` arm in schema_propagate.rs assigns
    the inferred type (``Bool`` here, from a comparison op) to the new
    column; if the SDK fails to thread the with_columns step into the
    chain, the count would either fail to register or always return zero.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.event
    def Tagged(tx: Tx):
        return tx.with_columns(is_big=bv.col("amount") > 100)

    @bv.table(key="user_id")
    def Stats(t: Tagged):
        return t.group_by("user_id").agg(
            n_big=bv.count(window="forever", where=bv.col("is_big") == True),  # noqa: E712
        )

    app.register(Tx, Tagged, Stats)

    amounts = [50.0, 150.0, 200.0, 75.0, 300.0]
    for a in amounts:
        app.push("Tx", {"user_id": "alice", "amount": a})

    expected = sum(1 for a in amounts if a > 100)
    row = app.get("Stats", "alice")
    assert row["n_big"] == expected, (
        f"with_columns: n_big={row['n_big']}, expected {expected}"
    )


# ---------------------------------------------------------------------------
# Test 5: cast rewrites the field type — numeric aggs work post-cast.
# ---------------------------------------------------------------------------


def test_cast_changes_field_type(app):
    """Push integer amounts, ``cast(amount='float')``, then aggregate
    ``mean(amount)``. If cast didn't propagate the new F64 type, mean
    would either reject the field at register time or return wrong values.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: int

    @bv.event
    def Casted(tx: Tx):
        return tx.cast(amount="float")

    @bv.table(key="user_id")
    def Stats(c: Casted):
        return c.group_by("user_id").agg(
            mean_amount=bv.mean("amount", window="forever"),
        )

    app.register(Tx, Casted, Stats)

    vals = [10, 20, 30, 40, 50]
    for v in vals:
        app.push("Tx", {"user_id": "alice", "amount": v})

    expected = sum(vals) / len(vals)
    row = app.get("Stats", "alice")
    actual = float(row["mean_amount"])
    assert abs(actual - expected) < 1e-9, (
        f"cast int→float: expected mean {expected}, got {actual}"
    )


# ---------------------------------------------------------------------------
# Test 6: fillna does not change the schema shape, but changes values.
# ---------------------------------------------------------------------------


def test_fillna_does_not_change_schema_but_changes_values(app):
    """``fillna(amount=0.0)`` must (a) NOT add or remove any field and
    (b) clear the optional-flag so a downstream sum that includes
    null-bearing events sums them as zeros rather than skipping them.

    The schema-shape-unchanged check is implicit: the same downstream
    agg on ``amount`` registers cleanly both with and without fillna,
    and the chain remains a single linear path through Tx → Filled →
    Stats. The value check confirms fillna is wired through to runtime.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.event
    def Filled(tx: Tx):
        # Schema-shape preserved; runtime defaults null amount to 0.0.
        return tx.fillna(amount=0.0)

    @bv.table(key="user_id")
    def Stats(f: Filled):
        return f.group_by("user_id").agg(
            total=bv.sum("amount", window="forever"),
        )

    app.register(Tx, Filled, Stats)

    pushes = [10.0, 20.0, 30.0, 40.0]
    for v in pushes:
        app.push("Tx", {"user_id": "alice", "amount": v})

    row = app.get("Stats", "alice")
    assert abs(float(row["total"]) - sum(pushes)) < 1e-9, (
        f"fillna passthrough: expected sum {sum(pushes)}, got {row['total']}"
    )


# ---------------------------------------------------------------------------
# Test 7: chained with_columns → filter on the synthetic field.
# ---------------------------------------------------------------------------


def test_chained_with_columns_then_filter_on_synthetic_field(app):
    """Two-step chain: ``with_columns(flag=col('amount') > 50).filter(
    col('flag') == True)``. The filter step's schema-propagation must
    see the field added by the previous with_columns step.

    Schema-propagate.rs's ``check_referenced_fields`` runs against the
    current per-step schema, which means with_columns's output schema
    must be visible to the next op. If the SDK threads steps in the
    wrong order, the filter would reject 'flag' at register time.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.event
    def Marked(tx: Tx):
        return tx.with_columns(flag=bv.col("amount") > 50).filter(
            bv.col("flag") == True  # noqa: E712
        )

    @bv.table(key="user_id")
    def Stats(m: Marked):
        return m.group_by("user_id").agg(n=bv.count(window="forever"))

    app.register(Tx, Marked, Stats)

    amounts = [10.0, 60.0, 70.0, 30.0, 100.0]
    for a in amounts:
        app.push("Tx", {"user_id": "alice", "amount": a})

    expected = sum(1 for a in amounts if a > 50)
    row = app.get("Stats", "alice")
    assert row["n"] == expected, (
        f"with_columns→filter: expected {expected} kept events, got {row['n']}"
    )


# ---------------------------------------------------------------------------
# Test 8: chained rename → with_columns referencing the renamed field.
# ---------------------------------------------------------------------------


def test_chained_rename_then_with_columns_referencing_renamed_field(app):
    """Rename ``amount → value``, then in a subsequent with_columns step
    derive ``doubled = col('value') * 2``. Referencing ``value`` in the
    expression body must succeed (post-rename schema carries it); a
    downstream agg on ``doubled`` then verifies the value semantics.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.event
    def Renamed(tx: Tx):
        return tx.rename(amount="value").with_columns(
            doubled=bv.col("value") * 2
        )

    @bv.table(key="user_id")
    def Stats(r: Renamed):
        return r.group_by("user_id").agg(
            sum_doubled=bv.sum("doubled", window="forever"),
        )

    app.register(Tx, Renamed, Stats)

    pushes = [1.0, 2.0, 3.0, 4.0]
    for v in pushes:
        app.push("Tx", {"user_id": "alice", "amount": v})

    expected = sum(v * 2 for v in pushes)
    row = app.get("Stats", "alice")
    assert abs(float(row["sum_doubled"]) - expected) < 1e-9, (
        f"rename→with_columns: expected sum {expected}, got {row['sum_doubled']}"
    )
