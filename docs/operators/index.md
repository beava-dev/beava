# Operator Catalog

> 54 operator pages (53 unique AggKind variants + `ema` alias documented inline in `ewma.md`), across 7 family subdirectories.

Each operator page follows the same 9-section template (Signature / Description / Parameters / Returns / Complexity / Examples / Wire / Edge cases / See also). Renamed ops (per [ADR-002](../../.planning/decisions/ADR-002-polars-op-rename.md)) use the new Polars-convention name as filename + H1; each carries a "Previously called `bv.<old>`" note for searchability.

> **Per-entity vs global aggregation:** All 53 operators work with both per-entity (`@bv.table(key="user_id")`) and global (`@bv.table` with no `key=`, per [ADR-003](../../.planning/decisions/ADR-003-global-aggregation-and-bv-lit.md)) aggregation. Op semantics are identical — only the state-keying dimension differs. See [`docs/concepts/global-aggregation.md`](../concepts/global-aggregation.md) for when to use which.

## Core (8)

| Op | Description |
|----|-------------|
| [`bv.count`](./core/count.md) | Event count over a window or lifetime. |
| [`bv.sum`](./core/sum.md) | Sum of a numeric field. |
| [`bv.mean`](./core/mean.md) *(renamed from `bv.avg` per ADR-002)* | Arithmetic mean of a numeric field. |
| [`bv.min`](./core/min.md) | Minimum value of a numeric field. |
| [`bv.max`](./core/max.md) | Maximum value of a numeric field. |
| [`bv.var`](./core/var.md) *(renamed from `bv.variance` per ADR-002)* | Sample variance via Welford. |
| [`bv.std`](./core/std.md) *(renamed from `bv.stddev` per ADR-002)* | Standard deviation (sqrt of variance). |
| [`bv.ratio`](./core/ratio.md) | Count matching predicate divided by total count. |

## Sketch (5)

| Op | Description |
|----|-------------|
| [`bv.n_unique`](./sketch/n_unique.md) *(renamed from `bv.count_distinct` per ADR-002)* | HLL cardinality estimate. |
| [`bv.quantile`](./sketch/quantile.md) *(renamed from `bv.percentile` per ADR-002)* | DDSketch-backed quantile estimator. |
| [`bv.top_k`](./sketch/top_k.md) | SpaceSaving top-K most-frequent values. |
| [`bv.bloom_member`](./sketch/bloom_member.md) | Bloom-filter ever-seen membership test. |
| [`bv.entropy`](./sketch/entropy.md) | Shannon entropy over categorical distribution. |

## Point / ordinal (5)

| Op | Description |
|----|-------------|
| [`bv.first`](./point-ordinal/first.md) | First observed value. |
| [`bv.last`](./point-ordinal/last.md) | Most recent value by arrival order. |
| [`bv.first_n`](./point-ordinal/first_n.md) | First N values. |
| [`bv.last_n`](./point-ordinal/last_n.md) | Last N values. |
| [`bv.lag`](./point-ordinal/lag.md) | Value n events ago. |

## Recency (10)

| Op | Description |
|----|-------------|
| [`bv.first_seen`](./recency/first_seen.md) | First-seen server arrival timestamp. |
| [`bv.last_seen`](./recency/last_seen.md) | Last-seen server arrival timestamp. |
| [`bv.age`](./recency/age.md) | Milliseconds since first_seen. |
| [`bv.has_seen`](./recency/has_seen.md) | Boolean ever-matched predicate. |
| [`bv.time_since`](./recency/time_since.md) | Milliseconds since last matching event. |
| [`bv.time_since_last_n`](./recency/time_since_last_n.md) | Milliseconds since kth most recent matching event. |
| [`bv.streak`](./recency/streak.md) | Length of current consecutive matching streak. |
| [`bv.max_streak`](./recency/max_streak.md) | Longest streak length ever observed. |
| [`bv.negative_streak`](./recency/negative_streak.md) | Length of current consecutive non-matching streak. |
| [`bv.first_seen_in_window`](./recency/first_seen_in_window.md) | Bloom + timestamp: is this value new in window N? |

## Decay (6)

| Op | Description |
|----|-------------|
| [`bv.ewma`](./decay/ewma.md) | Exponentially-weighted moving average. |
| [`bv.ewvar`](./decay/ewvar.md) | Exponentially-weighted variance. |
| [`bv.ew_zscore`](./decay/ew_zscore.md) | Current event z-score against EWMA/EWVar baseline. |
| [`bv.decayed_sum`](./decay/decayed_sum.md) | Forward-decay sum (Cormode). |
| [`bv.decayed_count`](./decay/decayed_count.md) | Forward-decay count. |
| [`bv.twa`](./decay/twa.md) | Time-weighted average. |

## Velocity (9)

| Op | Description |
|----|-------------|
| [`bv.rate_of_change`](./velocity/rate_of_change.md) | Rate or acceleration delta across two adjacent windows. |
| [`bv.inter_arrival_stats`](./velocity/inter_arrival_stats.md) | Mean / stddev / CV of gaps between matching events. |
| [`bv.burst_count`](./velocity/burst_count.md) | Max events in any sub-window inside outer window. |
| [`bv.delta_from_prev`](./velocity/delta_from_prev.md) | Current value minus previous event value. |
| [`bv.trend`](./velocity/trend.md) | Slope of EW linear regression. |
| [`bv.trend_residual`](./velocity/trend_residual.md) | Current value minus trend-predicted value. |
| [`bv.outlier_count`](./velocity/outlier_count.md) | Count of events beyond N-sigma in window. |
| [`bv.value_change_count`](./velocity/value_change_count.md) | Count of field value flips. |
| [`bv.z_score`](./velocity/z_score.md) | Entity-level z-score against rolling mean/stddev baseline. |

## Bounded buffer + geo (11)

| Op | Description |
|----|-------------|
| [`bv.histogram`](./buffer-geo/histogram.md) | Count per fixed bucket. |
| [`bv.hour_of_day_histogram`](./buffer-geo/hour_of_day_histogram.md) | 24-bin count histogram per entity. |
| [`bv.dow_hour_histogram`](./buffer-geo/dow_hour_histogram.md) | 168-bin (day x hour) histogram per entity. |
| [`bv.seasonal_deviation`](./buffer-geo/seasonal_deviation.md) | Z-score against this entity's hour-of-day baseline. |
| [`bv.event_type_mix`](./buffer-geo/event_type_mix.md) | Proportion per category over window. |
| [`bv.most_recent_n`](./buffer-geo/most_recent_n.md) | Deque of N most-recent values. |
| [`bv.reservoir_sample`](./buffer-geo/reservoir_sample.md) | Uniform K-sample over all history. |
| [`bv.geo_velocity`](./buffer-geo/geo_velocity.md) | Max implied km/h between consecutive events. |
| [`bv.geo_distance`](./buffer-geo/geo_distance.md) | Total path length in window. |
| [`bv.geo_spread`](./buffer-geo/geo_spread.md) | Max distance from mean center. |
| [`bv.distance_from_home`](./buffer-geo/distance_from_home.md) | Distance from running centroid of top-K frequent locations. |

## Aliases

- `bv.ema` is an alias of [`bv.ewma`](./decay/ewma.md) — documented inline on the ewma page (same `AggKind::Ewma` variant; 53 unique kinds + 1 alias = 54 page paths).

## Cost-class metadata

- See [cost-class.md](./cost-class.md) for per-op CPU tier (Tier 1 / Tier 2 / Tier 3) — alive v0 metadata, cross-linked from each op page's Complexity section.

## See also

- [pipeline-dsl/compilation-rules.md](../pipeline-dsl/compilation-rules.md) — chain compilation rules
- [examples/wire/](../../examples/wire/) — JSON wire form fixtures
- [ADR-002 Polars op rename](../../.planning/decisions/ADR-002-polars-op-rename.md)
