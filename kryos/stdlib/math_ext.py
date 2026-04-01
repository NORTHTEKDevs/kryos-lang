"""
Kryos Standard Library - Math Extensions

Registers extended math functions as interpreter built-ins.
Core math (sin, cos, tan, log, pow, floor, ceil, sqrt, min, max, abs)
already exist. This module adds: round, log10, random, pi, e.
"""

from __future__ import annotations

import math
import random as _random_mod
from typing import Any


def register_math_builtins(interpreter) -> None:
    """Register math extension functions.

    Functions already registered by _setup_builtins are skipped.
    Only new functions are added here.
    """
    env = interpreter.globals

    def _round(x: Any) -> int:
        return round(float(x))

    def _log10(x: Any) -> float:
        return math.log10(float(x))

    def _random_fn() -> float:
        return _random_mod.random()

    def _pi() -> float:
        return math.pi

    def _e() -> float:
        return math.e

    env.define("round", _round)
    env.define("log10", _log10)
    env.define("random", _random_fn)
    env.define("pi", _pi)
    env.define("e", _e)
