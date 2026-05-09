//! End-to-end JSON logging format verification.
//!
//! Runs the `log_probe` helper binary as a subprocess, captures its stdout, and
//! asserts every line is valid JSON with the required fields (timestamp, level,
//! target, message / fields flattened at top level).
//!
//! Uses serde_json only in dev-deps for parsing; production dep graph unaffected.

use std::process::Command;

fn log_probe_bin() -> &'static str {
    env!("CARGO_BIN_EXE_log_probe")
}

#[test]
fn probe_emits_valid_json_per_line() {
    let out = Command::new(log_probe_bin())
        .output()
        .expect("spawn log_probe");
    assert!(
        out.status.success(),
        "log_probe exit failed: {:?}",
        out.status
    );

    // tracing-subscriber's JSON formatter writes to stdout by default. Some
    // setups write to stderr; accept either.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let mut parsed_lines = 0usize;
    let mut saw_info = false;
    let mut saw_warn = false;
    let mut saw_error = false;

    for line in combined.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(trimmed)
            .unwrap_or_else(|e| panic!("not valid JSON: {trimmed}\nerror: {e}"));

        // Required fields per our formatter config (flatten_event = true pulls
        // the `message` to top-level; timestamp/level/target always present).
        assert!(v.get("timestamp").is_some(), "no timestamp in: {trimmed}");
        let level = v.get("level").and_then(|x| x.as_str()).unwrap_or("");
        let target = v.get("target").and_then(|x| x.as_str()).unwrap_or("");
        assert!(target.starts_with("beava."), "bad target in: {trimmed}");

        match level {
            "INFO" => saw_info = true,
            "WARN" => saw_warn = true,
            "ERROR" => saw_error = true,
            _ => {}
        }
        parsed_lines += 1;
    }

    assert!(
        parsed_lines >= 3,
        "expected ≥3 JSON log lines, got {parsed_lines}\n{combined}"
    );
    assert!(saw_info, "no INFO event in: {combined}");
    assert!(saw_warn, "no WARN event in: {combined}");
    assert!(saw_error, "no ERROR event in: {combined}");
}

#[test]
fn probe_stdout_has_structured_field() {
    let out = Command::new(log_probe_bin()).output().expect("spawn");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // Look for the structured `version` field on the info event. The expected
    // value is the workspace's CARGO_PKG_VERSION at compile time so this test
    // stays in lockstep with version bumps without manual edits.
    let expected = env!("CARGO_PKG_VERSION");
    let mut found = false;
    for line in combined.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if v.get("version").and_then(|x| x.as_str()) == Some(expected) {
                found = true;
                break;
            }
        }
    }
    assert!(
        found,
        "no structured `version={expected}` field found in:\n{combined}"
    );
}
