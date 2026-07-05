#!/usr/bin/env bash
# ecosystem_check.sh -- typecheck every ecosystem/ and packages/ .kry source.
# Catches lib rot that has no runnable artifact (a 2026-07-02 sweep found 7
# real failures in shipped sources that nothing executed). Runs from repo
# root; each file is checked from ITS package root so module resolution
# matches real usage. Exit 0 only if every non-excluded file checks clean.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
[ -x "$KRYOS" ] || { echo "kryos binary not found (set KRYOS_BIN)"; exit 1; }

# Intentional negative fixtures: these FAIL capability checks BY DESIGN
# (they exist to prove the compiler rejects them). Keep this list explicit.
EXCLUDE='kryos-capi/fixtures/mislabel_compute.kry
kryos-llm-router/fixtures/leaky_route.kry
kryos-resilient-llm/fixtures/leaky_io.kry
kryos-embed/agent/neg_control.kry'

is_excluded() {
  local f="$1"
  while IFS= read -r pat; do
    [ -n "$pat" ] && case "$f" in *"$pat") return 0 ;; esac
  done <<< "$EXCLUDE"
  return 1
}

total=0; failed=0
for pkg in ecosystem/*/ packages/*/; do
  [ -d "$pkg" ] || continue
  while IFS= read -r f; do
    rel="${f#"$pkg"}"
    is_excluded "$f" && continue
    total=$((total+1))
    # This gate verifies the ecosystem packages TYPE-CHECK (compile), not their
    # capability hygiene, so it pins --capabilities-mode=permissive rather than
    # the compiler default (now inferred). Migrating the ~132 package entry
    # points to declare their capabilities is tracked separately in
    # docs/capability-roadmap.md.
    if ! ( cd "$pkg" && timeout 25 "$KRYOS" check --capabilities-mode=permissive "$rel" >/dev/null 2>/tmp/eco_ck.txt ); then
      echo "  FAIL  $f"
      grep -m1 "error" /tmp/eco_ck.txt | head -c 160; echo ""
      failed=$((failed+1))
    fi
  done < <(find "$pkg" -name "*.kry" -not -path "*/node_modules/*")
done
echo "ecosystem-check: $((total-failed))/$total clean ($failed failed, 4 negative fixtures excluded by design)"
[ "$failed" -eq 0 ]
