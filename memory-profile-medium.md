# AggOp Memory Profile: fraud-team

## Workload Summary

- Workload: `medium`
- Events replayed per op: `2000`
- Derivations discovered: `1`
- Aggregate features discovered: `5`
- Per-entity structural estimate: `400` bytes

## Sorted Op Table

| Rank | Op | Shape | Stack bytes | Heap bytes | Total bytes |
|------|----|-------|-------------|------------|-------------|
| 1 | `count` | `lifetime` | 80 | 0 | 80 |
| 2 | `max` | `lifetime` | 80 | 0 | 80 |
| 3 | `mean` | `lifetime` | 80 | 0 | 80 |
| 4 | `min` | `lifetime` | 80 | 0 | 80 |
| 5 | `sum` | `lifetime` | 80 | 0 | 80 |

## Top 5 Offenders

### 1. `TxnAgg` / `avg_amt` / `mean`

- Bytes: stack=80 heap=0 total=80
- Shape: `lifetime`
- Recommendation: keep; scalar state spends only the shared AggOp slot
- Breakdown:

### 2. `TxnAgg` / `cnt` / `count`

- Bytes: stack=80 heap=0 total=80
- Shape: `lifetime`
- Recommendation: keep; scalar state spends only the shared AggOp slot
- Breakdown:

### 3. `TxnAgg` / `max_amt` / `max`

- Bytes: stack=80 heap=0 total=80
- Shape: `lifetime`
- Recommendation: keep; no targeted restructuring until workload ranking justifies it
- Breakdown:

### 4. `TxnAgg` / `min_amt` / `min`

- Bytes: stack=80 heap=0 total=80
- Shape: `lifetime`
- Recommendation: keep; no targeted restructuring until workload ranking justifies it
- Breakdown:

### 5. `TxnAgg` / `sum_amt` / `sum`

- Bytes: stack=80 heap=0 total=80
- Shape: `lifetime`
- Recommendation: keep; scalar state spends only the shared AggOp slot
- Breakdown:

## Metrics Coherence

- `/metrics` `beava_bytes_per_entity_p99`: `7000` bytes
- Profile per-entity estimate: `400` bytes
- Tolerance: `15.0%`
- Assertion: bytes_per_entity_p99 diverged by 6600 bytes; file sibling work to replace the static placeholder with live sampling.

## Notes

- `stack_bytes` is the inline `AggOp` enum slot for each feature.
- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.
