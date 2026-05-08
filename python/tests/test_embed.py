"""Tests for beava._embed and parse_url_to_transport URL dispatch.

Tests that use the `beava_binary` fixture will start the binary.
Tests that use monkeypatching exercise discovery logic without the real binary.
These tests are expected to FAIL (ImportError) until _embed.py and _transport.py
are created in Task 1.b.
"""

from __future__ import annotations

import stat
from pathlib import Path

import pytest

from beava._embed import BinaryNotFoundError, discover_binary, spawn_embedded_server
from beava._transport import (
    EmbedTransport,
    HttpTransport,
    TcpTransport,
    parse_url_to_transport,
)


class TestDiscoverBinary:
    def test_discover_binary_from_env_var_missing_file(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """BEAVA_BINARY set to a non-existent path raises BinaryNotFoundError immediately.

        If the user explicitly set BEAVA_BINARY, we must use that path or fail —
        we do NOT silently fall through to PATH / ./target/debug.
        """
        monkeypatch.setenv("BEAVA_BINARY", str(tmp_path / "no_such_file"))
        with pytest.raises(BinaryNotFoundError) as exc_info:
            discover_binary()
        assert "BEAVA_BINARY" in str(exc_info.value)

    def test_discover_binary_prefers_env_var_when_valid(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """BEAVA_BINARY set to a valid executable file returns that path."""
        dummy = tmp_path / "fake_beava"
        dummy.write_bytes(b"#!/bin/sh\necho hi")
        dummy.chmod(dummy.stat().st_mode | stat.S_IEXEC)

        monkeypatch.setenv("BEAVA_BINARY", str(dummy))
        found = discover_binary()
        assert found == dummy

    def test_discover_binary_falls_back_to_target_debug(
        self, monkeypatch: pytest.MonkeyPatch, beava_binary: Path
    ) -> None:
        """Without BEAVA_BINARY, discovers ./target/debug/beava from CWD walk."""
        monkeypatch.delenv("BEAVA_BINARY", raising=False)
        # beava_binary fixture already built the binary; cwd is repo root
        found = discover_binary()
        # Must resolve to an actual file at target/debug/beava
        assert found.name == "beava"
        assert found.is_file()

    def test_discover_binary_raises_when_not_found(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """No BEAVA_BINARY, no beava on PATH, no target/debug/beava → BinaryNotFoundError."""
        monkeypatch.delenv("BEAVA_BINARY", raising=False)
        monkeypatch.chdir(tmp_path)  # chdir to a dir with no target/debug/beava
        # Remove beava from PATH by patching shutil.which
        import beava._embed as embed_mod

        monkeypatch.setattr(
            embed_mod.shutil, "which", lambda _name, *_a, **_k: None
        )
        with pytest.raises(BinaryNotFoundError) as exc_info:
            discover_binary()
        msg = str(exc_info.value).lower()
        assert "install" in msg or "brew" in msg or "not found" in msg

    def test_discover_binary_skips_shebang_script_on_path(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        beava_binary: Path,
    ) -> None:
        """When `shutil.which("beava")` returns a Python shim shebang script
        (added by `pip install beava` via `[project.scripts]`), discovery
        MUST fall through past it. Otherwise embed-mode invokes the shim,
        which calls `discover_binary()` again, finds itself, and execs back
        — infinite loop.

        After skipping the shim, discovery falls through to the workspace
        target/debug/beava (provided by the `beava_binary` fixture).
        """
        monkeypatch.delenv("BEAVA_BINARY", raising=False)

        # Drop a fake `beava` shim that looks exactly like what
        # `pip install -e .` produces from `[project.scripts]`: shebang
        # pointing at python, then the entry-point loader code.
        shim = tmp_path / "beava"
        shim.write_text(
            "#!/usr/bin/env python3\n"
            "from beava._cli import main\n"
            "if __name__ == '__main__':\n"
            "    raise SystemExit(main())\n"
        )
        shim.chmod(shim.stat().st_mode | stat.S_IEXEC)

        import beava._embed as embed_mod

        monkeypatch.setattr(
            embed_mod.shutil, "which", lambda _name, *_a, **_k: str(shim)
        )

        found = discover_binary()
        # Must NOT be the shim — that would create an exec loop.
        assert found != shim, (
            f"discover_binary returned the Python shim {shim}; "
            "must skip shebang scripts and fall through to "
            "target/debug/beava."
        )
        # Must be the real Rust binary from the workspace target.
        assert found == beava_binary, (
            f"expected fall-through to {beava_binary}, got {found}"
        )


class TestSpawnEmbeddedServer:
    def test_spawn_embedded_server_parses_ports(self, beava_binary: Path) -> None:
        """spawn_embedded_server() returns (proc, http_url, tcp_url) with OS-assigned ports."""
        proc, http_url, tcp_url = spawn_embedded_server()
        try:
            assert http_url.startswith("http://")
            assert tcp_url.startswith("tcp://")

            # Ports must be non-default (OS assigns ephemeral ports)
            http_port = int(http_url.split(":")[-1])
            tcp_port = int(tcp_url.split(":")[-1])
            assert http_port > 0, f"http port is {http_port}"
            assert tcp_port > 0, f"tcp port is {tcp_port}"
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5.0)
            except Exception:
                proc.kill()
                proc.wait()

    def test_embedded_server_timeout_on_missing_bind_line(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """When binary never emits bind log lines, spawn_embedded_server raises within timeout."""
        import beava._embed as embed_mod

        # Monkeypatch discover_binary to return /bin/sleep (sleeps, never logs bind lines)
        monkeypatch.setattr(embed_mod, "discover_binary", lambda: Path("/bin/sleep"))
        with pytest.raises((TimeoutError, RuntimeError)) as exc_info:
            spawn_embedded_server(startup_timeout=1.0)
        msg = str(exc_info.value).lower()
        assert "timeout" in msg or "did not bind" in msg or "bind" in msg


class TestParseUrlToTransport:
    def test_parse_url_to_transport_http(self) -> None:
        """http:// URL returns an HttpTransport with the correct base_url."""
        t = parse_url_to_transport("http://localhost:7379")
        assert isinstance(t, HttpTransport)
        assert t.base_url == "http://localhost:7379"
        t.close()

    def test_parse_url_to_transport_https(self) -> None:
        """https:// URL returns an HttpTransport (TLS handled by httpx)."""
        t = parse_url_to_transport("https://api.example.com")
        assert isinstance(t, HttpTransport)
        t.close()

    def test_parse_url_to_transport_tcp(self) -> None:
        """tcp:// URL returns a TcpTransport with correct host and port."""
        t = parse_url_to_transport("tcp://localhost:7380")
        assert isinstance(t, TcpTransport)
        assert t.host == "localhost"
        assert t.port == 7380
        # Don't call close() — no socket opened yet (lazy connect)

    def test_parse_url_to_transport_none_triggers_embed(
        self, beava_binary: Path
    ) -> None:
        """parse_url_to_transport(None) returns an EmbedTransport wrapping a TcpTransport."""
        t = parse_url_to_transport(None)
        assert isinstance(t, EmbedTransport)
        t.close()

    def test_parse_url_to_transport_invalid_scheme(self) -> None:
        """Unknown URL scheme raises ValueError mentioning supported schemes."""
        with pytest.raises(ValueError) as exc_info:
            parse_url_to_transport("ws://foo:1234")
        msg = str(exc_info.value).lower()
        assert "http" in msg or "tcp" in msg or "scheme" in msg or "supported" in msg

    def test_parse_url_to_transport_empty_scheme_raises(self) -> None:
        """Scheme-less string raises ValueError."""
        with pytest.raises(ValueError):
            parse_url_to_transport("localhost:7379")
