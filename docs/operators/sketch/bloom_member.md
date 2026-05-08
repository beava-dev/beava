# bv.bloom_member

> Bloom-filter ever-seen membership test (lifetime-only).

## Signature

```python
bv.bloom_member(
    field: str,
    *,
    capacity: int = 1024,
    fpr: float = 0.01,
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.bloom_member` answers a single per-event question: **has this entity ever
seen this value before?** State is a Bloom filter sized to hold `capacity`
distinct values at false-positive rate `fpr` (default: 1024 entries at 1%
FPR ⇒ ~1.2 KB per entity, k=7 hash functions). Per-event update inserts the
value into the filter and reports the test result for that event.

Unlike windowed aggregations, `bv.bloom_member` is intentionally
**lifetime-only** — there is no `window=` kwarg. The "ever-seen" semantics
collapse if entries can expire, so the operator declares a `BoundedSketch`
ceiling at register time and never reclaims state.

Use `bv.bloom_member("device_id")` for "is this a new device for this user?"
or `bv.bloom_member("country", capacity=64)` for "is this country novel for
this account?". The result is the boolean test for the **current** event:
`true` iff the value MAY have been seen before (false-positive prone), `false`
iff the value is definitely new. False positives are bounded by `fpr`; false
negatives never occur (Bloom filters are conservative on the membership side).

`bv.bloom_member` belongs to the **sketch** family. Tier 2 cost (~35 ns
algorithmic floor — k hashes × k bit-set lookups). Field type can be `str`,
`i64`, or `f64`.

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the field to test ever-seen membership against. |
| `capacity` | `int` | No | `1024` | Number of distinct values the filter is sized for. Bytes ≈ `-capacity × ln(fpr) / (ln 2)^2 / 8`. |
| `fpr` | `float` | No | `0.01` | False-positive rate. Lower ⇒ larger filter; 0.01 (1%) is a typical default. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events are tested and inserted. |

## Returns

A single `bool`. When the entity has seen zero matching events the result is
`null` (Python `None`); on the first matching event the result is `false`
(definitely new) and the value is added to the filter; subsequent matching
events report `true` (may-have-been-seen) for repeats.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 2** (~35 ns algorithm floor / ~70 ns measured — k=7 hashes × k=7 bit-sets at default fpr) — see [cost-class.md](../cost-class.md#tier-2-moderate-30-100-nscall--6-ops) |
| Memory per entity | `BoundedSketch` — fixed size at register time: `~capacity × 9.6 bits` for fpr=0.01 (~1.2 KB at capacity=1024) |
| Lifetime mode (no `window=` kwarg) | **Required** — bloom is always lifetime; per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) the structural cap is declared at register time |

## Examples

### Example 1: Has this user ever logged in from this device?

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    device_id: str

@bv.table(key="user_id")
def UserDeviceCheck(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(seen_device_before=bv.bloom_member("device_id"))
    )

# Push events
app.push("Login", {"user_id": "alice", "device_id": "iphone-12"})
app.push("Login", {"user_id": "alice", "device_id": "iphone-12"})
app.push("Login", {"user_id": "alice", "device_id": "macbook-pro"})

# Query (returns bool result of the LAST matching event's check)
result = app.get("UserDeviceCheck", "alice")
# result == {"seen_device_before": false} # macbook-pro was new
```

### Example 2: Is this a novel destination country for this card?

```python
@bv.table(key="card_id")
def CardCountryCheck(txns) -> bv.Table:
    return (
        txns.group_by("card_id")
            .agg(seen_country_before=bv.bloom_member("dst_country",
                                                       capacity=64,
                                                       fpr=0.001))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserDeviceCheck",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "seen_device_before": {
      "op": "bloom_member",
      "params": {
        "field": "device_id",
        "capacity": 1024,
        "fpr": 0.01
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **Cold-start:** result is `null` until the first matching event arrives.
- **First matching event:** result is `false` (definitely new) and the value is added to the filter.
- **No `window=` kwarg:** by design — Bloom semantics require lifetime retention. Attempting to add `window=` raises a `TypeError` at SDK-helper-call time.
- **Capacity overshoot:** if the entity sees more than `capacity` distinct values, the actual false-positive rate exceeds the declared `fpr`. The filter still works (no false negatives), but rule writers should cap `capacity` generously for high-cardinality entities.
- **`fpr` near 0:** drives bit count up; fpr=0.001 ≈ ~14.4 bits/entry. Don't set fpr below `2^-32` — register-time validator rejects.
- **Non-hashable field type:** rejected at register time with `schema_mismatch`.
- **Lifetime mode:** **required** — the `BoundedSketch` cap is `~capacity × 9.6 bits` at fpr=0.01, declared at register time per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 2)
- [bv.n_unique](./n_unique.md) — cardinality companion (also lifetime-allowed)
- [bv.has_seen](../recency/has_seen.md) — boolean ever-matched on a `where=` predicate (no per-value tracking)
- [bv.first_seen](../recency/first_seen.md) — timestamp of first matching event
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
