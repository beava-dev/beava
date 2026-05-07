#!/usr/bin/env bash
# Local pre-PR check — runs the same gates CI runs (cargo fmt + clippy +
# tests + Python tests) and writes a one-line PASS/FAIL summary you can
# paste into the PR description as proof.
#
# Usage:
#   bash .github/scripts/check.sh            # run everything, print summary
#   bash .github/scripts/check.sh --fast     # skip cargo test (~10× faster)
#   bash .github/scripts/check.sh --output=  # path for full log (default ~/.beava-check.log)
#
# Exit code: 0 if all checks pass, non-zero otherwise.
set -uo pipefail

FAST=0
OUT="$HOME/.beava-check.log"
for arg in "$@"; do
  case "$arg" in
    --fast) FAST=1 ;;
    --output=*) OUT="${arg#--output=}" ;;
    -h|--help)
      sed -n '2,/^set/p' "$0" | sed 's/^# \{0,1\}//; /^set /d'
      exit 0 ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

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

if [[ -d python && -f python/pyproject.toml ]]; then
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
  run "pytest python/tests" \
    bash -c 'cd python && python -m pytest tests/ -q'
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
