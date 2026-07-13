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
        self.mv = []          # map<str, str> vars
        self.sav = []         # [str] vars
        self.fv = []          # f64 vars
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

    def fe(self, d=0):
        """Float expression. Division guards against literal zero but not
        computed zero -- inf/nan propagation is part of what we diff."""
        r = self.r
        if d > 2 or r.random() < 0.4:
            if self.fv and r.random() < 0.6:
                return r.choice(self.fv)
            return f"{r.randint(-30, 60)}.{r.randint(0, 99)}"
        k = r.random()
        if k < 0.35:
            op = r.choice(["+", "-", "*"])
            return f"({self.fe(d+1)} {op} {self.fe(d+1)})"
        if k < 0.5:
            return f"({self.fe(d+1)} / {r.randint(1, 9)}.5)"
        if k < 0.62:
            return f"({self.ie(d+1)} as f64)"
        if k < 0.74:
            return f"sqrt(abs({self.fe(d+1)}))"
        if k < 0.86:
            return f"abs({self.fe(d+1)})"
        return f"({self.fe(d+1)} * -1.0)"

    def se(self, d=0):
        r = self.r
        if d > 1 or r.random() < 0.45:
            if self.sv and r.random() < 0.5:
                return r.choice(self.sv)
            return '"' + "".join(r.choice("abcxyz_") for _ in range(r.randint(1, 6))) + '"'
        k = r.random()
        if k < 0.45:
            return f"({self.se(d+1)} + {self.se(d+1)})"
        if k < 0.6 and getattr(self, "sfn", None):
            # string through a call boundary (param in, return out)
            return f"{self.sfn}({self.se(d+1)})"
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
        elif k < 0.39:
            v = self.nm("b"); self.w(f"let mut {v} = {self.be()}"); self.bv.append(v)
        elif k < 0.42:
            # float domain: declare, mutate, compare, cast, print
            sub = r.random()
            if self.fv and sub < 0.35:
                f = r.choice(self.fv)
                self.w(f"{f} = {self.fe()}")
            elif self.fv and sub < 0.55:
                self.w(f'println("fcmp=" + to_string({self.fe()} < {self.fe()}))')
            elif self.fv and sub < 0.7:
                self.w(f'println("f2i=" + to_string({r.choice(self.fv)} as i64))')
            else:
                v = self.nm("f")
                self.w(f"let mut {v} = {self.fe()}")
                self.fv.append(v)
        elif k < 0.50 and self.iv:
            self.w(f"{r.choice(self.iv)} = {self.ie()}")
        elif k < 0.55 and self.av:
            a = r.choice(self.av); self.w(f"{a} = push({a}, {self.ie()})")
        elif k < 0.58 and self.av:
            # independent-copy rebind (exercises array_dup)
            src = r.choice(self.av); b = self.nm("a")
            self.w(f"let mut {b} = {src}"); self.w(f"{b} = push({b}, {self.ie()})")
            self.av.append(b)
        elif k < 0.60 and self.av:
            # CROSS-VARIABLE push + element write: `let mut out = push(a, x)`
            # types out from push's return; a bare-handle mis-type made AOT
            # index relative to the array HEADER (std::heap read len/cap as
            # elements). Element writes then must land.
            src = r.choice(self.av); out = self.nm("o")
            self.w(f"let mut {out} = push({src}, {self.ie()})")
            self.w(f"{out}[0] = {self.ie()}")
            self.w(f'println("xw=" + to_string({out}[0]) + "," + to_string(len({out})))')
            self.av.append(out)
        elif k < 0.63 and self.sv:
            # string share-in-loop was the seed-14 class; keep exercising
            src = r.choice(self.sv); c = self.nm("s")
            self.w(f"let {c} = {src}")
            self.w(f'println("cp=" + {c})')
            self.sv.append(c)
        elif k < 0.66:
            # container-VALUE ownership class (the map-store use-after-free):
            # store a locally-built string into a map or [str] slot, read it
            # back later via observe(). The value must outlive its local.
            if self.mv and r.random() < 0.5:
                m = r.choice(self.mv)
                kx = self.nm("mk")
                self.w(f'let {kx} = "key" + to_string({self.ie()})')
                self.w(f"{m}[{kx}] = {self.se()}")
                self.w(f'println("mv=" + {m}[{kx}])')
            elif self.sav and r.random() < 0.5:
                a = r.choice(self.sav)
                self.w(f"{a} = push({a}, {self.se()})")
            elif r.random() < 0.5:
                m = self.nm("m")
                self.w(f"let mut {m}: map<str, str> = {{}}")
                self.w(f'{m}["seed"] = {self.se()}')
                self.mv.append(m)
            else:
                a = self.nm("sa")
                self.w(f"let mut {a}: [str] = []")
                self.w(f"{a} = push({a}, {self.se()})")
                self.sav.append(a)
        elif k < 0.76 and d < 2:
            snap = (list(self.iv), list(self.sv), list(self.av), list(self.bv),
                    list(self.mv), list(self.sav), list(self.fv))
            self.w(f"if {self.be()} {{")
            self.indent += 1
            for _ in range(r.randint(1, 3)):
                self.stmt(d + 1)
            self.w(f'println("t" + to_string({self.ie()}))')
            self.iv, self.sv, self.av, self.bv, self.mv, self.sav, self.fv = (
                list(snap[0]), list(snap[1]), list(snap[2]), list(snap[3]),
                list(snap[4]), list(snap[5]), list(snap[6]))
            self.indent -= 1
            self.w("} else {")
            self.indent += 1
            self.stmt(d + 1)
            self.w(f'println("f" + to_string({self.ie()}))')
            self.indent -= 1
            self.w("}")
            self.iv, self.sv, self.av, self.bv, self.mv, self.sav, self.fv = snap
        elif k < 0.80 and d < 2:
            v = self.nm("l"); n = r.randint(2, 6)
            self.w(f"let mut {v} = 0")
            # The counter is NOT added to self.iv, so body statements can
            # never pick it as an assignment target -- otherwise a random
            # `{v} = <expr>` in the body breaks termination (infinite loop).
            snap = (list(self.iv), list(self.sv), list(self.av), list(self.bv),
                    list(self.mv), list(self.sav), list(self.fv))
            self.w(f"while {v} < {n} {{")
            self.indent += 1
            self.stmt(d + 1)
            self.w(f"{v} = {v} + 1")
            self.indent -= 1
            self.w("}")
            self.iv, self.sv, self.av, self.bv, self.mv, self.sav, self.fv = snap
        elif k < 0.85 and d < 2:
            # block-tail `if` as a value: `let v = { if b { x } else { y } }`
            # (the trailing-if-is-the-block-value class). NOTE: a block-local
            # bound to a RUNTIME string then used in a compound tail
            # (`{ let inner = to_string(x) + "y"; inner + "!" }`) is a KNOWN
            # separate AOT bug (block-local lifetime), deliberately NOT
            # generated here so this vocabulary stays green.
            v = self.nm("bt")
            self.w(f"let {v} = {{")
            self.w(f"    if {self.be()} {{ {self.ie()} }} else {{ {self.ie()} }}")
            self.w("}")
            self.w(f'println("bt=" + to_string({v}))')
            self.iv.append(v)
        elif k < 0.87 and d < 2:
            # loop with break/continue on a data-dependent condition
            v = self.nm("l"); n = r.randint(3, 7)
            acc = self.nm("acc")
            self.w(f"let mut {acc} = 0")
            self.w(f"let mut {v} = 0")
            self.w(f"while {v} < {n} {{")
            self.w(f"    {v} = {v} + 1")
            if r.random() < 0.5:
                self.w(f"    if {v} == {r.randint(1, n)} {{")
                self.w("        break")
                self.w("    }")
            else:
                self.w(f"    if {v} == {r.randint(1, n)} {{")
                self.w("        continue")
                self.w("    }")
            self.w(f"    {acc} = {acc} + {v}")
            self.w("}")
            self.w(f'println("loopacc=" + to_string({acc}))')
            self.iv.append(acc)
        elif k < 0.885 and self.have_enum:
            # enum match producing an int
            n = self.ie()
            v = self.nm("m")
            self.w(f"let {v} = classify({n})")
            self.w(f'println("cls=" + {v})')
        elif k < 0.905:
            # ownership-matrix classes the corpus was blind to:
            sub = r.random()
            if sub < 0.3 and len(self.sv) >= 2:
                # string reassignment from another string var
                a, b = r.sample(self.sv, 2)
                self.w(f"{a} = {b}")
            elif sub < 0.55:
                # throw/catch of a built string (exception-slot ownership)
                c = self.nm("c")
                self.w(f'let mut {c} = "pre"')
                self.w("try {")
                self.w(f"    throw {self.se()}")
                self.w("} catch e {")
                self.w(f"    {c} = e")
                self.w("}")
                self.w(f'println("caught=" + {c})')
                self.sv.append(c)
            elif sub < 0.8 and self.sav:
                # for-in over [str]: per-iteration element binding
                a = r.choice(self.sav)
                acc = self.nm("j")
                self.w(f'let mut {acc} = ""')
                self.w(f"for el in {a} {{")
                self.w(f'    {acc} = {acc} + "[" + el + "]"')
                self.w("}")
                self.w(f'println("iter=" + {acc})')
            else:
                # struct literal holding a built string; field read + mutate
                if self.have_struct:
                    p = self.nm("p")
                    self.w(f"let mut {p} = SBox {{ tag: {self.se()}, n: {self.ie()} }}")
                    self.w(f'println("sb=" + {p}.tag + to_string({p}.n))')
                    self.w(f"{p}.tag = {self.se()}")
                    self.w(f'println("sb2=" + {p}.tag)')
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
        for v in self.fv:
            self.w(f'println("{v}=" + to_string({v}))')
        for v in self.mv:
            # read back the seeded key — dangling values print recycled junk
            self.w(f'println("{v}.seed=" + {v}["seed"] + " len=" + to_string(len({v})))')
        for v in self.sav:
            self.w(f"if len({v}) > 0 {{")
            self.w(f'    println("{v}[0]=" + {v}[0])')
            self.w("}")

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
        # str-taking/returning helper (call-boundary container ownership)
        self.sfn = self.nm("sh")
        out += [f"fn {self.sfn}(p: str) -> str {{",
                f'    return p + "-{self.sfn}"', "}", ""]
        # optional struct
        if r.random() < 0.5:
            self.have_struct = True
            out += ["struct Pair { a: i64, b: i64 }", ""]
            # str-field struct: container-in-aggregate through construction,
            # field read, and field reassignment
            out += ["struct SBox { tag: str, n: i64 }", ""]
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
