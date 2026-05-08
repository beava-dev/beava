# Comment Conventions (internal)

Heuristic (CLAUDE.md): **default to no comments; only add when WHY is non-obvious.**
Internal comment-style guidance.

## DELETE patterns

- Paraphrasing what the code does (`// loop over users`, `// initialize state`)
- Doc comments on every private function (Rust `///` is for **public** API only — not on every helper)
- Section markers (`// === Setup ===`, `// === Cleanup ===`, `// ───────────────`)
- Self-narration sequences (`// First, we ... // Then, we ... // Finally, we ...`)
- Restating type signatures in prose (`/// fn add(a: i32, b: i32) -> i32 — adds two integers`)
- Phase / plan / task references in code (`// added in v0 for ...`, `// an internal plan …`)
- AI-tell phrases: "Note that ...", "Importantly, ...", "Here we ...", "We then ...", "It should be noted that ..."
- Multi-paragraph docstrings on a 5-line function
- Closing braces echoing the opening (`} // end of for`, `} // matches if x above`)

## KEEP patterns

- Explains WHY a non-obvious constraint exists (`// must be u32 — Python struct lib only handles u32 here`)
- Workaround for a specific bug (link the issue / commit / phase number)
- Invariant a reader can't infer from the code (`// guaranteed > 0 by validate_input in caller`)
- `SAFETY` contracts on `unsafe` blocks (Rust idiom)
- Doc comments on PUBLIC lib/SDK exports (cargo doc / Sphinx render targets)
- Hidden-state warnings (`// mutates self.state while iterating — careful`)
- Single-line "why this isn't obvious" notes
- **Architectural-invariant WHY-comments documenting v0 mio-only / v0 events-only / memory-governance / v0 hand-rolled-runtime commitments — explicit KEEP.** (Rationale: these encode locked architectural commitments tracked in CLAUDE.md §"mio-only Hot-Path Invariant", §"Events-Only Invariant", and the project-memory items; a reader cannot infer from the code alone that e.g. "do not call from tokio context" is a permanent architectural decision, not a stale comment. The phase-number reference in such comments is allowed because it points to the canonical lock-source. This rule supersedes the generic DELETE-pattern "Phase / plan / task references in code" for these specific four invariants only.)

## Verification recipe per component (Wave 2 plans 02–08 use)

Per Rust crate:

```bash
cargo test --workspace --features testing
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
cargo doc --no-deps --workspace
```

Per Python module:

```bash
pytest python/tests -x
mypy --strict python/beava
```

Per examples directory:

```bash
bash examples/test_examples.sh
```

## How a Wave-2 executor applies this

1. Read every `.rs` / `.py` / `.ts` / `.go` file in scope.
2. For every comment line: classify as KEEP or DELETE per the lists above.
3. Edit in place — keep the commit small (one PR-sized commit per crate).
4. Run the verification recipe; must stay green.
5. NO logic changes. NO refactoring beyond comment removal.
