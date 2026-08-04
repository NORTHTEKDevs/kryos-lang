#!/usr/bin/env bash
# RED TEAM round 1 (toolchain-supply lens, 2026-08-04): `kryos audit`
# ("Audit capability usage, extern surface, and secret patterns" per its own
# --help text) is completely blind to capability violations that `kryos
# check`/`kryos run`/`kryos build` reject outright. It never cross-references
# the same capability checker check/build use -- it just prints a static
# inventory of any `@capabilities(...)` annotations that are PRESENT and a
# regex sweep for extern blocks / secret-looking strings. A program with NO
# annotations, calling builtins that unconditionally require a capability, is
# reported as clean (exit 0, "(no @capabilities annotations found)") even
# though `kryos check` on the SAME file fails with E0505 and refuses to
# compile at all.
#
# This matters because `kryos audit`'s own description ("audit capability
# usage") invites exactly the wrong workflow: a reviewer who runs `kryos
# audit` on a third-party package expecting a security-relevant capability
# report gets a clean bill of health on code that is either (a) certain to
# fail to build the moment someone tries, or (b) -- the more dangerous case
# in a real package with SOME correct annotations elsewhere in the tree --
# silently omits the specific gated builtins actually reachable, since audit
# never runs the inference/enforcement pass check/build use to find them.
#
# Classify by comparing `kryos check`'s exit code (1, real capability
# errors) against `kryos audit`'s exit code and printed content (0, clean)
# on the IDENTICAL file -- not by grepping audit's own output for a marker.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KRYOS="${KRYOS:-$REPO_ROOT/compiler/target/release/kryos}"
if [[ ! -x "$KRYOS" && -x "$KRYOS.exe" ]]; then KRYOS="$KRYOS.exe"; fi
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$REPO_ROOT/compiler/stdlib}"

WORK_DIR="$(mktemp -d)"
cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

cat > "$WORK_DIR/undeclared_write.kry" <<'EOF'
fn main() {
    file_write("out.txt", "hi")
    println("wrote")
}
EOF

echo "=== kryos check (no @capabilities on main; calls file_write) ==="
"$KRYOS" check "$WORK_DIR/undeclared_write.kry"
CHECK_EXIT=$?
echo "check exit=$CHECK_EXIT"

echo
echo "=== kryos audit on the IDENTICAL file ==="
AUDIT_OUT="$("$KRYOS" audit "$WORK_DIR/undeclared_write.kry" 2>&1)"
AUDIT_EXIT=$?
echo "$AUDIT_OUT"
echo "audit exit=$AUDIT_EXIT"

echo
if [[ "$CHECK_EXIT" -ne 0 ]] && [[ "$AUDIT_EXIT" -eq 0 ]] && \
   echo "$AUDIT_OUT" | grep -q "no @capabilities annotations found"; then
    echo "CONFIRMED: kryos check rejects this file (capability violation,"
    echo "exit $CHECK_EXIT) while kryos audit reports it clean (exit 0) on"
    echo "the SAME source -- audit is blind to what the checker catches."
    echo "LEDGER item 13."
    exit 0
else
    echo "NOT REPRODUCED (check_exit=$CHECK_EXIT audit_exit=$AUDIT_EXIT) --"
    echo "this would mean audit now cross-checks capability requirements."
    exit 1
fi
