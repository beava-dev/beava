//! Phase 8 (folded scope): TCP `OP_PUSH` handler smoke test.
//!
//! Mirrors `phase6_push.rs` shape but drives the push through the TCP wire
//! instead of HTTP. The shared `execute_push` ensures both transports honor
//! the same WAL fsync + idem-cache + apply-loop semantics.

#![cfg(feature = "testing")]

use beava_core::wire::{CT_JSON, OP_ERROR_RESPONSE, OP_PUSH};
use beava_server::testing::TestServerBuilder;
use serde_json::json;
use std::time::Duration;

/// Register a `Transaction` event + count-aggregation derivation. Returns the
/// running TestServer.
async fn boot_with_transaction() -> beava_server::testing::TestServer {
    // dev_endpoints=true mounts `GET /get/:feature/:key` so we can query the
    // aggregated value end-to-end from HTTP after pushing via TCP.
    let ts = TestServerBuilder::new()
        .dev_endpoints(true)
        .spawn()
        .await
        .expect("spawn test server");

    let payload = json!({
        "nodes": [
            {
                "kind": "event",
                "name": "Transaction",
                "schema": {
                    "fields": {
                        "event_time": "i64",
                        "user_id": "str",
                        "amount": "f64"
                    },
                    "optional_fields": []
                },
            },
            {
                "kind": "derivation",
                "name": "TxnAgg",
                "output_kind": "table",
                "upstreams": ["Transaction"],
                "ops": [{"op": "group_by", "keys": ["user_id"], "agg": {
                    "cnt": {"op": "count", "params": {}}
                }}],
                "schema": {"fields": {"user_id": "str", "cnt": "i64"}, "optional_fields": []},
                "table_primary_key": ["user_id"]
            }
        ]
    });
    let mut tcp = ts.tcp_client().await.expect("tcp connect");
    let (op, _body) = tcp.register_json(payload).await.expect("register");
    assert_ne!(op, OP_ERROR_RESPONSE, "register should succeed");
    drop(tcp);
    ts
}

#[tokio::test]
async fn tcp_push_returns_ok_with_ack_lsn() {
    let ts = boot_with_transaction().await;
    let mut tcp = ts.tcp_client().await.expect("tcp connect");

    let body = json!({
        "event_time": 1000_i64,
        "user_id": "alice",
        "amount": 42.5
    });
    let (op, ack) = tcp.push_json("Transaction", body).await.expect("push_json");

    assert_eq!(op, OP_PUSH, "expected OP_PUSH success frame, got {op:#06x}");
    assert!(ack["ack_lsn"].as_u64().is_some(), "ack_lsn missing: {ack}");
    assert_eq!(ack["idempotent_replay"], false);
    assert!(ack["registry_version"].as_u64().is_some());

    drop(tcp);
    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn tcp_push_unknown_event_returns_error() {
    let ts = boot_with_transaction().await;
    let mut tcp = ts.tcp_client().await.expect("tcp connect");

    let (op, body) = tcp
        .push_json("NoSuchEvent", json!({"x": 1}))
        .await
        .expect("push_json");

    assert_eq!(op, OP_ERROR_RESPONSE, "expected error frame");
    assert_eq!(body["error"]["code"], "event_not_found", "body: {body}");

    drop(tcp);
    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn tcp_push_rejects_escaped_control_character_in_event_name() {
    let ts = boot_with_transaction().await;
    let mut tcp = ts.tcp_client().await.expect("tcp connect");

    let payload = br#"{"event":"Transaction\u0001","body":{"event_time":1000,"user_id":"alice","amount":1.0}}"#;
    let frame = tcp
        .send_raw(OP_PUSH, CT_JSON, bytes::Bytes::copy_from_slice(payload))
        .await
        .expect("raw tcp push");
    let body: serde_json::Value = serde_json::from_slice(&frame.payload).expect("json error body");

    assert_eq!(frame.op, OP_ERROR_RESPONSE, "expected error frame");
    assert_eq!(body["error"]["code"], "control_character_in_string");

    drop(tcp);
    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn tcp_push_invalid_body_returns_error() {
    let ts = boot_with_transaction().await;
    let mut tcp = ts.tcp_client().await.expect("tcp connect");

    // Wrong type: amount as string
    let (op, body) = tcp
        .push_json(
            "Transaction",
            json!({
                "event_time": 1000_i64,
                "user_id": "alice",
                "amount": "not-a-number"
            }),
        )
        .await
        .expect("push_json");

    assert_eq!(op, OP_ERROR_RESPONSE);
    assert_eq!(body["error"]["code"], "invalid_event", "body: {body}");

    drop(tcp);
    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn tcp_push_then_http_get_returns_aggregated_value() {
    // Proves the full TCP-push → apply-loop → state-table → HTTP-get path.
    let ts = boot_with_transaction().await;
    let mut tcp = ts.tcp_client().await.expect("tcp connect");

    for _ in 0..3 {
        let (op, _) = tcp
            .push_json(
                "Transaction",
                json!({"event_time": 1000_i64, "user_id": "alice", "amount": 10.0}),
            )
            .await
            .expect("push_json");
        assert_eq!(op, OP_PUSH);
    }
    drop(tcp);

    // Query via HTTP (same data plane).
    let url = format!("{}/get/cnt/alice", ts.base_url());
    let resp = reqwest::get(&url).await.expect("http get");
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.expect("json body");
    assert_eq!(body["value"], 3, "body: {body}");

    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn tcp_push_pipelined_three_succeed_in_order() {
    let ts = boot_with_transaction().await;
    let mut tcp = ts.tcp_client().await.expect("tcp connect");

    // Send 3 push frames without reading between.
    use beava_core::wire::{Frame, CT_JSON};
    use bytes::Bytes;
    for i in 0..3 {
        let envelope = json!({
            "event": "Transaction",
            "body": {"event_time": 1000_i64, "user_id": format!("u{i}"), "amount": (i as f64) * 10.0}
        });
        let payload = serde_json::to_vec(&envelope).unwrap();
        tcp.write_frame(&Frame {
            op: OP_PUSH,
            content_type: CT_JSON,
            payload: Bytes::from(payload),
        })
        .await
        .expect("write_frame");
    }
    let frames = tcp
        .read_n_frames(3)
        .await
        .expect("3 responses in strict FIFO");
    for f in &frames {
        assert_eq!(f.op, OP_PUSH, "every response must be OP_PUSH");
    }

    drop(tcp);
    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn tcp_push_idempotent_replay_returns_cached_with_flag() {
    // Register Transaction with dedupe_key=txn_id
    let ts = TestServerBuilder::new()
        .dev_endpoints(false)
        .spawn()
        .await
        .expect("spawn test server");
    let payload = json!({
        "nodes": [{
            "kind": "event",
            "name": "Transaction",
            "schema": {
                "fields": {
                    "event_time": "i64",
                    "user_id": "str",
                    "amount": "f64",
                    "txn_id": "str"
                },
                "optional_fields": []
            },
            "dedupe_key": "txn_id",
            "dedupe_window_ms": 60_000
        }]
    });
    let mut tcp = ts.tcp_client().await.expect("tcp connect");
    let (op, _) = tcp.register_json(payload).await.expect("register");
    assert_ne!(op, OP_ERROR_RESPONSE);

    let body = json!({
        "event_time": 1000_i64,
        "user_id": "alice",
        "amount": 10.0,
        "txn_id": "abc-123"
    });
    let (op1, ack1) = tcp
        .push_json("Transaction", body.clone())
        .await
        .expect("p1");
    let (op2, ack2) = tcp
        .push_json("Transaction", body.clone())
        .await
        .expect("p2");
    assert_eq!(op1, OP_PUSH);
    assert_eq!(op2, OP_PUSH);
    assert_eq!(ack1["ack_lsn"], ack2["ack_lsn"], "dedupe must replay LSN");
    assert_eq!(ack1["idempotent_replay"], false, "first push not a replay");
    assert_eq!(
        ack2["idempotent_replay"], true,
        "second push is a replay; flag flipped"
    );

    drop(tcp);
    let _ = tokio::time::timeout(Duration::from_secs(2), ts.shutdown()).await;
}
