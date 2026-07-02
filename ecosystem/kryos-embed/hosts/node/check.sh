#!/usr/bin/env bash
# ecosystem/kryos-embed/hosts/node/check.sh
# Acceptance test for the Node/WASM Kryos agent binding.
# Run from the repository root or from this directory.
# Exit 0 = ALL PASS.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
cd "$ROOT"

export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="$ROOT/compiler/target/release/kryos.exe"
SRC="$SCRIPT_DIR/agent_wasm.kry"
WASM_OUT="$SCRIPT_DIR/dist/kryos_embed_agent.wasm"
DEMO="$SCRIPT_DIR/demo_crm.mjs"
BINDING="$SCRIPT_DIR/kryos-agent.mjs"

fail=0
pass() { echo "  PASS  $1"; }
fail_gate() { echo "  FAIL  $1: $2"; fail=1; }

# ---- Step 1: Compile agent_wasm.kry -> WASM ----
echo ""
echo "=== Step 1: kryos build --release --backend wasm ==="
mkdir -p "$SCRIPT_DIR/dist"
if "$KRYOS" build --release --backend wasm "$SRC" -o "$WASM_OUT" 2>&1; then
    pass "wasm-compile"
else
    fail_gate "wasm-compile" "kryos build --backend wasm failed"
    exit 1
fi

# ---- Step 2: Verify WASM exports agent_query ----
echo ""
echo "=== Step 2: verify WASM exports agent_query ==="
exports_out=$(WASM_OUT="$WASM_OUT" node --input-type=module <<'EOJS'
import { readFileSync } from 'node:fs';
const wasmPath = process.env.WASM_OUT;
const bytes = readFileSync(wasmPath);
const mod = await WebAssembly.compile(bytes);
const exps = WebAssembly.Module.exports(mod).map(e => e.name);
console.log('EXPORTS: ' + exps.join(', '));
EOJS
)

if echo "$exports_out" | grep -q "agent_query"; then
    pass "wasm-exports-agent_query"
    echo "  $exports_out"
else
    fail_gate "wasm-exports-agent_query" "agent_query not in WASM exports: $exports_out"
fi

# ---- Step 3: Print WASM import manifest (the capability proof) ----
echo ""
echo "=== Step 3: WASM import manifest ==="
WASM_OUT="$WASM_OUT" node --input-type=module <<'EOJS'
import { readFileSync } from 'node:fs';
const wasmPath = process.env.WASM_OUT;
const bytes = readFileSync(wasmPath);
const mod = await WebAssembly.compile(bytes);
const imports = WebAssembly.Module.imports(mod);
console.log('Import manifest (' + imports.length + ' host functions):');
for (const i of imports) {
  console.log('  ' + i.module + '::' + i.name);
}
EOJS

# ---- Step 4: Run demo_crm.mjs and assert governance properties ----
echo ""
echo "=== Step 4: demo_crm.mjs assertions ==="
demo_out=$(node "$DEMO" 2>&1) || true

# (a) Within-budget call: answered=true, source present, spend=3
if echo "$demo_out" | grep -q "answered   : true" && \
   echo "$demo_out" | grep -q "source     : \"mock-llm-v1\"" && \
   echo "$demo_out" | grep -q "PASS: answered=true, source present, spendCents=3"; then
    pass "demo-within-budget-answered"
else
    fail_gate "demo-within-budget-answered" "missing within-budget assertions in output: $demo_out"
fi

# (b) Over-budget call: answered=false, spendCents=0
if echo "$demo_out" | grep -q "answered   : false" && \
   echo "$demo_out" | grep -q "spendCents : 0" && \
   echo "$demo_out" | grep -q "PASS: answered=false (refusal), spendCents=0"; then
    pass "demo-over-budget-refused-no-spend"
else
    fail_gate "demo-over-budget-refused-no-spend" "missing over-budget assertions in output: $demo_out"
fi

# (c) All assertions passed marker
if echo "$demo_out" | grep -q "All demo assertions PASSED"; then
    pass "demo-all-passed"
else
    fail_gate "demo-all-passed" "missing 'All demo assertions PASSED' in output: $demo_out"
fi

echo ""
echo "$demo_out"

# ---- Summary ----
echo ""
echo "========================================"
echo "check.sh results: pass=$((3 - fail)) fail=$fail / 3 checks"
echo "========================================"

if [ "$fail" -gt 0 ]; then
    echo "FAIL"
    exit 1
fi

echo "ALL ASSERTIONS PASSED"
exit 0
