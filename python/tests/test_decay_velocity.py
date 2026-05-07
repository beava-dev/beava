"""Phase 9 — Python SDK tests for decay/velocity helpers.

Covers AGG-DECAY-01..07, AGG-VEL-01..08, AGG-Z-01.
"""

from __future__ import annotations

import pytest

import beava as bv

# ─── Decay helpers ─────────────────────────────────────────────────────────────


class TestDecayHelpers:
    def test_ewma_produces_descriptor(self):
        d = bv.ewma("amount", half_life="5m")
        assert d.op == "ewma"
        assert d.field == "amount"
        assert d.half_life == "5m"

    def test_ema_is_alias_for_ewma(self):
        d = bv.ema("amount", half_life="5m")
        assert d.op == "ewma"
        assert d.field == "amount"
        assert d.half_life == "5m"

    def test_ewma_rejects_malformed_half_life(self):
        with pytest.raises(ValueError):
            bv.ewma("amount", half_life="foo")

    def test_ewma_rejects_forever_half_life(self):
        with pytest.raises(ValueError):
            bv.ewma("amount", half_life="forever")

    def test_ewvar_produces_descriptor(self):
        d = bv.ewvar("amount", half_life="1h")
        assert d.op == "ewvar"
        assert d.half_life == "1h"

    def test_ew_zscore_produces_descriptor(self):
        d = bv.ew_zscore("amount", half_life="30s")
        assert d.op == "ew_zscore"
        assert d.half_life == "30s"

    def test_decayed_sum_produces_descriptor(self):
        d = bv.decayed_sum("amount", half_life="10m")
        assert d.op == "decayed_sum"
        assert d.half_life == "10m"

    def test_decayed_count_no_field(self):
        d = bv.decayed_count(half_life="10m")
        assert d.op == "decayed_count"
        assert d.field is None

    def test_twa_requires_window(self):
        d = bv.twa("gauge", window="1h")
        assert d.op == "twa"
        assert d.window == "1h"
        with pytest.raises(TypeError):
            bv.twa("gauge")  # type: ignore[call-arg]


# ─── Velocity helpers ──────────────────────────────────────────────────────────


class TestVelocityHelpers:
    def test_rate_of_change(self):
        d = bv.rate_of_change("price", window="5m")
        assert d.op == "rate_of_change"
        assert d.window == "5m"

    def test_inter_arrival_stats_no_field(self):
        d = bv.inter_arrival_stats(window="5m")
        assert d.op == "inter_arrival_stats"
        assert d.field is None

    def test_burst_count_requires_sub_window(self):
        d = bv.burst_count(window="5m", sub_window="10s")
        assert d.op == "burst_count"
        assert d.sub_window == "10s"
        with pytest.raises(ValueError):
            bv.burst_count(window="5m", sub_window="bad")

    def test_delta_from_prev_no_window(self):
        d = bv.delta_from_prev("price")
        assert d.op == "delta_from_prev"
        assert d.window is None

    def test_trend(self):
        d = bv.trend("price", window="1h")
        assert d.op == "trend"

    def test_trend_residual(self):
        d = bv.trend_residual("price", window="1h")
        assert d.op == "trend_residual"

    def test_outlier_count_default_sigma(self):
        d = bv.outlier_count("amount", window="5m")
        assert d.op == "outlier_count"
        assert d.sigma == 3.0

    def test_outlier_count_custom_sigma(self):
        d = bv.outlier_count("amount", window="5m", sigma=2.5)
        assert d.sigma == 2.5

    def test_outlier_count_rejects_nonpositive_sigma(self):
        with pytest.raises(ValueError):
            bv.outlier_count("amount", window="5m", sigma=0)

    def test_value_change_count(self):
        d = bv.value_change_count("status", window="1h")
        assert d.op == "value_change_count"


# ─── Z-Score ───────────────────────────────────────────────────────────────────


class TestZScore:
    def test_z_score_produces_descriptor(self):
        d = bv.z_score("amount", baseline_window="7d")
        assert d.op == "z_score"
        assert d.window == "7d"


# ─── Wire encoding ─────────────────────────────────────────────────────────────


class TestWireEncoding:
    def test_ewma_to_agg_spec_includes_half_life(self):
        d = bv.ewma("amount", half_life="5m")
        spec = d.to_agg_spec()
        assert spec["op"] == "ewma"
        assert spec["params"]["half_life"] == "5m"
        assert spec["params"]["field"] == "amount"

    def test_burst_count_to_agg_spec_includes_sub_window(self):
        d = bv.burst_count(window="5m", sub_window="10s")
        spec = d.to_agg_spec()
        assert spec["params"]["sub_window"] == "10s"

    def test_outlier_count_to_agg_spec_includes_sigma(self):
        d = bv.outlier_count("amount", window="5m", sigma=2.5)
        spec = d.to_agg_spec()
        assert spec["params"]["sigma"] == 2.5

    def test_delta_from_prev_spec_has_no_window_key(self):
        d = bv.delta_from_prev("price")
        spec = d.to_agg_spec()
        assert "window" not in spec["params"]
