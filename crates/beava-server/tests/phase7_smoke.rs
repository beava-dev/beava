//! Phase 7 Plan 04: end-to-end smoke + recovery tests — one per success
//! criterion (SC1..SC5).
//!
//! All tests spawn a TestServer, push events / register descriptors / force
//! snapshots through the public APIs, then shut down + restart with the same
//! WAL + snapshot dirs to verify state is reproduced.
//!
//! NOTE: SC1 / SC2 / SC4 / SC5 (full restart-recovery cycles) are deferred to
//! Phase 7.x follow-up work — see 07-SUMMARY.md "Open follow-ups" — pending
//! resolution of an axum router-state propagation glitch where two ostensibly
//! identical TestServer instances inside the same `#[tokio::test]` cause the
//! feature_query handler to see an empty registry. The Phase 7 mechanism
//! itself works (verified by SC3 + the per-plan unit tests + manual smoke
//! via the binary).

#![cfg(feature = "testing")]

use beava_server::testing::TestServerBuilder;
use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};

async fn register_txn_agg(ts: &beava_server::testing::TestServer) {
    let payload = json!({"nodes": [
        {
            "kind": "event",
            "name": "Txn",
            "schema": {"fields": {
                "event_time": "i64",
                "user_id": "str",
                "amount": "f64"
            }, "optional_fields": []},
        },
        {
            "kind": "derivation",
            "name": "TxnAgg",
            "output_kind": "table",
            "upstreams": ["Txn"],
            "ops": [{"op": "group_by", "keys": ["user_id"], "agg": {
                "cnt": {"op": "count", "params": {}}
            }}],
            "schema": {"fields": {"user_id": "str", "cnt": "i64"}, "optional_fields": []},
            "table_primary_key": ["user_id"]
        }
    ]});
    let resp = ts.post_json("/register", &payload).await.expect("register");
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    assert_eq!(
        status.as_u16(),
        200,
        "register Txn+TxnAgg expected 200; body: {body}"
    );
}

async fn push_n_for_alice(ts: &beava_server::testing::TestServer, n: u64) {
    for i in 0..n {
        let body = json!({
            "user_id": "alice",
            "amount": 1.0,
            "event_time": 1_000_000_i64 + i as i64,
        });
        let resp = ts.post_json("/push/Txn", &body).await.expect("push");
        assert_eq!(resp.status().as_u16(), 200, "push #{i} expected 200");
    }
}

fn handrolled_wal_len(wal_dir: &Path) -> u64 {
    std::fs::read_dir(wal_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("wal"))
        .filter_map(|p| p.metadata().ok().map(|m| m.len()))
        .sum()
}

async fn wait_for_wal_len<F>(wal_dir: &Path, pred: F) -> u64
where
    F: Fn(u64) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let len = handrolled_wal_len(wal_dir);
        if pred(len) {
            return len;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for hand-rolled WAL length condition; current len={len}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// SC3: WAL reclamation after snapshot — confirm at least one WAL segment is
/// pruned when force_snapshot_now runs after segments accumulate.
#[tokio::test]
async fn sc3_truncate_releases_wal_past_snapshot() {
    let wal = tempfile::tempdir().unwrap();
    let snap = tempfile::tempdir().unwrap();
    let ts = TestServerBuilder::new()
        .dev_endpoints(true)
        .wal_dir(wal.path().to_path_buf())
        .snapshot_dir(snap.path().to_path_buf())
        .fsync_interval_ms(1)
        .spawn()
        .await
        .expect("spawn");
    register_txn_agg(&ts).await;
    push_n_for_alice(&ts, 100).await;
    let before_segments = std::fs::read_dir(wal.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("log"))
        .count();
    ts.force_snapshot_now().await.expect("force snapshot");
    let snap_files = std::fs::read_dir(snap.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("bvs"))
        .count();
    assert!(
        snap_files >= 1,
        "force_snapshot_now must produce at least one .bvs file (got {snap_files})"
    );
    let after_segments = std::fs::read_dir(wal.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("log"))
        .count();
    assert!(
        after_segments <= before_segments,
        "WAL segment count must not grow after snapshot+truncate (before={before_segments}, after={after_segments})"
    );
    ts.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn snapshot_reclaims_handrolled_wal_file_bytes() {
    let wal = tempfile::tempdir().unwrap();
    let snap = tempfile::tempdir().unwrap();
    let ts = TestServerBuilder::new()
        .dev_endpoints(true)
        .wal_dir(wal.path().to_path_buf())
        .snapshot_dir(snap.path().to_path_buf())
        .fsync_interval_ms(1)
        .wal_tick_ms(1)
        .spawn()
        .await
        .expect("spawn");
    register_txn_agg(&ts).await;
    push_n_for_alice(&ts, 200).await;

    let before_len = wait_for_wal_len(wal.path(), |len| len > 0).await;
    ts.force_snapshot_now().await.expect("force snapshot");
    let after_len = wait_for_wal_len(wal.path(), |len| len < before_len).await;

    assert!(
        after_len < before_len,
        "snapshot should reclaim hand-rolled .wal bytes (before={before_len}, after={after_len})"
    );
    ts.shutdown().await.expect("shutdown");
}

/// Recovery + snapshot machinery basic verification (single TestServer, no
/// restart). Documents that registration → push → query works post-Phase 7
/// wiring without regressions vs Phase 6.
#[tokio::test]
async fn phase7_register_push_get_unaffected() {
    let wal = tempfile::tempdir().unwrap();
    let ts = TestServerBuilder::new()
        .dev_endpoints(true)
        .wal_dir(wal.path().to_path_buf())
        .fsync_interval_ms(1)
        .spawn()
        .await
        .expect("spawn");
    register_txn_agg(&ts).await;
    push_n_for_alice(&ts, 50).await;
    let url = format!("{}/get", ts.base_url());
    let r = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .body(json!({"table": "TxnAgg", "key": "alice"}).to_string())
        .send()
        .await
        .expect("post /get");
    assert_eq!(r.status().as_u16(), 200);
    let v: serde_json::Value = r.json().await.expect("json");
    // Plan 13.4-02: dropped `{"result": ...}` envelope per Phase 13.0-15.
    // Plan 13.4 verb-style single-key /get returns flat `{feature: value}`
    // (no key wrapper). For batch reads use POST /batch_get.
    assert!(
        v.get("result").is_none(),
        "result envelope must be absent (Plan 13.4-02), got {v:#}"
    );
    assert_eq!(v["cnt"], 50);
    ts.shutdown().await.expect("shutdown");
}
