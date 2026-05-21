# Decay-Family Aggregation Operators

The 6 decay-family ops cover **exponentially-weighted statistics** (EWMA / EWVar / EW Z-Score), **forward-decay accumulators** (Cormode 2009), and **time-weighted averaging**. Five of the six use a `half_life` parameter to set an exponential decay rate; `bv.twa` uses `window` instead because it integrates held-time exactly rather than fading.

| Op | Required kwarg | Returns | CPU tier | Notes |
|----|----------------|---------|----------|-------|
| [`bv.ewma`](./ewma.md) (alias `bv.ema`) | `half_life` | `f64` or `null` | Tier 1 | Exponentially-weighted mean. |
| [`bv.ewvar`](./ewvar.md) | `half_life` | `f64` or `null` | Tier 1 | Exponentially-weighted variance — companion second-moment to EWMA. |
| [`bv.ew_zscore`](./ew_zscore.md) | `half_life` | `f64` or `null` | Tier 1 | Current-event z-score against EWMA / EWVar baseline; the standard drift-aware anomaly primitive. |
| [`bv.decayed_sum`](./decayed_sum.md) | `half_life` | `f64` or `null` | Tier 1 | Cormode forward-decay sum — recency-weighted total that converges to a stable steady-state. |
| [`bv.decayed_count`](./decayed_count.md) | `half_life` | `f64` or `null` | Tier 1 | Same primitive without `field` — answers "how active recently?". The cheapest decay op. |
| [`bv.twa`](./twa.md) | `window` | `f64` or `null` | Tier 1 | Time-weighted average for irregularly-sampled gauge fields. |

All six are `O(1)` memory per entity and Tier 1 CPU per [cost-class.md](../cost-class.md). The five EW-decay ops share a state shape of `(value, last_now_ms, initialized)` plus per-op extras; `bv.twa` carries `(sum_v_dt, sum_dt, last_v, last_t, initialized)`.

## Key invariants

- **Server processing-time only.** Decay coefficients use `Δt = now_ms() at this matching event - now_ms() at the previous matching event` per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md) (post-2026-05-19: partially overturned by ADR-004 for v0.1 bucketing). Producers cannot influence decay via payload fields. Late events (`Δt ≤ 0`) fall back to an unweighted blend and do **not** advance `last_now_ms`.
- **`half_life` is mandatory and finite for the EW family.** `bv.ewma`, `bv.ewvar`, `bv.ew_zscore`, `bv.decayed_sum`, `bv.decayed_count` all reject `"forever"` at SDK-helper-call time (regex `[1-9]\d*(?:ms|s|m|h|d)$`); use the corresponding lifetime ops ([`bv.first`](../point-ordinal/first.md), [`bv.var`](../core/var.md), [`bv.z_score`](../velocity/z_score.md), [`bv.sum`](../core/sum.md) `window="forever"`, [`bv.count`](../core/count.md) `window="forever"`) when an undecayed lifetime reading is what you want.
- **`bv.twa` accepts `window="forever"`.** Time-weighted average integrates held-time exactly, so the lifetime form is well-defined; per-op classification at `crates/beava-core/src/register_validate.rs` (~line 436) classifies all six ops as `O(1)` lifetime-bound (`OpLifetimeBound::O1`).
- **Reads do not decay forward.** `app.get(...)` returns the running statistic as of the **last matching event** — the helper does not re-decay the value to query time. (Re-decaying on read would mutate state on every `get`, breaking idempotence.)
- **Cold-start returns `null`** for all six ops. `bv.ewvar` and `bv.ew_zscore` additionally return `null` after only one matching event (variance is `0`; no spread to normalize against).
- **Cold-entity eviction (`@bv.event(cold_after=...)`)** drops the underlying state per the Redis-TTL pattern (V0-MEM-GOV-01); decay ops rebuild fresh on the next post-eviction matching event.

## When to use which

- **Smoothed running mean** that adapts to drift → [`bv.ewma`](./ewma.md). Pick `half_life` ≈ the timescale of the behaviour you care about.
- **Smoothed running variance** for anomaly scoring or volatility tracking → [`bv.ewvar`](./ewvar.md), usually paired with [`bv.ew_zscore`](./ew_zscore.md).
- **Recency-weighted total** (e.g. "spend in roughly the last hour") → [`bv.decayed_sum`](./decayed_sum.md). Steady-state ≈ `rate * value * half_life / ln(2)`.
- **Recency-weighted activity rate** → [`bv.decayed_count`](./decayed_count.md). Steady-state ≈ `rate * half_life / ln(2)`.
- **True time-weighted average** for gauges sampled at irregular intervals → [`bv.twa`](./twa.md).

> Note: `bv.rate_of_change` is **not** in the decay family — it lives under [velocity/rate_of_change.md](../velocity/rate_of_change.md) per the Phase 9 op classification (it computes a slope across two adjacent windows, not an exponentially-weighted statistic). Polished by [Plan 13.0-09](../../../.planning/phases/13.0-design-contract-spec-docs/).

## See also

- [Operator catalog index](../index.md) — full 53-op catalogue (decay is the 6-op family)
- [cost-class.md](../cost-class.md) — per-op CPU tier metadata (all six decay ops are Tier 1)
- [Velocity family](../velocity/) — sibling family for slope-style and inter-arrival statistics, including `rate_of_change`
- [Core family](../core/) — fixed-window arithmetic mean / variance / sum / count counterparts
- [shared.md window grammar](../../sdk-api/shared.md) — duration-string format (`\d+(ms\|s\|m\|h\|d)` and the `"forever"` literal)
- Per-operator memory governance: [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — every lifetime aggregation operator declares a finite per-entity memory ceiling at register-time
- [Pipeline DSL compilation rules](../../pipeline-dsl/compilation-rules.md) — how `bv.<op>(...)` calls compile to JSON wire form
