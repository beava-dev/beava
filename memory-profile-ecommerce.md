# AggOp Memory Profile: fraud-team

## Workload Summary

- Workload: `ecommerce`
- Events replayed per op: `2000`
- Derivations discovered: `1`
- Aggregate features discovered: `20`
- Per-entity structural estimate: `1333232` bytes

## Sorted Op Table

| Rank | Op | Shape | Stack bytes | Heap bytes | Total bytes |
|------|----|-------|-------------|------------|-------------|
| 1 | `n_unique` | `windowed` | 80 | 1037960 | 1038040 |
| 2 | `top_k` | `windowed` | 80 | 202080 | 202160 |
| 3 | `quantile` | `windowed` | 80 | 84080 | 84160 |
| 4 | `entropy` | `windowed` | 80 | 6232 | 6312 |
| 5 | `bloom_member` | `lifetime` | 80 | 1280 | 1360 |
| 6 | `max` | `lifetime` | 240 | 0 | 240 |
| 7 | `mean` | `lifetime` | 240 | 0 | 240 |
| 8 | `min` | `lifetime` | 240 | 0 | 240 |
| 9 | `sum` | `lifetime` | 240 | 0 | 240 |
| 10 | `count` | `lifetime` | 80 | 0 | 80 |
| 11 | `std` | `lifetime` | 80 | 0 | 80 |
| 12 | `var` | `lifetime` | 80 | 0 | 80 |

## Top 5 Offenders

### 1. `TxnAgg` / `merchants_distinct_1h` / `n_unique`

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

### 2. `TxnAgg` / `top_merchants_1h` / `top_k`

- Bytes: stack=80 heap=202080 total=202160
- Shape: `windowed` (1h)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `TopK exact BTreeMap entries across buckets`: 192000 bytes (BTreeMap, summed across active window buckets)
  - `Box<TopKStateWrap> across buckets`: 5920 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 2960 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Windowed wrapper overhead`: 1200 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
- Raw breakdown:
  - `Windowed bucket 11 / TopK exact BTreeMap entries`: 5472 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 15 / TopK exact BTreeMap entries`: 5472 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 19 / TopK exact BTreeMap entries`: 5472 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 23 / TopK exact BTreeMap entries`: 5472 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 27 / TopK exact BTreeMap entries`: 5472 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 3 / TopK exact BTreeMap entries`: 5472 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 31 / TopK exact BTreeMap entries`: 5472 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Windowed bucket 35 / TopK exact BTreeMap entries`: 5472 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)

### 3. `TxnAgg` / `amount_p99_1h` / `quantile`

- Bytes: stack=80 heap=84080 total=84160
- Shape: `windowed` (1h)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `Percentile exact samples across buckets`: 75776 bytes (Vec, summed across active window buckets)
  - `Box<PercentileStateWrap> across buckets`: 4144 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 2960 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Windowed wrapper overhead`: 1200 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
- Raw breakdown:
  - `Windowed bucket 0 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Windowed bucket 1 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Windowed bucket 10 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Windowed bucket 11 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Windowed bucket 12 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Windowed bucket 13 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Windowed bucket 14 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Windowed bucket 15 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)

### 4. `TxnAgg` / `category_entropy_1h` / `entropy`

- Bytes: stack=80 heap=6232 total=6312
- Shape: `windowed` (1h)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `Windowed bucket shell overhead`: 2960 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Box<EntropyStateWrap> across buckets`: 2072 bytes (Box, summed across active window buckets)
  - `Windowed wrapper overhead`: 1200 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
- Raw breakdown:
  - `WindowedOp spilled bucket SmallVec`: 1024 bytes (SmallVec, spilled capacity * size_of::<(i64, Box<AggOp>)>())
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 1 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 10 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 11 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 12 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 13 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

### 5. `TxnAgg` / `device_seen` / `bloom_member`

- Bytes: stack=80 heap=1280 total=1360
- Shape: `lifetime`
- Recommendation: keep for now; quantify sparse-to-dense sketch options next
- Breakdown:
  - `Bloom filter words`: 1232 bytes (Vec, capacity * size_of::<u64>() for bloom bit-array storage)
  - `Box<BloomMemberStateWrap>`: 48 bytes (Box, heap allocation for boxed Bloom wrapper)

## Metrics Coherence

- `/metrics` `beava_bytes_per_entity_p99`: `7000` bytes
- Profile per-entity estimate: `1333232` bytes
- Tolerance: `15.0%`
- Assertion: bytes_per_entity_p99 diverged by 1326232 bytes; file sibling work to replace the static placeholder with live sampling.

## Notes

- `stack_bytes` is the inline `AggOp` enum slot for each feature.
- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.
