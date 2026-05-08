# bv.time_since_last_n

> Milliseconds since the kth most recent matching event. `n` is a required register-time kwarg per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). Server processing-time per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md).

## Signature

```python
bv.time_since_last_n(
    *,
    n: int, # REQUIRED — register-time kwarg
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.time_since_last_n` returns the elapsed milliseconds between the
**oldest of the last `n` matching arrivals** and the query time. It
generalizes [`bv.time_since`](./time_since.md) (which is the `n = 1` case):
"how many ms since the 5th most recent successful login on this card?",
"how long ago did the 10th most recent ad impression land for this user?".

Internally `time_since_last_n` keeps a `VecDeque<i64>` of capacity `n`
holding the server `now_ms()` of the last `n` matching events. On each
match, the deque pushes the current `now_ms()` onto the back; once full,
the next push pops from the front. The query reads the front of the
deque (the oldest of the surviving `n` timestamps) and returns
`now_ms() - oldest`. Until the deque holds `n` entries (i.e., until at
least `n` matching events have been seen), the query returns `null`.

`n` is a **required keyword argument** per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md):
the lifetime-aggregation memory contract requires every unbounded-by-default
operator to declare a finite per-entity ceiling at register time.
`bv.time_since_last_n`'s ceiling is `n × 8` bytes (a deque of `i64`).
The register-time JSON-prelude shim (`pre_check_unbounded_op_in_lifetime_mode`)
rejects any `time_since_last_n` payload without `n` with the structured
error code `unbounded_op_in_lifetime_mode`. Picking `n` is a deliberate
capacity-planning step.

All timestamps are **server processing-time** — beava intentionally has no
event-time concept per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md)
(locked 2026-04-30). Producers cannot influence the captured `now_ms()`
values via the payload.

`bv.time_since_last_n` belongs to the **recency** family. Per-event update
is push_back + conditional pop_front (both O(1) on `VecDeque`); the read
side is one front-element read plus one subtraction. There is no `window=`
kwarg — `bv.time_since_last_n` is **lifetime-only**.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `n` | `int` | **Yes** | — | How far back to look. The result is "ms since the `n`th most recent match". Must be `≥ 1` per [V0-MEM-GOV-02 BoundedByRequiredKwarg("n")](../../../.planning/REQUIREMENTS.md). |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events advance the deque. |

## Returns

A single `i64` value: ms between the oldest of the last `n` matching
arrivals and the query-time `now_ms()`. Returns `null` (Python `None`) if
fewer than `n` matching events have been seen. Clamped to `>= 0` for
clock-skew safety.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~12 ns floor / ~35 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | **`BoundedByRequiredKwarg("n")`** — `n × 8` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.time_since_last_n` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: Time since the 5th most recent successful login per user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    status: str

@bv.table(key="user_id")
def UserSinceLast5Success(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(since_5th_ok=bv.time_since_last_n(n=5,
                                                       where=bv.col("status") == "ok"))
    )

# After 5 successful logins arrive at t = [1000, 2000, 3000, 4000, 5000] ms
# and the read happens at server time t = 7000 ms:
# time_since_last_n returns 7000 - 1000 = 6000 ms
# A 6th successful login at t = 6000 evicts the oldest (1000); next read
# at t = 7000 returns 7000 - 2000 = 5000 ms.
```

### Example 2: How fresh is the 10th most recent ad impression?

```python
@bv.table(key="user_id")
def UserSinceLast10Impressions(events) -> bv.Table:
    return (
        events.group_by("user_id")
              .agg(since_10th_imp_ms=bv.time_since_last_n(n=10))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserSinceLast5Success",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "since_5th_ok": {
      "op": "time_since_last_n",
      "params": {
        "n": 5,
        "where": "status == 'ok'"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **`n` missing at register time:** rejected with structured error code `unbounded_op_in_lifetime_mode` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). The register-time JSON-prelude shim catches this before any state is allocated.
- **`n=0` or negative `n`:** rejected by the SDK helper's pre-validation; the wire-level shim catches it as a fallback.
- **Fewer than `n` matching events:** result is `null`. The deque must hold exactly `n` timestamps before there is a `time_since_last_n` to compute.
- **Empty stream / cold-start:** result is `null`.
- **`where=` filter excludes everything:** the deque never advances; result stays `null`.
- **Reads grow without new events:** `time_since_last_n` increases monotonically between reads when no new matches arrive — same dynamic as [`bv.time_since`](./time_since.md).
- **Reads drop on a new match (once full):** every new match evicts the oldest timestamp; the new "oldest" is fresher, so the next read returns a smaller value.
- **Clock-skew safety:** result is clamped to `>= 0`.
- **Server-time, NOT event-time:** all timestamps are server-side `now_ms()` per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md). Producers cannot influence them.
- **Cold-entity eviction:** if [`@bv.event(cold_after=...)`](../../../.planning/REQUIREMENTS.md) evicts the entity, the deque is dropped and result returns `null` until `n` post-eviction matches accumulate (Redis-TTL pattern, V0-MEM-GOV-01).
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `n × 8` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) BoundedByRequiredKwarg("n").

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.time_since](./time_since.md) — degenerate `n=1` case (lighter — no deque allocation)
- [bv.last_seen](./last_seen.md) — absolute timestamp of the most recent match (the right-hand input for `n=1`)
- [bv.last_n](../point-ordinal/last_n.md) — symmetric "**values** of the last n matches" instead of timestamps
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — BoundedByRequiredKwarg memory governance contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
