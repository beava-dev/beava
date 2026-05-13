# Core Aggregation Operators

The 8 core ops cover the basic statistical primitives: counts, sums, central tendency, dispersion, and ratios.

All core ops are **O(1)** per-event memory and **Tier 1** CPU. Lifetime mode is permitted for all 8.

## Operators

| Op | Description |
|----|-------------|
| [`bv.count`](./count.md) | Event count over a window or lifetime. |
| [`bv.sum`](./sum.md) | Numeric sum of a field. |
| [`bv.mean`](./mean.md) | Arithmetic mean of a field. *(Renamed from `avg` per [ADR-002](../../../.planning/decisions/ADR-002-polars-op-rename.md).)* |
| [`bv.min`](./min.md) | Minimum value of a field. |
| [`bv.max`](./max.md) | Maximum value of a field. |
| [`bv.var`](./var.md) | Sample variance (Welford). *(Renamed from `variance`.)* |
| [`bv.std`](./std.md) | Standard deviation. *(Renamed from `stddev`.)* |
| [`bv.ratio`](./ratio.md) | Count matching predicate / total count. |

## See also

- [Operator catalog index](../index.md) — full 55-op catalogue
- [cost-class.md](../cost-class.md) — performance tier metadata
- [Pipeline DSL compilation rules](../../pipeline-dsl/compilation-rules.md)
