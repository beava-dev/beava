# AggOp Memory Profile: fraud-team

## Workload Summary

- Workload: `fraud`
- Events replayed per op: `2000`
- Derivations discovered: `9`
- Aggregate features discovered: `111`
- Per-entity structural estimate: `405997` bytes

## Sorted Op Table

| Rank | Op | Stack bytes | Heap bytes | Total bytes |
|------|----|-------------|------------|-------------|
| 1 | `n_unique` | 1200 | 299930 | 301130 |
| 2 | `count` | 1440 | 30864 | 32304 |
| 3 | `top_k` | 160 | 18366 | 18526 |
| 4 | `quantile` | 320 | 13481 | 13801 |
| 5 | `event_type_mix` | 80 | 9728 | 9808 |
| 6 | `sum` | 320 | 4576 | 4896 |
| 7 | `entropy` | 160 | 4308 | 4468 |
| 8 | `bloom_member` | 80 | 3308 | 3388 |
| 9 | `burst_count` | 240 | 3072 | 3312 |
| 10 | `distance_from_home` | 80 | 1720 | 1800 |
| 11 | `reservoir_sample` | 80 | 1600 | 1680 |
| 12 | `seasonal_deviation` | 80 | 1427 | 1507 |
| 13 | `dow_hour_histogram` | 80 | 1344 | 1424 |
| 14 | `geo_velocity` | 160 | 372 | 532 |
| 15 | `mean` | 80 | 416 | 496 |
| 16 | `min` | 80 | 416 | 496 |
| 17 | `std` | 80 | 416 | 496 |
| 18 | `var` | 80 | 416 | 496 |
| 19 | `first_seen` | 400 | 0 | 400 |
| 20 | `first_n` | 80 | 256 | 336 |
| 21 | `hour_of_day_histogram` | 80 | 255 | 335 |
| 22 | `geo_spread` | 80 | 251 | 331 |
| 23 | `age` | 320 | 0 | 320 |
| 24 | `negative_streak` | 320 | 0 | 320 |
| 25 | `geo_distance` | 80 | 171 | 251 |
| 26 | `last_n` | 80 | 160 | 240 |
| 27 | `last_seen` | 240 | 0 | 240 |
| 28 | `most_recent_n` | 80 | 160 | 240 |
| 29 | `time_since` | 240 | 0 | 240 |
| 30 | `decayed_count` | 160 | 0 | 160 |
| 31 | `first_seen_in_window` | 160 | 0 | 160 |
| 32 | `streak` | 160 | 0 | 160 |
| 33 | `lag` | 80 | 64 | 144 |
| 34 | `time_since_last_n` | 80 | 40 | 120 |
| 35 | `decayed_sum` | 80 | 0 | 80 |
| 36 | `delta_from_prev` | 80 | 0 | 80 |
| 37 | `ew_zscore` | 80 | 0 | 80 |
| 38 | `ewma` | 80 | 0 | 80 |
| 39 | `ewvar` | 80 | 0 | 80 |
| 40 | `first` | 80 | 0 | 80 |
| 41 | `has_seen` | 80 | 0 | 80 |
| 42 | `inter_arrival_stats` | 80 | 0 | 80 |
| 43 | `last` | 80 | 0 | 80 |
| 44 | `max` | 80 | 0 | 80 |
| 45 | `max_streak` | 80 | 0 | 80 |
| 46 | `outlier_count` | 80 | 0 | 80 |
| 47 | `rate_of_change` | 80 | 0 | 80 |
| 48 | `trend` | 80 | 0 | 80 |
| 49 | `trend_residual` | 80 | 0 | 80 |
| 50 | `twa` | 80 | 0 | 80 |
| 51 | `value_change_count` | 80 | 0 | 80 |
| 52 | `z_score` | 80 | 0 | 80 |

## Top 5 Offenders

### 1. `LoginByUser` / `ips_distinct_login_1h` / `n_unique`

- Bytes: stack=80 heap=48449 total=48529
- Recommendation: keep for now; quantify sparse-to-dense sketch options next
- Breakdown:
  - `Windowed bucket 7 / CountDistinct owned internals`: 1228 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 31 / CountDistinct owned internals`: 1224 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 23 / CountDistinct owned internals`: 1221 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 3 / CountDistinct owned internals`: 1220 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 27 / CountDistinct owned internals`: 1219 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 19 / CountDistinct owned internals`: 1217 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 35 / CountDistinct owned internals`: 1214 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 15 / CountDistinct owned internals`: 1213 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)

### 2. `TxnByIp` / `cards_per_ip_1h` / `n_unique`

- Bytes: stack=80 heap=48397 total=48477
- Recommendation: keep for now; quantify sparse-to-dense sketch options next
- Breakdown:
  - `Windowed bucket 19 / CountDistinct owned internals`: 1221 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 27 / CountDistinct owned internals`: 1220 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 35 / CountDistinct owned internals`: 1219 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 11 / CountDistinct owned internals`: 1216 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 15 / CountDistinct owned internals`: 1215 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 3 / CountDistinct owned internals`: 1215 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 23 / CountDistinct owned internals`: 1209 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 31 / CountDistinct owned internals`: 1209 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)

### 3. `TxnByCard` / `merchants_per_card_24h` / `n_unique`

- Bytes: stack=80 heap=24391 total=24471
- Recommendation: keep for now; quantify sparse-to-dense sketch options next
- Breakdown:
  - `Windowed bucket 1 / CountDistinct owned internals`: 10494 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 0 / CountDistinct owned internals`: 7188 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 2 / CountDistinct owned internals`: 6173 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 1 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 2 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 0 / Box<CountDistinct>`: 40 bytes (Box, heap allocation for boxed payload)

### 4. `TxnByUser` / `merchants_distinct_24h` / `n_unique`

- Bytes: stack=80 heap=24391 total=24471
- Recommendation: keep for now; quantify sparse-to-dense sketch options next
- Breakdown:
  - `Windowed bucket 1 / CountDistinct owned internals`: 10494 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 0 / CountDistinct owned internals`: 7188 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 2 / CountDistinct owned internals`: 6173 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 1 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 2 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 0 / Box<CountDistinct>`: 40 bytes (Box, heap allocation for boxed payload)

### 5. `TxnByDevice` / `cards_per_device_24h` / `n_unique`

- Bytes: stack=80 heap=22174 total=22254
- Recommendation: keep for now; quantify sparse-to-dense sketch options next
- Breakdown:
  - `Windowed bucket 1 / CountDistinct owned internals`: 8259 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 0 / CountDistinct owned internals`: 7205 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Windowed bucket 2 / CountDistinct owned internals`: 6174 bytes (estimate, deterministic serialized-size proxy for private sketch/container internals)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 1 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 2 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 0 / Box<CountDistinct>`: 40 bytes (Box, heap allocation for boxed payload)

## Metrics Coherence

- `/metrics` `beava_bytes_per_entity_p99`: `7000` bytes
- Profile per-entity estimate: `405997` bytes
- Tolerance: `15.0%`
- Assertion: bytes_per_entity_p99 diverged by 398997 bytes; file sibling work to replace the static placeholder with live sampling.

## Notes

- `stack_bytes` is the inline `AggOp` enum slot for each feature.
- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.
