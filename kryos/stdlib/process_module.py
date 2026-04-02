"""
Kryos Standard Library - Process Module

Registers process execution, timing, and process-control functions as
interpreter built-ins.  Provides shell command execution needed for IDE
tool integration, CLI argument access, and spawn synchronisation.
"""

from __future__ import annotations

import subprocess
import sys
import threading
import time
from typing import Any

# Script arguments passed from the CLI.  Populated by the CLI runner
# before the interpreter starts so that ``args()`` returns the correct
# values.
_script_args: list[str] = []


def set_script_args(argv: list[str]) -> None:
    """Called by the CLI to store the user's script arguments."""
    global _script_args
    _script_args = list(argv)


def register_process_builtins(interpreter) -> None:
    """Register process/execution utility functions."""
    env = interpreter.globals

    def _exec(command: Any) -> dict:
        cmd = str(command)
        try:
            result = subprocess.run(
                cmd, shell=True, capture_output=True, text=True, timeout=30
            )
            return {
                "stdout": result.stdout,
                "stderr": result.stderr,
                "exit_code": result.returncode,
            }
        except subprocess.TimeoutExpired:
            return {
                "stdout": "",
                "stderr": "Command timed out",
                "exit_code": -1,
            }

    def _exec_capture(command: Any) -> str:
        cmd = str(command)
        try:
            result = subprocess.run(
                cmd, shell=True, capture_output=True, text=True, timeout=30
            )
            if result.returncode != 0:
                raise RuntimeError(
                    f"exec_capture: command failed with exit code "
                    f"{result.returncode}: {result.stderr}"
                )
            return result.stdout
        except subprocess.TimeoutExpired:
            raise RuntimeError("exec_capture: command timed out")

    def _exec_timeout(command: Any, seconds: Any) -> dict:
        cmd = str(command)
        timeout = float(seconds)
        try:
            result = subprocess.run(
                cmd, shell=True, capture_output=True, text=True, timeout=timeout
            )
            return {
                "stdout": result.stdout,
                "stderr": result.stderr,
                "exit_code": result.returncode,
            }
        except subprocess.TimeoutExpired:
            return {
                "stdout": "",
                "stderr": "Command timed out",
                "exit_code": -1,
            }

    def _sleep(seconds: Any) -> None:
        time.sleep(float(seconds))

    def _args() -> list[str]:
        """Return command-line arguments passed after the script filename."""
        return list(_script_args)

    def _exit(*args_: Any) -> None:
        """Exit the program with an optional exit code (default 0)."""
        code = int(args_[0]) if args_ else 0
        sys.exit(code)

    def _wait_all(*handles: Any) -> None:
        """Wait for spawned threads to complete.

        With no arguments, waits for all threads tracked by the interpreter.
        With arguments, waits for the given thread handles.
        """
        if not handles:
            # Join all spawned threads tracked by the interpreter
            for t in interpreter._spawned_threads:
                if isinstance(t, threading.Thread):
                    t.join()
            interpreter._spawned_threads.clear()
        else:
            for h in handles:
                if isinstance(h, threading.Thread):
                    h.join()

    env.define("exec", _exec)
    env.define("exec_capture", _exec_capture)
    env.define("exec_timeout", _exec_timeout)
    env.define("sleep", _sleep)
    env.define("args", _args)
    env.define("exit", _exit)
    env.define("wait_all", _wait_all)
