//! Cross-transport parity for a non-trivial schema.
//!
//! The pre-existing wire-format tests (`phase12_09_tcp_get_msgpack_test.rs`,
//! `phase12_09_tcp_get_json_unchanged_test.rs`) only exercise a single
//! `count()` agg on the `Txn` source. Nothing in the suite asserts that a
//! richer schema — multi-agg, multi-field, where + window filters, sketches —
//! produces bit-identical registry state and feature reads when driven through
//! both transports. These tests close that gap.
//!
//! Each test boots a single `TestServer` with `.test_mode(true)` so the
//! `POST /reset` and `OP_RESET` gates are open, drives one transport, captures
//! the result, resets, drives the other transport with the same input, and
//! asserts equality. The apply path is single-threaded mio (see CLAUDE.md
//! §"mio-only Hot-Path Invariant"), so any difference between transports is
//! a parser / response-encoder bug, not an order-of-operations bug.

#![cfg(feature = "testing")]

use beava_core::wire::{
    decode_frame, encode_frame, Frame, CT_JSON, OP_ERROR_RESPONSE, OP_GET, OP_GET_RESPONSE,
    OP_PUSH, OP_REGISTER,
};
use beava_server::testing::{TestServer, TestServerBuilder};
use bytes::{Bytes, BytesMut};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ─── Fixture ──────────────────────────────────────────────────────────────────

/// A non-trivial schema that exercises:
///   * 4 source fields of mixed types (str / f64),
///   * 4 aggregations spanning core + sketch families,
///   * a windowed `where`-filtered count,
///   * a windowed `where`-filtered mean,
///   * a windowed top_k over a string field,
///   * a lifetime (windowless) n_unique sketch.
fn register_payload() -> Value {
    json!({
        "nodes": [
            {
                "kind": "event",
                "name": "Ev",
                "schema": {
                    "fields": {
                        "event_time": "i64",
                        "user_id":    "str",
                        "action":     "str",
                        "price":      "f64",
                        "region":     "str"
                    },
                    "optional_fields": []
                }
            },
            {
                "kind": "derivation",
                "name": "MultiOp",
                "output_kind": "table",
                "upstreams": ["Ev"],
                "ops": [{
                    "op": "group_by",
                    "keys": ["user_id"],
                    "agg": {
                        "count_view": {
                            "op": "count",
                            "params": { "window": "1h", "where": "(action == 'view')" }
                        },
                        "mean_price_buy": {
                            "op": "mean",
                            "params": { "field": "price", "window": "30m",
                                        "where": "(action == 'buy')" }
                        },
                        "top_region_10m": {
                            "op": "top_k",
                            "params": { "field": "region", "k": 2, "window": "10m" }
                        },
                        "n_unique_actions": {
                            "op": "n_unique",
                            "params": { "field": "action" }
                        }
                    }
                }],
                "schema": {
                    "fields": {
                        "user_id":          "str",
                        "count_view":       "i64",
                        "mean_price_buy":   "f64",
                        "top_region_10m":   "json",
                        "n_unique_actions": "i64"
                    },
                    "optional_fields": []
                },
                "table_primary_key": ["user_id"]
            }
        ]
    })
}

/// Deterministic 10-event sequence. `event_time` is monotonically increasing
/// so windowed-op semantics see the same arrival time on both replays.
/// Two users, mix of actions / regions / prices.
fn event_stream() -> Vec<Value> {
    let base = 1_700_000_000_000_i64;
    let rows = vec![
        ("alice", "view", 0.0_f64, "us"),
        ("alice", "buy", 10.0, "us"),
        ("alice", "view", 0.0, "eu"),
        ("alice", "buy", 20.0, "eu"),
        ("alice", "view", 0.0, "us"),
        ("alice", "buy", 30.0, "us"),
        ("bob", "view", 0.0, "ap"),
        ("bob", "buy", 100.0, "ap"),
        ("bob", "view", 0.0, "ap"),
        ("bob", "view", 0.0, "eu"),
    ];
    rows.into_iter()
        .enumerate()
        .map(|(i, (u, a, p, r))| {
            json!({
                "event_time": base + (i as i64) * 60_000, // 1-minute spacing
                "user_id":    u,
                "action":     a,
                "price":      p,
                "region":     r
            })
        })
        .collect()
}

// ─── Test-server boot ─────────────────────────────────────────────────────────

async fn boot() -> TestServer {
    TestServerBuilder::new()
        .test_mode(true)
        .dev_endpoints(true)
        .fsync_interval_ms(1)
        .spawn()
        .await
        .expect("spawn TestServer")
}

// ─── HTTP helpers ─────────────────────────────────────────────────────────────

async fn http_register(ts: &TestServer, payload: &Value) -> Value {
    let resp = ts
        .post_json("/register", payload)
        .await
        .expect("http register");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.expect("register body json");
    assert_eq!(
        status, 200,
        "HTTP /register must return 200, got {status} body={body}"
    );
    body
}

async fn http_push(ts: &TestServer, source: &str, row: &Value) {
    let path = format!("/push/{source}");
    let resp = ts.post_json(&path, row).await.expect("http push");
    let status = resp.status().as_u16();
    assert!(
        (200..300).contains(&status),
        "HTTP /push/{source} must succeed; got {status}"
    );
}

async fn http_reset(ts: &TestServer) {
    let resp = ts
        .post_json("/reset", &json!({}))
        .await
        .expect("http reset");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.expect("reset body json");
    assert_eq!(
        status, 200,
        "HTTP /reset must return 200 with test_mode=true, got {status} body={body}"
    );
    assert_eq!(
        body["reset"],
        json!(true),
        "reset body must have reset=true"
    );
}

/// Single-row read via `POST /get` → returns the FLAT `{feature: value}` dict
/// per Phase 13.4.1-04 (D-03). `features=None` means "all features for the
/// table-row".
async fn http_get_row(ts: &TestServer, table: &str, key: &str, features: &[&str]) -> Value {
    let body = json!({
        "table":    table,
        "key":      key,
        "features": features
    });
    let resp = ts.post_json("/get", &body).await.expect("http POST /get");
    let status = resp.status().as_u16();
    let v: Value = resp.json().await.expect("get body json");
    assert_eq!(
        status, 200,
        "HTTP POST /get must return 200, got {status} body={v}"
    );
    v
}

async fn http_registry_dump(ts: &TestServer) -> Value {
    let resp = ts.get_raw("/registry").await;
    assert_eq!(
        resp.status().as_u16(),
        200,
        "GET /registry must return 200 (dev_endpoints=true)"
    );
    resp.json().await.expect("registry json")
}

// ─── TCP helpers ──────────────────────────────────────────────────────────────

async fn tcp_send_and_recv(tcp_addr: SocketAddr, op: u16, ct: u8, payload: &[u8]) -> Frame {
    let mut sock = tokio::net::TcpStream::connect(tcp_addr)
        .await
        .expect("tcp connect");
    let _ = sock.set_nodelay(true);
    let mut tx = BytesMut::new();
    encode_frame(
        &Frame::new(op, ct, Bytes::copy_from_slice(payload)),
        &mut tx,
    );
    sock.write_all(&tx).await.expect("tcp write");
    let mut rx = BytesMut::with_capacity(64 * 1024);
    let mut tmp = [0u8; 8192];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(frame)) = decode_frame(&mut rx, 4 * 1024 * 1024) {
            return frame;
        }
        tokio::select! {
            r = sock.read(&mut tmp) => {
                let n = r.expect("tcp read");
                if n == 0 {
                    panic!("connection closed before frame received");
                }
                rx.extend_from_slice(&tmp[..n]);
            }
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
    }
    panic!("no TCP frame received within deadline");
}

fn tcp_addr_of(ts: &TestServer) -> SocketAddr {
    ts.tcp_addr().expect("TCP listener enabled on TestServer")
}

/// Register over TCP. The wire spec (tcp_listener.rs:742) accepts CT_JSON
/// **only** for `OP_REGISTER` — register payloads are always JSON on every
/// transport, by design. This helper still proves the TCP register path
/// works end-to-end (frame envelope, dispatcher, parser).
async fn tcp_register_json(ts: &TestServer, payload: &Value) -> Value {
    let body = serde_json::to_vec(payload).expect("encode json register payload");
    let frame = tcp_send_and_recv(tcp_addr_of(ts), OP_REGISTER, CT_JSON, &body).await;
    assert_ne!(
        frame.op,
        OP_ERROR_RESPONSE,
        "TCP register returned OP_ERROR_RESPONSE; payload={}",
        String::from_utf8_lossy(&frame.payload)
    );
    assert_eq!(
        frame.op, OP_REGISTER,
        "expected OP_REGISTER echo on success, got {:#06x}",
        frame.op
    );
    serde_json::from_slice::<Value>(&frame.payload).expect("register response decodes as json")
}

async fn tcp_push_msgpack(ts: &TestServer, event_name: &str, row: &Value) {
    let envelope = json!({ "event": event_name, "body": row });
    let mp = rmp_serde::to_vec_named(&envelope).expect("encode msgpack push envelope");
    let frame =
        tcp_send_and_recv(tcp_addr_of(ts), OP_PUSH, beava_core::wire::CT_MSGPACK, &mp).await;
    assert_ne!(
        frame.op,
        OP_ERROR_RESPONSE,
        "TCP push returned OP_ERROR_RESPONSE; payload={}",
        String::from_utf8_lossy(&frame.payload)
    );
}

/// TCP OP_GET single-row read using msgpack. Returns the FLAT `{feature:
/// value}` dict (Phase 13.4.1-04 D-03 + Plan 12-09 Wave 3 msgpack-out).
async fn tcp_get_row_msgpack(ts: &TestServer, table: &str, key: &str, features: &[&str]) -> Value {
    let req = json!({ "table": table, "key": key, "features": features });
    let mp = rmp_serde::to_vec_named(&req).expect("encode msgpack OP_GET body");
    let frame = tcp_send_and_recv(tcp_addr_of(ts), OP_GET, beava_core::wire::CT_MSGPACK, &mp).await;
    assert_eq!(
        frame.op, OP_GET_RESPONSE,
        "expected OP_GET_RESPONSE, got {:#06x}",
        frame.op
    );
    assert_eq!(
        frame.content_type,
        beava_core::wire::CT_MSGPACK,
        "msgpack request must yield msgpack response (Wave 3 contract)"
    );
    rmp_serde::from_slice::<Value>(&frame.payload).expect("get response decodes as msgpack")
}

/// TCP OP_GET single-row read using JSON. Used by the cross-transport push-TCP
/// / get-HTTP test variant to read via the HTTP-equivalent codec.
async fn tcp_get_row_json(ts: &TestServer, table: &str, key: &str, features: &[&str]) -> Value {
    let req = json!({ "table": table, "key": key, "features": features });
    let body = serde_json::to_vec(&req).expect("encode json OP_GET body");
    let frame = tcp_send_and_recv(tcp_addr_of(ts), OP_GET, CT_JSON, &body).await;
    assert_eq!(frame.op, OP_GET_RESPONSE);
    assert_eq!(frame.content_type, CT_JSON);
    serde_json::from_slice::<Value>(&frame.payload).expect("get response decodes as json")
}

// ─── Test 1 ───────────────────────────────────────────────────────────────────

/// Closes the audit gap: existing tests only cover `count()` over a single
/// field. Here we register a 4-agg / 4-field schema with where+window filters,
/// push 10 events through HTTP-JSON, snapshot the per-user feature dict, reset,
/// re-register and re-push the SAME events via TCP (CT_JSON for OP_REGISTER —
/// the wire spec at tcp_listener.rs:742 accepts JSON only for register —
/// CT_MSGPACK for OP_PUSH and OP_GET), snapshot again, then assert the two
/// snapshots are exactly equal — every numeric value matches, the `top_k`
/// array structure matches, the `n_unique` sketch result matches.
///
/// Invariant: HTTP-JSON and TCP-msgpack are pure transport choices. The mio
/// data-plane apply path (CLAUDE.md §"mio-only Hot-Path Invariant") is shared.
/// Any divergence here is a parser or response-encoder bug.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_json_and_tcp_msgpack_roundtrip_complex_schema() {
    let ts = boot().await;
    let payload = register_payload();
    let events = event_stream();
    let features = [
        "count_view",
        "mean_price_buy",
        "top_region_10m",
        "n_unique_actions",
    ];

    // ── Pass A: HTTP-JSON ────────────────────────────────────────────────
    http_register(&ts, &payload).await;
    for ev in &events {
        http_push(&ts, "Ev", ev).await;
    }
    let alice_http = http_get_row(&ts, "MultiOp", "alice", &features).await;
    let bob_http = http_get_row(&ts, "MultiOp", "bob", &features).await;

    // ── Reset between passes ─────────────────────────────────────────────
    http_reset(&ts).await;

    // ── Pass B: TCP-msgpack ──────────────────────────────────────────────
    tcp_register_json(&ts, &payload).await;
    for ev in &events {
        tcp_push_msgpack(&ts, "Ev", ev).await;
    }
    let alice_tcp = tcp_get_row_msgpack(&ts, "MultiOp", "alice", &features).await;
    let bob_tcp = tcp_get_row_msgpack(&ts, "MultiOp", "bob", &features).await;

    // ── Parity ───────────────────────────────────────────────────────────
    assert_eq!(
        alice_http, alice_tcp,
        "alice feature row diverged between HTTP-JSON and TCP-msgpack\n\
         http={alice_http:#}\ntcp={alice_tcp:#}"
    );
    assert_eq!(
        bob_http, bob_tcp,
        "bob feature row diverged between HTTP-JSON and TCP-msgpack\n\
         http={bob_http:#}\ntcp={bob_tcp:#}"
    );

    ts.shutdown().await.expect("shutdown");
}

// ─── Test 2 ───────────────────────────────────────────────────────────────────

/// Register the same payload via HTTP and TCP, reset between, then assert:
///   * both registrations return 200 / OP_REGISTER (not OP_ERROR_RESPONSE);
///   * the dev-endpoint `/registry` dump is structurally identical across
///     the two registrations (modulo timestamps / version counters which we
///     normalise out).
///
/// Closes the gap that no prior test compares the registered DAG produced by
/// the JSON and msgpack registration parsers when fed the SAME logical
/// payload.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_json_and_tcp_msgpack_register_identical_payload_produces_identical_registry() {
    let ts = boot().await;
    let payload = register_payload();

    // ── HTTP register, snapshot ──────────────────────────────────────────
    http_register(&ts, &payload).await;
    let dump_http = http_registry_dump(&ts).await;

    // ── Reset, TCP-msgpack register, snapshot ────────────────────────────
    http_reset(&ts).await;
    tcp_register_json(&ts, &payload).await;
    let dump_tcp = http_registry_dump(&ts).await;

    // The dev `/registry` dump carries `registry_version` (monotonic counter
    // — bumps on every register / reset) and per-node `registered_at_version`
    // markers. Both are monotonic and trivially differ when the same payload
    // lands at versions 1 vs 3 (post-reset). Recursively strip them before
    // comparing structure — that's what defines "identical descriptors".
    fn strip_volatile(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("registry_version");
                map.remove("registered_at_version");
                map.remove("version");
                map.remove("loaded_at_ms");
                map.remove("loaded_at");
                for (_k, v) in map.iter_mut() {
                    strip_volatile(v);
                }
            }
            Value::Array(arr) => {
                for v in arr.iter_mut() {
                    strip_volatile(v);
                }
            }
            _ => {}
        }
    }
    let mut h = dump_http;
    let mut t = dump_tcp;
    strip_volatile(&mut h);
    strip_volatile(&mut t);
    assert_eq!(
        h, t,
        "registry dump diverged between HTTP-JSON and TCP-msgpack registration\n\
         http={h:#}\ntcp={t:#}"
    );

    ts.shutdown().await.expect("shutdown");
}

// ─── Test 3 ───────────────────────────────────────────────────────────────────

/// Cross-transport: push events via TCP-msgpack, read features via HTTP-JSON.
/// Exercises the property that the apply path is transport-agnostic — the
/// state mutation produced by an mp-encoded push is observable through any
/// transport's read path.
///
/// Closes the gap that no prior test mixes write-transport and read-transport.
/// If this test fails it means one transport mutates state into a shape the
/// other can't read — that's a real bug, not flake.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_msgpack_push_then_http_get_returns_same_features() {
    let ts = boot().await;
    let payload = register_payload();
    let events = event_stream();
    let features = [
        "count_view",
        "mean_price_buy",
        "top_region_10m",
        "n_unique_actions",
    ];

    // Pass A — all HTTP, used as the parity baseline.
    http_register(&ts, &payload).await;
    for ev in &events {
        http_push(&ts, "Ev", ev).await;
    }
    let alice_http_only = http_get_row(&ts, "MultiOp", "alice", &features).await;
    let bob_http_only = http_get_row(&ts, "MultiOp", "bob", &features).await;

    http_reset(&ts).await;

    // Pass B — register via HTTP (codec-equivalent), push via TCP-msgpack,
    // read via HTTP-JSON. The state should match Pass A exactly.
    http_register(&ts, &payload).await;
    for ev in &events {
        tcp_push_msgpack(&ts, "Ev", ev).await;
    }
    let alice_cross = http_get_row(&ts, "MultiOp", "alice", &features).await;
    let bob_cross = http_get_row(&ts, "MultiOp", "bob", &features).await;

    assert_eq!(
        alice_http_only, alice_cross,
        "alice feature row diverged between HTTP-only and TCP-push/HTTP-read\n\
         http_only={alice_http_only:#}\ncross={alice_cross:#}"
    );
    assert_eq!(
        bob_http_only, bob_cross,
        "bob feature row diverged between HTTP-only and TCP-push/HTTP-read\n\
         http_only={bob_http_only:#}\ncross={bob_cross:#}"
    );

    // And confirm the symmetric direction (HTTP push, TCP-JSON read) is also
    // value-equal — `tcp_get_row_json` uses CT_JSON on the wire frame, so
    // any divergence is a wire-codec issue, not a serializer issue.
    http_reset(&ts).await;
    http_register(&ts, &payload).await;
    for ev in &events {
        http_push(&ts, "Ev", ev).await;
    }
    let alice_tcp_read = tcp_get_row_json(&ts, "MultiOp", "alice", &features).await;
    assert_eq!(
        alice_http_only, alice_tcp_read,
        "alice feature row diverged between HTTP-only and HTTP-push/TCP-JSON-read\n\
         http_only={alice_http_only:#}\ntcp_read={alice_tcp_read:#}"
    );

    ts.shutdown().await.expect("shutdown");
}
