#!/usr/bin/env bash
# test_bootstrap.sh -- baseline test of stage-1 self-host compilation.
#
# Builds stage-1 from current source, then compiles every self-host
# module with it, and prints OK / FAIL per module. Pass rate is the
# key bootstrap-progress metric.
#
# Usage (from compiler/):
#   self-host/test_bootstrap.sh
#
# Env:
#   KRYOS_SKIP_TYPES=1   - bypass stage-1's (incomplete) type checker
#   KRYOS_USE_REALLOC=1  - already default since baff370; here for clarity
#   KRYOS_CG_TRACE=1     - print per-function codegen progress
#
# Pre-fix baseline:  4/16 modules pass.
# 2026-06 baseline: 11/16 modules pass (codegen flaky -- sometimes 12).
# 2026-07-01:       16/16, stable across 4 consecutive runs (the MIR
#                   aggregate-lowering fixes landed in launch hardening).

set -u
cd "$(dirname "$0")/.."

# 1. Ensure kryos.exe (stage-0) is built.
if [ ! -x target/release/kryos.exe ] && [ ! -x target/release/kryos ]; then
    echo "Building stage-0 (kryos.exe) ..."
    cargo build --release -j 2 >/dev/null 2>&1 || {
        echo "FAIL: stage-0 build failed"
        exit 1
    }
fi
STAGE0=target/release/kryos.exe
[ ! -x "$STAGE0" ] && STAGE0=target/release/kryos

# 2. Build stage-1 from self-host source.
# KRYOS_NO_ASLR=1 keeps the VA layout deterministic across stage-1
# invocations, removing one source of bootstrap heap-state flake.
echo "Building stage-1 from self-host/main.kry ..."
KRYOS_NO_ASLR=1 "$STAGE0" build self-host/main.kry -o target/bootstrap/kryos-stage1 --skip-ownership 2>&1 | grep -iE "^error" | head -3
[ ! -f target/bootstrap/kryos-stage1 ] && { echo "FAIL: stage-1 build failed"; exit 1; }
cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe 2>/dev/null
STAGE1=target/bootstrap/kryos-stage1.exe
[ ! -x "$STAGE1" ] && STAGE1=target/bootstrap/kryos-stage1

# 3. Compile every self-host module with stage-1.
echo ""
echo "Module               | Result"
echo "---------------------|--------"
pass=0
fail=0
failed=""
for f in token lexer ast parser types mir lower optimize regalloc x86 codegen elf coff linker runtime main; do
    src=self-host/$f.kry
    [ ! -f "$src" ] && continue
    out=$(KRYOS_SKIP_TYPES=1 "$STAGE1" obj "$src" -o /tmp/_${f}.obj 2>&1)
    rc=$?
    if [ $rc -eq 0 ]; then
        printf "%-20s | OK\n" "$f.kry"
        pass=$((pass+1))
    else
        printf "%-20s | FAIL (rc=%d)\n" "$f.kry" "$rc"
        fail=$((fail+1))
        failed="$failed $f"
        # Surface any KRYOS_* diagnostic lines from the swallowed output
        # (DOUBLE-FREE, PANIC, corrupt array header, etc.). Keep concise.
        echo "$out" | grep -iE "DOUBLE-FREE|kryos panic|corrupt array|assertion failed" | head -3 | sed 's/^/                       | /'
    fi
done
echo ""
echo "PASS: $pass / 16    FAIL: $fail / 16"
[ -n "$failed" ] && echo "Failed:$failed"
echo ""
echo "Notes:"
echo "  - 16/16 since 2026-07-01 (stable across repeat runs). If a module"
echo "    regresses, KRYOS_CG_TRACE=1 shows which function it crashes on."
echo "  - Use KRYOS_CG_TRACE=1 to see which function each module crashes on."
echo ""
exit $([ "$fail" -eq 0 ] && echo 0 || echo 1)
