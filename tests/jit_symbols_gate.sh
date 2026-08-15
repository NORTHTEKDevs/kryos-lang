#!/usr/bin/env bash
# jit_symbols_gate.sh -- every runtime symbol must be registered with the JIT.
#
# WHY: `kryos run` AOT-compiles and links the runtime staticlibs, so it never
# sees this problem. `kryos test` and `kryos repl` use the IN-PROCESS Cranelift
# JIT, and there an unregistered runtime symbol is not a diagnostic --
# cranelift-jit panics the whole process ("can't resolve symbol X", rc=101).
#
# Measured 2026-08-14: `kryos test` on a file whose `@test` body built a basic
# struct literal panicked on `kryos_calloc`. Auditing every `pub extern "C" fn`
# in kryos-rt + kryos-stdlib-native against jit.rs found **141** unregistered
# symbols, not one -- actors, channels, async, base64, checked arithmetic and
# more. So `kryos test`/`kryos repl` were a minefield, and the only reason it
# looked fine is that nothing exercised them beyond trivial programs.
#
# This compares the two lists directly. It is a SOURCE-level check on purpose:
# catching this at runtime requires a program that happens to touch the missing
# symbol, which is exactly the sampling problem that let 141 accumulate.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
command -v python3 >/dev/null 2>&1 || { echo "jit-symbols: SKIP (no python3)"; exit 0; }

python3 - <<'PYEOF'
import re, glob, sys

jit = open('compiler/crates/kryos-codegen-cranelift/src/jit.rs', encoding='utf-8').read()
registered = set(re.findall(r'jit_builder\.symbol\(\s*"([a-z0-9_]+)"', jit))

exported = set()
for pat in ('compiler/crates/kryos-rt/src/**/*.rs',
            'compiler/crates/kryos-stdlib-native/src/**/*.rs'):
    for f in glob.glob(pat, recursive=True):
        s = open(f, encoding='utf-8', errors='replace').read()
        exported |= set(re.findall(r'pub extern "C" fn ([a-z0-9_]+)', s))

missing = sorted(exported - registered)
print("  runtime symbols exported: %d" % len(exported))
print("  registered with the JIT : %d" % len(exported & registered))
if not missing:
    print("jit-symbols: PASS -- every runtime symbol is reachable from `kryos test`/`repl`")
    sys.exit(0)
print("  MISSING: %d" % len(missing))
for m in missing[:25]:
    print("    " + m)
if len(missing) > 25:
    print("    ... and %d more" % (len(missing) - 25))
print("jit-symbols: FAIL -- these would panic `kryos test`/`kryos repl` (rc=101), not diagnose")
sys.exit(1)
PYEOF
