# bv.distance_from_home

> Distance (km) of the **current** event from the running centroid of the entity's last `samples` matching events. `samples` is a soft-defaulted register-time kwarg per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — `BoundedByConfig("samples", 100)`.

## Signature

```python
bv.distance_from_home(
    *,
    lat: str, # REQUIRED — name of the latitude field on the event
    lon: str, # REQUIRED — name of the longitude field on the event
    samples: int = 100, # SOFT default — BoundedByConfig per V0-MEM-GOV-02
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.distance_from_home` returns the **haversine distance (in km) from
the current event's `(lat, lon)` to the running centroid of the
entity's last `samples` matching events**. State is a circular buffer
of capacity `samples` storing recent `(lat, lon)` pairs plus the
last-observed point. On every accepted event the buffer is overwritten
at the head index and the head advances modulo `samples`. The query
computes the arithmetic-mean centroid of the buffer's current contents,
then returns `haversine(last, mean_centroid)`. Use it for "**how far
is this transaction from where this user usually transacts?**" — a
classical home-anomaly fraud signal that scores the current event
against the entity's recent geographic baseline.

`samples` is **optional with a default of 100** per
[V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md): the
lifetime-aggregation memory contract requires every unbounded-by-default
operator to declare a finite per-entity ceiling at register time, but
for `distance_from_home` the ceiling is **soft-defaulted**
(`BoundedByConfig("samples", 100)`) — the kwarg is OPTIONAL, but the
per-entity ring is always sized at `samples × 16 bytes` (two `f64`s per
slot) regardless of caller behavior. Per `register_validate.rs` line
482, the JSON-prelude shim's `pre_check_unbounded_op_in_lifetime_mode`
treats absent-`samples` as the default rather than a register-time
rejection. The default of 100 fits the "centroid of recent home
locations" use case at ~1.6 KB/entity. Pick a larger value (e.g.
`samples=500`) for entities with sparse activity where the last 100
events poorly characterise their home; pick smaller (e.g.
`samples=50`) for memory-sensitive deployments at the cost of more
volatile centroids.

`bv.distance_from_home` belongs to the **bounded-buffer + geo** family.
Per-event UPDATE is **Tier 3 (~12 ns floor / ~32 ns measured —
write to ring buffer at head index, O(1))** per
[cost-class.md](../cost-class.md#tier-3-algorithmic-floor-100-300-nscall--9-ops);
QUERY iterates the ring for the centroid (O(samples) — at most 100
points by default, cold-path on `app.get(...)`). Update cost is
effectively Tier 1; the cost class lists this op in Tier 3 because the
query path can dominate in query-heavy pipelines. State is behind a
`Box` for the `AggOp::DistanceFromHome` variant per v0 boxing
(the variant fits the 80-byte `AggOp` enum cap; see
`crates/beava-core/src/agg_op.rs` line 489 and
[SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md)).
There is no `window=` kwarg in v0 — `bv.distance_from_home` is
**lifetime-only**. The "home" is implicitly bounded by the ring's
`samples` capacity (newest event displaces oldest after ring fills);
for a time-bounded "home", compose with `@bv.event(cold_after=...)`
per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `lat` | `str` | **Yes** | — | Name of the latitude field on the event (NOT a literal coordinate). Field value must be `f64` or `i64` decimal degrees in `[-90, 90]`. Resolved to a column index at register time. |
| `lon` | `str` | **Yes** | — | Name of the longitude field on the event. Field value must be `f64` or `i64` decimal degrees in `[-180, 180]`. |
| `samples` | `int` | No | `100` | Capacity of the circular buffer of recent locations whose mean defines "home". Soft-defaulted per [V0-MEM-GOV-02 BoundedByConfig("samples", 100)](../../../.planning/REQUIREMENTS.md). Clamped to `≥ 1` at state construction. Per-entity memory: `samples × 16` bytes. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the ring + last. |

## Returns

A scalar `float` — the haversine distance in km from the current
(latest matching) event to the centroid of the buffer's current
contents. **Cold-start (zero matching events): returns `null`**. The
first event seeds the buffer (1 point in the partial ring) and `last`
to the same point — the centroid equals the latest observation, so
the first query returns `0.0`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event (UPDATE) | **Tier 3** (~12 ns floor / ~32 ns measured — ring-buffer write at head index, O(1)) — see [cost-class.md](../cost-class.md#tier-3-algorithmic-floor-100-300-nscall--9-ops). Update cost is effectively Tier 1; classified Tier 3 because of the query-time iteration cost (see next row). |
| CPU per query | **O(`samples`)** centroid iteration — at most `samples` × (2 f64 reads + 1 add) per `app.get(...)`. ~100 ns at `samples=100`. Cold-path; doesn't dominate apply-thread budget but flag if your pipeline is query-heavy. |
| Memory per entity | **`BoundedByConfig("samples", 100)`** — `samples × 16` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). At default `samples=100`: 1600 bytes for the ring + ~16 bytes for the last-point memo. Boxed inside `AggOp` per v0 (`crates/beava-core/src/agg_op.rs` line 489). |
| Lifetime mode | **Required** — `bv.distance_from_home` has no `window=` kwarg in v0; lifetime is the only mode. |

## Examples

### Example 1: Per-card distance from recent transaction centroid

```python
import beava as bv

@bv.event
class Txn:
    card_id: str
    latitude: float
    longitude: float

@bv.table(key="card_id")
def CardDistanceFromHome(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(km_from_home=bv.distance_from_home(
                lat="latitude",
                lon="longitude",
                samples=100,
            ))
    )

# After 100+ Boston-area transactions, then a Las Vegas swipe
result = app.get("CardDistanceFromHome", "card_xyz")
# result == {"km_from_home": 4128.5} # ~4100 km from the Boston centroid
```

### Example 2: Smaller ring for memory-sensitive deployments

```python
@bv.table(key="user_id")
def UserDistanceFromHomeSmall(events) -> bv.Table:
    return (
        events.group_by("user_id")
              .agg(km_from_home=bv.distance_from_home(
                  lat="lat",
                  lon="lon",
                  samples=50, # 800 bytes/entity; more volatile centroid
              ))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "CardDistanceFromHome",
  "output_kind": "table",
  "key": ["card_id"],
  "agg": {
    "km_from_home": {
      "op": "distance_from_home",
      "params": {
        "lat": "latitude",
        "lon": "longitude",
        "samples": 100
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Cold-start / zero matching events:** result is `null` — `last` is `None`, no centroid to compare against.
- **First matching event:** result is `0.0` — `last` and the single buffer point coincide; haversine is zero.
- **Buffer not yet full (`< samples` events seen):** centroid is the mean of the partial buffer (e.g. mean of 5 points after 5 events when `samples=100`). Newer events extend the buffer until it fills; subsequent events overwrite at the head index.
- **`samples=0` or negative `samples`:** clamped to `1` at state construction (`samples.max(1)`). The ring degenerates to "distance from the previous matching event" (single-slot buffer).
- **`samples` missing at register time:** defaults to `100` per `BoundedByConfig("samples", 100)` — does NOT trigger `unbounded_op_in_lifetime_mode`. The ring is still bounded, just at the soft default.
- **`lat` or `lon` missing on the event:** event is silently dropped (no buffer write, no `last` update). For stricter behavior, gate with `where=~bv.col("lat").isnull() & ~bv.col("lon").isnull()`.
- **Non-numeric `lat` / `lon` (`Value::Str`, `Value::Bool`, `Value::Null`):** event is silently dropped (`read_lat_lon` returns `None`).
- **All matching events at the same point:** centroid coincides with the latest observation; result is `0.0`. The home is wherever the entity has been.
- **Antimeridian crossings:** the arithmetic-mean centroid does NOT handle longitude wrap correctly (e.g. mean of `lon=179` and `lon=-179` is `0`, not `±180`). Workaround: shift longitudes into a continuous range upstream when the entity straddles the antimeridian.
- **Polar latitudes (`|lat| > ~85°`):** haversine itself stays accurate; the arithmetic-mean centroid in degree-space is a reasonable approximation away from the poles. Sub-arctic deployments should validate.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. v0's geo ops are lifetime-only. For "distance from home over the last 30 days", compose with `@bv.event(cold_after="30d")`.
- **Snapshot reload:** the ring buffer + head index serialize/deserialize cleanly via `serde`. WAL replay reconstructs the same buffer contents in the same head order, so the centroid is replay-deterministic.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); the ring is populated in arrival order. The "last" event for the query is whichever matching event arrived most recently.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `samples × 16` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) `BoundedByConfig("samples", 100)`.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 3 — query-iteration class; update is Tier 1)
- [bv.geo_velocity](./geo_velocity.md) — max-implied-km/h sibling (consecutive-event speed, not centroid distance)
- [bv.geo_distance](./geo_distance.md) — total-path-length sibling (cumulative km, not centroid distance)
- [bv.geo_spread](./geo_spread.md) — RMS-dispersion sibling (how spread out vs centroid distance — both express "geographic baseline")
- [bv.most_recent_n](./most_recent_n.md) — generic last-N-values sibling (this op is the geo-specific specialisation that computes a centroid + distance)
- [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md) — cold-entity eviction (`@bv.event(cold_after=...)`) for time-bounded "home"
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — `BoundedByConfig` lifetime-aggregation contract
- [SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md) — `AggOp::DistanceFromHome` boxing context
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
