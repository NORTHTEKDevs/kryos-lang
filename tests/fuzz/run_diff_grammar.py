#!/usr/bin/env python3
"""Differential JIT/AOT fuzz driver for tests/fuzz/gen_grammar.py.

Same diff contract as run_diff.py (stdout + exit code across `kryos run` vs
`kryos build --release`), extended with a THIRD bucket this task's ledger
explicitly asked for: when BOTH backends reject the same program with the
same error class, that is not a stdout divergence (nothing to diff -- see
gen_fuzz.py's own note on "cannot see both backends agree by being equally
broken"), but it IS worth surfacing, because the one bug this whole harness
family has found so far (the `Box_` trailing-underscore mangling bug) was
exactly this shape: both backends failing identically on a program that
should have compiled. `--report-both-fail` prints these separately; they are
never counted in the divergence rate.

Usage:
    python run_diff_grammar.py --scenarios all --seeds 1-200
    python run_diff_grammar.py --scenario mega_combo --seed 12345 -v
    python run_diff_grammar.py --scenarios all --seeds 1-50 --keep-dir out/
"""
import argparse
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
GEN = HERE / "gen_grammar.py"
KRYOS = REPO / "compiler" / "target" / "release" / "kryos.exe"
if not KRYOS.exists():
    KRYOS = REPO / "compiler" / "target" / "release" / "kryos"

RUN_TIMEOUT = 60
BUILD_TIMEOUT = 90

SCENARIOS = [
    "generic_multi_type", "closure_curry_escape", "dyn_trait_story",
    "spawn_channels", "actor_story", "enum_option_result_tuple",
    "mega_combo", "generic_fn_multi_inst", "narrow_cast_boundaries",
]


def parse_seed_range(spec):
    seeds = []
    for part in spec.split(","):
        part = part.strip()
        if "-" in part:
            a, b = part.split("-", 1)
            seeds.extend(range(int(a), int(b) + 1))
        else:
            seeds.append(int(part))
    return seeds


def generate(seed, scenario, out_path):
    cmd = [sys.executable, str(GEN), "--seed", str(seed), "--scenario", scenario, "-o", str(out_path)]
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    if r.returncode != 0:
        raise RuntimeError(f"generator failed for seed {seed}/{scenario}: {r.stderr}")


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


def diff_one(seed, scenario, work_dir, verbose=False):
    """Returns (verdict, detail). verdict in {'match', 'diverge', 'both-fail', 'error'}."""
    src_path = work_dir / f"seed_{seed}_{scenario}.kry"
    exe_path = work_dir / f"seed_{seed}_{scenario}.exe"
    try:
        generate(seed, scenario, src_path)
    except Exception as e:
        return "error", {"seed": seed, "scenario": scenario, "stage": "generate", "msg": str(e)}

    jit_rc, jit_out, jit_err, jit_timeout = run_jit(src_path)
    aot_rc, aot_out, aot_err, aot_timeout, aot_stage = run_aot(src_path, exe_path)

    detail = {
        "seed": seed, "scenario": scenario, "src": str(src_path),
        "jit_rc": jit_rc, "jit_timeout": jit_timeout, "jit_err": jit_err[-800:],
        "aot_rc": aot_rc, "aot_timeout": aot_timeout, "aot_err": aot_err[-800:],
        "aot_stage": aot_stage,
    }

    # Both backends reject the SOURCE identically (JIT run also fails to
    # compile, with rc != 0 and no successful execution, matching AOT's
    # build-failed) -- both-fail bucket, not a stdout divergence.
    jit_failed_to_run_program = jit_rc is not None and jit_rc != 0 and (jit_err.strip() or not jit_out.strip())
    if aot_stage == "build-failed" and jit_failed_to_run_program and not jit_timeout:
        detail["reason"] = "both backends reject (possible shared-MIR/checker bug, not a JIT/AOT divergence)"
        return "both-fail", detail

    if jit_timeout or aot_timeout:
        detail["reason"] = "timeout"
        return "diverge", detail

    if aot_stage == "build-failed":
        detail["reason"] = "aot-build-failed (jit succeeded)"
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
        print(f"  seed {seed}/{scenario}: MATCH (rc={jit_rc}, {len(jit_out.splitlines())} lines)")
    return "match", detail


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seed", type=int, help="single seed (implies -v)")
    ap.add_argument("--seeds", type=str, help="range spec, e.g. 1-200")
    ap.add_argument("--scenario", type=str, default=None, help="single scenario name")
    ap.add_argument("--scenarios", type=str, default="all", help='"all" (every scenario) or comma list')
    ap.add_argument("--keep-dir", type=str, default=None)
    ap.add_argument("-v", "--verbose", action="store_true")
    ap.add_argument("--max-report", type=int, default=15)
    ap.add_argument("--report-both-fail", action="store_true", help="print both-fail cases too")
    args = ap.parse_args()

    if args.seed is not None:
        seeds = [args.seed]
        args.verbose = True
    elif args.seeds:
        seeds = parse_seed_range(args.seeds)
    else:
        ap.error("pass --seed or --seeds")

    if args.scenario:
        scenarios = [args.scenario]
    elif args.scenarios == "all":
        scenarios = list(SCENARIOS) + ["all"]  # "all" scenario = shuffled combo of everything
    else:
        scenarios = [s.strip() for s in args.scenarios.split(",")]

    if args.keep_dir:
        work_dir = Path(args.keep_dir)
        work_dir.mkdir(parents=True, exist_ok=True)
        cleanup = False
    else:
        import tempfile
        work_dir = Path(tempfile.mkdtemp(prefix="kryos_grammar_fuzz_"))
        cleanup = True

    matches, diverges, both_fails, errors = 0, [], [], []
    total = 0
    t0 = time.time()
    for seed in seeds:
        for scenario in scenarios:
            total += 1
            verdict, detail = diff_one(seed, scenario, work_dir, args.verbose)
            if verdict == "match":
                matches += 1
            elif verdict == "both-fail":
                both_fails.append(detail)
                if args.report_both_fail:
                    print(f"  seed {seed}/{scenario}: BOTH-FAIL -- {detail['reason']}")
            elif verdict == "diverge":
                diverges.append(detail)
                print(f"  seed {seed}/{scenario}: DIVERGE -- {detail['reason']}")
            else:
                errors.append(detail)
                print(f"  seed {seed}/{scenario}: GEN-ERROR -- {detail['msg']}")

    elapsed = time.time() - t0
    print()
    print(f"== {total} cases in {elapsed:.1f}s: {matches} match, {len(diverges)} diverge, "
          f"{len(both_fails)} both-fail, {len(errors)} generator errors ==")
    if total:
        rate = 100.0 * len(diverges) / total
        print(f"   divergence rate: {rate:.2f}%")

    for d in diverges[: args.max_report]:
        print(f"\n--- divergence: seed {d['seed']}/{d['scenario']} ({d['src']}) ---")
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

    if args.report_both_fail:
        for d in both_fails[: args.max_report]:
            print(f"\n--- both-fail: seed {d['seed']}/{d['scenario']} ({d['src']}) ---")
            print(f"  jit_err tail: {d['jit_err'].strip()[-300:]}")
            print(f"  aot_err tail: {d['aot_err'].strip()[-300:]}")

    if cleanup and not diverges and not errors:
        import shutil
        shutil.rmtree(work_dir, ignore_errors=True)
    elif diverges or errors or (args.report_both_fail and both_fails):
        print(f"\n(generated sources kept at: {work_dir})")

    return 1 if (diverges or errors) else 0


if __name__ == "__main__":
    sys.exit(main())
