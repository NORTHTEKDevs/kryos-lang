"""
Kryos Standard Library - JSON Module

Registers JSON parsing and serialization functions as interpreter built-ins.
Uses Python's json module internally.
"""

from __future__ import annotations

import json
from typing import Any


def register_json_builtins(interpreter) -> None:
    """Register JSON utility functions."""
    env = interpreter.globals

    def _json_parse(s: Any) -> Any:
        """Parse a JSON string into a Kryos value (dicts, lists, primitives)."""
        try:
            return json.loads(str(s))
        except json.JSONDecodeError as exc:
            raise RuntimeError(f"json_parse() failed: {exc}") from exc

    def _json_stringify(value: Any) -> str:
        """Convert a Kryos value to a JSON string."""
        try:
            return json.dumps(value)
        except (TypeError, ValueError) as exc:
            raise RuntimeError(f"json_stringify() failed: {exc}") from exc

    def _json_get(obj: Any, key: Any) -> Any:
        """Get a value from a JSON object (dict) by key."""
        if isinstance(obj, dict):
            return obj.get(str(key), None)
        raise RuntimeError("json_get() first argument must be a JSON object")

    def _json_has(obj: Any, key: Any) -> bool:
        """Check if a key exists in a JSON object (dict)."""
        if isinstance(obj, dict):
            return str(key) in obj
        raise RuntimeError("json_has() first argument must be a JSON object")

    env.define("json_parse", _json_parse)
    env.define("json_stringify", _json_stringify)
    env.define("json_get", _json_get)
    env.define("json_has", _json_has)
