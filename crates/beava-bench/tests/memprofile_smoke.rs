use assert_cmd::Command;

#[test]
fn memprofile_smoke_writes_required_sections() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = temp.path().join("memory-profile-fraud-team.md");

    Command::cargo_bin("memprofile")
        .expect("memprofile binary")
        .args([
            "--workload",
            "fraud",
            "--events",
            "50",
            "--output",
            output.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();

    let report = std::fs::read_to_string(output).expect("read report");
    assert!(report.contains("# AggOp Memory Profile: fraud-team"));
    assert!(report.contains("Events requested from generator: `50`"));
    assert!(report.contains("Events replayed from generator: `50`"));
    assert!(report.contains("  - `Txn`: `50`"));
    assert!(!report.contains("Events replayed per op"));
    assert!(report.contains("## Sorted Op Table"));
    assert!(report.contains("## Sorted Op Path Details"));
    assert!(report.contains("## Top 5 Offenders"));
    assert!(report.contains("## Metrics Coherence"));
    assert!(report.contains("Aggregate features discovered: `111`"));
    assert!(report.contains("| Rank | Op | Shape | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |"));
    assert!(report.contains("enum_slot_bytes"));
    assert!(report.contains("payload_bytes"));
    assert!(report.contains("slack_bytes"));
    assert!(report.contains(
        "| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes |"
    ));
    assert!(report.contains(
        "| `Txn` | `TxnByUser` | `txn_count_lifetime` | `user_id` | 50 | 80 | 80 | 8 | 72 | 0 | 80 |"
    ));
    assert!(
        report.contains("| `Login` | `LoginByUser` | `ips_distinct_login_1h` | `user_id` | 0 |")
    );
    assert!(report.contains("- Path: `Txn` ->"));
    assert!(report.contains("- Events applied: `50`"));
    assert!(report.contains("stack=80 (enum_slot_bytes=80 payload_bytes="));
    assert!(report.contains("`windowed`"));
    assert!(report.contains("`lifetime`"));
    assert!(report.contains("- Breakdown rollup:"));
    assert!(report.contains("Windowed wrapper overhead"));
    assert!(report.contains("`slack_bytes` is unused capacity in the fixed-size `AggOp` enum slot"));
    assert!(!report.contains("CountDistinct owned internals"));
    assert!(!report.contains("enum overhead"));
}
