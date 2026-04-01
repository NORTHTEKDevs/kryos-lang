"""
Kryos Standard Library - I/O Module

Registers file system, path, environment, and temp file functions
as interpreter built-ins.
"""

from __future__ import annotations

import glob as _glob_mod
import os
import shutil
import tempfile
from typing import Any


def register_io_builtins(interpreter) -> None:
    """Register I/O utility functions."""
    env = interpreter.globals

    # ---- File Operations ----

    def _file_read(path: Any) -> str:
        p = str(path)
        try:
            with open(p, "r", encoding="utf-8") as fh:
                return fh.read()
        except Exception as exc:
            raise RuntimeError(f"file_read: cannot read '{p}': {exc}")

    def _file_write(path: Any, content: Any) -> bool:
        p = str(path)
        try:
            parent = os.path.dirname(p)
            if parent:
                os.makedirs(parent, exist_ok=True)
            with open(p, "w", encoding="utf-8") as fh:
                fh.write(str(content))
            return True
        except Exception as exc:
            raise RuntimeError(f"file_write: cannot write '{p}': {exc}")

    def _file_append(path: Any, content: Any) -> bool:
        p = str(path)
        try:
            with open(p, "a", encoding="utf-8") as fh:
                fh.write(str(content))
            return True
        except Exception as exc:
            raise RuntimeError(f"file_append: cannot append to '{p}': {exc}")

    def _file_exists(path: Any) -> bool:
        return os.path.exists(str(path))

    def _file_delete(path: Any) -> bool:
        p = str(path)
        try:
            os.remove(p)
            return True
        except Exception as exc:
            raise RuntimeError(f"file_delete: cannot delete '{p}': {exc}")

    def _file_lines(path: Any) -> list:
        p = str(path)
        try:
            with open(p, "r", encoding="utf-8") as fh:
                return [
                    line.rstrip("\n").rstrip("\r")
                    for line in fh
                    if line.strip()
                ]
        except Exception as exc:
            raise RuntimeError(f"file_lines: cannot read '{p}': {exc}")

    def _file_copy(src: Any, dst: Any) -> bool:
        try:
            shutil.copy2(str(src), str(dst))
            return True
        except Exception as exc:
            raise RuntimeError(f"file_copy: cannot copy '{src}' to '{dst}': {exc}")

    def _file_move(src: Any, dst: Any) -> bool:
        try:
            shutil.move(str(src), str(dst))
            return True
        except Exception as exc:
            raise RuntimeError(f"file_move: cannot move '{src}' to '{dst}': {exc}")

    def _file_size(path: Any) -> int:
        p = str(path)
        try:
            return os.path.getsize(p)
        except Exception as exc:
            raise RuntimeError(f"file_size: cannot stat '{p}': {exc}")

    def _file_modified(path: Any) -> int:
        p = str(path)
        try:
            return int(os.path.getmtime(p))
        except Exception as exc:
            raise RuntimeError(f"file_modified: cannot stat '{p}': {exc}")

    def _file_is_dir(path: Any) -> bool:
        return os.path.isdir(str(path))

    def _file_is_file(path: Any) -> bool:
        return os.path.isfile(str(path))

    # ---- Directory Operations ----

    def _dir_list(path: Any) -> list:
        p = str(path)
        try:
            return sorted(os.listdir(p))
        except Exception as exc:
            raise RuntimeError(f"dir_list: cannot list '{p}': {exc}")

    def _dir_create(path: Any) -> bool:
        p = str(path)
        try:
            os.makedirs(p, exist_ok=True)
            return True
        except Exception as exc:
            raise RuntimeError(f"dir_create: cannot create '{p}': {exc}")

    def _dir_remove(path: Any) -> bool:
        p = str(path)
        try:
            shutil.rmtree(p)
            return True
        except Exception as exc:
            raise RuntimeError(f"dir_remove: cannot remove '{p}': {exc}")

    # ---- Glob ----

    def _glob(pattern: Any) -> list:
        try:
            return sorted(_glob_mod.glob(str(pattern), recursive=True))
        except Exception as exc:
            raise RuntimeError(f"glob: error with pattern '{pattern}': {exc}")

    # ---- Path Utilities ----

    def _path_join(*parts: Any) -> str:
        return os.path.join(*(str(p) for p in parts)).replace("\\", "/")

    def _path_dirname(path: Any) -> str:
        return os.path.dirname(str(path)).replace("\\", "/")

    def _path_basename(path: Any) -> str:
        return os.path.basename(str(path))

    def _path_extension(path: Any) -> str:
        ext = os.path.splitext(str(path))[1]
        return ext.lstrip(".")

    def _path_resolve(path: Any) -> str:
        return os.path.abspath(str(path)).replace("\\", "/")

    # ---- Environment & Temp ----

    def _stdin_read() -> str:
        try:
            return input()
        except EOFError:
            return ""

    def _env_get(key: Any) -> str:
        return os.environ.get(str(key), "")

    def _env_set(key: Any, val: Any) -> None:
        os.environ[str(key)] = str(val)

    def _cwd() -> str:
        return os.getcwd().replace("\\", "/")

    def _temp_file(*args: Any) -> str:
        prefix = str(args[0]) if args else "kryos_"
        fd, path = tempfile.mkstemp(prefix=prefix)
        os.close(fd)
        return path.replace("\\", "/")

    def _temp_dir(*args: Any) -> str:
        prefix = str(args[0]) if args else "kryos_"
        path = tempfile.mkdtemp(prefix=prefix)
        return path.replace("\\", "/")

    # ---- Register all ----

    env.define("file_read", _file_read)
    env.define("file_write", _file_write)
    env.define("file_append", _file_append)
    env.define("file_exists", _file_exists)
    env.define("file_delete", _file_delete)
    env.define("file_lines", _file_lines)
    env.define("file_copy", _file_copy)
    env.define("file_move", _file_move)
    env.define("file_size", _file_size)
    env.define("file_modified", _file_modified)
    env.define("file_is_dir", _file_is_dir)
    env.define("file_is_file", _file_is_file)

    env.define("dir_list", _dir_list)
    env.define("dir_create", _dir_create)
    env.define("dir_remove", _dir_remove)

    env.define("glob", _glob)

    env.define("path_join", _path_join)
    env.define("path_dirname", _path_dirname)
    env.define("path_basename", _path_basename)
    env.define("path_extension", _path_extension)
    env.define("path_resolve", _path_resolve)

    env.define("stdin_read", _stdin_read)
    env.define("env_get", _env_get)
    env.define("env_set", _env_set)
    env.define("cwd", _cwd)
    env.define("temp_file", _temp_file)
    env.define("temp_dir", _temp_dir)
