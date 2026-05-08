# bv.first_n

> First N observed values of a field, in arrival order. `n` is a required register-time kwarg per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md).

## Signature

```python
bv.first_n(
    field: str,
    *,
    n: int, # REQUIRED — register-time kwarg
    where: bv.Col | None = None,
) -> AggDescriptor
```

## Description

`bv.first_n` returns the first `n` non-null values of `field` that the
entity has observed, preserved in arrival order. Once the buffer is full
the operator becomes a no-op for subsequent events — it's a "capture
forever" snapshot of the earliest matching events. Use it for fraud-shape
features like "the first 5 IPs we ever saw on this account" or "the first
3 device fingerprints used during onboarding".

`n` is a **required keyword argument** per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md):
the lifetime-aggregation memory contract requires that every unbounded-by-default
operator declare a finite per-entity ceiling at register time. `bv.first_n`'s
ceiling is exactly `n × sizeof(field)` bytes. The register-time JSON-prelude
shim (`pre_check_unbounded_op_in_lifetime_mode`) rejects any `first_n` payload
without `n` (or with `n=0` / negative `n`) with the structured error code
`unbounded_op_in_lifetime_mode`. There is no fallback — picking `n` is a
deliberate capacity-planning step.

`bv.first_n` belongs to the **point/ordinal** family. Per-event update is a
length check plus `Vec::push` until `len >= n`, then early-exit no-op.
Memory per entity is `O(n × sizeof(field))` bounded by the register-time
`n`. Each accepted event triggers one `Value::clone()` until the buffer
fills. There is no `window=` kwarg — `bv.first_n` is **lifetime-only**.
For "the most recent N values" use [`bv.last_n`](./last_n.md); for the
oldest *timestamp* see [`bv.first_seen`](../recency/first_seen.md).

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `field` | `str` | Yes | — | Name of the field whose first `n` values to capture. Any scalar type. |
| `n` | `int` | **Yes** | — | Number of values to capture. Must be `≥ 1` per [V0-MEM-GOV-02 BoundedByRequiredKwarg("n")](../../../.planning/REQUIREMENTS.md). Bounds the per-entity memory ceiling at register time. |
| `where` | `bv.Col` | No | `None` | Boolean expression on event fields; only matching events are candidates. |

## Returns

A JSON-array string holding up to `n` values in arrival order. When the
entity has seen zero matching events, the result is the empty list `"[]"`.
Wire format is a `Value::Str(serde_json::to_string(...))` because the v0
`Value` enum has no `List` variant — Python SDK readers parse it back into
a Python list transparently.

## Complexity

| Resource | Bound |
|----------|-------|
| CPU per event | **Tier 1** (~8 ns floor / ~30 ns measured) — see [cost-class.md](../cost-class.md#tier-1-fast-40-nscall--38-ops) |
| Memory per entity | **`BoundedByRequiredKwarg("n")`** — `n × sizeof(field)` bytes per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) |
| Lifetime mode | **Required** — `bv.first_n` has no `window=` kwarg; lifetime is the only mode |

## Examples

### Example 1: First 5 IPs ever seen for a user

```python
import beava as bv

@bv.event
class Login:
    user_id: str
    ip: str

@bv.table(key="user_id")
def UserFirstIPs(logins) -> bv.Table:
    return (
        logins.group_by("user_id")
              .agg(first_5_ips=bv.first_n("ip", n=5))
    )

# Push events
for ip in ["1.1.1.1", "2.2.2.2", "3.3.3.3", "4.4.4.4", "5.5.5.5", "6.6.6.6"]:
    app.push("Login", {"user_id": "alice", "ip": ip})

# Query
result = app.get("UserFirstIPs", "alice")
# result == {"first_5_ips": ["1.1.1.1", "2.2.2.2", "3.3.3.3", "4.4.4.4", "5.5.5.5"]}
# 6.6.6.6 was a no-op — buffer was already full
```

### Example 2: First 3 device fingerprints used while account was new

```python
@bv.table(key="user_id")
def UserOnboardingDevices(events) -> bv.Table:
    return (
        events.group_by("user_id")
              .agg(onboarding_devices=bv.first_n("device_id",
                                                   n=3,
                                                   where=bv.col("days_since_signup") < 7))
    )
```

## Wire

JSON wire form in a register payload:

```json
{
  "kind": "derivation",
  "name": "UserFirstIPs",
  "output_kind": "table",
  "key": ["user_id"],
  "agg": {
    "first_5_ips": {
      "op": "first_n",
      "params": {
        "field": "ip",
        "n": 5
      }
    }
  }
}
```

See [examples/wire/register-fraud-team.request.json](../../../examples/wire/register-fraud-team.request.json) for a full payload example.

## Edge cases

- **`n` missing at register time:** rejected with structured error code `unbounded_op_in_lifetime_mode` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md). The register-time JSON-prelude shim (`pre_check_unbounded_op_in_lifetime_mode` in `crates/beava-core/src/register_validate.rs`) catches this before any state is allocated.
- **`n=0` or negative `n`:** rejected by the SDK helper's pre-validation; the wire-level shim catches it as a fallback.
- **Fewer than `n` events seen:** returns the partial list (e.g., `["a", "b"]` after 2 events when `n=5`).
- **Empty stream / cold-start:** returns the empty list `"[]"`.
- **Null source field:** events whose `field` is `null` are skipped and do **not** consume buffer slots.
- **`where=` filter excludes everything:** returns `"[]"`; the buffer fills as matching events arrive.
- **`window=` kwarg attempted:** raises `TypeError` at SDK-helper-call time. There is no windowed `first_n` in v0.
- **Lifetime mode:** **the only mode.** Per-entity ceiling is `n × sizeof(field)` per [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) BoundedByRequiredKwarg("n").

## See also

- [cost-class.md](../cost-class.md) — performance tier (Tier 1)
- [bv.last_n](./last_n.md) — symmetric companion: most recent N values (also `BoundedByRequiredKwarg("n")`)
- [bv.first](./first.md) — degenerate `n=1` case (lighter — no `Vec` allocation)
- [bv.most_recent_n](../buffer-geo/most_recent_n.md) — N most recent values (deque shape, also `BoundedByRequiredKwarg("n")`)
- [V0-MEM-GOV-02](../../../.planning/REQUIREMENTS.md) — BoundedByRequiredKwarg memory governance contract
- [pipeline-dsl/compilation-rules.md](../../pipeline-dsl/compilation-rules.md) — chain compilation rules
