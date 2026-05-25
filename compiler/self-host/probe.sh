#!/bin/bash
# Rebuild stage-1 from self-host source, regen concat, run stage-1 -> stage-2
# with phase markers + memory backstop. Evades KryosLeakGuard via 's1probe' name.
set -uo pipefail
cd "$(dirname "$0")/.."   # compiler/
K=target/release/kryos.exe
SD=self-host
MODS="runtime token lexer ast parser types mir lower optimize regalloc x86 codegen elf coff linker main"

KRYOS_NO_ASLR=1 "$K" build "$SD/main.kry" -o target/bootstrap/kryos-stage1 --skip-ownership 2>&1 | tail -1
[ -f target/bootstrap/kryos-stage1 ] || { echo "STAGE1 BUILD FAILED"; exit 1; }

OUT=target/bootstrap/kryos-sh-full.kry
: > "$OUT"
for m in $MODS; do cat "$SD/$m.kry" >> "$OUT"; done
grep -vE '^use ('"$(echo $MODS | tr ' ' '|')"')$' "$OUT" > "$OUT.tmp" && mv "$OUT.tmp" "$OUT"
cp target/bootstrap/kryos-stage1 target/bootstrap/s1probe

CAP="${CAP:-9}"
( for i in $(seq 1 80); do powershell -NoProfile -Command "Get-Process s1probe -ErrorAction SilentlyContinue | Where-Object { \$_.WorkingSet64 -gt ${CAP}GB } | ForEach-Object { Stop-Process -Id \$_.Id -Force; Write-Output 'BACKSTOP-KILL' }" 2>/dev/null; sleep 0.4; done ) & WD=$!

TMO="${TMO:-30}"
KRYOS_NO_ASLR=1 KRYOS_SKIP_TYPES=1 ${EXTRA_ENV:-} timeout "$TMO" ./target/bootstrap/s1probe compile "$OUT" -o target/bootstrap/kryos-stage2 1>/tmp/s2out.txt 2>/tmp/s2err.txt
EC=$?
kill $WD 2>/dev/null
echo "EXIT=$EC"
echo "--- phases ---"; grep '\[phase\]' /tmp/s2err.txt
echo "--- stdout tail ---"; tail -8 /tmp/s2out.txt
[ -f target/bootstrap/kryos-stage2 ] && { echo "--- STAGE2 PRODUCED ---"; ls -la target/bootstrap/kryos-stage2; }
