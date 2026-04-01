"""
Kryos Standard Library - Terminal Control

Registers terminal control functions as interpreter built-ins.
Provides ANSI escape sequence output, cursor control, styling,
screen management, and raw key reading.
"""

from __future__ import annotations

import os
import sys
from typing import Any


def register_term_builtins(interpreter) -> None:
    """Register terminal control functions."""
    env = interpreter.globals

    # ---- Output Control ----

    def _term_clear() -> None:
        sys.stdout.write("\033[2J\033[H")
        sys.stdout.flush()

    def _term_write(text: Any) -> None:
        sys.stdout.write(str(text))
        sys.stdout.flush()

    def _term_flush() -> None:
        sys.stdout.flush()

    def _term_move(row: Any, col: Any) -> None:
        sys.stdout.write(f"\033[{int(row)};{int(col)}H")
        sys.stdout.flush()

    def _term_hide_cursor() -> None:
        sys.stdout.write("\033[?25l")
        sys.stdout.flush()

    def _term_show_cursor() -> None:
        sys.stdout.write("\033[?25h")
        sys.stdout.flush()

    # ---- Screen Management ----

    def _term_alt_screen() -> None:
        sys.stdout.write("\033[?1049h")
        sys.stdout.flush()

    def _term_main_screen() -> None:
        sys.stdout.write("\033[?1049l")
        sys.stdout.flush()

    def _term_size() -> list:
        try:
            size = os.get_terminal_size()
            return [size.columns, size.lines]
        except OSError:
            return [80, 24]

    # ---- Styling ----

    _COLOR_MAP = {
        "black":   (30, 40),
        "red":     (31, 41),
        "green":   (32, 42),
        "yellow":  (33, 43),
        "blue":    (34, 44),
        "magenta": (35, 45),
        "cyan":    (36, 46),
        "white":   (37, 47),
    }

    def _term_color(*args: Any) -> None:
        fg = str(args[0]).lower() if len(args) >= 1 else "white"
        fg_code = _COLOR_MAP.get(fg, (37, 47))[0]
        sys.stdout.write(f"\033[{fg_code}m")
        if len(args) >= 2:
            bg = str(args[1]).lower()
            bg_code = _COLOR_MAP.get(bg, (37, 47))[1]
            sys.stdout.write(f"\033[{bg_code}m")
        sys.stdout.flush()

    def _term_reset() -> None:
        sys.stdout.write("\033[0m")
        sys.stdout.flush()

    def _term_bold(text: Any) -> str:
        return f"\033[1m{text}\033[0m"

    def _term_dim(text: Any) -> str:
        return f"\033[2m{text}\033[0m"

    def _term_underline(text: Any) -> str:
        return f"\033[4m{text}\033[0m"

    def _term_rgb(r: Any, g: Any, b: Any, text: Any) -> str:
        return f"\033[38;2;{int(r)};{int(g)};{int(b)}m{text}\033[0m"

    # ---- Input ----

    def _term_raw_mode(enabled: Any) -> None:
        if sys.platform == "win32":
            # No-op on Windows; raw mode handled per-read by msvcrt
            return
        import termios
        import tty
        fd = sys.stdin.fileno()
        if enabled:
            tty.setraw(fd)
        else:
            # Restore to cooked mode
            termios.tcsetattr(fd, termios.TCSADRAIN, termios.tcgetattr(fd))

    def _term_read_key() -> str:
        if sys.platform == "win32":
            import msvcrt
            ch = msvcrt.getwch()
            if ch in ("\x00", "\xe0"):
                # Extended key
                ext = msvcrt.getwch()
                ext_map = {
                    "H": "ArrowUp",
                    "P": "ArrowDown",
                    "K": "ArrowLeft",
                    "M": "ArrowRight",
                    "G": "Home",
                    "O": "End",
                    "I": "PageUp",
                    "Q": "PageDown",
                    "S": "Delete",
                    "R": "Insert",
                    ";": "F1",
                    "<": "F2",
                    "=": "F3",
                    ">": "F4",
                    "?": "F5",
                    "@": "F6",
                    "A": "F7",
                    "B": "F8",
                    "C": "F9",
                    "D": "F10",
                }
                return ext_map.get(ext, f"Unknown({ord(ext)})")
            elif ch == "\r":
                return "Enter"
            elif ch == "\t":
                return "Tab"
            elif ch == "\x1b":
                return "Escape"
            elif ch == "\x7f" or ch == "\x08":
                return "Backspace"
            elif ord(ch) < 32:
                return f"Ctrl+{chr(ord(ch) + 96)}"
            else:
                return ch
        else:
            # Unix
            import tty
            import termios
            fd = sys.stdin.fileno()
            old_settings = termios.tcgetattr(fd)
            try:
                tty.setraw(fd)
                ch = sys.stdin.read(1)
                if ch == "\x1b":
                    seq = sys.stdin.read(1)
                    if seq == "[":
                        code = sys.stdin.read(1)
                        arrow_map = {
                            "A": "ArrowUp",
                            "B": "ArrowDown",
                            "C": "ArrowRight",
                            "D": "ArrowLeft",
                            "H": "Home",
                            "F": "End",
                        }
                        if code in arrow_map:
                            return arrow_map[code]
                        # Handle longer sequences like F1-F12, Delete, etc.
                        if code.isdigit():
                            num = code
                            next_ch = sys.stdin.read(1)
                            while next_ch.isdigit():
                                num += next_ch
                                next_ch = sys.stdin.read(1)
                            # next_ch should be '~'
                            tilde_map = {
                                "1": "Home",
                                "2": "Insert",
                                "3": "Delete",
                                "4": "End",
                                "5": "PageUp",
                                "6": "PageDown",
                                "11": "F1",
                                "12": "F2",
                                "13": "F3",
                                "14": "F4",
                                "15": "F5",
                                "17": "F6",
                                "18": "F7",
                                "19": "F8",
                                "20": "F9",
                                "21": "F10",
                                "23": "F11",
                                "24": "F12",
                            }
                            return tilde_map.get(num, f"Unknown({num})")
                        return f"Unknown({code})"
                    elif seq == "O":
                        code = sys.stdin.read(1)
                        f_map = {
                            "P": "F1",
                            "Q": "F2",
                            "R": "F3",
                            "S": "F4",
                        }
                        return f_map.get(code, f"Unknown(O{code})")
                    else:
                        return "Escape"
                elif ch == "\r" or ch == "\n":
                    return "Enter"
                elif ch == "\t":
                    return "Tab"
                elif ch == "\x7f":
                    return "Backspace"
                elif ord(ch) < 32:
                    return f"Ctrl+{chr(ord(ch) + 96)}"
                else:
                    return ch
            finally:
                termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)

    env.define("term_clear", _term_clear)
    env.define("term_write", _term_write)
    env.define("term_flush", _term_flush)
    env.define("term_move", _term_move)
    env.define("term_hide_cursor", _term_hide_cursor)
    env.define("term_show_cursor", _term_show_cursor)
    env.define("term_alt_screen", _term_alt_screen)
    env.define("term_main_screen", _term_main_screen)
    env.define("term_size", _term_size)
    env.define("term_color", _term_color)
    env.define("term_reset", _term_reset)
    env.define("term_bold", _term_bold)
    env.define("term_dim", _term_dim)
    env.define("term_underline", _term_underline)
    env.define("term_rgb", _term_rgb)
    env.define("term_raw_mode", _term_raw_mode)
    env.define("term_read_key", _term_read_key)
