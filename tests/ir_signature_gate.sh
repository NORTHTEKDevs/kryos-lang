#!/usr/bin/env bash
# IR signature gate — every LLVM call must agree with its callee's declaration.
#
# Guards a bug CLASS, not a bug. `call i64 @kryos_json_bool(i1 %b)` was emitted
# against `declare i64 @kryos_json_bool(i64)`: LLVM accepted the text, but the
# upper 63 bits of the argument register were undefined and the callee's
# `val != 0` test read garbage, so json_bool(false) built a JSON `true` on
# --release while the JIT was correct. Nothing in the test suite could see it --
# the value was only wrong under the right register pressure, and any nearby
# read of the same bool normalized the slot and masked it.
#
# The checker found two more of the same shape (char_from, string_char_at, both
# i32-against-i64) that had never surfaced as a failure: they survive today only
# because 32-bit x86-64 ops happen to zero the upper half of the register.
#
# FAILS on a narrow value passed where a wider one is declared (undefined upper
# bits) and on scalar/aggregate or int/float register-class mismatches. Reports
# but does not fail on narrowing READS of a return value -- those are safe while
# the runtime predicates return strictly 0/1, which they do.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KRYOS="$REPO/compiler/target/release/kryos"
[[ "${OS:-}" == "Windows_NT" || "$(uname -s)" =~ MINGW|MSYS|CYGWIN ]] && KRYOS="$KRYOS.exe"
[[ -x "$KRYOS" ]] || { echo "ir-signatures: build compiler first ($KRYOS missing)"; exit 2; }

PY=python
command -v python >/dev/null 2>&1 || PY=python3
command -v "$PY" >/dev/null 2>&1 || { echo "ir-signatures: SKIP (no python)"; exit 0; }

if ! command -v clang >/dev/null 2>&1; then
    for c in "/c/Program Files/LLVM/bin" "/c/Program Files (x86)/LLVM/bin"; do
        [[ -x "$c/clang.exe" || -x "$c/clang" ]] && export PATH="$c:$PATH" && break
    done
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# The conformance suite is the densest feature coverage per compile, which
# keeps this gate affordable while still exercising every codegen path that
# emits a runtime call.
mapfile -t SRCS < <(ls "$REPO"/tests/conformance/*.kry 2>/dev/null)
(( ${#SRCS[@]} > 0 )) || { echo "ir-signatures: no conformance sources found"; exit 2; }

emitted=0
failed_emit=0
for f in "${SRCS[@]}"; do
    b="$(basename "$f" .kry)"
    if timeout 240 "$KRYOS" build --release --emit-llvm "$f" -o "$TMP/$b.ll" >/dev/null 2>&1 \
       && [[ -s "$TMP/$b.ll" ]]; then
        emitted=$((emitted+1))
    else
        echo "  emit failed: $b"
        failed_emit=$((failed_emit+1))
    fi
done

if (( emitted == 0 )); then
    echo "ir-signatures: FAIL — emitted no IR at all"
    exit 1
fi

echo "ir-signatures: checking $emitted emitted module(s)"
if ! "$PY" "$REPO/tests/tools/check_ir_signatures.py" "$TMP"/*.ll; then
    echo "ir-signatures: FAIL"
    exit 1
fi

if (( failed_emit > 0 )); then
    echo "ir-signatures: FAIL — $failed_emit source(s) did not emit IR"
    exit 1
fi

echo "ir-signatures: PASS"
