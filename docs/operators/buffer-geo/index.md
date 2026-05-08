# Bounded Buffer + Geo Aggregation Operators

The `buffer-geo/` subdirectory hosts **11 ops** grouped into two related families: **bounded buffers** (count-per-bucket histograms, deques, reservoirs, categorical mixes) and **geo** (haversine-distance aggregations on lat/lon field pairs). Both families share the property that their state is non-`O(1)`-trivial but **bounded** — every op declares a finite per-entity ceiling at register time per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md), and every op is **lifetime-only** (no `window=` kwarg in v0).

## Bounded buffers (7)

| Op | Memory class | Required kwarg | One-line |
|----|--------------|----------------|----------|
| [`bv.histogram`](./histogram.md) | `BoundedByRequiredKwarg("buckets")` | `buckets` | Count per fixed numeric bucket of a field. |
| [`bv.hour_of_day_histogram`](./hour_of_day_histogram.md) | `O(1)` (24-bin structural cap) | — | UTC hour-of-day count histogram per entity. |
| [`bv.dow_hour_histogram`](./dow_hour_histogram.md) | `O(1)` (168-bin structural cap) | — | UTC day-of-week × hour count histogram per entity. |
| [`bv.seasonal_deviation`](./seasonal_deviation.md) | `O(1)` (24-HourBucket structural cap) | — | Z-score of latest event vs this entity's hour-of-day baseline. |
| [`bv.event_type_mix`](./event_type_mix.md) | `BoundedByConfig("max_categories", 256)` | — *(soft default)* | Per-category proportion over entity lifetime. |
| [`bv.most_recent_n`](./most_recent_n.md) | `BoundedByRequiredKwarg("n")` | `n` | Circular buffer of N most recent values. |
| [`bv.reservoir_sample`](./reservoir_sample.md) | `BoundedByRequiredKwarg("samples")` | `samples` | Uniform K-sample over full history (Vitter Algorithm R). |

## Geo (4)

| Op | Memory class | Required kwargs | One-line |
|----|--------------|------------------|----------|
| [`bv.geo_velocity`](./geo_velocity.md) | `O(1)` | `lat`, `lon` | Max implied great-circle km/h between consecutive matching events. |
| [`bv.geo_distance`](./geo_distance.md) | `O(1)` | `lat`, `lon` | Total cumulative ground-track length in km. |
| [`bv.geo_spread`](./geo_spread.md) | `O(1)` | `lat`, `lon` | RMS dispersion (km) of points around their running mean centre (Welford 2-D). |
| [`bv.distance_from_home`](./distance_from_home.md) | `BoundedByConfig("samples", 100)` | `lat`, `lon` | Distance (km) of current event from centroid of last `samples` points. |

All 4 geo ops use **haversine** great-circle distance on the `(lat, lon)` field pair (mean Earth radius 6371 km per `agg_geo.rs::haversine_km`). `lat=` and `lon=` are **field-name strings** (e.g. `lat="latitude"`, `lon="longitude"`) — they reference event columns, NOT literal coordinates. Note: `bv.unique_cells` and `bv.geo_entropy` were **removed in v0 D-05** per `project_v2_devex_first` simplification — users compose them via the `count_distinct(quadkey(lat, lon, zoom))` and `entropy(quadkey(lat, lon, zoom))` recipes; see [cost-class.md "Recipe Replacements"](../cost-class.md#recipe-replacements-post-phase-192).

## V0-MEM-GOV-02 compliance summary

All 11 ops in this subdirectory have a **finite per-entity memory ceiling declared at register time**. Three classes:

- **`BoundedByRequiredKwarg(name)` (3 ops):** `histogram` (`buckets`), `most_recent_n` (`n`), `reservoir_sample` (`samples`). Missing kwarg → JSON-prelude shim rejects with structured error code `unbounded_op_in_lifetime_mode` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).
- **`BoundedByConfig(name, default)` (2 ops):** `event_type_mix` (`max_categories=256`), `distance_from_home` (`samples=100`). Kwarg is OPTIONAL; the per-entity ceiling is always declared via the soft default.
- **`O(1)` by structural cap (6 ops):** `hour_of_day_histogram` (24 bins), `dow_hour_histogram` (168 bins), `seasonal_deviation` (24 HourBuckets + memo), `geo_velocity` / `geo_distance` / `geo_spread` (`Option<(f64, f64, ...)>` pair). No required kwarg; ceiling is structural.

## Lifetime-only family invariant

**All 11 ops are lifetime-only in v0** — none accept a `window=` kwarg. For windowed analogues, compose with `@bv.event(cold_after="...")` per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md): the per-entity state is dropped on TTL expiry and rebuilds fresh from the next post-eviction matching event. This is the canonical windowing workaround across the entire `buffer-geo/` family. (Compare with the `velocity/` family where 7 of 9 ops accept and require `window=`.)

## v0 boxing summary

7 of 11 ops in this subdirectory have `Box`-wrapped state in the `AggOp` enum to fit the **80-byte `size_of::<AggOp>()` cap** enforced by `crates/beava-core/tests/per_entity_size_dump.rs::aggop_size_within_cap`:

- **Boxed (7):** `hour_of_day_histogram`, `seasonal_deviation`, `event_type_mix`, `geo_velocity`, `geo_distance`, `geo_spread`, `distance_from_home`.
- **Inline (4):** `histogram`, `dow_hour_histogram` (Vec<u64> 24-byte header), `most_recent_n`, `reservoir_sample` (Vec<Value> 24-byte headers).

See [SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md) for the boxing rationale and per-variant breakdown.

## See also

- [Operator catalog index](../index.md) — master 54-page operator catalog
- [cost-class.md](../cost-class.md) — per-op CPU tier (Tier 1 / 2 / 3) — alive v0 metadata
- [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md) — cold-entity eviction (`@bv.event(cold_after=...)`) pattern
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — lifetime-aggregation memory governance contract
- [SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md) — `AggOp` boxing context
