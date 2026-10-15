#!/usr/bin/env bash
# repl_capability_gate.sh -- `kryos repl` holds only the capabilities a session
# was granted.
#
# WHY: the REPL compiled each input into an unannotated wrapper function,
# which inferred mode treats as an interior helper and never checks, so
# file_write / env_get ran at the prompt (bare, or inside any fn not named
# `main`) with no capability anywhere (tests/security/
# repl_toplevel_capability_bypass.sh). The wrapper is now annotated with the
# session grant: none by default, `--allow <caps>` or `:allow <caps>` to add.
#
# Every rejection check has a positive control, so the gate fails if the REPL
# simply stops running code.
set -uo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KRYOS="${KRYOS:-$REPO/compiler/target/release/kryos}"
[[ -x "$KRYOS" ]] || KRYOS="$KRYOS.exe"
[[ -x "$KRYOS" ]] || { echo "repl-capability: build the compiler first"; exit 2; }
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$REPO/compiler/stdlib}"

W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
WP="$W"
command -v cygpath >/dev/null 2>&1 && WP="$(cygpath -m "$W")"
T="$WP/out.txt"
fail=0
ok()  { echo "  ok   $1"; }
bad() { echo "  FAIL $1"; fail=1; }
repl() { timeout 60 "$KRYOS" repl "$@" 2>&1; }

# 1. no grant: a bare gated call is rejected and has no effect
out="$(printf 'file_write("%s", "x")\n:quit\n' "$T" | repl)"
if [[ ! -f "$W/out.txt" ]] && grep -q "E0505" <<<"$out"; then ok "bare file_write rejected (E0505), nothing written"
else bad "bare file_write ran or was not rejected"; fi
rm -f "$W/out.txt"

# 2. no grant: the call inside a fn with any name is rejected too
out="$(printf 'fn leak() {\n    file_write("%s", "y")\n}\nleak()\n:quit\n' "$T" | repl)"
if [[ ! -f "$W/out.txt" ]] && grep -qE "E0505|E0507" <<<"$out"; then ok "file_write inside fn leak() rejected"
else bad "file_write inside fn leak() ran or was not rejected"; fi
rm -f "$W/out.txt"

# 3. no grant: env_get (process) is rejected
# The marker is split in the source ("E" + "NV=") so a diagnostic that quotes
# the source line can never match the runtime output.
ENVPROG='println("E" + "NV=" + env_get("PATH"))'
out="$(printf '%s\n:quit\n' "$ENVPROG" | repl)"
if ! grep -q "^ENV=." <<<"$out" && grep -q "E0505" <<<"$out"; then ok "env_get rejected without process"
else bad "env_get read the environment with no grant"; fi

# 4. control: ungated code still runs with no grant
out="$(printf 'println(40 + 2)\n:quit\n' | repl)"
if grep -qx "42" <<<"$(tr -d '\r' <<<"$out")"; then ok "control: ungated expression runs"
else bad "control: the REPL did not evaluate 40 + 2"; fi

# 5. control: the --allow FLAG grants, and grants only what it names. Checked
#    on stdout (env_get under `process`) rather than through the filesystem,
#    so the check does not depend on the platform's temp-dir behaviour; the
#    file-writing variant follows it and prints the REPL's own output when it
#    disagrees, which is how a platform difference becomes diagnosable
#    instead of just red.
out="$(printf '%s\n:quit\n' "$ENVPROG" | repl --allow process)"
if grep -q "^ENV=." <<<"$out"; then ok "control: --allow process lets env_get run"
else bad "control: --allow process did not grant env_get"; printf '%s\n' "$out" | sed 's/^/      /' | head -6; fi

out="$(printf 'file_write("%s", "granted")\n:quit\n' "$T" | repl --allow fs:write)"
if [[ "$(cat "$W/out.txt" 2>/dev/null)" == "granted" ]]; then ok "control: --allow fs:write lets file_write run"
else bad "control: --allow fs:write did not grant file_write (target $T)"; printf '%s\n' "$out" | sed 's/^/      /' | head -8; fi
rm -f "$W/out.txt"
out="$(printf '%s\n:quit\n' "$ENVPROG" | repl --allow fs:write)"
if ! grep -q "^ENV=." <<<"$out"; then ok "--allow fs:write does not grant process"
else bad "--allow fs:write also granted env_get"; fi

# 6. control: :allow at the prompt grants for the rest of the session
out="$(printf '%s\n:allow process\n%s\n:quit\n' "$ENVPROG" "$ENVPROG" | repl)"
if [[ "$(grep -c "^ENV=." <<<"$out")" == "1" ]]; then ok ":allow process grants env_get from then on"
else bad ":allow process did not grant exactly the later call"; fi

# 7. an unknown name is refused, not silently "granted"
out="$(printf ':allow fs:raed\n:caps\n:quit\n' | repl)"
if grep -q "W-CAP-UNKNOWN" <<<"$out" && grep -q "capabilities: none" <<<"$out"; then ok ":allow refuses an unknown capability name"
else bad ":allow accepted an unknown capability name"; fi

[[ $fail -eq 0 ]] && echo "repl-capability: PASS" || echo "repl-capability: FAIL"
exit $fail
