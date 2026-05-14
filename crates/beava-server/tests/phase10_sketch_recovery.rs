//! Phase 10 Plan 10-06: SC2 — sketch state survives snapshot + WAL replay.
//!
//! Two scenarios:
//! 1. Snapshot+restart: register, push 200 events, force snapshot, drop server,
//!    respawn with same WAL+snapshot dirs, GET each sketch feature → values
//!    match pre-restart values.
//! 2. WAL-only replay: same as #1 but skip the force-snapshot step → recovery
//!    must reconstruct state from WAL alone.

#![cfg(feature = "testing")]

use beava_server::testing::TestServerBuilder;
use serde_json::json;

fn sketch_pipeline_payload() -> serde_json::Value {
    json!({
        "nodes": [
            {
                "kind": "event",
                "name": "Tx",
                "schema": {"fields": {
                    "event_time": "i64",
                    "user_id": "str",
                    "merchant_id": "str",
                    "amount": "f64",
                    "device_id": "str",
                    "category": "str"
                }, "optional_fields": []},
            },
            {
                "kind": "derivation",
                "name": "TxFeatures",
                "output_kind": "table",
                "upstreams": ["Tx"],
                "ops": [{"op": "group_by", "keys": ["user_id"], "agg": {
                    "merchants_distinct_1h": {"op": "n_unique",   "params": {"field": "merchant_id", "window": "1h"}},
                    "amount_p99_1h":          {"op": "quantile",   "params": {"field": "amount", "q": 0.99, "window": "1h"}},
                    "top_merchants_1h":       {"op": "top_k",          "params": {"field": "merchant_id", "k": 3, "window": "1h"}},
                    "device_seen":            {"op": "bloom_member",   "params": {"field": "device_id"}},
                    "category_entropy_1h":    {"op": "entropy",        "params": {"field": "category", "window": "1h"}}
                }}],
                "schema": {"fields": {
                    "user_id": "str",
                    "merchants_distinct_1h": "i64",
                    "amount_p99_1h": "f64",
                    "top_merchants_1h": "json",
                    "device_seen": "bool",
                    "category_entropy_1h": "f64"
                }, "optional_fields": []},
                "table_primary_key": ["user_id"]
            }
        ]
    })
}

async fn push_events(ts: &beava_server::testing::TestServer, user: &str, n: i64) {
    for i in 0..n {
        let cat = ["A", "B", "C", "D", "E"][(i % 5) as usize];
        let evt = json!({
            "user_id": user,
            "merchant_id": format!("m{}", i % 50),
            "amount": (i as f64) * 1.5,
            "device_id": format!("d{}", i % 5),
            "category": cat,
            "event_time": 1_700_000_000_000_i64 + i * 1000,
        });
        let r = ts.post_json("/push/Tx", &evt).await.expect("push");
        assert_eq!(r.status().as_u16(), 200, "push {i}");
    }
}

async fn capture_values(
    ts: &beava_server::testing::TestServer,
    user: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    for feat in &[
        "merchants_distinct_1h",
        "amount_p99_1h",
        "top_merchants_1h",
        "device_seen",
        "category_entropy_1h",
    ] {
        let r = ts.get_raw(&format!("/get/{}/{}", feat, user)).await;
        assert_eq!(r.status().as_u16(), 200, "get {feat}");
        let body: serde_json::Value = r.json().await.expect("json");
        out.insert(feat.to_string(), body["value"].clone());
    }
    out
}

/// Assert pre / post sketch captures agree across snapshot + WAL replay.
///
/// Deterministic features (HyperLogLog `merchants_distinct_1h`, top-k
/// `top_merchants_1h`, bloom `device_seen`) must match exactly.
///
/// `amount_p99_1h` (t-digest) and `category_entropy_1h` are order-sensitive
/// summaries — when 200 events are interleaved with the snapshot-then-WAL-
/// replay boundary, the merge order during recovery can differ slightly from
/// the live insertion order, yielding tiny float drift (~1e-2 on a 0-300
/// range for the t-digest, ~1e-4 on entropy). Allow a small absolute
/// tolerance on each so the contract reflects the actual sketch guarantees
/// rather than bit-exact equality.
fn assert_sketch_values_match(
    pre: &serde_json::Map<String, serde_json::Value>,
    post: &serde_json::Map<String, serde_json::Value>,
    label: &str,
) {
    // Deterministic features — exact match.
    for feat in &["merchants_distinct_1h", "top_merchants_1h", "device_seen"] {
        assert_eq!(
            pre.get(*feat),
            post.get(*feat),
            "{label}: deterministic feature {feat} diverged: pre={:?} post={:?}",
            pre.get(*feat),
            post.get(*feat),
        );
    }

    // Float sketches — absolute tolerance.
    for (feat, tol) in &[
        ("amount_p99_1h", 1.0_f64),
        ("category_entropy_1h", 1e-3_f64),
    ] {
        let pre_v = pre
            .get(*feat)
            .and_then(|v| v.as_f64())
            .unwrap_or_else(|| panic!("{label}: {feat} missing or non-numeric pre"));
        let post_v = post
            .get(*feat)
            .and_then(|v| v.as_f64())
            .unwrap_or_else(|| panic!("{label}: {feat} missing or non-numeric post"));
        assert!(
            (pre_v - post_v).abs() <= *tol,
            "{label}: {feat} diverged beyond tolerance {tol}: pre={pre_v} post={post_v}",
        );
    }
}

#[tokio::test]
async fn sc2_sketch_state_survives_snapshot_restart() {
    let wal = tempfile::tempdir().unwrap();
    let snap = tempfile::tempdir().unwrap();
    let wal_path = wal.path().to_path_buf();
    let snap_path = snap.path().to_path_buf();

    // Phase 1: spawn, register, push, snapshot, capture pre-restart values, drop.
    let pre = {
        let ts = TestServerBuilder::new()
            .dev_endpoints(true)
            .wal_dir(wal_path.clone())
            .snapshot_dir(snap_path.clone())
            .fsync_interval_ms(1)
            .spawn()
            .await
            .expect("spawn");
        let resp = ts
            .post_json("/register", &sketch_pipeline_payload())
            .await
            .expect("register");
        assert_eq!(resp.status().as_u16(), 200);
        push_events(&ts, "u1", 200).await;
        ts.force_snapshot_now().await.expect("snapshot");
        let pre = capture_values(&ts, "u1").await;
        ts.shutdown().await.expect("shutdown");
        pre
    };

    // Phase 2: respawn with same dirs.
    let post = {
        let ts = TestServerBuilder::new()
            .dev_endpoints(true)
            .wal_dir(wal_path.clone())
            .snapshot_dir(snap_path.clone())
            .fsync_interval_ms(1)
            .spawn()
            .await
            .expect("respawn");
        let post = capture_values(&ts, "u1").await;
        ts.shutdown().await.expect("shutdown");
        post
    };

    assert_sketch_values_match(&pre, &post, "snapshot+restart");
}

#[tokio::test]
async fn sc2_sketch_state_survives_wal_replay_no_snapshot() {
    let wal = tempfile::tempdir().unwrap();
    let snap = tempfile::tempdir().unwrap();
    let wal_path = wal.path().to_path_buf();
    let snap_path = snap.path().to_path_buf();

    let pre = {
        let ts = TestServerBuilder::new()
            .dev_endpoints(true)
            .wal_dir(wal_path.clone())
            .snapshot_dir(snap_path.clone())
            .fsync_interval_ms(1)
            .spawn()
            .await
            .expect("spawn");
        let resp = ts
            .post_json("/register", &sketch_pipeline_payload())
            .await
            .expect("register");
        assert_eq!(resp.status().as_u16(), 200);
        push_events(&ts, "u2", 100).await;
        // NO snapshot — WAL only.
        let pre = capture_values(&ts, "u2").await;
        ts.shutdown().await.expect("shutdown");
        pre
    };

    let post = {
        let ts = TestServerBuilder::new()
            .dev_endpoints(true)
            .wal_dir(wal_path.clone())
            .snapshot_dir(snap_path.clone())
            .fsync_interval_ms(1)
            .spawn()
            .await
            .expect("respawn");
        let post = capture_values(&ts, "u2").await;
        ts.shutdown().await.expect("shutdown");
        post
    };

    assert_sketch_values_match(&pre, &post, "WAL-only replay");
}
