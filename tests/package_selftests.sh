#!/usr/bin/env bash
# package_selftests.sh -- execute every packages/*/src/selftest.kry and
# require its PASS line. Complements ecosystem_check.sh (which only
# typechecks): these actually run, so a packages regression that still
# typechecks is caught here.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KRYOS="${KRYOS:-$ROOT/compiler/target/release/kryos}"
if [ ! -x "$KRYOS" ] && [ -x "$KRYOS.exe" ]; then KRYOS="$KRYOS.exe"; fi

total=0
failed=0
for st in "$ROOT"/packages/*/src/selftest.kry; do
    [ -e "$st" ] || continue
    total=$((total + 1))
    name="$(basename "$(dirname "$(dirname "$st")")")"
    out="$("$KRYOS" run "$st" 2>&1)"
    if printf '%s' "$out" | grep -q "^PASS"; then
        echo "PASS $name"
    else
        failed=$((failed + 1))
        echo "FAIL $name"
        printf '%s\n' "$out" | head -10
    fi
done

echo "package-selftests: $((total - failed))/$total passed"
[ "$failed" -eq 0 ]
