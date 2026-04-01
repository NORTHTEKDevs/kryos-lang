"""
Kryos Standard Library - Net Module

Registers HTTP, WebSocket, TCP, and URL utility functions as interpreter
built-ins. Uses Python's urllib, socket, and json modules internally.
WebSocket support requires the optional ``websocket-client`` package.
"""

from __future__ import annotations

import json
import socket
import urllib.parse
import urllib.request
from typing import Any


# ---------------------------------------------------------------------------
# Handle registry -- shared state for sockets and websockets
# ---------------------------------------------------------------------------

_handle_counter = 0
_handles: dict[int, Any] = {}


def _new_handle(obj: Any) -> int:
    global _handle_counter
    _handle_counter += 1
    _handles[_handle_counter] = obj
    return _handle_counter


def _get_handle(h: Any) -> Any:
    key = int(h)
    if key not in _handles:
        raise RuntimeError(f"Invalid handle: {key}")
    return _handles[key]


def _close_handle(h: Any) -> None:
    _handles.pop(int(h), None)


# ---------------------------------------------------------------------------
# Registration
# ---------------------------------------------------------------------------

def register_net_builtins(interpreter) -> None:
    """Register networking utility functions."""
    env = interpreter.globals

    # ---- HTTP ----

    def _http_get(url: Any) -> str:
        """GET request, return response body as string."""
        u = str(url)
        try:
            req = urllib.request.Request(u, headers={"User-Agent": "Kryos/1.0"})
            with urllib.request.urlopen(req, timeout=30) as resp:
                return resp.read().decode("utf-8")
        except Exception as exc:
            raise RuntimeError(f"http_get: request to '{u}' failed: {exc}")

    def _http_post(url: Any, body: Any, *args: Any) -> str:
        """POST request. Optional third argument is content_type (default application/json)."""
        u = str(url)
        content_type = str(args[0]) if args else "application/json"
        try:
            data = str(body).encode("utf-8")
            req = urllib.request.Request(
                u,
                data=data,
                headers={
                    "User-Agent": "Kryos/1.0",
                    "Content-Type": content_type,
                },
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=30) as resp:
                return resp.read().decode("utf-8")
        except Exception as exc:
            raise RuntimeError(f"http_post: request to '{u}' failed: {exc}")

    def _http_request(method: Any, url: Any, *args: Any) -> dict:
        """Full HTTP request.

        Arguments: method, url, [headers_dict], [body_str]
        Returns: {"status": int, "body": str, "headers": dict}
        """
        m = str(method).upper()
        u = str(url)
        headers_arg: dict = args[0] if len(args) > 0 and isinstance(args[0], dict) else {}
        body_arg: str | None = str(args[1]) if len(args) > 1 and args[1] is not None else None

        merged_headers = {"User-Agent": "Kryos/1.0"}
        for k, v in headers_arg.items():
            merged_headers[str(k)] = str(v)

        try:
            data = body_arg.encode("utf-8") if body_arg else None
            req = urllib.request.Request(u, data=data, headers=merged_headers, method=m)
            with urllib.request.urlopen(req, timeout=30) as resp:
                return {
                    "status": resp.status,
                    "body": resp.read().decode("utf-8"),
                    "headers": dict(resp.headers.items()),
                }
        except urllib.error.HTTPError as exc:
            return {
                "status": exc.code,
                "body": exc.read().decode("utf-8", errors="replace"),
                "headers": dict(exc.headers.items()) if exc.headers else {},
            }
        except Exception as exc:
            raise RuntimeError(f"http_request: {m} '{u}' failed: {exc}")

    def _http_get_json(url: Any, *args: Any) -> Any:
        """GET + parse JSON response. Optional second arg is headers dict."""
        u = str(url)
        headers_arg: dict = args[0] if len(args) > 0 and isinstance(args[0], dict) else {}
        merged_headers = {"User-Agent": "Kryos/1.0", "Accept": "application/json"}
        for k, v in headers_arg.items():
            merged_headers[str(k)] = str(v)
        try:
            req = urllib.request.Request(u, headers=merged_headers)
            with urllib.request.urlopen(req, timeout=30) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except Exception as exc:
            raise RuntimeError(f"http_get_json: request to '{u}' failed: {exc}")

    def _http_post_json(url: Any, data: Any, *args: Any) -> Any:
        """POST with JSON body + parse JSON response. Optional third arg is headers dict."""
        u = str(url)
        headers_arg: dict = args[0] if len(args) > 0 and isinstance(args[0], dict) else {}
        merged_headers = {
            "User-Agent": "Kryos/1.0",
            "Content-Type": "application/json",
            "Accept": "application/json",
        }
        for k, v in headers_arg.items():
            merged_headers[str(k)] = str(v)
        try:
            payload = json.dumps(data).encode("utf-8")
            req = urllib.request.Request(u, data=payload, headers=merged_headers, method="POST")
            with urllib.request.urlopen(req, timeout=30) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except Exception as exc:
            raise RuntimeError(f"http_post_json: request to '{u}' failed: {exc}")

    # ---- WebSocket ----

    def _ws_connect(url: Any, *args: Any) -> int:
        """Connect to a WebSocket server. Returns a handle (int)."""
        try:
            import websocket  # type: ignore[import-untyped]
        except ImportError:
            raise RuntimeError(
                "ws_connect: 'websocket-client' package not installed. "
                "Install with: pip install websocket-client"
            )
        u = str(url)
        headers_arg: dict = args[0] if len(args) > 0 and isinstance(args[0], dict) else {}
        header_list = [f"{k}: {v}" for k, v in headers_arg.items()]
        try:
            ws = websocket.create_connection(u, header=header_list or None, timeout=30)
            return _new_handle(("ws", ws))
        except Exception as exc:
            raise RuntimeError(f"ws_connect: failed to connect to '{u}': {exc}")

    def _ws_send(handle: Any, data: Any) -> bool:
        """Send a string message on a websocket handle."""
        entry = _get_handle(handle)
        if not isinstance(entry, tuple) or entry[0] != "ws":
            raise RuntimeError("ws_send: handle is not a websocket")
        try:
            entry[1].send(str(data))
            return True
        except Exception as exc:
            raise RuntimeError(f"ws_send: failed: {exc}")

    def _ws_recv(handle: Any) -> str:
        """Receive a message from a websocket handle (blocking)."""
        entry = _get_handle(handle)
        if not isinstance(entry, tuple) or entry[0] != "ws":
            raise RuntimeError("ws_recv: handle is not a websocket")
        try:
            return entry[1].recv()
        except Exception as exc:
            raise RuntimeError(f"ws_recv: failed: {exc}")

    def _ws_close(handle: Any) -> None:
        """Close a websocket handle."""
        entry = _get_handle(handle)
        if not isinstance(entry, tuple) or entry[0] != "ws":
            raise RuntimeError("ws_close: handle is not a websocket")
        try:
            entry[1].close()
        except Exception:
            pass
        _close_handle(handle)

    # ---- TCP ----

    def _tcp_connect(host: Any, port: Any) -> int:
        """Open a TCP connection. Returns a handle (int)."""
        h = str(host)
        p = int(port)
        try:
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.settimeout(30)
            sock.connect((h, p))
            return _new_handle(("tcp", sock))
        except Exception as exc:
            raise RuntimeError(f"tcp_connect: failed to connect to {h}:{p}: {exc}")

    def _tcp_send(handle: Any, data: Any) -> int:
        """Send data on a TCP handle. Returns bytes sent."""
        entry = _get_handle(handle)
        if not isinstance(entry, tuple) or entry[0] != "tcp":
            raise RuntimeError("tcp_send: handle is not a TCP socket")
        try:
            return entry[1].send(str(data).encode("utf-8"))
        except Exception as exc:
            raise RuntimeError(f"tcp_send: failed: {exc}")

    def _tcp_recv(handle: Any, *args: Any) -> str:
        """Receive data from a TCP handle. Optional max_bytes (default 4096)."""
        entry = _get_handle(handle)
        if not isinstance(entry, tuple) or entry[0] != "tcp":
            raise RuntimeError("tcp_recv: handle is not a TCP socket")
        max_bytes = int(args[0]) if args else 4096
        try:
            return entry[1].recv(max_bytes).decode("utf-8")
        except Exception as exc:
            raise RuntimeError(f"tcp_recv: failed: {exc}")

    def _tcp_close(handle: Any) -> None:
        """Close a TCP socket handle."""
        entry = _get_handle(handle)
        if not isinstance(entry, tuple) or entry[0] != "tcp":
            raise RuntimeError("tcp_close: handle is not a TCP socket")
        try:
            entry[1].close()
        except Exception:
            pass
        _close_handle(handle)

    # ---- URL Utilities ----

    def _url_encode(s: Any) -> str:
        """URL-encode a string (percent-encoding, no safe chars)."""
        return urllib.parse.quote(str(s), safe="")

    def _url_decode(s: Any) -> str:
        """URL-decode a percent-encoded string."""
        return urllib.parse.unquote(str(s))

    # ---- Register all ----

    env.define("http_get", _http_get)
    env.define("http_post", _http_post)
    env.define("http_request", _http_request)
    env.define("http_get_json", _http_get_json)
    env.define("http_post_json", _http_post_json)

    env.define("ws_connect", _ws_connect)
    env.define("ws_send", _ws_send)
    env.define("ws_recv", _ws_recv)
    env.define("ws_close", _ws_close)

    env.define("tcp_connect", _tcp_connect)
    env.define("tcp_send", _tcp_send)
    env.define("tcp_recv", _tcp_recv)
    env.define("tcp_close", _tcp_close)

    env.define("url_encode", _url_encode)
    env.define("url_decode", _url_decode)
