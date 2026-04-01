"""
Kryos Standard Library - Process Module

Registers process execution and timing functions as interpreter built-ins.
Provides shell command execution needed for IDE tool integration.
"""

from __future__ import annotations

import subprocess
import time
from typing import Any


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

    env.define("exec", _exec)
    env.define("exec_capture", _exec_capture)
    env.define("exec_timeout", _exec_timeout)
    env.define("sleep", _sleep)
