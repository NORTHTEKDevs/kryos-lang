"""
CLI command handler for ``kryos bench``.

Runs the benchmark suite and reports timings for both the Python interpreter
and the Rust VM (when available), with speedup comparison.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Dict, Optional


# ---------------------------------------------------------------------------
# ANSI helpers (mirror cli.py conventions)
# ---------------------------------------------------------------------------

_USE_COLOR = hasattr(sys.stdout, "isatty") and sys.stdout.isatty()


def _c(code: str, text: str) -> str:
    return f"\033[{code}m{text}\033[0m" if _USE_COLOR else text


def _red(t: str) -> str: return _c("31", t)
def _green(t: str) -> str: return _c("32", t)
def _yellow(t: str) -> str: return _c("33", t)
def _cyan(t: str) -> str: return _c("36", t)
def _bold(t: str) -> str: return _c("1", t)
def _dim(t: str) -> str: return _c("2", t)


# ---------------------------------------------------------------------------
# Benchmark suite
# ---------------------------------------------------------------------------

BENCHMARKS = ["fibonacci", "matrix", "strings", "sort", "http_bench"]


def _run_single(bench_file: Path, runtime: str, timeout: int = 120) -> Optional[float]:
    """Run a single benchmark on the given runtime. Returns wall-clock seconds or None on failure."""
    try:
        start = time.perf_counter()
        result = subprocess.run(
            [sys.executable, "-m", "kryos", "run", "--runtime", runtime, str(bench_file)],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        elapsed = time.perf_counter() - start
        if result.returncode == 0:
            return elapsed
        return None
    except (subprocess.TimeoutExpired, FileNotFoundError, OSError):
        return None


def _run_bench(args: argparse.Namespace) -> None:
    """Execute the ``kryos bench`` command."""
    bench_dir = Path(args.dir) if hasattr(args, "dir") and args.dir else (
        Path(__file__).parent.parent.parent / "benchmarks"
    )

    if not bench_dir.is_dir():
        print(_red(f"error: benchmark directory not found: {bench_dir}"), file=sys.stderr)
        sys.exit(1)

    # Determine which benchmarks to run
    names = BENCHMARKS
    if hasattr(args, "name") and args.name:
        names = [args.name]

    timeout = getattr(args, "timeout", 120) or 120

    print(_bold("Kryos Benchmark Suite"))
    print(_dim(f"Directory: {bench_dir}"))
    print()

    results: Dict[str, Dict[str, Optional[float]]] = {}

    for name in names:
        bench_file = bench_dir / f"{name}.kry"
        if not bench_file.exists():
            print(_yellow(f"  skip: {name}.kry not found"))
            continue

        print(f"  Running {_cyan(name)}...", end="", flush=True)

        # Python interpreter
        py_time = _run_single(bench_file, "python", timeout)

        # Rust VM
        rust_time = _run_single(bench_file, "rust", timeout)

        results[name] = {"python": py_time, "rust_vm": rust_time}
        print(" done")

    # Print results table
    print()
    print(f"{'Benchmark':<20} {'Python':>10} {'Rust VM':>10} {'Speedup':>10}")
    print("-" * 52)
    for name, times in results.items():
        py_str = f"{times['python']:.2f}s" if times["python"] is not None else "FAIL"
        rust_str = f"{times['rust_vm']:.2f}s" if times["rust_vm"] is not None else "N/A"
        if times["python"] is not None and times["rust_vm"] is not None and times["rust_vm"] > 0:
            speedup = f"{times['python'] / times['rust_vm']:.1f}x"
        else:
            speedup = "N/A"
        print(f"{name:<20} {py_str:>10} {rust_str:>10} {speedup:>10}")

    # Save baselines
    if not getattr(args, "no_save", False):
        baseline_path = bench_dir / "baselines.json"
        serializable = {
            name: {k: v for k, v in times.items()}
            for name, times in results.items()
        }
        with open(baseline_path, "w", encoding="utf-8") as f:
            json.dump(serializable, f, indent=2)
        print(f"\n{_green('Baselines saved to')} {baseline_path}")


# ---------------------------------------------------------------------------
# Registration -- called from cli.py
# ---------------------------------------------------------------------------

def register_bench_command(subparsers: argparse._SubParsersAction) -> Dict[str, Any]:
    """Register the ``bench`` subcommand.

    Returns a dict mapping command name -> handler function.
    """
    p = subparsers.add_parser(
        "bench",
        help="Run the benchmark suite and report timings",
    )
    p.add_argument(
        "--dir",
        default=None,
        help="Benchmark directory (default: benchmarks/)",
    )
    p.add_argument(
        "--name",
        default=None,
        choices=BENCHMARKS,
        help="Run a single benchmark by name",
    )
    p.add_argument(
        "--timeout",
        type=int,
        default=120,
        help="Timeout per benchmark in seconds (default: 120)",
    )
    p.add_argument(
        "--no-save",
        action="store_true",
        help="Don't save baselines.json",
    )

    return {"bench": _run_bench}
