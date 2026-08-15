#!/usr/bin/env bash
# authority_surface_gate.sh -- EVERY authority-granting builtin must be gated.
#
# WHY THIS IS THE SOUNDNESS-RELEVANT GATE. `tests/security_gate.sh` and
# `tools/loop/escape_status.sh` verify that known ATTACK SHAPES are rejected --
# they answer "can this particular laundering route defeat enforcement?". They
# do not answer the prior question: "is the set of primitives that GRANT
# authority gated at all?" A single ungated builtin makes every downstream shape
# check moot, because a program could just call it directly.
#
# The capability model is sound only if authority is unforgeable, and authority
# enters a Kryos program in exactly one place: a builtin the compiler classifies
# as requiring a capability. This gate enumerates that classification table
# straight from the compiler source and checks BOTH directions for every entry:
#
#   1. SOUNDNESS -- referenced with NO declared capability, it must be REJECTED
#                   with E0505. An acceptance is a real hole: unforgeable
#                   authority was forged.
#   2. USABILITY -- referenced WITH exactly the capability the compiler named in
#                   its own diagnostic, the E0505 must be GONE. Otherwise the
#                   capability cannot actually be granted and the gate is
#                   unusable rather than merely strict.
#
# Probe shape is a BY-NAME value reference (`let f = file_read`), not a call:
# uniform across every builtin regardless of arity or argument types, so an
# arity/type error can never mask an ungated builtin into a false PASS.
# CLAUDE.md documents by-name references as capability-tracked.
#
# Expectations are DERIVED from the compiler's own diagnostics rather than from
# a hardcoded builtin->capability table, which would rot silently the moment a
# builtin was reclassified -- the exact failure mode this repo keeps finding in
# its own documentation.
#
# TWO CALIBRATION LESSONS, both from earlier drafts of THIS gate being wrong:
#   - Several gated builtins live in a std module and are NOT in global scope
#     without an import (`path_exists`, `http_get`, `term_size`, ...). Bare
#     references to those emit BOTH E0102 (not in scope) AND E0505. Direction 2
#     must therefore test "no E0505 remains", NOT "rc=0" -- requiring rc=0
#     misreported 18 correctly-gated builtins as ungrantable.
#   - `required_capability_for_path` maps std MODULE names ("net", "io", "ffi")
#     to capabilities. Those are not builtins; including them polluted the list
#     with capability names that trivially fail to resolve.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

python3 - > "$TMP/builtins.txt" <<'PYEOF'
import re
src = 'compiler/crates/kryos-capabilities/src/model.rs'
s = open(src, encoding='utf-8', errors='replace').read()
# Cut the module-path table out: it maps std MODULE names, not builtins.
cut = s.index('pub fn required_capability_for_path') if 'pub fn required_capability_for_path' in s else len(s)
body = s[:cut]
pat = re.compile(r'((?:"[a-z0-9_]+"\s*\|\s*)*"[a-z0-9_]+")\s*=>\s*(?:Some\()?Capability::([A-Za-z]+)')
names = []
for m in pat.finditer(body):
    names += re.findall(r'"([a-z0-9_]+)"', m.group(1))
seen = []
for n in names:
    if n not in seen:
        seen.append(n)
print('\n'.join(seen))
PYEOF

count=0; ungated=0; ungrantable=0; gated=0; unnamed=0
ungated_names=""; ungrantable_names=""

while IFS= read -r b; do
    [ -z "$b" ] && continue
    count=$((count + 1))

    # --- direction 1: no capability declared -> must be REJECTED ---
    printf 'fn main() {\n    let f = %s\n    println("x")\n}\n' "$b" > "$TMP/n.kry"
    if out="$(timeout 60 "$K" check "$TMP/n.kry" 2>&1)"; then
        ungated=$((ungated + 1)); ungated_names="$ungated_names $b"
        echo "  UNGATED     $b -- compiled with NO declared capability"
        continue
    fi
    clean="$(printf '%s' "$out" | sed 's/\x1b\[[0-9;]*m//g')"
    cap="$(printf '%s' "$clean" | grep -oE 'requires `[a-z:]+` capability' | head -1 \
           | sed 's/requires `//; s/` capability//')"
    if [ -z "$cap" ]; then
        unnamed=$((unnamed + 1))
        echo "  note        $b -- rejected, but no capability named (not counted as gated)"
        continue
    fi
    gated=$((gated + 1))

    # --- direction 2: with that capability, the E0505 must be GONE ---
    printf '@capabilities(%s)\nfn main() {\n    let f = %s\n    println("x")\n}\n' "$cap" "$b" > "$TMP/y.kry"
    y="$(timeout 60 "$K" check "$TMP/y.kry" 2>&1 | sed 's/\x1b\[[0-9;]*m//g')"
    if printf '%s' "$y" | grep -q 'E0505'; then
        ungrantable=$((ungrantable + 1)); ungrantable_names="$ungrantable_names $b($cap)"
        echo "  UNGRANTABLE $b -- E0505 persists even WITH @capabilities($cap)"
    fi
done < "$TMP/builtins.txt"

echo
echo "  authority-granting builtins checked: $count"
echo "  gated (E0505 without the cap):       $gated"
echo "  rejected without naming a cap:       $unnamed"
if [ "$ungated" -eq 0 ] && [ "$ungrantable" -eq 0 ]; then
    echo "authority-surface: PASS -- 0 ungated, 0 ungrantable"
    exit 0
fi
[ "$ungated" -ne 0 ]     && echo "authority-surface: FAIL -- $ungated UNGATED --$ungated_names"
[ "$ungrantable" -ne 0 ] && echo "authority-surface: FAIL -- $ungrantable UNGRANTABLE --$ungrantable_names"
exit 1
