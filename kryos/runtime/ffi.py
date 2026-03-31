"""
Kryos Foreign Function Interface (FFI)

Enables calling Python modules and C/C++ shared libraries from Kryos code.

- PythonFFI (Community tier - ffi:python): Import and call any Python package.
- NativeFFI (Pro tier - ffi:native): Load and call C/C++ shared libraries via ctypes.
- FFIRegistry: Global registry tracking all loaded foreign modules/libraries.
"""

from __future__ import annotations

import ctypes
import importlib
import platform
import sys
from typing import Any, Optional


# ---------------------------------------------------------------------------
# Type marshaling maps: Kryos type strings <-> ctypes types
# ---------------------------------------------------------------------------

_KRYOS_TO_CTYPE: dict[str, type] = {
    "i32":    ctypes.c_int,
    "i64":    ctypes.c_long if platform.system() != "Windows" else ctypes.c_longlong,
    "u32":    ctypes.c_uint,
    "u64":    ctypes.c_ulong if platform.system() != "Windows" else ctypes.c_ulonglong,
    "f32":    ctypes.c_float,
    "f64":    ctypes.c_double,
    "bool":   ctypes.c_bool,
    "str":    ctypes.c_char_p,
    "void":   None,
    "ptr":    ctypes.c_void_p,
    "i8":     ctypes.c_int8,
    "i16":    ctypes.c_int16,
    "u8":     ctypes.c_uint8,
    "u16":    ctypes.c_uint16,
}


def _resolve_ctype(type_name: str) -> Optional[type]:
    """Resolve a Kryos type string to its ctypes equivalent.

    Supports array notation like ``[i32; 10]`` for fixed-size ctypes arrays.
    """
    type_name = type_name.strip()

    # Array notation: [T; N]
    if type_name.startswith("[") and ";" in type_name:
        inner = type_name[1:type_name.index(";")].strip()
        count_str = type_name[type_name.index(";") + 1 : type_name.index("]")].strip()
        base = _resolve_ctype(inner)
        if base is None:
            raise FFIError(f"unsupported array element type: {inner}")
        return base * int(count_str)

    ct = _KRYOS_TO_CTYPE.get(type_name)
    if ct is None and type_name != "void":
        raise FFIError(f"unsupported Kryos type for FFI: '{type_name}'")
    return ct


# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------

class FFIError(Exception):
    """Raised when an FFI operation fails."""


# ---------------------------------------------------------------------------
# Python FFI  (Community tier -- ffi:python)
# ---------------------------------------------------------------------------

class PythonFFI:
    """Import and call Python packages from Kryos.

    Uses ``importlib`` under the hood so any installed Python package is
    accessible.
    """

    def import_module(self, module_name: str) -> Any:
        """Import a Python module by its fully-qualified name.

        >>> ffi = PythonFFI()
        >>> m = ffi.import_module("math")
        >>> m.pi  # doctest: +ELLIPSIS
        3.14159...
        """
        try:
            return importlib.import_module(module_name)
        except ImportError as exc:
            raise FFIError(
                f"failed to import Python module '{module_name}': {exc}"
            ) from exc

    def call_function(self, module: Any, fn_name: str, *args: Any) -> Any:
        """Call *fn_name* on *module* (or any object) with the given arguments.

        >>> ffi = PythonFFI()
        >>> m = ffi.import_module("math")
        >>> ffi.call_function(m, "sqrt", 16.0)
        4.0
        """
        fn = self._getattr(module, fn_name)
        if not callable(fn):
            raise FFIError(
                f"'{fn_name}' on {module!r} is not callable"
            )
        try:
            return fn(*args)
        except Exception as exc:
            raise FFIError(
                f"error calling {fn_name}: {exc}"
            ) from exc

    def get_attribute(self, module: Any, attr_name: str) -> Any:
        """Get an attribute from a Python module or object.

        >>> ffi = PythonFFI()
        >>> m = ffi.import_module("sys")
        >>> isinstance(ffi.get_attribute(m, "path"), list)
        True
        """
        return self._getattr(module, attr_name)

    # -- internal helpers --------------------------------------------------

    @staticmethod
    def _getattr(obj: Any, name: str) -> Any:
        try:
            return getattr(obj, name)
        except AttributeError as exc:
            raise FFIError(
                f"attribute '{name}' not found on {obj!r}"
            ) from exc


# ---------------------------------------------------------------------------
# Native (C/C++) FFI  (Pro tier -- ffi:native)
# ---------------------------------------------------------------------------

class NativeFFI:
    """Call C/C++ shared libraries from Kryos using ``ctypes``.

    Supports ``.dll`` (Windows), ``.so`` (Linux), and ``.dylib`` (macOS).
    """

    def load_library(self, path: str) -> ctypes.CDLL:
        """Load a shared library from *path*.

        Returns a ``ctypes.CDLL`` handle that can be passed to
        :meth:`call_function`.
        """
        try:
            return ctypes.CDLL(path)
        except OSError as exc:
            raise FFIError(
                f"failed to load native library '{path}': {exc}"
            ) from exc

    def call_function(
        self,
        lib: ctypes.CDLL,
        fn_name: str,
        arg_types: list[str],
        return_type: str,
        *args: Any,
    ) -> Any:
        """Call a C function with full type marshaling.

        Parameters
        ----------
        lib:
            The library handle returned by :meth:`load_library`.
        fn_name:
            Name of the exported symbol.
        arg_types:
            Kryos type strings for each argument (e.g. ``["i32", "f64"]``).
        return_type:
            Kryos type string for the return value (e.g. ``"f64"``).
            Use ``"void"`` for functions that return nothing.
        *args:
            The actual argument values to pass.
        """
        try:
            fn = getattr(lib, fn_name)
        except AttributeError as exc:
            raise FFIError(
                f"symbol '{fn_name}' not found in library"
            ) from exc

        # Set argument types
        fn.argtypes = [_resolve_ctype(t) for t in arg_types]

        # Set return type
        restype = _resolve_ctype(return_type)
        fn.restype = restype

        # Marshal string arguments: encode Python str -> bytes for c_char_p
        coerced_args: list[Any] = []
        for val, type_name in zip(args, arg_types):
            if type_name == "str" and isinstance(val, str):
                coerced_args.append(val.encode("utf-8"))
            else:
                coerced_args.append(val)

        try:
            result = fn(*coerced_args)
        except Exception as exc:
            raise FFIError(
                f"error calling native function '{fn_name}': {exc}"
            ) from exc

        # Decode bytes -> str for string returns
        if return_type == "str" and isinstance(result, bytes):
            return result.decode("utf-8")

        return result


# ---------------------------------------------------------------------------
# FFI Registry
# ---------------------------------------------------------------------------

class FFIRegistry:
    """Global registry of loaded foreign modules and native libraries.

    Caches imports/loads so the same module or library is only loaded once.
    """

    def __init__(self) -> None:
        self.loaded_python: dict[str, Any] = {}
        self.loaded_native: dict[str, ctypes.CDLL] = {}
        self.python_ffi = PythonFFI()
        self.native_ffi = NativeFFI()

    # -- Python helpers ----------------------------------------------------

    def py_import(self, module_name: str) -> Any:
        """Import (and cache) a Python module."""
        if module_name not in self.loaded_python:
            self.loaded_python[module_name] = self.python_ffi.import_module(module_name)
        return self.loaded_python[module_name]

    def py_call(self, module: Any, fn_name: str, *args: Any) -> Any:
        """Call a function on a Python module."""
        return self.python_ffi.call_function(module, fn_name, *args)

    def py_attr(self, module: Any, attr_name: str) -> Any:
        """Get an attribute from a Python module/object."""
        return self.python_ffi.get_attribute(module, attr_name)

    # -- Native helpers ----------------------------------------------------

    def c_load(self, path: str) -> ctypes.CDLL:
        """Load (and cache) a native shared library."""
        if path not in self.loaded_native:
            self.loaded_native[path] = self.native_ffi.load_library(path)
        return self.loaded_native[path]

    def c_call(
        self,
        lib: ctypes.CDLL,
        fn_name: str,
        arg_types: list[str],
        return_type: str,
        *args: Any,
    ) -> Any:
        """Call a C function with type marshaling."""
        return self.native_ffi.call_function(lib, fn_name, arg_types, return_type, *args)

    # -- Introspection -----------------------------------------------------

    def list_loaded(self) -> dict[str, list[str]]:
        """Return names of all loaded modules/libraries."""
        return {
            "python": list(self.loaded_python.keys()),
            "native": list(self.loaded_native.keys()),
        }


# ---------------------------------------------------------------------------
# Module-level singleton for convenience
# ---------------------------------------------------------------------------

_global_registry: Optional[FFIRegistry] = None


def get_registry() -> FFIRegistry:
    """Return the global FFI registry, creating it on first access."""
    global _global_registry
    if _global_registry is None:
        _global_registry = FFIRegistry()
    return _global_registry
