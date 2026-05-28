//! Phase 6 Plan 03: `POST /push/{event_name}` + `IdemCache` integration tests.
//!
//! Exercises the full ingest path:
//! - HTTP body JSON parse + schema validation against the `@bv.event` descriptor
//! - Pre-apply dedupe-key lookup (byte-identical replay on hit)
//! - WAL append + group-commit fsync await
//! - apply_event_to_aggregations
//! - Idempotency-cache insert on miss
//!
//! See 06-03-PLAN.md for the task decomposition. These tests are the RED-commit
//! contract for Plan 03.
//!
//! Harness: uses `TestServer::builder().wal_dir(tempdir).spawn()` to spin up a
//! real Server instance with a per-test WAL directory. `fsync_interval_ms` is
//! set to 1 ms so tests finish quickly without fighting macOS `F_FULLSYNC`.

#![cfg(feature = "testing")]

use beava_server::testing::TestServerBuilder;
use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;

/// Register a Transaction event with optional dedupe_key and a count aggregation
/// grouped by user_id. Returns the tempdir so callers can keep it alive.
async fn register_transaction(
    ts: &beava_server::testing::TestServer,
    dedupe_key: Option<&str>,
    dedupe_window_ms: Option<u64>,
) {
    let mut event_node = json!({
        "kind": "event",
        "name": "Transaction",
        "schema": {
            "fields": {
                "event_time": "i64",
                "user_id": "str",
                "amount": "f64",
                "metadata": "json"
            },
            "optional_fields": ["metadata"]
        },
    });
    if let Some(k) = dedupe_key {
        event_node["dedupe_key"] = json!(k);
        // Schema must include txn_id field if dedupe_key points at it.
        event_node["schema"]["fields"][k] = json!("str");
    }
    if let Some(ms) = dedupe_window_ms {
        event_node["dedupe_window_ms"] = json!(ms);
    }

    let agg_node = json!({
        "kind": "derivation",
        "name": "TxnAgg",
        "output_kind": "table",
        "upstreams": ["Transaction"],
        "ops": [{"op": "group_by", "keys": ["user_id"], "agg": {
            "cnt": {"op": "count", "params": {}}
        }}],
        "schema": {"fields": {"user_id": "str", "cnt": "i64"}, "optional_fields": []},
        "table_primary_key": ["user_id"]
    });

    let payload = json!({"nodes": [event_node, agg_node]});
    let resp = ts.post_json("/register", &payload).await.expect("register");
    assert_eq!(resp.status().as_u16(), 200, "register must succeed");
}

async fn spawn_with_wal(tmp: &TempDir) -> beava_server::testing::TestServer {
    TestServerBuilder::new()
        .dev_endpoints(true)
        .wal_dir(tmp.path().to_path_buf())
        .fsync_interval_ms(1)
        .spawn()
        .await
        .expect("spawn")
}

async fn push_raw(
    ts: &beava_server::testing::TestServer,
    event_name: &str,
    body: &serde_json::Value,
) -> reqwest::Response {
    push_raw_bytes(ts, event_name, serde_json::to_vec(body).unwrap()).await
}

async fn push_raw_bytes(
    ts: &beava_server::testing::TestServer,
    event_name: &str,
    body: Vec<u8>,
) -> reqwest::Response {
    let url = format!("{}/push/{}", ts.base_url(), event_name);
    reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("push request")
}

#[tokio::test]
async fn push_happy_path_returns_ack_and_applies_event() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    let resp = push_raw(
        &ts,
        "Transaction",
        &json!({"user_id": "alice", "amount": 5.0, "event_time": 1_000_000}),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 200, "push must return 200");
    let body: serde_json::Value = resp.json().await.expect("ack body");
    assert!(body.get("ack_lsn").is_some(), "ack_lsn present: {body}");
    assert_eq!(body["idempotent_replay"], false);
    assert_eq!(body["registry_version"], 1);

    // Aggregation should now read cnt=1 for alice.
    let get = ts.get_raw("/get/cnt/alice").await;
    assert_eq!(get.status().as_u16(), 200);
    let got: serde_json::Value = get.json().await.unwrap();
    assert_eq!(got["value"], 1, "count should be 1 after one push: {got}");

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_rejects_control_characters_in_decoded_strings() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    let resp = push_raw(
        &ts,
        "Transaction",
        &json!({"user_id": "alice\u{0001}", "amount": 5.0, "event_time": 1_000_000}),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 400);
    let body: serde_json::Value = resp.json().await.expect("error body");
    assert_eq!(body["error"]["code"], "control_character_in_string");

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_rejects_control_characters_in_field_names() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    let resp = push_raw(
        &ts,
        "Transaction",
        &json!({"user\u{0001}_id": "alice", "amount": 5.0, "event_time": 1_000_000}),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 400);
    let body: serde_json::Value = resp.json().await.expect("error body");
    assert_eq!(body["error"]["code"], "control_character_in_string");

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_rejects_control_characters_in_nested_strings() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    let resp = push_raw(
        &ts,
        "Transaction",
        &json!({
            "user_id": "alice",
            "amount": 5.0,
            "event_time": 1_000_000,
            "metadata": {"nested": ["ok", "bad\u{0007}"]}
        }),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 400);
    let body: serde_json::Value = resp.json().await.expect("error body");
    assert_eq!(body["error"]["code"], "control_character_in_string");

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_verb_rejects_control_characters_in_event_name() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    let resp = ts
        .post_json(
            "/push",
            &json!({
                "event": "Transaction\u{0001}",
                "data": {"user_id": "alice", "amount": 5.0, "event_time": 1_000_000}
            }),
        )
        .await
        .expect("post /push verb");
    assert_eq!(resp.status().as_u16(), 400);
    let body: serde_json::Value = resp.json().await.expect("error body");
    assert_eq!(body["error"]["code"], "control_character_in_string");

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_allows_json_whitespace_and_escaped_non_control_strings() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    let body = br#"{
        "user_id": "alice \\n quote \" ok",
        "amount": 5.0,
        "event_time": 1000000
    }"#;
    let resp = push_raw_bytes(&ts, "Transaction", body.to_vec()).await;
    assert_eq!(resp.status().as_u16(), 200);

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_without_dedupe_key_bypasses_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    let r1 = push_raw(
        &ts,
        "Transaction",
        &json!({"user_id": "alice", "amount": 5.0, "event_time": 1_000_000}),
    )
    .await;
    assert_eq!(r1.status().as_u16(), 200);
    assert!(r1.headers().get("x-beava-idempotent-replay").is_none());
    let b1: serde_json::Value = r1.json().await.unwrap();

    let r2 = push_raw(
        &ts,
        "Transaction",
        &json!({"user_id": "alice", "amount": 5.0, "event_time": 1_000_001}),
    )
    .await;
    assert_eq!(r2.status().as_u16(), 200);
    assert!(r2.headers().get("x-beava-idempotent-replay").is_none());
    let b2: serde_json::Value = r2.json().await.unwrap();
    assert_ne!(b1["ack_lsn"], b2["ack_lsn"], "LSNs must differ");

    // Both applied → count=2
    let got: serde_json::Value = ts.get_json("/get/cnt/alice").await;
    assert_eq!(got["value"], 2);

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_with_dedupe_key_replays_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, Some("txn_id"), Some(60_000)).await;

    let body = json!({
        "txn_id": "t1",
        "user_id": "alice",
        "amount": 5.0,
        "event_time": 1_000_000,
    });
    let r1 = push_raw(&ts, "Transaction", &body).await;
    assert_eq!(r1.status().as_u16(), 200);
    assert!(r1.headers().get("x-beava-idempotent-replay").is_none());
    let b1_bytes = r1.bytes().await.expect("bytes");

    let r2 = push_raw(&ts, "Transaction", &body).await;
    assert_eq!(r2.status().as_u16(), 200);
    let header = r2
        .headers()
        .get("x-beava-idempotent-replay")
        .expect("replay header");
    assert_eq!(header.to_str().unwrap(), "1");
    let b2_bytes = r2.bytes().await.expect("bytes");

    assert_eq!(
        b1_bytes, b2_bytes,
        "dedupe replay body must be byte-identical"
    );

    // Count should be 1 (dupe did not apply)
    let got: serde_json::Value = ts.get_json("/get/cnt/alice").await;
    assert_eq!(got["value"], 1, "dedupe must not re-apply");

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_with_dedupe_different_key_no_replay() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, Some("txn_id"), Some(60_000)).await;

    let b1 = json!({"txn_id": "t1", "user_id": "alice", "amount": 5.0, "event_time": 1_000_000});
    let b2 = json!({"txn_id": "t2", "user_id": "alice", "amount": 5.0, "event_time": 1_000_001});

    let r1 = push_raw(&ts, "Transaction", &b1).await;
    assert_eq!(r1.status().as_u16(), 200);
    let a1: serde_json::Value = r1.json().await.unwrap();

    let r2 = push_raw(&ts, "Transaction", &b2).await;
    assert_eq!(r2.status().as_u16(), 200);
    assert!(r2.headers().get("x-beava-idempotent-replay").is_none());
    let a2: serde_json::Value = r2.json().await.unwrap();
    assert_ne!(a1["ack_lsn"], a2["ack_lsn"]);

    let got: serde_json::Value = ts.get_json("/get/cnt/alice").await;
    assert_eq!(got["value"], 2);

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_dedupe_after_window_expires() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, Some("txn_id"), Some(10)).await;

    let body = json!({"txn_id": "t1", "user_id": "alice", "amount": 5.0, "event_time": 1_000_000});
    let r1 = push_raw(&ts, "Transaction", &body).await;
    assert_eq!(r1.status().as_u16(), 200);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let r2 = push_raw(&ts, "Transaction", &body).await;
    assert_eq!(r2.status().as_u16(), 200);
    assert!(
        r2.headers().get("x-beava-idempotent-replay").is_none(),
        "after window expiry replay header must be absent"
    );

    let got: serde_json::Value = ts.get_json("/get/cnt/alice").await;
    assert_eq!(got["value"], 2, "re-apply after window expiry");

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_unknown_event_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;

    let resp = push_raw(&ts, "NonExistent", &json!({"x": 1})).await;
    assert_eq!(resp.status().as_u16(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "event_not_found");

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_schema_mismatch_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    // Missing "amount" field.
    let resp = push_raw(
        &ts,
        "Transaction",
        &json!({"user_id": "alice", "event_time": 1_000_000}),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "invalid_event");

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_ack_lsn_strictly_monotonic() {
    let tmp = tempfile::tempdir().unwrap();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    let mut lsns = Vec::new();
    for i in 0..5 {
        let r = push_raw(
            &ts,
            "Transaction",
            &json!({
                "user_id": "alice",
                "amount": 1.0,
                "event_time": 1_000_000 + i,
            }),
        )
        .await;
        assert_eq!(r.status().as_u16(), 200);
        let b: serde_json::Value = r.json().await.unwrap();
        lsns.push(b["ack_lsn"].as_u64().unwrap());
    }
    for w in lsns.windows(2) {
        assert!(w[0] < w[1], "LSN must strictly increase: {lsns:?}");
    }

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn push_persisted_to_wal() {
    let tmp = tempfile::tempdir().unwrap();
    let wal_dir = tmp.path().to_path_buf();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    let body = json!({"user_id": "alice", "amount": 5.0, "event_time": 1_000_000});
    let r = push_raw(&ts, "Transaction", &body).await;
    assert_eq!(r.status().as_u16(), 200);

    // Flush via shutdown so WAL is durable.
    ts.shutdown().await.expect("shutdown");

    // Plan 12.6-15: under the locked mio data-plane architecture, push events
    // are persisted to hand-rolled *.wal files (WalBufferRing → WalWriter),
    // not the legacy beava-persistence *.log segments (WalSink). Verify
    // durability by reading the *.wal segment(s) and grepping for the event
    // payload bytes.
    let wal_files: Vec<std::path::PathBuf> = std::fs::read_dir(&wal_dir)
        .expect("read wal dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("wal"))
        .collect();
    assert!(
        !wal_files.is_empty(),
        "at least one *.wal segment expected after push, got 0 in {wal_dir:?}"
    );
    let mut found = false;
    for f in &wal_files {
        let data = std::fs::read(f).expect("read wal segment");
        if String::from_utf8_lossy(&data).contains("alice") {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "hand-rolled WAL must contain the alice event payload across {} segments",
        wal_files.len()
    );
}

#[tokio::test]
async fn push_sync_data_before_ack() {
    let tmp = tempfile::tempdir().unwrap();
    let wal_dir = tmp.path().to_path_buf();
    let ts = spawn_with_wal(&tmp).await;
    register_transaction(&ts, None, None).await;

    let body = json!({"user_id": "alice", "amount": 5.0, "event_time": 1_000_000});
    let r = push_raw(&ts, "Transaction", &body).await;
    assert_eq!(r.status().as_u16(), 200);

    // Immediately (before shutdown) the WAL file should have grown past its
    // initial header: evidence that fsync happened before ACK.
    let mut found = false;
    for entry in std::fs::read_dir(&wal_dir).expect("read wal dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("log") {
            let size = std::fs::metadata(&path).expect("meta").len();
            // Header is 24 bytes (magic + version + start_lsn + registry_version);
            // any record bumps past that.
            if size > 24 {
                found = true;
                break;
            }
        }
    }
    assert!(
        found,
        "WAL segment should have record bytes flushed before ACK"
    );

    ts.shutdown().await.expect("shutdown");
}
