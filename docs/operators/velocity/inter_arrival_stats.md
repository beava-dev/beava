# bv.inter_arrival_stats

> Welford-style running statistics over inter-arrival gaps between matching events. v0 emits `mean_ms` only.

## Signature

```python
bv.inter_arrival_stats(
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.inter_arrival_stats` answers "how regularly do I see this entity?".
On every matching event the helper records the gap
`Δt = now_ms_curr - now_ms_prev` and folds it into a Welford accumulator
`(n, mean, m2)` over all inter-arrival gaps observed so far. The query
returns the running **mean** in milliseconds; cold-start (`n = 0`)
returns `null`. The accumulator carries enough state to compute stddev
and CV, but **v0 emits `mean_ms` only** — the SDK will surface a
`{mean_ms, stddev_ms, cv}` dict in v0.1+ when the wire-level return
shape is generalised; the underlying state already supports it.

This is the canonical "behavioural cadence" primitive — useful for
detecting bot-like uniform-gap patterns ("every 837 ms ± 4 ms"), human
bursty patterns ("clusters of clicks separated by minutes of pause"), or
sustained periodicity in an automated process. Pair it with
[`bv.burst_count`](./burst_count.md) when you also want the maximum
per-sub-window event count, or with [`bv.streak`](../recency/streak.md)
when you care about consecutive matches rather than gap statistics.

`bv.inter_arrival_stats` belongs to the **velocity** family. The op is
**field-less** by design — it operates on arrival timestamps, never on
payload values. Per-event update is one subtraction plus four scalar FP
ops (Welford); cost is **Tier 1** (~12 ns floor / ~32 ns measured) and
memory is `O(1)` per entity (`last_t`, `n`, `mean`, `m2`, `initialized`).
The `window=` kwarg is **required** by the Python SDK helper; the inner
state is itself lifetime-bound `O(1)`.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. See [shared.md window grammar](../../sdk-api/shared.md). |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events advance the inter-arrival accumulator. Non-matching events do not update `last_t`. |

(No `field=` kwarg — `inter_arrival_stats` is field-less by design; passing a positional argument raises `TypeError` at SDK-helper-call time.)

## Returns

A single `f64` — the running **mean inter-arrival gap in milliseconds** (`mean_ms`). Cold-start (no matching event seen) and one-event start (no gap recorded yet) both return `null` (Python `None`). v0.1+ widens this to a struct/dict `{mean_ms: float, stddev_ms: float, cv: float}` once the wire return shape supports nested values; the underlying Welford state already tracks `m2` to derive `stddev_ms` and `cv = stddev_ms / mean_ms`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~12 ns floor / ~32 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `InterArrivalStatsState` ≈ 40 B (`last_t: i64`, `n: u64`, `mean: f64`, `m2: f64`, `initialized: bool`) |
| Lifetime mode (`window="forever"`) | **Allowed** — classified `O1` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: Bot-detection cadence per IP

```python
import beava as bv

@bv.event
class Click:
    ip: str
    user_agent: str

@bv.table(key="ip")
def IpCadence(clicks) -> bv.Table:
    return (
        clicks.group_by("ip")
              .agg(mean_gap_1h=bv.inter_arrival_stats(window="1h"))
    )

# Push events in arrival order
app.push("Click", {"ip": "1.2.3.4", "user_agent": "bot/1.0"}) # mean = null
app.push("Click", {"ip": "1.2.3.4", "user_agent": "bot/1.0"}) # mean = Δ1
app.push("Click", {"ip": "1.2.3.4", "user_agent": "bot/1.0"}) # mean = avg(Δ1, Δ2)
app.push("Click", {"ip": "1.2.3.4", "user_agent": "bot/1.0"}) # mean = avg(Δ1, Δ2, Δ3)

# Query
result = app.get("IpCadence", "1.2.3.4")
# result == {"mean_gap_1h": <ms-between-clicks>}
# Suspiciously low + uniform → bot. v0.1+ will expose stddev / CV inline.
```

### Example 2: Filtered transaction-cadence per card

```python
@bv.table(key="card_id")
def CardOkCadence(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(mean_ok_gap=bv.inter_arrival_stats(
                     window="30m",
                     where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "IpCadence",
  "output_kind": "table",
  "key": ["ip"],
  "agg": {
    "mean_gap_1h": {
      "op": "inter_arrival_stats",
      "params": {
        "window": "1h"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start (`n = 0`):** result is `null`. The first matching event seeds `last_t = now_ms` and flips `initialized` but no gap is recorded yet (`n` stays 0).
- **Single-event entity:** result is still `null` — at least two matching events are required for one inter-arrival gap.
- **Late or duplicate event (`Δt ≤ 0`):** clamped to `0` via `(now_ms - last_t).max(0)` before folding into the Welford accumulator. Time never moves backward.
- **`where=` filter excludes the event:** no update; non-matching events do not advance `last_t` either, so a non-matching event in the middle of a sequence does not artificially inflate the next gap.
- **`field=` argument passed:** raises `TypeError` at SDK-helper-call time — `inter_arrival_stats` is field-less by design.
- **Missing `window=`:** raises `ValueError` at SDK-helper-call time.
- **Malformed `window=`:** raises `ValueError` at SDK-helper-call time; if it somehow reaches the server, `register_validate.rs` returns structured error `aggregation_invalid_window`.
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the underlying state; the next post-eviction matching event reseeds `last_t` and resets `n` to 0.
- **Future return-shape widening:** v0.1+ expects `{mean_ms, stddev_ms, cv}` as a dict; the wire `op` value `"inter_arrival_stats"` is forward-compatible — the change lands in the SDK return-decoder, not in the wire payload.

## See also

- [Velocity family index](./index.md) — overview of all 9 velocity-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.burst_count](./burst_count.md) — companion "max events in any sub-window" primitive
- [bv.last_seen](../recency/last_seen.md) — most-recent arrival timestamp (the basic recency primitive that pairs naturally with cadence statistics)
- [bv.streak](../recency/streak.md) — consecutive-match count (event-order, not gap-statistic)
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
