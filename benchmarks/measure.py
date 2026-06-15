#!/usr/bin/env python3
"""Honest benchmark harness.

Methodology (the whole point — the previous tables compared numbers at the
~30ms process-launch floor, which measures startup, not the language):

  1. Workloads are sized so the fastest competitor needs >= ~0.5s.
  2. Every binary runs WARMUP + N timed runs; we report the MEDIAN and the
     min..max spread.
  3. The per-runtime process-startup floor is measured separately (a
     hello-world per toolchain) and reported in its own column — it is NOT
     subtracted, so numbers are honest wall-clock, but readers can see how
     much of a fast result is startup.
  4. Timeouts are reported as ">Ns", not omitted.
  5. Raw results land in results.json; the markdown table is generated from
     the same data (no hand-edited numbers).

Usage:  python measure.py [--runs 5] [--timeout 180]
"""

import argparse
import json
import math
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
BIN = HERE / "bin"
BIN.mkdir(exist_ok=True)

# Canonical compute benchmarks + hashmap (general-program / map throughput).
# "strings" is intentionally excluded: the Kryos port builds with the naive
# `s = s + token` loop which is O(n^2) (concat allocates a fresh string each
# step), so it is not a like-for-like comparison against Rust's String::push_str
# / Go's strings.Builder. The files exist for the future liveness-gated
# consuming-append intrinsic; until then, measuring it would mislead.
BENCHES = ["fib", "mandelbrot", "nbody", "binary_trees", "fannkuch", "matmul", "hashmap"]

KRYOS = REPO / "compiler" / "target" / "release" / ("kryos.exe" if sys.platform == "win32" else "kryos")
EXE = ".exe" if sys.platform == "win32" else ""


def find_clang():
    c = shutil.which("clang")
    if c:
        return c
    for cand in [r"C:\Program Files\LLVM\bin\clang.exe", r"C:\Program Files (x86)\LLVM\bin\clang.exe"]:
        if Path(cand).exists():
            return cand
    return None


def find_clangpp():
    c = shutil.which("clang++")
    if c:
        return c
    for cand in [r"C:\Program Files\LLVM\bin\clang++.exe", r"C:\Program Files (x86)\LLVM\bin\clang++.exe"]:
        if Path(cand).exists():
            return cand
    return None


CLANG = find_clang()
CLANGPP = find_clangpp()
RUSTC = shutil.which("rustc")
GO = shutil.which("go")
# Mojo (Modular) — optional; only measured if a `mojo` toolchain is on PATH.
# Not available on every host (e.g. Windows without WSL+Modular). When absent
# the column is simply omitted rather than fabricated.
MOJO = shutil.which("mojo")
PYTHON = sys.executable


def run(cmd, timeout, cwd=None):
    t0 = time.perf_counter()
    try:
        r = subprocess.run(cmd, capture_output=True, timeout=timeout, cwd=cwd)
        dt = time.perf_counter() - t0
        return (dt, r.returncode)
    except subprocess.TimeoutExpired:
        return (None, "timeout")


def build_all():
    built = {}
    for b in BENCHES:
        targets = {}
        # Kryos LLVM AOT (--release)
        out = BIN / f"{b}_kry_llvm{EXE}"
        r = subprocess.run([str(KRYOS), "build", str(HERE / "kryos" / f"{b}.kry"), "--release", "--backend", "llvm", "-o", str(out)], capture_output=True)
        if r.returncode == 0:
            targets["kryos-llvm"] = [str(out)]
        else:
            print(f"  BUILD FAIL kryos-llvm {b}: {r.stderr.decode()[:200]}")
        # Kryos Cranelift AOT (debug backend, native object)
        out = BIN / f"{b}_kry_cl{EXE}"
        r = subprocess.run([str(KRYOS), "build", str(HERE / "kryos" / f"{b}.kry"), "-o", str(out)], capture_output=True)
        if r.returncode == 0:
            targets["kryos-cranelift"] = [str(out)]
        # Rust -O
        if RUSTC:
            out = BIN / f"{b}_rs{EXE}"
            r = subprocess.run([RUSTC, "-O", str(HERE / "rust" / f"{b}.rs"), "-o", str(out)], capture_output=True)
            if r.returncode == 0:
                targets["rust -O"] = [str(out)]
        # C via clang -O2
        if CLANG and (HERE / "c" / f"{b}.c").exists():
            out = BIN / f"{b}_c{EXE}"
            r = subprocess.run([CLANG, "-O2", str(HERE / "c" / f"{b}.c"), "-o", str(out)], capture_output=True)
            if r.returncode == 0:
                targets["clang -O2"] = [str(out)]
        # C++ via clang++ -O2 (idiomatic: std::vector / std::unordered_map / new-delete)
        if CLANGPP and (HERE / "cpp" / f"{b}.cpp").exists():
            out = BIN / f"{b}_cpp{EXE}"
            r = subprocess.run([CLANGPP, "-O2", "-std=c++17", str(HERE / "cpp" / f"{b}.cpp"), "-o", str(out)], capture_output=True)
            if r.returncode == 0:
                targets["clang++ -O2"] = [str(out)]
        # Mojo (optional; only if toolchain present)
        if MOJO and (HERE / "mojo" / f"{b}.mojo").exists():
            out = BIN / f"{b}_mojo{EXE}"
            r = subprocess.run([MOJO, "build", str(HERE / "mojo" / f"{b}.mojo"), "-o", str(out)], capture_output=True)
            if r.returncode == 0:
                targets["mojo"] = [str(out)]
        # Go
        if GO and (HERE / "go" / f"{b}.go").exists():
            out = BIN / f"{b}_go{EXE}"
            r = subprocess.run([GO, "build", "-o", str(out), str(HERE / "go" / f"{b}.go")], capture_output=True, cwd=str(HERE))
            if r.returncode == 0:
                targets["go"] = [str(out)]
        # Python
        if (HERE / "python" / f"{b}.py").exists():
            targets["python"] = [PYTHON, str(HERE / "python" / f"{b}.py")]
        built[b] = targets
    return built


def startup_floor(runs):
    """Hello-world wall-clock per runtime: the process-launch floor."""
    floors = {}
    hello_kry = HERE / "_hello_floor.kry"
    hello_kry.write_text('fn main() {\n    println("hi")\n}\n', encoding="utf-8")
    out = BIN / f"_hello_kry{EXE}"
    if subprocess.run([str(KRYOS), "build", str(hello_kry), "--release", "--backend", "llvm", "-o", str(out)], capture_output=True).returncode == 0:
        times = [run([str(out)], 30)[0] for _ in range(runs)]
        floors["kryos (native exe)"] = statistics.median([t for t in times if t])
    hello_rs = HERE / "_hello.rs"
    hello_rs.write_text('fn main() { println!("hi"); }\n', encoding="utf-8")
    out = BIN / f"_hello_rs{EXE}"
    if RUSTC and subprocess.run([RUSTC, "-O", str(hello_rs), "-o", str(out)], capture_output=True).returncode == 0:
        times = [run([str(out)], 30)[0] for _ in range(runs)]
        floors["rust (native exe)"] = statistics.median([t for t in times if t])
    times = [run([PYTHON, "-c", "print('hi')"], 30)[0] for _ in range(runs)]
    floors["python interpreter"] = statistics.median([t for t in times if t])
    hello_kry.unlink(missing_ok=True)
    hello_rs.unlink(missing_ok=True)
    return floors


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--only", type=str, default="", help="comma-separated bench subset; merges into existing results.json")
    args = ap.parse_args()
    global BENCHES
    prior = {}
    rp = HERE / "results.json"
    if args.only:
        BENCHES = [b.strip() for b in args.only.split(",") if b.strip()]
        if rp.exists():
            prior = json.loads(rp.read_text(encoding="utf-8")).get("benches", {})

    print("building all targets...")
    built = build_all()
    print("measuring startup floors...")
    floors = startup_floor(args.runs)

    results = {"floors": floors, "runs": args.runs, "timeout_s": args.timeout, "benches": dict(prior)}
    for b in BENCHES:
        results["benches"][b] = {}
        for lang, cmd in built[b].items():
            # warmup
            run(cmd, args.timeout)
            times = []
            timed_out = False
            for _ in range(args.runs):
                dt, rc = run(cmd, args.timeout)
                if dt is None:
                    timed_out = True
                    break
                if rc != 0:
                    times = []
                    break
                times.append(dt)
            if timed_out:
                results["benches"][b][lang] = {"status": "timeout"}
                print(f"  {b:14s} {lang:16s} >timeout({args.timeout}s)")
            elif times:
                med = statistics.median(times)
                results["benches"][b][lang] = {
                    "status": "ok",
                    "median_s": round(med, 4),
                    "min_s": round(min(times), 4),
                    "max_s": round(max(times), 4),
                }
                print(f"  {b:14s} {lang:16s} median {med:8.3f}s  ({min(times):.3f}..{max(times):.3f})")
            else:
                results["benches"][b][lang] = {"status": "error"}
                print(f"  {b:14s} {lang:16s} ERROR")

    (HERE / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    print(f"\nwrote {HERE / 'results.json'}")

    # Markdown table generated from the same data.
    lines = []
    langs_order = ["kryos-llvm", "kryos-cranelift", "rust -O", "clang -O2", "clang++ -O2", "mojo", "go", "python"]
    lines.append("| Benchmark | " + " | ".join(langs_order) + " | kryos-llvm / rust |")
    lines.append("|---" * (len(langs_order) + 2) + "|")
    for b in BENCHES:
        if b not in results["benches"]:
            continue
        row = [b]
        for l in langs_order:
            e = results["benches"][b].get(l)
            if not e:
                row.append("n/a")
            elif e["status"] == "ok":
                row.append(f"{e['median_s']:.3f}s")
            elif e["status"] == "timeout":
                row.append(f">{args.timeout}s")
            else:
                row.append("error")
        k = results["benches"][b].get("kryos-llvm", {})
        r = results["benches"][b].get("rust -O", {})
        if k.get("status") == "ok" and r.get("status") == "ok" and r["median_s"] > 0:
            row.append(f"{k['median_s'] / r['median_s']:.2f}x")
        else:
            row.append("n/a")
        lines.append("| " + " | ".join(row) + " |")
    (HERE / "results_table.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {HERE / 'results_table.md'}")
    print("\nstartup floors (medians):")
    for k, v in floors.items():
        print(f"  {k}: {v*1000:.1f}ms")


if __name__ == "__main__":
    main()
