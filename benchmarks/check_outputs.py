#!/usr/bin/env python3
"""Cross-check benchmark outputs: every port of a benchmark must print the same thing.

measure.py times the ports and checks their exit codes; it does not compare
what they print. Run this right after measure.py: for every port that
results.json records as measured, it runs the binary measure.py built in bin/
(or the Python port) once, and it fails unless all ports of a benchmark exit 0
with identical stdout.

Usage:  python benchmarks/check_outputs.py
"""

import collections
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
BIN = HERE / "bin"
EXE = ".exe" if sys.platform == "win32" else ""
# measure.py's port labels -> the binary name suffix its build_all() writes
SUFFIX = {
    "kryos-llvm": "kry_llvm",
    "kryos-cranelift": "kry_cl",
    "rust -O": "rs",
    "clang -O2": "c",
    "clang++ -O2": "cpp",
    "mojo": "mojo",
    "go": "go",
}


def command(bench, label):
    if label == "python":
        return [sys.executable, str(HERE / "python" / f"{bench}.py")]
    return [str(BIN / f"{bench}_{SUFFIX[label]}{EXE}")]


def main():
    results = json.loads((HERE / "results.json").read_text(encoding="utf-8"))
    bad = 0
    for bench, ports in results["benches"].items():
        outs = collections.OrderedDict()
        for label, entry in ports.items():
            if entry.get("status") != "ok":
                continue
            cmd = command(bench, label)
            if label != "python" and not Path(cmd[0]).exists():
                outs[label] = ("missing", cmd[0])
                continue
            r = subprocess.run(cmd, capture_output=True, timeout=600)
            outs[label] = (r.returncode, r.stdout.decode(errors="replace").replace("\r\n", "\n").strip())
        same = len(outs) > 1 and len(set(outs.values())) == 1 and all(rc == 0 for rc, _ in outs.values())
        if same:
            first = next(iter(outs.values()))[1]
            print(f"{bench:13s} identical across {len(outs)} ports ({', '.join(outs)}): {first[:100]!r}")
        else:
            bad += 1
            print(f"{bench:13s} DIFFER (or fewer than two ports)")
            for label, (rc, out) in outs.items():
                print(f"    {label:16s} rc={rc} out={out[:120]!r}")
    print("outputs: PASS" if bad == 0 else f"outputs: FAIL ({bad} benchmark(s))")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
