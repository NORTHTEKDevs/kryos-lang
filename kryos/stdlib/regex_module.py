"""
Kryos Standard Library - Regex Module

Regular expression support wrapping Python's re module.
"""
from __future__ import annotations
import re
from typing import Any

def register_regex_builtins(interpreter) -> None:
    env = interpreter.globals

    def _regex_match(pattern: Any, text: Any) -> Any:
        """Match pattern at start of text. Returns match map or none."""
        m = re.match(str(pattern), str(text))
        if m is None:
            return None
        return {"matched": m.group(0), "groups": list(m.groups()), "start": m.start(), "end": m.end()}

    def _regex_search(pattern: Any, text: Any) -> Any:
        """Search for pattern anywhere in text. Returns match map or none."""
        m = re.search(str(pattern), str(text))
        if m is None:
            return None
        return {"matched": m.group(0), "groups": list(m.groups()), "start": m.start(), "end": m.end()}

    def _regex_find_all(pattern: Any, text: Any) -> list:
        """Find all non-overlapping matches. Returns array of strings."""
        return re.findall(str(pattern), str(text))

    def _regex_replace(pattern: Any, replacement: Any, text: Any, *args: Any) -> str:
        """Replace all matches. Optional 4th arg is max replacements."""
        count = int(args[0]) if args else 0
        return re.sub(str(pattern), str(replacement), str(text), count=count)

    def _regex_split(pattern: Any, text: Any, *args: Any) -> list:
        """Split text by pattern. Optional 3rd arg is max splits."""
        maxsplit = int(args[0]) if args else 0
        return re.split(str(pattern), str(text), maxsplit=maxsplit)

    def _regex_test(pattern: Any, text: Any) -> bool:
        """Test if pattern matches anywhere in text. Returns bool."""
        return re.search(str(pattern), str(text)) is not None

    def _regex_escape(text: Any) -> str:
        """Escape special regex characters in text."""
        return re.escape(str(text))

    env.define("regex_match", _regex_match)
    env.define("regex_search", _regex_search)
    env.define("regex_find_all", _regex_find_all)
    env.define("regex_replace", _regex_replace)
    env.define("regex_split", _regex_split)
    env.define("regex_test", _regex_test)
    env.define("regex_escape", _regex_escape)
