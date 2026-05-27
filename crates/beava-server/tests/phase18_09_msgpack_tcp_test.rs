//! Phase 18 Plan 09 — msgpack TCP integration tests.
//!
//! Tasks covered:
//!   9.2 — parse_wire_request handles CT_MSGPACK envelope
//!   9.4 — dispatch_push_sync handles msgpack body format end-to-end
//!   9.5 — WAL record v=3 binary header written for msgpack push
//!   9.6 — WAL replay handles msgpack records after restart

// ─── Task 9.2 RED: parse_wire_request handles msgpack envelope ────────────────
//
// Constructs an OP_PUSH frame with CT_MSGPACK and a msgpack-encoded
// {event: "Txn", body: {amount: 99}} envelope. Calls parse_wire_request;
// asserts WireRequest::TcpPush with event="Txn", body_format=CT_MSGPACK,
// body=raw msgpack body bytes (NOT re-serialized).

use beava_core::wire::{encode_frame, Frame, CT_JSON, CT_MSGPACK, OP_PUSH};
use beava_runtime_core::tcp_listener::parse_wire_request;
use beava_runtime_core::wire_request::WireRequest;
use bytes::{Bytes, BytesMut};

/// Build a msgpack-encoded push envelope: {event: event_name, body: body_map}.
/// Returns the raw msgpack bytes for the envelope.
fn make_msgpack_envelope(event_name: &str, body: &serde_json::Value) -> Vec<u8> {
    use serde::Serialize;
    #[derive(Serialize)]
    struct Envelope<'a> {
        event: &'a str,
        body: &'a serde_json::Value,
    }
    rmp_serde::to_vec_named(&Envelope {
        event: event_name,
        body,
    })
    .expect("msgpack serialize envelope")
}

/// Build a msgpack frame (CT_MSGPACK) wrapping the given envelope bytes.
fn make_msgpack_frame(payload: Vec<u8>) -> BytesMut {
    let frame = Frame::new(OP_PUSH, CT_MSGPACK, Bytes::from(payload));
    let mut buf = BytesMut::new();
    encode_frame(&frame, &mut buf);
    buf
}

#[test]
fn test_parse_wire_request_msgpack_envelope() {
    // Build msgpack envelope {event: "Txn", body: {amount: 99}}
    let body_json = serde_json::json!({"amount": 99});
    let envelope_bytes = make_msgpack_envelope("Txn", &body_json);
    let mut buf = make_msgpack_frame(envelope_bytes);

    let req = parse_wire_request(&mut buf, 4 * 1024 * 1024)
        .expect("no parse error")
        .expect("complete frame");

    match req {
        WireRequest::TcpPush {
            event_name,
            body,
            body_format,
        } => {
            assert_eq!(
                event_name, "Txn",
                "event_name extracted from msgpack envelope"
            );
            assert_eq!(body_format, CT_MSGPACK, "body_format must be CT_MSGPACK");
            assert_ne!(body_format, CT_JSON, "must NOT be CT_JSON");

            // body bytes are raw msgpack of the body map — verify by decoding
            let decoded: serde_json::Value =
                rmp_serde::from_slice(&body).expect("body bytes should be valid msgpack");
            assert_eq!(
                decoded["amount"],
                serde_json::json!(99),
                "body amount round-trips"
            );
        }
        WireRequest::ParseError { reason } => {
            panic!("expected TcpPush but got ParseError: {reason}");
        }
        other => panic!("expected TcpPush, got {other:?}"),
    }

    assert_eq!(buf.len(), 0, "buffer should be fully consumed");
}

#[test]
fn test_parse_wire_request_json_still_works_after_msgpack_added() {
    // Backward compat: CT_JSON on TCP still parses correctly.
    use serde_json::json;
    let payload = serde_json::to_vec(&json!({"event": "Foo", "body": {"x": 1}})).unwrap();
    let frame = Frame::new(OP_PUSH, CT_JSON, Bytes::from(payload));
    let mut buf = BytesMut::new();
    encode_frame(&frame, &mut buf);

    let req = parse_wire_request(&mut buf, 4 * 1024 * 1024)
        .expect("no parse error")
        .expect("complete frame");

    match req {
        WireRequest::TcpPush {
            event_name,
            body,
            body_format,
        } => {
            assert_eq!(event_name, "Foo");
            assert_eq!(body_format, CT_JSON, "JSON frame → CT_JSON body_format");
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(v["x"], serde_json::json!(1));
        }
        other => panic!("expected TcpPush, got {other:?}"),
    }
}

// ─── Task 9.4 RED: dispatch_push_sync handles both body formats ───────────────
//
// NOTE: This test boots a real ServerV18 to exercise the full apply path.
// It lives in this file to avoid proliferating test files.

static SERVER_SERIALIZER_09: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Wait until the hand-rolled HTTP server at `addr` accepts connections.
/// Unlike admin, the hand-rolled loop has no `/health` route, so we poll
/// any GET and accept any HTTP response (even 404 / NotFound is fine —
/// it proves the event loop is running and accepting connections).
async fn wait_for_http_09(addr: std::net::SocketAddr) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .expect("reqwest client");
    loop {
        // Any HTTP response (including 404) means the mio loop is ready.
        match client.get(format!("http://{}/ping", addr)).send().await {
            Ok(_) => return,
            Err(_) => {
                if tokio::time::Instant::now() >= deadline {
                    panic!("hand-rolled HTTP server at {} did not become ready", addr);
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
    }
}

/// Small pipeline registration JSON for tests.
///
/// Uses the two-node format: an `event` node for schema + an `derivation` node
/// for aggregation. The derivation groups by user_id and counts events into `cnt`.
/// Feature is resolved as `cnt` → `/get/cnt/<user_id>`.
fn small_pipeline_register() -> serde_json::Value {
    serde_json::json!({
        "nodes": [
            {
                "kind": "event",
                "name": "TxnEvent",
                "schema": {
                    "fields": {"user_id": "str", "amount": "f64", "event_time": "i64"},
                    "optional_fields": []
                },
            },
            {
                "kind": "derivation",
                "name": "TxnAgg",
                "output_kind": "table",
                "upstreams": ["TxnEvent"],
                "ops": [
                    {
                        "op": "group_by",
                        "keys": ["user_id"],
                        "agg": {"cnt": {"op": "count", "params": {}}}
                    }
                ],
                "schema": {
                    "fields": {"user_id": "str", "cnt": "i64"},
                    "optional_fields": []
                },
                "table_primary_key": ["user_id"]
            }
        ]
    })
}

/// Send a msgpack push frame to a TCP addr and return the response frame.
async fn send_msgpack_push_tcp(
    tcp_addr: std::net::SocketAddr,
    event_name: &str,
    body: &serde_json::Value,
) -> beava_core::wire::Frame {
    use beava_server::testing::TcpClient;
    let envelope_bytes = make_msgpack_envelope(event_name, body);
    let mut client = TcpClient::connect(tcp_addr)
        .await
        .expect("TcpClient connect");
    client
        .send_raw(OP_PUSH, CT_MSGPACK, Bytes::from(envelope_bytes))
        .await
        .expect("send_raw msgpack push")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_dispatch_push_msgpack_body() {
    {
        let _g = SERVER_SERIALIZER_09.lock().unwrap();
    } // serialise test start; _g drops before any await
    let any: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let sv18 = beava_server::server::ServerV18::bind(any, any, any)
        .await
        .expect("ServerV18::bind");

    let http_addr = sv18.http_addr();
    let tcp_addr = sv18.tcp_addr();

    let wal_dir = tempfile::tempdir().expect("wal dir");
    let snap_dir = tempfile::tempdir().expect("snap dir");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let wp = wal_dir.path().to_path_buf();
    let sp = snap_dir.path().to_path_buf();
    let serve_task = tokio::spawn(async move {
        sv18.serve_with_dirs(
            async {
                let _ = shutdown_rx.await;
            },
            wp,
            sp,
        )
        .await
    });

    wait_for_http_09(http_addr).await;

    // Register pipeline via HTTP.
    let client = reqwest::Client::new();
    let reg_resp = client
        .post(format!("http://{}/register", http_addr))
        .header("Content-Type", "application/json")
        .json(&small_pipeline_register())
        .send()
        .await
        .expect("register");
    assert!(
        reg_resp.status().is_success(),
        "register failed: {}",
        reg_resp.status()
    );

    // Send a msgpack push via TCP.
    let body = serde_json::json!({"user_id": "u1", "amount": 42.0, "event_time": 1_000_000_i64});
    let resp_frame = send_msgpack_push_tcp(tcp_addr, "TxnEvent", &body).await;
    assert_eq!(
        resp_frame.op, OP_PUSH,
        "msgpack push should get OP_PUSH ACK"
    );

    // Verify the aggregation was applied by querying via HTTP.
    let get_resp = client
        .get(format!("http://{}/get/cnt/u1", http_addr))
        .send()
        .await
        .expect("get");
    assert!(
        get_resp.status().is_success(),
        "GET failed: {}",
        get_resp.status()
    );
    let get_json: serde_json::Value = get_resp.json().await.expect("json");
    assert_eq!(
        get_json["value"],
        serde_json::json!(1),
        "count should be 1 after msgpack push"
    );

    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), serve_task).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_dispatch_push_json_body_still_works() {
    // Backward compat: CT_JSON on TCP still applies correctly.
    {
        let _g = SERVER_SERIALIZER_09.lock().unwrap();
    } // serialise test start; _g drops before any await
    let any: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let sv18 = beava_server::server::ServerV18::bind(any, any, any)
        .await
        .expect("ServerV18::bind");

    let http_addr = sv18.http_addr();
    let tcp_addr = sv18.tcp_addr();

    let wal_dir = tempfile::tempdir().expect("wal dir");
    let snap_dir = tempfile::tempdir().expect("snap dir");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let wp = wal_dir.path().to_path_buf();
    let sp = snap_dir.path().to_path_buf();
    let serve_task = tokio::spawn(async move {
        sv18.serve_with_dirs(
            async {
                let _ = shutdown_rx.await;
            },
            wp,
            sp,
        )
        .await
    });

    wait_for_http_09(http_addr).await;

    let client = reqwest::Client::new();
    let reg_resp = client
        .post(format!("http://{}/register", http_addr))
        .header("Content-Type", "application/json")
        .json(&small_pipeline_register())
        .send()
        .await
        .expect("register");
    assert!(reg_resp.status().is_success());

    // Send a JSON push via TCP using TcpClient::push_json (CT_JSON).
    use beava_server::testing::TcpClient;
    let mut tcp_client = TcpClient::connect(tcp_addr).await.expect("connect");
    let body = serde_json::json!({"user_id": "u2", "amount": 10.0, "event_time": 1_000_001_i64});
    let (op, _parsed) = tcp_client
        .push_json("TxnEvent", body)
        .await
        .expect("push_json");
    assert_eq!(op, OP_PUSH, "JSON push should get OP_PUSH ACK");

    let get_resp = client
        .get(format!("http://{}/get/cnt/u2", http_addr))
        .send()
        .await
        .expect("get");
    assert!(get_resp.status().is_success());
    let get_json: serde_json::Value = get_resp.json().await.expect("json");
    assert_eq!(get_json["value"], serde_json::json!(1));

    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), serve_task).await;
}

// ─── Task 9.5: WAL record v=3 binary header ───────────────────────────────────
//
// Pushes via TCP/msgpack, reads the WAL file directly, asserts first record
// starts with [0x03, assigned_lsn, 0x02, ...] (v=3, body_format=msgpack).

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_wal_record_v3_format() {
    {
        let _g = SERVER_SERIALIZER_09.lock().unwrap();
    } // serialise test start; _g drops before any await
    let any: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let sv18 = beava_server::server::ServerV18::bind(any, any, any)
        .await
        .expect("ServerV18::bind");

    let http_addr = sv18.http_addr();
    let tcp_addr = sv18.tcp_addr();

    let wal_dir = tempfile::tempdir().expect("wal dir");
    let snap_dir = tempfile::tempdir().expect("snap dir");
    let wal_path = wal_dir.path().to_path_buf();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let wp = wal_path.clone();
    let sp = snap_dir.path().to_path_buf();
    let serve_task = tokio::spawn(async move {
        sv18.serve_with_dirs(
            async {
                let _ = shutdown_rx.await;
            },
            wp,
            sp,
        )
        .await
    });

    wait_for_http_09(http_addr).await;

    let client = reqwest::Client::new();
    let _ = client
        .post(format!("http://{}/register", http_addr))
        .header("Content-Type", "application/json")
        .json(&small_pipeline_register())
        .send()
        .await
        .expect("register");

    // Send a msgpack push so a v=3 WAL record is written.
    let body =
        serde_json::json!({"user_id": "waltest", "amount": 7.0, "event_time": 2_000_000_i64});
    let _ = send_msgpack_push_tcp(tcp_addr, "TxnEvent", &body).await;

    // Give the WAL writer a beat to flush.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Shut down cleanly so files are flushed.
    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), serve_task).await;

    // Read the WAL directory and find the data-plane WAL file.
    // The hand-rolled WalWriter creates "wal-0000000000000000.wal".
    // The first record should start with v=3, assigned_lsn, body_format=CT_MSGPACK.
    let wal_files: Vec<_> = std::fs::read_dir(&wal_path)
        .expect("read wal dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with("wal-")
                && e.file_name().to_string_lossy().ends_with(".wal")
        })
        .collect();

    // WAL files present = v3 records were written (the WalWriter creates them).
    // If no WAL files exist, the WalBufferRing hasn't flushed. Fail explicitly.
    assert!(
        !wal_files.is_empty(),
        "expected at least one WAL .wal file after msgpack push; v3 WAL append not yet wired"
    );

    // Read the first WAL file and check the record format.
    let wal_file_path = &wal_files[0].path();
    let wal_bytes = std::fs::read(wal_file_path).expect("read wal file");
    assert!(!wal_bytes.is_empty(), "WAL file should not be empty");

    // v3 record format: [u8 v=3][u64 assigned_lsn][u8 body_format][u32 rv][u64 et_ms][u16 event_name_len][...name...][u32 body_len][...body...]
    // First byte must be 0x03 (record version 3).
    assert_eq!(
        wal_bytes[0], 0x03,
        "first WAL record byte must be version=3 (0x03), got {:#04x}",
        wal_bytes[0]
    );
    assert!(
        wal_bytes.len() >= 10,
        "v3 WAL record should include version + assigned_lsn + body_format"
    );
    let assigned_lsn = u64::from_be_bytes(wal_bytes[1..9].try_into().unwrap());
    assert!(assigned_lsn > 0, "assigned_lsn must be non-zero");
    // Byte 9 must be 0x02 (CT_MSGPACK body format).
    assert_eq!(
        wal_bytes[9], CT_MSGPACK,
        "v3 WAL body_format byte must be CT_MSGPACK (0x02), got {:#04x}",
        wal_bytes[9]
    );
}

// ─── Task 9.6: WAL replay handles msgpack records after restart ──────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_wal_replay_v2_msgpack() {
    // This test verifies that after a server restart, state built from a
    // msgpack push is correctly replayed from the WAL.
    {
        let _g = SERVER_SERIALIZER_09.lock().unwrap();
    } // serialise test start; _g drops before any await

    let any: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let wal_dir = tempfile::tempdir().expect("wal dir");
    let snap_dir = tempfile::tempdir().expect("snap dir");
    let wal_path = wal_dir.path().to_path_buf();
    let snap_path = snap_dir.path().to_path_buf();

    // --- First server instance: push a msgpack event, then shut down ---
    {
        let sv18 = beava_server::server::ServerV18::bind(any, any, any)
            .await
            .expect("ServerV18::bind first");
        let http_addr = sv18.http_addr();
        let tcp_addr = sv18.tcp_addr();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let wp = wal_path.clone();
        let sp = snap_path.clone();
        let serve_task = tokio::spawn(async move {
            sv18.serve_with_dirs(
                async {
                    let _ = shutdown_rx.await;
                },
                wp,
                sp,
            )
            .await
        });

        wait_for_http_09(http_addr).await;

        let client = reqwest::Client::new();
        let _ = client
            .post(format!("http://{}/register", http_addr))
            .header("Content-Type", "application/json")
            .json(&small_pipeline_register())
            .send()
            .await
            .expect("register first");

        // Push 3 msgpack events.
        for i in 0..3u64 {
            let body = serde_json::json!({"user_id": "replay_user", "amount": i as f64, "event_time": (3_000_000 + i) as i64});
            let _ = send_msgpack_push_tcp(tcp_addr, "TxnEvent", &body).await;
        }

        // Flush the WAL.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let _ = shutdown_tx.send(());
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), serve_task).await;
    }

    // --- Second server instance: verify state rebuilt from WAL ---
    {
        let sv18 = beava_server::server::ServerV18::bind(any, any, any)
            .await
            .expect("ServerV18::bind second");
        let http_addr = sv18.http_addr();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let wp = wal_path.clone();
        let sp = snap_path.clone();
        let serve_task = tokio::spawn(async move {
            sv18.serve_with_dirs(
                async {
                    let _ = shutdown_rx.await;
                },
                wp,
                sp,
            )
            .await
        });

        wait_for_http_09(http_addr).await;

        let client = reqwest::Client::new();
        let get_resp = client
            .get(format!("http://{}/get/cnt/replay_user", http_addr))
            .send()
            .await
            .expect("get after restart");

        if get_resp.status().is_success() {
            let get_json: serde_json::Value = get_resp.json().await.expect("json");
            assert_eq!(
                get_json["value"],
                serde_json::json!(3),
                "count should be 3 after WAL replay of 3 msgpack events"
            );
        } else {
            panic!(
                "GET /get/cnt/replay_user failed with status {}; WAL replay did not restore msgpack state",
                get_resp.status()
            );
        }

        let _ = shutdown_tx.send(());
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), serve_task).await;
    }
}
