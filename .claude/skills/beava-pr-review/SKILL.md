---
name: beava-pr-review
description: |
  Reviews a beava PR diff for real bugs, beava-specific architectural
  invariants, and AI-generated "slop" patterns (hollow code, phantom
  imports, inflated comments, disconnected pipelines). Produces a
  structured BLOCK / WARN / NIT report keyed by file:line. Use when
  asked to "review this PR", "check for AI slop", "PR review beava",
  or "audit this diff before merge". Run the same gates the local
  `bash .github/scripts/check.sh` runs in addition to the LLM review.
---

# beava-pr-review

Diff-aware code reviewer tuned for the beava codebase. Checks three
layers in order; surface findings sorted by severity (BLOCK > WARN > NIT)
with `file:line` anchors.

## When to invoke

- User says "review this PR" / "PR review" / "audit this diff" / "check
  for slop" against a feature branch or a PR URL on `beava-dev/beava`.
- After a substantial code change but before requesting `ok-to-test`
  approval — the local check.sh covers fmt/lint/test, this skill
  covers the human-judgment layer.

## How to run

1. **Bound the diff.** `git diff main...HEAD --stat` (or `gh pr diff <N>
   -R beava-dev/beava` for a remote PR). Read only the changed lines +
   ~30 lines of context per hunk. Do NOT review unchanged code.
2. **Run automated gates first.** Before LLM review, surface anything
   the existing tools catch:
   - `bash .github/scripts/check.sh --fast` — fmt, clippy, ruff,
     mypy (advisory). If any FAIL, the PR is incomplete; tell the
     user to fix before requesting human review.
3. **Apply the three review layers below**, in order. Skip a layer only
   if the diff doesn't touch its scope (e.g. skip "AI slop" for a pure
   .gitignore change).
4. **Emit the report** in the format under "Output format" at the end.

---

## Layer 1 — Real bugs

The most common categories worth checking against the diff:

- **Off-by-one** — index math, slice bounds, half-open vs closed
  ranges. Especially in `crates/beava-core/src/agg_op.rs`,
  `crates/beava-runtime-core/src/router.rs`, anywhere computing
  `window_secs` / `bucket_secs`.
- **Null / Option / `?` propagation** — `.unwrap()` in non-test code is
  WARN unless the PR shows the panic is provably unreachable. `expect()`
  with a useful message is fine.
- **Resource leaks** — file descriptors, mio tokens, dropped futures
  (we hand-rolled the runtime; mio leaks don't auto-close like tokio).
- **Concurrency** — single-threaded apply is locked architecture
  (`project_no_sharded_apply`); any new thread / parallel iterator
  inside `dispatch_*_sync` is BLOCK.
- **Schema / wire compatibility** — changes to
  `crates/beava-runtime-core/src/router.rs` routes, `agg_op.rs`
  `AggKind` variants, or `python/beava/_app.py::_descriptor_to_node`
  wire shape break clients. WARN unless the PR body calls it out and
  ships a migration note.
- **TDD discipline** (Phase 3+ only) — every plan task must produce a
  `test(...)` commit before its `feat(...)` commit. Commits that show
  `feat:` without a preceding `test:` for the same scope are WARN
  (per CLAUDE.md §Conventions → TDD Discipline).

## Layer 2 — beava architectural invariants

Hard locks per CLAUDE.md / memory; violating them is BLOCK:

- **mio-only hot path** — `crates/beava-server/src/apply_shard.rs::dispatch_*_sync`
  is the only data-plane entry. New `axum::*` symbols outside
  `crates/beava-server/src/http_admin.rs` → BLOCK
  (`project_phase18_no_dual_runtime`).
- **events-only v0** — `OpNode::Table*`, `RecordType::TableUpsert/...`,
  `TemporalStore`, `MvccVersion`, `temporal_http`, `push_table`,
  `delete_table`, `fn retract(` resurfacing → BLOCK
  (`project_v0_events_only_scope`). Tables for *aggregation output*
  are allowed (per ADR-001 partial overturn), upsert/delete/retract are
  not.
- **Redis-shaped, processing-time only (v0)** — event-time semantics,
  watermarks, dual-stack runtimes → BLOCK
  (`project_redis_shaped_no_event_time_ever`; v0 still ships
  processing-time only, event-time is post-v0 roadmap).
- **No same-key sketch batching** → BLOCK (preserves
  read-after-write).
- **AI attribution in commits** — `Co-Authored-By: Claude` /
  `Generated with Claude` markers → BLOCK
  (`feedback_no_ai_attribution_in_commits`).

## Layer 3 — AI slop detection

Per the [Antislop ICLR 2026 paper](https://arxiv.org/pdf/2510.15061) +
[AI-SLOP Detector patterns](https://github.com/flamehaven01/AI-SLOP-Detector)
+ beava-specific tells.

### Code-shape slop (BLOCK if present, WARN if borderline)

- **Empty / hollow functions** — `fn foo() { /* TODO */ }` without a
  GitHub issue link, or a function whose body is *only* a single
  passthrough `self.x = x` that no caller actually invokes (check
  callers before flagging).
- **Phantom imports / unused dependencies** — `use crate::baz` where
  `baz` isn't actually referenced in the body. Pre-existing trailing
  unused imports in untouched files: NIT. New ones: WARN.
- **Disconnected pipelines** — a new `@bv.event` / `@bv.table` not
  wired into any `app.register(...)` call.
- **Defensive code for unreachable conditions** — `if x is not None and
  x is not None:` style; `try: ... except Exception: pass` swallowing
  errors with no logging or comment explaining what it's masking.
- **Buzzword inflation in comments** — comments that say "this leverages
  the synergy of the polymorphic abstraction" or similar. Comment density
  > 30% of LOC for code that isn't a public API contract: WARN.

### Comment / docstring slop (WARN)

- **WHAT-not-WHY commentary** — `// increment counter` next to `count
  += 1`. Per CLAUDE.md §Doing tasks: comments explain WHY, not WHAT.
- **References to the current task / fix / PR** — `// fixed in this PR`
  or `// added for issue #42` rot fast; belongs in the commit message,
  not the code.
- **Aspirational future-tense** — `// this will eventually support
  ...` without a tracked issue. NIT if minor; WARN if it affects API
  shape.
- **Over-explanation of standard library calls** — `// open the file
  in read mode` next to `open(path, 'r')`.

### Voice / textual slop (NIT unless egregious)

- **Em-dash density** — three or more em-dashes (—) in a single
  paragraph of doc / docstring is suspicious; LLMs reach for them
  ~1000× more than humans per the Antislop paper. Not auto-flag, but
  if combined with other signals → WARN.
- **First-person plural in user-facing copy** — "we wanted to make
  this easier for you" reads like a marketing draft.
- **Hedge-y openings** — "It is worth noting that…", "Importantly,…",
  "Indeed…" — strip in user-facing prose.

### Commit-message slop

- **Generic chore commits** — `chore: update files` / `fix: bug fix`
  → WARN. Should follow `type(scope): subject` per CLAUDE.md.
- **AI attribution** — already covered in Layer 2 (BLOCK).

---

## Output format

Render the report in this shape — keep it scannable:

```
# beava PR review — <branch or PR #>

## Summary
- <one sentence: ship-readiness signal>
- diff: <files changed> files / <±lines>
- automated gates: <pass|fail line from check.sh>

## BLOCK (N)
- `path/to/file.rs:42` — <one-line finding>
  Why: <one sentence>
  Fix: <one sentence>

## WARN (N)
- `path/to/file.py:88` — …

## NIT (N)
- `…`

## Looks good
- <2-4 bullets on what the diff does well — reviewers shouldn't only
  surface negatives>
```

If a layer has zero findings, write "_none_" under it. Don't omit the
header — the consistent shape makes follow-up automation easier.

---

## Anti-patterns for the reviewer (you)

- **Don't review unchanged code.** Bound to the diff.
- **Don't suggest sweeping refactors.** PRs land scoped; refactor
  proposals belong in a separate issue.
- **Don't fabricate file paths or line numbers.** Verify with `Read`
  or `git show` before citing them.
- **Don't quote the diff back at the user verbatim.** Summarise in
  your own voice; the user already has the diff.

## Sources

- [Antislop (ICLR 2026)](https://arxiv.org/pdf/2510.15061) — slop frequency baselines
- [AI-SLOP Detector](https://github.com/flamehaven01/AI-SLOP-Detector) — 27 adversarial pattern checks
- [LLM code review early results (arXiv 2404.18496)](https://arxiv.org/html/2404.18496v2) — diff-aware review patterns
- `CLAUDE.md` (this repo) — beava conventions, locked invariants
- Memory: `project_no_sharded_apply`, `project_phase18_no_dual_runtime`,
  `project_v0_events_only_scope`, `feedback_no_ai_attribution_in_commits`
