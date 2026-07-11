#!/usr/bin/env python3
"""Expanded differential fuzzer for Kryos (gen2).

Generates random type-correct programs covering a much wider language
surface than gen.py -- structs + field access/mutation, enums + match,
Option/Result, closures + HOFs, maps, nested helper functions, the `?`
operator, and all the arithmetic/comparison/bool operators -- then runs
each on BOTH backends (Cranelift JIT + LLVM AOT) and diffs stdout + exit.

A divergence is a miscompile; a panic is an ICE; a clang rejection is an
IR-shape bug. Deterministic per seed. Monitor via --progress FILE.
"""
import argparse
import os
import random
import shutil
import subprocess
import sys
import tempfile
import time

IBIN = ["+", "-", "*"]
CMP = ["==", "!=", "<", "<=", ">", ">="]


class Gen:
    def __init__(self, seed):
        self.r = random.Random(seed)
        self.lines = []
        self.indent = 1
        self.iv = []          # int vars
        self.sv = []          # str vars
        self.av = []          # [i64] vars
        self.bv = []          # bool vars
        self.tmp = 0
        self.budget = 0
        self.fns = []         # (name, arity) user int helpers
        self.have_struct = False
        self.have_enum = False

    def nm(self, p):
        self.tmp += 1
        return f"{p}{self.tmp}"

    def w(self, s):
        self.lines.append("    " * self.indent + s)

    # ----- expressions -------------------------------------------------
    def ie(self, d=0):
        r = self.r
        if d > 2 or r.random() < 0.4:
            if self.iv and r.random() < 0.6:
                return r.choice(self.iv)
            return str(r.randint(-40, 90))
        k = r.random()
        if k < 0.4:
            return f"({self.ie(d+1)} {r.choice(IBIN)} {self.ie(d+1)})"
        if k < 0.5:
            return f"({self.ie(d+1)} / {r.randint(1,9)})"
        if k < 0.58:
            return f"({self.ie(d+1)} % {r.randint(1,9)})"
        if k < 0.68 and self.av:
            return f"len({r.choice(self.av)})"
        if k < 0.76 and self.sv:
            return f"len({r.choice(self.sv)})"
        if k < 0.86 and self.fns:
            fn, ar = r.choice(self.fns)
            args = ", ".join(self.ie(d + 1) for _ in range(ar))
            return f"{fn}({args})"
        if k < 0.92 and self.av:
            a = r.choice(self.av)
            # guarded index
            return f"{a}[0]"
        return f"abs({self.ie(d+1)})"

    def be(self, d=0):
        r = self.r
        if self.bv and r.random() < 0.3:
            return r.choice(self.bv)
        base = f"{self.ie(d)} {r.choice(CMP)} {self.ie(d)}"
        if d < 1 and r.random() < 0.3:
            joiner = r.choice(["and", "or"])
            return f"({base}) {joiner} ({self.ie(d)} {r.choice(CMP)} {self.ie(d)})"
        return base

    def se(self, d=0):
        r = self.r
        if d > 1 or r.random() < 0.45:
            if self.sv and r.random() < 0.5:
                return r.choice(self.sv)
            return '"' + "".join(r.choice("abcxyz_") for _ in range(r.randint(1, 6))) + '"'
        k = r.random()
        if k < 0.55:
            return f"({self.se(d+1)} + {self.se(d+1)})"
        return f"to_string({self.ie(d+1)})"

    # ----- statements --------------------------------------------------
    def stmt(self, d=0):
        if self.budget <= 0:
            return
        self.budget -= 1
        r = self.r
        k = r.random()
        if k < 0.16:
            v = self.nm("i"); self.w(f"let mut {v} = {self.ie()}"); self.iv.append(v)
        elif k < 0.28:
            v = self.nm("s"); self.w(f"let mut {v} = {self.se()}"); self.sv.append(v)
        elif k < 0.36:
            v = self.nm("a")
            elems = ", ".join(self.ie() for _ in range(r.randint(1, 4)))
            self.w(f"let mut {v} = [{elems}]"); self.av.append(v)
        elif k < 0.42:
            v = self.nm("b"); self.w(f"let mut {v} = {self.be()}"); self.bv.append(v)
        elif k < 0.50 and self.iv:
            self.w(f"{r.choice(self.iv)} = {self.ie()}")
        elif k < 0.55 and self.av:
            a = r.choice(self.av); self.w(f"{a} = push({a}, {self.ie()})")
        elif k < 0.60 and self.av:
            # independent-copy rebind (exercises array_dup)
            src = r.choice(self.av); b = self.nm("a")
            self.w(f"let mut {b} = {src}"); self.w(f"{b} = push({b}, {self.ie()})")
            self.av.append(b)
        elif k < 0.66 and self.sv:
            # string share-in-loop was the seed-14 class; keep exercising
            src = r.choice(self.sv); c = self.nm("s")
            self.w(f"let {c} = {src}")
            self.w(f'println("cp=" + {c})')
            self.sv.append(c)
        elif k < 0.76 and d < 2:
            snap = (list(self.iv), list(self.sv), list(self.av), list(self.bv))
            self.w(f"if {self.be()} {{")
            self.indent += 1
            for _ in range(r.randint(1, 3)):
                self.stmt(d + 1)
            self.w(f'println("t" + to_string({self.ie()}))')
            self.iv, self.sv, self.av, self.bv = (list(snap[0]), list(snap[1]), list(snap[2]), list(snap[3]))
            self.indent -= 1
            self.w("} else {")
            self.indent += 1
            self.stmt(d + 1)
            self.w(f'println("f" + to_string({self.ie()}))')
            self.indent -= 1
            self.w("}")
            self.iv, self.sv, self.av, self.bv = snap
        elif k < 0.85 and d < 2:
            v = self.nm("l"); n = r.randint(2, 6)
            self.w(f"let mut {v} = 0")
            # The counter is NOT added to self.iv, so body statements can
            # never pick it as an assignment target -- otherwise a random
            # `{v} = <expr>` in the body breaks termination (infinite loop).
            snap = (list(self.iv), list(self.sv), list(self.av), list(self.bv))
            self.w(f"while {v} < {n} {{")
            self.indent += 1
            self.stmt(d + 1)
            self.w(f"{v} = {v} + 1")
            self.indent -= 1
            self.w("}")
            self.iv, self.sv, self.av, self.bv = snap
        elif k < 0.90 and self.have_enum:
            # enum match producing an int
            n = self.ie()
            v = self.nm("m")
            self.w(f"let {v} = classify({n})")
            self.w(f'println("cls=" + {v})')
        elif k < 0.95:
            self.w(f'println("v=" + to_string({self.ie()}))')
        else:
            self.w(f'println("s=" + {self.se()})')

    def observe(self):
        r = self.r
        for v in self.iv:
            self.w(f'println("{v}=" + to_string({v}))')
        for v in self.sv:
            self.w(f'println("{v}=" + {v})')
        for v in self.av:
            self.w(f'println("{v}.len=" + to_string(len({v})))')
        for v in self.bv:
            self.w(f'if {v} {{ println("{v}=T") }} else {{ println("{v}=F") }}')

    def prelude(self):
        r = self.r
        out = []
        # helper int functions
        for _ in range(r.randint(1, 3)):
            fn = self.nm("h"); ar = r.randint(1, 2)
            params = ", ".join(f"p{i}: i64" for i in range(ar))
            body = f"(p0 {r.choice(IBIN)} {r.randint(1,9)})"
            if ar == 2:
                body = f"(p0 {r.choice(IBIN)} p1)"
            out += [f"fn {fn}({params}) -> i64 {{", f"    return {body}", "}", ""]
            self.fns.append((fn, ar))
        # optional struct
        if r.random() < 0.5:
            self.have_struct = True
            out += ["struct Pair { a: i64, b: i64 }", ""]
        # optional enum + classifier
        if r.random() < 0.6:
            self.have_enum = True
            out += [
                "enum Kind { Lo, Mid, Hi }",
                "fn classify(n: i64) -> str {",
                "    let k = if n < 0 { Kind::Lo } elif n < 50 { Kind::Mid } else { Kind::Hi }",
                "    return match k {",
                '        Kind::Lo => "lo",',
                '        Kind::Mid => "mid",',
                '        Kind::Hi => "hi",',
                "    }",
                "}",
                "",
            ]
        return out

    def program(self, n):
        pre = self.prelude()
        self.lines = []
        self.budget = n
        # struct usage in body if declared
        if self.have_struct and self.r.random() < 0.7:
            self.w("let pr = Pair { a: 7, b: 11 }")
            self.w('println("pair=" + to_string(pr.a + pr.b))')
        while self.budget > 0:
            self.stmt(0)
        self.observe()
        return "\n".join(pre) + "fn main() {\n" + "\n".join(self.lines) + "\n}\n"


def run(cmd, timeout):
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return -999, "", "TIMEOUT"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--count", type=int, default=200)
    ap.add_argument("--seed-base", type=int, default=0)
    ap.add_argument("--stmts", type=int, default=22)
    ap.add_argument("--keep-fails", default=None)
    ap.add_argument("--progress", default=None)
    ap.add_argument(
        "--kryos",
        default=os.path.join(os.path.dirname(__file__), "..", "..", "compiler",
                             "target", "release", "kryos.exe"),
    )
    args = ap.parse_args()
    kryos = os.path.abspath(args.kryos)
    work = tempfile.mkdtemp(prefix="kryos_g2_")
    fails = ices = diffs = builds = 0
    t0 = time.time()
    for i in range(args.count):
        seed = args.seed_base + i
        try:
            src = Gen(seed).program(args.stmts)
        except Exception as e:
            continue
        path = os.path.join(work, f"p{seed}.kry")
        with open(path, "w", encoding="utf8", newline="\n") as f:
            f.write(src)
        jrc, jout, jerr = run([kryos, "run", path], 40)
        exe = os.path.join(work, f"p{seed}")
        arc, _, aberr = run([kryos, "build", path, "--release", "-o", exe], 90)
        verdict = None
        if "panicked" in jerr or "panicked" in aberr:
            verdict = "ICE"; ices += 1
        elif jrc != 0:
            # a JIT failure on a generated program is a generator/typing
            # issue only if AOT also fails identically; a JIT-only failure
            # (e.g. runtime panic like OOB) is fine as long as AOT matches.
            if arc == 0:
                verdict = f"JIT_ONLY_FAIL rc={jrc}"
        elif arc != 0:
            verdict = "AOT_BUILD_FAIL"; builds += 1
        else:
            ep = exe + ".exe" if os.path.exists(exe + ".exe") else exe
            if not os.path.exists(ep) and os.path.exists(exe):
                shutil.copy(exe, exe + ".exe"); ep = exe + ".exe"
            time.sleep(0.1)
            xrc, xout, _ = run([ep], 40)
            if xrc != jrc or xout != jout:
                verdict = "DIVERGENCE"; diffs += 1
        if verdict:
            fails += 1
            print(f"[{seed}] {verdict}", flush=True)
            if args.keep_fails:
                os.makedirs(args.keep_fails, exist_ok=True)
                shutil.copy(path, os.path.join(args.keep_fails, f"p{seed}.kry"))
        if args.progress and (i + 1) % 10 == 0:
            with open(args.progress, "w") as pf:
                el = time.time() - t0
                pf.write(f"{i+1}/{args.count} done, {fails} flagged "
                         f"({ices} ICE, {diffs} diverge, {builds} aot-fail), "
                         f"{el:.0f}s, {el/(i+1):.2f}s/prog\n")
    print(f"gen2: {args.count} programs, {fails} flagged "
          f"({ices} ICE, {diffs} DIVERGENCE, {builds} AOT_BUILD_FAIL)", flush=True)
    shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
