"""Transport parity on derived-event pipelines (HTTP-JSON vs TCP).

PR #109 (``crates/beava-server/tests/wire_roundtrip_parity.rs``) covers
transport parity at the Rust integration layer with a complex schema
(``sum + where + windowed + top_k + n_unique``) but does NOT exercise
**derivation chains** — ``@bv.event def`` derivations with synthetic
fields. The SDK has its own register-side serialisation logic
(``python/beava/_app.py::_to_register_json`` + ``_descriptor_to_node``)
and an apply-time pre-extraction path in
``crates/beava-core/src/agg_apply.rs``; a regression in either could
surface as transport-asymmetric behaviour where HTTP and TCP produce
different results for the same derived-event schema.

These tests close that gap by registering / pushing / reading the same
derived-event pipeline through both transports against a SINGLE server
instance (booted with both ``http://`` and ``tcp://`` listeners via
``spawn_embedded_server``) and asserting exact feature-value equality.

Wire-spec note (per PR #109 + ``tcp_listener.rs:742``): ``OP_REGISTER``
on the TCP transport accepts only ``CT_JSON`` — never ``CT_MSGPACK``.
The Python ``TcpTransport`` follows that contract and sends register
payloads as JSON over both transports. ``OP_PUSH`` / ``OP_GET`` also
currently flow as ``CT_JSON`` on both transports from the Python SDK;
parity here therefore tests the **framing envelope + dispatcher path**
(HTTP body vs custom TCP frame) rather than the JSON-vs-msgpack content
encoding. The Rust-level msgpack-on-TCP fast-path is covered by PR #109.

Anti-pattern guard (D-05, USER-LOCKED): no mock-object references; every
test hits a real spawned subprocess.
"""
from __future__ import annotations

from typing import Any, Generator

import pytest

import beava as bv

from ._helpers import _engine_available

pytestmark = pytest.mark.skipif(
    not _engine_available(),
    reason="requires Phase 13.4 engine + Phase 13.5 SDK rewrite + Phase 13.5.1 transport-impl",
)


# ---------------------------------------------------------------------------
# Shared fixture: one server, two App handles (HTTP + TCP) on the same process.
# ---------------------------------------------------------------------------


@pytest.fixture
def http_and_tcp_apps() -> Generator[tuple[Any, Any], None, None]:
    """Spawn ONE beava subprocess, yield ``(http_app, tcp_app)`` against it.

    Both Apps talk to the same server-side state — register on either,
    push on either, read on either. Parity is asserted by exercising the
    transport pair against shared state.

    Teardown order: close both Apps, then SIGTERM the subprocess.
    """
    from beava._embed import spawn_embedded_server, teardown_process

    proc, http_url, tcp_url, _env = spawn_embedded_server(test_mode=True)
    http_app: Any | None = None
    tcp_app: Any | None = None
    try:
        http_app = bv.App(url=http_url).__enter__()
        tcp_app = bv.App(url=tcp_url).__enter__()
        yield http_app, tcp_app
    finally:
        for handle in (http_app, tcp_app):
            if handle is not None:
                try:
                    handle.close()
                except Exception:
                    pass
        teardown_process(proc)


# ---------------------------------------------------------------------------
# Test 1: three-hop derived-event chain
# ---------------------------------------------------------------------------


def test_derived_event_three_hop_chain_parity_http_vs_tcp(
    http_and_tcp_apps: tuple[Any, Any],
) -> None:
    """Tx → Cleaned → Enriched → @bv.table — drive 30 events through each
    transport, assert exact equality.

    Exercises the SDK register-side derivation flattener
    (``_app.py::_descriptor_to_node`` chain-flatten logic) and the
    apply-time pre-extraction path for a non-trivial 3-hop chain with a
    synthetic ``with_columns`` field at each derivation step.

    Wire-spec note: ``OP_REGISTER`` on TCP accepts ``CT_JSON`` only (per
    ``tcp_listener.rs``); push / get can be either content-type, but the
    SDK uses ``CT_JSON`` on both transports today.
    """
    http_app, tcp_app = http_and_tcp_apps

    @bv.event
    class Tx:
        user_id: str
        amount: float
        category: str

    @bv.event
    def Cleaned(tx: Tx):
        # First hop: synthesize a ``flag`` column derived from amount.
        return tx.with_columns(flag=bv.lit("ok"))

    @bv.event
    def Enriched(c: Cleaned):
        # Second hop: synthesize a ``doubled`` column.
        return c.with_columns(doubled=bv.col("amount") + bv.col("amount"))

    @bv.table(key="user_id")
    def EnrichedByUser(e: Enriched):
        return e.group_by("user_id").agg(
            n=bv.count(window="forever"),
            sum_doubled=bv.sum("doubled", window="forever"),
        )

    # ── HTTP leg ──────────────────────────────────────────────────────────
    http_app.register(Tx, Cleaned, Enriched, EnrichedByUser)
    for i in range(30):
        http_app.push(
            "Tx",
            {"user_id": "alice", "amount": 1.0 + i, "category": "cat_a"},
        )
    http_row = http_app.get("EnrichedByUser", "alice")

    # Reset shared state and replay the same stream over the TCP transport.
    tcp_app.reset()
    tcp_app.register(Tx, Cleaned, Enriched, EnrichedByUser)
    for i in range(30):
        tcp_app.push(
            "Tx",
            {"user_id": "alice", "amount": 1.0 + i, "category": "cat_a"},
        )
    tcp_row = tcp_app.get("EnrichedByUser", "alice")

    # Sanity: pipeline produced the expected feature shape.
    assert set(http_row.keys()) == {"n", "sum_doubled"}, (
        f"HTTP row must have {{'n','sum_doubled'}}; got {http_row!r}"
    )
    assert http_row["n"] == 30, f"HTTP n=30 expected; got {http_row!r}"
    # sum_doubled = sum(2*(1..30)) = 2 * 465 = 930
    assert abs(http_row["sum_doubled"] - 930.0) < 1e-9, (
        f"HTTP sum_doubled=930.0 expected; got {http_row!r}"
    )

    # Parity: TCP row must match HTTP row exactly (key set + numeric values).
    assert tcp_row == http_row, (
        f"transport asymmetry on 3-hop derived chain: "
        f"http={http_row!r} tcp={tcp_row!r}"
    )

    # Cross-transport read parity: read via the OTHER App after the TCP push.
    cross = http_app.get("EnrichedByUser", "alice")
    assert cross == tcp_row, (
        f"cross-transport read mismatch: http_read={cross!r} tcp_read={tcp_row!r}"
    )


# ---------------------------------------------------------------------------
# Test 2: with_columns synthetic field — PR #106 bug class via transport lens
# ---------------------------------------------------------------------------


def test_with_columns_synthetic_field_parity(
    http_and_tcp_apps: tuple[Any, Any],
) -> None:
    """``with_columns(rate=col('a')/col('b'))`` over HTTP must equal over TCP.

    Retests the PR #106 synthetic-field bug class through the transport-
    parity lens: an asymmetry here means one transport's pre-extraction
    path is dropping or mis-typing the synthesized column.

    Wire-spec note: register over either transport is ``CT_JSON`` only.
    """
    http_app, tcp_app = http_and_tcp_apps

    @bv.event
    class Tele:
        user_id: str
        a: float
        b: float

    @bv.event
    def WithRate(tele: Tele):
        # ``rate = a / b`` — synthetic f64 column derived from two source fields.
        return tele.with_columns(rate=bv.col("a") / bv.col("b"))

    @bv.table(key="user_id")
    def UserRate(w: WithRate):
        return w.group_by("user_id").agg(
            n=bv.count(window="forever"),
            sum_rate=bv.sum("rate", window="forever"),
        )

    # Deterministic stream: a=2,4,6,...,40 ; b=2 → rate=1,2,3,...,20.
    # sum_rate = 1+2+...+20 = 210. n = 20.
    payloads = [
        {"user_id": "alice", "a": float(2 * (i + 1)), "b": 2.0} for i in range(20)
    ]

    http_app.register(Tele, WithRate, UserRate)
    for p in payloads:
        http_app.push("Tele", p)
    http_row = http_app.get("UserRate", "alice")

    tcp_app.reset()
    tcp_app.register(Tele, WithRate, UserRate)
    for p in payloads:
        tcp_app.push("Tele", p)
    tcp_row = tcp_app.get("UserRate", "alice")

    assert http_row["n"] == 20, f"HTTP n=20 expected; got {http_row!r}"
    assert abs(http_row["sum_rate"] - 210.0) < 1e-9, (
        f"HTTP sum_rate=210.0 expected; got {http_row!r}"
    )

    assert tcp_row == http_row, (
        f"transport asymmetry on with_columns synthetic field: "
        f"http={http_row!r} tcp={tcp_row!r}"
    )


# ---------------------------------------------------------------------------
# Test 3: fan-out branching — one source feeds two derivations + two tables
# ---------------------------------------------------------------------------


def test_fan_out_branching_parity(
    http_and_tcp_apps: tuple[Any, Any],
) -> None:
    """Source with two branched derivations + two tables; all four downstream
    tables must agree across transports.

    Exercises the SDK's multi-descriptor register payload assembly (two
    independent ``@bv.event def`` branches sharing one root) and the
    apply-time fan-out — the same event must populate BOTH branches'
    downstream tables identically on both transports.
    """
    http_app, tcp_app = http_and_tcp_apps

    @bv.event
    class Order:
        user_id: str
        amount: float
        status: str

    # Branch A: filter to status=="paid"; aggregate count + sum.
    @bv.event
    def PaidOrders(o: Order):
        return o.filter(bv.col("status") == bv.lit("paid"))

    # Branch B: tag with a synthetic column; aggregate count + sum.
    @bv.event
    def TaggedOrders(o: Order):
        return o.with_columns(tier=bv.lit("standard"))

    @bv.table(key="user_id")
    def PaidByUser(p: PaidOrders):
        return p.group_by("user_id").agg(
            paid_n=bv.count(window="forever"),
            paid_sum=bv.sum("amount", window="forever"),
        )

    @bv.table(key="user_id")
    def TaggedByUser(t: TaggedOrders):
        return t.group_by("user_id").agg(
            all_n=bv.count(window="forever"),
            all_sum=bv.sum("amount", window="forever"),
        )

    # 30 events: alternating paid/pending status, monotonic amount.
    stream = [
        {
            "user_id": "alice",
            "amount": float(i + 1),
            "status": "paid" if i % 2 == 0 else "pending",
        }
        for i in range(30)
    ]

    http_app.register(
        Order, PaidOrders, TaggedOrders, PaidByUser, TaggedByUser
    )
    for p in stream:
        http_app.push("Order", p)
    http_paid = http_app.get("PaidByUser", "alice")
    http_tagged = http_app.get("TaggedByUser", "alice")

    tcp_app.reset()
    tcp_app.register(
        Order, PaidOrders, TaggedOrders, PaidByUser, TaggedByUser
    )
    for p in stream:
        tcp_app.push("Order", p)
    tcp_paid = tcp_app.get("PaidByUser", "alice")
    tcp_tagged = tcp_app.get("TaggedByUser", "alice")

    # Sanity: branch A — 15 paid orders, sum = 1+3+5+...+29 = 225.
    assert http_paid["paid_n"] == 15, (
        f"HTTP PaidByUser n=15 expected; got {http_paid!r}"
    )
    assert abs(http_paid["paid_sum"] - 225.0) < 1e-9, (
        f"HTTP PaidByUser sum=225 expected; got {http_paid!r}"
    )
    # Sanity: branch B — 30 events, sum = 1+2+...+30 = 465.
    assert http_tagged["all_n"] == 30, (
        f"HTTP TaggedByUser n=30 expected; got {http_tagged!r}"
    )
    assert abs(http_tagged["all_sum"] - 465.0) < 1e-9, (
        f"HTTP TaggedByUser sum=465 expected; got {http_tagged!r}"
    )

    # Parity across BOTH branches.
    assert tcp_paid == http_paid, (
        f"transport asymmetry on fan-out branch A (PaidByUser): "
        f"http={http_paid!r} tcp={tcp_paid!r}"
    )
    assert tcp_tagged == http_tagged, (
        f"transport asymmetry on fan-out branch B (TaggedByUser): "
        f"http={http_tagged!r} tcp={tcp_tagged!r}"
    )


# ---------------------------------------------------------------------------
# Test 4: register force=True replaces the schema; no cross-schema residue
# ---------------------------------------------------------------------------


def test_register_force_parity(
    http_and_tcp_apps: tuple[Any, Any],
) -> None:
    """Register schema X via HTTP; force-register schema Y via TCP; push and
    read via the other transport; assert no schema-X residue.

    Locks register-replace semantics across transports: a ``force=True``
    re-register from one transport must fully take effect for reads from
    the other transport (no stale schema-X feature names leaking through).
    """
    http_app, tcp_app = http_and_tcp_apps

    # Schema X — single ``c`` feature.
    @bv.event
    class TxV1:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserTxV1(tx: TxV1):
        return tx.group_by("user_id").agg(c=bv.count(window="forever"))

    http_app.register(TxV1, UserTxV1)
    http_app.push("TxV1", {"user_id": "alice", "amount": 1.0})

    # Reset clears state + registry on the v0 default path (Phase 13.4
    # OP_RESET D-03). The follow-up force-register over TCP installs a new
    # schema Y with the SAME table name but different feature columns.
    tcp_app.reset()

    # Schema Y — ``c2`` + ``s2``, same table name ``UserTxV1`` to confirm
    # there is no schema-X residue post-force.
    @bv.event
    class TxV2:
        user_id: str
        amount: float

    @bv.table(key="user_id")
    def UserTxV1(tx: TxV2):  # noqa: F811 — intentional re-bind to test name reuse
        return tx.group_by("user_id").agg(
            c2=bv.count(window="forever"),
            s2=bv.sum("amount", window="forever"),
        )

    tcp_app.register(TxV2, UserTxV1, force=True)

    # Push via HTTP after force-register over TCP — events must route to
    # schema Y.
    for _ in range(5):
        http_app.push("TxV2", {"user_id": "alice", "amount": 4.0})

    # Read via TCP — must see schema-Y columns ONLY (no schema-X ``c``).
    tcp_row = tcp_app.get("UserTxV1", "alice")
    assert set(tcp_row.keys()) == {"c2", "s2"}, (
        f"force-register residue: TCP read returned non-Y keys: {tcp_row!r}"
    )
    assert tcp_row["c2"] == 5, f"TCP c2=5 expected; got {tcp_row!r}"
    assert abs(tcp_row["s2"] - 20.0) < 1e-9, (
        f"TCP s2=20.0 expected; got {tcp_row!r}"
    )

    # And via HTTP — same schema-Y row from the other transport.
    http_row = http_app.get("UserTxV1", "alice")
    assert http_row == tcp_row, (
        f"force-register cross-transport read mismatch: "
        f"http={http_row!r} tcp={tcp_row!r}"
    )
