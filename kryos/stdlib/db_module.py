"""
Kryos Standard Library - Database Module

Registers database functions: db_connect, db_query, db_query_one, db_execute, db_close.
Supports SQLite (built-in) and PostgreSQL (requires psycopg2).
URL scheme detection: sqlite:// or postgres://
"""

from __future__ import annotations

import re
import sqlite3
from typing import Any


# ---------------------------------------------------------------------------
# Handle registry -- shared state for database connections
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
        raise RuntimeError(f"Invalid db handle: {key}")
    return _handles[key]


def _close_handle(h: Any) -> None:
    _handles.pop(int(h), None)


def _rewrite_params(sql: str) -> str:
    """Rewrite $1, $2, ... to ? for SQLite."""
    return re.sub(r'\$\d+', '?', sql)


# ---------------------------------------------------------------------------
# Registration
# ---------------------------------------------------------------------------

def register_db_builtins(interpreter) -> None:
    """Register database utility functions."""
    env = interpreter.globals

    def _db_connect(url: Any) -> int:
        """Connect to a database. Returns a handle (int).

        Supported URL schemes:
          sqlite://:memory:   -- in-memory SQLite database
          sqlite:///path      -- file-backed SQLite database
          postgres://...      -- PostgreSQL (requires psycopg2-binary)
          postgresql://...    -- alias for postgres://
        """
        u = str(url)
        if u.startswith("sqlite://"):
            path = u[len("sqlite://"):]
            if path == ":memory:":
                conn = sqlite3.connect(":memory:")
            else:
                conn = sqlite3.connect(path)
            conn.execute("PRAGMA journal_mode=WAL")
            conn.execute("PRAGMA foreign_keys=ON")
            conn.row_factory = sqlite3.Row
            return _new_handle(("sqlite", conn))
        elif u.startswith("postgres://") or u.startswith("postgresql://"):
            try:
                import psycopg2  # type: ignore[import-untyped]
                import psycopg2.extras  # type: ignore[import-untyped]

                conn = psycopg2.connect(u)
                conn.autocommit = False
                return _new_handle(("postgres", conn))
            except ImportError:
                raise RuntimeError(
                    "db_connect: psycopg2 not installed. "
                    "Install with: pip install psycopg2-binary"
                )
        else:
            raise RuntimeError(f"db_connect: unsupported URL scheme: {u}")

    def _db_query(handle: Any, sql: Any, *args: Any) -> list:
        """Execute a SELECT query and return all rows as a list of dicts."""
        entry = _get_handle(handle)
        db_type, conn = entry
        params = args[0] if args else []
        if isinstance(params, (list, tuple)):
            params = list(params)
        else:
            params = []

        if db_type == "sqlite":
            cursor = conn.execute(_rewrite_params(str(sql)), params)
            columns = [desc[0] for desc in cursor.description] if cursor.description else []
            rows = []
            for row in cursor.fetchall():
                rows.append({columns[i]: row[i] for i in range(len(columns))})
            return rows
        elif db_type == "postgres":
            import psycopg2.extras  # type: ignore[import-untyped]

            cursor = conn.cursor(cursor_factory=psycopg2.extras.RealDictCursor)
            cursor.execute(str(sql), params)
            rows = [dict(row) for row in cursor.fetchall()]
            cursor.close()
            return rows
        return []

    def _db_query_one(handle: Any, sql: Any, *args: Any) -> Any:
        """Execute a SELECT query and return the first row, or nil."""
        rows = _db_query(handle, sql, *args)
        return rows[0] if rows else None

    def _db_execute(handle: Any, sql: Any, *args: Any) -> int:
        """Execute a non-SELECT statement. Returns number of rows affected."""
        entry = _get_handle(handle)
        db_type, conn = entry
        params = args[0] if args else []
        if isinstance(params, (list, tuple)):
            params = list(params)
        else:
            params = []

        if db_type == "sqlite":
            cursor = conn.execute(_rewrite_params(str(sql)), params)
            conn.commit()
            return cursor.rowcount
        elif db_type == "postgres":
            cursor = conn.cursor()
            cursor.execute(str(sql), params)
            affected = cursor.rowcount
            conn.commit()
            cursor.close()
            return affected
        return 0

    def _db_close(handle: Any) -> None:
        """Close a database connection."""
        entry = _get_handle(handle)
        _, conn = entry
        try:
            conn.close()
        except Exception:
            pass
        _close_handle(handle)

    # ---- Register all ----

    env.define("db_connect", _db_connect)
    env.define("db_query", _db_query)
    env.define("db_query_one", _db_query_one)
    env.define("db_execute", _db_execute)
    env.define("db_close", _db_close)
