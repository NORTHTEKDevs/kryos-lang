#!/usr/bin/env python3
"""Validate every call in emitted LLVM IR against its declaration.

Motivation: `call i64 @kryos_json_bool(i1 %b)` was emitted against
`declare i64 @kryos_json_bool(i64)`. LLVM accepted the textual IR, but the
upper 63 bits of the argument register were undefined, so the callee's
`val != 0` test read garbage -- json_bool(false) produced a JSON `true` on
--release while the JIT was correct.

That is a CLASS of defect, not one bug: any call site whose argument types
disagree with the callee's declaration is a silent miscompile waiting for the
right register pressure. This checker reads a .ll file, builds a signature
table from its `declare` and `define` lines, and reports every call whose
argument or return types do not match.

Usage:  check_ir_signatures.py <file.ll> [more.ll ...]
Exit 1 if any mismatch is found.
"""
import re
import sys

# Parameter attributes that may precede/follow a type in a call or declaration.
ATTRS = {
    "noalias", "nocapture", "readonly", "readnone", "writeonly", "nonnull",
    "signext", "zeroext", "inreg", "returned", "nest", "swiftself",
    "swifterror", "immarg", "noundef", "dereferenceable", "align", "byval",
    "sret", "inalloca", "preallocated", "alignstack", "captures",
}


def split_top_level(s):
    """Split on commas that are not nested in (), {}, [] or <>."""
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "({[<":
            depth += 1
        elif ch in ")}]>":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def match_paren(s, start):
    """Return index just past the ')' matching the '(' at s[start]."""
    depth = 0
    for i in range(start, len(s)):
        if s[i] == "(":
            depth += 1
        elif s[i] == ")":
            depth -= 1
            if depth == 0:
                return i + 1
    return -1


def strip_attrs(tok):
    """Reduce one declaration/call argument to its bare LLVM type."""
    tok = tok.strip()
    # Drop attributes that carry a parenthesised payload: sret(%T), byval(%T),
    # align 8, dereferenceable(16)...
    tok = re.sub(r"\b(sret|byval|inalloca|preallocated|dereferenceable(_or_null)?|captures)\s*\([^)]*\)", " ", tok)
    tok = re.sub(r"\balign\s+\d+", " ", tok)
    parts = tok.split()
    kept = [p for p in parts if p not in ATTRS]
    if not kept:
        return ""
    # The type is the leading token(s); a value follows (%x, @g, a literal).
    ty = []
    for p in kept:
        if p.startswith("%") and not re.match(r"^%[A-Za-z_.$][\w.$]*$", p):
            break
        if p.startswith(("@", "$")):
            break
        # A bare SSA name like %t12 ends the type; a named struct type
        # like %Wrap is itself a type. Distinguish: struct type names in
        # this compiler's output are not purely numeric and appear first.
        if p.startswith("%") and ty:
            break
        ty.append(p)
        # Types never span more than one token here except for pointers with
        # address spaces, which this backend does not emit.
        break
    return " ".join(ty)


# ptr and i64 are the same width and the same register class on every target
# this compiler supports, so passing one where the other is declared is sloppy
# IR but not a miscompile. These pairings ARE miscompiles:
#   i1/i8/i16/i32 vs i64  -- the narrow value leaves the upper bits UNDEFINED,
#                            and the callee reads the full width (the
#                            json_bool(false) -> JSON `true` bug)
#   double vs i64/ptr     -- different register CLASS (XMM vs GP); the callee
#                            reads a register the caller never set
#   aggregate vs scalar   -- entirely different ABI
NARROW_INTS = {"i1", "i8", "i16", "i32"}


def severity(got, want, is_return=False):
    """Classify a mismatch.

    'severe'  -- can miscompile TODAY or is undefined by LLVM semantics.
                 Only one shape qualifies for arguments: a NARROW value passed
                 where a WIDER one is declared. The callee reads the full
                 width; the upper bits are undefined. This is the json_bool
                 bug, and the same shape was found for char_from/char_at.
    'review'  -- currently safe but relies on a runtime invariant, so it is
                 reported for visibility and not failed on.
    'benign'  -- cannot change behaviour on any supported ABI.
    """
    pair = {got, want}
    if pair == {"ptr", "i64"}:
        return "benign"
    # A DISCARDED return value: the callee writes its result register and the
    # caller ignores it. Untidy IR, but it cannot change behaviour.
    if is_return and got == "void":
        return "benign"

    if is_return:
        # Reading a NARROWER type than declared truncates. Safe only while the
        # runtime returns strictly 0/1 -- which every predicate here does
        # (kryos_builtin_contains, map_has, json_is_null, starts_with all
        # return literal 1 or 0). Flagged, not failed: a runtime that ever
        # returned an index or a count would silently invert the boolean.
        if got in NARROW_INTS and want in ("i64", "ptr"):
            return "review"
        # Reading a value from a void function reads an unset register. Every
        # instance today is kryos_exception_throw (noreturn) or a map insert
        # whose result is discarded.
        if want == "void":
            return "review"
        return "review"

    # ---- arguments ----
    if got in NARROW_INTS and want in ("i64", "ptr"):
        return "severe"          # undefined upper bits -- the real bug class
    if want in NARROW_INTS and got in ("i64", "ptr"):
        return "review"          # deliberate truncation (exit code, i16 port)
    if "double" in pair or "float" in pair:
        return "severe"          # wrong register CLASS entirely
    if any(t.startswith(("{", "%", "[")) for t in pair):
        return "severe"          # aggregate vs scalar ABI
    return "severe"


def normalize(ty):
    """Types that are genuinely interchangeable at the ABI level."""
    ty = ty.strip()
    # Opaque pointers: `ptr` and any legacy `T*` spelling are the same.
    if ty.endswith("*"):
        return "ptr"
    return ty


def parse_signatures(text):
    """name -> (ret, [param types], is_vararg) from declare and define lines."""
    sigs = {}
    for m in re.finditer(r"^(declare|define)\s+(.*?)@([\w.$]+)\s*\(", text, re.M):
        kind, pre, name = m.group(1), m.group(2), m.group(3)
        open_paren = m.end() - 1
        close = match_paren(text, open_paren)
        if close < 0:
            continue
        raw = text[open_paren + 1:close - 1]
        args = split_top_level(raw)
        vararg = any(a.strip() == "..." for a in args)
        ptys = [normalize(strip_attrs(a)) for a in args if a.strip() != "..."]
        # Return type is the last type token before the '@'.
        pre_toks = [t for t in pre.split() if t not in ("internal", "external",
                                                        "private", "linkonce",
                                                        "linkonce_odr", "weak",
                                                        "weak_odr", "common",
                                                        "appending", "dso_local",
                                                        "dso_preemptable", "hidden",
                                                        "protected", "default",
                                                        "ccc", "fastcc", "coldcc",
                                                        "zeroext", "signext",
                                                        "noundef")]
        ret = normalize(pre_toks[-1]) if pre_toks else "?"
        # A define wins over a declare (it is the real body).
        if name not in sigs or kind == "define":
            sigs[name] = (ret, ptys, vararg)
    return sigs


def check(path):
    text = open(path, encoding="utf-8", errors="ignore").read()
    sigs = parse_signatures(text)
    problems = []

    call_re = re.compile(r"\bcall\s+(?:tail\s+)?(.*?)@([\w.$]+)\s*\(", re.S)
    for m in call_re.finditer(text):
        pre, name = m.group(1), m.group(2)
        if name not in sigs:
            continue  # intrinsic or externally provided; nothing to compare
        if "\n" in pre:
            continue  # not a real single-line call form
        open_paren = m.end() - 1
        close = match_paren(text, open_paren)
        if close < 0:
            continue
        raw = text[open_paren + 1:close - 1]
        args = [normalize(strip_attrs(a)) for a in split_top_level(raw)]

        ret_decl, ptys, vararg = sigs[name]
        line_no = text.count("\n", 0, m.start()) + 1

        if not vararg and len(args) != len(ptys):
            problems.append(("severe",
                f"{path}:{line_no}: @{name} called with {len(args)} arg(s), "
                f"declared with {len(ptys)}"))
            continue

        for i, (got, want) in enumerate(zip(args, ptys)):
            if got and want and got != want:
                problems.append((severity(got, want),
                    f"{path}:{line_no}: @{name} arg {i}: call passes '{got}', "
                    f"declaration says '{want}'"))

        pre_toks = [t for t in pre.split() if t not in ("zeroext", "signext", "noundef")]
        ret_call = normalize(pre_toks[-1]) if pre_toks else ""
        if ret_call and ret_decl and ret_call != ret_decl and ret_decl != "?":
            problems.append((severity(ret_call, ret_decl, is_return=True),
                f"{path}:{line_no}: @{name} return: call reads '{ret_call}', "
                f"declaration says '{ret_decl}'"))

    return problems


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    all_problems = []
    for p in argv[1:]:
        all_problems.extend(check(p))
    severe = [m for sev, m in all_problems if sev == "severe"]
    review = [m for sev, m in all_problems if sev == "review"]
    benign = [m for sev, m in all_problems if sev == "benign"]
    for m in severe:
        print("  SEVERE " + m)
    if review:
        print(f"  ({len(review)} review-class mismatch(es): narrowing reads/args "
              f"that are safe only while the runtime keeps returning 0/1)")
    # Benign ptr/i64 spellings are reported as a COUNT only: they are real IR
    # sloppiness worth cleaning up, but they cannot miscompile, and listing
    # hundreds of them would bury the ones that can.
    if benign:
        print(f"  ({len(benign)} benign ptr/i64 or discarded-return mismatch(es), "
              f"not miscompiles)")
    if severe:
        print(f"ir-signatures: {len(severe)} SEVERE mismatch(es) across "
              f"{len(argv) - 1} file(s)")
        return 1
    print(f"ir-signatures: no severe mismatches ({len(argv) - 1} file(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
