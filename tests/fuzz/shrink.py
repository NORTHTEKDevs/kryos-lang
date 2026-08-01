#!/usr/bin/env python3
"""Delta-debug (ddmin) a divergence found by run_diff.py down to a minimal repro.

Takes a .kry source that reproduces a JIT/AOT divergence and repeatedly
deletes chunks of lines, keeping any deletion that (a) still type-checks
(`kryos check`) and (b) still reproduces a divergence of the SAME KIND
(build failure, exit-code mismatch, or stdout mismatch). This is a plain
ddmin: shrink the chunk size geometrically, restart from chunk size 2 after
any successful deletion, stop when chunk size exceeds the remaining line
count.

Usage:
    python shrink.py path/to/case.kry -o minimal.kry
    python shrink.py --seed 4821 --blocks 20 -o minimal.kry   # generate first

This does NOT try to also simplify identifiers/literals -- only whole-line
deletion. For this harness's block-structured generator that already gets
down to "one offending block plus the fixed prelude" in practice, which is
minimal enough to read and root-cause by hand.
"""
import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_diff  # noqa: E402

KRYOS = run_diff.KRYOS


def compiles(lines, work_dir):
    p = work_dir / "cand.kry"
    p.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    r = subprocess.run([str(KRYOS), "check", str(p)], capture_output=True, text=True, timeout=30)
    return r.returncode == 0, p


def classify(src_path, work_dir):
    """Same divergence taxonomy as run_diff.diff_one, but operating on an
    already-materialized file instead of generating one."""
    exe_path = work_dir / "cand.exe"
    jit_rc, jit_out, jit_err, jit_timeout = run_diff.run_jit(src_path)
    aot_rc, aot_out, aot_err, aot_timeout, aot_stage = run_diff.run_aot(src_path, exe_path)

    if jit_timeout or aot_timeout:
        return "timeout"
    if aot_stage == "build-failed":
        return "aot-build-failed"
    if jit_rc != aot_rc:
        return f"exit-mismatch:{jit_rc}:{aot_rc}"
    if jit_out != aot_out:
        return "stdout-mismatch"
    return None  # no divergence


def still_diverges(lines, target_kind, work_dir):
    ok, path = compiles(lines, work_dir)
    if not ok:
        return False
    kind = classify(path, work_dir)
    if kind is None:
        return False
    # An exit-mismatch's exact codes may shift as we delete unrelated blocks
    # (e.g. removing a try/throw block changes which line panics) -- treat
    # any exit-mismatch as the same kind as any other exit-mismatch so
    # shrinking isn't blocked chasing an exact code match. Build-failure and
    # stdout-mismatch stay exact-category matches.
    def norm(k):
        return "exit-mismatch" if k.startswith("exit-mismatch") else k
    return norm(kind) == norm(target_kind)


def ddmin(lines, target_kind, work_dir, max_evals=600):
    n = 2
    evals = 0
    changed = True
    while len(lines) >= 2 and n <= len(lines) and evals < max_evals:
        chunk_size = max(1, len(lines) // n)
        removed_any = False
        i = 0
        while i < len(lines) and evals < max_evals:
            candidate = lines[:i] + lines[i + chunk_size:]
            evals += 1
            if candidate and still_diverges(candidate, target_kind, work_dir):
                lines = candidate
                removed_any = True
                # restart scan from the same position, don't advance i
            else:
                i += chunk_size
        if removed_any:
            n = 2
        else:
            n *= 2
    return lines, evals


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("src", nargs="?", help="existing .kry file that diverges")
    ap.add_argument("--seed", type=int)
    ap.add_argument("--blocks", type=int, default=None)
    ap.add_argument("-o", "--out", required=True)
    ap.add_argument("--max-evals", type=int, default=600)
    args = ap.parse_args()

    work_dir = Path(tempfile.mkdtemp(prefix="kryos_shrink_"))

    if args.src:
        src_path = Path(args.src)
    elif args.seed is not None:
        src_path = work_dir / "orig.kry"
        run_diff.generate(args.seed, args.blocks, src_path)
    else:
        ap.error("pass a source file or --seed")

    lines = src_path.read_text(encoding="utf-8").splitlines()

    kind = classify(src_path, work_dir)
    if kind is None:
        print("input does not reproduce a divergence -- nothing to shrink")
        return 1
    print(f"starting divergence kind: {kind}  ({len(lines)} lines)")

    minimal, evals = ddmin(lines, kind, work_dir, args.max_evals)
    print(f"shrunk {len(lines)} -> {len(minimal)} lines in {evals} evaluations")

    out_path = Path(args.out)
    out_path.write_text("\n".join(minimal) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {out_path}")

    # Final sanity re-check.
    ok, checked = compiles(minimal, work_dir)
    final_kind = classify(checked, work_dir) if ok else None
    print(f"final check: compiles={ok} kind={final_kind}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
