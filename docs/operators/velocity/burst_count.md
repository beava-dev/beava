# bv.burst_count

> Maximum events observed in any single `sub_window` slice inside the outer `window`. The "did we see a spike?" primitive.

## Signature

```python
bv.burst_count(
    *,
    window: str,
    sub_window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.burst_count` chops time into `sub_window`-sized buckets, counts
matching events per bucket, and returns the **maximum bucket count**
seen so far. Read it as "what is the worst burst this entity produced
over the last `window`?" — for example, "max events seen in any single
1-minute slice over the last hour", or "peak login attempts in any
5-second window over the last 5 minutes". The state is a 64-slot ring of
sub-window counters indexed by `floor(now_ms / sub_window_ms) % 64`,
plus a single `max_seen` counter that tracks the largest bucket count
ever observed within the lifetime of the windowed wrapper.

This is the canonical "spike detector" primitive — useful for
brute-force credential stuffing (peak login attempts), DDoS-shape
anomalies (peak request rate per IP), or fraud burst patterns (peak
authorisations per card in any short slice). Compared to a flat
[`bv.count(window="1h")`](../core/count.md), `burst_count` sees the
**peak intensity** rather than the total — an entity that spikes once
and then stays quiet looks identical to a steady streamer in a flat
count but stands out as suspicious in `burst_count`. Pair it with
[`bv.inter_arrival_stats`](./inter_arrival_stats.md) when you want both
peak burst and average cadence.

`bv.burst_count` belongs to the **velocity** family. The op is
**field-less** by design — it counts events, not values. Per-event
update is one modulo, one bucket compare, and one count update;
cost is **Tier 1** (~12 ns floor / ~32 ns measured) and memory is
`O(1)` per entity (a fixed 64-slot bucket ring + counters). The 64-slot
ring is structural — it does not grow with traffic.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `window` | `str` | Yes | — | Outer window — duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. |
| `sub_window` | `str` | Yes | — | Inner sub-window — duration string matching `\d+(ms\|s\|m\|h\|d)`. **Must be smaller than `window`.** `"forever"` is **rejected**. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events count toward bucket totals. |

(No `field=` kwarg — `burst_count` is field-less by design; passing a positional argument raises `TypeError` at SDK-helper-call time.)

## Returns

A single `i64` — the maximum count observed in any single sub-window slice. Cold-start (no matching events seen) returns `0`, never `null`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~12 ns floor / ~32 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `BurstCountState` ≈ 1.1 KB (`buckets: [u64; 64]` + `bucket_epoch: [i64; 64]` + `max_seen: u64` + `initialized: bool`) |
| Lifetime mode (`window="forever"`) | **Allowed** — classified `O1` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). The 64-bucket ring is the structural memory cap; growth-free. |

## Examples

### Example 1: Peak per-minute login attempts per IP, hourly window

```python
import beava as bv

@bv.event
class Login:
    ip: str
    status: str

@bv.table(key="ip")
def IpLoginBurst(logins) -> bv.Table:
    return (
        logins.group_by("ip")
              .agg(peak_per_min_1h=bv.burst_count(
                       window="1h",
                       sub_window="1m"))
    )

# Push events
# ... 100 events at t=0..1000ms (all in the same 1m bucket) ...
# Query
result = app.get("IpLoginBurst", "1.2.3.4")
# result == {"peak_per_min_1h": 100} # peak burst = 100 in one minute slice
```

### Example 2: Filtered peak failed-login burst per user

```python
@bv.table(key="user_id")
def UserFailBurst(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(peak_fail_per_5s=bv.burst_count(
                       window="5m",
                       sub_window="5s",
                       where=bv.col("status") == "failed"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "IpLoginBurst",
  "output_kind": "table",
  "key": ["ip"],
  "agg": {
    "peak_per_min_1h": {
      "op": "burst_count",
      "params": {
        "window": "1h",
        "sub_window": "1m"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `0` (integer), not `null`. The state's `max_seen` starts at 0 and only increases.
- **`sub_window >= window`:** semantically meaningless (only ever one bucket). The Python helper allows this — server-side `register_validate.rs` does not currently reject it; the result simply equals `bv.count(window=window, where=...)`. Treat it as a configuration smell; document with a `# pyright: ignore` if intentional.
- **Missing `sub_window=`:** raises `ValueError` at SDK-helper-call time. Wire-side, missing `params.sub_window` returns structured error `aggregation_invalid_sub_window` from `register_validate.rs`.
- **Malformed `sub_window=`** (e.g. `"5seconds"` / `"forever"` / `"0ms"`): raises `ValueError` at SDK-helper-call time; server returns `aggregation_invalid_sub_window` if reached.
- **Bucket-ring rollover:** the ring holds **64 distinct sub-windows**. Events that land 65 or more sub-windows after a previous bucket reuse its slot — the older bucket epoch is overwritten when the modulo index collides with a stale `bucket_epoch`. This is the structural memory cap; for outer windows that span >64 sub-windows, prefer increasing `sub_window` rather than letting buckets collide.
- **`where=` filter excludes the event:** no bucket update; non-matching events do not roll the bucket index either.
- **Missing `window=`:** raises `ValueError` at SDK-helper-call time.
- **Late or duplicate event:** indexed strictly by `floor(now_ms / sub_window_ms)` modulo 64; the bucket the event lands in is determined by its `now_ms` regardless of arrival order.
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the entire 64-slot ring; the next post-eviction matching event reseeds.

## See also

- [Velocity family index](./index.md) — overview of all 9 velocity-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.inter_arrival_stats](./inter_arrival_stats.md) — companion "average cadence" primitive
- [bv.count](../core/count.md) — flat per-window count (no peak detection)
- [bv.outlier_count](./outlier_count.md) — count of value-outliers; complements burst_count which counts arrival-density outliers
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
