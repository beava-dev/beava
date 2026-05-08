//! Smoke test for the `beava quickstart` subcommand (1a).
//!
//! Spawns the compiled `beava` binary with the `quickstart` subcommand
//! and asserts the 4-step contract:
//!  - exit 0
//!  - stdout contains all four `[1/4]`..`[4/4]` markers
//!  - drop-file `beava_quickstart.py` is written to CWD by default
//!  - drop-file is NOT written when `--no-file` is set
//!
//! Each test runs in a fresh `tempfile::tempdir()` set as cwd so the
//! drop-file lands in test-isolated space (and so concurrent test
//! runs don't fight over `./beava_quickstart.py`).

use std::process::{Command, Stdio};
use std::time::Duration;

fn beava_bin() -> &'static str {
    env!("CARGO_BIN_EXE_beava")
}

/// Loose contract pinning what `beava quickstart` MUST emit on stdout.
/// The exact formatting is unit-tested in `quickstart.rs::tests`; here
/// we just guarantee the binary boots, runs the demo, and exits cleanly.
#[test]
fn quickstart_runs_to_completion_and_prints_four_step_markers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // BEAVA_TCP_ENABLED=0 keeps the in-process server off TCP-8081 to
    // avoid colliding with a server already running on the dev box.
    let output = Command::new(beava_bin())
        .arg("quickstart")
        .arg("--no-file")
        .current_dir(tmp.path())
        .env("BEAVA_TCP_ENABLED", "0")
        // Ephemeral admin port — default 8090 collides with `beava` running
        // in another terminal on a dev box.
        .env("BEAVA_ADMIN_ADDR", "127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Long-ish timeout: in-process server bind + register + 5 pushes
        // + get is sub-second, but slow CI machines need slack.
        .output()
        .expect("spawn beava quickstart");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "expected exit 0; got {:?}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        output.status
    );

    for marker in ["[1/4]", "[2/4]", "[3/4]", "[4/4]"] {
        assert!(
            stdout.contains(marker),
            "stdout must contain {marker} marker:\n{stdout}"
        );
    }
}

/// Default behaviour: `beava quickstart` (no flags) writes
/// `beava_quickstart.py` to the CWD as the bridge from sandbox to a
/// real `beava` server.
#[test]
fn quickstart_writes_drop_file_in_cwd_by_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let output = Command::new(beava_bin())
        .arg("quickstart")
        .current_dir(tmp.path())
        .env("BEAVA_TCP_ENABLED", "0")
        .env("BEAVA_ADMIN_ADDR", "127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn beava quickstart");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected exit 0; got {:?}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        output.status
    );

    let drop_file = tmp.path().join("beava_quickstart.py");
    assert!(
        drop_file.exists(),
        "expected beava_quickstart.py to be written; cwd: {}",
        tmp.path().display()
    );
    let body = std::fs::read_to_string(&drop_file).expect("read drop file");
    assert!(
        body.contains("@bv.event"),
        "drop file must contain @bv.event"
    );
    assert!(
        body.contains("PageView"),
        "drop file must define PageView event"
    );
    assert!(
        body.contains("SiteMetrics"),
        "drop file must define SiteMetrics table"
    );
}

/// `--no-file` opt-out for CI / docker-exec runs where leaving a
/// file behind is noise.
#[test]
fn quickstart_no_file_flag_skips_drop_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let output = Command::new(beava_bin())
        .arg("quickstart")
        .arg("--no-file")
        .current_dir(tmp.path())
        .env("BEAVA_TCP_ENABLED", "0")
        .env("BEAVA_ADMIN_ADDR", "127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn beava quickstart --no-file");

    assert!(output.status.success(), "expected exit 0");

    let drop_file = tmp.path().join("beava_quickstart.py");
    assert!(
        !drop_file.exists(),
        "expected beava_quickstart.py NOT to be written when --no-file is set"
    );
}

/// Sanity: `--no-file` should not slow the demo to a crawl. If the
/// quickstart hangs (e.g. server bind raced with apply-thread spawn),
/// we want to know loudly rather than have CI time out at 60s.
#[test]
fn quickstart_completes_quickly_under_no_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let start = std::time::Instant::now();
    let output = Command::new(beava_bin())
        .arg("quickstart")
        .arg("--no-file")
        .current_dir(tmp.path())
        .env("BEAVA_TCP_ENABLED", "0")
        .env("BEAVA_ADMIN_ADDR", "127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn beava quickstart");
    let elapsed = start.elapsed();
    assert!(output.status.success(), "expected exit 0");
    assert!(
        elapsed < Duration::from_secs(15),
        "quickstart took {:?} — should complete in under 15s on any reasonable machine",
        elapsed
    );
}
