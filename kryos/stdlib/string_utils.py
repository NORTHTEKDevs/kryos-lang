"""
Kryos Standard Library - String Utilities

Registers string manipulation functions as interpreter built-ins.
Some of these (split, trim, replace, starts_with, ends_with, contains,
char_at, substr, upper, lower) already exist in the interpreter core.
This module adds aliases (to_upper, to_lower) for naming consistency.
"""

from __future__ import annotations
from typing import Any


def register_string_builtins(interpreter) -> None:
    """Register string utility functions.

    Functions already registered by _setup_builtins are skipped.
    Only new functions or aliases are added here.
    """
    env = interpreter.globals

    # -- Aliases for naming consistency --
    # The core interpreter registers `upper` and `lower`.
    # Register `to_upper` and `to_lower` as aliases.

    def _to_upper(s: Any) -> str:
        return str(s).upper()

    def _to_lower(s: Any) -> str:
        return str(s).lower()

    env.define("to_upper", _to_upper)
    env.define("to_lower", _to_lower)
