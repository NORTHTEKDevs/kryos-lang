"""
Kryos Standard Library - String Extensions

Registers additional string manipulation functions as interpreter built-ins.
These complement the core string_utils module with operations needed for
terminal UI rendering and text processing.
"""

from __future__ import annotations
from typing import Any


def register_string_ext_builtins(interpreter) -> None:
    """Register extended string utility functions."""
    env = interpreter.globals

    def _string_repeat(s: Any, n: Any) -> str:
        return str(s) * int(n)

    def _string_pad_left(*args: Any) -> str:
        if len(args) < 2 or len(args) > 3:
            raise RuntimeError("string_pad_left expects 2 or 3 arguments")
        s = str(args[0])
        width = int(args[1])
        char = str(args[2]) if len(args) == 3 else " "
        return s.rjust(width, char)

    def _string_pad_right(*args: Any) -> str:
        if len(args) < 2 or len(args) > 3:
            raise RuntimeError("string_pad_right expects 2 or 3 arguments")
        s = str(args[0])
        width = int(args[1])
        char = str(args[2]) if len(args) == 3 else " "
        return s.ljust(width, char)

    def _string_lines(s: Any) -> list:
        return str(s).split("\n")

    def _to_int(s: Any) -> int:
        try:
            return int(str(s))
        except ValueError as exc:
            raise RuntimeError(f"to_int: cannot parse '{s}' as integer: {exc}")

    def _to_float(s: Any) -> float:
        try:
            return float(str(s))
        except ValueError as exc:
            raise RuntimeError(f"to_float: cannot parse '{s}' as float: {exc}")

    def _string_index(s: Any, sub: Any) -> int:
        return str(s).find(str(sub))

    def _string_count(s: Any, sub: Any) -> int:
        return str(s).count(str(sub))

    env.define("string_repeat", _string_repeat)
    env.define("string_pad_left", _string_pad_left)
    env.define("string_pad_right", _string_pad_right)
    env.define("string_lines", _string_lines)
    env.define("to_int", _to_int)
    env.define("to_float", _to_float)
    env.define("string_index", _string_index)
    env.define("string_count", _string_count)
