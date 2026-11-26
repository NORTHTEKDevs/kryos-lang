#!/usr/bin/env bash
# DAP server JIT ownership gate (finish-line item A22).
# DapServer used to compile the debuggee with a throwaway JitCompiler: the
# compiler (which owns the executable pages and the string-literal data)
# dropped when launch() returned, so on Linux the freed pages were reused and
# the debuggee printed garbage literals (use-after-free; the REPL had the same
# bug, fixed in fda46cab). The server must own the JitCompiler for the life of
# the session. The UAF does not reproduce on Windows, so the discriminating
# check is on the source: `struct DapServer` must hold a JitCompiler field.
# A runtime smoke (12/12 literals through a real DAP session) guards the fix.
# Usage: dap_jit_owner_gate.sh [source-root]   (default: this repo; pass another
# checkout root to see the source check fail on an unfixed tree).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
SRC_ROOT="${1:-$ROOT}"
K="$ROOT/compiler/target/release/kryos.exe"; [ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
DAP="$SRC_ROOT/compiler/crates/kryos-cli/src/commands/dap.rs"
fail=0

if [ -f "$DAP" ] && tr -d '\r' < "$DAP" | tr '\n' ' ' | grep -Eq 'struct DapServer[[:space:]]*\{[^}]*JitCompiler'; then
    echo "  ok   struct DapServer owns a JitCompiler"
else
    echo "  FAIL: struct DapServer does not hold a JitCompiler ($DAP)"; fail=1
fi

out="$(KRYOS_BIN="$K" timeout 60 node tests/dap_jit_owner_drive.mjs tests/dap_jit_owner_prog.kry 2>&1)"; rc=$?
if [ "$rc" -eq 0 ] && printf '%s' "$out" | grep -q '"intactLiterals":12,'; then
    echo "  ok   dap session prints 12/12 intact literals"
else
    echo "  FAIL: dap session (exit $rc): $(printf '%s' "$out" | head -c 300)"; fail=1
fi

if [ "$fail" -eq 0 ]; then echo "dap_jit_owner_gate: PASS"; else echo "dap_jit_owner_gate: FAIL"; exit 1; fi
