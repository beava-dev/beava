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
    """`pip install beava` must ship ONLY the production `beava` binary,
    not the dev-only `log_probe`. The contract has three layers:

    1. python/pyproject.toml: build-backend = maturin, bindings = "bin",
       and NO `[project.scripts] beava` (the native bundled binary IS
       the shell command — a Python shim would shadow it).
    2. crates/beava-server/Cargo.toml: a `dev-tools` Cargo feature
       gates the log_probe bin (`required-features = ["dev-tools"]`)
       so default cargo + maturin builds skip it. Maturin 1.13.x has
       no pyproject-level bin filter, so the gate has to live at the
       Cargo level.
    3. [project.scripts] does NOT declare `beava` — the maturin native
       binary occupies that name slot directly."""
    # tomllib is stdlib on Python 3.11+; on 3.10 we fall back to the
    # text-mode regex contract below (the package supports 3.10).
    # Skip when the source tree isn't reachable from the test file's
    # location (e.g. tests mounted standalone into a Docker validation
    # container that only mounts `tests/v0/`). A wheel that survived
    # CI's full v0 suite already passed this contract — running it
    # again from the install side has no value.
    test_file = Path(__file__).resolve()
    if len(test_file.parents) < 4:
        pytest.skip("source tree not available; build contract is a CI-time gate")
    repo_root = test_file.parents[3]
    pyproject_path = repo_root / "python" / "pyproject.toml"
    cargo_path = repo_root / "crates" / "beava-server" / "Cargo.toml"
    if not pyproject_path.exists() or not cargo_path.exists():
        pytest.skip("source tree not available; build contract is a CI-time gate")

    try:
        import tomllib
    except ImportError:
        text = pyproject_path.read_text()
        assert 'build-backend = "maturin"' in text
        assert 'bindings = "bin"' in text
        # `[project.scripts]` may exist for other entries; what matters
        # is that no line wires `beava = ...` under it.
        assert "[project.scripts]" not in text or "beava = " not in text
        # Cargo-level gate on log_probe (text-mode contract).
        cargo_toml = cargo_path.read_text()
        assert 'name = "log_probe"' in cargo_toml
        assert 'required-features = ["dev-tools"]' in cargo_toml
        return

    cfg = tomllib.loads(pyproject_path.read_text())

    assert cfg["build-system"]["build-backend"] == "maturin", (
        "python/pyproject.toml build-backend must be 'maturin' — the "
        "Rust server binary ships in the wheel via maturin's bin mode."
    )

    maturin = cfg.get("tool", {}).get("maturin", {})
    assert maturin.get("bindings") == "bin", (
        "[tool.maturin] bindings must be 'bin' — without it the wheel "
        "would build a C-extension shim instead of the server binary."
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

    # The dev-tools Cargo feature gates log_probe out of the wheel.
    # Without this gate, `pip install beava` would land a `log_probe`
    # binary on the user's PATH alongside `beava` — Beava's wheel
    # stays narrow.
    cargo_cfg = tomllib.loads(cargo_path.read_text())
    bins = [b for b in cargo_cfg.get("bin", []) if b.get("name") == "log_probe"]
    assert bins, "crates/beava-server must declare a [[bin]] log_probe target"
    assert bins[0].get("required-features") == ["dev-tools"], (
        "log_probe must be gated by `required-features = ['dev-tools']` "
        "so the maturin-built wheel doesn't ship it."
    )
    features = cargo_cfg.get("features", {})
    assert "dev-tools" in features, (
        "crates/beava-server [features] must declare `dev-tools` for "
        "the log_probe gate to compile."
    )
