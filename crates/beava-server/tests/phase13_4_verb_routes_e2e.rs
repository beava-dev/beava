//! Phase 13.4 Plan 04 — verb-style HTTP routes (end-to-end gate).
//!
//! Boots a real `TestServer` and posts to the new verb-style routes:
//! - `POST /ping` — HTTP mirror of TCP `OP_PING (0x0000)`; returns 200 `{"pong": true, "registry_version": <n>}`.
//! - `POST /push` — event name lives in the JSON body (`{"event":"Tx","data":{...}}`)
//!   instead of the URL path.
//!
//! Also asserts that the legacy `POST /push/:event_name` route still works —
//! A-07 backward-compat in `SCRATCH-PLANNER-NOTES.md`. The ~20 in-tree tests
//! that hit the legacy URL shape must keep passing during the migration.
//!
//! TDD: this is the RED gate — Task 4.b stubbed `Route::Ping`, `Route::PushVerb`,
//! `Route::PushSyncVerb` to return `WireRequest::ParseError`, so Tests 1, 3, 4
//! fail with a 5xx. Test 2 (wrong-method on `/ping`) and Test 5 (legacy `/push/Tx`)
//! pass even before Task 4.d because they don't depend on the new dispatch.
//! Task 4.d wires `parse_verb_push` and the `HttpPing` dispatch arm to flip
//! Tests 1/3/4 to GREEN.

#![cfg(feature = "testing")]

use beava_server::testing::TestServer;
use serde_json::json;
use std::time::Duration;

// ─── Shared registration helper ─────────────────────────────────────────────

/// Tiny pipeline: `Tx` event with `cnt = count()` derived per `user_id`.
/// Used by the push tests.
fn register_payload() -> serde_json::Value {
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
                "output_kind": "table",
                "upstreams": ["Tx"],
                "ops": [{
                    "op": "group_by",
                    "keys": ["user_id"],
                    "agg": {
                        "cnt": {"op": "count", "params": {}}
                    }
                }],
                "schema": {
                    "fields": {"user_id": "str", "cnt": "i64"},
                    "optional_fields": []
                },
                "table_primary_key": ["user_id"]
            }
        ]
    })
}

async fn register(ts: &TestServer) {
    let resp = ts
        .post_json("/register", &register_payload())
        .await
        .expect("register");
    assert!(
        resp.status().is_success(),
        "register failed: status={}",
        resp.status()
    );
}

// ─── Test 1 — POST /ping returns 200 + {"pong": true, "registry_version": N} ─

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn post_ping_returns_200_pong_with_registry_version() {
    let ts = TestServer::spawn().await.expect("spawn");

    // Empty object body — the route's contract says body is empty, but
    // post_json always sends JSON; an empty `{}` is valid JSON and the
    // verb-style /ping handler ignores body content.
    //
    // Pre-register: registry_version is 0; the response must include
    // `{"pong": true, "registry_version": 0}` so SDK clients can use /ping
    // as a cheap registry-version probe (matches the locked v0 surface +
    // python/beava `app.ping()` contract).
    let resp = ts.post_json("/ping", &json!({})).await.expect("post /ping");
    let status = resp.status().as_u16();
    let body_text = resp.text().await.expect("body text");
    assert_eq!(
        status, 200,
        "POST /ping must return 200, got status={status}, body={body_text}"
    );

    let body: serde_json::Value =
        serde_json::from_str(&body_text).expect("response body must be JSON");
    assert_eq!(
        body["pong"], true,
        "POST /ping must return pong=true, got body={body:#}"
    );
    assert_eq!(
        body["registry_version"], 0,
        "POST /ping pre-register must report registry_version=0, got body={body:#}"
    );

    // After register, /ping must report the bumped registry_version.
    register(&ts).await;
    let resp = ts.post_json("/ping", &json!({})).await.expect("post /ping");
    let body: serde_json::Value = resp.json().await.expect("post-register body");
    assert_eq!(body["pong"], true);
    assert_eq!(
        body["registry_version"], 1,
        "post-register /ping must report registry_version=1, got body={body:#}"
    );

    ts.shutdown().await.ok();
}

// ─── Test 2 — wrong method on /ping returns 405 ────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn post_ping_wrong_method_returns_405() {
    let ts = TestServer::spawn().await.expect("spawn");

    let url = format!("{}/ping", ts.base_url());
    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("get /ping");
    assert_eq!(
        resp.status().as_u16(),
        405,
        "GET /ping must return 405, got status={}",
        resp.status()
    );

    ts.shutdown().await.ok();
}

// ─── Test 3 — POST /push (event in body) accepts and increments cnt ────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn post_push_verb_with_event_in_body_accepts() {
    let ts = TestServer::spawn().await.expect("spawn");
    register(&ts).await;

    // Verb-style push: event name in JSON body, not URL path.
    let body = json!({
        "event": "Tx",
        "data": {"event_time": 1000, "user_id": "alice", "amount": 12.50}
    });
    let resp = ts.post_json("/push", &body).await.expect("post /push verb");
    let status = resp.status().as_u16();
    let body_text = resp.text().await.expect("push body text");
    assert!(
        (200..300).contains(&status),
        "POST /push (verb) must return 2xx, got status={status}, body={body_text}"
    );

    // Verify the push landed: GET /get/cnt/alice should report cnt=1.
    let url = format!("{}/get/cnt/alice", ts.base_url());
    let get_resp = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("get cnt");
    assert_eq!(
        get_resp.status().as_u16(),
        200,
        "GET /get/cnt/alice must return 200 after verb-style push"
    );
    let cnt_body: serde_json::Value = get_resp.json().await.expect("cnt json body");
    assert_eq!(
        cnt_body["value"], 1,
        "cnt must be 1 after one verb-style push, got body={cnt_body:#}"
    );

    ts.shutdown().await.ok();
}

// ─── Test 4 — POST /push missing `event` field returns 400 + structured ────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn post_push_verb_missing_event_returns_400() {
    let ts = TestServer::spawn().await.expect("spawn");
    register(&ts).await;

    // Body missing the `event` field — verb-style push contract violation.
    let body = json!({
        "data": {"event_time": 1000, "user_id": "alice", "amount": 12.50}
    });
    let resp = ts
        .post_json("/push", &body)
        .await
        .expect("post /push missing event");
    let status = resp.status().as_u16();
    let body_text = resp.text().await.expect("body text");
    assert_eq!(
        status, 400,
        "POST /push without `event` must return 400, got status={status}, body={body_text}"
    );

    let body_json: serde_json::Value =
        serde_json::from_str(&body_text).expect("response body must be JSON");
    let code = body_json["error"]["code"]
        .as_str()
        .unwrap_or_else(|| panic!("structured error.code missing, body={body_json:#}"));
    assert_eq!(
        code, "missing_event_name_in_body",
        "expected error.code = missing_event_name_in_body, got code={code}, body={body_json:#}"
    );

    ts.shutdown().await.ok();
}

// ─── Test 5 — legacy /push/:event_name route still works ───────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn legacy_push_route_still_works() {
    let ts = TestServer::spawn().await.expect("spawn");
    register(&ts).await;

    // Legacy path-segment shape: event name in URL, body is the inner data
    // payload directly (no `{event, data}` envelope). A-07 backward-compat.
    let body = json!({"event_time": 1000, "user_id": "alice", "amount": 12.50});
    let url = format!("{}/push/Tx", ts.base_url());
    let resp = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("post legacy /push/Tx");
    let status = resp.status().as_u16();
    let body_text = resp.text().await.expect("legacy push body text");
    assert!(
        (200..300).contains(&status),
        "legacy POST /push/Tx must return 2xx (A-07 backward-compat), \
         got status={status}, body={body_text}"
    );

    // Verify the legacy push landed too.
    let url = format!("{}/get/cnt/alice", ts.base_url());
    let get_resp = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("get cnt legacy");
    let cnt_body: serde_json::Value = get_resp.json().await.expect("cnt json body");
    assert_eq!(
        cnt_body["value"], 1,
        "legacy push must increment cnt to 1, got body={cnt_body:#}"
    );

    ts.shutdown().await.ok();
}
