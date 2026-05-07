# Schema Evolution

> **Status:** Authoritative for v0. Documents the register-time semantics for
> additive descriptor changes (the default), destructive changes (`force=true`),
> and dry-run validation (`dry_run=true`). The contract is shared across all 3
> SDKs (Python / TypeScript / Go) per
> [shared.md § Schema evolution](sdk-api/shared.md#schema-evolution).
> **Last reviewed:** 2026-05-03 (Phase 13.0).

## Overview

beava registration is **additive-only by default** — re-running
`app.register(*descriptors)` with new event sources or new aggregation features
succeeds and bumps `registry_version`, leaving any pre-existing state intact.
Destructive changes (renaming a field, changing a field's type, removing an
aggregation feature) are **rejected with `registration_conflict` (HTTP 409)**
unless the caller explicitly opts in with `force=true`. The `dry_run=true` flag
runs the validator and computes the diff envelope without applying anything,
which migration tooling uses to preview register changes.

This shape — additive-default, destructive-by-flag, dry-run-by-flag — matches
the well-understood "schema migration safety net" pattern from Postgres / Liquibase
/ Alembic / dbt, adapted to a real-time feature server. The default refuses
silent breakages; the explicit-force escape hatch supports legitimate migrations
without requiring a rebuild.

The register payload + response shape lives in
[wire-spec OP_REGISTER](wire-spec.md#op_register-0x0001); this document focuses
on the semantics — **what** is additive, **what** is destructive, and **what**
the diff envelope contains.

## What's additive (default behavior — no flag needed)

These changes succeed with HTTP 200 and bump `registry_version`:

| Change | Result |
|--------|--------|
| Add a new `@bv.event` source descriptor | New event source registered; `added: ["NewEvent"]`. |
| Add a new `@bv.table` aggregation-output node | New table registered; `added: ["NewTable"]`. |
| Add a new derivation node (filter / select / with_columns / ... chain) | New derivation registered; `added: ["NewDerivation"]`. |
| Add a new aggregation feature inside an existing `agg` block | Existing table extends; `changed: [{name: "ExistingTable", added_features: [...]}]`. |
| Add a new **optional** field to an existing event schema | Existing event schema extends; `changed: [{name: "ExistingEvent", added_fields: [...]}]`. |
| Increase `keep_events_for` retention | Retention extends; in-flight events keep their old TTL until expiry, new events get the new TTL. |
| Add `cold_after=` to a previously-`cold_after=None` event source | Cold-entity TTL begins applying; existing entities continue under the prior unbounded retention until next event. |
| Re-register the **identical** payload (idempotent re-run) | No change; `registry_version` is **not** bumped; response is `{added: [], removed: [], changed: []}`. |

The "identical payload" idempotent case is important — `app.register(...)` is
**safe to call repeatedly** in deployment scripts. The server hashes the
descriptor payload and compares against the current registry; a no-op register
is a no-op (and the response says so).

## What's destructive (requires `force=true`)

These changes are **rejected with HTTP 409 `registration_conflict`** unless
`force=true` is passed:

| Change | Default behavior | With `force=true` |
|--------|------------------|---------------------|
| Remove an `@bv.event` source | 409 with `{removed: ["GoneEvent"]}` | 200; the event source is removed; in-memory state for that source is cleared; downstream derivations that reference it ALSO get removed (cascading). |
| Remove an `@bv.table` aggregation-output node | 409 with `{removed: ["GoneTable"]}` | 200; the table is removed; per-entity state for that table is cleared. |
| Remove an aggregation feature inside an existing `agg` block | 409 with `{changed: [{name: "T", removed_features: [...]}]}` | 200; the feature is removed; per-entity state for that feature is cleared. |
| Change a field's type (e.g., `f64` → `i64`) on an existing event | 409 with `{changed: [{name: "T", changed_fields: [{name: "amount", from: "f64", to: "i64"}]}]}` | 200; the field's type changes; per-entity aggregation state that depends on the field is **zeroed** (the new type is incompatible with the prior accumulated state). |
| Rename a field on an existing event | 409 with `{changed: [{name: "T", renamed_fields: [{from: "amt", to: "amount"}]}]}` | 200; the field is renamed; aggregation state keyed off the old name is zeroed. The recommended alternative is `events.rename(amt="amount")` in a derivation, which is fully additive (creates a new derivation with renamed schema, leaves the source untouched). |
| Add a new **required** field to an existing event schema (no default) | 409 with `{changed: [{name: "T", added_fields: [{name: "f", required: true, default: null}]}]}` | 200; the field is added as required; existing in-flight events without the field FAIL on push with `missing_field`. The recommended pattern is to add the field as **optional** OR to provide a `default=` value (which makes the change additive). |
| Reduce `keep_events_for` retention | 409 with `{changed: [{name: "T", keep_events_for: {from: "30d", to: "7d"}}]}` | 200; retention shrinks; events older than the new TTL are dropped on the next eviction sweep. |
| Change `dedupe_key` field on an existing event | 409 with `{changed: [{name: "T", dedupe_key: {from: "txn_id", to: "request_id"}}]}` | 200; deduplication state is reset. |

Destructive changes that ALSO require `force=true` even with the new field
being optional/defaulted:
- Removing an event source that has downstream derivations / tables (cascade).
- Renaming an event source (treated as remove + add).

`force=true` is **not transitive** — `force=true` on one register call grants
the destructive change for that call's payload only. The next register call
without `force=true` is back to additive-default discipline.

## `dry_run=True` flag

```python
diff = app.register(Txn, UserTxnFeatures, dry_run=True)
print(diff)
# {"added": ["UserTxnFeatures"], "removed": [], "changed": [...]}
```

`dry_run=True` runs the same validator + diff computation as a real register call,
but **does not mutate** the registry or in-memory state. The response carries
the `{added, removed, changed}` envelope identical to what a real register would
produce. `registry_version` is **not** bumped.

Use cases:

1. **Migration tooling preview** — run `dry_run=True` in CI to compute the diff
   between a feature-branch's pipeline and production. Surface the diff in
   pull-request comments.
2. **Destructive-change confirmation** — if `dry_run=True` returns a diff with
   non-empty `removed` or `changed` (with type changes / removals), a script can
   prompt the operator before re-running with `force=true`.
3. **Pre-deploy validation** — in CI, `dry_run=True` against a staging server
   asserts the descriptor payload validates structurally (cycle detection,
   schema propagation, lifetime-bound checks) without touching state.

The flag composes with `force=true` — `force=true, dry_run=True` returns the
diff for the destructive change WITHOUT applying it. Combined `dry_run +
force` is the canonical "what would happen if I forced this?" preview.

## Diff envelope shape

Every register response (success OR `registration_conflict` 409) carries the
diff envelope:

```json
{
  "status": "ok" | "conflict",
  "registry_version": 7,
  "added": ["NewEvent", "NewTable"],
  "removed": ["GoneEvent"],
  "changed": [
    {
      "name": "ExistingTable",
      "added_features": ["new_feature_1h"],
      "removed_features": ["gone_feature_1h"],
      "changed_features": [
        {
          "name": "feature_x",
          "from": {"op": "sum", "params": {"window": "1h"}},
          "to": {"op": "sum", "params": {"window": "5m"}}
        }
      ]
    },
    {
      "name": "ExistingEvent",
      "added_fields": [{"name": "new_field", "type": "str", "required": false}],
      "removed_fields": ["gone_field"],
      "changed_fields": [
        {"name": "amount", "from": "f64", "to": "i64"}
      ],
      "renamed_fields": [{"from": "amt", "to": "amount"}]
    }
  ]
}
```

On success (`status: "ok"`), `registry_version` is bumped (additive paths) or
preserved (idempotent re-register / `dry_run=True`).

On 409 conflict (`status: "conflict"`), `registry_version` is unchanged; the
response details which descriptors would have changed destructively. The
operator's recovery path is either:
- Revert the destructive change in the descriptor source, OR
- Re-issue with `force=true` to apply the destructive change.

## Validation diff matrix

Combined view of every register-time semantic. Each row distinguishes ADDITIVE
(no flag needed; `200 OK` + version bump) vs DESTRUCTIVE (409 default; 200 OK
with `force=true`):

| Change | Default behavior | With `force=true` | Recovery from prior state |
|--------|------------------|---------------------|---------------------------|
| Add event source | 200 + version bump | (same — already additive) | Re-deploy without the new source. |
| Remove event source | 409 `registration_conflict` | 200 + version bump; state cleared | Re-add the source; events pushed in the gap are LOST. |
| Add field (optional, with default) | 200 + version bump | (same — already additive) | Re-deploy without the field. |
| Add field (required, with default) | 200 + version bump | (same — already additive) | Re-deploy without the field. |
| Add field (required, no default) | 409 `registration_conflict` | 200 + version bump; in-flight pushes fail with `missing_field` until clients update | Add a default OR mark optional. |
| Change field type (compatible widening: `i64 → f64`) | 200 + version bump | (same — already additive widening) | Cast at the source. |
| Change field type (narrowing or incompatible: `f64 → i64`, `str → i64`) | 409 `registration_conflict` | 200 + version bump; per-entity state for the affected field is **zeroed** | Add a derivation with `cast(...)` instead of changing the source schema. |
| Rename field | 409 `registration_conflict` | 200 + version bump; aggregation state keyed off old name zeroed | Use `events.rename(...)` in a derivation (additive). |
| Add derivation node | 200 + version bump | (same — already additive) | Re-deploy without the derivation. |
| Remove derivation node | 409 `registration_conflict` | 200 + version bump; state cleared | Re-add the derivation; backfill from event source. |
| Change derivation chain (different ops) | 409 `registration_conflict` | 200 + version bump; per-entity state for downstream tables zeroed | Update the descriptor source; coordinate with consumers. |
| Add `@bv.table` | 200 + version bump | (same) | Re-deploy without the table. |
| Remove `@bv.table` | 409 `registration_conflict` | 200 + version bump; per-entity state cleared | Re-add; events pushed in the gap won't have rolled into the table. |
| Add aggregation feature inside existing table | 200 + version bump | (same) | Re-deploy without the feature. |
| Remove aggregation feature inside existing table | 409 `registration_conflict` | 200 + version bump; per-entity state for that feature cleared | Re-add the feature; new state starts from zero, no backfill. |
| Change aggregation feature parameters (e.g., `window` 1h → 5m) | 409 `registration_conflict` | 200 + version bump; per-entity state for that feature zeroed | The window-change invalidates accumulated state — there is no in-place re-bucketing in v0. |
| Idempotent re-register (identical payload) | 200, no version bump, empty diff | (same) | (no-op; safe to repeat in deployment scripts) |

## Worked examples

The following fixtures under [`examples/wire/`](../examples/wire/) demonstrate
each path:

| Path | Fixture |
|------|---------|
| Additive register (fraud-team-style) | [`register-fraud-team.request.json`](../examples/wire/register-fraud-team.request.json) → `200 OK` |
| Destructive register without `force` | [`register-conflict.error.json`](../examples/wire/register-conflict.error.json) → `409 Conflict` |
| Destructive register with `force=true` | [`register-force.request.json`](../examples/wire/register-force.request.json) → `200 OK` (zeroes affected state) |
| Dry-run validation | [`register-dry-run.request.json`](../examples/wire/register-dry-run.request.json) → `200 OK` (diff returned, no mutation) |

## Server-side enforcement

Validation runs through a layered pipeline in
`crates/beava-core/src/register_validate.rs`:

1. **JSON-prelude shims** (run first, BEFORE strict serde parse):
   - `pre_check_removed_ops` — rejects `op="join"` / `op="union"` with
     `feature_removed_no_joins_v0` / `feature_removed_no_unions_v0`.
   - `pre_check_legacy_event_time_keys` — rejects `event_time_field` /
     `tolerate_delay_ms` keys with `unknown_field_event_time_v0` /
     `unknown_field_tolerate_delay_v0`.
   - `pre_check_unsupported_node_kind` — rejects payloads with
     `kind="upsert"` / `kind="delete"` / `kind="retract"` etc. with
     `unsupported_node_kind`. Per ADR-001, `kind="table"` is now PERMITTED
     for aggregation-output (the v0 amendment lands in Phase 13.4).
   - `pre_check_unbounded_op_in_lifetime_mode` — rejects lifetime-mode ops
     without a finite memory bound with `unbounded_op_in_lifetime_mode`
     (per V0-MEM-GOV-02 from Phase 12.8).

2. **Strict serde deserialise** — `RegisterPayload` enforces structural
   shape; `serde` errors map to `schema_invalid` (HTTP 400).

3. **Topological sort + cycle detection** — Kahn's algorithm; cycles raise
   `registration_cycle` (HTTP 400).

4. **Per-descriptor structural validation** — name format, schema structure,
   key validity, aggregation parameter validity (e.g., `quantile.q ∈ (0,1)`).

5. **Schema propagation through op chains** — every `with_columns` /
   `cast` / `filter` expression is type-checked against its upstream schema;
   mismatches raise `schema_mismatch` / `unknown_field_reference` /
   `invalid_expression` / `invalid_cast_target`.

6. **Diff computation against current registry** — additive vs destructive
   classification per the matrix above. Destructive paths without `force=true`
   raise `registration_conflict` (HTTP 409) carrying the full diff envelope.

The JSON-prelude shim pattern (steps 1) was introduced in Phase 12.6 Plan 04
and extended through Phase 12.7 + 12.8 to keep structured error codes stable
across schema/code changes — when the corresponding Rust struct fields or enum
variants are removed, the shim catches the legacy payload BEFORE strict serde
returns a generic "unknown variant" / "unknown field" message.

## Cross-references

- [Wire spec](wire-spec.md) — canonical JSON contract for `OP_REGISTER` request
  and response shapes.
- [Error codes](error-codes.md) — alphabetical list of every structured code
  this document references (`registration_conflict`, `schema_mismatch`,
  `unsupported_node_kind`, `unbounded_op_in_lifetime_mode`, etc.) with HTTP
  status mapping.
- [SDK API — shared](sdk-api/shared.md#schema-evolution) — cross-language
  surface for the `force` and `dry_run` flags (Python kwargs / TS options /
  Go functional options).
- [SDK API — Python](sdk-api/python.md) — Python-specific `app.register(*,
  force=False, dry_run=False)` signature.
- [SDK API — TypeScript](sdk-api/typescript.md) — TS `app.register(descs,
  { force, dryRun })` signature.
- [SDK API — Go](sdk-api/go.md) — Go `app.Register(ctx, descs,
  beava.WithForce(), beava.WithDryRun())` signature.
- [Pipeline DSL Compilation Rules — Ambiguity Matrix](pipeline-dsl/compilation-rules.md#ambiguity-matrix)
  — the FORBIDDEN rows enumerate which structural changes the validator
  rejects unconditionally (`force=true` does NOT bypass them).
- [ADR-001](../.planning/decisions/ADR-001-bv-table-partial-overturn.md) —
  `@bv.table` aggregation-output revival; pre-13.4 the JSON-prelude shim
  rejects `kind="table"`, post-13.4 it permits the aggregation-output form
  and continues rejecting other table surfaces.
