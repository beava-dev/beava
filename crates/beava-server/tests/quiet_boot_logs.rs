//! Boot-log quietness contract.
//!
//! At default level (INFO), `beava` must emit only the records an operator
//! needs to see at startup: "I started", the bind addresses, and shutdown.
//! Anything else (recovery internals, WAL config, worker pool fan-out, apply
//! thread lifecycle) belongs at DEBUG so it stays out of operator stdout
//! unless they opt in via `RUST_LOG=debug`.
//!
//! The whitelist:
//!   - target=beava.server, message="beava starting"
//!   - target=beava.server, kind="server.http_bound"
//!   - target=beava.server, kind="server.tcp_bound"
//!   - target=beava.shutdown, message="shutdown initiated"
//!
//! Adding a new INFO log site in the boot or shutdown path will fail this
//! test — by design. If the new site is genuinely operator-relevant, add it
//! to the whitelist with rationale; otherwise emit at DEBUG.

use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Serialize against other CLI smoke tests that spawn the full beava binary
/// (cli_smoke.rs uses the same pattern). Concurrent subprocess boots stress
/// macOS launch budget under default cargo-test parallelism, leading to
/// flaky bind-window assertions.
static CLI_SUBPROCESS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn beava_bin() -> &'static str {
    env!("CARGO_BIN_EXE_beava")
}

fn unique_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(1);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("beava-quiet-boot-{tag}-{}-{n}", std::process::id()))
}

#[derive(Debug, Clone)]
struct InfoRecord {
    target: String,
    kind: Option<String>,
    message: Option<String>,
}

impl InfoRecord {
    fn tag(&self) -> String {
        match (&self.kind, &self.message) {
            (Some(k), _) => format!("{}::{}", self.target, k),
            (None, Some(m)) => format!("{}::msg={}", self.target, m),
            (None, None) => self.target.clone(),
        }
    }
}

fn collect_info_records(stdout: &str) -> Vec<InfoRecord> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            // Skip the plain-text banner block (added in PR #11).
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let level = v.get("level").and_then(|x| x.as_str()).unwrap_or("");
        if level != "INFO" {
            continue;
        }
        let target = v
            .get("target")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let kind = v.get("kind").and_then(|x| x.as_str()).map(String::from);
        let message = v.get("message").and_then(|x| x.as_str()).map(String::from);
        out.push(InfoRecord {
            target,
            kind,
            message,
        });
    }
    out
}

fn whitelist() -> std::collections::BTreeSet<&'static str> {
    [
        "beava.server::msg=beava starting",
        "beava.server::server.http_bound",
        "beava.server::server.tcp_bound",
        "beava.shutdown::msg=shutdown initiated",
    ]
    .into_iter()
    .collect()
}

#[test]
fn boot_and_shutdown_emit_only_whitelisted_info_records() {
    let _guard = CLI_SUBPROCESS_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());

    let wal_dir = unique_dir("wal");
    let snap_dir = unique_dir("snap");

    // PR #11 made --config optional; no -c flag here exercises the
    // built-in-defaults boot path. Ephemeral ports for HTTP/TCP/admin so
    // parallel test spawns don't collide.
    let mut child = Command::new(beava_bin())
        .env("BEAVA_LISTEN_ADDR", "127.0.0.1:0")
        .env("BEAVA_TCP_PORT", "0")
        .env("BEAVA_ADMIN_ADDR", "127.0.0.1:0")
        .env("BEAVA_WAL_DIR", &wal_dir)
        .env("BEAVA_SNAPSHOT_DIR", &snap_dir)
        // Make sure RUST_LOG isn't inherited from the dev shell — that
        // would override the default INFO level we're testing.
        .env_remove("RUST_LOG")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn beava");

    let stdout = child.stdout.take().expect("child stdout");
    let reader = BufReader::new(stdout);
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let lines_for_thread = lines.clone();
    let reader_handle = std::thread::spawn(move || {
        for line in reader.lines().map_while(Result::ok) {
            lines_for_thread.lock().unwrap().push(line);
        }
    });

    // Wait until both bind events appear — that's "boot complete".
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen_http = false;
    let mut seen_tcp = false;
    while Instant::now() < deadline && !(seen_http && seen_tcp) {
        std::thread::sleep(Duration::from_millis(40));
        let snap = lines.lock().unwrap().clone();
        for raw in &snap {
            if raw.contains("server.http_bound") {
                seen_http = true;
            }
            if raw.contains("server.tcp_bound") {
                seen_tcp = true;
            }
        }
    }
    assert!(
        seen_http && seen_tcp,
        "boot did not emit both bind events within 5s; got lines:\n{}",
        lines.lock().unwrap().join("\n")
    );

    // Give the apply thread a beat to enter steady-state — any INFO emitted
    // *after* binding (e.g. workers.started, dispatcher loop started)
    // should appear here so the test catches it.
    std::thread::sleep(Duration::from_millis(200));

    // Trigger graceful shutdown so the shutdown-path INFO sites get a chance
    // to fire too. Apply-thread shutdown logs (server.rs:1860) used to fire
    // here at INFO; the contract says they belong at DEBUG.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }

    // Collect the rest of stdout.
    let _ = child.wait().expect("wait");
    let _ = reader_handle.join();

    let stdout_buf = lines.lock().unwrap().join("\n");
    let info_records = collect_info_records(&stdout_buf);

    let allowed = whitelist();
    let mut violations = Vec::new();
    for rec in &info_records {
        let tag = rec.tag();
        if !allowed.contains(tag.as_str()) {
            violations.push(format!(
                "  - {} (target={}, kind={:?}, message={:?})",
                tag, rec.target, rec.kind, rec.message
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "Boot/shutdown emitted {} INFO record(s) outside the whitelist:\n{}\n\n\
         Whitelist (allowed at INFO):\n{}\n\n\
         Full stdout:\n{}",
        violations.len(),
        violations.join("\n"),
        allowed
            .iter()
            .map(|s| format!("  - {s}"))
            .collect::<Vec<_>>()
            .join("\n"),
        stdout_buf,
    );
}
