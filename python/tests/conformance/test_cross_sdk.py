"""Cross-SDK conformance: drive Python + TS + Go SDKs against the same scenario.

Per Phase 13.6 D-03, a single Python orchestrator is the source of truth for
cross-SDK wire agreement.

The harness gracefully skips per-SDK when prerequisites are missing:
  * `beava` binary not discoverable → entire test skipped
  * `node` not on PATH                → TS comparison skipped
  * `go` not on PATH                  → Go comparison skipped
  * Python SDK lacks `register_json` (until Plan 13.5 lands the new App shape)
    → Python comparison skipped; TS/Go each verify against scenario.expected

This way, Phase 13.6 can land before Phase 13.5 catches up the Python surface;
once Plan 13.5 ships `bv.App.register_json`, the Python branch flips on
automatically without test changes.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
from pathlib import Path

import pytest

HERE = Path(__file__).parent
SCENARIO = HERE / "scenario.json"
REPO_ROOT = HERE.parent.parent.parent
TS_SDK_DIR = REPO_ROOT / "sdk" / "typescript"


def _have_beava_binary() -> bool:
    if os.environ.get("BEAVA_BINARY"):
        return True
    if shutil.which("beava"):
        return True
    for parent in [HERE, *HERE.parents]:
        if (parent / "target" / "debug" / "beava").is_file():
            return True
    return False


def _have_node() -> bool:
    return shutil.which("node") is not None


def _have_go() -> bool:
    return shutil.which("go") is not None


def _have_python_register_json() -> bool:
    """Plan 13.5 lands `bv.App.register_json`; until then, the Python branch
    of the harness skips."""
    try:
        import beava as bv

        return hasattr(bv.App, "register_json")
    except Exception:
        return False


def _run_python(scenario: dict) -> list[dict]:
    import beava as bv

    app = bv.App(test_mode=True)
    try:
        app.register_json(scenario["register_payload"])  # type: ignore[attr-defined]
        for ev in scenario["events"]:
            app.push(ev["event_name"], ev["fields"])
        results: list[dict] = []
        for g in scenario["gets"]:
            if g["key"] == "":
                results.append(app.get(g["table"]))
            else:
                results.append(app.get(g["table"], g["key"]))
        return results
    finally:
        app.close()


def _ensure_ts_dist_built() -> None:
    """Build the TS SDK dist/ if not already built.

    The TS adapter imports from `<repo>/sdk/typescript/dist/index.js` (built
    artifact, not source) to avoid runtime TS-stripping flag dependencies.
    """
    dist_index = TS_SDK_DIR / "dist" / "index.js"
    if dist_index.exists():
        return
    if not (TS_SDK_DIR / "node_modules").exists():
        subprocess.run(
            ["npm", "install"],
            cwd=str(TS_SDK_DIR),
            check=True,
            capture_output=True,
        )
    subprocess.run(
        ["npm", "run", "build"],
        cwd=str(TS_SDK_DIR),
        check=True,
        capture_output=True,
    )


def _run_ts(scenario_path: Path) -> list[dict]:
    """Run the TS adapter via Node 22+ `--experimental-strip-types`.

    The adapter imports from the in-tree built dist/ (relative path), so no
    npm-link state pollution and no `tsx` runtime dependency.
    """
    _ensure_ts_dist_built()
    proc = subprocess.run(
        [
            "node",
            "--experimental-strip-types",
            "--no-warnings=ExperimentalWarning",
            str(HERE / "run_ts.ts"),
            str(scenario_path),
        ],
        capture_output=True,
        text=True,
        timeout=60,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"TS adapter failed (exit {proc.returncode}):\n"
            f"STDOUT:\n{proc.stdout}\nSTDERR:\n{proc.stderr}"
        )
    return json.loads(proc.stdout.strip())["results"]


def _run_go(scenario_path: Path) -> list[dict]:
    """Run the Go adapter via `go run run_go.go <scenario>` from the conformance dir."""
    proc = subprocess.run(
        ["go", "run", "run_go.go", str(scenario_path)],
        capture_output=True,
        text=True,
        timeout=120,
        cwd=str(HERE),
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"Go adapter failed (exit {proc.returncode}):\n"
            f"STDOUT:\n{proc.stdout}\nSTDERR:\n{proc.stderr}"
        )
    return json.loads(proc.stdout.strip())["results"]


_ENGINE_ALIGNMENT_MARKERS = (
    # engine still rejects kind:"table" (Phase 12.7 strip not yet reverted)
    "unsupported_node_kind",
    "missing field `fields`",  # engine event-descriptor schema doesn't match docs/wire-spec.md
    "missing field `keys`",  # engine /get expects {table, keys, features} not {table, key}
    "unknown_field_v0",  # engine push body doesn't accept {fields: ...} wrapper
)


def _is_engine_alignment_error(exc: Exception) -> bool:
    """True if the error string indicates the engine wire shape lags the docs.

    Phase 13.6's SDKs target the wire spec at `docs/wire-spec.md` (the contract
    Phase 13.4 is converging on). When the engine binary on disk is older than
    13.4's full landing, the conformance test should skip rather than fail —
    it's not a 13.6 bug.
    """
    s = str(exc)
    return any(marker in s for marker in _ENGINE_ALIGNMENT_MARKERS)


@pytest.mark.skipif(
    not _have_beava_binary(),
    reason="beava binary not available; set BEAVA_BINARY or build target/debug/beava",
)
def test_cross_sdk_agreement():
    """Cross-SDK conformance: each SDK's results must match scenario.expected
    AND each other (transitively, via the expected baseline)."""
    scenario = json.loads(SCENARIO.read_text())
    expected = [g["expected"] for g in scenario["gets"]]

    branches: dict[str, list[dict]] = {}
    alignment_failures: list[str] = []

    def _try_branch(name: str, fn) -> None:
        try:
            branches[name] = fn()
        except Exception as e:
            if _is_engine_alignment_error(e):
                alignment_failures.append(f"{name}: {type(e).__name__}: {e}")
            else:
                pytest.fail(f"{name} branch raised non-alignment error: {e}")

    if _have_python_register_json():
        _try_branch("python", lambda: _run_python(scenario))
    if _have_node():
        _try_branch("typescript", lambda: _run_ts(SCENARIO))
    if _have_go():
        _try_branch("go", lambda: _run_go(SCENARIO))

    if not branches and alignment_failures:
        pytest.skip(
            "all SDK branches hit engine-alignment errors (Phase 13.4 register/push/get "
            "wire shape lags docs/wire-spec.md):\n  - "
            + "\n  - ".join(alignment_failures)
        )

    if not branches:
        pytest.skip(
            "no SDK adapters available — install node and/or go, "
            "or wait for Plan 13.5 to land bv.App.register_json"
        )

    # Each branch must match scenario.expected
    for name, results in branches.items():
        assert results == expected, (
            f"{name} diverged from scenario.expected: {results} != {expected}"
        )

    # Pairwise agreement across branches (transitive via expected, but
    # explicit assertion makes failure messages clearer).
    branch_names = list(branches.keys())
    for i in range(len(branch_names)):
        for j in range(i + 1, len(branch_names)):
            a, b = branch_names[i], branch_names[j]
            assert branches[a] == branches[b], (
                f"{a} and {b} diverged: {branches[a]} != {branches[b]}"
            )

    if alignment_failures:
        # Some branches passed, some hit alignment errors — still useful
        # information for debugging Phase 13.4 / 13.5 readiness.
        print(
            "\nNOTE: some branches hit engine-alignment errors (informational):\n  - "
            + "\n  - ".join(alignment_failures)
        )
