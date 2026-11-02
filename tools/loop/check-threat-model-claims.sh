#!/usr/bin/env bash
# check-threat-model-claims.sh -- re-execute the verdicts docs/THREAT-MODEL.md
# states, on the in-place binary, from tests/threat_model/claims.tsv.
#
# WHY: every sentence in the threat model is tagged by how it was established.
# A sentence tagged "executed" is only as good as the commit it was executed at;
# when the compiler moves, the sentence silently goes stale (on 2026-11-02 five
# of the eight "zero-authority bypasses" documented the day before had been
# fixed). Each line of the manifest is a small program plus the verdict the
# document claims for it. This script runs every line and fails on any mismatch,
# so those sentences are tagged "gate" and cost one command to re-establish.
#
# Order of work:
#   1. resolve the binary exactly like check-escapes-documented.sh;
#   2. CANARY: a pure program must `kryos run` and print its marker, and a direct
#      file_read in deny!(fs:read) must be rejected by `kryos check`. A toolchain
#      that cannot do both (for example a kryos.exe without kryos_rt.lib beside
#      it) would make every "accepted" claim pass and every "rejected" claim
#      fail for the wrong reason, so the script refuses to run;
#   3. run each manifest line, print one ok/FAIL line, exit 1 on any FAIL.
#
# A claim here is a verdict on one program in one mode. It does not prove that
# no other program behaves differently: that is the whole point of the escape
# corpus and of section 4 of the document.
#
# Usage: check-threat-model-claims.sh
#        KRYOS_BIN=<path> check-threat-model-claims.sh
# Test-only flag (graph acceptance never passes it):
#        --manifest <file>   run another manifest (paths stay relative to the
#                            repo root); used to show that a flipped expectation fails
set -u

if ! command -v timeout >/dev/null 2>&1; then
  echo "check-threat-model-claims: FAIL -- coreutils \`timeout\` not found on PATH"
  exit 1
fi

MANIFEST_ARG=""
while [ $# -gt 0 ]; do
  case "$1" in
    --manifest) MANIFEST_ARG="${2:-}"; shift 2 || { echo "FAIL: --manifest needs a file"; exit 1; } ;;
    *) echo "FAIL: unknown argument: $1"; exit 1 ;;
  esac
done

ORIG_PWD="$(pwd)"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MANIFEST="${MANIFEST_ARG:-$ROOT/tests/threat_model/claims.tsv}"
# MIN_CLAIMS is a floor, like MIN_ROWS in check-escapes-documented.sh: claims are
# only ever added, so a lower count means one was deleted to make the check pass.
MIN_CLAIMS=73

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
# KRYOS_CACHE / KRYOS_CACHE_DIR would let a build skip the checker (an old
# bypass) and KRYOS_STDLIB_DIR would change which directory is trusted.
unset KRYOS_CACHE KRYOS_CACHE_DIR KRYOS_STDLIB_DIR

K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
if [ -z "${KRYOS_BIN:-}" ] && [ ! -x "$K" ]; then K="$ROOT/compiler/target/release/kryos.exe"; fi
case "$K" in /*|[A-Za-z]:*) ;; *) K="$ORIG_PWD/$K" ;; esac
if [ ! -f "$K" ] || [ ! -x "$K" ]; then
  echo "check-threat-model-claims: FAIL -- compiler not found or not executable: $K"
  echo "  set KRYOS_BIN, or build compiler/target/release/kryos[.exe]"
  exit 1
fi
echo "  ok   binary: $K"

# ------------------------------------------------------------------- canary
# Same two probes as check-escapes-documented.sh. Without them a kryos.exe that
# cannot link (no kryos_rt.lib / kryos_stdlib_native.lib beside it) would still
# let every `check` claim pass or fail on its own, and the `run` claims whose
# expectation is just "nonzero" would pass for the wrong reason.
cat > "$TMP/canary_pure.kry" <<'KRY'
fn main() {
    println("KRYOS-CANARY-OK-7731")
}
KRY
cat > "$TMP/canary_deny.kry" <<'KRY'
@capabilities(fs:read)
fn main() {
    deny!(fs:read) {
        let s = file_read("canary.txt")
        println(s)
    }
}
KRY
out="$(timeout 180 "$K" run "$TMP/canary_pure.kry" 2>&1)"; rc=$?
if [ "$rc" -eq 0 ] && printf '%s
' "$out" | tr -d '' | grep -qx "KRYOS-CANARY-OK-7731"; then
  echo "  ok   canary: a pure program builds, runs and prints its marker"
else
  echo "check-threat-model-claims: FAIL -- canary: \`kryos run\` of a pure program did not exit 0 and print its marker (rc=$rc)."
  echo "  The toolchain cannot build and run programs (are kryos_rt.lib and kryos_stdlib_native.lib beside the binary?),"
  echo "  so the claims would be checked against a toolchain that cannot back them."
  printf '%s
' "$out" | tail -5 | sed 's/^/    /'
  exit 1
fi
out="$(timeout 120 "$K" check "$TMP/canary_deny.kry" 2>&1)"; rc=$?
if [ "$rc" -ne 0 ] && printf '%s
' "$out" | grep -qE "E0110|E0505|E0507"; then
  echo "  ok   canary: file_read inside deny!(fs:read) is rejected by \`kryos check\`"
else
  echo "check-threat-model-claims: FAIL -- canary: \`kryos check\` did not reject file_read inside deny!(fs:read) (rc=$rc)."
  echo "  The capability checker is not rejecting a direct call, so the claims would be vacuous."
  printf '%s
' "$out" | tail -5 | sed 's/^/    /'
  exit 1
fi

strip_ansi() { sed 's/\x1b\[[0-9;]*m//g' | tr -d '\r'; }
# A checkout with core.autocrlf gives the manifest CRLF line ends; drop the CRs.
tr -d '\r' < "$MANIFEST" > "$TMP/manifest.tsv"
WT="$(cygpath -m "$TMP" 2>/dev/null || printf '%s' "$TMP")"
fails=0
n=0
seen=" "
while IFS=$'\t' read -r id cmd prog expect contains backs; do
  case "$id" in ''|'#'*) continue ;; esac
  n=$((n + 1))
  if [ -z "$cmd" ] || [ -z "$prog" ] || [ -z "$expect" ] || [ -z "$contains" ] || [ -z "$backs" ]; then
    echo "  FAIL $id: malformed manifest line (need id, cmd, program, expect, contains, backs)"; fails=$((fails + 1)); continue
  fi
  case "$seen" in *" $id "*) echo "  FAIL $id: duplicate claim id"; fails=$((fails + 1)); continue ;; esac
  seen="$seen$id "
  case "$expect" in 0|nz) ;; *) echo "  FAIL $id: expect must be 0 or nz (got \"$expect\")"; fails=$((fails + 1)); continue ;; esac
  cmd="${cmd//\{tmp\}/$WT}"
  # shellcheck disable=SC2206
  words=($cmd)
  case "$prog" in
    @dir:*)
      d="$ROOT/${prog#@dir:}"
      if [ ! -d "$d" ]; then echo "  FAIL $id: directory ${prog#@dir:} does not exist"; fails=$((fails + 1)); continue; fi
      (cd "$d" && timeout 240 "$K" "${words[@]}") > "$TMP/o.raw" 2>&1; rc=$?; out="$(strip_ansi < "$TMP/o.raw")" ;;
    *)
      if [ ! -f "$ROOT/$prog" ]; then echo "  FAIL $id: program $prog does not exist"; fails=$((fails + 1)); continue; fi
      (cd "$ROOT" && timeout 240 "$K" "${words[@]}" "$prog") > "$TMP/o.raw" 2>&1; rc=$?; out="$(strip_ansi < "$TMP/o.raw")" ;;
  esac
  # 124 is `timeout`; 101 and 139/134 are crashes. None of them is a verdict.
  if [ "$rc" -eq 124 ] || [ "$rc" -ge 100 ]; then
    echo "  FAIL $id: \`kryos $cmd $prog\` exited $rc (timeout or crash is not a verdict)"; fails=$((fails + 1)); continue
  fi
  ok_rc=1
  if [ "$expect" = "0" ] && [ "$rc" -ne 0 ]; then ok_rc=0; fi
  if [ "$expect" = "nz" ] && [ "$rc" -eq 0 ]; then ok_rc=0; fi
  ok_sub=1
  if [ "$contains" != "-" ] && ! printf '%s\n' "$out" | grep -q -F -e "$contains"; then ok_sub=0; fi
  if [ "$ok_rc" -eq 1 ] && [ "$ok_sub" -eq 1 ]; then
    echo "  ok   $id (rc=$rc, expected $expect, contains $contains)"
  else
    echo "  FAIL $id: \`kryos $cmd $prog\` rc=$rc (expected $expect), output contains \"$contains\": $([ "$ok_sub" -eq 1 ] && echo yes || echo NO)   [$backs]"
    printf '%s\n' "$out" | tail -4 | sed 's/^/       /'
    fails=$((fails + 1))
  fi
done < "$TMP/manifest.tsv"

if [ "$n" -eq 0 ]; then
  echo "check-threat-model-claims: FAIL -- no claims parsed from $MANIFEST"; exit 1
fi
if [ -z "$MANIFEST_ARG" ] && [ "$n" -lt "$MIN_CLAIMS" ]; then
  echo "  FAIL manifest has $n claims but at least $MIN_CLAIMS are required (MIN_CLAIMS): claims were removed"; fails=$((fails + 1))
fi

echo
echo "claims: $n, failed: $fails"
if [ "$fails" -eq 0 ]; then
  echo "check-threat-model-claims: PASS"
  exit 0
fi
echo "check-threat-model-claims: FAIL ($fails problem(s))"
exit 1
