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
    # `*_overreach.kry` are must-FAIL capability demos, asserted in Layer 5.
    case "$base" in *_overreach) continue ;; esac
    n=$((n+1))
    if ! "$KRYOS" build "$f" --release --backend llvm -o "$TMP/sc_$base" >/dev/null 2>&1; then
        echo "  AOT FAIL showcase/$base"
        bad=$((bad+1)); fail=1
    fi
done
echo "examples-gate: showcase $((n-bad))/$n"

# Layer 5: negative safety demos MUST be rejected. Each `*_overreach.kry` is a
# counterexample -- a tool exceeding the agent's granted capabilities, or a raw
# operation outside `unsafe` -- that the compiler must refuse with a safety
# error (E0500 unsafe, or E0505/E0506/E0507 capability). A demo that silently
# COMPILES would be a false safety claim.
neg=0; negbad=0
for f in "$REPO"/examples/showcase/*_overreach.kry; do
    [ -f "$f" ] || continue
    base="$(basename "$f" .kry)"
    neg=$((neg+1))
    out="$("$KRYOS" check "$f" 2>&1)"
    if echo "$out" | grep -qE 'error\[E050[0567]\]'; then
        :  # correctly rejected
    else
        echo "  NEGATIVE FAIL showcase/$base (expected a capability rejection, got none)"
        negbad=$((negbad+1)); fail=1
    fi
done
[ "$neg" -gt 0 ] && echo "examples-gate: capability-rejection $((neg-negbad))/$neg"

# Layer 4: multi-file project -- cross-module imports, bare AND module-qualified
# calls (`util::add` / alias `u::add`), JIT + AOT. Regression: the checker
# rejected module-qualified calls ("no method `add` found on type `util`")
# while the MIR lowering resolved them fine.
PROJ="$TMP/gate_proj"
mkdir -p "$PROJ/src"
cat > "$PROJ/kryos.toml" <<'EOF'
[package]
name = "gate_proj"
version = "0.1.0"
EOF
cat > "$PROJ/src/util.kry" <<'EOF'
fn add(a: i64, b: i64) -> i64 { return a + b }
fn shout(s: str) -> str { return s + "!" }
EOF
cat > "$PROJ/src/main.kry" <<'EOF'
use util as u

fn main() {
    println(to_string(add(1, 2)))
    println(to_string(util::add(3, 4)))
    println(to_string(u::add(5, 6)))
    println(shout("mod"))
}
EOF
expect_proj="3
7
11
mod!"
got_jit=$(cd "$PROJ" && "$KRYOS" run src/main.kry 2>/dev/null | tr -d '')
if [[ "$got_jit" != "$expect_proj" ]]; then
    echo "  PROJECT JIT FAIL (got: $(echo "$got_jit" | tr '
' ' '))"
    fail=1
fi
if (cd "$PROJ" && "$KRYOS" build --release src/main.kry -o "$PROJ/out" >/dev/null 2>&1); then
    got_aot=$("$PROJ/out" 2>/dev/null | tr -d '')
    if [[ "$got_aot" != "$expect_proj" ]]; then
        echo "  PROJECT AOT WRONG (got: $(echo "$got_aot" | tr '
' ' '))"
        fail=1
    fi
else
    echo "  PROJECT AOT BUILD FAIL"
    fail=1
fi
[[ $fail -eq 0 ]] && echo "examples-gate: project ok"

if [[ $fail -eq 0 ]]; then
    echo "examples-gate: PASS"
else
    echo "examples-gate: FAIL"
fi
exit $fail
