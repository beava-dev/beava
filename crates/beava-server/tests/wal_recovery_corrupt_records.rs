//! Recovery resilience: corrupt records in a hand-rolled v=2 `*.wal` file
//! must be skipped (warn + continue), not crash boot. Covers the
//! `recovery.v2_msgpack_decode_failed` and `recovery.v2_json_decode_failed`
//! warn-and-skip arms in
//! `crates/beava-server/src/recovery.rs::replay_handrolled_wal_dir`.
//!
//! Wire format (verbatim from apply_shard.rs:992-1011 and recovery.rs:144-145):
//!   `[u8 v=2][u8 body_format][u32 rv BE][u64 et_ms BE]
//!    [u16 name_len BE][N bytes name][u32 body_len BE][M bytes body]`
//!
//! The parser stops at the first byte that is not `0x02` (treated as EOF)
//! or on truncation. Decode failures on the body are warn-skipped per-record
//! so a single corrupt record doesn't drop the rest of the file.

#![cfg(feature = "testing")]

use beava_server::testing::TestServerBuilder;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Encode one v=2 record into a buffer.
///
/// `body_format` selects the on-disk encoding: `0x02 = CT_JSON` (server
/// default for HTTP /push), `0x01 = CT_MSGPACK`.
fn encode_v2_record(
    buf: &mut Vec<u8>,
    body_format: u8,
    rv: u32,
    et_ms: i64,
    event_name: &str,
    body: &[u8],
) {
    buf.push(0x02); // version
    buf.push(body_format);
    buf.extend_from_slice(&rv.to_be_bytes());
    buf.extend_from_slice(&et_ms.to_be_bytes());
    let name_bytes = event_name.as_bytes();
    buf.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
    buf.extend_from_slice(name_bytes);
    buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
    buf.extend_from_slice(body);
}

/// Write a hand-rolled WAL file to `dir/wal-0000000000000000.wal`
/// containing the given byte payload.
fn write_wal_file(dir: &Path, bytes: &[u8]) {
    fs::create_dir_all(dir).expect("mkdir wal dir");
    let path = dir.join("wal-0000000000000000.wal");
    let mut f = fs::File::create(&path).expect("create wal file");
    f.write_all(bytes).expect("write wal bytes");
    f.sync_all().expect("fsync wal");
}

/// Build a WAL file with: 1 valid CT_JSON record followed by 1 CORRUPT
/// CT_JSON record (body_len declared but body is not valid JSON). Parser
/// must read both record HEADERS successfully (so it advances `pos` past
/// the corrupt body) and then warn-skip the corrupt body via the
/// `serde_json::from_slice` failure arm.
fn build_wal_with_one_valid_and_one_corrupt_json() -> Vec<u8> {
    const CT_JSON: u8 = 0x02;
    let mut buf = Vec::new();

    // Record 1: valid CT_JSON body — `{"user_id":"alice","amount":1.0}`.
    let valid_body = br#"{"user_id":"alice","amount":1.0}"#;
    encode_v2_record(&mut buf, CT_JSON, 1, 1_700_000_000_000, "Txn", valid_body);

    // Record 2: CORRUPT CT_JSON body — declares body_len=5 but body is
    // `xxxxx` (not valid JSON). The header parses fine; the
    // `serde_json::from_slice::<Row>(&rec.body)` call fails; recovery
    // logs a `recovery.v2_json_decode_failed` warn and continues.
    let corrupt_body: &[u8] = b"xxxxx";
    encode_v2_record(&mut buf, CT_JSON, 1, 1_700_000_000_001, "Txn", corrupt_body);

    buf
}

/// Build a WAL file with: 1 valid CT_JSON record + 1 CORRUPT CT_MSGPACK
/// record. Exercises the `recovery.v2_msgpack_decode_failed` arm
/// (recovery.rs:251-260).
fn build_wal_with_corrupt_msgpack_record() -> Vec<u8> {
    const CT_JSON: u8 = 0x02;
    const CT_MSGPACK: u8 = 0x01;
    let mut buf = Vec::new();

    // Record 1: valid CT_JSON.
    let valid_body = br#"{"user_id":"alice","amount":1.0}"#;
    encode_v2_record(&mut buf, CT_JSON, 1, 1_700_000_000_000, "Txn", valid_body);

    // Record 2: CORRUPT CT_MSGPACK body — `rmp_serde` rejects arbitrary
    // bytes. 5 random bytes form a syntactically broken msgpack stream.
    let corrupt_msgpack: &[u8] = &[0xff, 0xfe, 0xfd, 0xfc, 0xfb];
    encode_v2_record(
        &mut buf,
        CT_MSGPACK,
        1,
        1_700_000_000_001,
        "Txn",
        corrupt_msgpack,
    );

    buf
}

/// Boot a TestServer pointed at a wal_dir that contains a v=2 WAL file
/// with one valid record + one CORRUPT CT_JSON record. The corrupt
/// record's body fails `serde_json::from_slice::<Row>`; recovery must
/// warn-and-skip (NOT panic) and the server must boot cleanly.
#[tokio::test]
async fn boot_with_corrupt_json_record_skips_and_recovers() {
    let wal = tempfile::tempdir().unwrap();
    let snap = tempfile::tempdir().unwrap();

    let wal_bytes = build_wal_with_one_valid_and_one_corrupt_json();
    write_wal_file(wal.path(), &wal_bytes);

    let ts = TestServerBuilder::new()
        .dev_endpoints(true)
        .wal_dir(wal.path().to_path_buf())
        .snapshot_dir(snap.path().to_path_buf())
        .fsync_interval_ms(1)
        .spawn()
        .await
        .expect("server must boot even with a corrupt WAL record");

    // Boot succeeded. Verify the server is live and responsive — proves
    // the corrupt record was warn-skipped, not panicked-on.
    let url = format!("{}/health", ts.base_url());
    let r = reqwest::get(&url).await.expect("GET /health");
    assert_eq!(
        r.status().as_u16(),
        200,
        "/health must be 200 after a corrupt-record recovery"
    );

    // The valid record was for the event "Txn"; that event isn't
    // registered yet in this boot, so the replay applied it to "no
    // aggregations" (no-op). After register + push, /get must return
    // a clean cnt=1 — confirming neither the corrupt nor valid replay
    // record poisoned the new registry/state.
    let reg_payload = serde_json::json!({"nodes": [
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
    let resp = ts
        .post_json("/register", &reg_payload)
        .await
        .expect("register");
    assert_eq!(
        resp.status().as_u16(),
        200,
        "register must succeed post-recovery"
    );

    let push_body =
        serde_json::json!({"user_id": "alice", "amount": 1.0, "event_time": 2_000_000_i64});
    let resp = ts.post_json("/push/Txn", &push_body).await.expect("push");
    assert_eq!(resp.status().as_u16(), 200);

    let get_url = format!("{}/get", ts.base_url());
    let r = reqwest::Client::new()
        .post(&get_url)
        .header("Content-Type", "application/json")
        .body(
            serde_json::json!({"table": "TxnAgg", "key": "alice", "features": ["cnt"]}).to_string(),
        )
        .send()
        .await
        .expect("POST /get");
    assert_eq!(r.status().as_u16(), 200);
    let body: serde_json::Value = r.json().await.expect("json body");
    assert_eq!(
        body["cnt"], 1,
        "post-recovery state must show exactly 1 event (the post-recovery push); \
         got body={body}"
    );

    ts.shutdown().await.expect("clean shutdown");
}

/// Sister test: corrupt CT_MSGPACK record. Same warn-and-skip contract,
/// different arm (`recovery.v2_msgpack_decode_failed`).
#[tokio::test]
async fn boot_with_corrupt_msgpack_record_skips_and_recovers() {
    let wal = tempfile::tempdir().unwrap();
    let snap = tempfile::tempdir().unwrap();

    let wal_bytes = build_wal_with_corrupt_msgpack_record();
    write_wal_file(wal.path(), &wal_bytes);

    let ts = TestServerBuilder::new()
        .dev_endpoints(true)
        .wal_dir(wal.path().to_path_buf())
        .snapshot_dir(snap.path().to_path_buf())
        .fsync_interval_ms(1)
        .spawn()
        .await
        .expect("server must boot even with a corrupt msgpack WAL record");

    // Boot succeeded → corrupt msgpack record was warn-skipped.
    let url = format!("{}/health", ts.base_url());
    let r = reqwest::get(&url).await.expect("GET /health");
    assert_eq!(r.status().as_u16(), 200);

    ts.shutdown().await.expect("clean shutdown");
}

/// Boot with a WAL file containing only a corrupt record. Recovery must
/// still succeed (replay_event_count = 0) and the server must boot.
/// Verifies that an entirely-corrupt WAL doesn't block startup — the
/// per-record warn-skip handles 100%-bad files just like it handles
/// partially-bad ones.
#[tokio::test]
async fn boot_with_only_corrupt_record_still_boots() {
    let wal = tempfile::tempdir().unwrap();
    let snap = tempfile::tempdir().unwrap();

    // Single CORRUPT CT_JSON record — declared body_len=5 but body is
    // not valid JSON.
    const CT_JSON: u8 = 0x02;
    let mut buf = Vec::new();
    encode_v2_record(&mut buf, CT_JSON, 1, 1_700_000_000_000, "Txn", b"xxxxx");
    write_wal_file(wal.path(), &buf);

    let ts = TestServerBuilder::new()
        .dev_endpoints(true)
        .wal_dir(wal.path().to_path_buf())
        .snapshot_dir(snap.path().to_path_buf())
        .fsync_interval_ms(1)
        .spawn()
        .await
        .expect("server must boot even with an all-corrupt WAL");

    let url = format!("{}/health", ts.base_url());
    let r = reqwest::get(&url).await.expect("GET /health");
    assert_eq!(r.status().as_u16(), 200);

    ts.shutdown().await.expect("clean shutdown");
}

/// Boot with an empty WAL dir — exercises the "no .wal files at all"
/// shortcut in `replay_handrolled_wal_dir`. Belt-and-braces sanity check
/// that the read_dir-filter path doesn't trip on an empty dir.
#[tokio::test]
async fn boot_with_empty_wal_dir_recovers_cleanly() {
    let wal = tempfile::tempdir().unwrap();
    let snap = tempfile::tempdir().unwrap();

    let ts = TestServerBuilder::new()
        .dev_endpoints(true)
        .wal_dir(wal.path().to_path_buf())
        .snapshot_dir(snap.path().to_path_buf())
        .fsync_interval_ms(1)
        .spawn()
        .await
        .expect("server must boot with an empty wal dir");

    let url = format!("{}/health", ts.base_url());
    let r = reqwest::get(&url).await.expect("GET /health");
    assert_eq!(r.status().as_u16(), 200);

    ts.shutdown().await.expect("clean shutdown");
}
