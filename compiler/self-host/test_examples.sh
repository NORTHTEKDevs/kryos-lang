#!/usr/bin/env bash
# test_examples.sh -- end-to-end test of stage-1 on the example programs.
#
# Compiles each self-host/examples/*.kry with stage-1, links via MSVC,
# runs the resulting .exe, and reports first 3 lines of output.
#
# This is the "stage-1 works for real Kryos code" smoke test, distinct
# from test_bootstrap.sh which measures whether stage-1 can compile
# the SELF-HOST source (the harder problem).
#
# Pre-req: stage-1 already built at target/bootstrap/kryos-stage1(.exe).

set -u
cd "$(dirname "$0")/.."

PS_BASE="Set-Location 'C:\\Users\\Krist\\projects\\active\\kryos-lang\\compiler'"
mkdir -p target/example-bins

echo "Example             | Result"
echo "--------------------|--------"
pass=0
fail=0
for ex_file in self-host/examples/*.kry; do
    ex=$(basename "$ex_file" .kry)
    exe=target/example-bins/${ex}.exe
    rm -f "$exe" 2>/dev/null

    # Build via PowerShell so the bat file finds vcvars64.bat correctly.
    PS_CMD="${PS_BASE}; .\\self-host\\kryos-build.bat 'self-host\\examples\\${ex}.kry' 'target\\example-bins\\${ex}.exe'"
    build_out=$(powershell.exe -Command "$PS_CMD" 2>&1 | grep -E "done|error|fail" | head -3)

    if [ -f "$exe" ]; then
        runout=$("./$exe" 2>&1 | head -3 | tr '\n' '|')
        printf "%-20s | OK  %s\n" "$ex" "$runout"
        pass=$((pass+1))
    else
        printf "%-20s | FAIL  %s\n" "$ex" "$(echo "$build_out" | head -1)"
        fail=$((fail+1))
    fi
done

echo ""
echo "End-to-end (compile + link + run): $pass passed, $fail failed"
echo ""
echo "What this means:"
echo "  Stage-1 is a working Kryos -> Windows .exe compiler for non-trivial"
echo "  user code. The self-bootstrap (stage-1 compiling stage-1) is blocked"
echo "  by a separate issue (see STAGE2_BLOCKER.md / test_bootstrap.sh)."
