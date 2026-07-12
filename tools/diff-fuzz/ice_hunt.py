#!/usr/bin/env python3
"""ICE hunt: the compiler must NEVER panic on malformed input.

Takes valid corpus programs, applies mutations (truncation, byte flips,
token deletion/duplication, bracket vandalism, random splices), runs
`kryos check` on each mutant, and flags any Rust panic / internal
compiler error. Diagnostics + nonzero exit are the correct outcome;
a panic is a bug. Deterministic per seed.
"""
import argparse
import glob
import os
import random
import subprocess
import sys
import tempfile

PANIC_MARKS = ("panicked at", "RUST_BACKTRACE", "internal compiler error",
               "thread 'main' panicked", "stack overflow")


def mutants(src, rng, n):
    out = []
    for _ in range(n):
        kind = rng.randrange(7)
        s = src
        if kind == 0 and len(s) > 10:                  # truncate
            s = s[: rng.randrange(1, len(s))]
        elif kind == 1 and len(s) > 10:                # byte flip
            i = rng.randrange(len(s))
            s = s[:i] + chr(rng.randrange(32, 127)) + s[i + 1:]
        elif kind == 2:                                # delete a random line
            lines = s.split("\n")
            if len(lines) > 2:
                del lines[rng.randrange(len(lines))]
            s = "\n".join(lines)
        elif kind == 3:                                # duplicate a random line
            lines = s.split("\n")
            i = rng.randrange(len(lines))
            lines.insert(i, lines[i])
            s = "\n".join(lines)
        elif kind == 4:                                # bracket vandalism
            for ch, rep in ((rng.choice("{}()[]"), ""),):
                s = s.replace(ch, rep, rng.randrange(1, 4))
        elif kind == 5 and len(s) > 20:                # splice two halves swapped
            i = rng.randrange(5, len(s) - 5)
            s = s[i:] + s[:i]
        else:                                          # inject junk token
            lines = s.split("\n")
            i = rng.randrange(len(lines))
            junk = rng.choice(["@@@@", "let let", "}} {{", '"unterminated',
                               "fn fn(", "match {", "1e999999", "::::",
                               "\x00", "/* unclosed", "'", "\\u{FFFFFF}"])
            lines.insert(i, junk)
            s = "\n".join(lines)
        out.append(s)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--per-file", type=int, default=8)
    ap.add_argument("--corpus", default=None, help="glob of .kry files")
    ap.add_argument(
        "--kryos",
        default=os.path.join(os.path.dirname(__file__), "..", "..", "compiler",
                             "target", "release", "kryos.exe"),
    )
    args = ap.parse_args()
    kryos = os.path.abspath(args.kryos)
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    pats = args.corpus or os.path.join(root, "tests", "smoke", "*.kry")
    files = sorted(glob.glob(pats))
    rng = random.Random(args.seed)
    work = tempfile.mkdtemp(prefix="kryos_ice_")
    total = ices = 0
    for f in files:
        try:
            src = open(f, encoding="utf8", errors="replace").read()
        except OSError:
            continue
        for j, m in enumerate(mutants(src, rng, args.per_file)):
            total += 1
            p = os.path.join(work, f"m{total}.kry")
            with open(p, "w", encoding="utf8", errors="replace", newline="\n") as fh:
                fh.write(m)
            try:
                r = subprocess.run([kryos, "check", p], capture_output=True,
                                   text=True, timeout=30, errors="replace")
                blob = (r.stdout or "") + (r.stderr or "")
                if any(mark in blob for mark in PANIC_MARKS):
                    ices += 1
                    keep = os.path.join(work, f"ICE_{os.path.basename(f)}_{j}.kry")
                    os.replace(p, keep)
                    print(f"ICE: {keep}", flush=True)
                    print("  " + blob.strip().split("\n")[0][:150], flush=True)
            except subprocess.TimeoutExpired:
                ices += 1
                print(f"HANG(30s): mutant of {os.path.basename(f)} #{j}", flush=True)
    print(f"ice-hunt: {total} mutants, {ices} ICE/hang", flush=True)
    sys.exit(1 if ices else 0)


if __name__ == "__main__":
    main()
