"""Transport layer for the Beava Python SDK.

- :class:`Transport` — abstract interface implemented by each backend.
- :class:`HttpTransport` — ``httpx.Client`` over HTTP/HTTPS.
- :class:`TcpTransport` — stdlib ``socket`` with the binary frame codec.
- :class:`EmbedTransport` — wraps :class:`TcpTransport` plus a subprocess
  handle for embed mode.
- :func:`make_transport` / :func:`parse_url_to_transport` — URL-scheme
  dispatch.

URL dispatch:

- ``http://...`` / ``https://...`` → :class:`HttpTransport`
- ``tcp://...`` → :class:`TcpTransport`
- ``None`` → :class:`EmbedTransport` (spawn local binary)
- anything else → :exc:`ValueError`
"""

from __future__ import annotations

import json
import socket
import subprocess
import urllib.parse
from typing import TYPE_CHECKING, Any, cast

import httpx

from beava._errors import RegistrationError
from beava._wire import (
    CT_JSON,
    CT_MSGPACK,
    MAX_FRAME_BYTES,
    OP_BATCH_GET,
    OP_GET,
    OP_GET_RESPONSE,
    OP_PING,
    OP_PUSH,
    OP_REGISTER,
    OP_RESET,
    encode_frame,
    parse_register_response,
    read_frame,
)

if TYPE_CHECKING:
    from types import TracebackType


class Transport:
    """Abstract transport interface (duck-typed).

    Concrete backends implement the seven ``send_*`` methods plus
    :meth:`close` and the context-manager protocol. The transport owns the
    wire-format choice (JSON over HTTP, default JSON over TCP / embed); the
    :class:`App` only constructs Python-shaped payloads. Methods that a
    backend doesn't support raise ``NotImplementedError`` from this base.
    """

    def send_register(self, payload_json: bytes) -> dict[str, Any]:
        raise NotImplementedError

    def send_push(
        self,
        *,
        event_name: str,
        fields: dict[str, Any],
    ) -> dict[str, Any]:
        raise NotImplementedError

    def send_get(
        self,
        *,
        table: str,
        key: str | list[Any],
        features: list[str] | None = None,
    ) -> dict[str, Any]:
        raise NotImplementedError

    def send_batch_get(
        self,
        *,
        requests: list[
            tuple[str, str | list[Any]]
            | tuple[str, str | list[Any], list[str] | None]
        ],
    ) -> list[dict[str, Any]]:
        raise NotImplementedError

    def send_reset(self) -> None:
        raise NotImplementedError

    def send_ping(self) -> dict[str, Any]:
        raise NotImplementedError

    def close(self) -> None:
        raise NotImplementedError

    def __enter__(self) -> "Transport":
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: "TracebackType | None",
    ) -> None:
        self.close()


class HttpTransport(Transport):
    """Send register requests via HTTP/JSON using ``httpx.Client``.

    The client is long-lived; reuse across multiple ``send_register`` calls
    on the same App instance is safe (httpx connection-pools under the hood).

    Args:
        base_url: Server base URL, e.g. ``"http://localhost:7379"``.
        timeout: Request timeout in seconds.
    """

    def __init__(self, base_url: str, timeout: float = 30.0) -> None:
        self.base_url = base_url.rstrip("/")
        self._client = httpx.Client(base_url=self.base_url, timeout=timeout)

    def send_register(self, payload_json: bytes) -> dict[str, Any]:
        """POST ``/register`` with a JSON payload.

        Args:
            payload_json: UTF-8 JSON bytes matching the wire contract.

        Returns:
            Parsed success body dict (``status='ok'``,
            ``registry_version=N``, ...).

        Raises:
            RegistrationError: Server returned 4xx or 5xx with a JSON
                error body.
        """
        r = self._client.post(
            "/register",
            content=payload_json,
            headers={"Content-Type": "application/json"},
        )
        body = cast(dict[str, Any], r.json())
        if r.status_code == 200:
            return body
        error = body.get("error", {})
        raise RegistrationError(
            code=error.get("code", "unknown"),
            path=error.get("path", ""),
            message=error.get("reason") or error.get("message", ""),
            errors=[],
        )

    def send_push(
        self,
        *,
        event_name: str,
        fields: dict[str, Any],
    ) -> dict[str, Any]:
        """POST ``/push`` with verb-style body ``{event, data}``.

        The wire body shape is ``{"event": <name>, "data": {<fields>}}`` —
        matches the server parser. (The HTTP-API doc currently says
        ``{event_name, fields}``; the SDK follows the impl, the doc is
        scheduled for an erratum.)

        Returns:
            Parsed success body dict (``{"ack_lsn": int, ...}``).

        Raises:
            RegistrationError: Server returned non-2xx with a JSON error body.
        """
        body_bytes = json.dumps(
            {"event": event_name, "data": fields}, ensure_ascii=False
        ).encode("utf-8")
        r = self._client.post(
            "/push",
            content=body_bytes,
            headers={"Content-Type": "application/json"},
        )
        body = cast(dict[str, Any], r.json())
        if r.status_code == 200:
            return body
        error = body.get("error", {})
        raise RegistrationError(
            code=error.get("code", "unknown"),
            path=error.get("path", ""),
            message=error.get("reason") or error.get("message", ""),
            errors=[],
        )

    def send_get(
        self,
        *,
        table: str,
        key: str | list[Any],
        features: list[str] | None = None,
    ) -> dict[str, Any]:
        """POST ``/get`` with verb-style body ``{table, key, features?}``.

        Returns a row-shape flat dict (cold-start = ``{}``). The
        ``features`` filter is included on the wire when non-None and
        omitted otherwise (full row).

        Raises:
            RegistrationError: Server returned non-2xx with a JSON error body.
        """
        payload: dict[str, Any] = {"table": table, "key": key}
        if features is not None:
            payload["features"] = features
        body_bytes = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        r = self._client.post(
            "/get",
            content=body_bytes,
            headers={"Content-Type": "application/json"},
        )
        body = cast(dict[str, Any], r.json())
        if r.status_code == 200:
            return body
        error = body.get("error", {})
        raise RegistrationError(
            code=error.get("code", "unknown"),
            path=error.get("path", ""),
            message=error.get("reason") or error.get("message", ""),
            errors=[],
        )

    def send_batch_get(
        self,
        *,
        requests: list[
            tuple[str, str | list[Any]]
            | tuple[str, str | list[Any], list[str] | None]
        ],
    ) -> list[dict[str, Any]]:
        """POST ``/batch_get`` with body ``{requests:[{table,key,features?}, ...]}``.

        The server returns ``body["results"]`` as a list of flat row dicts
        (no wrapping envelope); this method returns that list verbatim.
        Per-entry ``features`` filter is supported via the 3-tuple form.

        Args:
            requests: list of per-entry tuples — either ``(table, key)`` or
                ``(table, key, features)``. ``features=None`` requests the
                full row.

        Returns:
            list of flat row dicts in request order.

        Raises:
            RegistrationError: Server returned non-2xx with a JSON error body.
            TypeError: A request entry is neither a 2-tuple nor a 3-tuple.
        """
        wire_requests: list[dict[str, Any]] = []
        for entry in requests:
            if len(entry) == 2:
                tbl, k = entry
                wire_requests.append({"table": tbl, "key": k})
            elif len(entry) == 3:
                tbl, k, feats = entry
                wire_entry: dict[str, Any] = {"table": tbl, "key": k}
                if feats is not None:
                    wire_entry["features"] = feats
                wire_requests.append(wire_entry)
            else:
                raise TypeError(
                    f"batch_get request entry must be a 2- or 3-tuple "
                    f"(table, key) or (table, key, features); "
                    f"got {len(entry)}-tuple"
                )
        body_bytes = json.dumps(
            {"requests": wire_requests}, ensure_ascii=False
        ).encode("utf-8")
        r = self._client.post(
            "/batch_get",
            content=body_bytes,
            headers={"Content-Type": "application/json"},
        )
        body = r.json()
        if r.status_code == 200:
            results: list[dict[str, Any]] = body.get("results", [])
            return results
        error = body.get("error", {})
        raise RegistrationError(
            code=error.get("code", "unknown"),
            path=error.get("path", ""),
            message=error.get("reason") or error.get("message", ""),
            errors=[],
        )

    def send_reset(self) -> None:
        """POST ``/reset``. Test-mode-gated server-side.

        On non-test-mode servers the engine returns 403 ``reset_disabled``;
        this method surfaces that as
        ``RegistrationError(code="reset_disabled")``.

        Raises:
            RegistrationError: Server returned non-200; in particular,
                ``code="reset_disabled"`` if the server is not in test mode.
        """
        r = self._client.post(
            "/reset",
            content=b"{}",
            headers={"Content-Type": "application/json"},
        )
        if r.status_code == 200:
            return
        try:
            body = r.json()
            error = body.get("error", {})
        except Exception:
            error = {"code": "unparseable_error", "message": r.text[:200]}
        raise RegistrationError(
            code=error.get("code", "unknown"),
            path=error.get("path", ""),
            message=error.get("reason") or error.get("message", ""),
            errors=[],
        )

    def send_ping(self) -> dict[str, Any]:
        """``POST /ping`` → ``{"pong": True, "registry_version": <n>}``.

        Verb-style liveness probe on the data plane (locked v0 surface).
        Returns the bumped registry counter so SDK clients can use this
        as a cheap cache-key invalidation / schema-evolution probe on
        long-lived connections.

        Returns:
            ``{"pong": True, "registry_version": <int>}``.

        Raises:
            RegistrationError: If the server returns a non-200 status
                (defensively wrapped for callers that don't want to deal
                with raw httpx exceptions).
        """
        r = self._client.post(
            "/ping",
            content=b"{}",
            headers={"Content-Type": "application/json"},
        )
        if r.status_code == 200:
            return cast(dict[str, Any], r.json())
        try:
            body = r.json()
            error = body.get("error", {})
        except Exception:
            error = {"code": "unparseable_error", "message": r.text[:200]}
        raise RegistrationError(
            code=error.get("code", "unknown"),
            path=error.get("path", ""),
            message=error.get("reason") or error.get("message", ""),
            errors=[],
        )

    def _http_get_single(self, feature: str, key: str) -> object:
        """``GET /get/{feature}/{key}`` → unwrapped feature value.

        Args:
            feature: Feature name (e.g. ``"cnt"``).
            key: Entity key value (e.g. ``"alice"``; URL-encoded if needed).

        Returns:
            The unwrapped feature value (contents of the response's
            ``"value"`` field). Raises if the server returned non-2xx.
        """
        r = self._client.get(f"/get/{feature}/{key}")
        r.raise_for_status()
        body = r.json()
        return body.get("value")

    def close(self) -> None:
        """Close the underlying httpx client connection pool."""
        self._client.close()

    def __enter__(self) -> "HttpTransport":
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: "TracebackType | None",
    ) -> None:
        self.close()


class TcpTransport(Transport):
    """Send requests over the binary-framed TCP protocol.

    Connection is lazy — opened on first use, reused for the lifetime of
    the transport. Strict-FIFO: one in-flight request per connection.

    Args:
        host: Server hostname or IP address.
        port: Server TCP port.
        max_frame_bytes: Maximum frame size (must match server config;
            default 4 MiB).
        timeout: Socket connect/recv timeout in seconds.
    """

    def __init__(
        self,
        host: str,
        port: int,
        *,
        max_frame_bytes: int = MAX_FRAME_BYTES,
        timeout: float = 30.0,
    ) -> None:
        self.host = host
        self.port = port
        self.max_frame_bytes = max_frame_bytes
        self._timeout = timeout
        self._socket: socket.socket | None = None

    def _ensure_connected(self) -> socket.socket:
        """Return the existing socket or open a new connection."""
        if self._socket is None:
            self._socket = socket.create_connection(
                (self.host, self.port), timeout=self._timeout
            )
        return self._socket

    def send_register(self, payload_json: bytes) -> dict[str, Any]:
        """Send an ``OP_REGISTER`` frame; return the parsed response dict.

        Args:
            payload_json: UTF-8 JSON bytes matching the wire contract.

        Returns:
            Parsed success body dict.

        Raises:
            RegistrationError: Server responded with ``OP_ERROR_RESPONSE``.
        """
        sock = self._ensure_connected()
        sock.sendall(encode_frame(OP_REGISTER, CT_JSON, payload_json))
        frame = read_frame(sock, self.max_frame_bytes)
        return parse_register_response(frame)

    def send_push(
        self,
        *,
        event_name: str,
        fields: dict[str, Any],
    ) -> dict[str, Any]:
        """Send an ``OP_PUSH`` frame with the given event and fields.

        The wire envelope is ``{"event": event_name, "body": fields}``,
        JSON-encoded. v0 defaults to JSON on this path (the server's
        msgpack path is reserved for the read fast-path only).

        On success the server echoes ``OP_PUSH`` (0x0010) with a body
        like ``{"ack_lsn": N, "idempotent_replay": bool,
        "registry_version": M}``. On validation failure the server
        emits ``OP_ERROR_RESPONSE`` (0xFFFF) with body
        ``{"error": {"code": "..."}, "registry_version": M}``; we
        raise :exc:`RegistrationError` so fire-and-forget callers
        don't silently drop events.

        Returns:
            Parsed JSON ACK dict (e.g. ``{"ack_lsn": 42}``).

        Raises:
            RegistrationError: Server returned ``OP_ERROR_RESPONSE`` or
                an unexpected response opcode.
        """
        envelope = json.dumps(
            {"event": event_name, "body": fields}, ensure_ascii=False
        ).encode("utf-8")
        sock = self._ensure_connected()
        sock.sendall(encode_frame(OP_PUSH, CT_JSON, envelope))
        frame = read_frame(sock, self.max_frame_bytes)
        if frame.op != OP_PUSH:
            try:
                err_body = json.loads(frame.payload.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError):
                err_body = {"error": {"code": "unparseable_error"}}
            raise RegistrationError(
                code=err_body.get("error", {}).get("code", "unexpected_frame"),
                message=(
                    f"expected OP_PUSH (0x0010), "
                    f"got op={frame.op:#06x} ct={frame.ct:#04x} body={err_body!r}"
                ),
            )
        result: dict[str, Any] = json.loads(frame.payload.decode("utf-8"))
        return result

    def send_get(
        self,
        *,
        table: str,
        key: str | list[Any],
        features: list[str] | None = None,
    ) -> dict[str, Any]:
        """Send ``OP_GET`` (0x0020); expect ``OP_GET_RESPONSE`` (0x0023).

        Wire body: ``{"table": ..., "key": ..., "features"?: [...]}``.
        ``features`` is included only when non-None (full row otherwise).

        Returns:
            Parsed row-shape flat dict (cold-start = ``{}``).

        Raises:
            RegistrationError: Server returned ``OP_ERROR_RESPONSE`` or
                an unexpected response opcode.
        """
        payload: dict[str, Any] = {"table": table, "key": key}
        if features is not None:
            payload["features"] = features
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        sock = self._ensure_connected()
        sock.sendall(encode_frame(OP_GET, CT_JSON, body))
        frame = read_frame(sock, self.max_frame_bytes)
        if frame.op != OP_GET_RESPONSE:
            try:
                err_body = json.loads(frame.payload.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError):
                err_body = {"error": {"code": "unparseable_error"}}
            raise RegistrationError(
                code=err_body.get("error", {}).get("code", "unexpected_frame"),
                message=(
                    f"expected OP_GET_RESPONSE (0x0023), "
                    f"got op={frame.op:#06x} ct={frame.ct:#04x} body={err_body!r}"
                ),
            )
        result: dict[str, Any] = json.loads(frame.payload.decode("utf-8"))
        return result

    def send_batch_get(
        self,
        *,
        requests: list[
            tuple[str, str | list[Any]]
            | tuple[str, str | list[Any], list[str] | None]
        ],
    ) -> list[dict[str, Any]]:
        """Send ``OP_BATCH_GET`` (0x0024); expect ``OP_GET_RESPONSE`` (0x0023).

        Wire body: ``{"requests": [{"table", "key", "features"?}, ...]}``;
        wire response: ``{"results": [<flat row>, ...]}``. Per-entry
        ``features`` filter is supported via the 3-tuple request form.

        Returns:
            list of flat row dicts in request order.

        Raises:
            RegistrationError: Server returned ``OP_ERROR_RESPONSE`` or
                an unexpected response shape.
            TypeError: A request entry is neither a 2-tuple nor a 3-tuple.
        """
        wire_requests: list[dict[str, Any]] = []
        for entry in requests:
            if len(entry) == 2:
                tbl, k = entry
                wire_requests.append({"table": tbl, "key": k})
            elif len(entry) == 3:
                tbl, k, feats = entry
                wire_entry: dict[str, Any] = {"table": tbl, "key": k}
                if feats is not None:
                    wire_entry["features"] = feats
                wire_requests.append(wire_entry)
            else:
                raise TypeError(
                    f"batch_get request entry must be a 2- or 3-tuple "
                    f"(table, key) or (table, key, features); "
                    f"got {len(entry)}-tuple"
                )
        body = json.dumps(
            {"requests": wire_requests}, ensure_ascii=False
        ).encode("utf-8")
        sock = self._ensure_connected()
        sock.sendall(encode_frame(OP_BATCH_GET, CT_JSON, body))
        frame = read_frame(sock, self.max_frame_bytes)
        if frame.op != OP_GET_RESPONSE:
            try:
                err_body = json.loads(frame.payload.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError):
                err_body = {"error": {"code": "unparseable_error"}}
            raise RegistrationError(
                code=err_body.get("error", {}).get("code", "unexpected_frame"),
                message=(
                    f"expected OP_GET_RESPONSE (0x0023) for OP_BATCH_GET, "
                    f"got op={frame.op:#06x} ct={frame.ct:#04x} body={err_body!r}"
                ),
            )
        decoded = json.loads(frame.payload.decode("utf-8"))
        if not isinstance(decoded, dict) or "results" not in decoded:
            raise RegistrationError(
                code="unexpected_frame",
                message=f"expected dict with 'results' key, got {decoded!r}",
            )
        results: list[dict[str, Any]] = decoded["results"]
        return results

    def send_reset(self) -> None:
        """Send ``OP_RESET`` (0x0040). Test-mode-gated server-side.

        Successful reset: the server replies with ``OP_GET_RESPONSE``
        (0x0023) — the generic JSON success frame — carrying
        ``{"reset": true, "registry_version": N}``. (The opcode is reused
        rather than introducing a dedicated ``OP_RESET_RESPONSE``.)

        Disabled (non-test mode): the server replies with
        ``OP_ERROR_RESPONSE`` and ``code="reset_disabled_in_production"``.

        Raises:
            RegistrationError: ``OP_RESET`` denied (non-test mode) or
                unexpected response opcode.
        """
        sock = self._ensure_connected()
        sock.sendall(encode_frame(OP_RESET, CT_JSON, b"{}"))
        frame = read_frame(sock, self.max_frame_bytes)
        if frame.op == OP_GET_RESPONSE:
            return
        try:
            err_body = json.loads(frame.payload.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            err_body = {"error": {"code": "unparseable_error"}}
        raise RegistrationError(
            code=err_body.get("error", {}).get("code", "unexpected_frame"),
            message=(
                f"OP_RESET denied or unexpected response: "
                f"op={frame.op:#06x} body={err_body!r}"
            ),
        )

    def send_ping(self) -> dict[str, Any]:
        """Send an ``OP_PING`` frame; return the parsed response dict.

        Returns:
            Dict with ``server_version`` and ``registry_version`` keys.

        Raises:
            RegistrationError: Unexpected response opcode.
        """
        sock = self._ensure_connected()
        sock.sendall(encode_frame(OP_PING, CT_JSON, b"{}"))
        frame = read_frame(sock, self.max_frame_bytes)
        if frame.op != OP_PING:
            raise RegistrationError(
                code="unexpected_frame",
                message=f"expected OP_PING (0x0000), got op={frame.op:#06x}",
            )
        result: dict[str, object] = json.loads(frame.payload.decode("utf-8"))
        return result

    def _tcp_get_single(
        self,
        feature: str,
        key: str,
        *,
        wire_format: str = "msgpack",
    ) -> object:
        """Send a single ``OP_GET`` frame; return the unwrapped feature value.

        Defaults to msgpack on the wire (the production read fast-path).
        Pass ``wire_format="json"`` to force the JSON path (regression
        coverage / debugging).

        Args:
            feature: Feature name (e.g. ``"cnt"``).
            key: Entity key value (e.g. ``"alice"``).
            wire_format: ``"msgpack"`` (default) or ``"json"``.

        Returns:
            The unwrapped feature value (``response["value"]``).

        Raises:
            ValueError: ``wire_format`` is not ``"msgpack"`` or ``"json"``.
            ImportError: ``wire_format="msgpack"`` but ``msgpack`` package
                is not installed.
            RegistrationError: Server returned ``OP_ERROR_RESPONSE`` or an
                unexpected op.
        """
        body_dict = {"feature": feature, "key": key}
        if wire_format == "msgpack":
            try:
                import msgpack
            except ImportError as exc:
                raise ImportError(
                    "wire_format='msgpack' requires the 'msgpack' package: "
                    "pip install msgpack"
                ) from exc
            body = msgpack.packb(body_dict, use_bin_type=True)
            ct = CT_MSGPACK
        elif wire_format == "json":
            body = json.dumps(body_dict, ensure_ascii=False).encode("utf-8")
            ct = CT_JSON
        else:
            raise ValueError(
                f"wire_format must be 'msgpack' or 'json', got {wire_format!r}"
            )

        sock = self._ensure_connected()
        sock.sendall(encode_frame(OP_GET, ct, body))
        frame = read_frame(sock, self.max_frame_bytes)
        if frame.op != OP_GET_RESPONSE:
            try:
                err_body = json.loads(frame.payload.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError):
                err_body = {"error": {"code": "unparseable_error"}}
            raise RegistrationError(
                code=err_body.get("error", {}).get("code", "unexpected_frame"),
                message=(
                    f"expected OP_GET_RESPONSE (0x0023), "
                    f"got op={frame.op:#06x} ct={frame.ct:#04x} body={err_body!r}"
                ),
            )

        # The server replies in the same content-type as the request: a
        # msgpack request gets a msgpack response, a JSON request gets JSON.
        if frame.ct == CT_MSGPACK:
            try:
                import msgpack
            except ImportError as exc:
                raise ImportError(
                    "received CT_MSGPACK response but 'msgpack' package not "
                    "installed: pip install msgpack"
                ) from exc
            decoded = msgpack.unpackb(frame.payload, raw=False)
        else:
            decoded = json.loads(frame.payload.decode("utf-8"))
        if not isinstance(decoded, dict):
            raise RegistrationError(
                code="unexpected_frame",
                message=f"expected dict response body, got {type(decoded).__name__}",
            )
        return decoded.get("value")

    def close(self) -> None:
        """Close the underlying socket if open."""
        if self._socket is not None:
            try:
                self._socket.close()
            except OSError:
                pass
            self._socket = None

    def __enter__(self) -> "TcpTransport":
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: "TracebackType | None",
    ) -> None:
        self.close()


class EmbedTransport(Transport):
    """Wraps a :class:`TcpTransport` and a subprocess handle for embed mode.

    Created by :func:`parse_url_to_transport` / :func:`make_transport` when
    ``url=None``. ``close()`` terminates the subprocess after closing the
    socket.

    Args:
        tcp: The :class:`TcpTransport` connected to the embedded server.
        proc: The :class:`subprocess.Popen` handle for the embedded server.
    """

    def __init__(
        self,
        tcp: TcpTransport,
        proc: "subprocess.Popen[bytes]",
        *,
        spawn_env: dict[str, str] | None = None,
    ) -> None:
        self._tcp = tcp
        self._proc = proc
        # The env dict passed to the spawned binary is exposed so tests
        # can assert ``BEAVA_TEST_MODE=1`` propagation.
        self._spawn_env: dict[str, str] = spawn_env or {}

    def send_register(self, payload_json: bytes) -> dict[str, Any]:
        return self._tcp.send_register(payload_json)

    def send_push(
        self,
        *,
        event_name: str,
        fields: dict[str, Any],
    ) -> dict[str, Any]:
        """Delegate to the embedded TcpTransport."""
        return self._tcp.send_push(event_name=event_name, fields=fields)

    def send_get(
        self,
        *,
        table: str,
        key: str | list[Any],
        features: list[str] | None = None,
    ) -> dict[str, Any]:
        return self._tcp.send_get(table=table, key=key, features=features)

    def send_batch_get(
        self,
        *,
        requests: list[
            tuple[str, str | list[Any]]
            | tuple[str, str | list[Any], list[str] | None]
        ],
    ) -> list[dict[str, Any]]:
        return self._tcp.send_batch_get(requests=requests)

    def send_reset(self) -> None:
        self._tcp.send_reset()

    def send_ping(self) -> dict[str, Any]:
        return self._tcp.send_ping()

    def _tcp_get_single(
        self,
        feature: str,
        key: str,
        *,
        wire_format: str = "msgpack",
    ) -> object:
        return self._tcp._tcp_get_single(feature, key, wire_format=wire_format)

    def close(self) -> None:
        """Close the TCP socket then terminate the embedded server process."""
        self._tcp.close()
        from beava._embed import teardown_process

        teardown_process(self._proc)

    def __enter__(self) -> "EmbedTransport":
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: "TracebackType | None",
    ) -> None:
        self.close()


def parse_url_to_transport(url: str | None) -> Transport:
    """Return the appropriate transport for the given URL or None (embed mode).

    Args:
        url: One of:
            - ``"http://..."`` or ``"https://..."`` → :class:`HttpTransport`
            - ``"tcp://host:port"`` → :class:`TcpTransport`
            - ``None`` → embed mode: spawn a local beava binary and return
              an :class:`EmbedTransport` connected to it over TCP.

    Returns:
        A concrete :class:`Transport` instance.

    Raises:
        ValueError: URL scheme is not ``http``, ``https``, ``tcp``, or ``None``.
        BinaryNotFoundError: Embed mode but binary cannot be located.
    """
    if url is None:
        from beava._embed import spawn_embedded_server

        proc, _http_url, tcp_url, env = spawn_embedded_server()
        parsed = urllib.parse.urlparse(tcp_url)
        host = parsed.hostname or "127.0.0.1"
        port = parsed.port or 8081
        tcp = TcpTransport(host=host, port=port)
        return EmbedTransport(tcp=tcp, proc=proc, spawn_env=env)

    if url.startswith("http://") or url.startswith("https://"):
        return HttpTransport(url)

    if url.startswith("tcp://"):
        parsed = urllib.parse.urlparse(url)
        host = parsed.hostname or "127.0.0.1"
        port = parsed.port or 8081
        return TcpTransport(host=host, port=port)

    raise ValueError(
        f"unsupported URL scheme in {url!r}; "
        f"supported schemes: http://, https://, tcp://, or None for embed mode"
    )


def make_transport(
    url: str | None = None,
    *,
    timeout: float = 30.0,
    test_mode: bool = False,
) -> Transport:
    """URL-scheme dispatch with ``test_mode`` propagation.

    Routes ``url=None`` to an embed-mode subprocess spawn (with optional
    ``BEAVA_TEST_MODE=1`` env var); ``http(s)://`` to :class:`HttpTransport`;
    ``tcp://`` to :class:`TcpTransport`.

    Args:
        url: Server URL or ``None`` for embed mode.
        timeout: HTTP / socket timeout in seconds.
        test_mode: When True + ``url=None``, spawns the binary with
            ``BEAVA_TEST_MODE=1``. Has no effect in network mode (the
            caller is expected to emit a ``UserWarning`` before calling).

    Returns:
        A concrete :class:`Transport` instance.
    """
    if url is None:
        from beava._embed import spawn_embedded_server

        proc, _http_url, tcp_url, env = spawn_embedded_server(test_mode=test_mode)
        parsed = urllib.parse.urlparse(tcp_url)
        host = parsed.hostname or "127.0.0.1"
        port = parsed.port or 8081
        tcp = TcpTransport(host=host, port=port, timeout=timeout)
        return EmbedTransport(tcp=tcp, proc=proc, spawn_env=env)

    if url.startswith("http://") or url.startswith("https://"):
        return HttpTransport(base_url=url, timeout=timeout)

    if url.startswith("tcp://"):
        parsed = urllib.parse.urlparse(url)
        host = parsed.hostname or "127.0.0.1"
        port = parsed.port or 8081
        return TcpTransport(host=host, port=port, timeout=timeout)

    raise ValueError(
        f"unsupported URL scheme in {url!r}; "
        f"supported schemes: http://, https://, tcp://, or None for embed mode"
    )
