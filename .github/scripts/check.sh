#!/usr/bin/env bash
# Local pre-PR check — runs the same gates CI runs (cargo fmt + clippy +
# tests + Python tests + beava-js Vitest) and writes a one-line PASS/FAIL
# summary you can paste into the PR description as proof.
#
# Usage:
#   bash .github/scripts/check.sh            # run everything, print summary
#   bash .github/scripts/check.sh --fast     # skip cargo test (~10× faster)
#   bash .github/scripts/check.sh --rust     # rust gates only (cargo fmt/clippy/test)
#   bash .github/scripts/check.sh --python   # python gates only (ruff + mypy + pytest)
#   bash .github/scripts/check.sh --js       # beava-js only (pnpm install + turbo lint/check-types/test)
#   bash .github/scripts/check.sh --output=  # path for full log (default ~/.beava-check.log)
#
# When target/debug/beava exists, beava-js Vitest also runs HTTP integration tests (BEAVA_INTEGRATION=1).
# Otherwise three integration tests are skipped (same as CI after cargo build).
#
# --rust, --python, and --js are mutually exclusive; pass none to run rust + python + beava-js.
#
# Exit code: 0 if all checks pass, non-zero otherwise.
set -uo pipefail

FAST=0
RUN_RUST=1
RUN_PYTHON=1
RUN_JS=1
OUT="$HOME/.beava-check.log"
for arg in "$@"; do
  case "$arg" in
    --fast) FAST=1 ;;
    --rust) RUN_PYTHON=0; RUN_JS=0 ;;
    --python) RUN_RUST=0; RUN_JS=0 ;;
    --js) RUN_RUST=0; RUN_PYTHON=0; RUN_JS=1 ;;
    --output=*) OUT="${arg#--output=}" ;;
    -h|--help)
      sed -n '2,/^set/p' "$0" | sed 's/^# \{0,1\}//; /^set /d'
      exit 0 ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

if [[ "$RUN_RUST" -eq 0 && "$RUN_PYTHON" -eq 0 && "$RUN_JS" -eq 0 ]]; then
  echo "error: no check targets enabled (use --rust, --python, or --js)" >&2
  exit 2
fi

: > "$OUT"
SUMMARY=()
FAILED=0

run() {
  local name="$1"; shift
  local started=$(date +%s)
  printf '\n=== %s ===\n' "$name" | tee -a "$OUT" >&2
  if "$@" >>"$OUT" 2>&1; then
    SUMMARY+=("PASS  $name  ($(($(date +%s) - started))s)")
    printf '  ✓ %s\n' "$name" >&2
  else
    SUMMARY+=("FAIL  $name  ($(($(date +%s) - started))s)")
    printf '  ✗ %s — see %s\n' "$name" "$OUT" >&2
    FAILED=1
  fi
}

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

if [[ "$RUN_RUST" -eq 1 ]]; then
  run "cargo fmt --all --check" \
    cargo fmt --all --check

  run "cargo clippy --workspace --all-targets --features testing -- -D warnings" \
    cargo clippy --workspace --all-targets --features testing -- -D warnings

  if [[ "$FAST" -eq 0 ]]; then
    if command -v cargo-nextest >/dev/null 2>&1; then
      run "cargo nextest run --features testing --no-fail-fast" \
        cargo nextest run --features testing --no-fail-fast
    else
      run "cargo test --workspace --features testing" \
        cargo test --workspace --features testing
    fi
  fi
fi

if [[ "$RUN_PYTHON" -eq 1 && -d python && -f python/pyproject.toml ]]; then
  if command -v ruff >/dev/null 2>&1; then
    run "ruff check python/" \
      bash -c 'cd python && ruff check .'
  else
    SUMMARY+=("SKIP  ruff check python/  (ruff not installed)")
  fi
  if command -v mypy >/dev/null 2>&1; then
    # Advisory — strict mypy will likely flag pre-existing items.
    if (cd python && mypy beava) >>"$OUT" 2>&1; then
      SUMMARY+=("PASS  mypy --strict beava/")
    else
      SUMMARY+=("WARN  mypy --strict beava/  (advisory; not blocking)")
    fi
  else
    SUMMARY+=("SKIP  mypy --strict beava/  (mypy not installed)")
  fi
  # Honor pyproject's testpaths (= tests/v0) — CI gates against this suite only.
  # Legacy tests/internal/, tests/bench/, tests/integration/, tests/conformance/
  # are documented drift, tracked as v0.0.x cleanup backlog.
  run "pytest python (v0 acceptance suite)" \
    bash -c 'cd python && python -m pytest -q --no-header'
fi

if [[ "$RUN_JS" -eq 1 && -f "$REPO_ROOT/beava-js/package.json" ]]; then
  js_cmd='cd beava-js && (command -v corepack >/dev/null 2>&1 && corepack enable pnpm || true) && pnpm install && pnpm exec turbo run lint check-types test'
  if [[ -f "$REPO_ROOT/target/debug/beava" || -f "$REPO_ROOT/target/debug/beava.exe" ]]; then
    run "pnpm turbo lint/check-types/test (beava-js, +HTTP integration)" \
      env BEAVA_INTEGRATION=1 BEAVA_REPO_ROOT="$REPO_ROOT" bash -c "$js_cmd"
  else
    printf '\n(note: target/debug/beava missing; 3 HTTP integration Vitest tests skipped — run: cargo build --bin beava)\n' | tee -a "$OUT" >&2
    run "pnpm turbo lint/check-types/test (beava-js, unit Vitest only)" \
      bash -c "$js_cmd"
  fi
fi

# Tail-of-log + summary.
echo
echo "─── Summary ────────────────────────────────────────────────"
printf '%s\n' "${SUMMARY[@]}"
echo "─── Full log: $OUT ─────────────────────────────────────────"

# Markdown block — copy-paste straight into the PR's verification section.
if [[ -t 1 ]]; then
  echo
  echo "Paste this into your PR description under \"Verification\":"
  echo '```'
  printf '%s\n' "${SUMMARY[@]}"
  echo '```'
fi

exit "$FAILED"
