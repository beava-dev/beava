"""Smoke coverage for ``beava.cli`` — the placeholder entry-point module.

``beava.cli`` reserves the console-script entry-point ID; the runnable
fallback CLI lives in ``beava._cli`` (covered by ``test_cli.py``). For
now ``beava.cli.main()`` only documents itself as not-wired-yet and
raises ``SystemExit`` with a clear message. The contract is small but
worth locking — when subcommands ship, accidentally exposing a partial
surface would break this test.
"""
from __future__ import annotations

import pytest


def test_cli_module_exposes_main() -> None:
    """``beava.cli`` must export a callable ``main`` and list it in ``__all__``."""
    import beava.cli as cli_mod

    assert hasattr(cli_mod, "main"), "beava.cli must expose main()"
    assert callable(cli_mod.main)
    assert "main" in cli_mod.__all__


def test_cli_main_raises_system_exit_until_wired() -> None:
    """Until the argparse subcommand graph is wired, ``main()`` must raise
    ``SystemExit`` so a bare ``python -c 'from beava.cli import main; main()'``
    fails loudly rather than returning ``None`` silently."""
    from beava.cli import main

    with pytest.raises(SystemExit) as exc_info:
        main()
    # The placeholder message must be on the SystemExit value so users
    # invoking the bare module see why it stopped.
    assert "not wired" in str(exc_info.value)
