#!/usr/bin/env bash
# RED TEAM round (toolchain-realworld lens, 2026-08-06): `kryos repl` only
# runs the capability checker for a function literally NAMED `main`. Every
# other way of executing a capability-gated builtin at the REPL -- a bare
# top-level statement, or a user-defined function with ANY other name --
# bypasses the checker completely and executes unconditionally. Verified
# with file_write (fs:write), file_read (fs:read), and env_get (process,
# explicitly called out in CLAUDE.md as capable of exfiltrating secrets).
#
# This is a COMPLETE, trivial, zero-effort bypass of the entire capability
# system through a first-class, documented CLI subcommand -- not a crafted
# closure-laundering shape. Compare the same builtin call across four paths
# on the IDENTICAL operation:
#   1. `kryos check`/`kryos run` on a .kry file with no @capabilities: E0505,
#      refuses to compile. (expected, correct)
#   2. `kryos eval "file_write(...)"` (--help: "wraps in fn main and runs"):
#      E0505, refuses to compile. (expected, correct -- eval goes through the
#      same wrap-in-fn-then-check pipeline as a real file)
#   3. `kryos repl` with the SAME call as a bare top-level statement (typing
#      `file_write(...)` directly at the prompt, the way every REPL user
#      interacts with a REPL): SUCCEEDS. File is written to disk. Zero
#      diagnostic.
#   4. `kryos repl` with the SAME call wrapped in a user-defined function
#      named `leak()` (anything other than `main`), then invoking `leak()`:
#      ALSO SUCCEEDS, zero diagnostic -- defining and calling a
#      non-`main`-named function is just as unchecked as a bare statement.
#   5. `kryos repl` with the IDENTICAL wrapping, but the function is named
#      `main()` and invoked as `main()`: correctly rejected with E0505.
#      This isolates the exact defect -- the REPL appears to special-case a
#      function literally named `main` (likely reusing the same
#      wrap-and-check machinery `kryos eval` uses) rather than running the
#      capability checker over every statement/function it evaluates; `main`
#      is the ONE name that happens to get checked, and it is not a name a
#      real REPL session has any reason to prefer.
#
# Classify by whether the target file actually exists on disk afterward (the
# real-world observable), not by exit code or by grepping the REPL's own
# echoed input.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KRYOS="${KRYOS:-$REPO_ROOT/compiler/target/release/kryos}"
if [[ ! -x "$KRYOS" && -x "$KRYOS.exe" ]]; then KRYOS="$KRYOS.exe"; fi
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$REPO_ROOT/compiler/stdlib}"

WORK_DIR="$(mktemp -d)"
cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

# Kryos.exe is a native Windows binary; a path embedded in text piped over
# stdin to `kryos repl` does NOT go through MSYS's automatic argv
# path-translation (unlike a path passed as a real CLI argument, which the
# other scripts in this directory rely on) -- it must already be a
# Windows-style path or the native fs calls resolve it against the wrong
# root. Convert once, up front, and use the Windows form for every REPL
# stdin script below.
if command -v cygpath >/dev/null 2>&1; then
    WORK_DIR_WIN="$(cygpath -m "$WORK_DIR")"
else
    WORK_DIR_WIN="$WORK_DIR"
fi
TARGET="$WORK_DIR/leak.txt"
# Forward-slash form (cygpath -m) so it drops cleanly into a Kryos string
# literal without colliding with Kryos's own backslash-escape syntax.
TARGET_WIN="$WORK_DIR_WIN/leak.txt"

echo "=== 1. kryos check: file_write with no @capabilities in a real file ==="
cat > "$WORK_DIR/no_caps.kry" <<EOF
fn main() {
    file_write("$TARGET", "leak")
}
EOF
CHECK_OUT="$("$KRYOS" check "$WORK_DIR/no_caps.kry" 2>&1)"
CHECK_EXIT=$?
echo "check exit=$CHECK_EXIT"
echo "$CHECK_OUT" | grep -q "E0505" && echo "  correctly rejected (E0505)"

echo
echo "=== 2. kryos eval: identical call, one-liner path ==="
EVAL_OUT="$("$KRYOS" eval "file_write(\"$TARGET\", \"leak\")" 2>&1)"
EVAL_EXIT=$?
echo "eval exit=$EVAL_EXIT"
echo "$EVAL_OUT" | grep -q "E0505" && echo "  correctly rejected (E0505)"
EVAL_WROTE=0
[[ -f "$TARGET" ]] && EVAL_WROTE=1
rm -f "$TARGET"

echo
echo "=== 3. kryos repl: identical call as a bare TOP-LEVEL statement ==="
REPL_OUT="$(printf 'let r = file_write("%s", "REPL LEAK: zero @capabilities declared anywhere")\nprintln("write_result=" + to_string(r))\n:quit\n' "$TARGET_WIN" | timeout 15 "$KRYOS" repl 2>&1)"
echo "$REPL_OUT"
TOPLEVEL_WROTE=0
if [[ -f "$TARGET" ]] && grep -q "REPL LEAK" "$TARGET" 2>/dev/null; then
    TOPLEVEL_WROTE=1
fi
rm -f "$TARGET"

echo
echo "=== 4. kryos repl: identical call wrapped in a NON-main-named fn, then called ==="
REPL_FN_OUT="$(printf 'fn leak() {\n    file_write("%s", "should not exist")\n}\nleak()\n:quit\n' "$TARGET_WIN" | timeout 15 "$KRYOS" repl 2>&1)"
echo "$REPL_FN_OUT"
FN_WROTE=0
[[ -f "$TARGET" ]] && FN_WROTE=1
FN_REJECTED=0
echo "$REPL_FN_OUT" | grep -q "E0505" && FN_REJECTED=1
rm -f "$TARGET"

echo
echo "=== 4b. kryos repl: identical call wrapped in fn main(), then main() called ==="
REPL_MAIN_OUT="$(printf 'fn main() {\n    file_write("%s", "should not exist either")\n}\nmain()\n:quit\n' "$TARGET_WIN" | timeout 15 "$KRYOS" repl 2>&1)"
echo "$REPL_MAIN_OUT"
MAIN_WROTE=0
[[ -f "$TARGET" ]] && MAIN_WROTE=1
MAIN_REJECTED=0
echo "$REPL_MAIN_OUT" | grep -q "E0505" && MAIN_REJECTED=1
rm -f "$TARGET"

echo
echo "=== 5. kryos repl: env_get (process capability) as a bare top-level statement ==="
ENV_OUT="$(printf 'let e = env_get("USERNAME")\nprintln("env_leak=" + e)\n:quit\n' | timeout 15 "$KRYOS" repl 2>&1)"
echo "$ENV_OUT"
ENV_LEAKED=0
echo "$ENV_OUT" | grep -q "env_leak=" && ! echo "$ENV_OUT" | grep -q "env_leak=$" && ENV_LEAKED=1

echo
if [[ "$CHECK_EXIT" -ne 0 && "$EVAL_WROTE" -eq 0 && \
      "$TOPLEVEL_WROTE" -eq 1 && \
      "$FN_WROTE" -eq 1 && "$FN_REJECTED" -eq 0 && \
      "$MAIN_WROTE" -eq 0 && "$MAIN_REJECTED" -eq 1 && \
      "$ENV_LEAKED" -eq 1 ]]; then
    echo "CONFIRMED: kryos repl only runs the capability checker for a"
    echo "function literally named \`main\`. The identical file_write call"
    echo "is correctly rejected by check/run/eval and by a repl-defined"
    echo "main(), but SUCCEEDS unconditionally both as a bare top-level"
    echo "statement AND inside a repl-defined function with any OTHER name"
    echo "-- and the same holds for env_get (process capability,"
    echo "secret-exfiltration relevant per CLAUDE.md)."
    exit 0
else
    echo "NOT (fully) REPRODUCED -- check_exit=$CHECK_EXIT eval_wrote=$EVAL_WROTE"
    echo "toplevel_wrote=$TOPLEVEL_WROTE fn_wrote=$FN_WROTE fn_rejected=$FN_REJECTED"
    echo "main_wrote=$MAIN_WROTE main_rejected=$MAIN_REJECTED"
    echo "env_leaked=$ENV_LEAKED -- this would mean the repl now enforces"
    echo "capabilities more broadly than just a function named main."
    exit 1
fi
