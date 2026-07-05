#!/usr/bin/env bash
# strict_caps_examples.sh -- every public example/showcase program must pass
# `kryos check --strict-capabilities`. This makes the examples EXEMPLARY: they
# demonstrate correct, least-privilege @capabilities declarations, and it keeps
# the capability system honest -- the language's central pitch is enforced on
# its own demos.
#
# Excludes: pure library files with no entry point, and the deliberate
# negative-control fixtures elsewhere (this only covers examples/ + showcase).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
taskkill //F //IM kryostokens.exe >/dev/null 2>&1 || true

total=0; failed=0
check() {
  local f="$1"
  total=$((total+1))
  if ! timeout 30 "$KRYOS" check --strict-capabilities "$f" >/tmp/sc_out.txt 2>&1; then
    echo "  FAIL  $f"
    grep -m3 -oE "builtin \`[a-z_0-9]+\` requires \`[a-z:]+\`" /tmp/sc_out.txt | sed 's/^/    /'
    failed=$((failed+1))
  fi
}

for f in examples/*.kry examples/showcase/*.kry examples/showcase/extra/*.kry; do
  [ -f "$f" ] || continue
  # Library-style files with no `fn main` are skipped (nothing to gate at an
  # entry point); the capability checker still runs on them during normal builds.
  grep -q "fn main" "$f" || continue
  check "$f"
done

echo "strict-caps examples: $((total-failed))/$total pass"
[ "$failed" -eq 0 ]
