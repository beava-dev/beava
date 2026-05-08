# bv.twa

> Time-weighted average for irregularly-sampled gauge fields.

## Signature

```python
bv.twa(
    field: str,
    *,
    window: str,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.twa` returns the **time-weighted average** of a gauge-style field —
the integral of `value` against arrival time divided by elapsed time.
On each matching event the helper accumulates
`sum_v_dt += last_v * (now - last_t)` and `sum_dt += (now - last_t)`,
then sets `last_v = x`, `last_t = now`. At query time the value is
`sum_v_dt / sum_dt` (or `last_v` if only one observation has been
recorded). Time deltas use **server processing-time** (`now_ms()` at
arrival) per
[`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md);
beava intentionally has no event-time concept.

The point of TWA is to handle gauges that are reported at irregular
intervals — CPU utilisation, queue depth, thermostat reading, current
balance — where a plain `bv.mean` would over-weight whichever sample
was reported most often. `bv.twa("cpu_util", window="5m")` answers
"what was the time-weighted average CPU utilisation over the last 5
minutes?" — a sample reported once and held for 4 minutes contributes
4× as much as a sample reported and immediately replaced. Use TWA
whenever the *time the value was held* matters more than the *number of
times it was reported*.

`bv.twa` belongs to the **decay** family (it lives next to EWMA in the
catalogue because both are time-weighted, even though TWA does not
decay — it averages held-time-weighted exactly). Per-event update is
four scalar operations; cost is **Tier 1** (~15 ns algorithm floor /
~35 ns measured) and memory is `O(1)` per entity. Unlike the other
decay ops, `bv.twa` requires a `window=` kwarg (not `half_life`); the
windowing reuses the standard bucket machinery for fixed-horizon TWA,
and `window="forever"` is allowed for a lifetime TWA per
`crates/beava-core/src/register_validate.rs` (TWA's lifetime bound is
classified as `O1`).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Numeric field (`i64` or `f64`) — the gauge value. |
| `window` | `str` | Yes | — | Duration string matching `\d+(ms\|s\|m\|h\|d)` or `"forever"`. Required (TWA without a horizon would have no defined denominator). |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the running integral. |

## Returns

A single `f64` — the time-weighted average. Cold-start (no matching
events) returns `null` (Python `None`). After exactly one matching
event the value is the gauge sample itself (no held-time integral yet).

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~15 ns floor / ~35 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | `O(1)` — `(sum_v_dt, sum_dt, last_v, last_t, initialized)` ≈ 40 B |
| Lifetime mode (`window="forever"`) | **Allowed** — TWA classified as `O1` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |

## Examples

### Example 1: TWA of CPU utilisation per host, 5m window

```python
import beava as bv

@bv.event
class HostMetric:
    host_id: str
    cpu_util: float

@bv.table(key="host_id")
def HostCpuTwa(metrics) -> bv.Table:
    return (
        metrics.group_by("host_id")
               .agg(cpu_twa_5m=bv.twa("cpu_util", window="5m"))
    )

# Push events at irregular intervals
app.push("HostMetric", {"host_id": "node-01", "cpu_util": 0.20})
# 4 minutes of high load reported as one sample at the start...
app.push("HostMetric", {"host_id": "node-01", "cpu_util": 0.95})
# ...then a flurry of low-utilisation samples in the next minute
app.push("HostMetric", {"host_id": "node-01", "cpu_util": 0.10})
app.push("HostMetric", {"host_id": "node-01", "cpu_util": 0.05})

result = app.get("HostCpuTwa", "node-01")
# result == {"cpu_twa_5m": <weighted toward 0.95 because that sample was
# held for 4× longer than the trailing samples>}
```

### Example 2: Lifetime TWA of account balance, only after activation

```python
@bv.table(key="account_id")
def AccountAvgBalance(snapshots) -> bv.Table:
    return (
        snapshots.group_by("account_id")
                 .agg(balance_twa=bv.twa("balance",
                                            window="forever",
                                            where=bv.col("activated") == True))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "HostCpuTwa",
  "output_kind": "table",
  "key": ["host_id"],
  "agg": {
    "cpu_twa_5m": {
      "op": "twa",
      "params": {
        "field": "cpu_util",
        "window": "5m"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Empty stream / cold-start:** result is `null`. The first matching event seeds `last_v = x`, `last_t = now`, with no `sum_v_dt` contribution yet (no held-time elapsed).
- **Single matching event:** `sum_dt == 0`, so the query returns `last_v` directly (the sole observation).
- **Late or duplicate event (Δt ≤ 0):** `dt = max(now - last_t, 0)`; if `dt == 0` no integral contribution is added but `last_v` is still updated to `x` (replaces the same-instant gauge value with the newer one).
- **Missing or non-numeric `field`:** the event is silently skipped.
- **`where=` filter excludes the event:** no update.
- **Missing `window=`:** raises `ValueError` at SDK-helper-call time. `_validate_window(window, "twa", requires_window=True)` enforces it.
- **`window="forever"`:** explicitly allowed; the helper integrates over the full lifetime of the entity. Footprint stays `O(1)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).
- **No new events for a long time:** the held-time integral stops accumulating at `last_t` and only resumes on the next matching event. (Like `bv.decayed_sum`, querying does not mutate state — there is no "decay forward to now" behaviour.)
- **Cold-entity eviction (`@bv.event(cold_after=...)`):** drops the underlying state.

## See also

- [Decay family index](./index.md) — overview of all 6 decay-family ops
- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.mean](../core/mean.md) — arithmetic mean over a fixed window (use this when *number of samples* matters, not *time the value was held*)
- [bv.ewma](./ewma.md) — exponentially-weighted moving average (use this when older observations should fade smoothly rather than the current TWA semantics where every sample contributes proportional to its held duration)
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
