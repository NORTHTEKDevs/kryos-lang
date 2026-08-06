#!/usr/bin/env bash
# RED TEAM round (toolchain-realworld lens, 2026-08-06): `kryos doc` never
# surfaces `@capabilities(...)` annotations anywhere in generated
# documentation, for any function, even one explicitly declared
# `@capabilities(fs:write)`. `kryos-doc`'s source has zero references to
# capabilities at all -- there is no flag to opt in, either.
#
# This compounds the already-documented `kryos audit` gap (LEDGER item 13,
# `tests/security/audit_blind_to_capability_violations.sh`): audit is blind
# to VIOLATIONS it should catch but does not; doc is blind to declared
# annotations it easily COULD show (they're right there in the AST it
# already walks to build the function index). For a language whose entire
# pitch is that capabilities are a visible, auditable part of a function's
# contract, the one first-party tool a reviewer would actually run to
# understand a third-party package's API surface -- `kryos doc` -- gives
# zero indication of which functions touch the filesystem, network, process,
# or crypto. A reviewer reading generated docs for `save_report(path, text)`
# has no way to know it requires `fs:write` without reading the source
# directly, defeating the entire point of generating docs in the first
# place for a capability-safe language.
#
# Classify by grepping the generated markdown for the capability family name
# ("fs:write") that IS present in the source and IS shown by `kryos audit`
# on the same file, not by exit code (doc always exits 0 here; it never
# fails, it just omits the information).

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KRYOS="${KRYOS:-$REPO_ROOT/compiler/target/release/kryos}"
if [[ ! -x "$KRYOS" && -x "$KRYOS.exe" ]]; then KRYOS="$KRYOS.exe"; fi
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$REPO_ROOT/compiler/stdlib}"

WORK_DIR="$(mktemp -d)"
cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

cat > "$WORK_DIR/gated.kry" <<'EOF'
/// Save a report to disk. Requires filesystem write access.
@capabilities(fs:write)
fn save_report(path: str, text: str) {
    file_write(path, text)
}

fn main() {
    println("noop")
}
EOF

echo "=== kryos audit: capability inventory on this file ==="
AUDIT_OUT="$("$KRYOS" audit "$WORK_DIR/gated.kry" 2>&1)"
echo "$AUDIT_OUT"
AUDIT_SHOWS_CAP=0
echo "$AUDIT_OUT" | grep -q "fs:write" && AUDIT_SHOWS_CAP=1

echo
echo "=== kryos doc: generated markdown for the SAME file ==="
DOC_OUT="$("$KRYOS" doc "$WORK_DIR/gated.kry" 2>&1)"
echo "$DOC_OUT"
DOC_SHOWS_CAP=0
echo "$DOC_OUT" | grep -qi "capab\|fs:write" && DOC_SHOWS_CAP=1
DOC_SHOWS_FN=0
echo "$DOC_OUT" | grep -q "save_report" && DOC_SHOWS_FN=1

echo
if [[ "$AUDIT_SHOWS_CAP" -eq 1 && "$DOC_SHOWS_FN" -eq 1 && "$DOC_SHOWS_CAP" -eq 0 ]]; then
    echo "CONFIRMED: kryos audit correctly inventories the fs:write"
    echo "annotation, and kryos doc documents the SAME function (it appears"
    echo "in the function index/signature) but never mentions its capability"
    echo "requirement anywhere in the generated output."
    exit 0
else
    echo "NOT REPRODUCED -- audit_shows_cap=$AUDIT_SHOWS_CAP"
    echo "doc_shows_fn=$DOC_SHOWS_FN doc_shows_cap=$DOC_SHOWS_CAP -- this"
    echo "would mean kryos doc now surfaces capability annotations."
    exit 1
fi
