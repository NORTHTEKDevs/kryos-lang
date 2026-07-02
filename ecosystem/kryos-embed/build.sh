#!/usr/bin/env bash
# ecosystem/kryos-embed/build.sh
#
# Reproduces the kryos_embed_agent.dll build from source.
# Run from the REPO ROOT:
#   bash ecosystem/kryos-embed/build.sh
#
# Steps:
#   1. strict-capabilities check on agent_embed.kry (must PASS)
#   2. strict-capabilities check on neg_control.kry  (must FAIL with E0505)
#   3. emit LLVM IR from agent_embed.kry
#   4. sed-patch IR: promote agent_call + agent_response_len from `internal` to `dllexport`
#   5. zig cc link -> dist/kryos_embed_agent.dll
#   6. Write dist/agent.caps.json (verified authority manifest)
#
# The `verified: true` claim in the manifest is honest because step 1 above
# would abort the build if agent_embed.kry under-declared its capabilities.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="$ROOT/compiler/target/release/kryos.exe"
KRYOS_RT="$ROOT/compiler/target/release/kryos_rt.lib"
KRYOS_STD="$ROOT/compiler/target/release/kryos_stdlib_native.lib"
SRC="$ROOT/ecosystem/kryos-embed/agent/agent_embed.kry"
NEG="$ROOT/ecosystem/kryos-embed/agent/neg_control.kry"
DIST="$ROOT/ecosystem/kryos-embed/dist"
TMP="${TEMP:-/tmp}"
LL="$TMP/kryos_embed_agent_$$.ll"
LL_EXP="$TMP/kryos_embed_agent_exp_$$.ll"
OUT_DLL="$DIST/kryos_embed_agent.dll"
OUT_CAPS="$DIST/agent.caps.json"

mkdir -p "$DIST"

echo "[1/6] strict-capabilities check: agent_embed.kry (must PASS)"
if "$KRYOS" check --strict-capabilities "$SRC" >/dev/null 2>&1; then
    echo "  PASS  caps-check-positive"
else
    echo "  FAIL  caps-check-positive: agent_embed.kry failed strict-capabilities"
    exit 1
fi

echo "[2/6] strict-capabilities check: neg_control.kry (must FAIL with E0505)"
NEG_OUT=$("$KRYOS" check --strict-capabilities "$NEG" 2>&1 || true)
if echo "$NEG_OUT" | grep -q "E0505"; then
    echo "  PASS  caps-check-negative (E0505 present)"
else
    echo "  FAIL  caps-check-negative: expected E0505 in neg_control.kry output"
    echo "$NEG_OUT"
    exit 1
fi

echo "[3/6] emit LLVM IR from agent_embed.kry"
if "$KRYOS" build --release --emit-llvm "$SRC" -o "$LL" >/dev/null 2>&1; then
    echo "  PASS  emit-llvm"
else
    echo "  FAIL  emit-llvm: kryos build --release --emit-llvm failed"
    exit 1
fi

echo "[4/6] patch IR: dllexport agent_call + agent_response_len"
# Kryos emits every function as `define internal`. We promote BOTH exports.
# agent_call(i64 %_0, i64 %_1)  and  agent_response_len()  are the only exports.
sed 's/define internal i64 @agent_call/define dllexport i64 @agent_call/;
     s/define internal i64 @agent_response_len/define dllexport i64 @agent_response_len/' \
    "$LL" > "$LL_EXP"

PATCHED=$(grep -c "define dllexport" "$LL_EXP" || true)
if [ "$PATCHED" -lt 2 ]; then
    echo "  FAIL  patch-ir: expected 2 dllexport symbols, found $PATCHED"
    cat "$LL_EXP" | grep "define dllexport" || true
    exit 1
fi
echo "  PASS  patch-ir ($PATCHED dllexport symbols)"

echo "[5/6] link DLL: $OUT_DLL"
if zig cc -target x86_64-windows-msvc -shared \
       -o "$OUT_DLL" "$LL_EXP" \
       "$KRYOS_RT" "$KRYOS_STD" \
       -lntdll -luserenv -lws2_32 -ldbghelp -luser32 -lbcrypt -ladvapi32 \
       -Wno-override-module >/dev/null 2>&1; then
    echo "  PASS  link-dll ($(du -sh "$OUT_DLL" | cut -f1))"
else
    echo "  FAIL  link-dll: zig cc failed"
    exit 1
fi

echo "[6/6] write authority manifest: $OUT_CAPS"
# This manifest is HONEST: verified:true is set only because step [1] above
# confirms that the compiler accepted agent_embed.kry under --strict-capabilities.
# A build that fails step [1] never reaches this step.
cat > "$OUT_CAPS" <<'CAPS_EOF'
{
  "library": "kryos_embed_agent",
  "abi": "c",
  "schema": "kryos-embed-caps-v1",
  "exports": {
    "agent_call": {
      "capabilities": ["ffi"],
      "signature": "agent_call(req_ptr: i64, req_len: i64) -> i64",
      "notes": "reads host memory via from_ptr; returns NUL-terminated JSON ptr via cstr; mock LLM is pure compute",
      "verified": true
    },
    "agent_response_len": {
      "capabilities": ["ffi"],
      "signature": "agent_response_len() -> i64",
      "notes": "returns byte length of last response string; reads DLL-owned global",
      "verified": true
    }
  },
  "governance": {
    "budget_gate": "pre-llm",
    "llm_backend": "mock-llm-v1",
    "cost_per_call_cents": 3,
    "provenance_field": "source"
  }
}
CAPS_EOF
echo "  PASS  manifest written"

# Cleanup temp IR files
rm -f "$LL" "$LL_EXP"

echo ""
echo "kryos-embed build: ALL STEPS PASSED"
echo "  DLL:      $OUT_DLL"
echo "  Manifest: $OUT_CAPS"
