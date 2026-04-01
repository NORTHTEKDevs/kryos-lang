"""
Kryos Standard Library

Built-in modules that extend the Kryos runtime with essential functionality.
All functions registered here are available in every .kry program without imports.
"""

from kryos.stdlib.string_utils import register_string_builtins
from kryos.stdlib.math_ext import register_math_builtins
from kryos.stdlib.collections import register_collection_builtins
from kryos.stdlib.json_module import register_json_builtins


def register_all_stdlib(interpreter) -> None:
    """Register all standard library built-in functions with the interpreter."""
    register_string_builtins(interpreter)
    register_math_builtins(interpreter)
    register_collection_builtins(interpreter)
    register_json_builtins(interpreter)
