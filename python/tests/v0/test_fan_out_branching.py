"""Fan-out branching tests — one event source consumed by N>=2 downstream consumers.

Covers the production-shape "one transaction event feeds user stats + merchant stats
+ fraud signals + audit" case. The dual to ``test_v0_dag.py``'s fan-IN test (one
derivation taking multiple upstream events): a single ``app.push(...)`` must
atomically update EVERY downstream consumer that lists the event class in its
parameter list / aggregation source, including the ``apply_field_names`` union
remap that PR #106 fixed.

The agg-apply loop (``crates/beava-core/src/agg_apply.rs``) iterates every
compiled aggregation registered against the source name on each push; these tests
pin the SDK-visible contract for that loop: counts, sums, and filter predicates
must be consistent across all N consumers after the same push count.

Tests:
  1. test_single_event_pushes_to_three_tables_atomically — one Tx, three tables
     keyed by user/merchant/card_fp; all three updated per push.
  2. test_single_event_pushes_to_two_derivations_and_one_table — one Tx splits
     into BigTx/SmallTx derivations plus an AllTxStats table; each event lands
     in exactly one of big/small AND in all-tx.
  3. test_fan_out_then_fan_in — Tx fan-OUT into A (ok) and B (risky), then C
     fan-IN of A+B; verifies the fan-in correctly merges the branched streams.
  4. test_fan_out_atomicity_after_register_force — register branching set,
     push, register a DIFFERENT branching set with ``force=True`` sharing the
     same source, push more. Asserts only the NEW aggs see the new events.
  5. test_fan_out_with_overlapping_where_predicates — Tx → A (where ok=True) +
     B (where amount>=100); events satisfying both increment BOTH (an event in
     A does NOT suppress its appearance in B).
"""
from __future__ import annotations

import random
from typing import Any

import pytest

import beava as bv

from ._helpers import ENTITIES, _engine_available, cold_start_equivalent

pytestmark = pytest.mark.skipif(
    not _engine_available(),
    reason="requires Phase 13.4 engine + Phase 13.5 SDK rewrite (fan-out branching)",
)


# ---------------------------------------------------------------------------
# Test 1: one event, three downstream tables, atomic per-push update
# ---------------------------------------------------------------------------


def test_single_event_pushes_to_three_tables_atomically(app: Any) -> None:
    """One Tx → UserStats + MerchantStats + CardStats; all three updated per push.

    Mirrors the canonical fraud shape: one transaction event feeds per-user,
    per-merchant, and per-card aggregations that are all expected to be
    consistent after any number of pushes. The apply loop must visit every
    compiled aggregation on the source for each row.
    """

    @bv.event
    class Tx:
        user_id: str
        merchant_id: str
        card_fp: str
        amount: float

    @bv.table(key="user_id")
    def UserStats(tx: Tx):
        return tx.group_by("user_id").agg(
            n_user=bv.count(window="forever"),
            total_user=bv.sum("amount", window="forever"),
        )

    @bv.table(key="merchant_id")
    def MerchantStats(tx: Tx):
        return tx.group_by("merchant_id").agg(
            n_merchant=bv.count(window="forever"),
            total_merchant=bv.sum("amount", window="forever"),
        )

    @bv.table(key="card_fp")
    def CardStats(tx: Tx):
        return tx.group_by("card_fp").agg(
            n_card=bv.count(window="forever"),
            total_card=bv.sum("amount", window="forever"),
        )

    app.register(Tx, UserStats, MerchantStats, CardStats)

    rng = random.Random(311)
    users = ENTITIES
    merchants = ["m_amazon", "m_uber", "m_starbucks"]
    cards = ["c_4242", "c_5555", "c_3782", "c_6011"]

    n_user: dict[str, int] = {u: 0 for u in users}
    total_user: dict[str, float] = {u: 0.0 for u in users}
    n_merchant: dict[str, int] = {m: 0 for m in merchants}
    total_merchant: dict[str, float] = {m: 0.0 for m in merchants}
    n_card: dict[str, int] = {c: 0 for c in cards}
    total_card: dict[str, float] = {c: 0.0 for c in cards}

    for _ in range(600):
        u = rng.choice(users)
        m = rng.choice(merchants)
        c = rng.choice(cards)
        amount = rng.uniform(1.0, 500.0)
        n_user[u] += 1
        total_user[u] += amount
        n_merchant[m] += 1
        total_merchant[m] += amount
        n_card[c] += 1
        total_card[c] += amount
        app.push(
            "Tx",
            {"user_id": u, "merchant_id": m, "card_fp": c, "amount": amount},
        )

    # All three tables must be consistent — each push hit every one.
    for u, exp_n in n_user.items():
        if exp_n == 0:
            continue
        row = app.get("UserStats", u)
        assert row.get("n_user") == exp_n, f"UserStats[{u}]: n={row.get('n_user')} != {exp_n}"
        assert abs(row.get("total_user", 0.0) - total_user[u]) < 1e-6, (
            f"UserStats[{u}]: total={row.get('total_user')} != {total_user[u]}"
        )

    for m, exp_n in n_merchant.items():
        if exp_n == 0:
            continue
        row = app.get("MerchantStats", m)
        assert row.get("n_merchant") == exp_n, (
            f"MerchantStats[{m}]: n={row.get('n_merchant')} != {exp_n}"
        )
        assert abs(row.get("total_merchant", 0.0) - total_merchant[m]) < 1e-6, (
            f"MerchantStats[{m}]: total={row.get('total_merchant')} != {total_merchant[m]}"
        )

    for c, exp_n in n_card.items():
        if exp_n == 0:
            continue
        row = app.get("CardStats", c)
        assert row.get("n_card") == exp_n, f"CardStats[{c}]: n={row.get('n_card')} != {exp_n}"
        assert abs(row.get("total_card", 0.0) - total_card[c]) < 1e-6, (
            f"CardStats[{c}]: total={row.get('total_card')} != {total_card[c]}"
        )

    # Push count consistency: sum of per-entity counts in each table = events pushed.
    assert sum(n_user.values()) == 600
    assert sum(n_merchant.values()) == 600
    assert sum(n_card.values()) == 600

    assert cold_start_equivalent(app.get("UserStats", "unknown_user_xx"))
    assert cold_start_equivalent(app.get("MerchantStats", "unknown_merchant_xx"))
    assert cold_start_equivalent(app.get("CardStats", "unknown_card_xx"))


# ---------------------------------------------------------------------------
# Test 2: one event, two derivations (big/small filter) + one table
# ---------------------------------------------------------------------------


def test_single_event_pushes_to_two_derivations_and_one_table(app: Any) -> None:
    """Tx → BigTx (amount>=500) + SmallTx (amount<500) + AllTxStats (all).

    Branches by mutually-exclusive filter into two derivations and an unfiltered
    table. Every event must increment exactly one of big/small AND the all-tx
    table; the partition must hold.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    @bv.event
    def BigTx(tx: Tx):
        return tx.filter(bv.col("amount") >= 500)

    @bv.event
    def SmallTx(tx: Tx):
        return tx.filter(bv.col("amount") < 500)

    @bv.table(key="user_id")
    def AllTxStats(tx: Tx):
        return tx.group_by("user_id").agg(
            n_all=bv.count(window="forever"),
            total_all=bv.sum("amount", window="forever"),
        )

    @bv.table(key="user_id")
    def BigTxStats(big: BigTx):
        return big.group_by("user_id").agg(n_big=bv.count(window="forever"))

    @bv.table(key="user_id")
    def SmallTxStats(small: SmallTx):
        return small.group_by("user_id").agg(n_small=bv.count(window="forever"))

    app.register(Tx, BigTx, SmallTx, AllTxStats, BigTxStats, SmallTxStats)

    rng = random.Random(312)
    n_all: dict[str, int] = {u: 0 for u in ENTITIES}
    total_all: dict[str, float] = {u: 0.0 for u in ENTITIES}
    n_big: dict[str, int] = {u: 0 for u in ENTITIES}
    n_small: dict[str, int] = {u: 0 for u in ENTITIES}

    for _ in range(800):
        u = rng.choice(ENTITIES)
        amount = rng.uniform(0.0, 1000.0)
        n_all[u] += 1
        total_all[u] += amount
        if amount >= 500:
            n_big[u] += 1
        else:
            n_small[u] += 1
        app.push("Tx", {"user_id": u, "amount": amount})

    for u in ENTITIES:
        if n_all[u] == 0:
            continue
        all_row = app.get("AllTxStats", u)
        big_row = app.get("BigTxStats", u)
        small_row = app.get("SmallTxStats", u)

        assert all_row.get("n_all") == n_all[u], (
            f"AllTxStats[{u}]: n_all={all_row.get('n_all')} != {n_all[u]}"
        )
        assert abs(all_row.get("total_all", 0.0) - total_all[u]) < 1e-6, (
            f"AllTxStats[{u}]: total={all_row.get('total_all')} != {total_all[u]}"
        )
        assert big_row.get("n_big", 0) == n_big[u], (
            f"BigTxStats[{u}]: n_big={big_row.get('n_big', 0)} != {n_big[u]}"
        )
        assert small_row.get("n_small", 0) == n_small[u], (
            f"SmallTxStats[{u}]: n_small={small_row.get('n_small', 0)} != {n_small[u]}"
        )

        # Partition invariant: big + small == all (BigTx and SmallTx are
        # mutually exclusive by filter; every Tx lands in exactly one).
        assert n_big[u] + n_small[u] == n_all[u], (
            f"[{u}] partition broken: big={n_big[u]} + small={n_small[u]} != all={n_all[u]}"
        )

    assert cold_start_equivalent(app.get("BigTxStats", "unknown_big_xx"))


# ---------------------------------------------------------------------------
# Test 3: fan-OUT then fan-IN — Tx → (A, B) → C
# ---------------------------------------------------------------------------


def test_fan_out_then_fan_in(app: Any) -> None:
    """Tx fan-OUT into AOk + BRisky, then fan-IN at the table layer via CStats.

    Exercises same-source branching followed by re-merge at the consumer.
    v0's @bv.event def chain emits a SINGLE upstream root through the wire
    payload (``_to_register_json`` walks ``_parent`` up to the EventSource),
    so a true multi-source fan-in event derivation isn't expressible
    end-to-end at the SDK→server boundary. The pragmatic fan-in is at the
    table-consumer layer: AStats reads AOk, BStats reads BRisky, and CStats
    reads Tx directly — CStats acts as the union (every push hits it) and
    its count must be the strict superset of A+B counts (AOk/BRisky
    predicates are mutually exclusive on this fixture).
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float
        kind: str  # "ok" or "risky" or "neither" — mutually-exclusive partitions

    @bv.event
    def AOk(tx: Tx):
        return tx.filter(bv.col("kind") == "ok")

    @bv.event
    def BRisky(tx: Tx):
        return tx.filter(bv.col("kind") == "risky")

    @bv.table(key="user_id")
    def AStats(a: AOk):
        return a.group_by("user_id").agg(
            n_a=bv.count(window="forever"),
            sum_a=bv.sum("amount", window="forever"),
        )

    @bv.table(key="user_id")
    def BStats(b: BRisky):
        return b.group_by("user_id").agg(
            n_b=bv.count(window="forever"),
            sum_b=bv.sum("amount", window="forever"),
        )

    # CStats is the fan-IN consumer: derives directly from Tx (source-level),
    # so every push that flows through AOk OR BRisky also lands here.
    @bv.table(key="user_id")
    def CStats(tx: Tx):
        return tx.group_by("user_id").agg(
            n_c=bv.count(window="forever"),
            sum_c=bv.sum("amount", window="forever"),
        )

    app.register(Tx, AOk, BRisky, AStats, BStats, CStats)

    rng = random.Random(313)
    n_a: dict[str, int] = {u: 0 for u in ENTITIES}
    sum_a: dict[str, float] = {u: 0.0 for u in ENTITIES}
    n_b: dict[str, int] = {u: 0 for u in ENTITIES}
    sum_b: dict[str, float] = {u: 0.0 for u in ENTITIES}
    n_c: dict[str, int] = {u: 0 for u in ENTITIES}
    sum_c: dict[str, float] = {u: 0.0 for u in ENTITIES}

    for _ in range(700):
        u = rng.choice(ENTITIES)
        amount = rng.uniform(1.0, 100.0)
        # Mutually exclusive partition so n_a + n_b <= n_c, with a "neither"
        # bucket so fan-in (CStats) sees strictly more events than A+B.
        kind = rng.choice(["ok", "risky", "neither"])
        if kind == "ok":
            n_a[u] += 1
            sum_a[u] += amount
        elif kind == "risky":
            n_b[u] += 1
            sum_b[u] += amount
        n_c[u] += 1
        sum_c[u] += amount
        app.push("Tx", {"user_id": u, "amount": amount, "kind": kind})

    for u in ENTITIES:
        if n_c[u] == 0:
            continue
        a_row = app.get("AStats", u)
        b_row = app.get("BStats", u)
        c_row = app.get("CStats", u)

        assert a_row.get("n_a", 0) == n_a[u], (
            f"AStats[{u}]: n_a={a_row.get('n_a', 0)} != {n_a[u]}"
        )
        assert abs(a_row.get("sum_a", 0.0) - sum_a[u]) < 1e-6, (
            f"AStats[{u}]: sum_a={a_row.get('sum_a', 0.0)} != {sum_a[u]}"
        )
        assert b_row.get("n_b", 0) == n_b[u], (
            f"BStats[{u}]: n_b={b_row.get('n_b', 0)} != {n_b[u]}"
        )
        assert abs(b_row.get("sum_b", 0.0) - sum_b[u]) < 1e-6, (
            f"BStats[{u}]: sum_b={b_row.get('sum_b', 0.0)} != {sum_b[u]}"
        )
        # Fan-in invariant: CStats sees the union (every push). Since
        # AOk/BRisky filters are mutually exclusive, CStats's count
        # is the strict superset of A + B (plus the "neither" bucket).
        assert c_row.get("n_c", 0) == n_c[u], (
            f"CStats[{u}]: n_c={c_row.get('n_c', 0)} != {n_c[u]} "
            f"(expected union of branches + neither bucket)"
        )
        assert c_row.get("n_c", 0) >= n_a[u] + n_b[u], (
            f"CStats[{u}]: fan-in {c_row.get('n_c', 0)} < n_a+n_b={n_a[u] + n_b[u]}"
        )
        assert abs(c_row.get("sum_c", 0.0) - sum_c[u]) < 1e-6, (
            f"CStats[{u}]: sum_c={c_row.get('sum_c', 0.0)} != {sum_c[u]}"
        )

    assert cold_start_equivalent(app.get("CStats", "unknown_c_xx"))


# ---------------------------------------------------------------------------
# Test 4: register force=True swaps the branching set for the same source
# ---------------------------------------------------------------------------


def test_fan_out_atomicity_after_register_force(app: Any) -> None:
    """Register branch-set A → push → register branch-set B with force=True → push.

    After ``force=True`` the OLD aggs are gone; the NEW aggs see ONLY the events
    pushed after the force-register. Locks down the register-replace semantics
    when interacting with fan-out: an SDK regression that leaves stale aggs in
    the registry would surface as the OLD tables still incrementing on the new
    events.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float

    # --- branching set A: two tables on Tx ----------------------------------
    @bv.table(key="user_id")
    def StatsAOne(tx: Tx):
        return tx.group_by("user_id").agg(a1=bv.count(window="forever"))

    @bv.table(key="user_id")
    def StatsATwo(tx: Tx):
        return tx.group_by("user_id").agg(a2=bv.sum("amount", window="forever"))

    app.register(Tx, StatsAOne, StatsATwo)

    rng = random.Random(314)
    pre_force_pushes = 200
    pre_total: dict[str, float] = {u: 0.0 for u in ENTITIES}
    pre_count: dict[str, int] = {u: 0 for u in ENTITIES}
    for _ in range(pre_force_pushes):
        u = rng.choice(ENTITIES)
        amount = rng.uniform(1.0, 100.0)
        pre_count[u] += 1
        pre_total[u] += amount
        app.push("Tx", {"user_id": u, "amount": amount})

    # Sanity: pre-force aggs caught the pre-force events.
    for u in ENTITIES:
        if pre_count[u] == 0:
            continue
        row_a1 = app.get("StatsAOne", u)
        row_a2 = app.get("StatsATwo", u)
        assert row_a1.get("a1") == pre_count[u]
        assert abs(row_a2.get("a2", 0.0) - pre_total[u]) < 1e-6

    # --- branching set B: DIFFERENT tables on the same Tx source ------------
    @bv.table(key="user_id")
    def StatsBOne(tx: Tx):
        return tx.group_by("user_id").agg(b1=bv.count(window="forever"))

    @bv.table(key="user_id")
    def StatsBTwo(tx: Tx):
        return tx.group_by("user_id").agg(
            b2=bv.mean("amount", window="forever"),
        )

    # force=True replaces the whole pipeline on this Tx source.
    result = app.register(Tx, StatsBOne, StatsBTwo, force=True)
    assert result["status"] == "ok"

    # --- push more, then assert ONLY B sees them ----------------------------
    post_pushes = 300
    post_amounts: dict[str, list[float]] = {u: [] for u in ENTITIES}
    for _ in range(post_pushes):
        u = rng.choice(ENTITIES)
        amount = rng.uniform(1.0, 100.0)
        post_amounts[u].append(amount)
        app.push("Tx", {"user_id": u, "amount": amount})

    for u in ENTITIES:
        if not post_amounts[u]:
            continue
        b1_row = app.get("StatsBOne", u)
        b2_row = app.get("StatsBTwo", u)
        # B's count sees only the post-force events.
        assert b1_row.get("b1") == len(post_amounts[u]), (
            f"StatsBOne[{u}]: b1={b1_row.get('b1')} != post-only count {len(post_amounts[u])}"
        )
        # B's mean sees only post-force amounts.
        exp_mean = sum(post_amounts[u]) / len(post_amounts[u])
        assert abs(b2_row.get("b2", 0.0) - exp_mean) < 1e-3, (
            f"StatsBTwo[{u}]: b2={b2_row.get('b2')} != post-only mean {exp_mean}"
        )

    # Old A-tables must be GONE — re-querying them must return cold-start.
    # Per ADR-001 partial overturn + force_required semantics, force=True drops
    # the removed tables; a subsequent app.get on the dropped name surfaces as
    # cold-start (empty or None) or, on some transports, a RegistrationError /
    # not-found error. Accept either shape — the contract is "no stale data".
    for u in ENTITIES:
        if pre_count[u] == 0:
            continue
        try:
            row_a1 = app.get("StatsAOne", u)
        except Exception:
            row_a1 = {}
        # Cold-start equivalent OR no "a1" feature (table dropped).
        assert cold_start_equivalent(row_a1) or "a1" not in row_a1, (
            f"StatsAOne[{u}] should be dropped after force=True; got {row_a1!r}"
        )


# ---------------------------------------------------------------------------
# Test 5: overlapping where= predicates on a fan-out
# ---------------------------------------------------------------------------


def test_fan_out_with_overlapping_where_predicates(app: Any) -> None:
    """Tx → A (where ok=True) + B (where amount>=100); both can fire on one event.

    Verifies the apply loop visits every aggregation independently — a Tx with
    ok=True AND amount>=100 must increment BOTH A's and B's counts. An
    apply-loop short-circuit bug (e.g. "first matching agg wins") would show as
    A's count being correct but B's being short.
    """

    @bv.event
    class Tx:
        user_id: str
        amount: float
        ok: bool

    @bv.table(key="user_id")
    def StatsOk(tx: Tx):
        return tx.group_by("user_id").agg(
            n_ok=bv.count(window="forever", where=bv.col("ok") == True),  # noqa: E712
        )

    @bv.table(key="user_id")
    def StatsBig(tx: Tx):
        return tx.group_by("user_id").agg(
            n_big=bv.count(window="forever", where=bv.col("amount") >= 100),
        )

    app.register(Tx, StatsOk, StatsBig)

    rng = random.Random(315)
    n_ok: dict[str, int] = {u: 0 for u in ENTITIES}
    n_big: dict[str, int] = {u: 0 for u in ENTITIES}
    n_overlap: dict[str, int] = {u: 0 for u in ENTITIES}

    for _ in range(700):
        u = rng.choice(ENTITIES)
        amount = rng.uniform(0.0, 200.0)
        ok = rng.random() < 0.5
        if ok:
            n_ok[u] += 1
        if amount >= 100:
            n_big[u] += 1
        if ok and amount >= 100:
            n_overlap[u] += 1
        app.push("Tx", {"user_id": u, "amount": amount, "ok": ok})

    for u in ENTITIES:
        ok_row = app.get("StatsOk", u)
        big_row = app.get("StatsBig", u)
        assert ok_row.get("n_ok", 0) == n_ok[u], (
            f"StatsOk[{u}]: n_ok={ok_row.get('n_ok', 0)} != {n_ok[u]}"
        )
        assert big_row.get("n_big", 0) == n_big[u], (
            f"StatsBig[{u}]: n_big={big_row.get('n_big', 0)} != {n_big[u]}"
        )

    # Sanity: at least one entity had overlapping events (ok=True AND amount>=100)
    # — otherwise the test isn't actually exercising the overlap case.
    assert sum(n_overlap.values()) > 0, (
        "test seed didn't produce any overlapping events — broaden the seed range"
    )

    assert cold_start_equivalent(app.get("StatsOk", "unknown_ok_xx"))
    assert cold_start_equivalent(app.get("StatsBig", "unknown_big_xx"))
