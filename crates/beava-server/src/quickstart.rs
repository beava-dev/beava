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

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

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
pub fn run(_no_file: bool) -> anyhow::Result<()> {
    todo!("1a-green: wire spawn → register → push → get → format → drop_file → teardown")
}

/// Render the 4-step walkthrough as a single String.
///
/// Pure function — given a [`QuickstartResult`], always produces the same
/// output.  Mirrors the homepage hero exactly: `@bv.event PageView`,
/// `@bv.table SiteMetrics` (no key=, global), feature names
/// `median_dwell_1h` / `page_views_today` / `top_page_1h`.
pub fn format_walkthrough(_r: &QuickstartResult) -> String {
    todo!("1a-green: implement formatter — Unicode box-drawing, 4 steps")
}

/// Write `beava_quickstart.py` to `dir` iff it doesn't already exist
/// (never clobber user edits) and `no_file` is false. Returns the
/// outcome.
pub fn write_drop_file_if_absent(
    _dir: &Path,
    _no_file: bool,
) -> std::io::Result<DropFileOutcome> {
    todo!("1a-green: implement drop-file logic")
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fake_result() -> QuickstartResult {
        QuickstartResult {
            server_version: "0.1.0".into(),
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
        assert!(
            s.contains("/push"),
            "step [4/4] must include /push example"
        );
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

        r.drop_file_outcome = DropFileOutcome::SkippedAlreadyExists(
            std::path::PathBuf::from("./beava_quickstart.py"),
        );
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
