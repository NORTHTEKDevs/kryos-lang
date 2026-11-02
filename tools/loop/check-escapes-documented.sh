#!/usr/bin/env bash
# check-escapes-documented.sh -- every known capability escape is documented
# with a committed repro, and nothing documented has been fixed.
#
# Acceptance for graph nodes `stage2-rows` and `escape-root` once they were
# converted to documented-limitation nodes (tools/loop/GRAPH.md, failure
# policy). The old acceptance, `escape_status.sh | grep -q "STILL ESCAPING: 0"`,
# could pass on a broken toolchain: a kryos.exe without kryos_rt.lib beside it
# cannot link, no run prints the leak marker, so every row counted as "fixed".
# Measured 2026-11-01: "STILL ESCAPING: 0 now-rejected: 99" on a toolchain that
# could not run a single program.
#
# What this does, in order:
#   1. resolve the binary exactly like escape_status.sh; FAIL if it is missing;
#   2. CANARY: a pure program must `kryos run` and print its marker, and a
#      program that calls file_read inside deny!(fs:read) must be rejected by
#      `kryos check`; either failing means the measurement would be vacuous;
#   3. run escape_status.sh and parse it (the STILL ESCAPING line must exist,
#      missing must be 0, every accepted row must show the leak);
#   4. compare the accepted rows with the table between the known-escapes
#      markers in docs/THREAT-MODEL.md, id for id and path for path.
# Exit 0 only when documented == accepted, with nothing stale or undocumented.
#
# Usage: check-escapes-documented.sh
#        KRYOS_BIN=<path> check-escapes-documented.sh
# Test-only flags (graph acceptance never passes them):
#        --from-output <file>    parse a saved escape_status.sh output; skips
#                                the binary check and the canary
#        --threat-model <file>   read the table from another file
set -u

FROM_OUTPUT=""
TM_ARG=""
while [ $# -gt 0 ]; do
  case "$1" in
    --from-output) FROM_OUTPUT="${2:-}"; shift 2 || { echo "FAIL: --from-output needs a file"; exit 1; } ;;
    --threat-model) TM_ARG="${2:-}"; shift 2 || { echo "FAIL: --threat-model needs a file"; exit 1; } ;;
    *) echo "FAIL: unknown argument: $1"; exit 1 ;;
  esac
done

ORIG_PWD="$(pwd)"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TM="${TM_ARG:-$ROOT/docs/THREAT-MODEL.md}"

fails=0
fail() { echo "  FAIL: $*"; fails=$((fails + 1)); }
ok() { echo "  ok   $*"; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# ------------------------------------------------------------------ binary
K=""
if [ -z "$FROM_OUTPUT" ]; then
  K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
  if [ -z "${KRYOS_BIN:-}" ] && [ ! -x "$K" ]; then K="$ROOT/compiler/target/release/kryos.exe"; fi
  case "$K" in /*|[A-Za-z]:*) ;; *) K="$ORIG_PWD/$K" ;; esac
  if [ ! -f "$K" ] || [ ! -x "$K" ]; then
    echo "check-escapes-documented: FAIL -- compiler not found or not executable: $K"
    echo "  set KRYOS_BIN, or build compiler/target/release/kryos[.exe]"
    exit 1
  fi
  ok "binary: $K"

  # ------------------------------------------------------------- canary
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
  if [ "$rc" -eq 0 ] && printf '%s\n' "$out" | tr -d '\r' | grep -qx "KRYOS-CANARY-OK-7731"; then
    ok "canary: a pure program builds, runs and prints its marker"
  else
    echo "check-escapes-documented: FAIL -- canary: \`kryos run\` of a pure program did not exit 0 and print its marker (rc=$rc)."
    echo "  The toolchain cannot build and run programs (is kryos_rt.lib beside the binary?),"
    echo "  so the escape measurement would be vacuous: every row would count as fixed."
    printf '%s\n' "$out" | tail -5 | sed 's/^/    /'
    exit 1
  fi
  out="$(timeout 120 "$K" check "$TMP/canary_deny.kry" 2>&1)"; rc=$?
  if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -qE "E0110|E0505|E0507"; then
    ok "canary: file_read inside deny!(fs:read) is rejected by \`kryos check\`"
  else
    echo "check-escapes-documented: FAIL -- canary: \`kryos check\` did not reject file_read inside deny!(fs:read) (rc=$rc)."
    echo "  The capability checker is not rejecting a direct call, so the escape measurement would be vacuous."
    printf '%s\n' "$out" | tail -5 | sed 's/^/    /'
    exit 1
  fi
fi

# --------------------------------------------------------- escape_status
ES="$TMP/escape_status.out"
if [ -n "$FROM_OUTPUT" ]; then
  echo "  !!!! TEST-ONLY: parsing a saved escape_status.sh output ($FROM_OUTPUT)."
  echo "  !!!! No binary was checked and no canary ran. Graph acceptance never passes --from-output."
  if [ ! -f "$FROM_OUTPUT" ]; then
    echo "check-escapes-documented: FAIL -- --from-output file not found: $FROM_OUTPUT"; exit 1
  fi
  tr -d '\r' < "$FROM_OUTPUT" > "$ES"
else
  KRYOS_BIN="$K" bash "$ROOT/tools/loop/escape_status.sh" > "$ES.raw" 2>&1
  es_rc=$?
  tr -d '\r' < "$ES.raw" > "$ES"
  if [ "$es_rc" -ne 0 ]; then
    echo "check-escapes-documented: FAIL -- escape_status.sh exited $es_rc"
    tail -5 "$ES" | sed 's/^/    /'
    exit 1
  fi
fi

summary="$(grep -E '^STILL ESCAPING: [0-9]+ +now-rejected: [0-9]+ +missing: [0-9]+ *$' "$ES" | tail -1)"
if [ -z "$summary" ]; then
  echo "check-escapes-documented: FAIL -- escape_status.sh output has no \"STILL ESCAPING: N now-rejected: N missing: N\" line"
  tail -5 "$ES" | sed 's/^/    /'
  exit 1
fi
s_esc="$(printf '%s\n' "$summary" | sed -E 's/^STILL ESCAPING: ([0-9]+) .*/\1/')"
s_rej="$(printf '%s\n' "$summary" | sed -E 's/.*now-rejected: ([0-9]+) .*/\1/')"
s_mis="$(printf '%s\n' "$summary" | sed -E 's/.*missing: ([0-9]+) *$/\1/')"
echo "  measured: $summary"
if [ "$s_mis" -ne 0 ]; then
  fail "escape_status.sh reports $s_mis missing repro file(s); a row without a file measures nothing"
fi

# Rows: "<id> <name> rc=<check> rc=<strict> <LEAK|no> <ESCAPES|fixed>"
# Emits: id<TAB>name<TAB>check_rc<TAB>leak<TAB>verdict
awk '
  NF >= 6 && $3 ~ /^rc=/ && $4 ~ /^rc=/ {
    c = $3; sub(/^rc=/, "", c)
    print $1 "\t" $2 "\t" c "\t" $5 "\t" $NF
  }
' "$ES" > "$TMP/rows.tsv"

rows_n="$(wc -l < "$TMP/rows.tsv" | tr -d ' ')"
if [ "$rows_n" -eq 0 ]; then
  fail "no result rows parsed from escape_status.sh output"
fi
if [ "$rows_n" -ne $((s_esc + s_rej)) ]; then
  fail "parsed $rows_n rows but the summary counts $((s_esc + s_rej)) (escaping $s_esc + rejected $s_rej)"
fi

# Empty associative arrays are "unbound" to `set -u` in bash 4/5, and an empty
# table or zero accepted rows is a valid state, so nounset is off from here on.
set +u
declare -A ROW_NAME ROW_C1 ROW_VERDICT ACCEPTED
n_esc_rows=0
dup_rows=""
while IFS=$'\t' read -r id name c1 leak verdict; do
  if [ -n "${ROW_NAME[$id]:-}" ]; then dup_rows="$dup_rows $id"; fi
  ROW_NAME[$id]="$name"; ROW_C1[$id]="$c1"; ROW_VERDICT[$id]="$verdict"
  case "$c1" in
    0) ACCEPTED[$id]=1
       if [ "$verdict" != "ESCAPES" ]; then
         fail "row $id ($name) is accepted by \`kryos check\` but the leak did not reproduce (leak=$leak verdict=$verdict): the run probably failed; investigate"
       else
         n_esc_rows=$((n_esc_rows + 1))
       fi ;;
    1) if [ "$verdict" = "ESCAPES" ]; then fail "row $id verdict ESCAPES but check rc=1 (inconsistent output)"; fi ;;
    *) fail "row $id ($name): \`kryos check\` exited $c1 (not 0 or 1): a timeout or crash is not a rejection" ;;
  esac
done < "$TMP/rows.tsv"
[ -n "$dup_rows" ] && fail "escape_status.sh printed duplicate row id(s):$dup_rows"
if [ "$n_esc_rows" -ne "$s_esc" ]; then
  fail "$n_esc_rows rows are accepted-and-leaking but the summary says STILL ESCAPING: $s_esc"
fi
accepted_n="${#ACCEPTED[@]}"

# -------------------------------------------------------------- the table
if [ ! -f "$TM" ]; then
  echo "check-escapes-documented: FAIL -- threat model not found: $TM"; exit 1
fi
tr -d '\r' < "$TM" > "$TMP/tm.md"
nb="$(grep -cF '<!-- known-escapes:begin -->' "$TMP/tm.md")"
ne="$(grep -cF '<!-- known-escapes:end -->' "$TMP/tm.md")"
if [ "$nb" -ne 1 ] || [ "$ne" -ne 1 ]; then
  echo "check-escapes-documented: FAIL -- $TM must contain exactly one known-escapes:begin and one known-escapes:end marker (found $nb / $ne)"
  exit 1
fi
bl="$(grep -nF '<!-- known-escapes:begin -->' "$TMP/tm.md" | cut -d: -f1)"
el="$(grep -nF '<!-- known-escapes:end -->' "$TMP/tm.md" | cut -d: -f1)"
if [ "$bl" -ge "$el" ]; then
  echo "check-escapes-documented: FAIL -- known-escapes:end comes before known-escapes:begin"; exit 1
fi

# Emits: id<TAB>repro<TAB>shape<TAB>root, or MALFORMED<TAB>line. Cells are
# trimmed of spaces and backticks; the header and the |---| separator are skipped.
sed -n "$((bl + 1)),$((el - 1))p" "$TMP/tm.md" | awk -F'|' '
  function trim(s) { gsub(/^[ \t`]+|[ \t`]+$/, "", s); return s }
  /^[ \t]*\|/ {
    line = $0
    n = NF
    # leading "|" gives an empty $1; a trailing "|" gives an empty $NF
    last = n; if (trim($n) == "" ) last = n - 1
    cells = last - 1
    id = trim($2)
    if (id == "id") next
    if (id ~ /^[-: ]*$/) next
    if (cells != 4 || trim($2) == "" || trim($3) == "" || trim($4) == "" || trim($5) == "") {
      print "MALFORMED\t" line; next
    }
    print trim($2) "\t" trim($3) "\t" trim($4) "\t" trim($5)
  }
' > "$TMP/doc.tsv"

declare -A DOC_REPRO DOC_SEEN
doc_n=0
while IFS=$'\t' read -r id repro shape root; do
  if [ "$id" = "MALFORMED" ]; then
    fail "malformed table row in $TM: $repro (need | id | repro | shape | root |)"
    continue
  fi
  doc_n=$((doc_n + 1))
  if [ -n "${DOC_SEEN[$id]:-}" ]; then
    fail "duplicate id $id in the known-escapes table"
    continue
  fi
  DOC_SEEN[$id]=1
  DOC_REPRO[$id]="$repro"
done < "$TMP/doc.tsv"

stale=0
undoc=0

# Every documented id must be an accepted row, with the right repro path.
for id in "${!DOC_REPRO[@]}"; do
  repro="${DOC_REPRO[$id]}"
  if [ -z "${ROW_NAME[$id]:-}" ]; then
    fail "documented id $id has no row in escape_status.sh (bogus entry: ids must be EXACTLY the escape_status row id)"
    stale=$((stale + 1)); continue
  fi
  if [ -z "${ACCEPTED[$id]:-}" ]; then
    fail "documented id $id is NOT accepted any more (check rc=${ROW_C1[$id]}): stale entry, it was fixed; remove it from the table"
    stale=$((stale + 1)); continue
  fi
  want="tests/security/${ROW_NAME[$id]}.kry"
  if [ "$repro" != "$want" ]; then
    fail "documented id $id: repro is \"$repro\" but the escape_status row is \"$want\""
  elif [ ! -f "$ROOT/$repro" ]; then
    fail "documented id $id: repro file $repro does not exist"
  fi
done

# Every accepted row must be documented.
for id in "${!ACCEPTED[@]}"; do
  if [ -z "${DOC_REPRO[$id]:-}" ]; then
    fail "accepted escape $id (${ROW_NAME[$id]}) is not in the known-escapes table: undocumented escape"
    undoc=$((undoc + 1))
  fi
done

echo
echo "documented escapes: $doc_n, accepted: $accepted_n, undocumented: $undoc, stale: $stale"
if [ "$fails" -eq 0 ]; then
  echo "check-escapes-documented: PASS"
  exit 0
fi
echo "check-escapes-documented: FAIL ($fails problem(s))"
exit 1
