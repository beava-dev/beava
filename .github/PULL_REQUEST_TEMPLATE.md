# Summary

<!-- 1-3 sentences. What does this PR change? Why? -->

## Linked issues

<!-- Closes #N / Related to #N — if applicable. -->

---

## Local checks

CI auto-fires on every PR (tokio/ruff/polars/deno standard). For external
fork PRs, GitHub's native "Approve and run workflows" button gates the
first run — a maintainer clicks Approve in the Checks tab. Internal PRs
auto-run.

Run the same gates locally before opening the PR to save round-trips.

### One-shot

```bash
bash .github/scripts/check.sh           # full: fmt + clippy + tests + pytest
bash .github/scripts/check.sh --fast    # skip cargo test (~10× faster)
bash .github/scripts/check.sh --rust    # rust gates only
bash .github/scripts/check.sh --python  # python gates only (ruff + mypy + pytest)
```

The script prints a `PASS / FAIL` summary you can paste below as proof.

### Or run each manually

```bash
# Rust
cargo fmt --all --check
cargo clippy --workspace --all-targets --features testing -- -D warnings
cargo nextest run --features testing --no-fail-fast    # or: cargo test --workspace --features testing

# Python SDK (honors pyproject testpaths = tests/v0 — the gated suite)
cd python && python -m pytest -v

# Docker image (matches publish-edge-image.yml)
docker build -f deploy/Dockerfile.beava -t beava:dev .
docker run --rm -p 8080:8080 beava:dev

# Website (only if you touched beava-website/project/**)
cd beava-website && npm install && npm run build
```

---

## Verification

Paste the summary block from `bash .github/scripts/check.sh` here so the
reviewer can see the local run passed:

```text
PASS  cargo fmt --all --check  (1s)
PASS  cargo clippy --workspace --all-targets --features testing -- -D warnings  (37s)
PASS  cargo nextest run --features testing --no-fail-fast  (84s)
PASS  ruff check python/  (0s)
PASS  mypy --strict beava/
PASS  pytest python (v0 acceptance suite)  (13s)
```

<!-- Replace the example block above with your real output. -->

---

## Pre-flight checklist

- [ ] `bash .github/scripts/check.sh` exits 0 (or each step passes manually)
- [ ] Verification block above replaced with real output
- [ ] Docs updated (`docs/*.md`, `beava-website/project/docs/`, decorator docstrings) if user-visible behavior changed
- [ ] No stale repository URLs (canonical: `beava-dev/beava`)
- [ ] If this commits a schema change to `beava-website/deploy/site-metrics-pipeline.json`, called out in this PR body — the deploy workflow re-registers with `force=true`, which silently lands destructive edits

## Notes for the reviewer

<!-- Anything you want a reviewer to look at first / verify in browser. -->
