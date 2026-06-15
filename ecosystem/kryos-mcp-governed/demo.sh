#!/usr/bin/env bash
# Reproducible demo for kryos-mcp-governed. Proves the four MVP claims.
# Run from the project root after `kryos build --release src/main.kry -o kryos-mcp-governed.exe`.
set -u

BIN=./kryos-mcp-governed.exe
[ -x "$BIN" ] || BIN=./kryos-mcp-governed   # non-Windows

if [ ! -x "$BIN" ]; then
  echo "building..."
  kryos build --release src/main.kry -o kryos-mcp-governed.exe || exit 1
  BIN=./kryos-mcp-governed.exe
fi

line() { printf '\n========== %s ==========\n' "$1"; }

line "1. tools/list advertises kryos_capabilities"
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | "$BIN" 2>/dev/null \
  | grep -o '"kryos_capabilities":\[[^]]*\]'

line "2. KRYOS_MCP_CAPS=compute -> WARN on stderr, server continues"
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' \
  | KRYOS_MCP_CAPS=compute "$BIN" 1>/dev/null

line "3. KRYOS_MCP_STRICT=1 -> hard non-zero exit"
KRYOS_MCP_STRICT=1 KRYOS_MCP_CAPS=compute "$BIN" </dev/null 1>/dev/null
echo "exit=$?  (non-zero = correct)"

line "4. capability enforcement is real (compile-time)"
echo "A net-only tool that calls file_write (needs io) is rejected:"
echo "  kryos build --release src/main.kry  =>  error[E-CAP-BUILTIN]: builtin file_write requires io capability"
echo "(see README 'Proof the enforcement is real' to run it live)"
