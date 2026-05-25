#!/usr/bin/env python3
"""Make the self-host source explicit-typed at call-init / call-field sites so the
weak self-host type inferencer (which types let-bound locals ANY unless the RHS is
a struct literal) does not miscompile field accesses. stage-0 (Rust) infers
correctly; this brings stage-1's output (stage-2) up to parity by removing the need
to infer. Proven-safe path: same as the lexer's `let mut lex: Lexer = lexer_new(src)`.

Two transforms:
  B) p_peek(X).{start,end,text}  ->  p_peek_{start,end,text}(X)   [parser.kry only]
     plus 3 accessor helpers using an annotated local. (.kind already works: index 0.)
  A) let [mut] VAR = STRUCT_FN(...)   ->   let [mut] VAR: RetType = STRUCT_FN(...)
     only when the call is the COMPLETE rhs (closing paren ends the statement).
"""
import re
import sys
from pathlib import Path

MODULES = ("token lexer ast parser types mir lower optimize regalloc x86 "
           "codegen elf coff linker runtime main").split()
PRIMITIVES = {"i64", "i32", "f64", "bool", "str", "void", "usize", "u8", "u64"}
HERE = Path(__file__).parent
apply = "--apply" in sys.argv

# ---- struct-returning function map -------------------------------------------
SIG_RE = re.compile(r"\bfn\s+(\w+)\s*\([^;{]*?\)\s*->\s*([A-Z]\w*)\b")
struct_ret = {}
for mod in MODULES:
    for m in SIG_RE.finditer((HERE / f"{mod}.kry").read_text(encoding="utf-8", errors="replace")):
        if m.group(2) not in PRIMITIVES:
            struct_ret[m.group(1)] = m.group(2)
print(f"struct-returning fns: {len(struct_ret)}")


def matching_paren(s, open_idx):
    """Index of the ')' matching the '(' at open_idx, or -1."""
    depth = 0
    for i in range(open_idx, len(s)):
        if s[i] == '(':
            depth += 1
        elif s[i] == ')':
            depth -= 1
            if depth == 0:
                return i
    return -1


# ---- Transform B: p_peek field accessors (parser.kry) ------------------------
HELPERS = """
fn p_peek_start(p: Parser) -> i32 {
    let t: Token = p_peek(p)
    return t.start
}

fn p_peek_end(p: Parser) -> i32 {
    let t: Token = p_peek(p)
    return t.end
}

fn p_peek_text(p: Parser) -> str {
    let t: Token = p_peek(p)
    return t.text
}
"""
ptxt = (HERE / "parser.kry").read_text(encoding="utf-8", errors="replace")
b_count = 0
for field in ("start", "end", "text"):
    ptxt, n = re.subn(rf"\bp_peek\(([^()]*)\)\.{field}\b", rf"p_peek_{field}(\1)", ptxt)
    b_count += n
# insert helpers right after the existing p_peek_kind function
if "fn p_peek_start(" not in ptxt:
    anchor = "fn p_peek_kind(p: Parser) -> i32 {\n    return p_peek(p).kind\n}\n"
    if anchor in ptxt:
        ptxt = ptxt.replace(anchor, anchor + HELPERS, 1)
    else:
        raise SystemExit("p_peek_kind anchor not found; aborting")
print(f"transform B (p_peek.field -> accessor): {b_count} sites + 3 helpers")
if apply:
    (HERE / "parser.kry").write_text(ptxt, encoding="utf-8")

# ---- Transform A: annotate let-init when call is the complete rhs ------------
LET_RE = re.compile(r"^(\s*)let\s+(mut\s+)?(\w+)\s*=\s*(\w+)\(")
total = 0
per_callee = {}
for mod in MODULES:
    p = HERE / f"{mod}.kry"
    src = ptxt if (mod == "parser" and apply) else p.read_text(encoding="utf-8", errors="replace")
    lines = src.split("\n")
    out, changed = [], 0
    for ln in lines:
        m = LET_RE.match(ln)
        if m and m.group(4) in struct_ret:
            open_idx = m.end(4)            # index of '('
            close = matching_paren(ln, open_idx)
            tail = ln[close + 1:].strip() if close != -1 else "X"
            if close != -1 and (tail == "" or tail.startswith("//")):
                indent, mut, var, callee = m.groups()
                ret = struct_ret[callee]
                out.append(f"{indent}let {mut or ''}{var}: {ret} = {callee}{ln[open_idx:]}")
                changed += 1
                per_callee[callee] = per_callee.get(callee, 0) + 1
                continue
        out.append(ln)
    total += changed
    if changed and apply:
        p.write_text("\n".join(out), encoding="utf-8")
    print(f"  {mod}.kry: {changed}")

print(f"transform A (let annotations): {total}  ({'APPLIED' if apply else 'dry-run'})")
print("top callees:", sorted(per_callee.items(), key=lambda x: -x[1])[:12])
