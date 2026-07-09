#!/usr/bin/env bash
# Windows self-hosting bootstrap + fixed-point check for Kryos.
#
#   stage-0 = Rust/Cranelift compiler (target/release/kryos.exe)
#   stage-1 = self-host compiled by stage-0           (different backend)
#   stage-2 = self-host compiled by stage-1           (self-host backend)
#   stage-3 = self-host compiled by stage-2
#   stage-4 = self-host compiled by stage-3
#
# stage-1 != stage-2 (different backends). stage-2 == stage-3 == stage-4 proves
# the compiler reproduces itself: SELF-HOSTING.
#
# Windows links externally (link-win.ps1: link.exe + kryos_rt.lib +
# kryos_stdlib_native.lib + rt_shim_win.obj). The self-host typechecker now
# runs CLEAN on its own source (0 errors) within bounded memory, so
# KRYOS_SKIP_TYPES is no longer set -- the obj path type-checks itself.
set -euo pipefail
cd "$(dirname "$0")/.."          # compiler/
K=target/release/kryos.exe
SD=self-host
BT=target/bootstrap
MODS="runtime token lexer ast parser types mir lower optimize regalloc x86 codegen elf coff linker main"
export KRYOS_NO_ASLR=1

link() { powershell -NoProfile -ExecutionPolicy Bypass -File "$SD/link-win.ps1" "$(cygpath -w "$1")" "$(cygpath -w "$2")" >/dev/null 2>&1; }
# Run a FRESHLY-linked stage exe, retrying once after a short settle if it
# returns 126/127. Windows Defender briefly holds a newly written binary on
# first exec (the "not found despite existing" AV scan), which otherwise
# aborts the bootstrap under `set -e`. Same guard the parity harness uses.
run_stage() {
    local exe="$1"; shift
    local rc=0 attempt=0
    set +e
    while [ $attempt -lt 6 ]; do
        "$exe" "$@"; rc=$?
        # 126/127 from a freshly-linked binary is the AV on-first-exec hold
        # (Defender reports "not found" while it scans), not a real failure.
        # Settle and retry; anything else is a genuine result, return it now.
        if [ $rc -ne 126 ] && [ $rc -ne 127 ]; then break; fi
        attempt=$((attempt + 1))
        sleep 2
    done
    set -e
    return $rc
}
regen() { : > "$BT/kryos-sh-full.kry"; for m in $MODS; do cat "$SD/$m.kry" >> "$BT/kryos-sh-full.kry"; done
  grep -vE '^use ('"$(echo $MODS|tr ' ' '|')"')$' "$BT/kryos-sh-full.kry" > "$BT/kryos-sh-full.kry.tmp" && mv "$BT/kryos-sh-full.kry.tmp" "$BT/kryos-sh-full.kry"; }

echo "stage-0 -> stage-1 (Rust/Cranelift compiles self-host)"
"$K" build "$SD/main.kry" -o "$BT/kryos-stage1" --skip-ownership >/dev/null
regen
SRC="$BT/kryos-sh-full.kry"

echo "stage-1 -> stage-2.obj"
run_stage "$BT/kryos-stage1" obj "$SRC" -o "$BT/kryos-stage2.obj" >/dev/null
link "$BT/kryos-stage2.obj" "$BT/kryos-stage2.exe"

echo "stage-2 -> stage-3.obj"
run_stage "$BT/kryos-stage2.exe" obj "$SRC" -o "$BT/kryos-stage3.obj" >/dev/null
link "$BT/kryos-stage3.obj" "$BT/kryos-stage3.exe"

echo "stage-3 -> stage-4.obj"
run_stage "$BT/kryos-stage3.exe" obj "$SRC" -o "$BT/kryos-stage4.obj" >/dev/null

echo "--- fixed-point check ---"
h2=$(sha256sum "$BT/kryos-stage2.obj" | cut -d' ' -f1)
h3=$(sha256sum "$BT/kryos-stage3.obj" | cut -d' ' -f1)
h4=$(sha256sum "$BT/kryos-stage4.obj" | cut -d' ' -f1)
echo "stage-2.obj $h2"
echo "stage-3.obj $h3"
echo "stage-4.obj $h4"
if [ "$h2" = "$h3" ] && [ "$h3" = "$h4" ]; then
  echo "PASS: stage-2 == stage-3 == stage-4 -- Kryos is self-hosting."
else
  echo "FAIL: stages differ -- not a fixed point."; exit 1
fi
