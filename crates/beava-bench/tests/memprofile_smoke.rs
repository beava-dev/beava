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
    assert!(report.contains("## Sorted Op Table"));
    assert!(report.contains("## Top 5 Offenders"));
    assert!(report.contains("## Metrics Coherence"));
    assert!(report.contains("Aggregate features discovered: `111`"));
    assert!(report.contains("| Rank | Op | Shape |"));
    assert!(report.contains("`windowed`"));
    assert!(report.contains("`lifetime`"));
    assert!(report.contains("- Breakdown rollup:"));
    assert!(report.contains("Windowed wrapper overhead"));
    assert!(!report.contains("CountDistinct owned internals"));
}
