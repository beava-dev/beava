"""Derivation-on-derivation chain coverage.

Audit gap (2026-05-14): ``test_lit.py`` and the other v0 acceptance tests
all exercise the linear shape ``EventSource → Derivation → Table``. No test
covered the **derivation-on-derivation** shape — ``@bv.event def Stage2(s1:
Stage1): ...`` where ``Stage1`` is itself a ``@bv.event``-decorated
function, not an event source. That multi-hop shape (raw event → cleanup
hop → enrichment hop → table) is the most common production layout for
fraud / ad-tech pipelines; tests below lock it in.

Coverage:
  1. Three-hop ``filter → filter`` chain — terminal count matches the
     pure-Python ground truth of the composed predicate.
  2. Three-hop ``with_columns → with_columns`` chain — synthetic fields
     introduced at hop 1 survive a second hop and aggregate correctly.
  3. Three-hop ``filter → select → filter`` chain — schema narrows
     through the chain; downstream stages see the projected fields.
  4. Raw chain rejection at register time — passing an un-wrapped
     ``EventDerivation`` (a bare ``.filter(...)`` result) to
     ``app.register`` raises ``RegistrationError`` with the canonical
     ``@bv.event def`` rewrite hint. Locks the error path at
     ``python/beava/_app.py::App.register`` (kept in sync with the
     parameter-annotation rejection at ``_events.py:330-342``).
"""
from __future__ import annotations

import random

import pytest

import beava as bv
from beava._errors import RegistrationError

from ._helpers import (
    ENTITIES,
    _engine_available,
    cold_start_equivalent,
)

pytestmark = pytest.mark.skipif(
    not _engine_available(),
    reason="requires Phase 13.4 engine + Phase 13.5 SDK rewrite (derivation chains)",
)


# ---------------------------------------------------------------------------
# Test 1: three-hop filter-on-filter chain
# ---------------------------------------------------------------------------


def test_three_hop_derivation_chain_filters_correctly(app):
    """Tx → HighValueTx(amount>=100) → VeryHighValueTx(amount>=1000) → Table.

    Audit gap: previously untested — ``HighValueTx`` itself is a
    ``@bv.event def``, so the parameter annotation on ``VeryHighValueTx``
    resolves to a derivation-function, not an event-source class.
    Locks the multi-hop chain through ``_events.py::_make_event_derivation``
    and ``_app.py::_to_register_json``.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.event
    def HighValueTx(tx: Tx):
        return tx.filter(bv.col("amount") >= 100)

    @bv.event
    def VeryHighValueTx(high: HighValueTx):
        return high.filter(bv.col("amount") >= 1000)

    @bv.table(key="user_id")
    def TxStats(vh: VeryHighValueTx):
        return vh.group_by("user_id").agg(n=bv.count(window="forever"))

    app.register(Tx, HighValueTx, VeryHighValueTx, TxStats)

    rng = random.Random(2026_05_14)
    expected: dict[str, int] = {entity: 0 for entity in ENTITIES}
    for _ in range(30):
        user = rng.choice(ENTITIES)
        amount = rng.uniform(10.0, 2000.0)
        # Composed predicate: both hop filters must pass.
        if amount >= 100 and amount >= 1000:
            expected[user] += 1
        app.push("Tx", {"user_id": user, "amount": amount})

    for entity, exp in expected.items():
        result = app.get("TxStats", entity)
        if exp == 0:
            assert cold_start_equivalent(result) or result.get("n", 0) == 0, (
                f"{entity}: expected cold-start or n=0, got {result!r}"
            )
            continue
        assert result.get("n", 0) == exp, (
            f"{entity}: terminal count {result.get('n', 0)} != expected {exp} "
            f"(composed amount>=100 ∧ amount>=1000)"
        )

    assert cold_start_equivalent(app.get("TxStats", "unknown_3hop"))


# ---------------------------------------------------------------------------
# Test 2: three-hop with_columns-on-with_columns chain
# ---------------------------------------------------------------------------


def test_three_hop_with_with_columns_in_each_stage(app):
    """Click → Tagged(source='web') → Scored(score=amount*2) → Table(sum=score).

    Audit gap: synthetic-field propagation across two derivation hops.
    Hop 1 introduces ``source``; hop 2 introduces ``score`` derived from
    the original ``amount``. The terminal aggregation reads ``score`` —
    proves the with_columns merge survives the derivation-on-derivation
    rewrite at server-side schema propagation.
    """

    @bv.event
    class Click:
        user_id: str
        amount: float

    @bv.event
    def Tagged(click: Click):
        return click.with_columns(source=bv.lit("web"))

    @bv.event
    def Scored(tagged: Tagged):
        # score = amount * 2 — references the original Click field,
        # which must survive through the Tagged hop's schema.
        return tagged.with_columns(score=bv.col("amount") * bv.lit(2.0))

    @bv.table(key="user_id")
    def UserScore(scored: Scored):
        return scored.group_by("user_id").agg(
            total_score=bv.sum("score", window="forever"),
            n=bv.count(window="forever"),
        )

    app.register(Click, Tagged, Scored, UserScore)

    rng = random.Random(2026_05_14 + 1)
    expected_score: dict[str, float] = {entity: 0.0 for entity in ENTITIES}
    expected_n: dict[str, int] = {entity: 0 for entity in ENTITIES}
    for _ in range(400):
        user = rng.choice(ENTITIES)
        amount = rng.uniform(1.0, 50.0)
        expected_score[user] += amount * 2.0
        expected_n[user] += 1
        app.push("Click", {"user_id": user, "amount": amount})

    for entity in ENTITIES:
        if expected_n[entity] == 0:
            continue
        result = app.get("UserScore", entity)
        assert result.get("n", 0) == expected_n[entity], (
            f"{entity}: count {result.get('n', 0)} != {expected_n[entity]}"
        )
        actual = float(result.get("total_score", 0.0))
        assert abs(actual - expected_score[entity]) < 1e-3, (
            f"{entity}: total_score {actual} != expected {expected_score[entity]} "
            f"(synthetic field must propagate hop1 → hop2 → table)"
        )

    assert cold_start_equivalent(app.get("UserScore", "unknown_wc3"))


# ---------------------------------------------------------------------------
# Test 3: three-hop filter → select → filter (schema-narrowing)
# ---------------------------------------------------------------------------


def test_three_hop_with_filter_then_select_then_filter(app):
    """Tx → Positive(filter amount>0) → Trim(select amount,user_id) → Big(filter amount>500).

    Audit gap: mixes chain ops across hops — hop 1 filters, hop 2 narrows
    the schema via select, hop 3 filters on a field that survived the
    select. Confirms field projection through the derivation-on-derivation
    chain and that the terminal filter can reference fields preserved by
    the upstream select.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float
        # An unused field that hop 2 drops via select.
        note: str

    @bv.event
    def Positive(tx: Tx):
        return tx.filter(bv.col("amount") > 0)

    @bv.event
    def Trim(positive: Positive):
        # Narrow schema: drop `note`, keep amount + user_id.
        return positive.select("user_id", "amount")

    @bv.event
    def Big(trim: Trim):
        # Must succeed referencing `amount` — survived the select narrowing.
        return trim.filter(bv.col("amount") > 500)

    @bv.table(key="user_id")
    def BigCount(big: Big):
        return big.group_by("user_id").agg(n=bv.count(window="forever"))

    app.register(Tx, Positive, Trim, Big, BigCount)

    rng = random.Random(2026_05_14 + 2)
    expected: dict[str, int] = {entity: 0 for entity in ENTITIES}
    for _ in range(500):
        user = rng.choice(ENTITIES)
        amount = rng.uniform(-200.0, 1000.0)
        note = rng.choice(["a", "b", "c"])
        # Composed: amount > 0 ∧ amount > 500 → amount > 500.
        if amount > 500:
            expected[user] += 1
        app.push("Tx", {"user_id": user, "amount": amount, "note": note})

    for entity, exp in expected.items():
        result = app.get("BigCount", entity)
        if exp == 0:
            assert cold_start_equivalent(result) or result.get("n", 0) == 0, (
                f"{entity}: expected cold-start or n=0, got {result!r}"
            )
            continue
        assert result.get("n", 0) == exp, (
            f"{entity}: terminal count {result.get('n', 0)} != expected {exp} "
            f"(filter→select→filter chain)"
        )

    assert cold_start_equivalent(app.get("BigCount", "unknown_fsf"))


# ---------------------------------------------------------------------------
# Test 4: raw chain rejected at register-time with canonical rewrite hint
# ---------------------------------------------------------------------------


def test_raw_chain_as_parameter_is_rejected_with_helpful_hint(app):
    """Raw ``EventDerivation`` (unwrapped chain) → RegistrationError with rewrite hint.

    Audit gap: the rejection path at ``python/beava/_app.py::App.register``
    (and the sibling check at ``_events.py:330-342``) is what prevents
    users from accidentally registering bare ``.filter(...)`` results —
    those expressions have no stable name the apply-time routing index
    can key on. This test locks BOTH error paths:
      A. ``app.register(raw_chain)`` → ``RegistrationError`` with the
         canonical ``@bv.event def`` rewrite hint.
      B. Annotating an ``@bv.event def`` parameter with a raw chain →
         ``TypeError`` at decoration time, also with the rewrite hint.
    """

    @bv.event
    class Click:
        user_id: str
        amount: float

    # ── Path A: register-time rejection of a bare chain.
    # `Click.filter(...)` returns an EventDerivation that lacks the
    # `_is_bv_event_function` marker — the register() check fires.
    raw_chain = Click.filter(bv.col("amount") > 100)

    with pytest.raises(RegistrationError) as exc_info:
        app.register(Click, raw_chain)

    assert exc_info.value.code == "invalid_descriptor", (
        f"raw chain registration must surface code='invalid_descriptor'; "
        f"got code={exc_info.value.code!r}"
    )
    msg = exc_info.value.message
    # Canonical rewrite hint must mention @bv.event so users find the fix.
    assert "@bv.event" in msg, (
        f"RegistrationError message missing canonical '@bv.event' rewrite hint; "
        f"got message={msg!r}"
    )
    assert "raw chain" in msg or "EventDerivation" in msg, (
        f"RegistrationError message must name the offending shape; got message={msg!r}"
    )

    # ── Path B: decoration-time rejection — annotating a param with a raw
    # EventDerivation also fails, with a TypeError that points at the
    # same @bv.event def rewrite.
    raw_chain_for_annotation = Click.filter(bv.col("amount") > 0)

    with pytest.raises(TypeError) as type_exc:
        @bv.event
        def Bad(raw: raw_chain_for_annotation):  # noqa: B008 — intentional misuse
            return raw.filter(bv.col("amount") > 100)

    type_msg = str(type_exc.value)
    assert "@bv.event" in type_msg, (
        f"raw-chain annotation TypeError missing '@bv.event' rewrite hint; "
        f"got {type_msg!r}"
    )
    assert "EventDerivation" in type_msg or "raw chain" in type_msg, (
        f"raw-chain annotation TypeError must name the offending shape; got {type_msg!r}"
    )
