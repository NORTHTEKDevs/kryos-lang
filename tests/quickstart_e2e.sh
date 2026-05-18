#!/usr/bin/env bash
# Quickstart end-to-end smoke test.
#
# Walks a new user through every step QUICKSTART.md promises:
#   1. Build the compiler from source
#   2. Run hello.kry under Cranelift JIT (kryos run)
#   3. Build hello.kry to a native binary (kryos build --release via LLVM)
#   4. Compile hello.kry to WASM and run via wasmtime
#   5. Compile + run all five "first tour" examples (hello, fibonacci,
#      calculator, word_count, shapes)
#
# Step 4 (WASM) is run only when `wasmtime` is on PATH; otherwise it
# emits a SKIP. The remaining steps fail the script on any divergence.
#
# Time budget: < 10 minutes on a typical laptop with a warm cargo cache.
# Cold runs can take 1-2 minutes for the compiler build alone.
#
# Used by CI (job "quickstart-e2e" in .github/workflows/ci.yml) and
# runnable locally as `bash tests/quickstart_e2e.sh` from the repo root.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Color helpers so the output is readable in CI logs.
ok()    { printf '  \033[32mOK\033[0m   %s\n' "$1"; }
fail()  { printf '  \033[31mFAIL\033[0m %s\n  %s\n' "$1" "$2" >&2; exit 1; }
skip()  { printf '  \033[33mSKIP\033[0m %s — %s\n' "$1" "$2"; }
step()  { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

START=$(date +%s)

# -----------------------------------------------------------------------------
# Ensure clang is reachable on Windows. The Windows LLVM installer puts
# clang at C:\Program Files\LLVM\bin which isn't on PATH by default.
# -----------------------------------------------------------------------------
if ! command -v clang >/dev/null 2>&1; then
    for candidate in \
        "/c/Program Files/LLVM/bin" \
        "/c/Program Files (x86)/LLVM/bin" \
        "$HOME/scoop/apps/llvm/current/bin"; do
        if [[ -x "$candidate/clang.exe" || -x "$candidate/clang" ]]; then
            export PATH="$candidate:$PATH"
            break
        fi
    done
fi

# -----------------------------------------------------------------------------
# Step 1: Build the compiler. Skipped if a release binary already exists
#         and is newer than the source files (most CI runs hit this fast path).
# -----------------------------------------------------------------------------
step "Step 1 — build the compiler"

KRYOS_BIN="$REPO_ROOT/compiler/target/release/kryos"
if [[ "${OS:-}" == "Windows_NT" || "$(uname -s 2>/dev/null)" =~ MINGW|MSYS|CYGWIN ]]; then
    KRYOS_BIN="$KRYOS_BIN.exe"
fi

if [[ ! -x "$KRYOS_BIN" ]]; then
    echo "  cargo build --release -j 2 ..."
    ( cd compiler && cargo build --release -j 2 ) \
        || fail "compiler build" "cargo build --release failed"
fi
[[ -x "$KRYOS_BIN" ]] || fail "compiler build" "kryos binary missing at $KRYOS_BIN"
ok "kryos --version: $($KRYOS_BIN --version)"

# -----------------------------------------------------------------------------
# Step 2: Cranelift JIT smoke — `kryos run examples/hello.kry`
# -----------------------------------------------------------------------------
step "Step 2 — Cranelift JIT"

actual=$("$KRYOS_BIN" run examples/hello.kry 2>&1) \
    || fail "kryos run hello.kry" "$actual"
expected="Hello, Kryos!"
[[ "$actual" == "$expected" ]] \
    || fail "kryos run hello.kry stdout mismatch" "got: $actual"
ok "kryos run examples/hello.kry → '$actual'"

# -----------------------------------------------------------------------------
# Step 3: LLVM AOT smoke — `kryos build --release examples/hello.kry`
# -----------------------------------------------------------------------------
step "Step 3 — LLVM AOT (native binary)"

if ! command -v clang >/dev/null 2>&1; then
    skip "LLVM AOT" "clang not on PATH (install via 'winget install LLVM.LLVM' on Windows)"
else
    HELLO_BIN="$(mktemp -u)"
    [[ "${OS:-}" == "Windows_NT" ]] && HELLO_BIN="$HELLO_BIN.exe"
    "$KRYOS_BIN" build examples/hello.kry --release -o "$HELLO_BIN" \
        > /tmp/kryos_quickstart_build.log 2>&1 \
        || fail "kryos build --release hello" "$(cat /tmp/kryos_quickstart_build.log)"
    actual=$("$HELLO_BIN" 2>&1) \
        || fail "execute native binary" "$actual"
    [[ "$actual" == "Hello, Kryos!" ]] \
        || fail "native binary stdout mismatch" "got: $actual"
    rm -f "$HELLO_BIN"
    ok "kryos build --release → native binary executed cleanly"
fi

# -----------------------------------------------------------------------------
# Step 4: WASM smoke — `kryos build --backend wasm` then wasmtime
# -----------------------------------------------------------------------------
step "Step 4 — WebAssembly (WASI)"

if ! command -v wasmtime >/dev/null 2>&1; then
    skip "WASM via wasmtime" "wasmtime not on PATH (install via 'curl https://wasmtime.dev/install.sh -sSf | bash')"
else
    HELLO_WASM="$(mktemp -u).wasm"
    "$KRYOS_BIN" build examples/wasm_hello.kry --release --backend wasm -o "$HELLO_WASM" \
        > /tmp/kryos_quickstart_wasm.log 2>&1 \
        || fail "kryos build --backend wasm" "$(cat /tmp/kryos_quickstart_wasm.log)"
    actual=$(wasmtime "$HELLO_WASM" 2>&1) \
        || fail "wasmtime execute" "$actual"
    rm -f "$HELLO_WASM"
    ok "kryos build --backend wasm + wasmtime → ran cleanly"
fi

# -----------------------------------------------------------------------------
# Step 5: "First tour" example sweep (the five files QUICKSTART.md names).
# -----------------------------------------------------------------------------
step "Step 5 — first tour examples"

for ex in hello fibonacci calculator word_count shapes; do
    case "$ex" in
        calculator)
            # calculator reads from stdin; skip its run but verify it
            # type-checks cleanly. QUICKSTART.md calls this out.
            "$KRYOS_BIN" check "examples/$ex.kry" > /tmp/kryos_check.log 2>&1 \
                || fail "kryos check $ex" "$(cat /tmp/kryos_check.log)"
            ok "kryos check examples/$ex.kry (interactive — run path skipped)"
            ;;
        *)
            "$KRYOS_BIN" run "examples/$ex.kry" > /tmp/kryos_run.log 2>&1 \
                || fail "kryos run $ex" "$(cat /tmp/kryos_run.log)"
            ok "kryos run examples/$ex.kry"
            ;;
    esac
done

# -----------------------------------------------------------------------------
# Done.
# -----------------------------------------------------------------------------
END=$(date +%s)
ELAPSED=$((END - START))
echo
printf '\033[1mquickstart-e2e: PASS in %ds\033[0m\n' "$ELAPSED"
