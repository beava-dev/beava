#!/bin/sh
# beava installer — one command, server binary + Python SDK + nothing else.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/beava-dev/beava/main/scripts/install.sh | sh
#
# What it does:
#   1. Detect host platform (Darwin/Linux × arm64/x86_64).
#   2. Find the matching wheel asset on the latest beava GitHub Release.
#   3. `pip install --user` it.
#
# The wheel ships the Rust `beava` binary inside (maturin bin-mode) so
# after install, `beava` is on `~/.local/bin/` and `import beava` works
# in Python. No Rust toolchain needed on the user's box — we did the
# cargo build in CI.
#
# Why GitHub Releases (not PyPI) — beava is pre-PyPI-publish; the
# release-wheels.yml workflow uploads platform wheels to GH Releases
# on every tag. When PyPI publishing lands, this script keeps working
# (the wheels stay on GH) and `pip install beava` becomes the
# preferred form.

set -eu

REPO="${BEAVA_REPO:-beava-dev/beava}"
RELEASE_TAG="${BEAVA_VERSION:-latest}"

# ─── platform detection ───────────────────────────────────────────────
os="$(uname -s)"
arch="$(uname -m)"
case "${os}-${arch}" in
  Darwin-arm64)               wheel_tag="macosx_11_0_arm64" ;;
  Darwin-x86_64)              wheel_tag="macosx_10_12_x86_64" ;;
  Linux-x86_64)               wheel_tag="manylinux_2_17_x86_64.manylinux2014_x86_64" ;;
  Linux-aarch64|Linux-arm64)  wheel_tag="manylinux_2_17_aarch64.manylinux2014_aarch64" ;;
  *)
    printf >&2 "beava installer: unsupported platform %s-%s\n" "$os" "$arch"
    printf >&2 "  Supported: Darwin-arm64, Darwin-x86_64, Linux-x86_64, Linux-aarch64\n"
    printf >&2 "  Or run the server in Docker:\n"
    printf >&2 "    docker run -p 8080:8080 beavadev/beava:edge\n"
    exit 1
    ;;
esac

# ─── prerequisites ────────────────────────────────────────────────────
have() { command -v "$1" >/dev/null 2>&1; }

PIP=""
if have pip3; then PIP=pip3
elif have pip; then PIP=pip
elif have python3; then PIP="python3 -m pip"
else
  printf >&2 "beava installer: pip not found.\n"
  printf >&2 "  Install Python 3.10+ first (https://python.org), then re-run.\n"
  exit 1
fi

if ! have curl; then
  printf >&2 "beava installer: curl not found. Install curl, then re-run.\n"
  exit 1
fi

# ─── locate wheel asset ───────────────────────────────────────────────
# Resolve "latest" through the API. For an explicit version
# (BEAVA_VERSION=v0.0.0), hit the tagged-release endpoint instead.
if [ "$RELEASE_TAG" = "latest" ]; then
  api="https://api.github.com/repos/${REPO}/releases/latest"
else
  api="https://api.github.com/repos/${REPO}/releases/tags/${RELEASE_TAG}"
fi

# Pull just the browser_download_url lines and pattern-match the
# wheel for our platform. Avoids a jq dependency.
url=$(curl -fsSL "$api" \
  | grep -oE '"browser_download_url"[^"]*"[^"]+\.whl"' \
  | sed -E 's/.*"(https[^"]+)".*/\1/' \
  | grep -- "-${wheel_tag}\.whl$" \
  | head -n 1 || true)

if [ -z "$url" ]; then
  printf >&2 "beava installer: no wheel asset matching %s on release %s\n" "$wheel_tag" "$RELEASE_TAG"
  printf >&2 "  Inspected: %s\n" "$api"
  printf >&2 "  This may mean the release for this platform isn't published yet.\n"
  printf >&2 "  Workaround: docker run -p 8080:8080 beavadev/beava:edge\n"
  exit 1
fi

# ─── install ──────────────────────────────────────────────────────────
printf "beava installer: downloading and installing\n"
printf "  platform : %s-%s\n" "$os" "$arch"
printf "  wheel    : %s\n" "$(basename "$url")"
printf "\n"

# `--user` puts the binary at ~/.local/bin/beava (Linux) or
# ~/Library/Python/<ver>/bin/beava (macOS). Either way, the user's
# shell PATH usually picks it up. If it doesn't, --user prints the
# install location for the user to add manually.
$PIP install --user --upgrade "$url"

# ─── post-install message ─────────────────────────────────────────────
beava_bin="$($PIP show beava 2>/dev/null | grep -E '^Location:' | awk '{print $2}')"
printf "\n"
printf "beava installed.\n"
printf "  Try it:    beava --help\n"
printf "  Quickstart: https://beava.dev/docs/quickstart\n"
printf "\n"
if ! have beava; then
  printf "Note: ~/.local/bin (or your Python user-scripts dir) may not be on \$PATH.\n"
  printf "  Add it to your shell rc (e.g. ~/.zshrc):\n"
  printf "    export PATH=\"\$HOME/.local/bin:\$PATH\"\n"
fi
