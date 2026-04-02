"""
Kryos Standard Library - Set Module

Set data structure for unique collections. Uses Python frozenset internally
but exposed as mutable sets for ease of use.
"""
from __future__ import annotations
from typing import Any

def register_set_builtins(interpreter) -> None:
    env = interpreter.globals

    def _set_new(*args: Any) -> set:
        """Create a new set. Optional: pass items as arguments or an array."""
        if len(args) == 1 and isinstance(args[0], list):
            return set(args[0])
        return set(args)

    def _set_add(s: Any, value: Any) -> set:
        """Add a value to the set. Returns the set."""
        if not isinstance(s, set):
            raise RuntimeError("set_add: first argument must be a set")
        s.add(value)
        return s

    def _set_remove(s: Any, value: Any) -> set:
        """Remove a value from the set. Returns the set."""
        if not isinstance(s, set):
            raise RuntimeError("set_remove: first argument must be a set")
        s.discard(value)
        return s

    def _set_has(s: Any, value: Any) -> bool:
        """Check if a value is in the set."""
        if not isinstance(s, set):
            raise RuntimeError("set_has: first argument must be a set")
        return value in s

    def _set_size(s: Any) -> int:
        """Get the number of elements in the set."""
        if not isinstance(s, set):
            raise RuntimeError("set_size: first argument must be a set")
        return len(s)

    def _set_union(a: Any, b: Any) -> set:
        """Union of two sets."""
        return set(a) | set(b)

    def _set_intersection(a: Any, b: Any) -> set:
        """Intersection of two sets."""
        return set(a) & set(b)

    def _set_difference(a: Any, b: Any) -> set:
        """Difference: elements in a but not in b."""
        return set(a) - set(b)

    def _set_to_array(s: Any) -> list:
        """Convert set to array."""
        return list(s)

    def _set_from_array(arr: Any) -> set:
        """Create set from array (deduplicates)."""
        if isinstance(arr, list):
            return set(arr)
        return set()

    env.define("set_new", _set_new)
    env.define("set_add", _set_add)
    env.define("set_remove", _set_remove)
    env.define("set_has", _set_has)
    env.define("set_size", _set_size)
    env.define("set_union", _set_union)
    env.define("set_intersection", _set_intersection)
    env.define("set_difference", _set_difference)
    env.define("set_to_array", _set_to_array)
    env.define("set_from_array", _set_from_array)
