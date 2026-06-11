#!/usr/bin/env bash
# Backend parity matrix runner.
#
# Builds the compiler once in release mode (if not already), then for every
# .kry file in tests/smoke/ runs it under both backends:
#
#   - Cranelift  : `kryos run FILE`
#   - LLVM       : `kryos build FILE --release --backend llvm -o BIN && BIN`
#
# Records pass/fail per file per backend, prints a matrix, and exits 0 only
# when both backends agree on every test. Designed to be CI-runnable; see
# .github/workflows/ci.yml step "parity matrix".
#
# Output:
#   tests/parity/results/parity-<sha>.txt   — human-readable matrix
#   tests/parity/results/parity-<sha>.json  — machine-readable, one entry per test
#
# Usage:
#   tests/parity/run_parity.sh              # run all smoke tests
#   tests/parity/run_parity.sh test_xyz     # run a single test (no .kry extension)
#   FAIL_FAST=1 tests/parity/run_parity.sh  # stop at first divergence
#
# Exit codes:
#   0 — every smoke test passes on both backends
#   1 — at least one divergence (backend disagreement) or runtime failure
#   2 — environment / build failure (compiler did not build)

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SMOKE_DIR="$REPO_ROOT/tests/smoke"
RESULTS_DIR="$REPO_ROOT/tests/parity/results"
mkdir -p "$RESULTS_DIR"

# Ensure clang is reachable. The LLVM AOT path shells out to clang for
# the .ll -> object step. On Windows, the standard LLVM installer puts
# clang.exe at C:\Program Files\LLVM\bin which isn't on PATH by default.
if ! command -v clang >/dev/null 2>&1; then
    for candidate in \
        "/c/Program Files/LLVM/bin" \
        "/c/Program Files (x86)/LLVM/bin" \
        "$HOME/scoop/apps/llvm/current/bin"; do
        if [[ -x "$candidate/clang.exe" || -x "$candidate/clang" ]]; then
            export PATH="$candidate:$PATH"
            echo "parity: added clang to PATH from $candidate"
            break
        fi
    done
fi
if ! command -v clang >/dev/null 2>&1; then
    echo "parity: WARN — clang not found on PATH; LLVM AOT half will fail with no-such-clang"
    echo "parity: install LLVM and re-run, e.g.: winget install LLVM.LLVM"
fi

SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo nogit)"
TXT_OUT="$RESULTS_DIR/parity-$SHA.txt"
JSON_OUT="$RESULTS_DIR/parity-$SHA.json"

# Locate or build the compiler.
KRYOS_BIN="$REPO_ROOT/compiler/target/release/kryos"
if [[ "${OS:-}" == "Windows_NT" || "$(uname -s 2>/dev/null)" =~ MINGW|MSYS|CYGWIN ]]; then
    KRYOS_BIN="$KRYOS_BIN.exe"
fi

if [[ ! -x "$KRYOS_BIN" ]]; then
    echo "parity: kryos binary not found at $KRYOS_BIN — building..."
    ( cd "$REPO_ROOT/compiler" && cargo build --release -j 2 ) || {
        echo "parity: FATAL — cargo build --release failed"
        exit 2
    }
fi

if [[ ! -x "$KRYOS_BIN" ]]; then
    echo "parity: FATAL — kryos binary still not at $KRYOS_BIN after build"
    exit 2
fi

# Optional single-test filter.
FILTER="${1:-}"

# Collect smoke files.
SMOKE_FILES=()
if [[ -n "$FILTER" ]]; then
    f="$SMOKE_DIR/$FILTER.kry"
    [[ -f "$f" ]] || { echo "parity: no such smoke file: $f"; exit 2; }
    SMOKE_FILES=("$f")
else
    while IFS= read -r -d '' f; do SMOKE_FILES+=("$f"); done \
        < <(find "$SMOKE_DIR" -maxdepth 1 -name '*.kry' -print0 | sort -z)
fi

TOTAL=${#SMOKE_FILES[@]}
echo "parity: $TOTAL smoke test(s) at $SHA"
echo "parity: kryos = $KRYOS_BIN"

# Per-test runner. Echoes a single result line:
#   <name>\t<cranelift_status>\t<llvm_status>\t<class>
# where status ∈ {PASS, FAIL_BUILD, FAIL_RUN, FAIL_DIFF}
# and `class` is one of A/B/B'/C/D/E/T/-, classified at write-time.
run_one() {
    local kry="$1"
    local name; name="$(basename "$kry" .kry)"
    local tmp_out tmp_err exe_out exe_path
    tmp_out="$(mktemp)"; tmp_err="$(mktemp)"; exe_out="$(mktemp)"
    exe_path="$(mktemp -u)"
    [[ "${OS:-}" == "Windows_NT" ]] && exe_path="$exe_path.exe"

    # Cranelift: kryos run
    local cl_status="PASS"
    if ! "$KRYOS_BIN" run "$kry" > "$tmp_out" 2> "$tmp_err"; then
        cl_status="FAIL_RUN"
    fi

    # LLVM: kryos build --release --backend llvm -o BIN ; ./BIN
    local llvm_status="PASS"
    local llvm_err=""
    if ! "$KRYOS_BIN" build "$kry" --release --backend llvm -o "$exe_path" \
            > "$tmp_out" 2> "$tmp_err"; then
        llvm_status="FAIL_BUILD"
        llvm_err="$(head -c 4000 "$tmp_err")"
    else
      "$exe_path" > "$exe_out" 2>> "$tmp_err"
      rc=$?
      if [[ $rc -ne 0 ]]; then
        if [[ $rc -eq 126 || $rc -eq 127 ]]; then
            # Freshly-written exe blocked by the AV on-first-exec scan
            # (Windows Defender holds new binaries briefly; rc 127 "not
            # found" despite the file existing). One retry after settling.
            sleep 2
            if ! "$exe_path" > "$exe_out" 2>> "$tmp_err"; then
                llvm_status="FAIL_RUN"
                llvm_err="$(head -c 4000 "$tmp_err")"
            fi
        else
            llvm_status="FAIL_RUN"
            llvm_err="$(head -c 4000 "$tmp_err")"
        fi
      fi
    fi

    # Classify failure (only meaningful when LLVM != Cranelift).
    local class="-"
    if [[ "$llvm_status" != "PASS" ]]; then
        if   grep -q "use of undefined value '@"           <<<"$llvm_err"; then class="A"
        elif grep -qE "defined with type 'i64' but expected 'ptr'"   <<<"$llvm_err"; then class="B"
        elif grep -qE "defined with type 'ptr' but expected 'i64'"   <<<"$llvm_err"; then class="B'"
        elif grep -q "multiple definition of local value"  <<<"$llvm_err"; then class="C"
        elif grep -qE "invalid cast opcode|opaque"         <<<"$llvm_err"; then class="D"
        elif grep -q "insertvalue operand must be aggregate" <<<"$llvm_err"; then class="E"
        elif [[ "$llvm_status" == "FAIL_RUN" ]]; then class="T"
        fi
    fi

    rm -f "$tmp_out" "$tmp_err" "$exe_out" "$exe_path" 2>/dev/null || true
    printf '%s\t%s\t%s\t%s\n' "$name" "$cl_status" "$llvm_status" "$class"
}

# Run sequentially. Parallelising risks cargo-build races for `kryos build`.
declare -a ROWS=()
PASS_BOTH=0
FAIL_LLVM=0
FAIL_CL=0

for f in "${SMOKE_FILES[@]}"; do
    row="$(run_one "$f")"
    ROWS+=("$row")
    cl="$(cut -f2 <<<"$row")"; llvm="$(cut -f3 <<<"$row")"; name="$(cut -f1 <<<"$row")"; class="$(cut -f4 <<<"$row")"
    if [[ "$cl" == "PASS" && "$llvm" == "PASS" ]]; then
        ((PASS_BOTH+=1))
        echo "  OK    $name"
    elif [[ "$cl" != "PASS" ]]; then
        ((FAIL_CL+=1))
        echo "  CL!   $name ($cl)"
        [[ "${FAIL_FAST:-0}" == "1" ]] && break
    else
        ((FAIL_LLVM+=1))
        echo "  LLVM!  $name ($llvm, class=$class)"
        [[ "${FAIL_FAST:-0}" == "1" ]] && break
    fi
done

# Write outputs.
{
    printf 'parity matrix @ %s\n' "$SHA"
    printf 'total=%d  both_pass=%d  llvm_only_fail=%d  cranelift_fail=%d\n\n' \
        "$TOTAL" "$PASS_BOTH" "$FAIL_LLVM" "$FAIL_CL"
    printf '%-50s  %-10s  %-10s  %s\n' "test" "cranelift" "llvm" "class"
    printf '%-50s  %-10s  %-10s  %s\n' "----" "---------" "----" "-----"
    for row in "${ROWS[@]}"; do
        n="$(cut -f1 <<<"$row")"; c="$(cut -f2 <<<"$row")"; l="$(cut -f3 <<<"$row")"; cls="$(cut -f4 <<<"$row")"
        printf '%-50s  %-10s  %-10s  %s\n' "$n" "$c" "$l" "$cls"
    done
} | tee "$TXT_OUT"

{
    printf '{\n  "sha": "%s",\n  "total": %d,\n  "both_pass": %d,\n  "llvm_only_fail": %d,\n  "cranelift_fail": %d,\n  "tests": [\n' \
        "$SHA" "$TOTAL" "$PASS_BOTH" "$FAIL_LLVM" "$FAIL_CL"
    sep=""
    for row in "${ROWS[@]}"; do
        n="$(cut -f1 <<<"$row")"; c="$(cut -f2 <<<"$row")"; l="$(cut -f3 <<<"$row")"; cls="$(cut -f4 <<<"$row")"
        printf '%s    {"name":"%s","cranelift":"%s","llvm":"%s","class":"%s"}' "$sep" "$n" "$c" "$l" "$cls"
        sep=",\n"
    done
    printf '\n  ]\n}\n'
} > "$JSON_OUT"

echo
echo "parity: txt  -> $TXT_OUT"
echo "parity: json -> $JSON_OUT"

# Exit non-zero on any divergence.
if (( FAIL_LLVM > 0 || FAIL_CL > 0 )); then
    exit 1
fi
exit 0
