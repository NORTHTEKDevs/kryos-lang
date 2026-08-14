#!/usr/bin/env bash
# LEDGER item 13, FIXED 2026-08-13 -- regression pin.
#
# RED TEAM round 1 (toolchain-supply lens, found 2026-08-04): `kryos audit`
# ("Audit capability usage, extern surface, and secret patterns" per its own
# --help text) was completely blind to capability violations that `kryos
# check`/`kryos run`/`kryos build` reject outright. It never cross-referenced
# the same capability checker check/build use -- it only printed a static
# inventory of any `@capabilities(...)` annotations that were PRESENT and a
# regex sweep for extern blocks / secret-looking strings. A program with NO
# annotations, calling a builtin that unconditionally requires a capability,
# was reported as clean (exit 0, "(no @capabilities annotations found)")
# even though `kryos check` on the SAME file failed with E0505 and refused
# to compile at all.
#
# FIX: `kryos audit` now re-runs the SAME inferred-mode capability
# inference/enforcement pass `kryos check`/`run`/`build` use, per file
# (compiler/crates/kryos-cli/src/commands/audit_cmd.rs::check_cap_violations,
# via kryos_driver::check_file_with_options_full(.., CapabilityMode::Inferred)),
# and surfaces the resulting E0500-E0508 diagnostics in a dedicated
# "Capability violations" section. `audit` now exits non-zero when it finds
# one, matching what `kryos check` would do -- a reviewer can no longer get a
# clean bill of health on code the compiler refuses to build. See the
# `audit`-vs-`doc` companion gap (a DIFFERENT, still-open LEDGER item) in
# tests/security/doc_never_shows_capabilities.sh.
#
# This script now asserts the FIXED behavior and pins it: `kryos check` and
# `kryos audit` must AGREE (both reject) on the identical file, and audit's
# own output must name the real violation (E0505 / file_write / fs:write),
# not just "no @capabilities annotations found".

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
if [[ "$CHECK_EXIT" -ne 0 ]] && [[ "$AUDIT_EXIT" -ne 0 ]] && \
   echo "$AUDIT_OUT" | grep -q "Capability violations" && \
   echo "$AUDIT_OUT" | grep -q "E0505" && \
   echo "$AUDIT_OUT" | grep -q "fs:write"; then
    echo "FIXED (regression pin holds): kryos check rejects this file"
    echo "(capability violation, exit $CHECK_EXIT) and kryos audit now"
    echo "AGREES -- it surfaces the E0505/fs:write violation by name and"
    echo "exits non-zero ($AUDIT_EXIT) instead of reporting a false clean"
    echo "bill of health. LEDGER item 13, CLOSED."
    exit 0
else
    echo "REGRESSION: kryos audit no longer surfaces this capability"
    echo "violation the way LEDGER item 13's fix requires"
    echo "(check_exit=$CHECK_EXIT audit_exit=$AUDIT_EXIT) -- audit has"
    echo "regressed back to a false-clean report. Do not re-close this item"
    echo "without re-reading compiler/crates/kryos-cli/src/commands/audit_cmd.rs."
    exit 1
fi
