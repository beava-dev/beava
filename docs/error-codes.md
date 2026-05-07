# Error Codes

> **Status:** Authoritative for v0. Documents every structured error code beava
> emits across the wire (HTTP and TCP transports), the Python exception
> hierarchy, the 9 frozen `ValidationError.kind` values, and the HTTP status
> mapping. This document is the canonical reference for SDK error handling.
> **Last reviewed:** 2026-05-03 (Phase 13.0).

## Overview

Every beava error response — whether emitted over HTTP (any `4xx` / `5xx`) or
over TCP (the `OP_ERROR_RESPONSE = 0xFFFF` opcode) — carries a JSON body
conforming to [`error.schema.json`](../examples/wire/schemas/error.schema.json):

```json
{
  "code": "<structured-code-string>",
  "path": "<JSON-path-or-DAG-path>",
  "message": "<human-readable-string>"
}
```

The **`code`** field is a stable structured identifier — renaming a code
(e.g., `schema_mismatch` → `field_type_mismatch`) is a breaking change that
requires an ADR. Across all 3 SDKs (Python / TypeScript / Go), error handling
should dispatch on `code`, **not** on the human-readable `message`.

The **`path`** field is an optional DAG / JSON path locating the offending
element. Examples: `"descriptors[1].schema.amount"` (during register
validation), `"fields.amount"` (during push), `"requests[2].table"` (during
batch_get). Optional — absent for transport-level errors.

The **`message`** field is human-readable. **Forward-looking framing per
Phase 12.7 D-02** applies — messages say "X is not supported in v0",
**not** "X has been removed" or "X was deprecated". The framing avoids
implying a previous-version reference for users who never saw older
revisions of the surface.

The error envelope is shared between transports. TCP wraps it in a frame
with `op = 0xFFFF` (`OP_ERROR_RESPONSE`) and `content_type = 0x01` (JSON);
HTTP returns it as the response body with the appropriate status code.

## Python exception hierarchy

The Python SDK exposes three exception types (re-exported from `beava`):

```python
class RegistrationError(Exception):
    """Raised when registration fails — locally (DAG/schema) or on the server (409)."""
    code: str                              # Structured code (one of the codes below)
    path: str                              # DAG / JSON path
    message: str                           # Human-readable
    errors: list[ValidationError]          # Full list when server returns multiple
```

```python
class BinaryNotFoundError(Exception):
    """Raised by embed mode when the beava binary cannot be found.

    Discovery order:
      1. BEAVA_BINARY env var
      2. 'beava' on PATH
      3. ./target/debug/beava (dev convenience)
      4. This exception with install-guidance message.
    """
```

```python
@dataclass(frozen=True)
class ValidationError:
    """Frozen dataclass for client-side and server-side validation errors."""
    kind: str                              # One of the 9 frozen kinds below
    path: str                              # JSON-pointer-style path
    message: str                           # Human-readable
```

`RegistrationError` is the canonical exception raised on register failures —
both local (DAG cycle / schema mismatch detected client-side) and remote
(server returned 4xx / 5xx). Push-time and get-time errors raise the same
class with the appropriate `code` set, but each surface (`push` /
`batch_get`) layers its own per-call exception class on top in v0.1+ for
language-idiomatic dispatching.

The `errors` attribute is populated when the server returns multiple
validation errors in a single response (the fail-soft batching pattern).
For single-error responses, `errors` is empty and the top-level `code` /
`path` / `message` carries the singleton.

For TypeScript and Go equivalents see
[shared.md § Error semantics](sdk-api/shared.md#error-semantics).

## ValidationError kinds (9 frozen values)

The 9 `kind` values are **frozen for v0** — adding a new kind requires an
ADR. Source: `python/beava/_errors.py::VALIDATION_ERROR_KINDS`.

| Kind | When |
|------|------|
| `cycle` | Descriptor list forms a cycle through `upstreams` (e.g., `A → B → A`). |
| `missing_upstream` | A `derivation` references an upstream not declared in this batch and not previously registered. |
| `schema_mismatch` | A push field has the wrong type and cannot be coerced; or `bv.sum` field arg is not a `string` per Q1 Path B; or expression type-inference rejects the operand types. |
| `bad_return_type` | A function-form `@bv.event` returns the wrong descriptor shape (e.g., the body returns a `GroupBy` instead of an `EventDerivation`). |
| `unknown_field_type` | Field type annotation is not in the supported vocabulary (`str` / `f64` / `i64` / `bool` / `bytes` / `datetime`). |
| `table_key_invalid` | Composite-key shape is malformed at register time (empty list, non-string element, references unknown field). |
| `registration_conflict` | Destructive change (field type change, field removal, derivation removal) without `force=true`. |
| `duplicate_name` | Two descriptors in the same register call have the same `_name`. |
| `unsupported_node_kind` | Body has `kind="upsert"` / `kind="delete"` / `kind="retract"` etc. — pre-12.7 surface that is permanently killed per `project_v0_events_only_scope`. Per ADR-001, `kind="table"` is now PERMITTED for aggregation-output (Phase 13.4). |

## Structured codes (alphabetical)

The canonical reference. Each entry lists the HTTP status the server returns,
when the code fires, the typical `path` format, the recovery action, and a
worked-example fixture link if one exists under `examples/wire/`.

### code = "aggregation_invalid_half_life"

**HTTP status:** 400
**When:** Decay op (`ewma`, `ewvar`, `ew_zscore`, `decayed_sum`,
`decayed_count`) `half_life=` kwarg missing, malformed, non-positive, or set
to `"forever"` (`forever` is REJECTED for decay ops).
**Path:** `descriptors[<i>].agg.<feature>.params.half_life`.
**Recovery:** Provide a positive finite duration string (e.g., `"5m"`).

### code = "aggregation_invalid_param"

**HTTP status:** 400
**When:** Aggregation parameter out of valid range — e.g., `quantile.q ∉ (0,1)`,
`top_k.k ∉ (0, 1024]`, `bloom_member.fpr ∉ (0,1)`, `outlier_count.sigma ≤ 0`.
**Path:** `descriptors[<i>].agg.<feature>.params.<param>`.
**Recovery:** Set the parameter to a value within the documented range.

### code = "aggregation_invalid_sub_window"

**HTTP status:** 400
**When:** `burst_count.sub_window` missing, malformed, or non-positive.
**Path:** `descriptors[<i>].agg.<feature>.params.sub_window`.
**Recovery:** Provide a positive duration string smaller than the outer
`window=`.

### code = "aggregation_invalid_window"

**HTTP status:** 400
**When:** `window=` string does not match `\d+(ms|s|m|h|d)` or `forever`. Or:
windowed-only op (e.g., `bloom_member`) was given a `window=` kwarg
(`window_not_supported`). Or: required-window op (`sum`, `avg`, `min`, `max`,
`variance`, `stddev`) had `window=` omitted.
**Path:** `descriptors[<i>].agg.<feature>.params.window`.
**Recovery:** Provide a valid duration string per the
[shared.md window grammar](sdk-api/shared.md#window-grammar). Examples: `"5m"`,
`"1h"`, `"100ms"`, `"7d"`, `"forever"`.

### code = "aggregation_on_table_not_supported"

**HTTP status:** 400
**When:** A `@bv.table` aggregation references another `@bv.table` as its
upstream (table-to-table aggregation). Per ADR-001, only events feed
aggregations in v0.
**Path:** `descriptors[<i>].upstreams[<j>]`.
**Recovery:** Aggregate from event sources only. Compose downstream features
client-side via separate `app.get(...)` calls.

### code = "bad_return_type"

**HTTP status:** 400
**When:** A function-form `@bv.event` returns the wrong descriptor shape — e.g.,
the body returns a `GroupBy` (intermediate) instead of an `EventDerivation`.
**Path:** `<descriptor_name>`.
**Recovery:** Ensure the function body returns a fully-resolved descriptor
(close any `group_by` chain with an `.agg(...)` call to produce a `Table`).

### code = "batch_too_large"

**HTTP status:** 400
**When:** `OP_BATCH_GET` request carries more than 10000 entries in `requests`.
**Path:** `requests` (no specific index).
**Recovery:** Split the batch into multiple smaller calls. The 10000 limit is
configurable server-side via `BEAVA_MAX_BATCH_GET` but defaults to 10000 per
P99 < 10ms latency budget.

### code = "bv_table_class_form_not_supported"

**HTTP status:** 400
**When:** `@bv.table` decorator is applied to a class (class-form decorator).
v0 ships **function-form only** per ADR-001. The class-form decorator is
captured for v0.1+.
**Path:** `<descriptor_name>`.
**Recovery:** Convert to function form — `@bv.table(key="user_id") def
UserFeatures(txn) -> bv.Table: return txn.group_by("user_id").agg(...)`.

### code = "cycle"

**HTTP status:** 400
**When:** Descriptor DAG contains a cycle through `upstreams` (e.g.,
`A → B → A`).
**Path:** `<cycle path: A -> B -> A>`.
**Recovery:** Break the cycle. Cycles indicate the DAG is malformed — typically
a missing rename or a derivation that accidentally references its own output.

### code = "dedupe_replay"

**HTTP status:** 200 *(not an error in the operational sense; documented for
completeness)*
**When:** A push with a registered `dedupe_key` matched a recent push within
the registered `dedupe_window` — server returns the prior `ack_lsn` with
`idempotent_replay: true`.
**Path:** `fields.<dedupe_key>`.
**Recovery:** This is normal idempotency behavior; clients can check
`idempotent_replay` in the response and treat the original push as authoritative.

### code = "duplicate_name"

**HTTP status:** 400
**When:** Two descriptors in the same register call have the same `name`.
**Path:** `descriptors[<i>].name`.
**Recovery:** Rename one of the descriptors. Names are the primary keys of the
registry — duplicates are impossible to disambiguate.

### code = "event_time_not_supported_in_v0"

**HTTP status:** 400
**When:** Register payload contains an `event_time_field` or `tolerate_delay_ms`
key, OR an `@bv.event` schema declares an `event_time` field. Per
`project_redis_shaped_no_event_time_ever` (locked 2026-04-30), beava is
processing-time only; the server stamps wall-clock arrival time on every push.
**Path:** `descriptors[<i>].event_time_field` or `descriptors[<i>].schema.event_time`.
**Recovery:** Remove the `event_time` field / kwarg. Windowed operators bucket
on server-side `now_ms()` automatically. The Python SDK rejects this at
decoration time (`TypeError`); register-time rejection is the server's defense
against hand-written JSON. Wire-level codes from Phase 12.6:
`unknown_field_event_time_v0` / `unknown_field_tolerate_delay_v0`.

### code = "feature_not_in_table"

**HTTP status:** 400
**When:** `OP_GET` / `OP_BATCH_GET` `features[i]` is not a feature of the named
table.
**Path:** `requests[<i>].features[<j>]` or `features[<i>]`.
**Recovery:** Check the feature name against the table's declared `agg` map
(use `GET /registry` for the live registry).

### code = "feature_removed_no_joins_v0"

**HTTP status:** 400
**When:** Register payload contains `op="join"`. Joins are permanently killed
per `project_redis_shaped_no_event_time_ever` (locked 2026-04-30).
**Path:** `nodes[<i>].ops[<j>]`.
**Recovery:** Compose client-side via push/get patterns and entity-key sharding.
Joins return alongside tables in v0.1+ if/when justified by demand.

### code = "feature_removed_no_unions_v0"

**HTTP status:** 400
**When:** Register payload contains `op="union"`. Unions are deferred with
joins per the same architectural lock.
**Path:** `nodes[<i>].ops[<j>]`.
**Recovery:** Multiplex client-side for v0; first-class union returns alongside
joins in a future minor release.

### code = "force_required"

**HTTP status:** 409 Conflict
**Wire opcode (TCP):** `OP_ERROR_RESPONSE = 0xFFFF` (JSON body shape below)
**When:** A `POST /register` payload contains a destructive change (rename,
type-change, op removal, agg removal, window-change, or key-cols change) and
`force=true` is not set in the body. Per D-01 (Phase 13.4 Plan 06,
USER-LOCKED), destructive registry changes require an explicit `force=true`
opt-in to apply; otherwise the server rejects with this code.
**Path:** *(no path; the offending changes are enumerated in `error.diff.destructive`)*
**Recovery:** Either (1) send `force=true` in the register body to apply
destructively + bump `registry_version` (existing per-entity state for the
affected aggregations becomes inconsistent — the wire contract is satisfied
but per-entity state-zeroing is a future refinement), OR (2) send
`dry_run=true` to preview the diff without mutating state, OR (3) amend the
payload to be additive-only (new descriptor, new agg in existing block, new
field on event source — these always succeed without `force`).

Response body shape:

```json
{
  "error": {
    "code": "force_required",
    "reason": "Destructive registry change requires force=true. See diff for details.",
    "diff": {
      "additive": [
        {"kind": "new_descriptor", "descriptor_kind": "event", "name": "NewEvent"},
        {"kind": "new_agg", "table": "UserSpend", "agg": "tx_count_24h", "source": "count"},
        {"kind": "new_field", "event": "Tx", "field": "merchant_id", "type": "str"}
      ],
      "destructive": [
        {"kind": "rename", "from": "tx_count_1h", "to": "tx_count_one_hour"},
        {"kind": "type_change", "field": "Tx.amount", "from": "f64", "to": "i64"},
        {"kind": "op_removal", "table": "UserSpend", "agg": "group_by[0]"},
        {"kind": "agg_removal", "table": "UserSpend", "agg": "tx_count_5m"},
        {"kind": "window_change", "agg": "UserSpend.tx_count_1h", "from": "1h", "to": "30m"},
        {"kind": "key_cols_change", "table": "UserSpend", "from": ["user_id"], "to": ["user_id", "merchant_id"]}
      ]
    }
  },
  "registry_version": 7
}
```

The `additive` and `destructive` lists are sorted deterministically by
`(kind, primary_field)` so two preview / classify calls with identical inputs
produce byte-identical JSON output (idempotent diffs — required for CI
diff-checks against staging registries).

The diff envelope is **categorized lists** (NOT JSON-Patch). Each entry's
`kind` discriminator names the destructive class per D-01.

`force_required` is distinct from the legacy `registration_conflict` (HTTP
409) emitted by the Phase 2 diff machinery — `registration_conflict`
predates D-01 and uses a different envelope shape. Both codes coexist in
the v0 surface; the dispatch order is force_required FIRST (Phase 13.4
Plan 06), legacy `registration_conflict` SECOND (additive-only path with a
diff that still detects schema drift from the prior registry).

**Source:** Phase 13.4 Plan 06 / D-01 (USER-LOCKED).

### code = "frame_too_large"

**HTTP status:** *(TCP only — no HTTP equivalent; HTTP frames are rejected by
the LB / web server before reaching beava)*
**When:** TCP frame `length` field exceeds the server's
`DEFAULT_TCP_MAX_FRAME_BYTES` (default 4 MiB).
**Path:** *(no path)*
**Recovery:** Split the request into smaller frames. Increase
`BEAVA_TCP_MAX_FRAME_BYTES` server-side if the request is legitimately large
(register payloads with hundreds of descriptors). The connection is closed
after this error — clients reconnect.

### code = "invalid_bloom_fpr"

**HTTP status:** 400
**When:** `bloom_member.fpr` is outside `(0.0, 1.0)`.
**Path:** `descriptors[<i>].agg.<feature>.params.fpr`.
**Recovery:** Provide `fpr ∈ (0, 1)` — typical values: `0.01`, `0.001`.

### code = "invalid_cast_target"

**HTTP status:** 400
**When:** A `bv.col(...).cast("complex64")` (or any cast target outside `{"str",
"int", "float", "bool"}`) reaches the wire. The Python SDK rejects this at
decoration time (`ValueError`); register-time rejection catches hand-written
JSON.
**Path:** `descriptors[<i>].ops[<j>].exprs.<col>` (within a `with_columns`
expression).
**Recovery:** Use one of the four supported cast targets.

### code = "invalid_expression"

**HTTP status:** 400
**When:** An expression string in a `filter` / `with_columns` / `where=`
predicate fails to parse against the canonical expression grammar (unbalanced
parens, unknown operator, malformed literal).
**Path:** `descriptors[<i>].ops[<j>].expr` (filter) or
`descriptors[<i>].ops[<j>].exprs.<col>` (with_columns) or
`descriptors[<i>].agg.<feature>.params.where` (where predicate).
**Recovery:** Fix the expression. The SDK should produce valid grammar
automatically — invalid expressions reaching the wire indicate a bug in the
SDK porter (cross-link: [expressions.md grammar](pipeline-dsl/expressions.md#grammar-canonical)).

### code = "invalid_percentile_q"

**HTTP status:** 400
**When:** `quantile.q` (formerly `percentile.q`) is outside `(0.0, 1.0)`.
**Path:** `descriptors[<i>].agg.<feature>.params.q`.
**Recovery:** Provide `q ∈ (0, 1)` — typical values: `0.5`, `0.95`, `0.99`.

### code = "invalid_top_k_k"

**HTTP status:** 400
**When:** `top_k.k` is outside `(0, 1024]`.
**Path:** `descriptors[<i>].agg.<feature>.params.k`.
**Recovery:** Provide `k ∈ (0, 1024]`. Default is 10 if omitted.

### code = "joins_not_supported"

**HTTP status:** 400
**When:** SDK-detected join attempt (cross-event aggregation, `bv.col` reaching
across event sources). Server-side equivalent is `feature_removed_no_joins_v0`.
**Path:** `<descriptor_name>`.
**Recovery:** beava is Redis-shaped, processing-time only — no cross-stream
joins ever (per `project_redis_shaped_no_event_time_ever`). Compose features
client-side.

### code = "key_shape_mismatch"

**HTTP status:** 400
**When:** `OP_GET` / `OP_BATCH_GET` `key` shape does not match the table's
declared `key` (e.g., single-string key for a composite-key table, or wrong
element types in the array).
**Path:** `requests[<i>].key` or `key`.
**Recovery:** Match the registered key shape. Composite keys are arrays in the
order the table declared them.

### code = "missing_field"

**HTTP status:** 400
**When:** A required field is missing from the push `fields` object.
**Path:** `fields.<field_name>`.
**Recovery:** Send all required fields per the registered schema. Mark the
field as `bv.Optional[T]` in the schema if it is genuinely optional.

### code = "missing_upstream"

**HTTP status:** 400
**When:** A `derivation` references an `upstream` not declared in this batch
and not previously registered.
**Path:** `descriptors[<i>].upstreams[<j>]`.
**Recovery:** Add the missing upstream to the same register call, or register
it first.

### code = "op_not_implemented"

**HTTP status:** 501 *(or 400 on TCP via `OP_ERROR_RESPONSE`)*
**When:** Client sent an opcode in a reserved range — e.g., `OP_PUSH_SYNC =
0x0011`, `OP_PUSH_MANY = 0x0012`, `OP_SET = 0x0030..0x003F`. v0 servers reply
with `op_not_implemented`; the opcodes are reserved for v0.1+.
**Path:** *(no path)*
**Recovery:** Use the v0 opcode set (`OP_PING`, `OP_REGISTER`, `OP_PUSH`,
`OP_GET`, `OP_BATCH_GET`, `OP_RESET`).

### code = "registration_conflict"

**HTTP status:** 409
**When:** A descriptor changes a field type, removes a field, removes a
derivation, or otherwise destructively mutates the registry without
`force=true`.
**Path:** `descriptors[<i>].schema.<field>` or `descriptors[<i>].agg.<feature>`.
**Recovery:** Either revert the destructive change OR re-issue with
`force=true` to apply (zeroes affected aggregations). Use `dry_run=true` to
preview the diff first. See [schema-evolution.md](schema-evolution.md) for the
full additive-vs-destructive matrix.
**Example:** [`examples/wire/register-conflict.error.json`](../examples/wire/register-conflict.error.json).

### code = "registration_cycle"

**HTTP status:** 400
**When:** Equivalent to `cycle` but emitted by the server-side Kahn topological
sort. The SDK's local validator surfaces it as `cycle` (the
`ValidationError.kind`); the wire code is `registration_cycle`.
**Path:** `<cycle path>`.
**Recovery:** Break the cycle.

### code = "reset_disabled_in_production"

**HTTP status:** 403 Forbidden
**Wire opcode (TCP):** `OP_ERROR_RESPONSE = 0xFFFF` (JSON body shape below)
**When:** A `POST /reset` (HTTP) or `OP_RESET` (TCP, opcode `0x0040`) request
arrives at a server whose effective `test_mode` flag is `false`. Per D-03
(Phase 13.4 Plan 08, USER-LOCKED), `OP_RESET` is the full state + registry
clear — production-by-default rejects it.
**Path:** *(no path)*
**Recovery:** Enable `test_mode` via either of two boot-time opt-ins (the
flag is computed at boot as the OR of both):

1. **Shell env var** — start the server with `BEAVA_TEST_MODE=1` in the
   environment. Per D-03 the check is exactly `== "1"`; `=true`, `=yes`,
   `=on`, etc. are NOT accepted.
2. **Programmatic Rust** — `ServerV18::bind_with_config(.., ServerV18Config {
   test_mode: true, .. })`. Used by integration tests that spawn an
   in-process server. Equivalent kwarg in the Python SDK is
   `bv.App(test_mode=True)` (embed mode); network mode (`bv.App(url=..,
   test_mode=True)`) ignores the kwarg with a warning since the server
   controls the gate.

Production servers MUST NOT set either gate. Test fixtures should set the
env var or pass `test_mode: true` to the constructor. The gate is read at
boot and cached on `AppState.effective_test_mode` — the env var cannot be
flipped at runtime to escalate.

Response body shape:

```json
{
  "error": {
    "code": "reset_disabled_in_production",
    "reason": "OP_RESET requires server test_mode (set BEAVA_TEST_MODE=1 or pass Config { test_mode: true } at server construction). See docs/error-codes.md."
  }
}
```

The `reason` text intentionally mentions BOTH opt-in paths so users see
actionable error text. Test
`phase13_4_reset_default_rejected::default_config_no_env_var_post_reset_returns_403_structured`
pins this contract.

**Source:** Phase 13.4 Plan 08 / D-03 (USER-LOCKED). Predecessor: the
pre-D-03 sketch used the shorter code `reset_disabled` with a single-flag
gate; this entry replaces it.

### code = "schema_invalid"

**HTTP status:** 400
**When:** Descriptor structure does not conform to its JSON Schema (missing
required field at the structural level, wrong nested type, malformed payload).
**Path:** `descriptors[<i>].<field>`.
**Recovery:** Fix the descriptor against
[`examples/wire/schemas/register.request.schema.json`](../examples/wire/schemas/register.request.schema.json).

### code = "schema_mismatch"

**HTTP status:** 400
**When:** A push field has the wrong type and cannot be coerced (e.g., string
`"abc"` for an `f64` field), OR `bv.sum(field=...)` was given a non-string
field arg per Q1 Path B (use the [two-stage `with_columns` + `sum`
pattern](pipeline-dsl/compilation-rules.md#boolean-sum-trick-recommended-pattern-for-conditional-counts)),
OR expression type-inference rejected the operand types (e.g., `bool & i64`),
OR boolean-combinator operands are not both `bool`.
**Path:** `fields.<field_name>` (push) or `descriptors[<i>].agg.<feature>` /
`descriptors[<i>].ops[<j>].expr` (register).
**Recovery:** Cast at the source, or fix the type at the source. For
boolean-sum, use the two-stage pattern: `with_columns(flag_int=col.cast("int")).group_by(...).agg(c=sum("flag_int", ...))`.
**Example:** [`examples/wire/push-validation-error.error.json`](../examples/wire/push-validation-error.error.json).

### code = "schema_propagation_failure"

**HTTP status:** 400
**When:** Schema propagation through an op chain failed at register time —
e.g., a `with_columns` expression references a field not in the upstream
schema, or a downstream op depends on a column the upstream chain doesn't
produce.
**Path:** `descriptors[<i>].ops[<j>]`.
**Recovery:** Trace the op chain; ensure every referenced column exists at the
point of reference. The SDK's `validate_descriptors` should surface this
client-side.

### code = "session_windows_not_supported_in_v0"

**HTTP status:** 400
**When:** Register payload references `bv.session(gap_ms=..., inner=...)` or
any session-window construct. Session windows are deferred to v0.1+ per
`.planning/ideas/session-windows-v0.1.md`.
**Path:** `descriptors[<i>].agg.<feature>`.
**Recovery:** Approximate via fixed-window aggregation (e.g., 30-minute
sliding window) for v0; session windows return in v0.1+ if/when justified.

### code = "table_key_invalid"

**HTTP status:** 400
**When:** `@bv.table(key=...)` declared with an empty list, a non-string
element, OR a key that references a field not in the upstream event source's
schema.
**Path:** `descriptors[<i>].key`.
**Recovery:** Provide a non-empty list of strings, each naming a field in the
upstream schema.

### code = "topological_order_violation"

**HTTP status:** 400
**When:** A descriptor references an upstream that appears later in the
descriptor list. The validator's topological-sort pass detected the violation
before cycle detection.
**Path:** `descriptors[<i>].upstreams[<j>]`.
**Recovery:** Reorder descriptors so every upstream appears before its
downstream. The SDK's `topo_sort` does this automatically; manual JSON
authors must take care.

### code = "unbounded_op_in_lifetime_mode"

**HTTP status:** 400
**When:** A windowless op (lifetime mode — `window=` omitted or set to
`"forever"`) without a finite per-entity memory bound was registered. Per
V0-MEM-GOV-02 (Phase 12.8), every windowless op MUST declare a bound via
`O1` / `BoundedSketch` / `BoundedByRequiredKwarg` / `BoundedByConfig`.
**Path:** `descriptors[<i>].agg.<feature>`.
**Recovery:** Either (1) add a `window=` kwarg to bound the op to a sliding
window, OR (2) use an op that has an O(1) lifetime bound (count / sum / avg /
... — see the catalogue), OR (3) for ops that REQUIRE a bound kwarg
(`first_n`, `last_n`, `lag`, etc.), provide the kwarg. The env-var
`BEAVA_MEMORY_GOV_ENFORCE=0` disables this check (operators MUST NOT disable
in production).

### code = "unions_not_supported_in_v0"

**HTTP status:** 400
**When:** SDK-detected union attempt (`bv.union(*events)`). Server-side
equivalent is `feature_removed_no_unions_v0`.
**Path:** `<descriptor_name>`.
**Recovery:** Multiplex client-side for v0; first-class union returns alongside
joins in a future minor.

### code = "unknown_event"

**HTTP status:** 404
**When:** The `event_name` (URL path on HTTP, routing prefix on TCP) is not
registered.
**Path:** *(no path; the offending name is in the URL or routing prefix)*
**Recovery:** Register the event source first via `OP_REGISTER`; check spelling.

### code = "unknown_field_event_time_v0"

**HTTP status:** 400
**When:** Register payload contains an `event_time_field` decorator key. Per
`project_redis_shaped_no_event_time_ever`, event-time was permanently removed
2026-04-30. Wire-level shim catches this in `pre_check_legacy_event_time_keys`.
**Path:** `nodes[<i>].<name>.event_time_field`.
**Recovery:** Drop the `event_time_field` key. Windowed operators bucket on
server-side `now_ms()` automatically.

### code = "unknown_field_reference"

**HTTP status:** 400
**When:** An expression references a field not in the upstream schema —
typically a typo (`bv.col("amunt")` instead of `bv.col("amount")`).
**Path:** `descriptors[<i>].ops[<j>].expr` or
`descriptors[<i>].ops[<j>].exprs.<col>` (or for `select` /
`drop` / `rename`, the `fields` / `mapping` keys).
**Recovery:** Fix the field reference. The SDK should validate against the
declared schema client-side.

### code = "unknown_field_tolerate_delay_v0"

**HTTP status:** 400
**When:** Register payload contains a `tolerate_delay_ms` decorator key. Per
the same `project_redis_shaped_no_event_time_ever` lock, out-of-order
tolerance is degenerate — the server timestamps at dispatch.
**Path:** `nodes[<i>].<name>.tolerate_delay_ms`.
**Recovery:** Drop the `tolerate_delay_ms` key.

### code = "unknown_field_type"

**HTTP status:** 400
**When:** A field type annotation is not in the supported wire vocabulary
(`str` / `f64` / `i64` / `bool` / `bytes` / `datetime`).
**Path:** `descriptors[<i>].schema.<field_name>`.
**Recovery:** Use one of the supported types. Custom types must be serialized
to one of the supported wire types at the source.

### code = "unknown_op"

**HTTP status:** 400
**When:** `agg.<feature>.op` references an op-string not in the operator
catalogue. After ADR-002, valid op-strings include the new Polars names
(`mean`, `var`, `std`, `n_unique`, `quantile`); old names (`avg`, `variance`,
`stddev`, `count_distinct`, `percentile`) are accepted as aliases by the
Python SDK in v0 but emit a `DeprecationWarning`.
**Path:** `descriptors[<i>].agg.<feature>.op`.
**Recovery:** Use one of the 53 catalogued op-strings. See
[docs/operators/index.md](operators/index.md) for the full list.

### code = "unknown_table"

**HTTP status:** 404
**When:** `OP_GET` / `OP_BATCH_GET` `table` is not a registered table name.
**Path:** `requests[<i>].table` or `table`.
**Recovery:** Register the table first via `OP_REGISTER`; check the registry
via `GET /registry`.

### code = "unsupported_content_type"

**HTTP status:** 415
**When:** HTTP request `Content-Type` is not `application/json`, OR the TCP
frame `content_type` byte is not `0x01` (JSON) or `0x02` (MessagePack
reserved).
**Path:** *(no path)*
**Recovery:** Set `Content-Type: application/json` on HTTP requests; use
`content_type = 0x01` on TCP frames.

### code = "unsupported_node_kind"

**HTTP status:** 400
**When:** Register payload has `kind="upsert"` / `kind="delete"` /
`kind="retract"` etc. — pre-12.7 surface that is permanently killed per
`project_v0_events_only_scope`. Per ADR-001, `kind="table"` is now PERMITTED
for aggregation-output (the JSON-prelude shim amendment lands in Phase 13.4).
**Path:** `nodes[<i>].<name>.kind`.
**Recovery:** Use `kind="event"`, `kind="table"` (aggregation-output only per
ADR-001), or `kind="derivation"`. Mutation surfaces (upsert/delete/retract)
are not supported in v0.

### code = "validation_failed"

**HTTP status:** 400
**When:** A custom validator on the event source rejected the push payload
(e.g., a future `validate=` kwarg on `@bv.event` rejects malformed values
beyond the type-level check).
**Path:** `fields.<field_name>` or `fields`.
**Recovery:** Read the `path` + `message` for the specific constraint that
failed; fix at the source.

### code = "wal_truncate_failed"

**HTTP status:** 500
**When:** `OP_RESET` triggered a WAL truncation that failed at the I/O layer
(disk full, permission error, hardware fault).
**Path:** *(no path)*
**Recovery:** The server's state is undefined after this error; restart is
recommended. Investigate the underlying I/O issue.

### code = "window_not_supported"

**HTTP status:** 400
**When:** A windowless-only op (`bloom_member`) was given a `window=` kwarg.
Some sketch ops are intentionally lifetime-only because windowed bloom filters
double the memory cost without sufficient benefit in v0.
**Path:** `descriptors[<i>].agg.<feature>.params.window`.
**Recovery:** Remove the `window=` kwarg; use lifetime mode for these ops.

## HTTP status mapping

| Status | Meaning | Codes that produce it |
|--------|---------|------------------------|
| `200` | Success | (no error code) — also `dedupe_replay` (idempotency, not an error). |
| `400` | Client error: validation, malformed input, business-rule rejection | All `aggregation_invalid_*`, `aggregation_on_table_not_supported`, `bad_return_type`, `batch_too_large`, `bv_table_class_form_not_supported`, `cycle`, `duplicate_name`, `event_time_not_supported_in_v0`, `feature_not_in_table`, `feature_removed_no_joins_v0`, `feature_removed_no_unions_v0`, `invalid_*` (cast / expression / percentile / topk / bloom), `joins_not_supported`, `key_shape_mismatch`, `missing_field`, `missing_upstream`, `registration_cycle`, `schema_invalid`, `schema_mismatch`, `schema_propagation_failure`, `session_windows_not_supported_in_v0`, `table_key_invalid`, `topological_order_violation`, `unbounded_op_in_lifetime_mode`, `unions_not_supported_in_v0`, `unknown_field_event_time_v0`, `unknown_field_reference`, `unknown_field_tolerate_delay_v0`, `unknown_field_type`, `unknown_op`, `validation_failed`, `window_not_supported`. |
| `403` | Forbidden — server policy rejects the operation | `reset_disabled_in_production`. |
| `404` | Not found | `unknown_event`, `unknown_table`. |
| `409` | Conflict — destructive change without `force=true` | `registration_conflict`, `force_required`. |
| `415` | Unsupported Media Type | `unsupported_content_type`. |
| `500` | Server error | `wal_truncate_failed`. |
| `501` | Not implemented (or `OP_ERROR_RESPONSE` on TCP) | `op_not_implemented`. |

The structured `code` is the **contract**; the HTTP status is a hint.
SDKs should dispatch on `code`, not on status, except for transport-level
concerns (retry on `5xx`, fail-fast on `4xx`).

## Forward-looking framing rule

Per Phase 12.7 D-02 (locked 2026-05-01), error messages use **forward-looking
framing**:

- ✅ "X is not supported in v0" — implies future versions may add support.
- ❌ "X has been removed" — implies the user once had it and lost it.
- ❌ "X is deprecated" — implies a migration path exists; for fresh-install
  v0 users no migration applies.

Examples (from the registered error library):

- `unsupported_node_kind`: "Node kind `upsert` is not supported in v0. beava
  v0 ships events-only (supported kinds: `event`, `table`,
  `derivation`)..."
- `event_time_not_supported_in_v0`: "The `event_time_field` decorator key is
  not supported in v0..."
- `session_windows_not_supported_in_v0`: "Session windows are not supported
  in v0..."
- `bv_table_class_form_not_supported`: "The `@bv.table` class form is not
  supported in v0; use the function form per ADR-001..."

The wire-level shim codes (`feature_removed_no_joins_v0` /
`feature_removed_no_unions_v0` / `unknown_field_event_time_v0` /
`unknown_field_tolerate_delay_v0`) are an exception — they predate the
12.7 D-02 framing lock and are kept stable for backwards compatibility with
deployment scripts that grep for them. Their **messages** are forward-looking
even though the **codes** are retrospective.

## Cross-references

- [Wire spec](wire-spec.md) — every error code referenced in this doc
  appears in a per-opcode error table in the wire spec.
- [Schema evolution](schema-evolution.md) — `registration_conflict` in
  destructive paths; `force=true` and `dry_run=true` flag semantics.
- [Pipeline DSL Compilation Rules — Ambiguity Matrix](pipeline-dsl/compilation-rules.md#ambiguity-matrix)
  — every FORBIDDEN row links to one of the codes above.
- [SDK API — shared](sdk-api/shared.md#error-semantics) — cross-language
  error-handling semantics.
- [SDK API — Python](sdk-api/python.md) — `RegistrationError` /
  `BinaryNotFoundError` / `ValidationError` Python signatures.
- [`examples/wire/schemas/error.schema.json`](../examples/wire/schemas/error.schema.json)
  — JSON Schema for the error envelope.
- [`examples/wire/register-conflict.error.json`](../examples/wire/register-conflict.error.json)
  — worked `registration_conflict` envelope.
- [`examples/wire/push-validation-error.error.json`](../examples/wire/push-validation-error.error.json)
  — worked `schema_mismatch` envelope.
- ADR pointers: [ADR-001](../.planning/decisions/ADR-001-bv-table-partial-overturn.md)
  (`@bv.table` aggregation-output revival narrows `unsupported_node_kind`),
  [ADR-002](../.planning/decisions/ADR-002-polars-op-rename.md) (Polars
  op-rename narrative; old names emit `DeprecationWarning` in Python v0).
