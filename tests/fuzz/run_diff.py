#!/usr/bin/env python3
"""Differential JIT/AOT fuzz driver.

For each seed: generate a program (gen_fuzz.py's grammar), run it under
`kryos run` (Cranelift) and `kryos build --release` + execute (LLVM), and
diff stdout + exit code. Any mismatch (including one backend failing to
build/link where the other succeeds) is reported as a divergence.

Usage:
    python run_diff.py --seeds 1-200                  # sweep, bounded report
    python run_diff.py --seed 12345 --blocks 24 -v     # single-case replay
    python run_diff.py --seeds 1-50 --keep-dir out/    # keep generated .kry

Exit code: 0 if no divergence found, 1 if any found (so this doubles as a
CI gate driver -- see fuzz_gate.sh).
"""
import argparse
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
GEN = HERE / "gen_fuzz.py"
KRYOS = REPO / "compiler" / "target" / "release" / "kryos.exe"
if not KRYOS.exists():
    KRYOS = REPO / "compiler" / "target" / "release" / "kryos"

RUN_TIMEOUT = 60
BUILD_TIMEOUT = 90


def parse_seed_range(spec):
    """'1-200' -> range(1,201); '5' -> [5]; '1-5,9,20-22' -> combined list."""
    seeds = []
    for part in spec.split(","):
        part = part.strip()
        if "-" in part:
            a, b = part.split("-", 1)
            seeds.extend(range(int(a), int(b) + 1))
        else:
            seeds.append(int(part))
    return seeds


def generate(seed, blocks, out_path):
    cmd = [sys.executable, str(GEN), "--seed", str(seed), "-o", str(out_path)]
    if blocks is not None:
        cmd += ["--blocks", str(blocks)]
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    if r.returncode != 0:
        raise RuntimeError(f"generator failed for seed {seed}: {r.stderr}")


def run_jit(src_path):
    try:
        r = subprocess.run(
            [str(KRYOS), "run", str(src_path)],
            capture_output=True, text=True, timeout=RUN_TIMEOUT,
        )
        return r.returncode, r.stdout, r.stderr, False
    except subprocess.TimeoutExpired:
        return None, "", "", True


def run_aot(src_path, exe_path):
    try:
        b = subprocess.run(
            [str(KRYOS), "build", "--release", str(src_path), "-o", str(exe_path)],
            capture_output=True, text=True, timeout=BUILD_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return None, "", "", True, "build-timeout"
    if b.returncode != 0:
        return None, "", b.stdout + b.stderr, False, "build-failed"
    try:
        r = subprocess.run(
            [str(exe_path)], capture_output=True, text=True, timeout=RUN_TIMEOUT,
        )
        return r.returncode, r.stdout, r.stderr, False, "ok"
    except subprocess.TimeoutExpired:
        return None, "", "", True, "run-timeout"
    except OSError as e:
        return None, "", str(e), False, "exec-failed"


def diff_one(seed, blocks, work_dir, verbose=False):
    """Returns (verdict, detail_dict). verdict in {'match', 'diverge', 'error'}."""
    src_path = work_dir / f"seed_{seed}.kry"
    exe_path = work_dir / f"seed_{seed}.exe"
    try:
        generate(seed, blocks, src_path)
    except Exception as e:
        return "error", {"seed": seed, "stage": "generate", "msg": str(e)}

    jit_rc, jit_out, jit_err, jit_timeout = run_jit(src_path)
    aot_rc, aot_out, aot_err, aot_timeout, aot_stage = run_aot(src_path, exe_path)

    detail = {
        "seed": seed,
        "src": str(src_path),
        "jit_rc": jit_rc, "jit_timeout": jit_timeout, "jit_err": jit_err[-800:],
        "aot_rc": aot_rc, "aot_timeout": aot_timeout, "aot_err": aot_err[-800:],
        "aot_stage": aot_stage,
    }

    if jit_timeout or aot_timeout:
        detail["reason"] = "timeout"
        return "diverge", detail

    if aot_stage == "build-failed":
        # JIT ran (even if it crashed) but AOT could not even be built --
        # that is itself a divergence in capability, not just value.
        detail["reason"] = "aot-build-failed"
        return "diverge", detail

    if jit_rc != aot_rc:
        detail["reason"] = f"exit code jit={jit_rc} aot={aot_rc}"
        return "diverge", detail

    if jit_out != aot_out:
        detail["reason"] = "stdout differs"
        detail["jit_out"] = jit_out
        detail["aot_out"] = aot_out
        return "diverge", detail

    if verbose:
        print(f"  seed {seed}: MATCH (rc={jit_rc}, {len(jit_out.splitlines())} lines)")
    return "match", detail


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seed", type=int, help="single seed (implies -v)")
    ap.add_argument("--seeds", type=str, help="range spec, e.g. 1-200 or 1-50,500-510")
    ap.add_argument("--blocks", type=int, default=None)
    ap.add_argument("--keep-dir", type=str, default=None, help="keep generated sources/binaries here")
    ap.add_argument("-v", "--verbose", action="store_true")
    ap.add_argument("--max-report", type=int, default=10, help="max divergences to print in full")
    args = ap.parse_args()

    if args.seed is not None:
        seeds = [args.seed]
        args.verbose = True
    elif args.seeds:
        seeds = parse_seed_range(args.seeds)
    else:
        ap.error("pass --seed or --seeds")

    if args.keep_dir:
        work_dir = Path(args.keep_dir)
        work_dir.mkdir(parents=True, exist_ok=True)
        cleanup = False
    else:
        import tempfile
        work_dir = Path(tempfile.mkdtemp(prefix="kryos_fuzz_"))
        cleanup = True

    matches, diverges, errors = 0, [], []
    t0 = time.time()
    for seed in seeds:
        verdict, detail = diff_one(seed, args.blocks, work_dir, args.verbose)
        if verdict == "match":
            matches += 1
        elif verdict == "diverge":
            diverges.append(detail)
            print(f"  seed {seed}: DIVERGE -- {detail['reason']}")
        else:
            errors.append(detail)
            print(f"  seed {seed}: GEN-ERROR -- {detail['msg']}")

    elapsed = time.time() - t0
    print()
    print(f"== {len(seeds)} cases in {elapsed:.1f}s: {matches} match, "
          f"{len(diverges)} diverge, {len(errors)} generator errors ==")
    if seeds:
        rate = 100.0 * len(diverges) / len(seeds)
        print(f"   divergence rate: {rate:.1f}%")

    for d in diverges[: args.max_report]:
        print(f"\n--- divergence: seed {d['seed']} ({d['src']}) ---")
        print(f"  reason: {d['reason']}")
        print(f"  jit_rc={d['jit_rc']} timeout={d['jit_timeout']}")
        print(f"  aot_rc={d['aot_rc']} timeout={d['aot_timeout']} stage={d.get('aot_stage')}")
        if "jit_out" in d:
            jl = d["jit_out"].splitlines()
            al = d["aot_out"].splitlines()
            for i in range(max(len(jl), len(al))):
                jv = jl[i] if i < len(jl) else "<EOF>"
                av = al[i] if i < len(al) else "<EOF>"
                if jv != av:
                    print(f"    line {i}: jit={jv!r} aot={av!r}")
        if d["jit_err"].strip():
            print(f"  jit stderr tail: {d['jit_err'].strip()[-300:]}")
        if d["aot_err"].strip():
            print(f"  aot stderr tail: {d['aot_err'].strip()[-300:]}")

    if cleanup and not diverges and not errors:
        import shutil
        shutil.rmtree(work_dir, ignore_errors=True)
    elif diverges or errors:
        print(f"\n(generated sources kept at: {work_dir})")

    return 1 if (diverges or errors) else 0


if __name__ == "__main__":
    sys.exit(main())
