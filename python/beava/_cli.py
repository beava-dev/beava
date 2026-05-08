"""``beava`` console script — exec the Rust server binary.

Wired via ``[project.scripts] beava = "beava._cli:main"`` in
``pyproject.toml``. Locates the server binary using the same 4-step
discovery order as embed mode (see ``_embed.discover_binary``) and
``os.execv``s into it, forwarding ``sys.argv[1:]``.

If the binary isn't found, exits with a structured stderr message
listing install paths — never a traceback. Shell scripts that chain
``beava -c ... && next-step`` halt cleanly when the prerequisite is
missing.

The Python pip wheel does NOT bundle the Rust binary. Users who want
``beava -c beava.yaml`` to work must install the binary via Docker
(``docker run beavadev/beava:edge``), Cargo (``cargo install --git
https://github.com/beava-dev/beava beava-server``), or by building
from source. The shim keeps ``pip install beava`` from looking like
a server install when it isn't.
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
