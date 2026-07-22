#!/usr/bin/env bash
# concurrency_smoke.sh -- regression gate for concurrency primitives that must
# not DEADLOCK. Each program spawns real worker threads and must complete within
# a short timeout; a hang (timeout, exit 124) is the failure signal.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# completes <name> <expected-substring> <source> : must finish (rc 0) within
# the timeout and print the expected marker; a deadlock times out (rc 124).
completes() {
  local name="$1" want="$2" src="$3" f="$TMP/$1.kry"
  printf '%s' "$src" > "$f"
  local out rc
  out="$(timeout 15 "$KRYOS" run "$f" 2>&1)"; rc=$?
  if [ "$rc" -ne 0 ] || [[ "$out" != *"$want"* ]]; then
    echo "  FAIL  $name -- rc=$rc (124=deadlock) out=[$(printf '%s' "$out" | tr '\n' ' ')]"
    fail=$((fail+1))
  fi
}

# --- std::chan::WaitGroup MUST complete for >1 worker. Was a deadlock: the
# counter was a plain struct field, so each spawn-deep-copied worker decremented
# its own private copy and the shared count never reached 0 (wg_wait blocked
# forever for any worker count > 1). ---
completes chan_wg_4workers 'after wait' \
'use std::chan::{new_wait_group, wg_add, wg_done, wg_wait}
fn main() {
    let wg = wg_add(new_wait_group(), 4)
    let mut i = 0
    while i < 4 { let wgc = wg  spawn { sleep(10)  wg_done(wgc) }  i = i + 1 }
    wg_wait(wg)
    println("after wait")
}'

completes chan_wg_16workers 'all done' \
'use std::chan::{new_wait_group, wg_add, wg_done, wg_wait}
fn main() {
    let n = 16
    let wg = wg_add(new_wait_group(), n)
    let mut i = 0
    while i < n { let wgc = wg  spawn { sleep(5)  wg_done(wgc) }  i = i + 1 }
    wg_wait(wg)
    println("all done")
}'

completes chan_wg_1worker 'done-1' \
'use std::chan::{new_wait_group, wg_add, wg_done, wg_wait}
fn main() {
    let wg = wg_add(new_wait_group(), 1)
    let wgc = wg
    spawn { sleep(5)  wg_done(wgc) }
    wg_wait(wg)
    println("done-1")
}'

if [ "$fail" -eq 0 ]; then
  echo "concurrency-smoke: all programs completed (no deadlock)"
else
  echo "concurrency-smoke: $fail program(s) deadlocked/failed"
  exit 1
fi
[ "$fail" -eq 0 ]
