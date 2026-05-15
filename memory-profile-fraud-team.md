# AggOp Memory Profile: fraud-team

## Workload Summary

- Workload: `fraud`
- Events replayed per op: `2000`
- Derivations discovered: `9`
- Aggregate features discovered: `111`
- Per-entity structural estimate: `3085692` bytes

## Sorted Op Table

| Rank | Op | Shape | Stack bytes | Heap bytes | Total bytes |
|------|----|-------|-------------|------------|-------------|
| 1 | `n_unique` | `windowed` | 1040 | 2712248 | 2713288 |
| 2 | `top_k` | `windowed` | 160 | 241768 | 241928 |
| 3 | `count` | `windowed` | 1200 | 30864 | 32064 |
| 4 | `entropy` | `windowed` | 80 | 30824 | 30904 |
| 5 | `quantile` | `windowed` | 320 | 28608 | 28928 |
| 6 | `event_type_mix` | `lifetime` | 80 | 9728 | 9808 |
| 7 | `sum` | `windowed` | 160 | 4576 | 4736 |
| 8 | `n_unique` | `lifetime` | 160 | 4304 | 4464 |
| 9 | `burst_count` | `windowed` | 240 | 3072 | 3312 |
| 10 | `distance_from_home` | `lifetime` | 80 | 1720 | 1800 |
| 11 | `reservoir_sample` | `lifetime` | 80 | 1600 | 1680 |
| 12 | `seasonal_deviation` | `lifetime` | 80 | 1427 | 1507 |
| 13 | `dow_hour_histogram` | `lifetime` | 80 | 1344 | 1424 |
| 14 | `bloom_member` | `lifetime` | 80 | 1280 | 1360 |
| 15 | `geo_velocity` | `lifetime` | 160 | 372 | 532 |
| 16 | `mean` | `windowed` | 80 | 416 | 496 |
| 17 | `min` | `windowed` | 80 | 416 | 496 |
| 18 | `std` | `windowed` | 80 | 416 | 496 |
| 19 | `var` | `windowed` | 80 | 416 | 496 |
| 20 | `first_seen` | `lifetime` | 400 | 0 | 400 |
| 21 | `first_n` | `lifetime` | 80 | 256 | 336 |
| 22 | `hour_of_day_histogram` | `lifetime` | 80 | 255 | 335 |
| 23 | `geo_spread` | `lifetime` | 80 | 251 | 331 |
| 24 | `age` | `lifetime` | 320 | 0 | 320 |
| 25 | `negative_streak` | `lifetime` | 320 | 0 | 320 |
| 26 | `geo_distance` | `lifetime` | 80 | 171 | 251 |
| 27 | `count` | `lifetime` | 240 | 0 | 240 |
| 28 | `last_n` | `lifetime` | 80 | 160 | 240 |
| 29 | `last_seen` | `lifetime` | 240 | 0 | 240 |
| 30 | `most_recent_n` | `lifetime` | 80 | 160 | 240 |
| 31 | `time_since` | `lifetime` | 240 | 0 | 240 |
| 32 | `decayed_count` | `lifetime` | 160 | 0 | 160 |
| 33 | `first_seen_in_window` | `windowed` | 160 | 0 | 160 |
| 34 | `streak` | `lifetime` | 160 | 0 | 160 |
| 35 | `sum` | `lifetime` | 160 | 0 | 160 |
| 36 | `lag` | `lifetime` | 80 | 64 | 144 |
| 37 | `entropy` | `lifetime` | 80 | 56 | 136 |
| 38 | `time_since_last_n` | `lifetime` | 80 | 40 | 120 |
| 39 | `decayed_sum` | `lifetime` | 80 | 0 | 80 |
| 40 | `delta_from_prev` | `lifetime` | 80 | 0 | 80 |
| 41 | `ew_zscore` | `lifetime` | 80 | 0 | 80 |
| 42 | `ewma` | `lifetime` | 80 | 0 | 80 |
| 43 | `ewvar` | `lifetime` | 80 | 0 | 80 |
| 44 | `first` | `lifetime` | 80 | 0 | 80 |
| 45 | `has_seen` | `lifetime` | 80 | 0 | 80 |
| 46 | `inter_arrival_stats` | `windowed` | 80 | 0 | 80 |
| 47 | `last` | `lifetime` | 80 | 0 | 80 |
| 48 | `max` | `lifetime` | 80 | 0 | 80 |
| 49 | `max_streak` | `lifetime` | 80 | 0 | 80 |
| 50 | `outlier_count` | `windowed` | 80 | 0 | 80 |
| 51 | `rate_of_change` | `windowed` | 80 | 0 | 80 |
| 52 | `trend` | `windowed` | 80 | 0 | 80 |
| 53 | `trend_residual` | `windowed` | 80 | 0 | 80 |
| 54 | `twa` | `windowed` | 80 | 0 | 80 |
| 55 | `value_change_count` | `windowed` | 80 | 0 | 80 |
| 56 | `z_score` | `windowed` | 80 | 0 | 80 |

## Top 5 Offenders

### 1. `LoginByUser` / `ips_distinct_login_1h` / `n_unique`

- Bytes: stack=80 heap=1037960 total=1038040
- Shape: `windowed` (1h)
- Recommendation: keep for now; quantify sketch precision and window bucket fanout separately
- Breakdown rollup:
  - `CountDistinct hash-set slots across buckets`: 1032192 bytes (HashSet, summed across active window buckets)
  - `Windowed bucket shell overhead`: 2960 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Box<CountDistinctStateWrap> across buckets`: 1480 bytes (Box, summed across active window buckets)
  - `Windowed wrapper overhead`: 1200 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `CountDistinct exact-array values across buckets`: 128 bytes (Vec, summed across active window buckets)
- Raw breakdown:
  - `Windowed bucket 1 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 10 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 11 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 12 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 13 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 14 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 15 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 16 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)

### 2. `TxnByIp` / `cards_per_ip_1h` / `n_unique`

- Bytes: stack=80 heap=1037960 total=1038040
- Shape: `windowed` (1h)
- Recommendation: keep for now; quantify sketch precision and window bucket fanout separately
- Breakdown rollup:
  - `CountDistinct hash-set slots across buckets`: 1032192 bytes (HashSet, summed across active window buckets)
  - `Windowed bucket shell overhead`: 2960 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Box<CountDistinctStateWrap> across buckets`: 1480 bytes (Box, summed across active window buckets)
  - `Windowed wrapper overhead`: 1200 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `CountDistinct exact-array values across buckets`: 128 bytes (Vec, summed across active window buckets)
- Raw breakdown:
  - `Windowed bucket 1 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 10 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 11 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 12 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 13 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 14 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 15 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 16 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)

### 3. `TxnByIp` / `ip_top_users` / `top_k`

- Bytes: stack=80 heap=129320 total=129400
- Shape: `windowed` (1d)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `TopK count-min counters across buckets`: 65536 bytes (Vec, summed across active window buckets)
  - `TopK exact BTreeMap entries across buckets`: 62400 bytes (BTreeMap, summed across active window buckets)
  - `Box<TopKStateWrap> across buckets`: 480 bytes (Box, summed across active window buckets)
  - `TopK heap index map across buckets`: 392 bytes (AHashMap, summed across active window buckets)
  - `Windowed bucket shell overhead`: 240 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `TopK heap entries across buckets`: 96 bytes (Vec, summed across active window buckets)
- Raw breakdown:
  - `Windowed bucket 1 / TopK count-min counters`: 65536 bytes (Vec, capacity * size_of::<i64>() for count-min sketch counters)
  - `Windowed bucket 0 / TopK exact BTreeMap entries`: 33600 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 2 / TopK exact BTreeMap entries`: 28800 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 1 / TopK heap index map`: 392 bytes (AHashMap, estimated slot cost for TopK heap-position side index)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<TopKStateWrap>`: 160 bytes (Box, heap allocation for boxed TopK wrapper)
  - `Windowed bucket 1 / Box<TopKStateWrap>`: 160 bytes (Box, heap allocation for boxed TopK wrapper)
  - `Windowed bucket 2 / Box<TopKStateWrap>`: 160 bytes (Box, heap allocation for boxed TopK wrapper)

### 4. `TxnByUser` / `top_merchants_24h` / `top_k`

- Bytes: stack=80 heap=112448 total=112528
- Shape: `windowed` (1d)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `TopK exact BTreeMap entries across buckets`: 111552 bytes (BTreeMap, summed across active window buckets)
  - `Box<TopKStateWrap> across buckets`: 480 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 240 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
- Raw breakdown:
  - `Windowed bucket 1 / TopK exact BTreeMap entries`: 49152 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 0 / TopK exact BTreeMap entries`: 33600 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 2 / TopK exact BTreeMap entries`: 28800 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<TopKStateWrap>`: 160 bytes (Box, heap allocation for boxed TopK wrapper)
  - `Windowed bucket 1 / Box<TopKStateWrap>`: 160 bytes (Box, heap allocation for boxed TopK wrapper)
  - `Windowed bucket 2 / Box<TopKStateWrap>`: 160 bytes (Box, heap allocation for boxed TopK wrapper)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

### 5. `LoginByUser` / `uas_distinct_login_24h` / `n_unique`

- Bytes: stack=80 heap=86552 total=86632
- Shape: `windowed` (1d)
- Recommendation: keep for now; quantify sketch precision and window bucket fanout separately
- Breakdown rollup:
  - `CountDistinct hash-set slots across buckets`: 86016 bytes (HashSet, summed across active window buckets)
  - `Windowed bucket shell overhead`: 240 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Box<CountDistinctStateWrap> across buckets`: 120 bytes (Box, summed across active window buckets)
- Raw breakdown:
  - `Windowed bucket 0 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 1 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Windowed bucket 2 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 1 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 2 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 0 / Box<CountDistinctStateWrap>`: 40 bytes (Box, heap allocation for boxed CountDistinct wrapper)

## Metrics Coherence

- `/metrics` `beava_bytes_per_entity_p99`: `7000` bytes
- Profile per-entity estimate: `3085692` bytes
- Tolerance: `15.0%`
- Assertion: bytes_per_entity_p99 diverged by 3078692 bytes; file sibling work to replace the static placeholder with live sampling.

## Notes

- `stack_bytes` is the inline `AggOp` enum slot for each feature.
- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.
