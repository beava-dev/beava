"""Phase 3 SDK REGISTER-compile bench (plan 05.5-05).

Measures the full Python-side REGISTER JSON compile pipeline for a realistic
10-descriptor DAG (validate_descriptors + topo_sort + _to_register_json per
descriptor + json.dumps().encode()).  No network I/O.

NOTE: No 'from __future__ import annotations' — @bv.event function-form reads
param.annotation directly at decoration time (must be the live descriptor object,
not a string).  All 10 descriptors are defined at module scope.

Plan 12.7-06: Per `project_v0_events_only_scope` (locked 2026-04-30) the
4 @bv.table descriptors were replaced with 4 additional event sources +
derivations to keep the 10-descriptor DAG shape. Tables return in v0.1+
if/when justified by demand.

Run:
    pytest tests/bench_register_compile.py -v
    pytest tests/bench_register_compile.py --benchmark-only
    pytest -m bench
"""

import json
from typing import Any

import pytest
from beava._validate import topo_sort, validate_descriptors

import beava as bv

# ---------------------------------------------------------------------------
# 10-descriptor DAG — module-level so @bv.event function-form annotations
# resolve to live descriptor objects (not deferred strings).
# Mix: 3 event sources, 3 event derivations, 2 table sources, 2 table derivations.
# ---------------------------------------------------------------------------

# -- event sources (3) --


# Plan 12.6-08: event_time field removed from fixtures per the no-event-time
# pivot. The server stamps wall-clock arrival time on every push.
@bv.event
class BenchTx:
    user_id: str
    amount: float
    status: str
    ts: int


@bv.event
class BenchLogin:
    user_id: str
    ip: str
    ts: int


@bv.event
class BenchPageView:
    user_id: str
    url: str
    ts: int


# -- event derivations (3): filter → with_columns → select chain on BenchTx --


@bv.event
def bench_tx_positive(src: BenchTx):  # type: ignore[no-untyped-def]
    return src.filter(bv.col("amount") > 0)


@bv.event
def bench_tx_big(src: bench_tx_positive):  # type: ignore[no-untyped-def]
    return src.with_columns(is_big=bv.col("amount") > 500)


@bv.event
def bench_tx_final(src: bench_tx_big):  # type: ignore[no-untyped-def]
    return src.select("user_id", "amount", "is_big")


# -- additional event sources (2) — Plan 12.7-06: replace table sources --


@bv.event
class BenchUserBaseline:
    user_id: str
    lifetime_spend: float
    last_seen: int


@bv.event
class BenchUserFlags:
    user_id: str
    is_vip: bool


# -- additional event derivations (2) — Plan 12.7-06: replace table derivations --


@bv.event
def bench_user_filtered(base: BenchUserBaseline):  # type: ignore[no-untyped-def]
    return base.filter(bv.col("lifetime_spend") > 0)


@bv.event
class BenchLoginHistory:
    user_id: str
    ip: str
    count: int


# Ordered list — topo_sort will reorder as needed; input order here is valid.
_DESCRIPTORS: list[Any] = [
    BenchTx,
    BenchLogin,
    BenchPageView,
    bench_tx_positive,
    bench_tx_big,
    bench_tx_final,
    BenchUserBaseline,
    BenchUserFlags,
    bench_user_filtered,
    BenchLoginHistory,
]

assert len(_DESCRIPTORS) == 10, f"Expected 10 descriptors, got {len(_DESCRIPTORS)}"


# ---------------------------------------------------------------------------
# Bench
# ---------------------------------------------------------------------------


@pytest.mark.bench
def test_register_compile_10_descriptors(benchmark: Any) -> None:
    """Benchmark the full SDK-side REGISTER JSON compile pipeline.

    Measures: validate_descriptors + topo_sort + _to_register_json (per descriptor)
    + json.dumps().encode() for a 10-descriptor DAG.  No network I/O.
    """
    assert len(_DESCRIPTORS) == 10

    def compile_register_json() -> bytes:
        errs = validate_descriptors(_DESCRIPTORS)
        assert errs == []
        sorted_descs = topo_sort(_DESCRIPTORS)
        payload = {"nodes": [d._to_register_json() for d in sorted_descs]}
        return json.dumps(payload).encode("utf-8")

    result = benchmark(compile_register_json)
    # Sanity: output is non-empty JSON bytes with expected structure.
    assert result.startswith(b"{")
    assert b'"nodes"' in result
