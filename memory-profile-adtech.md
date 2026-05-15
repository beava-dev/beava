# AggOp Memory Profile: fraud-team

## Workload Summary

- Workload: `adtech`
- Events replayed per op: `2000`
- Derivations discovered: `1`
- Aggregate features discovered: `7`
- Per-entity structural estimate: `1122600` bytes

## Sorted Op Table

| Rank | Op | Shape | Stack bytes | Heap bytes | Total bytes |
|------|----|-------|-------------|------------|-------------|
| 1 | `n_unique` | `windowed` | 80 | 1037960 | 1038040 |
| 2 | `quantile` | `windowed` | 80 | 84080 | 84160 |
| 3 | `count` | `lifetime` | 80 | 0 | 80 |
| 4 | `max` | `lifetime` | 80 | 0 | 80 |
| 5 | `mean` | `lifetime` | 80 | 0 | 80 |
| 6 | `min` | `lifetime` | 80 | 0 | 80 |
| 7 | `sum` | `lifetime` | 80 | 0 | 80 |

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

### 2. `TxnAgg` / `amount_p99_1h` / `quantile`

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

### 3. `TxnAgg` / `avg_amt` / `mean`

- Bytes: stack=80 heap=0 total=80
- Shape: `lifetime`
- Recommendation: keep; scalar state spends only the shared AggOp slot
- Breakdown:

### 4. `TxnAgg` / `cnt` / `count`

- Bytes: stack=80 heap=0 total=80
- Shape: `lifetime`
- Recommendation: keep; scalar state spends only the shared AggOp slot
- Breakdown:

### 5. `TxnAgg` / `max_amt` / `max`

- Bytes: stack=80 heap=0 total=80
- Shape: `lifetime`
- Recommendation: keep; no targeted restructuring until workload ranking justifies it
- Breakdown:

## Metrics Coherence

- `/metrics` `beava_bytes_per_entity_p99`: `7000` bytes
- Profile per-entity estimate: `1122600` bytes
- Tolerance: `15.0%`
- Assertion: bytes_per_entity_p99 diverged by 1115600 bytes; file sibling work to replace the static placeholder with live sampling.

## Notes

- `stack_bytes` is the inline `AggOp` enum slot for each feature.
- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.
