"""``beava`` Python-side fallback — exec the Rust server binary.

From v0.4.0 onward, the maturin-built wheel ships the Rust ``beava``
binary directly into ``<sysconfig.get_path("scripts")>/beava`` — that
binary IS the ``beava`` shell command after ``pip install beava``. This
module is no longer wired via ``[project.scripts]``.

The shim is retained as a manual fallback runnable as
``python -m beava._cli``: it locates the server binary using the
same 5-step discovery order as embed mode (see
``_embed.discover_binary``) and ``os.execv``s into it, forwarding
``sys.argv[1:]``. Useful when:

- A user's PATH ordering shadows the wheel's ``beava`` and they want
  embed-mode discovery semantics from the command line.
- An editable install (``maturin develop`` or ``pip install -e .``)
  hasn't placed the binary in scripts/ yet but a ``cargo build``
  binary lives under the workspace ``target/`` tree.

If the binary isn't found, exits with a structured stderr message
listing install paths — never a traceback. Shell scripts that chain
``python -m beava._cli -c ... && next-step`` halt cleanly when the
prerequisite is missing.
"""
from __future__ import annotations

import os
import sys

from beava._embed import discover_binary
from beava._errors import BinaryNotFoundError


def main() -> int:
    """Locate the beava binary and ``os.execv`` into it.

    Returns:
        Never returns on the happy path (``execv`` replaces the
        process). Returns ``2`` if the binary discovery fails.
    """
    try:
        binary = discover_binary()
    except BinaryNotFoundError as e:
        # Print to stderr so stdout stays clean for downstream consumers
        # (e.g. ``beava ... | jq``). Use the full discovery error
        # message verbatim — it already names the install paths.
        print(str(e), file=sys.stderr)
        # Exit code 2 ("misuse of shell builtin" / missing prerequisite)
        # — matches conventions for "command found but can't proceed".
        sys.exit(2)

    # POSIX convention: argv[0] of the exec'd process is the program
    # itself. Forward everything after our own program name verbatim.
    binary_str = str(binary)
    os.execv(binary_str, [binary_str, *sys.argv[1:]])
    # Unreachable — execv replaces the process. Returning is here only
    # to satisfy the type checker in case execv is mocked.
    return 0


if __name__ == "__main__":
    main()
