//! CLI smoke tests — exercise the compiled `beava` binary end-to-end.
//!
//! Tests that start the server binary (valid config path) spawn a child process,
//! wait for the banner line to appear on stdout, then send SIGTERM. Port 0 is not
//! available via CLI flag (the flag takes a full address), so we use OS-allocated
//! ports from the ephemeral range and accept a brief startup race.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::NamedTempFile;

/// Plan 12.6-15: serialize tests that spawn the full beava binary —
/// concurrent subprocess boots stress macOS launch-budget under default
/// cargo-test parallelism, leading to flaky assertions on the override-port
/// bind window.
static CLI_SUBPROCESS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn beava_bin() -> &'static str {
    env!("CARGO_BIN_EXE_beava")
}

#[test]
fn help_exits_zero_and_mentions_config_flag() {
    let out = Command::new(beava_bin())
        .arg("--help")
        .output()
        .expect("spawn beava --help");
    assert!(
        out.status.success(),
        "--help should exit 0, got {:?}",
        out.status
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--config"), "stdout: {stdout}");
}

#[test]
fn version_flag_works() {
    let out = Command::new(beava_bin())
        .arg("--version")
        .output()
        .expect("spawn beava --version");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // CARGO_PKG_VERSION resolves at compile time to the workspace version
    // in Cargo.toml — keeps this assertion in lockstep with version bumps
    // without manual edits.
    let expected = env!("CARGO_PKG_VERSION");
    assert!(
        stdout.contains(expected),
        "expected version {expected} in stdout: {stdout}"
    );
}

/// Allocate a free TCP port by binding briefly then releasing it.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("local_addr").port()
}

/// Per-test unique WAL directory so parallel cli_smoke tests don't collide on
/// the default `./beava-wal` path (Phase 6 Plan 03).
fn unique_wal_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(1);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("beava-cli-smoke-wal-{}-{n}", std::process::id()))
}

/// Per-test unique snapshot directory (Phase 7 Plan 03).
fn unique_snapshot_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(1);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("beava-cli-smoke-snap-{}-{n}", std::process::id()))
}

#[test]
fn loads_valid_config_starts_and_prints_banner() {
    let _guard = CLI_SUBPROCESS_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let port = free_port();
    let mut f = NamedTempFile::new().expect("tempfile");
    writeln!(f, "listen_addr: \"127.0.0.1:{port}\"\nlog_level: info").unwrap();

    let wal_dir = unique_wal_dir();
    let snap_dir = unique_snapshot_dir();
    let child = Command::new(beava_bin())
        // Disable TCP wire listener for CLI smoke tests — avoids port conflicts
        // when multiple cli_smoke tests run in parallel and default-bind TCP 8081.
        .env("BEAVA_TCP_ENABLED", "0")
        .env("BEAVA_WAL_DIR", &wal_dir)
        .env("BEAVA_SNAPSHOT_DIR", &snap_dir)
        .arg("--config")
        .arg(f.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server");

    // Give the server a moment to start and print the banner line.
    std::thread::sleep(Duration::from_millis(300));

    // Send SIGTERM to trigger graceful shutdown.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }

    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("beava v"),
        "expected banner in stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn missing_config_errors_with_path_in_message() {
    let out = Command::new(beava_bin())
        .arg("--config")
        .arg("/nonexistent/beava.yaml")
        .output()
        .expect("spawn");
    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("/nonexistent/beava.yaml"),
        "stderr should name the path: {stderr}"
    );
}

/// `beava` (no `--config`) MUST NOT silently load a `beava.yaml` that
/// happens to sit in the current working directory. The implicit lookup
/// was a footgun: new users running `beava` from a directory that
/// happened to contain an unrelated `beava.yaml` got bound to whatever
/// that file said instead of the announced built-in defaults — and the
/// quickstart docs say "no config required" while the binary
/// contradicted them.
///
/// Resolution order is now:
///   1. `--config <path>`  (explicit; fail if missing)
///   2. Built-in defaults + `BEAVA_*` env-var overrides
///
/// Anyone who actually wants a YAML must point at it explicitly with
/// `-c` / `--config`. The boot banner's `config source` line documents
/// which branch was taken.
#[test]
fn no_args_ignores_beava_yaml_in_cwd() {
    let _guard = CLI_SUBPROCESS_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());

    // Build a tempdir with a `beava.yaml` containing a distinctive port —
    // if the binary silently picks it up, the boot banner will name that
    // port. Defaults bind on 127.0.0.1:8080.
    let tmp = tempfile::tempdir().expect("tempdir");
    let yaml_path = tmp.path().join("beava.yaml");
    std::fs::write(
        &yaml_path,
        "listen_addr: \"127.0.0.1:19741\"\nlog_level: info\n",
    )
    .expect("write beava.yaml");

    // Override admin + WAL + snapshot dirs via env so this test doesn't
    // collide with the default 8090 / ./beava-wal paths used by other
    // tests, and disable TCP wire to avoid port-8081 conflicts.
    let wal_dir = unique_wal_dir();
    let snap_dir = unique_snapshot_dir();
    let child = Command::new(beava_bin())
        .current_dir(tmp.path())
        // Force everything else off-default so this test can isolate the
        // implicit-yaml lookup as the only signal under inspection.
        .env("BEAVA_LISTEN_ADDR", "127.0.0.1:0")
        .env("BEAVA_TCP_ENABLED", "0")
        .env("BEAVA_ADMIN_ADDR", "127.0.0.1:0")
        .env("BEAVA_WAL_DIR", &wal_dir)
        .env("BEAVA_SNAPSHOT_DIR", &snap_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server");

    // Give the server long enough to print its boot banner.
    std::thread::sleep(Duration::from_millis(500));
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Must NOT load the CWD's beava.yaml — the distinctive port from the
    // tempdir's yaml must not appear in the boot banner.
    assert!(
        !stdout.contains("19741"),
        "beava silently loaded ./beava.yaml from CWD; expected built-in \
         defaults, got banner mentioning yaml port 19741:\n{stdout}"
    );
    // Boot banner's source label must announce defaults, not the
    // implicit yaml.
    assert!(
        stdout.contains("config source : built-in defaults"),
        "expected banner to announce built-in defaults; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("config source : ./beava.yaml"),
        "banner must not announce ./beava.yaml as the source; got:\n{stdout}"
    );
}

#[test]
fn env_var_overrides_listen_addr() {
    let _guard = CLI_SUBPROCESS_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let port = free_port();
    let override_port = free_port();
    let mut f = NamedTempFile::new().expect("tempfile");
    writeln!(f, "listen_addr: \"127.0.0.1:{port}\"\nlog_level: info").unwrap();

    let wal_dir = unique_wal_dir();
    let snap_dir = unique_snapshot_dir();
    let child = Command::new(beava_bin())
        .env("BEAVA_LISTEN_ADDR", format!("127.0.0.1:{override_port}"))
        // Disable TCP wire listener — avoids port conflicts across parallel tests.
        .env("BEAVA_TCP_ENABLED", "0")
        // Phase 13.5.1 Plan 07b: pin admin sidecar to ephemeral port so
        // parallel cli_smoke / pytest spawns don't fight over default 8090.
        .env("BEAVA_ADMIN_ADDR", "127.0.0.1:0")
        .env("BEAVA_WAL_DIR", &wal_dir)
        .env("BEAVA_SNAPSHOT_DIR", &snap_dir)
        .arg("--config")
        .arg(f.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server");

    // Plan 12.6-15: poll-until-bind instead of fixed 300 ms sleep. macOS
    // launch jitter under parallel cargo-test workloads can push first-bind
    // beyond 300 ms — the original test was flaky 1/3 runs.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut is_bound = false;
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect(format!("127.0.0.1:{override_port}")).is_ok() {
            is_bound = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait_with_output();

    assert!(
        is_bound,
        "expected server to bind on override port {override_port}"
    );
}

/// F5 end-to-end: `beava --http-addr ADDR --tcp-addr ADDR --memory-only --test-mode`
/// boots without a YAML, binds where the CLI says, and lets /reset succeed.
///
/// This is the canonical "user copy-pastes the README CLI section into their
/// shell" path. It exercises every locked v0 flag at once:
///   - `--http-addr` and `--tcp-addr` win over the default 8080/8081 ports.
///   - `--memory-only` skips the WAL writer (otherwise the default
///     `./beava-wal` dir would collide across parallel tests).
///   - `--test-mode` opens the destructive `/reset` endpoint, which
///     returns 200 only when test_mode is on.
#[test]
fn cli_flags_boot_and_dispatch_reset() {
    let _guard = CLI_SUBPROCESS_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());

    let http_port = free_port();
    let tcp_port = free_port();
    let admin_port = free_port();

    let child = Command::new(beava_bin())
        // Pin admin to an ephemeral port so parallel test spawns don't
        // fight on default 8090; admin isn't part of the F5 surface, just
        // an environmental knob.
        .env("BEAVA_ADMIN_ADDR", format!("127.0.0.1:{admin_port}"))
        .arg("--http-addr")
        .arg(format!("127.0.0.1:{http_port}"))
        .arg("--tcp-addr")
        .arg(format!("127.0.0.1:{tcp_port}"))
        .arg("--memory-only")
        .arg("--test-mode")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server");

    // Poll-until-bind on the HTTP override port.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut is_bound = false;
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect(format!("127.0.0.1:{http_port}")).is_ok() {
            is_bound = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        is_bound,
        "expected /ping listener bound on --http-addr 127.0.0.1:{http_port}"
    );

    // Hit /reset — it returns 403 unless --test-mode is honoured. The
    // body shape is `{"reset": true, "registry_version": <n>}` from
    // server.rs::format_reset_response. cli_smoke is std-only (no tokio
    // reactor in scope), so write a minimal HTTP/1.1 request by hand.
    use std::io::{Read, Write};
    let mut stream =
        std::net::TcpStream::connect(format!("127.0.0.1:{http_port}")).expect("connect /reset");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    let req = "POST /reset HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
    stream.write_all(req.as_bytes()).expect("write /reset");
    // Read in chunks until we see the body close-tag `}` after the
    // headers — the server keeps the connection open even with
    // Connection: close, so a naive read_to_string blocks. The reset
    // response is ~50 bytes; reading 1 KB into a buffer is enough.
    let mut raw = Vec::with_capacity(1024);
    let mut chunk = [0u8; 256];
    while let Ok(n) = stream.read(&mut chunk) {
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..n]);
        let raw_str = String::from_utf8_lossy(&raw);
        if let Some(body_start) = raw_str.find("\r\n\r\n") {
            let after_headers = &raw_str[body_start + 4..];
            if after_headers.contains('}') {
                break;
            }
        }
    }
    let raw_str = String::from_utf8_lossy(&raw).to_string();
    let status_line = raw_str.lines().next().unwrap_or("");
    let body = raw_str.split("\r\n\r\n").nth(1).unwrap_or("").to_string();

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait_with_output();

    assert!(
        status_line.contains("200"),
        "/reset must return 200 with --test-mode; got status_line={status_line:?} body={body}"
    );
    assert!(
        body.contains("\"reset\":true") || body.contains("\"reset\": true"),
        "/reset body must contain reset:true; got {body}"
    );
}
