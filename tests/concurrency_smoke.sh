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

# fails_fast <name> <expected-rc> <expected-substring> <file> : must exit with
# the given NON-ZERO code within the timeout (not 124/timeout) and print the
# expected marker. For the two spawn-throw-and-reentrancy defects (LEDGER
# items 16 and 11(a)): an uncaught `throw` in a `spawn` task, and a mutating
# closure calling itself through its own stored value, must never hang --
# they are now FATAL (a clean, attributable process exit), not silent
# permanent hangs.
# completes_file <name> <expected-substring> <path> : like `completes`, but
# for an existing .kry file on disk rather than inline source.
completes_file() {
  local name="$1" want="$2" f="$3"
  local out rc
  out="$(timeout 15 "$KRYOS" run "$f" 2>&1)"; rc=$?
  if [ "$rc" -ne 0 ] || [[ "$out" != *"$want"* ]]; then
    echo "  FAIL  $name -- rc=$rc (124=deadlock) out=[$(printf '%s' "$out" | tr '\n' ' ')]"
    fail=$((fail+1))
  fi
}

fails_fast() {
  local name="$1" want_rc="$2" want="$3" f="$4"
  local out rc
  out="$(timeout 15 "$KRYOS" run "$f" 2>&1)"; rc=$?
  if [ "$rc" -ne "$want_rc" ] || [[ "$out" != *"$want"* ]]; then
    echo "  FAIL  $name -- rc=$rc (want $want_rc; 124=deadlock) out=[$(printf '%s' "$out" | tr '\n' ' ')]"
    fail=$((fail+1))
  fi
}

# --- An uncaught `throw` inside a `spawn` task used to silently skip the
# rest of the task -- including a `wg_done(wg)` the caller relied on -- so
# `wg_wait()` hung forever with no diagnostic (LEDGER item 16). Now the
# whole process terminates (exit 101, same contract as an uncaught main-
# thread throw) instead of hanging. ---
fails_fast spawn_uncaught_throw_no_hang 101 'uncaught exception in spawned thread' \
  "$ROOT/tests/security/attack_spawn_uncaught_throw_waitgroup_hang.kry"

# --- A mutating closure that reaches itself through its own stored value
# (e.g. a map/struct self-reference) used to spin forever against the
# item-7b serialization lock it already held on the same thread -- a
# permanent, unrecoverable hang with no timeout (LEDGER item 11(a)). Now
# detected and reported as a clean panic (exit 98) instead of hanging. ---
fails_fast closure_lock_reentrant_no_hang 98 'reentrant call into a mutating shared closure' \
  "$ROOT/tests/security/attack_closure_lock_reentrant_deadlock.kry"

# --- std::sync::Mutex.lock()/.unlock() called as a bare statement without
# reassigning (`mu = mu.lock()`) used to leave the REAL native mutex locked
# forever with zero diagnostic: a second lock() on the same never-reassigned
# binding spun forever, 100% of one core (LEDGER item 31). Now detected as a
# same-thread double-lock and reported as a clean panic (exit 98) instead of
# hanging. ---
fails_fast mutex_unreassigned_lock_no_hang 98 'deadlock: this thread already holds this std::sync::Mutex' \
  "$ROOT/tests/security/attack_mutex_unreassigned_self_deadlock.kry"

# --- Control: the CORRECT `mu = mu.lock()` / `mu = mu.unlock()` reassignment
# pattern must still complete cleanly -- proves the fix above did not turn a
# legitimate lock/unlock/lock/unlock cycle into a false-positive panic. ---
completes mutex_reassigned_lock_completes 'no hang' \
'use std::sync::{mutex_new}
fn main() {
    let mut mu = mutex_new()
    mu = mu.lock()
    mu = mu.unlock()
    mu = mu.lock()
    mu = mu.unlock()
    println("no hang")
}'

# --- LEDGER item 46a: std::chan::ChanWaitGroup only released ONE blocked
# wg_wait() caller -- completion was signalled by a single send() on a
# buffered(1) channel (a queue token, notify_one), so with 2+ concurrent
# waiters every waiter past the first hung forever on an empty, never-closed
# channel. wg_done() now CLOSES done_ch instead of sending a token: close is
# a real broadcast (notify_all + every later recv on a closed/drained
# channel returns immediately), so all waiters wake. ---
completes_file chan_wg_multi_waiter_no_hang 'main: got SECOND waiter completion' \
  "$ROOT/tests/security/attack_chan_waitgroup_multi_waiter_hang.kry"

# --- ADJACENT SHAPE of item 46a: 5 waiters (not just 2), spawned via a
# dynamic loop (a different binding/container form than two hand-written
# spawn blocks), PLUS one more waiter that arrives AFTER wg_done() has
# already closed done_ch (exercises the late-arrival fast path together
# with the broadcast-wakeup path in the same run). ---
completes chan_wg_5_waiters_plus_late 'all 6 waiters done' \
'use std::chan::{new_wait_group, wg_add, wg_done, wg_wait}
fn main() {
    let mut wg = new_wait_group()
    wg = wg_add(wg, 1)
    let results = chan()
    let mut i = 0
    while i < 5 {
        let wgc = wg
        spawn { wg_wait(wgc)  send(results, 1) }
        i = i + 1
    }
    sleep_ms(300)
    let wgc2 = wg
    spawn { wg_done(wgc2) }
    let mut got = 0
    while got < 5 {
        recv(results)
        got = got + 1
    }
    sleep_ms(50)
    let wgc3 = wg
    spawn { wg_wait(wgc3)  send(results, 1) }
    recv(results)
    println("all 6 waiters done")
}'

# --- LEDGER item 46b: the per-closure mutating-lock serializer (item 7b)
# only detected a thread RE-ENTERING the SAME lock address it already held.
# A classic AB-BA deadlock between TWO DIFFERENT closures' locks (closure A
# calls into closure B while a second thread holds B and calls into A) was
# invisible and hung forever. A cross-thread wait-for-graph cycle detector
# now catches it and fails loudly (a clean panic) instead of hanging. ---
fails_fast cross_closure_ab_ba_no_hang 98 'deadlock:' \
  "$ROOT/tests/security/attack_cross_closure_lock_deadlock.kry"

# --- ADJACENT SHAPE of item 46b: a 3-way circular wait (A->B->C->A) across
# THREE distinct closures' locks, not just a 2-closure AB-BA pair -- proves
# the detector walks a real cycle of any length, not a hard-coded pairwise
# check. ---
fails_fast cross_closure_3cycle_no_hang 98 'deadlock:' \
  "$ROOT/tests/security/attack_cross_closure_lock_3cycle_deadlock.kry"

# --- LEDGER item 46c: the codegen-inserted closure-call lock was a bare
# spin-then-yield_now() CAS loop with no knowledge of the cooperative
# executor. Holding it across a coop-yield point (a blocking op routed
# through io_offload, e.g. sleep_ms) parked the OS thread while the coop
# baton passed to a second task that spun on the real CAS forever, never
# yielding the baton back -- neither task could ever finish. The spin now
# yields the COOP BATON (not just the OS thread) on every failed attempt
# when running on a coop task, so the scheduler can resume the lock holder. ---
completes_file closure_lock_coop_sleep_yield_no_hang 'order: A=1 B=3' \
  "$ROOT/tests/security/attack_closure_lock_coop_yield_deadlock.kry"

# --- Control for the above: identical program with the coop-yield-while-
# locked call removed -- must also complete (proves the fix did not merely
# get lucky on this one shape, and that the deadlock-detector side of the
# fix does not false-positive on ordinary, non-contending coop usage). ---
completes_file closure_lock_coop_sleep_yield_control 'order: A=1 B=3' \
  "$ROOT/tests/security/attack_closure_lock_coop_yield_deadlock_control.kry"

# --- ADJACENT SHAPE of item 46c: the yield point inside the locked closure
# body is an EXPLICIT coop_yield() call (the language's own documented
# cooperative-suspension primitive) rather than a blocking I/O op routed
# through io_offload -- a different underlying mechanism, same hazard.
# Proves the fix is not sleep_ms/io_offload-specific. ---
completes_file closure_lock_coop_direct_yield_no_hang 'order: A=1 B=3' \
  "$ROOT/tests/security/attack_closure_lock_coop_direct_yield_deadlock.kry"

if [ "$fail" -eq 0 ]; then
  echo "concurrency-smoke: all programs completed (no deadlock)"
else
  echo "concurrency-smoke: $fail program(s) deadlocked/failed"
  exit 1
fi
[ "$fail" -eq 0 ]
