"""Tests for the `beava` console script (`pip install beava` → shell command).

The pip-installed `beava` shim must locate the server binary via the same
discovery order as embed mode (`$BEAVA_BINARY` → `$PATH` → workspace
`target/debug/beava`) and exec into it, forwarding argv. Failure to find
a binary must produce a structured stderr message + non-zero exit, NOT
a stack trace.
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


def test_pyproject_declares_console_script() -> None:
    """`pip install beava` must put a `beava` shell command on the
    user's PATH. The console_script entry in pyproject.toml is the
    contract; if it's missing, `pip install` ships only the library
    and the user has to install the Rust binary separately to run
    the server."""
    pyproject = Path(__file__).resolve().parents[2] / "pyproject.toml"
    text = pyproject.read_text()
    # Hand-rolled grep instead of pulling tomllib — the contract is one
    # line and we want to fail fast if it gets removed.
    assert "[project.scripts]" in text, (
        "python/pyproject.toml is missing the [project.scripts] table; "
        "pip install beava will not install a `beava` shell command."
    )
    assert 'beava = "beava._cli:main"' in text, (
        "python/pyproject.toml [project.scripts] does not declare "
        '`beava = "beava._cli:main"` — the shim won\'t wire up.'
    )
