"""Tests for the Python-side `beava._cli` fallback (`python -m beava._cli`).

From v0.4.0, the pip-installed `beava` shell command is the maturin-bundled
Rust server binary itself — `[project.scripts]` no longer wires a Python
shim. `beava._cli` survives as a manual fallback runnable via
`python -m beava._cli`; it must locate the server binary via the same
discovery order as embed mode (`$BEAVA_BINARY` → wheel-bundled binary in
`<sysconfig.get_path("scripts")>` → `$PATH` → workspace
`target/{release,debug}/beava`) and exec into it, forwarding argv.
Failure to find a binary must produce a structured stderr message +
non-zero exit, NOT a stack trace.
"""
from __future__ import annotations

import sys
from pathlib import Path
from unittest.mock import patch

import pytest

from beava._errors import BinaryNotFoundError


def test_main_execs_discovered_binary_with_forwarded_argv() -> None:
    """Happy path: discover the binary, exec into it with forwarded argv."""
    from beava import _cli

    fake_path = Path("/usr/local/bin/beava")
    captured: dict[str, object] = {}

    def fake_execv(path: object, argv: list[str]) -> None:
        captured["path"] = path
        captured["argv"] = argv
        # Simulate the never-returns nature of execv by raising a sentinel
        # the test catches.
        raise SystemExit(0)

    with (
        patch.object(_cli, "discover_binary", return_value=fake_path),
        patch.object(_cli.os, "execv", side_effect=fake_execv),
        patch.object(sys, "argv", ["beava", "-c", "beava.yaml", "--port", "9000"]),
    ):
        with pytest.raises(SystemExit) as exc_info:
            _cli.main()
        assert exc_info.value.code == 0

    assert captured["path"] == str(fake_path)
    # argv[0] must be the binary path (POSIX convention); argv[1:] forwards
    # everything after the wrapper's own program name.
    assert captured["argv"] == [str(fake_path), "-c", "beava.yaml", "--port", "9000"]


def test_main_no_args_still_execs() -> None:
    """`beava` with zero args must exec the binary with no extra args."""
    from beava import _cli

    fake_path = Path("/opt/beava/bin/beava")
    captured: dict[str, object] = {}

    def fake_execv(path: object, argv: list[str]) -> None:
        captured["path"] = path
        captured["argv"] = argv
        raise SystemExit(0)

    with (
        patch.object(_cli, "discover_binary", return_value=fake_path),
        patch.object(_cli.os, "execv", side_effect=fake_execv),
        patch.object(sys, "argv", ["beava"]),
    ):
        with pytest.raises(SystemExit):
            _cli.main()

    assert captured["argv"] == [str(fake_path)]


def test_main_binary_not_found_clean_exit(capsys: pytest.CaptureFixture[str]) -> None:
    """`BinaryNotFoundError` must surface as a structured stderr message
    + non-zero exit, NOT an uncaught exception traceback. Users who ran
    `pip install beava` without the server installed get a clear next
    step."""
    from beava import _cli

    err_msg = (
        "beava binary not found. Install with one of:\n"
        "  docker run beavadev/beava:edge\n"
        "  cargo install --git https://github.com/beava-dev/beava beava-server\n"
        "Or set BEAVA_BINARY=/path/to/beava."
    )

    with (
        patch.object(_cli, "discover_binary", side_effect=BinaryNotFoundError(err_msg)),
        patch.object(sys, "argv", ["beava", "-c", "beava.yaml"]),
    ):
        with pytest.raises(SystemExit) as exc_info:
            _cli.main()

    # Non-zero exit code so shell scripts (`beava -c ... && next-step`)
    # halt cleanly instead of barreling on after a missing binary.
    assert isinstance(exc_info.value.code, int) and exc_info.value.code != 0
    captured = capsys.readouterr()
    # The error message MUST land on stderr (Unix convention; stdout
    # could be piped to a config consumer).
    assert "beava binary not found" in captured.err
    assert "BEAVA_BINARY" in captured.err
    # And NOT on stdout — clean separation.
    assert "beava binary not found" not in captured.out


def test_pyproject_declares_maturin_bundled_binary() -> None:
    """`pip install beava` must ship the Rust server binary directly.
    The build backend is maturin in `bindings = "bin"` mode pointed at
    the workspace `crates/beava-server/Cargo.toml`, with `bins =
    ["beava"]` filtering out dev-only second binaries (log_probe).
    These three together are the contract that produces a `beava`
    shell command at `<sysconfig.get_path("scripts")>/beava` after
    install — without them, `pip install beava` ships only the SDK
    and the bundled-server promise on the homepage breaks."""
    # tomllib is stdlib on Python 3.11+; on 3.10 we fall back to the
    # text-mode regex contract below (the package supports 3.10).
    try:
        import tomllib
    except ImportError:
        pyproject = Path(__file__).resolve().parents[2] / "pyproject.toml"
        text = pyproject.read_text()
        assert 'build-backend = "maturin"' in text
        assert 'bindings = "bin"' in text
        assert '"beava"' in text and "bins = " in text
        # `[project.scripts]` may exist for other entries; what matters
        # is that no line wires `beava = ...` under it.
        assert "[project.scripts]" not in text or "beava = " not in text
        return

    pyproject = Path(__file__).resolve().parents[2] / "pyproject.toml"
    cfg = tomllib.loads(pyproject.read_text())

    assert cfg["build-system"]["build-backend"] == "maturin", (
        "python/pyproject.toml build-backend must be 'maturin' — the "
        "Rust server binary ships in the wheel via maturin's bin mode."
    )

    maturin = cfg.get("tool", {}).get("maturin", {})
    assert maturin.get("bindings") == "bin", (
        "[tool.maturin] bindings must be 'bin' — without it the wheel "
        "would build a C-extension shim instead of the server binary."
    )
    assert "beava" in maturin.get("bins", []), (
        "[tool.maturin].bins must include 'beava' — the production "
        "server binary that becomes the `beava` shell command after "
        "pip install."
    )

    # `[project.scripts]` must NOT wire a Python `beava` console script:
    # the maturin-bundled native binary IS the `beava` shell command. A
    # console_script shim of the same name would shadow the binary in
    # the wheel's scripts/ directory and reintroduce the exec-loop risk
    # that `_embed._is_shebang_script` defends against.
    project_scripts = cfg.get("project", {}).get("scripts", {})
    assert "beava" not in project_scripts, (
        "[project.scripts] must not declare a `beava` entry — the "
        "maturin bundled binary IS the shell command."
    )
