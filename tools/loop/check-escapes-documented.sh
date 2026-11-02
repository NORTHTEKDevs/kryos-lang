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
#      markers in docs/THREAT-MODEL.md, id for id and path for path;
#   5. verify the second table (known-bypasses, section 4.5): zero-authority
#      shapes that are NOT rows of escape_status.sh. Each listed repro is
#      re-run (check, check --strict-capabilities, run or build) and must still
#      behave as documented; every tests/security/bypass_*.kry must be listed.
# Exit 0 only when documented == accepted, with nothing stale or undocumented.
# The corpus is a fixed list inside escape_status.sh: this check cannot find an
# escape nobody wrote down, and it refuses to run on fewer rows than MIN_ROWS.
#
# Usage: check-escapes-documented.sh
#        KRYOS_BIN=<path> check-escapes-documented.sh
# Test-only flags (graph acceptance never passes them):
#        --from-output <file>    parse a saved escape_status.sh output; skips
#                                the binary check and the canary
#        --threat-model <file>   read the table from another file
set -u

# Floor on the number of rows escape_status.sh must print. Rows are only ever
# added (99 at PR #38); a lower count means rows were deleted, which would let
# an escape disappear from the measurement and from the table together. Raise
# this when the corpus grows.
MIN_ROWS=99

if [ "${BASH_VERSINFO[0]:-0}" -lt 4 ]; then
  echo "check-escapes-documented: FAIL -- bash >= 4 is required (associative arrays); this is ${BASH_VERSION:-unknown}"
  exit 1
fi
if ! command -v timeout >/dev/null 2>&1; then
  echo "check-escapes-documented: FAIL -- coreutils \`timeout\` not found on PATH"
  exit 1
fi

FROM_OUTPUT=""
FROM_OUTPUT_SEEN=0
TM_ARG=""
while [ $# -gt 0 ]; do
  case "$1" in
    --from-output) FROM_OUTPUT_SEEN=1; FROM_OUTPUT="${2:-}"; shift 2 || { echo "FAIL: --from-output needs a file"; exit 1; } ;;
    --threat-model) TM_ARG="${2:-}"; shift 2 || { echo "FAIL: --threat-model needs a file"; exit 1; } ;;
    *) echo "FAIL: unknown argument: $1"; exit 1 ;;
  esac
done
if [ "$FROM_OUTPUT_SEEN" -eq 1 ] && [ -z "$FROM_OUTPUT" ]; then
  echo "FAIL: --from-output needs a non-empty file name"; exit 1
fi

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
if [ "$FROM_OUTPUT_SEEN" -eq 0 ]; then
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
if [ "$FROM_OUTPUT_SEEN" -eq 1 ]; then
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
if [ "$rows_n" -lt "$MIN_ROWS" ]; then
  fail "escape_status.sh printed $rows_n rows but at least $MIN_ROWS are required (MIN_ROWS): rows were removed from the measurement"
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
# table_rows <name>: print the rows of the table between the <name>:begin and
# <name>:end markers as id<TAB>col2<TAB>col3<TAB>col4, or MALFORMED<TAB>line.
# Cells are trimmed of spaces and backticks; the header and the |---| separator
# are skipped. Returns 1 when the markers are not exactly once each, in order.
table_rows() {
  local name="$1" nb ne bl el
  nb="$(grep -cF "<!-- $name:begin -->" "$TMP/tm.md")"
  ne="$(grep -cF "<!-- $name:end -->" "$TMP/tm.md")"
  if [ "$nb" -ne 1 ] || [ "$ne" -ne 1 ]; then
    echo "check-escapes-documented: FAIL -- $TM must contain exactly one $name:begin and one $name:end marker (found $nb / $ne)" >&2
    return 1
  fi
  bl="$(grep -nF "<!-- $name:begin -->" "$TMP/tm.md" | cut -d: -f1)"
  el="$(grep -nF "<!-- $name:end -->" "$TMP/tm.md" | cut -d: -f1)"
  if [ "$bl" -ge "$el" ]; then
    echo "check-escapes-documented: FAIL -- $name:end comes before $name:begin" >&2
    return 1
  fi
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
  '
}

table_rows known-escapes > "$TMP/doc.tsv" || exit 1

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

# ------------------------------------------------- known-bypasses (4.5)
# Zero-authority shapes that are not rows of escape_status.sh. Columns:
#   | id | repro | shape | verified by |      id = B<number>
# verified by:
#   runs           accepted by `check` and `check --strict-capabilities`, and
#                  `kryos run` prints the line BYPASS-<id>-OK
#   inferred-only  accepted by `check` (default) and rejected by
#                  `check --strict-capabilities`; not run
#   cache          the three-step build-cache sequence documented in the repro
table_rows known-bypasses > "$TMP/by.tsv" || exit 1
declare -A BY_REPRO BY_MODE BY_SEEN
by_n=0
by_ok=0
by_bad=0
while IFS=$'\t' read -r id repro shape mode; do
  if [ "$id" = "MALFORMED" ]; then
    fail "malformed known-bypasses row in $TM: $repro (need | id | repro | shape | verified by |)"; by_bad=$((by_bad + 1)); continue
  fi
  by_n=$((by_n + 1))
  if ! printf '%s' "$id" | grep -qE '^B[0-9]+$'; then fail "known-bypasses id \"$id\" must look like B1, B2, ..."; by_bad=$((by_bad + 1)); continue; fi
  if [ -n "${BY_SEEN[$id]:-}" ]; then fail "duplicate known-bypasses id $id"; by_bad=$((by_bad + 1)); continue; fi
  BY_SEEN[$id]=1
  case "$mode" in runs|inferred-only|cache) ;; *) fail "known-bypasses $id: \"verified by\" must be runs, inferred-only or cache (got \"$mode\")"; by_bad=$((by_bad + 1)); continue ;; esac
  case "$repro" in tests/security/bypass_*.kry) ;; *) fail "known-bypasses $id: repro \"$repro\" must be tests/security/bypass_*.kry"; by_bad=$((by_bad + 1)); continue ;; esac
  if [ ! -f "$ROOT/$repro" ]; then fail "known-bypasses $id: repro file $repro does not exist"; by_bad=$((by_bad + 1)); continue; fi
  BY_REPRO[$id]="$repro"; BY_MODE[$id]="$mode"
done < "$TMP/by.tsv"

# every committed tests/security/bypass_*.kry must be in the table
for f in "$ROOT"/tests/security/bypass_*.kry; do
  [ -f "$f" ] || continue
  rel="tests/security/$(basename "$f")"
  found=0
  for id in "${!BY_REPRO[@]}"; do [ "${BY_REPRO[$id]}" = "$rel" ] && found=1; done
  if [ "$found" -eq 0 ]; then fail "$rel is not listed in the known-bypasses table: undocumented bypass repro"; by_bad=$((by_bad + 1)); fi
done

if [ "$FROM_OUTPUT_SEEN" -eq 1 ]; then
  echo "  !!!! TEST-ONLY: known-bypasses rows were parsed but not executed (no binary)."
else
  WT="$(cygpath -m "$TMP" 2>/dev/null || printf '%s' "$TMP")"
  for id in "${!BY_REPRO[@]}"; do
    repro="${BY_REPRO[$id]}"; mode="${BY_MODE[$id]}"; f="$ROOT/$repro"
    case "$mode" in
      runs|inferred-only)
        timeout 120 "$K" check "$f" >/dev/null 2>&1; r1=$?
        timeout 120 "$K" check --strict-capabilities "$f" >/dev/null 2>&1; r2=$?
        if [ "$r1" -gt 1 ] || [ "$r2" -gt 1 ]; then
          fail "bypass $id: \`kryos check\` exited $r1 / $r2 (a timeout or crash is not a verdict)"; by_bad=$((by_bad + 1)); continue
        fi
        if [ "$mode" = "inferred-only" ]; then
          if [ "$r1" -eq 0 ] && [ "$r2" -eq 1 ]; then by_ok=$((by_ok + 1)); ok "bypass $id ($repro): accepted by default check, rejected by --strict-capabilities"
          else fail "bypass $id: expected check rc 0 (default) and 1 (strict), got $r1 / $r2: stale or changed entry; update the table"; by_bad=$((by_bad + 1)); fi
          continue
        fi
        if [ "$r1" -ne 0 ] || [ "$r2" -ne 0 ]; then
          fail "bypass $id: expected check rc 0 / 0 (default / strict), got $r1 / $r2: it was fixed or changed; remove or update the table row"; by_bad=$((by_bad + 1)); continue
        fi
        runout="$(timeout 180 "$K" run "$f" 2>&1 | tr -d '\r')"
        if printf '%s\n' "$runout" | grep -qx "BYPASS-$id-OK"; then
          by_ok=$((by_ok + 1)); ok "bypass $id ($repro): accepted in default and strict, run printed BYPASS-$id-OK"
        else
          fail "bypass $id: accepted by check but \`kryos run\` did not print BYPASS-$id-OK"
          printf '%s\n' "$runout" | tail -3 | sed 's/^/    /'; by_bad=$((by_bad + 1))
        fi ;;
      cache)
        cdir="$WT/bypass_cache"; mkdir -p "$cdir"
        o1="$(env -u KRYOS_CACHE KRYOS_CACHE_DIR="$cdir" timeout 240 "$K" build -o "$WT/by_default.exe" "$f" 2>&1)"; r1=$?
        if [ "$r1" -eq 0 ] || ! printf '%s\n' "$o1" | grep -q "E0505"; then
          fail "bypass $id: control failed: a plain \`kryos build\` of $repro must be rejected with E0505 (rc=$r1)"
          printf '%s\n' "$o1" | tail -3 | sed 's/^/    /'; by_bad=$((by_bad + 1)); continue
        fi
        o2="$(env -u KRYOS_CACHE KRYOS_CACHE_DIR="$cdir" timeout 240 "$K" build --capabilities-mode=permissive --cache -o "$WT/by_permissive.exe" "$f" 2>&1)"; r2=$?
        if [ "$r2" -ne 0 ]; then
          fail "bypass $id: setup failed: \`kryos build --capabilities-mode=permissive --cache\` exited $r2"
          printf '%s\n' "$o2" | tail -3 | sed 's/^/    /'; by_bad=$((by_bad + 1)); continue
        fi
        rm -f "$TMP/by_hit.exe"*
        o3="$(env -u KRYOS_CACHE KRYOS_CACHE_DIR="$cdir" timeout 240 "$K" build --cache -o "$WT/by_hit.exe" "$f" 2>&1)"; r3=$?
        if [ "$r3" -eq 0 ] && { [ -f "$TMP/by_hit.exe" ] || [ -f "$TMP/by_hit" ]; }; then
          by_ok=$((by_ok + 1)); ok "bypass $id ($repro): default-mode \`build --cache\` returned the artifact built in permissive mode"
        else
          fail "bypass $id: the default-mode \`build --cache\` no longer reuses the permissive artifact (rc=$r3): it was fixed; remove or update the table row"
          printf '%s\n' "$o3" | tail -3 | sed 's/^/    /'; by_bad=$((by_bad + 1))
        fi ;;
    esac
  done
fi

echo
echo "documented escapes: $doc_n, accepted: $accepted_n, undocumented: $undoc, stale: $stale"
echo "documented bypasses: $by_n, verified: $by_ok, problems: $by_bad"
if [ "$fails" -eq 0 ]; then
  echo "check-escapes-documented: PASS"
  exit 0
fi
echo "check-escapes-documented: FAIL ($fails problem(s))"
exit 1
