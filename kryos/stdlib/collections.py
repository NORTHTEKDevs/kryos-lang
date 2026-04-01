"""
Kryos Standard Library - Collection Utilities

Registers higher-order collection functions as interpreter built-ins.
These functions accept Kryos user-defined functions (KryosFunction) as
well as plain Python callables for the callback argument.
"""

from __future__ import annotations
from typing import Any


def _invoke(interpreter, fn: Any, args: list[Any]) -> Any:
    """Invoke a function -- handles both KryosFunction and plain callables."""
    from kryos.compiler.interpreter import KryosFunction
    if isinstance(fn, KryosFunction):
        return interpreter._call_function(fn, args)
    if callable(fn):
        return fn(*args)
    raise RuntimeError(f"'{fn}' is not callable")


def register_collection_builtins(interpreter) -> None:
    """Register collection utility functions."""
    env = interpreter.globals

    def _map(arr: Any, fn: Any) -> list:
        if not isinstance(arr, list):
            raise RuntimeError("map() first argument must be an array")
        return [_invoke(interpreter, fn, [item]) for item in arr]

    def _filter(arr: Any, fn: Any) -> list:
        if not isinstance(arr, list):
            raise RuntimeError("filter() first argument must be an array")
        return [item for item in arr if _invoke(interpreter, fn, [item])]

    def _reduce(arr: Any, fn: Any, initial: Any) -> Any:
        if not isinstance(arr, list):
            raise RuntimeError("reduce() first argument must be an array")
        acc = initial
        for item in arr:
            acc = _invoke(interpreter, fn, [acc, item])
        return acc

    def _sort(arr: Any) -> list:
        if not isinstance(arr, list):
            raise RuntimeError("sort() argument must be an array")
        return sorted(arr)

    def _reverse(arr: Any) -> list:
        if not isinstance(arr, list):
            raise RuntimeError("reverse() argument must be an array")
        return list(reversed(arr))

    def _zip(a: Any, b: Any) -> list:
        if not isinstance(a, list) or not isinstance(b, list):
            raise RuntimeError("zip() arguments must be arrays")
        return [list(pair) for pair in zip(a, b)]

    def _enumerate(arr: Any) -> list:
        if not isinstance(arr, list):
            raise RuntimeError("enumerate() argument must be an array")
        return [[i, v] for i, v in enumerate(arr)]

    def _find(arr: Any, fn: Any) -> Any:
        if not isinstance(arr, list):
            raise RuntimeError("find() first argument must be an array")
        for item in arr:
            if _invoke(interpreter, fn, [item]):
                return item
        return None

    def _any(arr: Any, fn: Any) -> bool:
        if not isinstance(arr, list):
            raise RuntimeError("any() first argument must be an array")
        return any(_invoke(interpreter, fn, [item]) for item in arr)

    def _all(arr: Any, fn: Any) -> bool:
        if not isinstance(arr, list):
            raise RuntimeError("all() first argument must be an array")
        return all(_invoke(interpreter, fn, [item]) for item in arr)

    def _flat_map(arr: Any, fn: Any) -> list:
        if not isinstance(arr, list):
            raise RuntimeError("flat_map() first argument must be an array")
        result = []
        for item in arr:
            mapped = _invoke(interpreter, fn, [item])
            if isinstance(mapped, list):
                result.extend(mapped)
            else:
                result.append(mapped)
        return result

    def _sum(arr: Any) -> Any:
        if not isinstance(arr, list):
            raise RuntimeError("sum() argument must be an array")
        if len(arr) == 0:
            return 0
        return sum(arr)

    def _count(arr: Any) -> int:
        if not isinstance(arr, list):
            raise RuntimeError("count() argument must be an array")
        return len(arr)

    env.define("map", _map)
    env.define("filter", _filter)
    env.define("reduce", _reduce)
    env.define("sort", _sort)
    env.define("reverse", _reverse)
    env.define("zip", _zip)
    env.define("enumerate", _enumerate)
    env.define("find", _find)
    env.define("any", _any)
    env.define("all", _all)
    env.define("flat_map", _flat_map)
    env.define("sum", _sum)
    env.define("count", _count)
