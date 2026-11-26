#!/usr/bin/env bash
# W0001 reaches the LSP and the REPL (finish-line item A21).
# The string-based compile paths (kryos-lsp check_source, driver
# compile_source / check_source) parsed with `parse`, which discards parser
# WARNINGS on success, so the ambiguous newline-led `-1` / `[0]()` continuation
# warning W0001 that `kryos check` prints never reached publishDiagnostics or
# the REPL. Override the binary with KRYOS_BIN to run against another build.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
export KRYOS_BIN="$K"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

printf 'fn main() {\n    let a = 5\n    -1\n    println(a)\n}\n' > "$TMP/trap.kry"
printf 'fn main() {\n    let a = 5\n    let b = a\n    println(b)\n}\n' > "$TMP/clean.kry"

out="$(timeout 60 node tests/lsp_w0001_drive.mjs "$TMP/trap.kry" 2>&1)"
if printf '%s' "$out" | grep -q '"code":"W0001"'; then echo "  ok   LSP publishes W0001 for newline-led -1"
else echo "  FAIL: LSP did not publish W0001:"; printf '%s\n' "$out" | head -3 | sed 's/^/    /'; fail=1; fi

out="$(timeout 60 node tests/lsp_w0001_drive.mjs "$TMP/clean.kry" 2>&1)"
if printf '%s' "$out" | grep -q 'W0001'; then echo "  FAIL: LSP published W0001 for a clean file"; fail=1
else echo "  ok   LSP control: no W0001 for a clean file"; fi

out="$(printf 'fn hello() { println("HELLO-RAN") }\nlet m = [hello]\n[0]()\n:quit\n' | timeout 60 "$K" repl 2>&1)"
if printf '%s' "$out" | grep -q 'W0001'; then echo "  ok   REPL prints W0001 for newline-led [0]()"
else echo "  FAIL: REPL did not print W0001:"; printf '%s\n' "$out" | head -4 | sed 's/^/    /'; fail=1; fi

out="$(printf 'let a = 5\nprintln(a + 1)\n:quit\n' | timeout 60 "$K" repl 2>&1)"
if printf '%s' "$out" | grep -q 'W0001'; then echo "  FAIL: REPL printed W0001 for clean input"; fail=1
elif printf '%s' "$out" | grep -q '^6'; then echo "  ok   REPL control: clean input runs, no W0001"
else echo "  FAIL: REPL control did not print 6:"; printf '%s\n' "$out" | head -4 | sed 's/^/    /'; fail=1; fi

[ "$fail" -eq 0 ] && echo "lsp-repl-w0001-gate: all green"
exit "$fail"
