# bv.geo_spread

> RMS dispersion (km) of an entity's matching events around their running mean centre. Reads `lat` and `lon` field-name kwargs at register time; tracks Welford online second moments + an equirectangular cos-corrected projection at query time.

## Signature

```python
bv.geo_spread(
    *,
    lat: str, # REQUIRED — name of the latitude field on the event
    lon: str, # REQUIRED — name of the longitude field on the event
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.geo_spread` returns the **RMS dispersion (in km) of an entity's
matching events around their running mean centre**. State maintains
four running scalars per entity — `n`, `mean_lat`, `mean_lon`, plus
two Welford second-moment accumulators `m2_lat` and `m2_lon` — updated
online per Welford 1962. The query computes
`sqrt(var_lat_km² + var_lon_km²)` after projecting the per-axis
variance from degrees² to km² via an **equirectangular approximation
with a cos-correction** at the running-mean latitude (`KM_PER_DEG_LAT
= 111.32 km`; `km_per_deg_lon = 111.32 × cos(mean_lat)`). Use it for
"**how spread out are this user's transactions geographically?**" or
"**is this account suddenly transacting all over the country?**" —
features that detect geographic dispersion as a single scalar.

`lat` and `lon` are **required keyword arguments** that name two
fields on the event (e.g. `lat="latitude"`, `lon="longitude"`) — they
are NOT literal coordinates. The 4-scalar Welford state is `O(1)` per
entity and is updated in **constant time** regardless of how many
events the entity has seen — there is no buffer to walk on each
update. This was a deliberate v0 fix: the prior
implementation walked an O(n)-per-push samples vector (5,000–25,000
ns/event traced); the Welford rewrite is pure scalar math (~18 ns
floor / ~40 ns measured per [cost-class.md](../cost-class.md)).

`bv.geo_spread` belongs to the **bounded-buffer + geo** family. Per-event
update is **Tier 3** (~18 ns floor / ~40 ns measured — borderline Tier 2/3
per the audit, kept in Tier 3 per the official cost-class
classification). State is behind a `Box` for the `AggOp::GeoSpread`
variant per v0 boxing (the variant fits the 80-byte `AggOp`
enum cap; see `crates/beava-core/src/agg_op.rs` line 488 and
[SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md)).
There is no `window=` kwarg in v0 — `bv.geo_spread` is **lifetime-only**.
Compose with `@bv.event(cold_after="...")` for time-bounded dispersion
per [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `lat` | `str` | **Yes** | — | Name of the latitude field on the event (NOT a literal coordinate). Field value must be `f64` or `i64` decimal degrees in `[-90, 90]`. Resolved to a column index at register time. |
| `lon` | `str` | **Yes** | — | Name of the longitude field on the event. Field value must be `f64` or `i64` decimal degrees in `[-180, 180]`. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events update the Welford accumulators. |

## Returns

A scalar `float` — the RMS dispersion in kilometres, computed as
`sqrt(var_lat_km² + var_lon_km²)` with a cos-correction at the running
mean latitude. **Cold-start (fewer than 2 matching events): returns
`null`** — variance is undefined for `n < 2` per the Welford contract.
After 2+ matching events the result is a non-negative `f64`.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 3** (~18 ns floor / ~40 ns measured — pure scalar Welford updates: 2 means + 2 second-moment accumulators) — see [cost-class.md](../cost-class.md#tier-3-algorithmic-floor-100-300-nscall--9-ops). Borderline Tier 2/3; kept in Tier 3 per audit classification. |
| Memory per entity | **`O(1)`** — `(u64, f64, f64, f64, f64)` for the 4 Welford accumulators + counter. Boxed inside `AggOp` per v0 (`crates/beava-core/src/agg_op.rs` line 488). |
| Lifetime mode | **Required** — `bv.geo_spread` has no `window=` kwarg in v0; lifetime is the only mode. |

## Examples

### Example 1: How geographically spread are this user's transactions?

```python
import beava as bv

@bv.event
class Txn:
    user_id: str
    latitude: float
    longitude: float
    amount: float

@bv.table(key="user_id")
def UserGeoSpread(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(spread_km=bv.geo_spread(lat="latitude", lon="longitude"))
    )

# After 4 transactions at NYC, Boston, DC, Philadelphia
result = app.get("UserGeoSpread", "alice")
# result == {"spread_km": 215.7} # ~200 km RMS dispersion across the NE corridor
```

### Example 2: Spread for high-value transactions only

```python
@bv.table(key="user_id")
def UserHighValueSpread(txns) -> bv.Table:
    return (
        txns.group_by("user_id")
            .agg(high_value_spread=bv.geo_spread(
                lat="latitude",
                lon="longitude",
                where=bv.col("amount") > 500.0,
            ))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserGeoSpread",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "spread_km": {
      "op": "geo_spread",
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

- **Cold-start / zero or one matching event:** result is `null`. Variance is undefined for `n < 2` per the Welford contract; no value to report.
- **All matching events at the same `(lat, lon)`:** result is `0.0` once `n ≥ 2` — zero variance produces zero dispersion.
- **`lat` or `lon` missing on the event:** event is silently dropped (no state mutation). For stricter behavior, gate with `where=~bv.col("lat").isnull() & ~bv.col("lon").isnull()`.
- **Non-numeric `lat` / `lon` (`Value::Str`, `Value::Bool`, `Value::Null`):** event is silently dropped (`read_lat_lon` returns `None`).
- **Polar latitudes (`|lat| > ~85°`):** the equirectangular cos-correction degenerates as `cos(lat) → 0`. The variance computation is still numerically stable; the km projection becomes inaccurate. Sub-arctic transactions are rare in fraud workloads; the approximation is documented and accepted for v0. If you need polar accuracy, derive a custom feature using `geo_distance` or `count_distinct(quadkey(...))` instead.
- **Antimeridian crossings (e.g. events at `lon=179.9` and `lon=-179.9`):** the equirectangular projection treats these as ~360° apart in longitude, inflating the dispersion. Workaround: shift longitudes into a continuous range upstream (e.g. add 360 to negative-lon events when the entity straddles the antimeridian).
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. v0's geo ops are lifetime-only. For "spread over the last 30 days", compose with `@bv.event(cold_after="30d")`.
- **Out-of-order event-time:** **does not matter.** beava is processing-time-only per [`project_redis_shaped_no_event_time_ever`](../../../.planning/PROJECT.md). Welford accumulators commute under reordering: same set of points → same final variance regardless of arrival sequence.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `O(1)`; no `BoundedBy*` register-time check needed.

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 3 — borderline 2/3, audit-classified as 3)
- [bv.geo_velocity](./geo_velocity.md) — max-implied-km/h sibling (consecutive-event speed, not dispersion)
- [bv.geo_distance](./geo_distance.md) — total-path-length sibling (cumulative km along the trail)
- [bv.distance_from_home](./distance_from_home.md) — current-event distance from a centroid of recent locations
- [bv.variance](../core/var.md) — 1-D Welford variance companion (this op is the 2-D geographic version)
- [V0-MEM-GOV-01](../../../.planning/REQUIREMENTS.md) — cold-entity eviction (`@bv.event(cold_after=...)`) for windowing the lifetime
- [SUMMARY](../../../.planning/phases/12.9-aggop-memory-boxing/12.9-SUMMARY.md) — `AggOp::GeoSpread` boxing context
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
