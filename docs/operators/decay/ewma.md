# bv.ewma

> Exponentially-weighted moving average over arrival-time, with `half_life`-controlled decay.

## Signature

```python
bv.ewma(
    field: str,
    *,
    half_life: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.ewma` is an exponentially-weighted moving average — a single running
estimate of "what is this entity's typical value of `field` lately?",
with the influence of older events decaying exponentially with arrival
age. Each new observation updates the estimate as
`value_t = α * x_t + (1 - α) * value_{t-1}`, where the decay coefficient
`α = 1 - 0.5^(Δt / half_life)` (equivalently `1 - exp(-Δt * ln(2) / half_life)`).
`Δt` is the **server processing-time** gap (`now_ms()` at this event minus
`now_ms()` at the previous event) per
[`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md) —
beava intentionally has no event-time concept, so older events here means
"older by arrival order, weighted by elapsed wall-time between arrivals".

`half_life` is the time after which an observation's influence has decayed
to ½. So `bv.ewma("amount", half_life="1h")` means "an event 1h old
contributes half as much as a brand-new event; an event 2h old contributes
¼; an event 3h old, ⅛". Use `bv.ewma` when you want a smoothed running
estimate of a numeric field that adapts to drift faster than a long
fixed-window mean would (e.g. a 5-minute mean drops history sharply at
5m, while EWMA decays smoothly), or when the right window length is
genuinely uncertain — pick a half-life equal to the timescale of the
behaviour you care about and let history fade naturally.

`bv.ewma` belongs to the **decay** family. Per-event update is one
`exp()` call plus three scalar multiplies; cost is **Tier 1** (~15 ns
algorithm floor / ~35 ns measured) and memory is `O(1)` per entity
(three slots: `value`, `last_now_ms`, `initialized`). Lifetime mode is
the only mode — `half_life` sets the decay rate, no `window=` kwarg
exists.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Numeric field (`i64` or `f64`) to track. |
| `half_life` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)`. Must be positive; `"forever"` is **rejected** (decay with infinite half-life is just a running last-value). |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the EWMA. |

## Returns

A single `f64` — the current EWMA estimate. Cold-start (no matching
events seen) returns `null` (Python `None`).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~15 ns floor / ~35 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `(value: f64, last_now_ms: i64, initialized: bool)` ≈ 24 B |
| Lifetime mode | **Required** — no `window=` kwarg; `half_life` controls decay rate |

## Examples

### Example 1: EWMA of transaction amount per user, 1h half-life

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    amount: float

@bv.table(key="user_id")
def UserAmtEwma(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(amt_ewma_1h=bv.ewma("amount", half_life="1h"))
    )

# Push events
app.push("Txn", {"user_id": "alice", "amount": 100.0}) # value = 100
app.push("Txn", {"user_id": "alice", "amount": 200.0}) # value blends toward 200

# Query
result = app.get("UserAmtEwma", "alice")
# result == {"amt_ewma_1h": <float between 100 and 200, biased by elapsed Δt>}
```

### Example 2: Filtered EWMA of approved-payment amounts, 30m half-life

```python
@bv.table(key="user_id")
def UserOkAmtEwma(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(ok_amt_ewma=bv.ewma("amount",
                                      half_life="30m",
                                      where=bv.col("status") == "ok"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserAmtEwma",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "amt_ewma_1h": {
      "op": "ewma",
      "params": {
        "field": "amount",
        "half_life": "1h"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null`. The first matching event seeds `value = x` and flips the `initialized` flag.
- **Late or duplicate event (Δt ≤ 0):** the helper applies an unweighted blend (`value = 0.5 * x + 0.5 * value`) and does **not** advance `last_now_ms`. Time never moves backward; this preserves replay determinism.
- **Missing or non-numeric `field`:** the event is silently skipped (no update); the EWMA value is unchanged. This matches `bv.mean` / `bv.sum` behaviour.
- **`where=` filter excludes the event:** no update; non-matching events do not advance `last_now_ms` either.
- **Missing `half_life=`:** raises `ValueError` at SDK-helper-call time.
- **`half_life="forever"`:** rejected by `_validate_half_life` with `ValueError` — decay with infinite half-life would freeze on the first observation; use [`bv.first`](../point-ordinal/first.md) for that semantic.
- **`half_life="0…"`:** rejected at SDK call time (regex requires `[1-9]\d*` prefix). Server-side, `register_validate.rs` returns structured error `aggregation_invalid_half_life` if a malformed `half_life` somehow reaches it.
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the underlying state; the next event after eviction seeds a new EWMA from `x`.

## Aliases

- `bv.ema` — same op; alias preserved as a convention shortcut.
  `ema` and `ewma` map to the same `AggKind::Ewma` variant in `crates/beava-core/src/agg_op.rs` and the same `O(1)` lifetime-bound classification (`crates/beava-core/src/register_validate.rs` line ~436 lists `"ewma" | "ema"` together). The Python helper `bv.ema(...)` is a thin pass-through to `bv.ewma(...)` (`python/beava/_agg.py` line 345-347). Choose whichever name reads better in your code; the wire `op` value is `"ewma"` for both.

## See also

- [Decay family index](./index.md) — overview of all 6 decay-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.ewvar](./ewvar.md) — companion exponentially-weighted variance (state-shares the same `last_now_ms` convention)
- [bv.ew_zscore](./ew_zscore.md) — current event z-score against EWMA / EWVar baseline
- [bv.mean](../core/mean.md) — fixed-window arithmetic mean (no decay; pick this when window is fixed)
- [bv.twa](./twa.md) — time-weighted average for irregularly-sampled gauge fields
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
