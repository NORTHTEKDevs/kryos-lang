#!/usr/bin/env bash
# minilisp_gate.sh -- regression pin for LEDGER item 44 (P0 memory corruption:
# an enum with a heap [Value] payload constructed from a shared/borrowed
# source aliased its source's buffer instead of getting an independent
# reference -- segfault/illegal-instruction on JIT, silent wrong answer on
# AOT). Runs the full tests/minilisp/ corpus against BOTH backends with
# KRYOS_BOX_DIAG + KRYOS_FREE_DIAG enabled so any use-after-free/double-free
# diagnostic is caught even on a run that happens not to crash, and pins each
# program's CORRECT output (independently derived from the Lisp source below,
# not copied from any prior run) so a regression shows up as a wrong-answer
# diff, not just a crash.
#
# THE FIX (kryos-codegen-{cranelift,llvm} RValue::EnumVariant construction):
# both backends stored enum payload fields as a raw bit-copy with no retain,
# clone, or dup -- unlike the parallel RValue::Struct (non-@copy) path, which
# already deep-dups Array fields and clones Str fields specifically to give
# the new aggregate an independent heap reference. So `Value.ListV(args)`
# (args a shared/borrowed `[Value]` array parameter -- the "list" builtin at
# examples/showcase/minilisp.kry:654 is the smallest trigger, no closure or
# map/frame binding required) made the new enum alias the SAME array as its
# source with no compensating reference; a later independent drop of the
# source local and of the enum's own field then double-freed it (and,
# transitively, each of its enum-boxed elements). Both backends now clone
# Str fields (kryos_string_clone) and deep-dup Array fields (kryos_array_dup,
# elem_kind extended to cover Enum elements via kryos_struct_retain, since an
# enum box shares the exact struct-box header layout) at EnumVariant
# construction time. Verified test-vacuity both directions (2026-08-17):
# reverting the fix reproduces `kryos run .. t9b.lisp` segfaulting (rc=139)
# exactly as LEDGER item 44 recorded; restoring it is green again.
#
# AOT RESIDUAL -- FIXED (2026-08-18, session 5, see LEDGER item 44 WAVE 3):
# the SEPARATE, deeper gap that lived on the LLVM/AOT backend for a closure
# CALLED THROUGH `apply()` is now closed. Root cause: `push`'s
# aggregate-boxing codegen path (kryos-codegen-llvm's `"push"` builtin arm)
# boxes a still-unboxed Enum SSA aggregate via `kryos_calloc` + a raw `store`
# with NO per-field dup of the just-boxed copy's heap-typed (str/array)
# payload -- unlike `RValue::EnumVariant` construction (5c611fa), which
# already dups those fields at ITS OWN boxing/construction site. A value
# reaching push from a shared/unretained container read (e.g. `eval()`
# forwarding whatever `env_lookup` read out of an env-frame map, gotcha #23's
# "shared handle" policy) got re-boxed as an ALIAS of the same payload with
# no compensating reference; a later independent drop of the env frame and
# of the pushed array element then double-freed it. Fix: a new generated
# per-enum helper (`__kryos_dup_fields_<Name>`, `emit_enum_dup_field_helpers`,
# mirrors `__kryos_drop_<Name>`'s tag-switch structure) dups the box's
# heap-typed fields in place after boxing, EXCEPT when the pushed local's
# only defining assignment(s) in the function are direct `RValue::EnumVariant`
# constructions (`local_is_always_fresh_enum_construction`) -- construction
# already dups its own fields, so re-dup'ing a value that can never alias
# anything else would leak, not fix anything (measured: an adversarial
# construct-then-immediately-push loop leaked ~150 bytes/iteration before
# this guard). All 10 original corpus cases are now genuinely clean (correct
# output, ZERO diag lines) on AOT -- t1-t9b previously needed the
# `aot_known_wrong=1` exemption below; none do anymore, kept literally 0.
# `mem_plateau_check.sh` still PASSes (4MB, unaffected) and a targeted
# construct-then-push probe (50k vs 500k iterations) shows bounded, non-
# proportional growth after the guard (was clearly proportional before it).
# NOT independently proven leak-free in every shape -- only in the exact
# patterns measured this session; see LEDGER item 44 for the full account.
#
# A first attempt at a general fix (extending kryos-mir::lower.rs's
# `retain_for_ty` to cover `MirType::Enum` generically, reusing
# `kryos_struct_retain` at all 13 of that function's call sites) was tried
# and REVERTED across two earlier sessions: it regressed previously-correct
# simple cases by retaining a value that was still an unboxed SSA aggregate,
# not yet a real pointer (`kryos_struct_retain on NON-BOX pointer`) --
# proving the fix needed to be scoped at the CODEGEN layer (where the actual
# LLVM-level representation, ptr vs aggregate, is known) rather than MIR.
#
# JIT STILL OPEN (untouched by the above, tracked in LEDGER item 44): t10
# (the closure-counter shape) fails NONDETERMINISTICALLY on JIT --
# `rc=132`/illegal-instruction with `KRYOS_BOX_DIAG`/`KRYOS_FREE_DIAG` set
# (10/10 reproductions this session), `rc=0`/"ERROR: unbound symbol: n"
# without them (10/10) -- classic UAF-roulette, zero diag lines either way.
# This is a DIFFERENT bug from the one fixed above (Cranelift/JIT was never
# touched by this session's fix) and is NOT closed. Deliberately given NO
# known-wrong exemption below (unlike the AOT column, which now stays 0
# everywhere): t10 is a newly-pinned regression target for unresolved work,
# not a documented tolerated gap, so its JIT failure is a real, visible FAIL
# in this gate's output, not silently swallowed.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
INTERP="$ROOT/examples/showcase/minilisp.kry"
CORPUS="$ROOT/tests/minilisp"

pass=0; fail=0; failed=""

# name  expected-last-line  aot_known_wrong(1/0)
# Expected values independently derived from each .lisp file's own Lisp
# semantics (see the per-case comment), not copied from a prior run's output.
# name  expected-last-line  aot_known_wrong(1/0)
declare -a CASES=(
  # t1: square(x)=x*x ; square(7) = 49
  "t1|49|0"
  # t2: apply1(f,x)=f(x) ; apply1(square,7) = square(7) = 49
  "t2|49|0"
  # t3: mymap(f,lst) conses (f (car lst)) onto (mymap f (cdr lst)) ;
  #     mymap(square,(1 2 3)) = (1 4 9)
  "t3|(1 4 9)|0"
  # t4: count(n) = n=0 ? 0 : 1+count(n-1) ; count(5) = 5
  "t4|5|0"
  # t5: build(n) = n=0 ? () : cons(n, build(n-1)) ; build(3) = (3 2 1)
  "t5|(3 2 1)|0"
  # t6: rep(f,n) = n=0 ? 0 : f(n)+rep(f,n-1) ; rep(square,3) = 9+4+1 = 14
  "t6|14|0"
  # t7: build2(f,n) = n=0 ? () : cons(n, build2(f,n-1)), f unused ;
  #     build2(square,3) = (3 2 1)
  "t7|(3 2 1)|0"
  # t8: build3(f,n) = n=0 ? () : cons(f(n), build3(f,n-1)) ;
  #     build3(square,3) = (9 4 1)
  "t8|(9 4 1)|0"
  # t9: suml(lst) = null?(lst) ? 0 : car(lst)+suml(cdr(lst)) ;
  #     suml((1 2 3 4)) = 10
  "t9|10|0"
  # t9b: first(lst)=car(lst) ; first((7 8 9)) = 7
  "t9b|7|0"
  # t10: closure-counter shape -- make-counter returns a closure that
  # set!-mutates a captured local n on each call; two successive top-
  # level calls to the SAME returned closure c1 must observe 1 then 2
  # (LEDGER item 44 WAVE 2 second target -- the demo's own
  # run_closure_counter_demo section died nondeterministically; this pins
  # the shape as its own corpus case so it runs under the gate's usual
  # diag+output checks). AOT is clean (WAVE 3 fix). JIT still fails here
  # NONDETERMINISTICALLY (rc=132 illegal-instruction, or rc=0 with a wrong
  # "unbound symbol" answer, zero diag lines either way) -- a SEPARATE,
  # still-open bug this session's fix does not touch. Deliberately given NO
  # known-wrong exemption (unlike aot_known_wrong above): this is a real,
  # visible FAIL below, not silently swallowed, because it is a newly-pinned
  # regression target for unresolved work, not a documented tolerated gap.
  "t10|2|0"
)

run_one() {
    local backend="$1" bin_desc="$2" name="$3" want="$4" aot_known_wrong="$5"
    shift 5
    local out rc lastline diagn
    out="$(KRYOS_BOX_DIAG=1 KRYOS_FREE_DIAG=1 timeout 30 "$@" "$CORPUS/$name.lisp" 2>&1)"
    rc=$?
    diagn="$(printf '%s\n' "$out" | grep -ciE "KRYOS-FREE-DIAG|double free")"
    lastline="$(printf '%s\n' "$out" | tail -1)"

    if [ "$diagn" -ne 0 ]; then
        fail=$((fail + 1)); failed="$failed ${backend}:${name}(diag)"
        printf '  FAIL  %-22s %-4s diag=%s rc=%s\n' "$name" "$backend" "$diagn" "$rc"
        printf '%s\n' "$out" | head -6 | sed 's/^/          /'
        return
    fi
    if [ "$rc" != 0 ]; then
        fail=$((fail + 1)); failed="$failed ${backend}:${name}(rc=$rc)"
        printf '  FAIL  %-22s %-4s rc=%s (want 0)\n' "$name" "$backend" "$rc"
        printf '%s\n' "$out" | tail -4 | sed 's/^/          /'
        return
    fi

    if [ "$lastline" = "$want" ]; then
        pass=$((pass + 1)); printf '  ok    %-22s %-4s -> %s\n' "$name" "$backend" "$lastline"
    elif [ "$backend" = "AOT" ] && [ "$aot_known_wrong" = "1" ]; then
        pass=$((pass + 1))
        printf '  ok*   %-22s %-4s -> %-16s (KNOWN-WRONG, LEDGER item 44 residual, not a crash/diag)\n' \
            "$name" "$backend" "$lastline"
    else
        fail=$((fail + 1)); failed="$failed ${backend}:${name}(wrong-answer)"
        printf '  FAIL  %-22s %-4s -> %-16s (want %s)\n' "$name" "$backend" "$lastline" "$want"
    fi
}

echo "== minilisp corpus: JIT (kryos run) =="
for c in "${CASES[@]}"; do
    IFS='|' read -r name want aot_known_wrong <<< "$c"
    run_one JIT "kryos run" "$name" "$want" "$aot_known_wrong" "$K" run "$INTERP"
done

echo "== minilisp corpus: AOT (kryos build --release) =="
W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
AOT_BIN="$W/minilisp"
if KRYOS_STDLIB_DIR="$KRYOS_STDLIB_DIR" timeout 120 "$K" build --release "$INTERP" -o "$AOT_BIN" >"$W/build.log" 2>&1; then
    [ -x "$AOT_BIN" ] || AOT_BIN="$AOT_BIN.exe"
    for c in "${CASES[@]}"; do
        IFS='|' read -r name want aot_known_wrong <<< "$c"
        run_one AOT "kryos build" "$name" "$want" "$aot_known_wrong" "$AOT_BIN"
    done
else
    fail=$((fail + 1)); failed="$failed AOT:build-failed"
    echo "  FAIL  AOT build failed:"
    tail -10 "$W/build.log" | sed 's/^/          /'
fi

echo
if [ "$fail" -eq 0 ]; then
    echo "minilisp: $pass/$pass ok (both backends fully correct)"
    exit 0
fi
echo "minilisp: $fail FAILED --$failed"
exit 1
