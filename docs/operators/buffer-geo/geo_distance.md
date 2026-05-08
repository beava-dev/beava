# bv.geo_distance

> Total great-circle path length (km) traversed across consecutive matching events. Reads `lat` and `lon` field-name kwargs at register time; sums haversine distance between adjacent points.

## Signature

```python
bv.geo_distance(
    *,
    lat: str, # REQUIRED — name of the latitude field on the event
    lon: str, # REQUIRED — name of the longitude field on the event
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.geo_distance` returns the **cumulative ground-track length in
kilometres** the entity has traversed across all matching events seen
so far. On every accepted event the operator reads `(lat, lon)` from
the register-time-named fields, computes the haversine great-circle
distance to the previously-stored point, and adds it to a running
total. Use it for "**total km a delivery driver has covered today**",
"**total path length of a phone before the SIM swap**", or "**how far
has this rideshare account moved over the lifetime of the trip**" —
features that need cumulative travel as a fraud, abuse, or
operational signal.

`lat` and `lon` are **required keyword arguments** that name two
fields on the event (e.g. `lat="latitude"`, `lon="longitude"`) — they
are NOT literal coordinates. Distance math goes through the
`haversine` crate (great-circle on a spherical Earth, mean radius
**6371 km** per `agg_geo.rs::haversine_km`). Each segment is the
haversine of `prev → current`, summed into a `f64` accumulator. There
is no notion of a "trip" or "session" boundary — the accumulator runs
for the entity's lifetime. To window it, drop the entity state via
`@bv.event(cold_after=...)` or scope by entity-key (one entity per
trip).

`bv.geo_distance` belongs to the **bounded-buffer + geo** family.
State is `O(1)` per entity — `Option<(f64, f64)>` for the previous
point + `f64` for the running total — behind a `Box` for the
`AggOp::GeoDistance` variant per v0 boxing (the variant fits
the 80-byte `AggOp` enum cap; see `crates/beava-core/src/agg_op.rs`
line 487 and [SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md)).
Per-event update is **Tier 2** (~18 ns floor / ~42 ns measured per
[cost-class.md](../cost-class.md#tier-2-moderate-30-100-nscall--6-ops)) —
two field reads + one haversine + one floating-point add. There is no
`window=` kwarg in v0 — `bv.geo_distance` is **lifetime-only**.
Compose with `@bv.event(cold_after="...")` for time-bounded
accumulation per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `lat` | `str` | **Yes** | — | Name of the latitude field on the event (NOT a literal coordinate). Field value must be `f64` or `i64` decimal degrees in `[-90, 90]`. Resolved to a column index at register time. |
| `lon` | `str` | **Yes** | — | Name of the longitude field on the event. Field value must be `f64` or `i64` decimal degrees in `[-180, 180]`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events are added to the path total. |

## Returns

A scalar `float` — the cumulative kilometres of ground track for the
entity. **Cold-start (zero or one matching event): returns `0.0`** —
not `null`. The first event seeds `prev = (lat, lon)` with no
distance accumulated; subsequent events add `haversine(prev, current)`
to the total.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 2** (~18 ns floor / ~42 ns measured — two field reads + haversine + one f64 add) — see [cost-class.md](../cost-class.md#tier-2-moderate-30-100-nscall--6-ops). The haversine is the irreducible Tier 2 cost. |
| Memory per entity | **`O(1)`** — `Option<(f64, f64)>` for the previous point + `f64` for the running total. Boxed inside `AggOp` per v0 (`crates/beava-core/src/agg_op.rs` line 487). |
| Lifetime mode | **Required** — `bv.geo_distance` has no `window=` kwarg in v0; lifetime is the only mode. |

## Examples

### Example 1: Total km per delivery driver

```python
import beava as bv

@bv.event
class GpsPing:
    driver_id: str
    latitude: float
    longitude: float

@bv.table(key="driver_id")
def DriverTotalKm(pings) -> bv.Table:
    return (
        pings.group_by("driver_id")
             .agg(total_km=bv.geo_distance(lat="latitude", lon="longitude"))
    )

# After 50 GPS pings tracing a 12 km route
result = app.get("DriverTotalKm", "driver_42")
# result == {"total_km": 12.34}
```

### Example 2: Distance per session, scoped via cold-entity TTL

```python
@bv.event(cold_after="2h") # entity state drops after 2h of inactivity
class TripPing:
    trip_id: str
    lat: float
    lon: float

@bv.table(key="trip_id")
def TripPathLength(pings) -> bv.Table:
    return (
        pings.group_by("trip_id")
             .agg(path_km=bv.geo_distance(lat="lat", lon="lon"))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "DriverTotalKm",
  "output_kind": "table",
  "key": ["driver_id"],
  "agg": {
    "total_km": {
      "op": "geo_distance",
      "params": {
        "lat": "latitude",
        "lon": "longitude"
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Cold-start / zero matching events:** result is `0.0` (not `null`). The semantic is "no distance accumulated yet".
- **Single matching event:** result is `0.0` — the first event seeds `prev` but contributes no segment to the path.
- **`lat` or `lon` missing on the event:** event is silently dropped (no state mutation, no `prev` update). For stricter behavior, gate with `where=~bv.col("lat").isnull() & ~bv.col("lon").isnull()`.
- **Non-numeric `lat` / `lon` (`Value::Str`, `Value::Bool`, `Value::Null`):** event is silently dropped (`read_lat_lon` returns `None`).
- **Two consecutive identical points:** segment distance is `0.0`; the running total is unchanged. `prev` is overwritten.
- **Very large path (e.g. years of GPS pings):** the `f64` accumulator can hold ~9 quadrillion km before precision loss matters — far beyond any real-world workload.
- **Polar latitudes (`|lat| > ~85°`):** haversine stays correct (no equirectangular approximation in this op); only `bv.geo_spread` is affected by the cos-correction.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. v0's geo ops are lifetime-only. Compose with `@bv.event(cold_after="...")` for time-bounded accumulation, or scope by entity-key (one entity per trip / day / shift).
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); segments are computed in arrival order. Backfill / replay produces the same total as long as the event ORDER matches.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `O(1)`; no `BoundedBy*` register-time check needed.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 2 — haversine floor)
- [bv.geo_velocity](./geo_velocity.md) — max-implied-km/h sibling (uses the same haversine but divides by Δt)
- [bv.geo_spread](./geo_spread.md) — RMS-dispersion sibling (how spread out are this entity's points around their mean centroid)
- [bv.distance_from_home](./distance_from_home.md) — current-event distance from a centroid of recent locations
- [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md) — cold-entity eviction (`@bv.event(cold_after=...)`) for windowing the lifetime
- [SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md) — `AggOp::GeoDistance` boxing context
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
