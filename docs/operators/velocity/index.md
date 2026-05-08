# Velocity / Trend / Z-Score Aggregation Operators

The 9 velocity-family ops cover **rate-of-change** between adjacent events, **behavioural cadence** (inter-arrival statistics, burst-count peaks), **value-trajectory** (delta-from-prev, trend slope, trend residual), **outlier flags** (sigma-bound counts), **value-flip counts**, and **entity-level z-scores**. Together they answer "is this entity moving, drifting, spiking, or breaking from its own history?".

| Op | Required kwarg(s) | Returns | CPU tier | Notes |
|----|-------------------|---------|----------|-------|
| [`bv.rate_of_change`](./rate_of_change.md) | `window` | `f64` or `null` | Tier 1 | Two-event delta divided by `Δt_ms`. |
| [`bv.inter_arrival_stats`](./inter_arrival_stats.md) | `window` | `f64` or `null` | Tier 1 | v0 emits `mean_ms`; v0.1+ widens to `{mean_ms, stddev_ms, cv}`. Field-less. |
| [`bv.burst_count`](./burst_count.md) | `window`, `sub_window` | `i64` | Tier 1 | Max events in any 1 of 64 sliding sub-window slots. Field-less. |
| [`bv.delta_from_prev`](./delta_from_prev.md) | — | `f64` or `null` | Tier 1 | Lifetime-only; absolute jump (no `Δt`). |
| [`bv.trend`](./trend.md) | `window` | `f64` or `null` | Tier 1 | OLS slope of `(now_ms, field)`; smoother than `rate_of_change`. |
| [`bv.trend_residual`](./trend_residual.md) | `window` | `f64` or `null` | Tier 1 | Latest value minus its trend-line prediction; shares state with `bv.trend`. |
| [`bv.outlier_count`](./outlier_count.md) | `window` (`sigma=3.0` default) | `i64` | Tier 2 | Welford-baseline + sigma-threshold count; the only Tier 2 velocity op (`sqrt()` per event). |
| [`bv.value_change_count`](./value_change_count.md) | `window` | `i64` | Tier 1 | Counts adjacent flips, not net distinct values. |
| [`bv.z_score`](./z_score.md) | `baseline_window` | `f64` or `null` | Tier 1 | Current event's deviation against the entity's running `(mean, stddev)`. |

All 9 are `O(1)` memory per entity — see the per-op page Complexity section for byte-level state shapes. Eight of nine are Tier 1; only [`bv.outlier_count`](./outlier_count.md) is Tier 2 (one `sqrt()` per event for the threshold test). Per [cost-class.md](../cost-class.md), the entire velocity family sits in the fast-update tier.

## Key invariants

- **Server processing-time only.** Every windowed dispatch in this family bins events by `now_ms()` per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md). Producers cannot influence binning via payload fields; there is no event-time concept. Late events (`Δt ≤ 0`) are clamped to `0` (in `inter_arrival_stats`) or skipped (in `rate_of_change`).
- **`window=` is the canonical kwarg name** for 7 of 9. [`bv.delta_from_prev`](./delta_from_prev.md) is **lifetime-only** (no `window=` — it just diffs the most recent two values regardless of elapsed time). [`bv.z_score`](./z_score.md) uses the SDK ergonomic name **`baseline_window=`** to make the "baseline-against-which-the-current-event-is-scored" intent explicit; the wire-form `params` field is still `"window"`.
- **`burst_count` requires both `window=` AND `sub_window=`.** `sub_window` partitions the outer window into bucketed slots; the helper rejects missing or malformed `sub_window=` at SDK call time and the server returns structured error `aggregation_invalid_sub_window` if it reaches `register_validate.rs`.
- **`outlier_count` defaults to `sigma=3.0`** — the classic three-sigma rule. Tighten to `sigma=2.0` for ~5% tail; loosen to `sigma=4.0` for ~0.006% tail. The op also warms up for `MIN_BASELINE_N = 5` matching events before firing the outlier check, to avoid spurious early-stream increments.
- **Lifetime mode (`window="forever"`) is allowed for all 9.** Per `crates/beava-core/src/register_validate.rs` (~line 439–449) every velocity-family op is classified `OpLifetimeBound::O1` — finite per-entity memory ceiling, register-time accepted in lifetime mode. For long-lifetime trend / z-score tracking, mind the FP-precision caveats noted on the per-op pages and prefer a fixed `window=` ≤ 1d on busy entities, or fall back to [`bv.ewma`](../decay/ewma.md) / [`bv.ew_zscore`](../decay/ew_zscore.md) which carry a bounded-magnitude state by design.
- **Cold-start returns `null` (or `0` for the counters).** `rate_of_change`, `inter_arrival_stats`, `delta_from_prev`, `trend`, `trend_residual`, `z_score` return `null` until enough events accumulate (typically `n >= 2`); `burst_count`, `outlier_count`, `value_change_count` return `0` and only ever increment.
- **Cold-entity eviction (`@bv.event(cold_after=...)`)** drops the underlying state per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md); velocity ops rebuild fresh on the next post-eviction matching event.

## When to use which

- **"Is the latest value out of line?"** → [`bv.z_score`](./z_score.md) (magnitude) or [`bv.outlier_count`](./outlier_count.md) (count).
- **"Is this signal accelerating?"** → [`bv.rate_of_change`](./rate_of_change.md) for two-event reactive; [`bv.trend`](./trend.md) for window-wide smoothed slope.
- **"Did the latest event break from a directional pattern?"** → [`bv.trend_residual`](./trend_residual.md).
- **"How big was the most recent jump?"** → [`bv.delta_from_prev`](./delta_from_prev.md).
- **"How busy is this entity?"** → [`bv.inter_arrival_stats`](./inter_arrival_stats.md) for cadence; [`bv.burst_count`](./burst_count.md) for peak.
- **"How often does this value flip?"** → [`bv.value_change_count`](./value_change_count.md).

> Note: per [REQUIREMENTS.md](../../../.planning/REQUIREMENTS.md), `z_score` is family `AGG-Z-*` — it lives here in `velocity/` per RESEARCH §3 directory layout (entity-level statistics that pair naturally with the velocity / trend / outlier ops). [`bv.rate_of_change`](./rate_of_change.md) is the canonical v0 velocity-family op per RESEARCH §5 — it lives here, **not** in [decay/](../decay/), because it computes a slope across two adjacent events rather than an exponentially-weighted statistic.

## See also

- [Operator catalog index](../index.md) — full 53-op catalogue (velocity is the 9-op family)
- [cost-class.md](../cost-class.md) — per-op CPU tier metadata (8 Tier 1 + 1 Tier 2 in this family)
- [Decay family](../decay/) — sibling family for exponentially-weighted statistics; [`bv.ew_zscore`](../decay/ew_zscore.md) is the drift-aware counterpart to [`bv.z_score`](./z_score.md), and [`bv.ewma`](../decay/ewma.md) is the smoothed counterpart to the underlying signals fed into [`bv.trend`](./trend.md) / [`bv.rate_of_change`](./rate_of_change.md)
- [Sketch family](../sketch/) — sibling family for cardinality / quantile / categorical estimators; [`bv.entropy`](../sketch/entropy.md) and [`bv.n_unique`](../sketch/n_unique.md) are non-numeric-friendly anomaly primitives
- [Recency family](../recency/) — sibling family for "when did this entity last act?" — [`bv.streak`](../recency/streak.md) pairs naturally with [`bv.value_change_count`](./value_change_count.md) (consecutive matches vs. flips)
- [shared.md window grammar](../../sdk-api/shared.md) — duration-string format (`\d+(ms\|s\|m\|h\|d)` and the `"forever"` literal)
- Per-operator memory governance: [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — every lifetime aggregation operator declares a finite per-entity memory ceiling at register-time
- [Pipeline DSL compilation rules](../../pipeline-dsl/compilation-rules.md) — how `bv.<op>(...)` calls compile to JSON wire form
