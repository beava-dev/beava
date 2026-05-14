//! Phase 12.6 Plan 10 — architectural test enforcing mio-only data-plane
//! invariants.
//!
//! Two locked architectural commitments from Phase 12.6 are guarded here:
//!
//! **1. Single hot-path entry.**
//! The mio event loop is the only data-plane runtime. The legitimate callers
//! of `apply_event_to_aggregations` are exactly two:
//! `crates/beava-server/src/apply_shard.rs::dispatch_push_sync` (mio data
//! plane — invoked from `dispatch_one`); and
//! `crates/beava-server/src/recovery.rs::replay_handrolled_wal_dir` plus
//! `replay_wal_from_lsn` (cold-path WAL replay on boot). Any third caller is
//! an architectural regression per `project_phase18_no_dual_runtime` +
//! `project_redis_shaped_no_event_time_ever`.
//!
//! **2. axum is restricted to the admin sidecar.**
//! `axum::Router`, `axum::Json`, `axum::Extension`, `axum::extract::*`,
//! `axum::routing::*` imports may only appear in
//! `crates/beava-server/src/http_admin.rs` — the canonical tokio admin
//! sidecar bound by `ServerV18` at `cfg.admin_addr`. Any other appearance
//! (especially in a re-introduced data-plane handler) is a regression.
//!
//! Companion to `phase12_6_legacy_axum_killed.rs` (Plan 07): that test
//! asserts the legacy axum *files* and *symbols* were deleted. This test
//! asserts the *architectural invariants* hold going forward — it is a
//! tripwire for any future code change that would re-introduce a second
//! hot-path entry or re-spread axum across the data plane.
//!
//! ## Verification probe (TDD red-form for an architectural-invariant test)
//!
//! Architectural-invariant tests are confirmation tests, not behaviour-driven
//! RED-GREEN tests. To convince yourself the test actually catches violations:
//!
//! **Probe 1 — apply_event_to_aggregations.** In a non-allowlisted file under
//! `crates/beava-server/src/` (e.g. `runtime_core_glue.rs`), temporarily add
//! a call to `beava_core::agg_apply::apply_event_to_aggregations(...)` inside
//! a `fn` body. Run `cargo test -p beava-server --test
//! phase12_6_mio_only_dataplane`. Confirm
//! `only_apply_shard_and_recovery_call_apply_event_to_aggregations` fails
//! with the file's path in the violation list. REVERT the edit.
//!
//! **Probe 2 — axum.** In the same file, temporarily add a reference to
//! `axum::Router`. Run the test. Confirm
//! `axum_imports_only_in_admin_sidecar` fails. REVERT the edit.
//!
//! Both probes were exercised during Plan 12.6-10 execution and reverted
//! before commit; this docstring records the procedure so future maintainers
//! can re-validate the invariant.

use std::path::{Path, PathBuf};

/// Workspace root — two parents up from `CARGO_MANIFEST_DIR`
/// (`crates/beava-server`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Recursively collect every `.rs` file under `dir`.
fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    visit(&p, out);
                } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                    out.push(p);
                }
            }
        }
    }
    visit(dir, &mut out);
    out
}

/// Drop lines whose first non-whitespace characters are `//` (line comments)
/// or `///` / `//!` (doc comments). Block comments are not stripped — they
/// are rare in this codebase and any false positive in a `/* ... */` block
/// would be flagged by the violation list, prompting the author to either
/// move the comment or update the test.
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("//") || t.starts_with("///") || t.starts_with("//!"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Locate `crates/beava-server/src/`.
fn server_src() -> PathBuf {
    workspace_root().join("crates/beava-server/src")
}

/// Test 1 — Single hot-path entry. `apply_event_to_aggregations(` may only
/// appear in `apply_shard.rs` (mio data plane) and `recovery.rs` (WAL
/// replay). Any third caller is an architectural regression.
///
/// The grep is line-comment-stripped so doc-references in module-level
/// comments (e.g. `snapshot_task.rs` line 101) do not count.
#[test]
fn only_apply_shard_and_recovery_call_apply_event_to_aggregations() {
    let src = server_src();
    let files = collect_rs_files(&src);
    // Allowlist: the two legitimate callers per
    // `project_phase18_no_dual_runtime` + Plan 12.6-10 must_have.
    const ALLOWLIST: &[&str] = &["apply_shard.rs", "recovery.rs"];

    let mut violations = Vec::new();
    for f in &files {
        let basename = f
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if ALLOWLIST.contains(&basename.as_str()) {
            continue;
        }
        let content = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let stripped = strip_line_comments(&content);
        // Match `apply_event_to_aggregations(` AND the replay variant
        // `apply_event_to_aggregations_replay(` — both are hot-path
        // entries gated by the mio-only invariant.
        if stripped.contains("apply_event_to_aggregations(")
            || stripped.contains("apply_event_to_aggregations_replay(")
        {
            violations.push(format!(
                "{}: contains a call to `apply_event_to_aggregations[_replay](` (only apply_shard.rs and recovery.rs may call it)",
                f.display()
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "Architectural regression: only `apply_shard.rs::dispatch_push_sync` (mio data plane) \
         and `recovery.rs::replay_*` (WAL replay) may call `apply_event_to_aggregations`. \
         Per `project_phase18_no_dual_runtime` + `project_redis_shaped_no_event_time_ever`, \
         the mio event loop is the SOLE data-plane runtime. Any third caller is a regression.\n\
         Violations:\n{}",
        violations.join("\n")
    );
}

/// Test 2 — axum is restricted to the admin sidecar. `axum::Router /
/// axum::Json / axum::Extension / axum::extract::* / axum::routing::*` may
/// only appear in `http_admin.rs` (the tokio admin sidecar bound by
/// `ServerV18` at `cfg.admin_addr`). Any other appearance is a regression.
///
/// The grep is line-comment-stripped so doc-references in module-level
/// comments (e.g. `server.rs` mentions of "tokio/axum on the admin plane")
/// do not count.
#[test]
fn axum_imports_only_in_admin_sidecar() {
    let src = server_src();
    let files = collect_rs_files(&src);
    // Allowlist: the canonical tokio admin sidecar (kept per Plan 12.6-07
    // Deviation 1 — `BoundAdminServer::bind` is consumed by
    // `ServerV18::bind`).
    const ALLOWLIST: &[&str] = &["http_admin.rs"];
    // Patterns covering the typical axum surface used by handlers/routers.
    const PATTERNS: &[&str] = &[
        "axum::Router",
        "axum::Json",
        "axum::Extension",
        "axum::extract::",
        "axum::routing::",
        "axum::http::",
        "axum::body::",
        "axum::response::",
        "axum::middleware",
    ];

    let mut violations = Vec::new();
    for f in &files {
        let basename = f
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if ALLOWLIST.contains(&basename.as_str()) {
            continue;
        }
        let content = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let stripped = strip_line_comments(&content);
        for needle in PATTERNS {
            if stripped.contains(needle) {
                violations.push(format!("{}: contains '{}'", f.display(), needle));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Architectural regression: `axum::*` symbols are restricted to `http_admin.rs` \
         (the tokio admin sidecar bound by ServerV18 at `cfg.admin_addr`). Per \
         `project_phase18_no_dual_runtime` the mio data plane has no axum surface. \
         Any other appearance is a regression — re-introducing a second data-plane \
         runtime requires explicit user override + new ADR.\n\
         Violations:\n{}",
        violations.join("\n")
    );
}

/// Test 3 — sanity check that the test infrastructure itself works: the
/// allowlisted files DO contain the patterns we are guarding. If this
/// assertion fails, the test has been silently rendered a no-op (e.g. a
/// future refactor moved `apply_event_to_aggregations` out of `apply_shard.rs`
/// without updating the allowlist).
#[test]
fn sanity_allowlisted_callers_actually_contain_patterns() {
    let root = workspace_root();
    let apply_shard = root.join("crates/beava-server/src/apply_shard.rs");
    let recovery = root.join("crates/beava-server/src/recovery.rs");
    let http_admin = root.join("crates/beava-server/src/http_admin.rs");

    let apply_shard_src = std::fs::read_to_string(&apply_shard).expect("apply_shard.rs must exist");
    let recovery_src = std::fs::read_to_string(&recovery).expect("recovery.rs must exist");
    let http_admin_src = std::fs::read_to_string(&http_admin).expect("http_admin.rs must exist");

    let apply_shard_stripped = strip_line_comments(&apply_shard_src);
    let recovery_stripped = strip_line_comments(&recovery_src);
    let http_admin_stripped = strip_line_comments(&http_admin_src);

    assert!(
        apply_shard_stripped.contains("apply_event_to_aggregations("),
        "sanity: apply_shard.rs must contain a call to apply_event_to_aggregations \
         (the canonical mio hot-path entry). If this fails, the dispatch path has \
         moved — update the test allowlist."
    );
    assert!(
        recovery_stripped.contains("apply_event_to_aggregations(")
            || recovery_stripped.contains("apply_event_to_aggregations_replay("),
        "sanity: recovery.rs must contain a call to apply_event_to_aggregations \
         (or the `_replay` variant — the cold-path WAL replay site). If this \
         fails, recovery has moved — update the test allowlist."
    );
    assert!(
        http_admin_stripped.contains("axum::"),
        "sanity: http_admin.rs must contain `axum::` (the canonical tokio admin \
         sidecar). If this fails, the admin sidecar has moved — update the test \
         allowlist."
    );
}
