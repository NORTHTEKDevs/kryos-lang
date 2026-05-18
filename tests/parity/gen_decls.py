#!/usr/bin/env python3
"""
Generate LLVM IR `declare` lines from runtime Rust `extern "C" fn` signatures.

Reads `pub extern "C" fn kryos_*(...) -> ...` lines from stdin and emits a
matching `declare <ret> @<name>(<args>)` line per function.

Rust type → LLVM type:
    i64, u64, usize    → i64
    i32, u32           → i32
    i16, u16           → i16
    i8, u8, c_char     → i8
    f64                → double
    f32                → float
    bool               → i1
    () / no return     → void
    *const T, *mut T   → ptr
    everything else    → i64  (best-effort default)

Only emits a declaration when the symbol starts with `kryos_`. Sorts and
deduplicates by symbol name so the output is stable.
"""

import re
import sys

RUST_TO_LLVM = {
    "i64": "i64", "u64": "i64", "usize": "i64", "isize": "i64",
    "i32": "i32", "u32": "i32",
    "i16": "i16", "u16": "i16",
    "i8":  "i8",  "u8":  "i8",  "c_char": "i8",
    "f64": "double",
    "f32": "float",
    "bool": "i1",
}

def map_ty(rust_ty: str) -> str:
    rust_ty = rust_ty.strip()
    if rust_ty == "" or rust_ty == "()":
        return "void"
    if rust_ty.startswith("*const") or rust_ty.startswith("*mut"):
        return "ptr"
    # Strip type params: KryosString<...> → KryosString
    base = re.sub(r"<.*", "", rust_ty).strip()
    return RUST_TO_LLVM.get(base, "i64")  # default to i64

# Pattern: pub (unsafe)? extern "C" fn kryos_NAME(arg1: T1, arg2: T2, ...) -> RET
# Greedy match for the return type so we capture the full type before `{`.
PATTERN = re.compile(
    r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(kryos_[a-zA-Z0-9_]+)\s*\(([^)]*)\)\s*(?:->\s*([^{]+))?'
)

ARG_PATTERN = re.compile(r'(?:[a-zA-Z_][a-zA-Z0-9_]*\s*:\s*)([^,]+)')

decls = {}
for line in sys.stdin:
    line = line.strip()
    m = PATTERN.match(line)
    if not m:
        continue
    name = m.group(1)
    args_str = m.group(2) or ""
    ret_str = m.group(3) or "()"
    # Parse args; each is "name: type" — extract the types.
    arg_types = []
    if args_str.strip():
        for arg_match in ARG_PATTERN.finditer(args_str):
            arg_types.append(map_ty(arg_match.group(1)))
    ret_ty = map_ty(ret_str)
    decls[name] = (ret_ty, arg_types)

for name in sorted(decls.keys()):
    ret_ty, arg_types = decls[name]
    args_repr = ", ".join(arg_types)
    print(f'declare {ret_ty} @{name}({args_repr})')
