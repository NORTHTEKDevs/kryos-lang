#!/usr/bin/env python3
"""Memory-safety fuzzer: run generated programs under KRYOS_FREE_DIAG and
flag DOUBLE-FREE reports. Output-diff fuzzing misses heap bugs that corrupt
memory without changing observable output (a double-free of a temp whose
slot is not reused, an over-release balanced by a leak). This sweep catches
those by asking the runtime's free-diagnostics to report over-frees.

Reuses gen2's generator for program variety. Deterministic per seed.
"""
import argparse
import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(__file__))
import gen2  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--count", type=int, default=500)
    ap.add_argument("--seed-base", type=int, default=0)
    ap.add_argument("--stmts", type=int, default=24)
    ap.add_argument("--keep-fails", default=None)
    ap.add_argument(
        "--kryos",
        default=os.path.join(os.path.dirname(__file__), "..", "..", "compiler",
                             "target", "release", "kryos.exe"),
    )
    args = ap.parse_args()
    kryos = os.path.abspath(args.kryos)
    work = tempfile.mkdtemp(prefix="kryos_mem_")
    env = dict(os.environ)
    env["KRYOS_FREE_DIAG"] = "1"
    env["KRYOS_FREE_DIAG_MAX"] = "50"

    total = flagged = 0
    for i in range(args.count):
        seed = args.seed_base + i
        try:
            src = gen2.Gen(seed).program(args.stmts)
        except Exception:
            continue
        total += 1
        path = os.path.join(work, f"m{seed}.kry")
        with open(path, "w", encoding="utf8", newline="\n") as f:
            f.write(src)
        try:
            r = subprocess.run([kryos, "run", path], capture_output=True,
                               text=True, timeout=40, errors="replace")
        except subprocess.TimeoutExpired:
            continue
        blob = (r.stdout or "") + (r.stderr or "")
        if "DOUBLE-FREE" in blob:
            flagged += 1
            print(f"[{seed}] DOUBLE-FREE", flush=True)
            line = next((l for l in blob.splitlines() if "DOUBLE-FREE" in l), "")
            print("  " + line[:140], flush=True)
            if args.keep_fails:
                os.makedirs(args.keep_fails, exist_ok=True)
                import shutil
                shutil.copy(path, os.path.join(args.keep_fails, f"m{seed}.kry"))

    print(f"memsafety-fuzz: {total} programs, {flagged} with double-free", flush=True)
    sys.exit(1 if flagged else 0)


if __name__ == "__main__":
    main()
