"""
Kryos Standard Library - Server Module

Registers HTTP server functions: http_app, app_get, app_post, app_put,
app_delete, app_use, app_listen, respond, plus built-in middleware:
cors_middleware, logger_middleware, serve_static, rate_limit.
Uses Python's http.server for development mode.
"""

from __future__ import annotations

import json
import mimetypes
import os
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from socketserver import ThreadingMixIn
from typing import Any
from urllib.parse import parse_qs, urlparse


# ---------------------------------------------------------------------------
# App registry -- shared state for server instances
# ---------------------------------------------------------------------------

_app_counter = 0
_apps: dict[int, dict] = {}

# Thread-local response storage (safe for concurrent requests)
_thread_local = threading.local()

# Rate-limit tracking per IP
_rate_limits: dict[str, list] = {}
_rate_lock = threading.Lock()

# Maximum request body size (10 MB)
MAX_BODY_SIZE = 10 * 1024 * 1024


# ---------------------------------------------------------------------------
# Registration
# ---------------------------------------------------------------------------

def register_server_builtins(interpreter) -> None:
    """Register HTTP server utility functions."""
    env = interpreter.globals

    def _http_app() -> int:
        """Create a new HTTP application. Returns a handle (int)."""
        global _app_counter
        _app_counter += 1
        _apps[_app_counter] = {
            "routes": [],
            "middleware": [],
        }
        return _app_counter

    def _app_get(handle: Any, path: Any, handler: Any) -> int:
        """Register a GET route handler. Returns the app handle."""
        h = int(handle)
        _apps[h]["routes"].append(("GET", str(path), handler))
        return h

    def _app_post(handle: Any, path: Any, handler: Any) -> int:
        """Register a POST route handler. Returns the app handle."""
        h = int(handle)
        _apps[h]["routes"].append(("POST", str(path), handler))
        return h

    def _app_put(handle: Any, path: Any, handler: Any) -> int:
        """Register a PUT route handler. Returns the app handle."""
        h = int(handle)
        _apps[h]["routes"].append(("PUT", str(path), handler))
        return h

    def _app_delete(handle: Any, path: Any, handler: Any) -> int:
        """Register a DELETE route handler. Returns the app handle."""
        h = int(handle)
        _apps[h]["routes"].append(("DELETE", str(path), handler))
        return h

    def _app_use(handle: Any, middleware: Any) -> int:
        """Register middleware on an app. Returns the app handle."""
        h = int(handle)
        _apps[h]["middleware"].append(middleware)
        return h

    def _respond(status: Any, body: Any, *args: Any) -> None:
        """Set the response for the current request.

        Arguments: status (int), body (str), [headers_dict]
        """
        headers = args[0] if args else {}
        _thread_local.response = {
            "status": int(status),
            "body": str(body),
            "headers": headers if isinstance(headers, dict) else {},
        }

    def _match_route(routes: list, method: str, path: str) -> tuple:
        """Match a route pattern like /api/users/:id against a request path."""
        for route_method, pattern, handler in routes:
            if route_method != method:
                continue
            pattern_parts = pattern.strip("/").split("/")
            path_parts = path.strip("/").split("/")
            if len(pattern_parts) != len(path_parts):
                continue
            params: dict[str, str] = {}
            matched = True
            for pp, ap in zip(pattern_parts, path_parts):
                if pp.startswith(":"):
                    params[pp[1:]] = ap
                elif pp != ap:
                    matched = False
                    break
            if matched:
                return handler, params
        return None, {}

    def _app_listen(handle: Any, port: Any) -> None:
        """Start listening for HTTP requests (blocking).

        Arguments: app_handle (int), port (int)
        """
        h = int(handle)
        p = int(port)
        app = _apps[h]

        class KryosHandler(BaseHTTPRequestHandler):
            def do_request(self) -> None:
                from kryos.compiler.interpreter import KryosFunction

                parsed = urlparse(self.path)
                query: dict[str, Any] = {}
                for k, v in parse_qs(parsed.query).items():
                    query[k] = v[0] if len(v) == 1 else v

                content_length = int(self.headers.get("Content-Length", 0))
                if content_length > MAX_BODY_SIZE:
                    self.send_response(413)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(json.dumps({"error": "request body too large"}).encode())
                    return
                body = (
                    self.rfile.read(content_length).decode("utf-8")
                    if content_length > 0
                    else ""
                )

                handler, params = _match_route(app["routes"], self.command, parsed.path)

                headers = {k.lower(): v for k, v in self.headers.items()}
                req = {
                    "method": self.command,
                    "path": parsed.path,
                    "params": params,
                    "query": query,
                    "headers": headers,
                    "body": body,
                }

                _thread_local.response = {"status": 200, "body": "", "headers": {}}

                try:
                    # Build the final handler (route handler or 404)
                    def call_handler():
                        if handler is None:
                            _thread_local.response = {
                                "status": 404,
                                "body": json.dumps({"error": "not found"}),
                                "headers": {},
                            }
                            return
                        if isinstance(handler, KryosFunction):
                            interpreter._call_function(handler, [req])
                        else:
                            handler(req)

                    # Build middleware chain from inside out.
                    # Each middleware receives (req, next_fn) and must call
                    # next_fn() to continue the chain.
                    chain = call_handler
                    for mw in reversed(app["middleware"]):
                        outer_chain = chain  # capture current chain

                        if isinstance(mw, KryosFunction):
                            # Kryos-authored middleware
                            def _make_kryos_mw(m, c):
                                def _next_fn(r=None):
                                    c()
                                def _run():
                                    interpreter._call_function(m, [req, _next_fn])
                                return _run
                            chain = _make_kryos_mw(mw, outer_chain)
                        elif callable(mw):
                            # Python-native middleware
                            def _make_py_mw(m, c):
                                def _run():
                                    m(req, c)
                                return _run
                            chain = _make_py_mw(mw, outer_chain)

                    # Execute the full middleware -> handler chain
                    chain()
                except Exception as e:
                    _thread_local.response = {
                        "status": 500,
                        "body": json.dumps({"error": str(e)}),
                        "headers": {},
                    }

                resp = getattr(_thread_local, "response", {"status": 500, "body": "", "headers": {}})
                self.send_response(resp["status"])
                resp_headers = resp.get("headers", {})
                if "content-type" not in {k.lower() for k in resp_headers}:
                    self.send_header("Content-Type", "application/json")
                for k, v in resp_headers.items():
                    self.send_header(k, v)
                self.end_headers()
                body_out = resp.get("body", "")
                if isinstance(body_out, bytes):
                    self.wfile.write(body_out)
                else:
                    self.wfile.write(str(body_out).encode())

            def do_GET(self) -> None:
                self.do_request()

            def do_POST(self) -> None:
                self.do_request()

            def do_PUT(self) -> None:
                self.do_request()

            def do_DELETE(self) -> None:
                self.do_request()

            def do_OPTIONS(self) -> None:
                self.do_request()

            def log_message(self, format: str, *args: Any) -> None:
                pass  # suppress default logging

        class ThreadedHTTPServer(ThreadingMixIn, HTTPServer):
            daemon_threads = True

        server = ThreadedHTTPServer(("0.0.0.0", p), KryosHandler)
        print(f"Listening on :{p}")
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            server.shutdown()

    # ------------------------------------------------------------------
    # Built-in middleware
    # ------------------------------------------------------------------

    def _cors_middleware(req: Any, next_fn: Any) -> None:
        """Built-in CORS middleware.  Adds CORS headers and handles OPTIONS
        preflight requests automatically."""
        if req.get("method") == "OPTIONS":
            _thread_local.response = {
                "status": 204,
                "body": "",
                "headers": {
                    "Access-Control-Allow-Origin": "*",
                    "Access-Control-Allow-Methods": "GET, POST, PUT, DELETE, OPTIONS",
                    "Access-Control-Allow-Headers": "Content-Type, Authorization",
                    "Access-Control-Max-Age": "86400",
                },
            }
            return
        next_fn()
        resp = getattr(_thread_local, "response", {})
        resp.setdefault("headers", {})
        resp["headers"]["Access-Control-Allow-Origin"] = "*"
        resp["headers"]["Access-Control-Allow-Methods"] = (
            "GET, POST, PUT, DELETE, OPTIONS"
        )
        resp["headers"]["Access-Control-Allow-Headers"] = (
            "Content-Type, Authorization"
        )

    def _logger_middleware(req: Any, next_fn: Any) -> None:
        """Built-in request logging middleware.  Prints method, path,
        status code, and elapsed time."""
        start = time.time()
        next_fn()
        elapsed = (time.time() - start) * 1000
        resp = getattr(_thread_local, "response", {})
        status = resp.get("status", 200)
        print(f"  {req.get('method', '?')} {req.get('path', '?')} {status} {elapsed:.1f}ms")

    def _serve_static(directory: Any) -> Any:
        """Return middleware that serves static files from *directory*.

        Usage in Kryos::

            app_use(app, serve_static("./public"))
        """
        base = str(directory)

        real_base = os.path.realpath(base)

        def middleware(req: Any, next_fn: Any) -> None:
            path = req.get("path", "/").lstrip("/")
            file_path = os.path.join(base, path)
            real_path = os.path.realpath(file_path)
            # Path traversal protection: must stay within base directory
            if not real_path.startswith(real_base):
                next_fn()
                return
            if os.path.isfile(real_path):
                mime_type = mimetypes.guess_type(real_path)[0] or "application/octet-stream"
                with open(real_path, "rb") as f:
                    content = f.read()
                _thread_local.response = {
                    "status": 200,
                    "body": content.decode("utf-8", errors="replace"),
                    "headers": {"Content-Type": mime_type},
                }
            else:
                next_fn()

        return middleware

    def _rate_limit(max_requests: Any, window_seconds: Any) -> Any:
        """Return middleware that rate-limits by client IP.

        Usage in Kryos::

            app_use(app, rate_limit(100, 60))   // 100 req per 60 sec
        """
        max_req = int(max_requests)
        window = float(window_seconds)

        def middleware(req: Any, next_fn: Any) -> None:
            ip = req.get("headers", {}).get("x-forwarded-for", "127.0.0.1")
            now = time.time()
            with _rate_lock:
                if ip not in _rate_limits:
                    _rate_limits[ip] = []
                _rate_limits[ip] = [t for t in _rate_limits[ip] if now - t < window]
                if len(_rate_limits[ip]) >= max_req:
                    _thread_local.response = {
                        "status": 429,
                        "body": json.dumps({"error": "too many requests"}),
                        "headers": {},
                    }
                    return
                _rate_limits[ip].append(now)
            next_fn()

        return middleware

    # ---- Register all ----

    env.define("http_app", _http_app)
    env.define("app_get", _app_get)
    env.define("app_post", _app_post)
    env.define("app_put", _app_put)
    env.define("app_delete", _app_delete)
    env.define("app_use", _app_use)
    env.define("app_listen", _app_listen)
    env.define("respond", _respond)

    # Built-in middleware
    env.define("cors_middleware", _cors_middleware)
    env.define("logger_middleware", _logger_middleware)
    env.define("serve_static", _serve_static)
    env.define("rate_limit", _rate_limit)
