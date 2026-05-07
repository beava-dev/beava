//! Phase 13.4 Plan 06 — D-01 force=true diff matrix (STRICT classification +
//! categorized-lists payload).
//!
//! Per D-01 (USER-LOCKED), destructive registry changes (rename, type-change,
//! op removal, agg removal, window-change, key-cols change) require an explicit
//! `force=true` flag in the `POST /register` body. Without it, the server
//! returns:
//!
//! ```json
//! HTTP/1.1 409 Conflict
//! {
//!   "error": {
//!     "code": "force_required",
//!     "reason": "Destructive registry change requires force=true. See diff for details.",
//!     "diff": {
//!       "additive": [...],
//!       "destructive": [...]
//!     }
//!   }
//! }
//! ```
//!
//! Additive changes (new descriptor, new agg in existing block, new field on
//! event source) succeed without `force` and bump `registry_version`. The
//! diff payload is a **categorized JSON list** (NOT JSON-Patch) per D-01.
//!
//! Forward-looking error code is `force_required` per A-04 in
//! SCRATCH-PLANNER-NOTES.md.
//!
//! TDD: this is the RED gate — current register handler emits the legacy
//! `registration_conflict` (HTTP 409 + ResponseDiff) shape from the Phase 2
//! diff machinery. Task 6.b adds `classify_register_diff` +
//! `register_check_force_required` and routes the new categorized-payload
//! path through the dispatch chain.

#![cfg(feature = "testing")]

use beava_server::testing::TestServer;
use serde_json::{json, Value};

// ─── Shared register-payload helpers ────────────────────────────────────────

/// Baseline: one event source `Tx` with `amount: f64` + one `UserSpend` table
/// keyed by `user_id` with two windowed aggregations:
///   - `cnt = count(window=1h)`
///   - `total = sum(amount, window=1h)`
fn baseline_payload() -> Value {
    json!({
        "nodes": [
            {
                "kind": "event",
                "name": "Tx",
                "schema": {
                    "fields": {
                        "event_time": "i64",
                        "user_id": "str",
                        "amount": "f64"
                    },
                    "optional_fields": []
                }
            },
            {
                "kind": "derivation",
                "name": "UserSpend",
                "output_kind": "event",
                "upstreams": ["Tx"],
                "ops": [{
                    "op": "group_by",
                    "keys": ["user_id"],
                    "agg": {
                        "cnt":   {"op": "count", "params": {"window": "1h"}},
                        "total": {"op": "sum",   "params": {"field": "amount", "window": "1h"}}
                    }
                }],
                "schema": {
                    "fields": {"user_id": "str", "cnt": "i64", "total": "f64"},
                    "optional_fields": []
                }
            }
        ]
    })
}

/// Helper: register a payload, returning (status, body).
async fn post_register(ts: &TestServer, body: &Value) -> (u16, Value) {
    let resp = ts
        .post_json("/register", body)
        .await
        .expect("post /register");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.expect("json body");
    (status, body)
}

/// Helper: assert the 409 body carries the new D-01 force_required + diff
/// envelope. Returns the destructive-list slice for the caller to inspect
/// individual entries.
fn assert_force_required_409(status: u16, body: &Value) -> Vec<Value> {
    assert_eq!(
        status, 409,
        "expected HTTP 409 (force_required), got status={status}, body={body:#}"
    );
    let code = body["error"]["code"].as_str().unwrap_or_default();
    assert_eq!(
        code, "force_required",
        "expected error.code='force_required' (Phase 13.4 D-01), got code={code}, body={body:#}"
    );
    let diff = &body["error"]["diff"];
    assert!(
        diff.is_object(),
        "expected error.diff to be an object, got: {diff}"
    );
    assert!(
        diff["additive"].is_array(),
        "expected error.diff.additive to be an array, got: {diff:#}"
    );
    let destructive = diff["destructive"]
        .as_array()
        .unwrap_or_else(|| panic!("expected error.diff.destructive to be an array, got: {diff:#}"))
        .clone();
    assert!(
        !destructive.is_empty(),
        "destructive list must be non-empty for a force_required rejection, body={body:#}"
    );
    destructive
}

/// Asserts at least one destructive entry has `kind == expected_kind`.
fn assert_destructive_kind(destructive: &[Value], expected_kind: &str) {
    let found = destructive
        .iter()
        .any(|e| e.get("kind").and_then(|k| k.as_str()) == Some(expected_kind));
    assert!(
        found,
        "expected destructive entry with kind='{expected_kind}', got: {destructive:#?}"
    );
}

// ─── Test 1 — destructive rename without force → 409 + force_required ───────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_destructive_rename_without_force_returns_409() {
    let ts = TestServer::spawn().await.expect("spawn");
    let (status_a, _body_a) = post_register(&ts, &baseline_payload()).await;
    assert!(
        (200..300).contains(&status_a),
        "baseline register must succeed, got status={status_a}"
    );

    // Rename the `UserSpend` derivation to `UserSpendRenamed` while keeping
    // the same schema/ops.  This is the canonical destructive rename: one
    // descriptor disappears (UserSpend) and a new one with the same shape
    // appears (UserSpendRenamed).
    let mut payload_b = baseline_payload();
    payload_b["nodes"][1]["name"] = json!("UserSpendRenamed");
    let (status_b, body_b) = post_register(&ts, &payload_b).await;
    let destructive = assert_force_required_409(status_b, &body_b);
    assert_destructive_kind(&destructive, "rename");

    ts.shutdown().await.ok();
}

// ─── Test 2 — destructive type-change without force → 409 ───────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_destructive_type_change_without_force_returns_409() {
    let ts = TestServer::spawn().await.expect("spawn");
    let (status_a, _) = post_register(&ts, &baseline_payload()).await;
    assert!((200..300).contains(&status_a));

    // Flip `Tx.amount` from f64 to i64.
    let mut payload_b = baseline_payload();
    payload_b["nodes"][0]["schema"]["fields"]["amount"] = json!("i64");
    // Downstream derivation schema must also reflect the upstream change.
    payload_b["nodes"][1]["schema"]["fields"]["total"] = json!("i64");

    let (status_b, body_b) = post_register(&ts, &payload_b).await;
    let destructive = assert_force_required_409(status_b, &body_b);
    assert_destructive_kind(&destructive, "type_change");

    ts.shutdown().await.ok();
}

// ─── Test 3 — destructive op removal without force → 409 ────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_destructive_op_removal_without_force_returns_409() {
    let ts = TestServer::spawn().await.expect("spawn");
    let (status_a, _) = post_register(&ts, &baseline_payload()).await;
    assert!((200..300).contains(&status_a));

    // Re-register with the `UserSpend` derivation's ops list shortened — drop
    // the only op (group_by). This is technically also an agg-block removal;
    // the diff classifier emits an `op_removal` entry for the missing
    // group_by step on the derivation.
    let mut payload_b = baseline_payload();
    payload_b["nodes"][1]["ops"] = json!([]);
    let (status_b, body_b) = post_register(&ts, &payload_b).await;
    let destructive = assert_force_required_409(status_b, &body_b);
    assert_destructive_kind(&destructive, "op_removal");

    ts.shutdown().await.ok();
}

// ─── Test 4 — destructive window-change without force → 409 ─────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_destructive_window_change_without_force_returns_409() {
    let ts = TestServer::spawn().await.expect("spawn");
    let (status_a, _) = post_register(&ts, &baseline_payload()).await;
    assert!((200..300).contains(&status_a));

    // Shrink the `cnt` aggregation window from 1h → 30m.
    let mut payload_b = baseline_payload();
    payload_b["nodes"][1]["ops"][0]["agg"]["cnt"]["params"]["window"] = json!("30m");
    payload_b["nodes"][1]["ops"][0]["agg"]["total"]["params"]["window"] = json!("30m");

    let (status_b, body_b) = post_register(&ts, &payload_b).await;
    let destructive = assert_force_required_409(status_b, &body_b);
    assert_destructive_kind(&destructive, "window_change");

    ts.shutdown().await.ok();
}

// ─── Test 5 — destructive key-cols change without force → 409 ───────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_destructive_key_cols_change_without_force_returns_409() {
    let ts = TestServer::spawn().await.expect("spawn");
    let (status_a, _) = post_register(&ts, &baseline_payload()).await;
    assert!((200..300).contains(&status_a));

    // Switch group_by keys from ["user_id"] to ["user_id", "amount"].
    let mut payload_b = baseline_payload();
    payload_b["nodes"][1]["ops"][0]["keys"] = json!(["user_id", "amount"]);

    let (status_b, body_b) = post_register(&ts, &payload_b).await;
    let destructive = assert_force_required_409(status_b, &body_b);
    assert_destructive_kind(&destructive, "key_cols_change");

    ts.shutdown().await.ok();
}

// ─── Test 6 — destructive with force=true succeeds and bumps registry_version

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_destructive_with_force_true_succeeds_and_bumps_registry_version() {
    let ts = TestServer::spawn().await.expect("spawn");
    let (status_a, body_a) = post_register(&ts, &baseline_payload()).await;
    assert!((200..300).contains(&status_a));
    let v_a = body_a["registry_version"]
        .as_u64()
        .unwrap_or_else(|| panic!("baseline response must include registry_version: {body_a:#}"));

    // Now retry the destructive window-change WITH force=true.
    let mut payload_b = baseline_payload();
    payload_b["nodes"][1]["ops"][0]["agg"]["cnt"]["params"]["window"] = json!("30m");
    payload_b["nodes"][1]["ops"][0]["agg"]["total"]["params"]["window"] = json!("30m");
    payload_b["force"] = json!(true);

    let (status_b, body_b) = post_register(&ts, &payload_b).await;
    assert!(
        (200..300).contains(&status_b),
        "force=true on destructive change must succeed, got status={status_b}, body={body_b:#}"
    );
    let v_b = body_b["registry_version"]
        .as_u64()
        .unwrap_or_else(|| panic!("force=true response must include registry_version: {body_b:#}"));
    assert!(
        v_b > v_a,
        "registry_version must bump after force=true apply: was {v_a}, now {v_b}"
    );

    ts.shutdown().await.ok();
}

// ─── Test 7 — pure additive change (new descriptor) succeeds without force ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_additive_only_succeeds_without_force() {
    let ts = TestServer::spawn().await.expect("spawn");
    let (status_a, _) = post_register(&ts, &baseline_payload()).await;
    assert!((200..300).contains(&status_a));

    // Append a new event source AND a new derivation that depends on it.
    // Both are pure additions; no change to existing descriptors.
    let mut payload_b = baseline_payload();
    let new_event = json!({
        "kind": "event",
        "name": "Logout",
        "schema": {
            "fields": {"event_time": "i64", "user_id": "str"},
            "optional_fields": []
        }
    });
    let new_derivation = json!({
        "kind": "derivation",
        "name": "LogoutFeatures",
        "output_kind": "event",
        "upstreams": ["Logout"],
        "ops": [{
            "op": "group_by",
            "keys": ["user_id"],
            "agg": {
                "logout_cnt": {"op": "count", "params": {"window": "1h"}}
            }
        }],
        "schema": {
            "fields": {"user_id": "str", "logout_cnt": "i64"},
            "optional_fields": []
        }
    });
    payload_b["nodes"].as_array_mut().unwrap().push(new_event);
    payload_b["nodes"]
        .as_array_mut()
        .unwrap()
        .push(new_derivation);

    let (status_b, body_b) = post_register(&ts, &payload_b).await;
    assert!(
        (200..300).contains(&status_b),
        "pure additive change must succeed without force, got status={status_b}, body={body_b:#}"
    );
    // No `force_required` error envelope.
    assert!(
        body_b.get("error").is_none(),
        "additive register must not return error, got: {body_b:#}"
    );

    ts.shutdown().await.ok();
}

// ─── Test 8 — first-time register (empty prior) succeeds without force ──────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_first_time_no_force_required() {
    let ts = TestServer::spawn().await.expect("spawn");
    let (status, body) = post_register(&ts, &baseline_payload()).await;
    assert!(
        (200..300).contains(&status),
        "first-time register must succeed without force; nothing destructive against empty prior, got status={status}, body={body:#}"
    );
    assert!(
        body.get("error").is_none(),
        "first-time register must not return error, got: {body:#}"
    );
    ts.shutdown().await.ok();
}

// ─── Test 9 — agg removal (un-ignored Plan 09) ──────────────────────────────
//
// Covers the 6th destructive class (`agg_removal`). Plan 13.4-09 landed
// the `output_kind=table` end-to-end acceptance and un-ignored this test
// — it actually passed against the existing engine even before Plan 09's
// parse_entity_key fix because the destructive-rejection happens at
// register-validate time (no GET path involved).

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_destructive_agg_removal_without_force_returns_409() {
    let ts = TestServer::spawn().await.expect("spawn");
    let (status_a, _) = post_register(&ts, &baseline_payload()).await;
    assert!((200..300).contains(&status_a));

    // Drop the `total` aggregation from the existing block.
    let mut payload_b = baseline_payload();
    payload_b["nodes"][1]["ops"][0]["agg"]
        .as_object_mut()
        .unwrap()
        .remove("total");
    payload_b["nodes"][1]["schema"]["fields"]
        .as_object_mut()
        .unwrap()
        .remove("total");

    let (status_b, body_b) = post_register(&ts, &payload_b).await;
    let destructive = assert_force_required_409(status_b, &body_b);
    assert_destructive_kind(&destructive, "agg_removal");

    ts.shutdown().await.ok();
}

// ─── Test 11 — additive NewField on existing event with force=true succeeds ─
//
// Regression for the production deploy that 409'd after PR #1 merged. The
// pipeline added `session_id` to the `PageView` event source. Two diff
// systems disagreed on the classification:
//
//   - `classify_register_diff` (apply_shard pre-flight): NewField on an
//     existing event source → `additive`. No destructive entries.
//   - `compute_diff` (legacy, inside `execute_register_with_wal`): the
//     event source's schema differs → `changed: [ConflictDetail]`,
//     unconditionally returns `RegisterOutcome::Conflict` (HTTP 409
//     `registration_conflict`) regardless of `force=true` on the wire.
//
// `apply_shard`'s force-handling block at the time of the bug only ran
// `force_remove_descriptors` when `diff.destructive` was non-empty. Pure
// additive changes against existing descriptors slipped through the
// pre-flight, hit the legacy compute_diff inside execute_register, and
// 409'd — even with `force=true` set on the request body.
//
// Fixed behavior: with `force=true`, additive entries that target an
// existing descriptor (NewField on event, NewAgg on table) MUST also be
// pre-removed so execute_register sees them as fully new. The
// caller-explicit `force=true` carries the "drop existing state" intent
// for both destructive AND additive-against-existing changes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_additive_new_field_on_existing_event_with_force_true_succeeds() {
    let ts = TestServer::spawn().await.expect("spawn");
    let (status_a, body_a) = post_register(&ts, &baseline_payload()).await;
    assert!((200..300).contains(&status_a));
    let v_a = body_a["registry_version"]
        .as_u64()
        .unwrap_or_else(|| panic!("baseline response must include registry_version: {body_a:#}"));

    // Re-register the SAME shape with one new field added to the existing
    // `Tx` event source. `classify_register_diff` will emit
    // `additive: [NewField{event:Tx, field:session_id}]`,
    // `destructive: []`. With force=true, the request must succeed.
    let mut payload_b = baseline_payload();
    payload_b["nodes"][0]["schema"]["fields"]["session_id"] = json!("str");
    payload_b["force"] = json!(true);

    let (status_b, body_b) = post_register(&ts, &payload_b).await;
    assert!(
        (200..300).contains(&status_b),
        "force=true on additive NewField against existing event must succeed, got status={status_b}, body={body_b:#}"
    );
    assert!(
        body_b.get("error").is_none(),
        "force=true additive register must not return error envelope, got: {body_b:#}"
    );
    let v_b = body_b["registry_version"]
        .as_u64()
        .unwrap_or_else(|| panic!("force=true response must include registry_version: {body_b:#}"));
    assert!(
        v_b > v_a,
        "registry_version must bump after additive force=true apply: was {v_a}, now {v_b}"
    );

    ts.shutdown().await.ok();
}
