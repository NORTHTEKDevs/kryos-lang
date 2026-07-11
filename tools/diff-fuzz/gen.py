#!/usr/bin/env python3
"""Differential fuzzer for Kryos: generates random type-correct programs,
runs each on BOTH backends (Cranelift JIT via `kryos run`, LLVM AOT via
`kryos build --release`), and diffs stdout + exit code.

Any divergence is a miscompile candidate; any compiler panic is an ICE;
any clang rejection is an IR-shape bug. Deterministic per seed.

Usage: python gen.py --count 300 [--seed-base 0] [--keep-fails DIR]
"""
import argparse
import os
import random
import shutil
import subprocess
import sys
import tempfile
import time

INT_BINOPS = ["+", "-", "*"]
CMP_OPS = ["==", "!=", "<", "<=", ">", ">="]


class Gen:
    def __init__(self, seed):
        self.r = random.Random(seed)
        self.lines = []
        self.indent = 1
        self.int_vars = []
        self.str_vars = []
        self.arr_vars = []
        self.tmp = 0
        self.stmt_budget = 0

    def name(self, p):
        self.tmp += 1
        return f"{p}{self.tmp}"

    def w(self, s):
        self.lines.append("    " * self.indent + s)

    # ---- expressions -------------------------------------------------
    def int_expr(self, depth=0):
        r = self.r
        if depth > 2 or r.random() < 0.35:
            if self.int_vars and r.random() < 0.6:
                return r.choice(self.int_vars)
            return str(r.randint(-50, 100))
        k = r.random()
        if k < 0.55:
            op = r.choice(INT_BINOPS)
            return f"({self.int_expr(depth + 1)} {op} {self.int_expr(depth + 1)})"
        if k < 0.7 and self.arr_vars:
            a = r.choice(self.arr_vars)
            return f"len({a})"
        if k < 0.85 and self.str_vars:
            return f"len({r.choice(self.str_vars)})"
        # guarded division: divisor never zero
        d = r.randint(1, 9)
        return f"({self.int_expr(depth + 1)} / {d})"

    def bool_expr(self):
        r = self.r
        return f"{self.int_expr()} {r.choice(CMP_OPS)} {self.int_expr()}"

    def str_expr(self, depth=0):
        r = self.r
        if depth > 1 or r.random() < 0.4:
            if self.str_vars and r.random() < 0.5:
                return r.choice(self.str_vars)
            word = "".join(r.choice("abcxyz") for _ in range(r.randint(1, 5)))
            return f'"{word}"'
        k = r.random()
        if k < 0.6:
            return f"({self.str_expr(depth + 1)} + {self.str_expr(depth + 1)})"
        return f"to_string({self.int_expr(depth + 1)})"

    # ---- statements ---------------------------------------------------
    def stmt(self, depth=0):
        if self.stmt_budget <= 0:
            return
        self.stmt_budget -= 1
        r = self.r
        k = r.random()
        if k < 0.22:
            v = self.name("i")
            self.w(f"let mut {v} = {self.int_expr()}")
            self.int_vars.append(v)
        elif k < 0.34:
            v = self.name("s")
            self.w(f"let mut {v} = {self.str_expr()}")
            self.str_vars.append(v)
        elif k < 0.42:
            v = self.name("a")
            elems = ", ".join(self.int_expr() for _ in range(r.randint(1, 4)))
            self.w(f"let mut {v} = [{elems}]")
            self.arr_vars.append(v)
        elif k < 0.52 and self.int_vars:
            v = r.choice(self.int_vars)
            self.w(f"{v} = {self.int_expr()}")
        elif k < 0.58 and self.arr_vars:
            a = r.choice(self.arr_vars)
            self.w(f"{a} = push({a}, {self.int_expr()})")
        elif k < 0.63 and self.arr_vars:
            # `let mut b = a; b = push(b, x)` -- exercises independent-copy
            # semantics (array_dup). b must not alias a.
            src = r.choice(self.arr_vars)
            b = self.name("a")
            self.w(f"let mut {b} = {src}")
            self.w(f"{b} = push({b}, {self.int_expr()})")
            self.arr_vars.append(b)
        elif k < 0.70 and depth < 2:
            snap = (list(self.int_vars), list(self.str_vars), list(self.arr_vars))
            self.w(f"if {self.bool_expr()} {{")
            self.indent += 1
            for _ in range(r.randint(1, 3)):
                self.stmt(depth + 1)
            self.w(f'println("br" + to_string({self.int_expr()}))')
            self.int_vars, self.str_vars, self.arr_vars = (list(snap[0]), list(snap[1]), list(snap[2]))
            self.indent -= 1
            self.w("} else {")
            self.indent += 1
            self.stmt(depth + 1)
            self.indent -= 1
            self.w("}")
            self.int_vars, self.str_vars, self.arr_vars = snap
        elif k < 0.80 and depth < 2:
            v = self.name("l")
            n = r.randint(2, 6)
            self.w(f"let mut {v} = 0")
            # counter NOT exposed to body (would break loop termination)
            snap = (list(self.int_vars), list(self.str_vars), list(self.arr_vars))
            self.w(f"while {v} < {n} {{")
            self.indent += 1
            self.stmt(depth + 1)
            self.w(f"{v} = {v} + 1")
            self.indent -= 1
            self.w("}")
            self.int_vars, self.str_vars, self.arr_vars = snap
        elif k < 0.9:
            self.w(f'println("v=" + to_string({self.int_expr()}))')
        else:
            self.w(f'println("s=" + {self.str_expr()})')

    def observe(self):
        # Print all live state so divergence anywhere becomes visible.
        for v in self.int_vars:
            self.w(f'println("{v}=" + to_string({v}))')
        for v in self.str_vars:
            self.w(f'println("{v}=" + {v})')
        for a in self.arr_vars:
            self.w(f'println("{a}.len=" + to_string(len({a})))')
            self.w(f"if len({a}) > 0 {{")
            self.indent += 1
            self.w(f'println("{a}[0]=" + to_string({a}[0]))')
            self.indent -= 1
            self.w("}")

    def helper_fn(self):
        r = self.r
        fname = self.name("f")
        body_expr = f"(a {r.choice(INT_BINOPS)} b)"
        return fname, [
            f"fn {fname}(a: i64, b: i64) -> i64 {{",
            f"    return {body_expr} {r.choice(INT_BINOPS)} {r.randint(1, 9)}",
            "}",
            "",
        ]

    def program(self, n_stmts):
        header = []
        fns = []
        for _ in range(self.r.randint(0, 2)):
            fname, lines = self.helper_fn()
            fns.append(fname)
            header.extend(lines)
        self.lines = []
        self.stmt_budget = n_stmts
        while self.stmt_budget > 0:
            if fns and self.r.random() < 0.15:
                self.stmt_budget -= 1
                f = self.r.choice(fns)
                self.w(
                    f'println("call=" + to_string({f}({self.int_expr()}, {self.int_expr()})))'
                )
            else:
                self.stmt(0)
        self.observe()
        body = "\n".join(self.lines)
        return "\n".join(header) + "fn main() {\n" + body + "\n}\n"


def run(cmd, timeout, cwd=None):
    try:
        p = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout, cwd=cwd
        )
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return -999, "", "TIMEOUT"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--count", type=int, default=100)
    ap.add_argument("--seed-base", type=int, default=0)
    ap.add_argument("--stmts", type=int, default=25)
    ap.add_argument("--keep-fails", default=None)
    ap.add_argument(
        "--kryos",
        default=os.path.join(
            os.path.dirname(__file__), "..", "..", "compiler", "target", "release", "kryos.exe"
        ),
    )
    args = ap.parse_args()

    kryos = os.path.abspath(args.kryos)
    work = tempfile.mkdtemp(prefix="kryos_dfuzz_")
    fails = 0
    ices = 0
    diffs = 0
    for i in range(args.count):
        seed = args.seed_base + i
        src = Gen(seed).program(args.stmts)
        path = os.path.join(work, f"p{seed}.kry")
        with open(path, "w", encoding="utf8", newline="\n") as f:
            f.write(src)

        jit_rc, jit_out, jit_err = run([kryos, "run", path], 40)
        exe = os.path.join(work, f"p{seed}")
        aot_rc, _, aot_berr = run(
            [kryos, "build", path, "--release", "-o", exe], 90
        )
        verdict = None
        if "panicked" in jit_err or "panicked" in aot_berr:
            verdict = "ICE"
            ices += 1
        elif jit_rc != 0:
            verdict = f"JIT_FAIL rc={jit_rc}"
        elif aot_rc != 0:
            verdict = f"AOT_BUILD_FAIL"
        else:
            exe_path = exe + ".exe" if os.path.exists(exe + ".exe") else exe
            if not os.path.exists(exe_path) and os.path.exists(exe):
                shutil.copy(exe, exe + ".exe")
                exe_path = exe + ".exe"
            time.sleep(0.15)
            arc, aout, aerr = run([exe_path], 40)
            if arc != jit_rc or aout != jit_out:
                verdict = "DIVERGENCE"
                diffs += 1
        if verdict:
            fails += 1
            print(f"[{seed}] {verdict}")
            if args.keep_fails:
                os.makedirs(args.keep_fails, exist_ok=True)
                shutil.copy(path, os.path.join(args.keep_fails, f"p{seed}.kry"))
        if (i + 1) % 25 == 0:
            print(f"  ...{i + 1}/{args.count} done ({fails} flagged)")

    print(
        f"diff-fuzz: {args.count} programs, {fails} flagged "
        f"({ices} ICE, {diffs} output-divergence)"
    )
    shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
