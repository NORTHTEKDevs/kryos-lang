#!/usr/bin/env python3
"""Differential JIT/AOT fuzz generator for Kryos.

Emits a random-but-VALID Kryos program: a fixed shared prelude (a scalar
struct, a heap-field struct, a generic struct, an enum, and one throwing
helper) followed by N randomly chosen "block" functions -- one per category
below -- each returning a deterministic i64. `main` calls every block in
order, prints one tagged line per block ("B<i> <category>: <value>"), XORs
the value into a running checksum, and prints the checksum last.

Everything is deterministic given (seed, blocks): no clock/env/random-number
reads, no concurrency, no float op that can produce NaN/Inf, no construct
documented in CLAUDE.md as a known cross-backend divergence (e.g. NaN sign
bits, parsed "-0.0", closures escaping through a mutated container, generic
f64-closure returns, dyn-Trait containers, i128). The point of this harness
is to find NEW divergences, not to re-report cataloged ones.

Usage:
    python gen_fuzz.py --seed 12345 --blocks 24 -o case.kry
    python gen_fuzz.py --seed 12345 --blocks 24        # prints to stdout

Replay: the same (seed, blocks) always produces byte-identical output --
that's the whole reproducibility story. `--blocks` defaults to a seed-derived
value if omitted so a bare `--seed N` is still a stable, replayable case.
"""
import argparse
import random
import sys

WORDS = [
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf",
    "hotel", "india", "juliet", "kilo", "lima", "mike", "november",
    "oscar", "papa", "quebec", "romeo", "sierra", "tango", "uniform",
    "victor", "whiskey", "xray", "yankee", "zulu", "kryos", "fuzz",
    "shard", "block",
]

PRELUDE = """\
use std::iter::{map, filter, fold}

struct Point { x: i64, y: i64 }
impl Point {
    fn sum(self: Point) -> i64 {
        return self.x + self.y
    }
    fn scaled(self: Point, k: i64) -> i64 {
        return self.x * k - self.y
    }
}

struct Bag { tag: str, items: [i64] }
fn bag_checksum(b: Bag) -> i64 {
    let mut s: i64 = 0
    for v in b.items {
        s = s + v
    }
    return s + len(b.tag)
}

struct Box_<T> { val: T }
impl<T> Box_<T> {
    fn get(self: Box_<T>) -> T {
        return self.val
    }
}

enum Op {
    Add(i64, i64),
    Sub(i64, i64),
    Neg(i64),
}
fn eval_op(o: Op) -> i64 {
    match o {
        Add(a, b) => return a + b,
        Sub(a, b) => return a - b,
        Neg(a) => return 0 - a,
    }
}

fn risky(n: i64) -> i64 {
    if n % 3 == 0 {
        throw "zero-mod:{n}"
    }
    return n * 2
}
"""


def sub(text, **reps):
    for k, v in reps.items():
        text = text.replace("@@" + k + "@@", str(v))
    return text


def tmpl_int_arith(rng):
    a, b, c = (rng.randint(-500, 500) for _ in range(3))
    sh1, sh2 = rng.randint(0, 19), rng.randint(0, 19)
    mod_val = rng.choice([3, 5, 7, 11, 13, 17])
    body = """\
    let a: i64 = @@a@@
    let b: i64 = @@b@@
    let c: i64 = @@c@@
    let mut acc: i64 = a + b * c - (a - b)
    acc = acc ^ (a & b)
    acc = acc | (b ^ c)
    acc = acc + ((a << @@sh1@@) >> @@sh2@@)
    let d: f64 = (acc as f64) / 2.0
    let e: i64 = (d as i64)
    return e + (acc % @@mod_val@@)"""
    return sub(body, a=a, b=b, c=c, sh1=sh1, sh2=sh2, mod_val=mod_val)


def tmpl_float_arith(rng):
    x = round(rng.uniform(-100.0, 100.0), 3)
    y = round(rng.uniform(-100.0, 100.0), 3)
    k = rng.randint(-50, 50)
    body = """\
    let x: f64 = @@x@@
    let y: f64 = @@y@@
    let mut f: f64 = x * 2.0 + y / 4.0 - 1.5
    f = f + sqrt(abs(x) + 1.0)
    f = f * 1.000001
    let n: i64 = (f as i64)
    return n + @@k@@"""
    return sub(body, x=x, y=y, k=k)


def tmpl_string_ops(rng):
    s1 = rng.choice(WORDS)
    s2 = rng.choice(WORDS)
    combo_len = len(s1) + 1 + len(s2)
    start = rng.randint(0, combo_len)
    end = rng.randint(start, combo_len)
    needle = rng.choice(WORDS)
    body = """\
    let s1: str = "@@s1@@"
    let s2: str = "@@s2@@"
    let combo: str = s1 + "-" + s2
    let inter: str = "v{len(s1)}_{len(s2)}"
    let sub: str = substr(combo, @@start@@, @@end@@)
    let mut score: i64 = len(combo) * 3 - len(sub)
    if contains(combo, "@@needle@@") {
        score = score + 100
    } else {
        score = score - 7
    }
    if combo < inter {
        score = score + 1
    } else {
        score = score - 1
    }
    return score + len(inter)"""
    return sub(body, s1=s1, s2=s2, start=start, end=end, needle=needle)


def tmpl_array_ops(rng):
    n = rng.randint(5, 15)
    base = rng.randint(-50, 50)
    step = rng.randint(1, 9)
    mod_val = rng.choice([7, 11, 13, 17, 19])
    offset = rng.randint(-10, 10)
    body = """\
    let mut arr: [i64] = []
    let mut i: i64 = 0
    while i < @@n@@ {
        arr = push(arr, (@@base@@ + i * @@step@@) % @@mod_val@@ - @@offset@@)
        i = i + 1
    }
    let mut total: i64 = 0
    for v in arr {
        total = total + v
    }
    sort(arr)
    let first: i64 = arr[0]
    let last: i64 = arr[len(arr) - 1]
    return total + first * 2 - last"""
    return sub(body, n=n, base=base, step=step, mod_val=mod_val, offset=offset)


def tmpl_map_ops(rng):
    v1, v2, v3 = (rng.randint(-100, 100) for _ in range(3))
    body = """\
    let mut m: map<str, i64> = {}
    m["alpha"] = @@v1@@
    m["beta"] = @@v2@@
    m["gamma"] = @@v3@@
    let mut total: i64 = 0
    if contains(m, "alpha") { total = total + m["alpha"] }
    if contains(m, "beta") { total = total + m["beta"] }
    if contains(m, "delta") { total = total - 1 } else { total = total + 1 }
    m["alpha"] = m["alpha"] + @@v3@@
    return total + m["alpha"] - m["gamma"]"""
    return sub(body, v1=v1, v2=v2, v3=v3)


def tmpl_struct_scalar(rng):
    x, y, x2, y2 = (rng.randint(-200, 200) for _ in range(4))
    k = rng.randint(-10, 10)
    body = """\
    let p: Point = Point { x: @@x@@, y: @@y@@ }
    let q: Point = Point { x: @@x2@@, y: @@y2@@ }
    let s: i64 = p.sum() + q.scaled(@@k@@)
    let r: Point = Point { x: p.x + q.x, y: p.y - q.y }
    return s + r.sum()"""
    return sub(body, x=x, y=y, x2=x2, y2=y2, k=k)


def tmpl_struct_heap(rng):
    i1, i2, i3, i4 = (rng.randint(-100, 100) for _ in range(4))
    tag = rng.choice(WORDS)
    body = """\
    let items: [i64] = [@@i1@@, @@i2@@, @@i3@@, @@i4@@]
    let b: Bag = Bag { tag: "@@tag@@", items: items }
    let cs: i64 = bag_checksum(b)
    let b2: Bag = Bag { tag: b.tag + "!", items: b.items }
    return cs + bag_checksum(b2)"""
    return sub(body, i1=i1, i2=i2, i3=i3, i4=i4, tag=tag)


def tmpl_enum_match(rng):
    a1, b1, a2, b2, a3 = (rng.randint(-50, 50) for _ in range(5))
    sel = rng.randint(0, 6)
    body = """\
    let ops: [Op] = [Op.Add(@@a1@@, @@b1@@), Op.Sub(@@a2@@, @@b2@@), Op.Neg(@@a3@@)]
    let mut total: i64 = 0
    for o in ops {
        total = total + eval_op(o)
    }
    match @@sel@@ {
        1 | 2 | 3 => { total = total + 10 },
        _ => { total = total - 10 },
    }
    return total"""
    return sub(body, a1=a1, b1=b1, a2=a2, b2=b2, a3=a3, sel=sel)


def tmpl_closure_direct(rng):
    k1, k2, k3, k4, k5 = (rng.randint(-20, 20) for _ in range(5))
    body = """\
    let mul_add = |x: i64| x * @@k1@@ + @@k2@@
    let make = |n| |x| x + n
    let add5 = make(@@k3@@)
    let mut cnt: i64 = 0
    let counter = || {
        cnt = cnt + 1
        cnt
    }
    let v1 = mul_add(@@k4@@)
    let v2 = add5(@@k5@@)
    let c1 = counter()
    let c2 = counter()
    let c3 = counter()
    return v1 + v2 + c1 + c2 + c3"""
    return sub(body, k1=k1, k2=k2, k3=k3, k4=k4, k5=k5)


def tmpl_closure_hof(rng):
    nums = [rng.randint(-30, 30) for _ in range(rng.randint(4, 9))]
    lit = "[" + ", ".join(str(n) for n in nums) + "]"
    body = """\
    let nums: [i64] = @@lit@@
    let doubled = map(nums, |x| x * 2)
    let evens = filter(nums, |x| x % 2 == 0)
    let total = fold(nums, 0, |acc, x| acc + x)
    let mut esum: i64 = 0
    for v in evens { esum = esum + v }
    let mut dsum: i64 = 0
    for v in doubled { dsum = dsum + v }
    return total + esum - dsum"""
    return sub(body, lit=lit)


def tmpl_generics(rng):
    gx, gy = rng.randint(-100, 100), rng.randint(-100, 100)
    gs = rng.choice(WORDS)
    body = """\
    let bi: Box_<i64> = Box_ { val: @@gx@@ }
    let bs: Box_<str> = Box_ { val: "@@gs@@" }
    let sum_i: i64 = bi.get() + @@gy@@
    let sum_s: i64 = len(bs.get())
    return sum_i + sum_s"""
    return sub(body, gx=gx, gy=gy, gs=gs)


def tmpl_control_flow(rng):
    n1 = rng.randint(5, 20)
    n2 = rng.randint(3, 12)
    k = rng.randint(-5, 5)
    r1 = rng.randint(0, 5)
    r2 = r1 + rng.randint(1, 10)
    body = """\
    let mut total: i64 = 0
    let mut i: i64 = 0
    while i < @@n1@@ {
        if i % 2 == 0 {
            total = total + i
        } elif i % 3 == 0 {
            total = total - i
        } else {
            total = total + 1
        }
        i = i + 1
    }
    let mut j: i64 = 0
    loop {
        if j >= @@n2@@ { break }
        total = total + j * @@k@@
        j = j + 1
    }
    for r in range(@@r1@@, @@r2@@) {
        total = total + r
    }
    return total"""
    return sub(body, n1=n1, n2=n2, k=k, r1=r1, r2=r2)


def tmpl_try_throw(rng):
    n = rng.randint(6, 12)
    body = """\
    let mut total: i64 = 0
    let mut i: i64 = 0
    while i < @@n@@ {
        try {
            total = total + risky(i)
        } catch e {
            total = total + len(e)
        }
        i = i + 1
    }
    return total"""
    return sub(body, n=n)


TEMPLATES = [
    ("int_arith", tmpl_int_arith),
    ("float_arith", tmpl_float_arith),
    ("string_ops", tmpl_string_ops),
    ("array_ops", tmpl_array_ops),
    ("map_ops", tmpl_map_ops),
    ("struct_scalar", tmpl_struct_scalar),
    ("struct_heap", tmpl_struct_heap),
    ("enum_match", tmpl_enum_match),
    ("closure_direct", tmpl_closure_direct),
    ("closure_hof", tmpl_closure_hof),
    ("generics", tmpl_generics),
    ("control_flow", tmpl_control_flow),
    ("try_throw", tmpl_try_throw),
]


def generate(seed, blocks):
    rng = random.Random(seed)
    chosen = [rng.choice(TEMPLATES) for _ in range(blocks)]

    lines = [
        "// AUTO-GENERATED by tests/fuzz/gen_fuzz.py -- differential JIT/AOT fuzz case",
        f"// Replay: python tests/fuzz/gen_fuzz.py --seed {seed} --blocks {blocks}",
        "",
        PRELUDE,
    ]

    for idx, (name, fn) in enumerate(chosen):
        body = fn(rng)
        lines.append(f"fn block_{idx}() -> i64 {{\n{body}\n}}\n")

    main_lines = ["fn main() {", "    let mut acc: i64 = 0"]
    for idx, (name, _fn) in enumerate(chosen):
        main_lines.append(f"    let v{idx} = block_{idx}()")
        main_lines.append(f'    println("B{idx} {name}: " + to_string(v{idx}))')
        main_lines.append(f"    acc = acc ^ v{idx}")
    main_lines.append('    println("CHECKSUM: " + to_string(acc))')
    main_lines.append("}")

    lines.append("\n".join(main_lines))
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--blocks", type=int, default=None, help="default: derived from seed, 12-30")
    ap.add_argument("-o", "--out", default=None)
    args = ap.parse_args()

    blocks = args.blocks
    if blocks is None:
        blocks = 12 + (args.seed % 19)

    src = generate(args.seed, blocks)
    if args.out:
        with open(args.out, "w", encoding="utf-8", newline="\n") as f:
            f.write(src)
    else:
        sys.stdout.write(src)


if __name__ == "__main__":
    main()
