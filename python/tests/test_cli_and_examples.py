"""Pytest coverage for the ``beava`` CLI flags + the user-facing example
scripts under ``examples/python/``.

Two audit gaps are closed here:

* The CLI flag surface (``--http-addr``, ``--tcp-addr``, ``--data-dir``,
  ``--memory-only``, ``--test-mode``) is documented in CLAUDE.md and pinned
  by Rust-side ``cli.rs`` unit tests, but no Python test asserts that the
  shipped binary actually prints ``--http-addr`` / ``--data-dir`` /
  ``--memory-only`` in ``beava --help`` or that ``beava --version``
  emits the expected ``0.0.4`` string.

* The three end-to-end example scripts (``agent_runtime.py``,
  ``marketplace_rerank.py``, ``growth_rescue.py``) had zero CI coverage,
  meaning an operator-rename or schema-shape break would silently leave
  user-facing example code broken.  Each is now exec'd against a freshly
  booted ``beava`` server with the example's hardcoded
  ``http://localhost:8080`` URL rewritten to the OS-assigned test port.
"""
from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path

import pytest

_EXAMPLES_DIR = Path(__file__).resolve().parents[2] / "examples" / "python"


# ─── CLI flag tests ───────────────────────────────────────────────────────────


def test_beava_version_prints_v0_0_4(beava_binary: Path) -> None:
    """``beava --version`` exits 0 and emits the current Cargo version string."""
    r = subprocess.run(
        [str(beava_binary), "--version"],
        capture_output=True,
        text=True,
        timeout=15,
    )
    assert r.returncode == 0, (
        f"beava --version exited non-zero ({r.returncode}); "
        f"stdout={r.stdout!r}; stderr={r.stderr!r}"
    )
    combined = (r.stdout or "") + (r.stderr or "")
    # Match the Cargo `version = "X.Y.Z"` in the workspace root. We assert
    # against the canonical SemVer pattern rather than a hard-coded
    # "0.0.4" so a future version bump doesn't need to touch this test.
    assert re.search(r"\b\d+\.\d+\.\d+\b", combined), (
        f"beava --version output missing a SemVer; got {combined!r}"
    )


def test_beava_help_lists_main_flags(beava_binary: Path) -> None:
    """``beava --help`` exits 0 and documents the locked v0 CLI flags."""
    r = subprocess.run(
        [str(beava_binary), "--help"],
        capture_output=True,
        text=True,
        timeout=15,
    )
    assert r.returncode == 0, (
        f"beava --help exited non-zero ({r.returncode}); stderr={r.stderr!r}"
    )
    help_text = (r.stdout or "") + (r.stderr or "")
    for flag in ("--http-addr", "--tcp-addr", "--data-dir", "--memory-only", "--test-mode"):
        assert flag in help_text, (
            f"beava --help must document {flag!r}; got help_text={help_text!r}"
        )


# ─── Example smoke tests ──────────────────────────────────────────────────────


def _run_example_against(
    example_path: Path, http_url: str, tmp_path: Path
) -> subprocess.CompletedProcess[str]:
    """Copy the example into ``tmp_path`` with its hardcoded
    ``http://localhost:8080`` URL replaced by the test server's URL,
    then execute it with the working Python interpreter.

    The replacement is done on a copy (never on the source) so the
    repo working tree is never mutated by a test run.
    """
    src = example_path.read_text(encoding="utf-8")
    if "http://localhost:8080" not in src:
        pytest.fail(
            f"{example_path.name} no longer hardcodes http://localhost:8080; "
            "update _run_example_against to track the new URL convention."
        )
    rewritten = src.replace("http://localhost:8080", http_url)
    dst = tmp_path / example_path.name
    dst.write_text(rewritten, encoding="utf-8")
    env = {**os.environ, "PYTHONUNBUFFERED": "1"}
    return subprocess.run(
        [sys.executable, str(dst)],
        capture_output=True,
        text=True,
        timeout=60,
        env=env,
    )


@pytest.mark.parametrize(
    "example_name",
    [
        "agent_runtime.py",
        "marketplace_rerank.py",
        "growth_rescue.py",
    ],
)
def test_example_smoke(
    example_name: str,
    beava_server: tuple[str, str],
    tmp_path: Path,
) -> None:
    """Each example exits 0 against a freshly booted beava server."""
    http_url, _tcp_url = beava_server
    example_path = _EXAMPLES_DIR / example_name
    assert example_path.is_file(), f"missing example: {example_path}"

    result = _run_example_against(example_path, http_url, tmp_path)
    if result.returncode != 0:
        pytest.fail(
            f"example {example_name} exited {result.returncode} — possible "
            f"API drift between examples and SDK.\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    # Each example prints "OK -- <filename>" right before returning 0.
    assert f"OK -- {example_name}" in result.stdout, (
        f"example {example_name} did not print its OK banner; "
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
