# Processing-Time Only (No Event-Time)

beava's only time source is server-side processing time — `now_ms()`
sampled at the moment the apply loop processes the event. There is no
event-time, no watermark, no late-event handling, no out-of-order
reordering, no time-travel queries. State is a function of arrival-order
events plus the query time, and that's the whole model.

This is a permanent architectural commitment, not a v0 simplification.
The locked memory file is `project_redis_shaped_no_event_time_ever`,
locked 2026-04-30. Reviving event-time semantics requires an explicit
user override and a new ADR.

## What "no event-time ever" means

- **All windowed operators** (`bv.count(window="1h")`,
  `bv.sum("amount", window="5m")`, etc.) bucket by server-side `now_ms()`
  at apply time. The event payload may carry a timestamp field, but the
  windowing engine never reads it for bucket assignment.
- **Recency operators** (`first_seen`, `last_seen`, `time_since`, `age`,
  `streak`) record the server's arrival timestamp, not anything the event
  carries.
- **No `event_time_ms` field on the wire.** The wire spec has no slot
  for one, and the server's JSON-prelude shim rejects pushes that try to
  smuggle one in. See
  [pre-existing shim error codes](../error-codes.md): pushes with
  `event_time_ms` return `unknown_field_event_time_v0`.
- **No `event_time_field=` decorator kwarg.** `@bv.event` does not accept
  it; passing it returns `unknown_field_event_time_v0` at register-time.
- **No `tolerate_delay_ms=`** — there's no concept of "lateness," so
  there's no tolerance. The shim returns `unknown_field_tolerate_delay_v0`.
- **No watermarks, no late-event handling, no out-of-order reordering.**
  Events arrive, get processed in arrival order, that's the order. If you
  push two events with the "same" event-time but they arrive at the
  server at different times, beava treats them as two events in the
  arrival order it observed them.
- **No joins.** Event ↔ event windowed joins, event ↔ table enrichment,
  table ↔ table key-matched joins — all forever-rejected as part of the
  same architectural commitment. State is a function of arrival-order
  events on a single keyspace, queried at read time. Compose via push/get
  and entity-key sharding.

If your code attempts any of these, the structured error code is
`event_time_not_supported_in_v0` (or one of the shim codes above for
specific field names).

## Why no event-time

This is a deliberate scope-and-correctness decision, not a deferred
feature. Three reasons:

1. **Operational simplicity.** Event-time + watermarks adds a whole
   subsystem (allowed-lateness configuration, watermark generation,
   trigger semantics, late-firing semantics, garbage-collection windows).
   That subsystem buys you correctness in the face of out-of-order
   arrivals — a property whose value depends entirely on whether your
   producers actually misorder events. beava's target workload (fraud /
   ad-tech / behavioral analytics) consumes from sources that are
   in-order or near-enough.
2. **Mental model parity with Redis.** beava is "Redis for stateful
   streaming features." Redis has no event-time and no one wants it to.
   Users push, users get, the answer is whatever the current state says.
   beava sits in the same operational slot.
3. **Eliminates a whole class of correctness bugs.** Watermark-driven
   pipelines have failure modes (early-firing, late-firing, allowed
   lateness misconfiguration, dropped events past the watermark) that
   take significant operator skill to manage. beava doesn't have them
   because the model doesn't admit them.

The trade is real: if your use case is *historical replay with original
event timestamps* (e.g. backfilling a feature pipeline against a year of
S3-archived events and getting the same per-day rollups you'd get from
real-time), beava cannot help. That is the v0.1+ historical extraction
engine — see
[`.planning/ideas/v0.1-historical-extraction-engine.md`](../../.planning/ideas/v0.1-historical-extraction-engine.md).

## Implications for users

- **Push events as soon as they happen.** Don't queue events to send in
  bulk; the server timestamps them on arrival, so a 1-hour delay between
  event and push will show up as a 1-hour shift in any windowed
  aggregation.
- **If you must replay history**, the result reflects current `now_ms()`
  at the moment of replay, not the event's original timestamp. A
  back-fill of historical purchase events into a `count(window="1h")`
  aggregation will all land in the current 1-hour bucket, not in the
  buckets they "should" have landed in. This is correct given the
  semantics, just probably not what you wanted.
- **For event-time semantics**, wait for v0.1+. The historical extraction
  engine spec uses arrival-time-as-event-time semantics in the live path
  and a separate offline replay path for backfill. Same data model on
  both sides.
- **Time-source determinism.** `now_ms()` is read once per event at the
  start of the apply step and reused across every aggregation that
  processes that event. So within a single push, all windowed ops see
  the same `now_ms` — no within-event drift.

## What you still get

The "no event-time" choice does not mean "no time semantics." It means
"server-side time, processing-time only." You still get:

- Sliding windows (`window="1h"`, `window="5m"`, etc.) bucketed by
  server-clock `now_ms()`.
- Recency markers (`time_since`, `age`, `first_seen`, `last_seen`) using
  server-clock arrival timestamps.
- Decay-family operators (`ewma`, `decayed_sum`, `twa`) using `now_ms()`
  delta from each previous arrival.
- Cold-entity TTL eviction (`@bv.event(cold_after="30d")`) — see
  [V0-MEM-GOV-01](../architecture/memory-budget.md). The "30 days" is
  measured against server-side `last_seen_ms`, not anything the event
  carries.

The whole windowed-aggregation surface stays available — it just runs on
processing-time semantics.

## Cross-references

- `~/.claude/projects/-Users-petrpan26-work-tally/memory/project_redis_shaped_no_event_time_ever.md`
  — locked architectural commitment; the canonical "why."
- [`CLAUDE.md` § mio-only Hot-Path Invariant](../../CLAUDE.md) — single
  data-plane runtime; the same locked-commitment family.
- [error-codes.md](../error-codes.md) —
  `event_time_not_supported_in_v0`, `unknown_field_event_time_v0`,
  `unknown_field_tolerate_delay_v0`, `joins_not_supported`,
  `feature_removed_no_joins_v0`, `feature_removed_no_unions_v0`.
- [`.planning/ideas/v0.1-historical-extraction-engine.md`](../../.planning/ideas/v0.1-historical-extraction-engine.md)
  — v0.1+ replay-style engine for historical data.
- [pipeline-dsl/compilation-rules.md](../pipeline-dsl/compilation-rules.md)
  — `window=` kwarg semantics; ambiguity matrix rows for forbidden
  event-time patterns.
