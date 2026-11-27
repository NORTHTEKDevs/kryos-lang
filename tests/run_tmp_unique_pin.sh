#!/usr/bin/env bash
# run_tmp_unique_pin.sh -- RUN-TMP-1: `kryos run` must link to a unique
# per-invocation path. Four same-named programs (main.kry) run concurrently
# against one shared TMP dir, ROUNDS times: every run must exit 0 and print its
# own marker, and nothing may be left behind in TMP afterwards.
# On master the output was <temp>/main.exe, so concurrent runs overwrote /
# locked each other's binary (link failures, wrong program output).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
ROUNDS="${ROUNDS:-8}"

W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
mkdir -p "$W/tmp"
if command -v cygpath >/dev/null 2>&1; then WT="$(cygpath -w "$W/tmp")"; else WT="$W/tmp"; fi
export TMP="$WT" TEMP="$WT" TMPDIR="$WT"

for p in a b c d; do
  mkdir -p "$W/$p"
  printf 'fn main() { println("program-%s") }\n' "$p" > "$W/$p/main.kry"
done

bad=0
for r in $(seq 1 "$ROUNDS"); do
  for p in a b c d; do
    ( "$K" run "$W/$p/main.kry" > "$W/$p.out" 2> "$W/$p.err"; echo $? > "$W/$p.rc" ) &
  done
  wait
  for p in a b c d; do
    if [ "$(cat "$W/$p.rc")" != "0" ] || [ "$(cat "$W/$p.out")" != "program-$p" ]; then
      bad=$((bad + 1))
      [ "$bad" = 1 ] && echo "first bad: round $r prog $p rc=$(cat "$W/$p.rc") out=$(head -c 40 "$W/$p.out") err=$(head -c 120 "$W/$p.err")"
    fi
  done
done

left="$(ls -A "$W/tmp" | wc -l)"
if [ "$bad" != 0 ]; then echo "FAIL run_tmp_unique_pin: $bad bad runs of $((ROUNDS * 4))"; exit 1; fi
if [ "$left" != 0 ]; then echo "FAIL run_tmp_unique_pin: $left entries left in TMP: $(ls -A "$W/tmp" | head -3 | tr '\n' ' ')"; exit 1; fi
echo "PASS run_tmp_unique_pin: $((ROUNDS * 4)) concurrent same-name runs ok, TMP clean"
