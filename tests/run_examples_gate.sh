#!/usr/bin/env bash
# Examples gate — the release-battery sweep over example programs.
#
# Layers:
#   1. `kryos check` every root examples/*.kry (skipping wasm_* which target
#      the wasm backend) — the user-facing examples must always type-check.
#   2. AOT-compile (LLVM --release) every compiler regression fixture in
#      compiler/tests/fixtures/ AND JIT-run the side-effect-free ones.
#   3. AOT-compile every showcase app in examples/showcase/.
#
# Exit 0 only when every layer is clean. Used locally and by the release
# checklist; CI runs equivalent sweeps per-platform.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KRYOS="$REPO/compiler/target/release/kryos"
[[ "${OS:-}" == "Windows_NT" || "$(uname -s)" =~ MINGW|MSYS|CYGWIN ]] && KRYOS="$KRYOS.exe"
[[ -x "$KRYOS" ]] || { echo "examples-gate: build compiler first ($KRYOS missing)"; exit 2; }

if ! command -v clang >/dev/null 2>&1; then
    for c in "/c/Program Files/LLVM/bin" "/c/Program Files (x86)/LLVM/bin"; do
        [[ -x "$c/clang.exe" || -x "$c/clang" ]] && export PATH="$c:$PATH" && break
    done
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
fail=0

# Layer 1: type-check all root examples (non-wasm).
n=0; bad=0
for f in "$REPO"/examples/*.kry; do
    base="$(basename "$f")"
    [[ "$base" == wasm_* ]] && continue
    n=$((n+1))
    if ! "$KRYOS" check "$f" >/dev/null 2>&1; then
        echo "  CHECK FAIL examples/$base"
        bad=$((bad+1)); fail=1
    fi
done
echo "examples-gate: root check $((n-bad))/$n"

# Layer 2: fixtures — AOT-compile all; JIT-run the non-server ones.
# (ai_agent/http_api/mcp_server bind sockets or wait on stdin — compile only.)
NORUN="ai_agent http_api mcp_server"
n=0; bad=0
for f in "$REPO"/compiler/tests/fixtures/*.kry; do
    base="$(basename "$f" .kry)"
    n=$((n+1))
    if ! "$KRYOS" build "$f" --release --backend llvm -o "$TMP/fx_$base" >/dev/null 2>&1; then
        echo "  AOT FAIL fixtures/$base"
        bad=$((bad+1)); fail=1
        continue
    fi
    if [[ " $NORUN " != *" $base "* ]]; then
        if ! "$KRYOS" run "$f" >/dev/null 2>&1; then
            echo "  JIT-RUN FAIL fixtures/$base"
            bad=$((bad+1)); fail=1
        fi
    fi
done
echo "examples-gate: fixtures $((n-bad))/$n"

# Layer 3: showcase apps AOT-compile.
n=0; bad=0
for f in "$REPO"/examples/showcase/*.kry; do
    base="$(basename "$f" .kry)"
    n=$((n+1))
    if ! "$KRYOS" build "$f" --release --backend llvm -o "$TMP/sc_$base" >/dev/null 2>&1; then
        echo "  AOT FAIL showcase/$base"
        bad=$((bad+1)); fail=1
    fi
done
echo "examples-gate: showcase $((n-bad))/$n"

if [[ $fail -eq 0 ]]; then
    echo "examples-gate: PASS"
else
    echo "examples-gate: FAIL"
fi
exit $fail
