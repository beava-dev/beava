# bv.geo_velocity

> Maximum implied great-circle speed (km/h) between consecutive matching events for an entity. Reads `lat` and `lon` field-name kwargs at register time; computes haversine distance / Δt on the apply path.

## Signature

```python
bv.geo_velocity(
    *,
    lat: str, # REQUIRED — name of the latitude field on the event
    lon: str, # REQUIRED — name of the longitude field on the event
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.geo_velocity` returns the **maximum implied great-circle speed in
km/h** observed between any two consecutive matching events for an entity.
On every accepted event the operator reads `(lat, lon)` from the
register-time-named fields, computes the haversine distance to the
previously-stored point, divides by the elapsed time (server-side
processing-time `now_ms()`, milliseconds), converts to km/h, and updates
a running maximum. Use it for **impossible-travel detection** in fraud:
"this card was used in NYC at 14:02:00 and in Singapore at 14:02:30 — the
implied speed is 1.6 million km/h, almost certainly not a real human".

`lat` and `lon` are **required keyword arguments** that name two fields
on the event (e.g. `lat="latitude"`, `lon="longitude"`) — they are NOT
literal coordinates. The latitude/longitude values must be `f64` or
`i64` (integer-degrees are silently coerced); other types or missing
fields cause the event to be skipped without state mutation. Distance
math goes through the `haversine` crate (great-circle on a spherical
Earth, mean radius **6371 km** per CONTEXT D-02 / `agg_geo.rs::haversine_km`).
This is accurate to within ~0.5% on Earth's surface, well below the
signal-to-noise ratio of fraud-detection thresholds.

`bv.geo_velocity` belongs to the **bounded-buffer + geo** family. State
is `O(1)` per entity — three `f64` slots for the previous `(lat, lon, t)`
plus one `f64` for the running max km/h, behind a `Box` for the
`AggOp::GeoVelocity` variant per v0 boxing (the variant fits the
80-byte `AggOp` enum cap; see `crates/beava-core/src/agg_op.rs` line 486
and [SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md)).
Per-event update is **Tier 2** (~20 ns floor / ~45 ns measured per
[cost-class.md](../cost-class.md#tier-2-moderate-30-100-nscall--6-ops)) —
two field reads + one haversine (`sin`/`cos`/`sqrt` identities) + one
divide + one compare. There is no `window=` kwarg in v0 — `bv.geo_velocity`
is **lifetime-only**. For "max speed in the last 24 h", compose with
`@bv.event(cold_after="24h")` per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md);
the per-entity state is dropped on TTL expiry and rebuilds fresh from the
next post-eviction matching event.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `lat` | `str` | **Yes** | — | Name of the latitude field on the event (NOT a literal coordinate). Field value must be `f64` or `i64` decimal degrees in `[-90, 90]`. Resolved to a column index at register time. |
| `lon` | `str` | **Yes** | — | Name of the longitude field on the event. Field value must be `f64` or `i64` decimal degrees in `[-180, 180]`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the prev-point and max km/h. |

## Returns

A scalar `float` — the maximum km/h observed between any two consecutive
matching events for the entity. **Cold-start (fewer than 2 matching
events): returns `null`** — no prior point to compare against, so no
implied speed has been measured yet. After 2+ matching events the result
is a non-negative `f64`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 2** (~20 ns floor / ~45 ns measured — two field reads + haversine `sin`/`cos`/`sqrt` + divide + compare) — see [cost-class.md](../cost-class.md#tier-2-moderate-30-100-nscall--6-ops). The haversine is the irreducible Tier 2 cost. |
| Memory per entity | **`O(1)`** — `Option<(f64, f64, i64)>` for the previous point + `f64` for max km/h. Boxed inside `AggOp` per v0 (`crates/beava-core/src/agg_op.rs` line 486). |
| Lifetime mode | **Required** — `bv.geo_velocity` has no `window=` kwarg in v0; lifetime is the only mode. |

## Examples

### Example 1: Impossible-travel detection per card

```python
import beava as bv

@bv.event
class CardSwipe:
    card_id: str
    latitude: float
    longitude: float

@bv.table(key="card_id")
def CardMaxImpliedKmh(swipes) -> bv.Table:
    return (
        swipes.group_by("card_id")
              .agg(max_kmh=bv.geo_velocity(lat="latitude", lon="longitude"))
    )

# Push events
app.push("CardSwipe", {"card_id": "abc", "latitude": 40.7128, "longitude": -74.0060}) # NYC
# 30 seconds later
app.push("CardSwipe", {"card_id": "abc", "latitude": 1.3521, "longitude": 103.8198}) # Singapore

result = app.get("CardMaxImpliedKmh", "abc")
# result == {"max_kmh": 1_867_000.0} # ~1.86 million km/h — physically impossible
```

### Example 2: Per-user max km/h with a `where=` filter

```python
@bv.table(key="user_id")
def UserMaxKmhMobile(events) -> bv.Table:
    return (
        events.group_by("user_id")
              .agg(max_kmh=bv.geo_velocity(
                  lat="lat",
                  lon="lon",
                  where=bv.col("device_type") == "mobile",
              ))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "CardMaxImpliedKmh",
  "output_kind": "table",
  "key": ["card_id"],
  "agg": {
    "max_kmh": {
      "op": "geo_velocity",
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

- **Cold-start / fewer than 2 matching events:** result is `null` — no prior point to compute Δdistance / Δt against. The first matching event seeds `prev = (lat, lon, now_ms)`; the second event produces the first km/h value.
- **Δt = 0 (two events at the same processing-time millisecond):** the implied speed is undefined; the operator skips the divide and updates `prev` only. Subsequent events compare against the latest `(lat, lon, now_ms)`.
- **`lat` or `lon` missing on the event:** event is silently dropped (no state mutation). For stricter behavior, gate with `where=~bv.col("lat").isnull() & ~bv.col("lon").isnull()`.
- **Non-numeric `lat` / `lon` (`Value::Str`, `Value::Bool`, `Value::Null`):** event is silently dropped (`read_lat_lon` returns `None`).
- **Out-of-range coordinates** (`lat ∉ [-90, 90]` or `lon ∉ [-180, 180]`): the haversine math doesn't bounds-check; the result is whatever the spherical-trig identities produce. Validate upstream if your data source can emit garbage values.
- **Polar latitudes (`|lat| > ~85°`):** the equirectangular cos-correction degrades; the haversine itself stays correct. Sub-arctic transactions are rare in fraud workloads; the approximation is documented and accepted for v0.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. v0's geo ops are lifetime-only. For a "max speed in the last 24 h" view, compose with `@bv.event(cold_after="24h")`.
- **Out-of-order event-time:** **does not matter for ordering, matters for Δt.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md); `now_ms` is the server-side arrival timestamp. If an event is delayed in transit, the implied speed reflects arrival cadence, not the event's true time of occurrence.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `O(1)`; no `BoundedBy*` register-time check needed (the geo state is structurally bounded by the `Option<(f64, f64, i64)>` shape).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 2 — haversine floor)
- [bv.geo_distance](./geo_distance.md) — total-path-length sibling (cumulative km, not max km/h)
- [bv.geo_spread](./geo_spread.md) — RMS-dispersion sibling (how spread out are this entity's points around their mean centroid)
- [bv.distance_from_home](./distance_from_home.md) — current-event distance from a centroid of recent locations
- [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md) — cold-entity eviction (`@bv.event(cold_after=...)`) for windowing the lifetime
- [SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md) — `AggOp::GeoVelocity` boxing context
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
