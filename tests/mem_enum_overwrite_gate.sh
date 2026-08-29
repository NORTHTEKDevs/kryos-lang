#!/usr/bin/env bash
# mem_enum_overwrite_gate.sh -- regression gate for LEDGER item 49 (item 45
# REOPENED): the enum-array-payload leak on container-slot OVERWRITE
# (`arr[i] = v` / `m[k] = v`), a DIFFERENT data-flow path into the same
# underlying gap item 45's own fix (mem_enum_array_push_gate.sh) does not
# reach -- that fix targets a fresh-push-and-drop shape; these fixtures
# OVERWRITE an EXISTING slot every iteration instead.
#
# Root cause: `release_if_ne_fn` in kryos-mir/src/lower.rs never covered
# Struct/Enum -- only Str/Array/Map -- so the array/map-index-assignment
# lowering's "read old, store new, release old (guarded by pointer
# inequality)" sequence silently did NOTHING for a Struct/Enum-valued slot.
# Fixed by adding a dedicated `Instruction::DropIfNe` MIR instruction
# (type-directed, backend-emitted) plus TWO backend-specific wrinkles
# discovered and fixed in the same session:
#   1. `drop_unescaped_str_temps`'s whole-window guard treated the new
#      DropIfNe instruction as an unmodeled shape and bailed out, silently
#      defeating item 45's OWN array-field cleanup for any statement that
#      also contains a DropIfNe -- exactly the shape this item's repro
#      combines them in. Fixed by adding DropIfNe to the guard's allowlist.
#   2. Cranelift's `kryos_array_set`/`kryos_map_insert(_str)` codegen
#      UNCONDITIONALLY retains a Struct/Enum 3rd argument (LEDGER item 44
#      WAVE 1's own compensating fix for a different danger), which nothing
#      balances for an unnamed/inline enum RHS -- DropIfNe now neutralizes
#      that extra unit via one `kryos_struct_release_shared` call before the
#      normal drop (NOT a second full drop -- that double-frees a droppable
#      field, since the slot's very-first occupant, from the container's own
#      initializer, was never retained at all and only needs one release).
#   3. LLVM boxes a Struct/Enum 3rd argument DIFFERENTLY per callee:
#      kryos_array_set uses kryos_calloc, kryos_map_insert(_str) uses
#      kryos_arc_alloc_i64 -- freeing an arc-boxed value with kryos_free
#      silently leaks it (bogus size-class, reported not deallocated) rather
#      than crashing. DropIfNe now routes the map case through
#      kryos_arc_release instead.
#
# Measured on the ORCHESTRATOR's pre-fix binary (2026-08-28), array shape:
# 66MB @ 500k iters, 371MB @ 3M iters (~124MB/M). This session's own
# pre-fix measurement (git stash + rebuild, both backends, both
# containers) at 3,000,000 iters: array AOT 371MB, array JIT 371MB, map AOT
# 463MB, map JIT 371MB. Post-fix: all four flat at 3-4MB.
#
# Windows-only (PowerShell PeakWorkingSet64 polling, matching
# mem_plateau_check.sh's own fallback technique).
#
# NOT COVERED here (adjacent shapes enumerated and tested this session,
# found ALSO leaking, but NOT fixed -- left OPEN, see LEDGER item 49's
# residual entries; do not add a "PASS" leg for these without a real fix):
#   - struct-field assignment holding an Enum/Struct (`h.v = Val.V(..)`):
#     LLVM represents a Struct/Enum-typed struct FIELD as an INLINE
#     aggregate (not a boxed pointer like an array/map ELEMENT), so
#     DropIfNe's raw-pointer-inequality guard does not apply as-is --
#     confirmed by trying it: invalid LLVM IR, a build failure, reverted.
#     tests/mem/struct_field_enum_overwrite_leak.kry.
#   - Str-payload enum construction (`Enum.V(str_expr)`) leaking the
#     pre-clone string on ANY construction (push OR overwrite) -- a sibling
#     gap to item 45 (which only ever covered the Array-typed-field case in
#     `drop_unescaped_str_temps`), pre-existing, NOT overwrite-specific, NOT
#     introduced or worsened by this session's fix.
#     tests/mem/enum_str_push_leak.kry reproduces it via item 45's OWN
#     push-and-drop shape (no overwrite involved at all).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
ITERS=3000000
CEIL_MB=50   # steady state ~3-4MB all four legs; pre-fix leak hits 371-463MB at this iter count

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-enum-overwrite: SKIP (no powershell -- this gate is Windows-only, matching mem_plateau_check.sh's fallback path)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"

fail=0

run_leg() {
  # $1 = label, $2 = probe .kry path, $3 = child process name (no .exe)
  local label="$1" probe="$2" procname="$3"
  local win_probe aot_bin win_aot aot_bytes aot_mb jit_bytes jit_mb

  win_probe="$(cygpath -w "$ROOT/$probe" 2>/dev/null || echo "$ROOT/$probe")"
  aot_bin="$(mktemp -u).exe"
  win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"

  if ! "$KRYOS" build --release "$ROOT/$probe" -o "$aot_bin" >/dev/null 2>&1; then
    echo "mem-enum-overwrite: $1 -- AOT build FAILED"
    fail=1
    return
  fi

  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\meog_${1}_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  rm -f "$aot_bin"
  case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
  if [ -z "$aot_bytes" ]; then
    echo "mem-enum-overwrite: $1 AOT leg -- could not read peak RSS (powershell probe returned nothing)"
    fail=1
  else
    aot_mb=$(( aot_bytes / 1024 / 1024 ))
    echo "mem-enum-overwrite: $1 AOT peak RSS ${aot_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
    [ "$aot_mb" -gt "$CEIL_MB" ] && { echo "mem-enum-overwrite: $1 AOT FAIL -- leak present"; fail=1; }
  fi

  # JIT leg: `kryos run` execs the Cranelift-compiled binary as a CHILD
  # process and deletes it on exit, so poll the child by its predictable
  # temp name rather than the outer `kryos.exe run` driver.
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\meog_${1}_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name '$procname' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
  if [ -z "$jit_bytes" ]; then
    echo "mem-enum-overwrite: $1 JIT leg -- could not read peak RSS (powershell probe returned nothing, or the child ran too briefly to sample -- rerun if this flakes)"
    fail=1
  else
    jit_mb=$(( jit_bytes / 1024 / 1024 ))
    echo "mem-enum-overwrite: $1 JIT peak RSS ${jit_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
    [ "$jit_mb" -gt "$CEIL_MB" ] && { echo "mem-enum-overwrite: $1 JIT FAIL -- leak present"; fail=1; }
  fi
}

run_leg array "tests/mem/enum_array_overwrite_leak.kry" "enum_array_overwrite_leak"
run_leg map "tests/mem/enum_map_overwrite_leak.kry" "enum_map_overwrite_leak"

if [ "$fail" -eq 0 ]; then
  echo "mem-enum-overwrite: PASS"
fi
exit $fail
