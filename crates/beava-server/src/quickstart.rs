//! `beava quickstart` subcommand — magical first-touch demo.
//!
//! Spawns an in-process `ServerV18` on ephemeral ports, registers a
//! `PageView` → `SiteMetrics` pipeline (mirroring the homepage hero),
//! pushes 5 events, queries the global row, prints a 4-step formatted
//! walkthrough, and tears down. Optionally drops a `beava_quickstart.py`
//! file in the CWD that bridges the sandbox to a real `beava` server.
//!
//! Public API:
//! - [`run`] — top-level entry point used by `main.rs` when the
//!   `quickstart` subcommand is selected.
//! - [`format_walkthrough`] — render the 4-step output from a captured
//!   [`QuickstartResult`]. Pure function; unit-tested.
//! - [`write_drop_file_if_absent`] — write `beava_quickstart.py` if it
//!   doesn't already exist; never clobber user edits.
//! - [`QUICKSTART_PY`] — verbatim contents of the dropped file.

use anyhow::Context;
use std::fmt::Write as _;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::server::{ServerV18, ServerV18Config};
use beava_persistence::{Persistence, SyncMode};

/// Captured outcome of a quickstart run, fed into [`format_walkthrough`].
///
/// Pure-data struct so the formatter is a deterministic pure function
/// and can be unit-tested against fake values.
#[derive(Debug, Clone)]
pub struct QuickstartResult {
    /// Beava server semver, from `env!("CARGO_PKG_VERSION")`.
    pub server_version: String,
    /// HTTP listen address the in-process server bound (ephemeral port).
    pub bind_addr: SocketAddr,
    /// Wall-clock time from server-spawn to first `/ready` 200.
    pub ready_in: Duration,
    /// Registry version returned from `/register` (always 1 on a fresh
    /// quickstart run).
    pub registry_version: u64,
    /// `ack_lsn` values returned from each of the 5 pushes.
    pub ack_lsns: Vec<u64>,
    /// The 5 events that were pushed (used to render the [2/4] section).
    pub events: Vec<EventDisplay>,
    /// Pretty-printed JSON body returned from `POST /get`. Rendered
    /// verbatim under the [3/4] heading.
    pub get_response_pretty: String,
    /// Outcome of the drop-file step ([4/4]).
    pub drop_file_outcome: DropFileOutcome,
}

/// One displayed event from the [2/4] section.
#[derive(Debug, Clone)]
pub struct EventDisplay {
    pub session_id: String,
    pub path: String,
    pub dwell_ms: i64,
}

/// What happened to the `beava_quickstart.py` drop-file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropFileOutcome {
    /// Wrote the file to disk.
    Wrote(std::path::PathBuf),
    /// File already existed; left it alone (don't clobber user edits).
    SkippedAlreadyExists(std::path::PathBuf),
    /// `--no-file` was passed.
    SkippedNoFile,
}

/// Top-level entry point invoked by `main.rs` on `beava quickstart`.
///
/// Spawns an in-process server, runs the 4 steps, prints the walkthrough,
/// optionally drops `beava_quickstart.py` in the CWD, tears down. Returns
/// non-zero exit (via `anyhow::Error`) if any step fails.
pub fn run(no_file: bool) -> anyhow::Result<()> {
    // Per-process WAL + snapshot dirs under the OS temp dir so quickstart
    // never pollutes the CWD (which is where the drop-file lands). Same
    // pattern as `ServerV18::serve()`. We don't auto-clean — quickstart
    // is one-shot and the OS temp dir is the right cleanup boundary.
    let unique = format!(
        "beava-quickstart-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let wal_dir = std::env::temp_dir().join(format!("{unique}-wal"));
    let snap_dir = std::env::temp_dir().join(format!("{unique}-snap"));
    std::fs::create_dir_all(&wal_dir).context("create WAL temp dir")?;
    std::fs::create_dir_all(&snap_dir).context("create snapshot temp dir")?;

    // Single-thread tokio mirrors main.rs's runtime. The mio data plane
    // runs on its own thread inside `serve_with_dirs`.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    rt.block_on(async move {
        let started = Instant::now();

        // Bind on ephemeral ports so quickstart never collides with a
        // real `beava` already running on 8080/8081/8090. The HTTP
        // address the user sees in [1/4]/[2/4]/[3/4] is whatever the
        // OS assigned.
        let any: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let mut sv18_cfg = ServerV18Config::from_env();
        sv18_cfg.persistence = Persistence::Disk {
            wal_dir: wal_dir.clone(),
            snapshot_dir: snap_dir.clone(),
            sync_mode: SyncMode::Periodic,
        };

        let server = ServerV18::bind_with_config(any, Some(any), any, sv18_cfg)
            .await
            .context("bind in-process beava server")?;
        let bind_addr = server.http_addr();
        let admin_addr = server.admin_addr();

        // Spawn the serve loop on its own task; signal teardown via
        // a oneshot channel once the 4 steps complete.
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let serve_task = tokio::spawn(async move {
            server
                .serve(async {
                    let _ = shutdown_rx.await;
                })
                .await
        });

        // Wait until /ready on the admin port reports 200. The mio
        // apply thread is up by then.
        wait_ready(admin_addr, Duration::from_secs(10))
            .await
            .context("wait for /ready")?;
        let ready_in = started.elapsed();

        // [1/4] Register the pipeline.
        let registry_version = step_register(bind_addr)
            .await
            .context("step [1/4] register")?;

        // [2/4] Push 5 events.
        let events = canonical_events();
        let mut ack_lsns = Vec::with_capacity(events.len());
        for e in &events {
            let ack = step_push(bind_addr, e)
                .await
                .with_context(|| format!("step [2/4] push session_id={}", e.session_id))?;
            ack_lsns.push(ack);
        }

        // [3/4] Query the global row.
        let get_response_pretty = step_get(bind_addr).await.context("step [3/4] get")?;

        // [4/4] Drop the file before printing — so the walkthrough's
        // step [4/4] can announce the actual outcome.
        let cwd = std::env::current_dir().context("getcwd")?;
        let drop_file_outcome =
            write_drop_file_if_absent(&cwd, no_file).context("write beava_quickstart.py")?;

        let result = QuickstartResult {
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            bind_addr,
            ready_in,
            registry_version,
            ack_lsns,
            events,
            get_response_pretty,
            drop_file_outcome,
        };

        // Print the walkthrough as one block — keeps the unit-tested
        // formatter the single source of truth for what stdout looks
        // like, and avoids interleaving with the apply thread's
        // tracing output if it's still flushing.
        print!("{}", format_walkthrough(&result));

        // Tear down. Send shutdown, wait for the serve task to drain
        // (best-effort — quickstart's exit code reflects step errors,
        // not teardown errors).
        let _ = shutdown_tx.send(());
        let _ = tokio::time::timeout(Duration::from_secs(5), serve_task).await;

        Ok::<(), anyhow::Error>(())
    })
}

/// Render the 4-step walkthrough as a single String.
///
/// Pure function — given a [`QuickstartResult`], always produces the same
/// output.  Mirrors the homepage hero exactly: `@bv.event PageView`,
/// `@bv.table SiteMetrics` (no key=, global), feature names
/// `median_dwell_1h` / `page_views_today` / `top_page_1h`.
pub fn format_walkthrough(r: &QuickstartResult) -> String {
    let mut s = String::with_capacity(4096);

    // Header.
    writeln!(s, "beava quickstart · v{}", r.server_version).unwrap();
    writeln!(
        s,
        "═══════════════════════════════════════════════════════════════"
    )
    .unwrap();
    writeln!(s).unwrap();
    writeln!(
        s,
        "  Spinning up an in-process beava server on {}…",
        r.bind_addr
    )
    .unwrap();
    writeln!(s, "  ✓ ready in {:.2}s", r.ready_in.as_secs_f64()).unwrap();
    writeln!(s).unwrap();

    // [1/4] Define a feature
    writeln!(s, "[1/4] Define a feature").unwrap();
    writeln!(s, "───────────────────────").unwrap();
    writeln!(s, "  @bv.event").unwrap();
    writeln!(s, "  class PageView:").unwrap();
    writeln!(s, "      session_id: str").unwrap();
    writeln!(s, "      path: str").unwrap();
    writeln!(s, "      dwell_ms: int").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "  @bv.table   # no key= → one row, site-wide").unwrap();
    writeln!(s, "  def SiteMetrics(e: PageView):").unwrap();
    writeln!(s, "      return e.agg(").unwrap();
    writeln!(
        s,
        "          median_dwell_1h  = bv.quantile(\"dwell_ms\", q=0.5, window=\"1h\"),"
    )
    .unwrap();
    writeln!(s, "          page_views_today = bv.count(window=\"24h\"),").unwrap();
    writeln!(
        s,
        "          top_page_1h      = bv.top_k(\"path\", k=1, window=\"1h\"),"
    )
    .unwrap();
    writeln!(s, "      )").unwrap();
    writeln!(s).unwrap();
    writeln!(
        s,
        "  POST /register → 201 (registry_version={})",
        r.registry_version
    )
    .unwrap();
    writeln!(s).unwrap();

    // [2/4] Push 5 events
    writeln!(s, "[2/4] Push 5 events").unwrap();
    writeln!(s, "───────────────────").unwrap();
    // Pad the path column so the dwell_ms and ack_lsn columns line up
    // even when paths vary in length ("/", "/pricing", "/docs", …).
    // 10 chars covers the longest canonical path "/pricing" (8) + 2
    // chars of slack for surrounding quotes.
    let max_path_w = r.events.iter().map(|e| e.path.len()).max().unwrap_or(8);
    for (i, e) in r.events.iter().enumerate() {
        let ack = r.ack_lsns.get(i).copied().unwrap_or(0);
        // Right-align dwell_ms to 4 chars (covers 0..9999 in i64).
        writeln!(
            s,
            "  POST /push  {{event:\"PageView\", data:{{session_id:\"{}\", path:{:<width$} dwell_ms:{:>4}}}}}  → ack_lsn={}",
            e.session_id,
            format!("\"{}\",", e.path),
            e.dwell_ms,
            ack,
            width = max_path_w + 3
        ).unwrap();
    }
    writeln!(s).unwrap();

    // [3/4] Query the global row
    writeln!(s, "[3/4] Query the global row").unwrap();
    writeln!(s, "──────────────────────────").unwrap();
    writeln!(s, "  POST /get  {{table:\"SiteMetrics\", key:\"\"}}").unwrap();
    writeln!(s, "  → {}", r.get_response_pretty).unwrap();
    writeln!(s).unwrap();

    // [4/4] Now run it for real
    writeln!(s, "[4/4] Now run it for real").unwrap();
    writeln!(s, "─────────────────────────").unwrap();
    match &r.drop_file_outcome {
        DropFileOutcome::Wrote(p) => {
            writeln!(
                s,
                "  Wrote {} — same pipeline, talks to a real server.",
                p.display()
            )
            .unwrap();
        }
        DropFileOutcome::SkippedAlreadyExists(p) => {
            writeln!(s, "  {} already exists — keeping yours.", p.display()).unwrap();
        }
        DropFileOutcome::SkippedNoFile => {
            writeln!(s, "  Skipping beava_quickstart.py drop (--no-file).").unwrap();
        }
    }
    writeln!(s).unwrap();
    writeln!(s, "  In one terminal:        $ beava").unwrap();
    writeln!(s, "  In another:             $ python beava_quickstart.py").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "  Or curl the surface yourself:").unwrap();
    writeln!(s, "    $ curl -X POST :8080/register -d @schema.json").unwrap();
    writeln!(
        s,
        "    $ curl -X POST :8080/push     -d '{{\"event\":\"PageView\",\"data\":{{\"session_id\":\"s\",\"path\":\"/\",\"dwell_ms\":1000}}}}'"
    )
    .unwrap();
    writeln!(
        s,
        "    $ curl -X POST :8080/get      -d '{{\"table\":\"SiteMetrics\",\"key\":\"\"}}'"
    )
    .unwrap();
    writeln!(s).unwrap();
    writeln!(s, "  Docs:  https://beava.dev/docs/quickstart").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "  Tearing down sandbox server.").unwrap();

    s
}

/// Write `beava_quickstart.py` to `dir` iff it doesn't already exist
/// (never clobber user edits) and `no_file` is false. Returns the
/// outcome.
pub fn write_drop_file_if_absent(dir: &Path, no_file: bool) -> std::io::Result<DropFileOutcome> {
    if no_file {
        return Ok(DropFileOutcome::SkippedNoFile);
    }
    let path = dir.join("beava_quickstart.py");
    if path.exists() {
        return Ok(DropFileOutcome::SkippedAlreadyExists(path));
    }
    std::fs::write(&path, QUICKSTART_PY)?;
    Ok(DropFileOutcome::Wrote(path))
}

/// Verbatim contents of the dropped `beava_quickstart.py` file.
///
/// Mirrors the in-process pipeline exactly so users can see the same
/// 4 steps rendered against a real `beava` server. No editorialising
/// comments — just the same shape, ready to be edited.
pub const QUICKSTART_PY: &str = r#"# beava_quickstart.py — same pipeline as `beava quickstart`.
# Run a real server in another terminal (`beava`) and run this file:
#     $ python beava_quickstart.py

import beava as bv


@bv.event
class PageView:
    session_id: str
    path: str
    dwell_ms: int


@bv.table   # no key= → one row, site-wide
def SiteMetrics(e: PageView):
    return e.agg(
        median_dwell_1h  = bv.quantile("dwell_ms", q=0.5, window="1h"),
        page_views_today = bv.count(window="24h"),
        top_page_1h      = bv.top_k("path", k=1, window="1h"),
    )


app = bv.App("127.0.0.1:8080")
app.register(PageView, SiteMetrics)

for sid, path, dwell in [
    ("s_1", "/",        1240),
    ("s_2", "/pricing", 3380),
    ("s_3", "/docs",     890),
    ("s_4", "/",        2110),
    ("s_5", "/docs",    5620),
]:
    app.push("PageView", {"session_id": sid, "path": path, "dwell_ms": dwell})

print(app.get("SiteMetrics"))
"#;

// ─── internals (HTTP self-loop + step orchestration) ─────────────────

/// The 5 canonical demo events. Hand-tuned so `quantile(dwell_ms, q=0.5)`
/// over a 1h window lands cleanly on the median sample (2110) — both
/// the in-process run and the dropped `beava_quickstart.py` produce the
/// same numbers.
fn canonical_events() -> Vec<EventDisplay> {
    vec![
        EventDisplay {
            session_id: "s_1".into(),
            path: "/".into(),
            dwell_ms: 1240,
        },
        EventDisplay {
            session_id: "s_2".into(),
            path: "/pricing".into(),
            dwell_ms: 3380,
        },
        EventDisplay {
            session_id: "s_3".into(),
            path: "/docs".into(),
            dwell_ms: 890,
        },
        EventDisplay {
            session_id: "s_4".into(),
            path: "/".into(),
            dwell_ms: 2110,
        },
        EventDisplay {
            session_id: "s_5".into(),
            path: "/docs".into(),
            dwell_ms: 5620,
        },
    ]
}

/// Poll `GET /ready` on the admin port until 200 or `timeout` elapses.
async fn wait_ready(admin_addr: SocketAddr, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    let mut last_err: Option<anyhow::Error> = None;
    while Instant::now() < deadline {
        match get_status(admin_addr, "/ready").await {
            Ok(200) => return Ok(()),
            Ok(other) => {
                last_err = Some(anyhow::anyhow!("/ready returned {other}"));
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("/ready never reported 200"))).context(format!(
        "admin /ready did not become 200 within {timeout:?}"
    ))
}

/// POST `body` (raw JSON) to `path` on `addr`. Returns `(status_code, body)`.
///
/// Reads Content-Length-many body bytes after the response headers
/// rather than relying on EOF — the in-process server keeps the socket
/// open even after `Connection: close` requests (separate v0.0.x bug;
/// the client side is HTTP/1.1-correct regardless).
async fn post_json(addr: SocketAddr, path: &str, body: &str) -> anyhow::Result<(u16, String)> {
    let mut stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("connect TCP {addr}"))?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(req.as_bytes())
        .await
        .context("write HTTP request")?;
    stream.flush().await.context("flush HTTP request")?;
    read_http_response(&mut stream, Duration::from_secs(5)).await
}

/// GET `path` on `addr`. Returns the status code (the body is discarded).
async fn get_status(addr: SocketAddr, path: &str) -> anyhow::Result<u16> {
    let mut stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("connect TCP {addr}"))?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .await
        .context("write HTTP GET")?;
    stream.flush().await.context("flush HTTP GET")?;
    let (status, _) = read_http_response(&mut stream, Duration::from_secs(2)).await?;
    Ok(status)
}

/// Read one HTTP/1.1 response off a TCP stream. Reads bytes until
/// the `\r\n\r\n` header terminator, parses `Content-Length` (or
/// defaults to 0), then reads exactly that many body bytes.
///
/// Does NOT support chunked transfer encoding — the engine always
/// emits Content-Length for the 6 routes quickstart hits.
async fn read_http_response(
    stream: &mut TcpStream,
    timeout: Duration,
) -> anyhow::Result<(u16, String)> {
    use tokio::io::AsyncReadExt;

    tokio::time::timeout(timeout, async {
        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        // Read until we see the header terminator.
        let header_end = loop {
            let mut chunk = [0u8; 1024];
            let n = stream
                .read(&mut chunk)
                .await
                .context("read response bytes")?;
            if n == 0 {
                anyhow::bail!("EOF before HTTP headers complete");
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(idx) = find_subslice(&buf, b"\r\n\r\n") {
                break idx + 4;
            }
            if buf.len() > 16 * 1024 {
                anyhow::bail!("HTTP headers exceeded 16 KiB without terminator");
            }
        };

        // Parse the head: status code + Content-Length.
        let head = std::str::from_utf8(&buf[..header_end - 4]).context("HTTP head not UTF-8")?;
        let status_line = head
            .lines()
            .next()
            .ok_or_else(|| anyhow::anyhow!("HTTP response missing status line"))?;
        let status: u16 = status_line
            .split(' ')
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("HTTP status line missing code"))?
            .parse()
            .context("parse HTTP status code")?;
        let content_length: usize = head
            .lines()
            .skip(1)
            .find_map(|line| {
                let (name, val) = line.split_once(':')?;
                if name.trim().eq_ignore_ascii_case("content-length") {
                    val.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);

        // We may already have all (or some) of the body in `buf`.
        let already = buf.len() - header_end;
        if content_length > already {
            let need = content_length - already;
            let mut body_extra = vec![0u8; need];
            stream
                .read_exact(&mut body_extra)
                .await
                .context("read remaining body")?;
            buf.extend_from_slice(&body_extra);
        }
        let body = std::str::from_utf8(&buf[header_end..header_end + content_length])
            .context("HTTP body not UTF-8")?
            .to_string();
        Ok::<(u16, String), anyhow::Error>((status, body))
    })
    .await
    .context("HTTP response timeout")?
}

/// Find `needle` in `haystack`. Tiny inline helper — quickstart's
/// hot path is bounded by HTTP latency, not search time.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Step [1/4] — register the PageView/SiteMetrics pipeline. Returns
/// `registry_version` from the response.
///
/// Wire shape: top-level `nodes` array. The `SiteMetrics` global table
/// is expressed as a `derivation` with `output_kind: "table"` and
/// `keys: []` / `table_primary_key: []` (the global-aggregation
/// sentinel from ADR-003). NOT to be confused with the older
/// `examples/wire/register-fraud-team.request.json` "descriptors"
/// shape — that fixture is stale relative to the live engine.
async fn step_register(addr: SocketAddr) -> anyhow::Result<u64> {
    let body = r#"{
  "nodes": [
    {
      "kind": "event",
      "name": "PageView",
      "schema": {
        "fields": {"session_id": "str", "path": "str", "dwell_ms": "i64"},
        "optional_fields": []
      }
    },
    {
      "kind": "derivation",
      "name": "SiteMetrics",
      "output_kind": "table",
      "upstreams": ["PageView"],
      "ops": [{
        "op": "group_by",
        "keys": [],
        "agg": {
          "median_dwell_1h":  {"op": "quantile", "params": {"field": "dwell_ms", "q": 0.5, "window": "1h"}},
          "page_views_today": {"op": "count",    "params": {"window": "24h"}},
          "top_page_1h":      {"op": "top_k",    "params": {"field": "path", "k": 1, "window": "1h"}}
        }
      }],
      "schema": {
        "fields": {
          "median_dwell_1h": "f64",
          "page_views_today": "i64",
          "top_page_1h": "json"
        },
        "optional_fields": []
      },
      "table_primary_key": []
    }
  ]
}"#;
    let (status, resp) = post_json(addr, "/register", body).await?;
    if status != 200 && status != 201 {
        anyhow::bail!("/register returned {status}: {resp}");
    }
    let v: serde_json::Value =
        serde_json::from_str(&resp).context("parse /register response JSON")?;
    let rv = v
        .get("registry_version")
        .and_then(|x| x.as_u64())
        .unwrap_or(1);
    Ok(rv)
}

/// Step [2/4] — push one PageView event. Returns `ack_lsn`.
async fn step_push(addr: SocketAddr, e: &EventDisplay) -> anyhow::Result<u64> {
    let body = format!(
        r#"{{"event":"PageView","data":{{"session_id":"{}","path":"{}","dwell_ms":{}}}}}"#,
        e.session_id, e.path, e.dwell_ms
    );
    let (status, resp) = post_json(addr, "/push", &body).await?;
    if status != 200 {
        anyhow::bail!("/push returned {status}: {resp}");
    }
    let v: serde_json::Value = serde_json::from_str(&resp).context("parse /push response JSON")?;
    let ack = v.get("ack_lsn").and_then(|x| x.as_u64()).unwrap_or(0);
    Ok(ack)
}

/// Step [3/4] — query the global SiteMetrics row. Returns the body
/// pretty-printed for display.
async fn step_get(addr: SocketAddr) -> anyhow::Result<String> {
    let body = r#"{"table":"SiteMetrics","key":""}"#;
    let (status, resp) = post_json(addr, "/get", body).await?;
    if status != 200 {
        anyhow::bail!("/get returned {status}: {resp}");
    }
    // Pretty-print the response so [3/4] reads cleanly. Fall back to
    // the raw body if it isn't JSON (shouldn't happen — engine always
    // emits JSON).
    let pretty = serde_json::from_str::<serde_json::Value>(&resp)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .map(|s| {
            // Indent each line by 6 spaces so the JSON sits under the
            // "  → " arrow without breaking the column.
            s.lines()
                .enumerate()
                .map(|(i, line)| {
                    if i == 0 {
                        line.to_string()
                    } else {
                        format!("    {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or(resp);
    Ok(pretty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fake_result() -> QuickstartResult {
        QuickstartResult {
            server_version: env!("CARGO_PKG_VERSION").into(),
            bind_addr: "127.0.0.1:8081".parse().unwrap(),
            ready_in: Duration::from_millis(180),
            registry_version: 1,
            ack_lsns: vec![1, 2, 3, 4, 5],
            events: vec![
                EventDisplay {
                    session_id: "s_1".into(),
                    path: "/".into(),
                    dwell_ms: 1240,
                },
                EventDisplay {
                    session_id: "s_2".into(),
                    path: "/pricing".into(),
                    dwell_ms: 3380,
                },
                EventDisplay {
                    session_id: "s_3".into(),
                    path: "/docs".into(),
                    dwell_ms: 890,
                },
                EventDisplay {
                    session_id: "s_4".into(),
                    path: "/".into(),
                    dwell_ms: 2110,
                },
                EventDisplay {
                    session_id: "s_5".into(),
                    path: "/docs".into(),
                    dwell_ms: 5620,
                },
            ],
            get_response_pretty: r#"{
      "median_dwell_1h":  2110,
      "page_views_today": 5,
      "top_page_1h":      [["/", 2]]
    }"#
            .into(),
            drop_file_outcome: DropFileOutcome::Wrote("./beava_quickstart.py".into()),
        }
    }

    #[test]
    fn formatter_includes_all_four_step_markers() {
        let s = format_walkthrough(&fake_result());
        for marker in ["[1/4]", "[2/4]", "[3/4]", "[4/4]"] {
            assert!(
                s.contains(marker),
                "formatter must include {marker}; got:\n{s}"
            );
        }
    }

    #[test]
    fn formatter_includes_pipeline_decorators_and_homepage_field_names() {
        let s = format_walkthrough(&fake_result());
        // Mirrors the homepage hero verbatim.
        assert!(s.contains("@bv.event"), "formatter must include @bv.event");
        assert!(s.contains("@bv.table"), "formatter must include @bv.table");
        assert!(
            s.contains("class PageView:"),
            "formatter must include `class PageView:`"
        );
        assert!(
            s.contains("def SiteMetrics"),
            "formatter must include `def SiteMetrics`"
        );
        for field in ["median_dwell_1h", "page_views_today", "top_page_1h"] {
            assert!(
                s.contains(field),
                "formatter must include homepage field {field}; got:\n{s}"
            );
        }
    }

    #[test]
    fn formatter_renders_real_pushed_event_values() {
        let s = format_walkthrough(&fake_result());
        // Every dwell_ms from fake_result must surface in the [2/4]
        // section so the user can see the actual pushed values.
        for v in [1240, 3380, 890, 2110, 5620] {
            assert!(
                s.contains(&v.to_string()),
                "formatter must include dwell_ms={v}; got:\n{s}"
            );
        }
        // Each session_id and path must surface too.
        for sid in ["s_1", "s_2", "s_3", "s_4", "s_5"] {
            assert!(s.contains(sid), "formatter must include {sid}");
        }
        for p in ["/", "/pricing", "/docs"] {
            assert!(
                s.contains(&format!("\"{p}\"")),
                "formatter must include path \"{p}\""
            );
        }
    }

    #[test]
    fn formatter_renders_get_response_verbatim() {
        let s = format_walkthrough(&fake_result());
        // The pretty-printed get response from QuickstartResult is
        // displayed under [3/4] verbatim — that's how step 4's
        // "run it for real" promise stays honest.
        assert!(
            s.contains("median_dwell_1h"),
            "[3/4] must surface median_dwell_1h"
        );
        assert!(
            s.contains("[[\"/\", 2]]"),
            "[3/4] must surface real top_page_1h shape"
        );
    }

    #[test]
    fn formatter_includes_step_4_run_for_real_curl_examples() {
        let s = format_walkthrough(&fake_result());
        assert!(s.contains("curl"), "step [4/4] must include curl examples");
        assert!(
            s.contains("/register"),
            "step [4/4] must include /register example"
        );
        assert!(s.contains("/push"), "step [4/4] must include /push example");
        assert!(s.contains("/get"), "step [4/4] must include /get example");
    }

    #[test]
    fn formatter_announces_drop_file_outcome() {
        let mut r = fake_result();
        r.drop_file_outcome =
            DropFileOutcome::Wrote(std::path::PathBuf::from("./beava_quickstart.py"));
        let s = format_walkthrough(&r);
        assert!(
            s.contains("beava_quickstart.py"),
            "step [4/4] must name the drop file"
        );

        r.drop_file_outcome = DropFileOutcome::SkippedAlreadyExists(std::path::PathBuf::from(
            "./beava_quickstart.py",
        ));
        let s = format_walkthrough(&r);
        assert!(
            s.to_lowercase().contains("keeping yours")
                || s.to_lowercase().contains("already exists"),
            "step [4/4] must announce the file was preserved when present; got:\n{s}"
        );

        r.drop_file_outcome = DropFileOutcome::SkippedNoFile;
        let s = format_walkthrough(&r);
        assert!(
            s.to_lowercase().contains("--no-file") || s.to_lowercase().contains("no file"),
            "step [4/4] must mention --no-file when that flag was set; got:\n{s}"
        );
    }

    #[test]
    fn drop_file_writes_when_absent() {
        let dir = tempdir().expect("tempdir");
        let outcome = write_drop_file_if_absent(dir.path(), false).expect("write_drop_file");
        let path = dir.path().join("beava_quickstart.py");
        match &outcome {
            DropFileOutcome::Wrote(p) => assert_eq!(p, &path, "Wrote path mismatch"),
            other => panic!("expected Wrote; got {other:?}"),
        }
        assert!(path.exists(), "drop file must exist after write");
        let body = std::fs::read_to_string(&path).expect("read");
        // Body must mirror the homepage pipeline.
        assert!(body.contains("@bv.event"), "drop file must use @bv.event");
        assert!(body.contains("@bv.table"), "drop file must use @bv.table");
        assert!(
            body.contains("PageView"),
            "drop file must define PageView event"
        );
        assert!(
            body.contains("SiteMetrics"),
            "drop file must define SiteMetrics table"
        );
    }

    #[test]
    fn drop_file_skips_when_already_present_and_preserves_user_edits() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("beava_quickstart.py");
        let user_content = "# user-edited file — do not clobber\nprint('mine')\n";
        std::fs::write(&path, user_content).expect("seed user file");

        let outcome = write_drop_file_if_absent(dir.path(), false).expect("write_drop_file");
        match &outcome {
            DropFileOutcome::SkippedAlreadyExists(p) => {
                assert_eq!(p, &path, "SkippedAlreadyExists path mismatch")
            }
            other => panic!("expected SkippedAlreadyExists; got {other:?}"),
        }
        let body = std::fs::read_to_string(&path).expect("read");
        assert_eq!(
            body, user_content,
            "user's edits to beava_quickstart.py must be preserved verbatim"
        );
    }

    #[test]
    fn drop_file_skips_when_no_file_flag_set() {
        let dir = tempdir().expect("tempdir");
        let outcome = write_drop_file_if_absent(dir.path(), true).expect("write_drop_file");
        assert_eq!(
            outcome,
            DropFileOutcome::SkippedNoFile,
            "expected SkippedNoFile"
        );
        let path = dir.path().join("beava_quickstart.py");
        assert!(
            !path.exists(),
            "drop file must not be written when --no-file is set"
        );
    }
}
