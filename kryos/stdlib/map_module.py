"""
Kryos Standard Library - Map/Dict Module

Registers dictionary/map manipulation functions as interpreter built-ins.
Kryos uses Python dicts internally (e.g. from json_parse) but needs
first-class builtins to create and manipulate them.
"""

from __future__ import annotations
from typing import Any


def register_map_builtins(interpreter) -> None:
    """Register map/dict utility functions."""
    env = interpreter.globals

    def _map_new() -> dict:
        return {}

    def _map_set(m: Any, key: Any, value: Any) -> dict:
        if not isinstance(m, dict):
            raise RuntimeError("map_set: first argument must be a map")
        d = dict(m)
        d[str(key)] = value
        return d

    def _map_get(*args: Any) -> Any:
        if len(args) < 2 or len(args) > 3:
            raise RuntimeError("map_get expects 2 or 3 arguments")
        m, key = args[0], args[1]
        default = args[2] if len(args) == 3 else None
        if not isinstance(m, dict):
            raise RuntimeError("map_get: first argument must be a map")
        return m.get(str(key), default)

    def _map_has(m: Any, key: Any) -> bool:
        if not isinstance(m, dict):
            raise RuntimeError("map_has: first argument must be a map")
        return str(key) in m

    def _map_keys(m: Any) -> list:
        if not isinstance(m, dict):
            raise RuntimeError("map_keys: first argument must be a map")
        return list(m.keys())

    def _map_values(m: Any) -> list:
        if not isinstance(m, dict):
            raise RuntimeError("map_values: first argument must be a map")
        return list(m.values())

    def _map_remove(m: Any, key: Any) -> dict:
        if not isinstance(m, dict):
            raise RuntimeError("map_remove: first argument must be a map")
        d = dict(m)
        d.pop(str(key), None)
        return d

    def _map_merge(a: Any, b: Any) -> dict:
        if not isinstance(a, dict) or not isinstance(b, dict):
            raise RuntimeError("map_merge: both arguments must be maps")
        d = dict(a)
        d.update(b)
        return d

    def _map_from(*args: Any) -> dict:
        if len(args) % 2 != 0:
            raise RuntimeError(
                "map_from expects an even number of arguments (key-value pairs)"
            )
        d = {}
        for i in range(0, len(args), 2):
            d[str(args[i])] = args[i + 1]
        return d

    env.define("map_new", _map_new)
    env.define("map_set", _map_set)
    env.define("map_get", _map_get)
    env.define("map_has", _map_has)
    env.define("map_keys", _map_keys)
    env.define("map_values", _map_values)
    env.define("map_remove", _map_remove)
    env.define("map_merge", _map_merge)
    env.define("map_from", _map_from)
