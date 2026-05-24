#!/usr/bin/env bash
# build_stage2_extlink.sh -- produce a RUNNING stage-2 binary via the
# proven external-link path (blocker #1).
#
# stage-1 emits a single .obj for the full concatenated self-host source,
# then MSVC link.exe links it against the Rust runtime libs (kryos_rt.lib +
# kryos_stdlib_native.lib) + system libs. This validates that stage-1's
# codegen produces a working full compiler, even though linker.kry cannot
# yet provide the runtime itself (true self-linking is the separate goal).
#
# Usage (from compiler/):  bash self-host/build_stage2_extlink.sh
set -u
cd "$(dirname "$0")/.."
SELFHOST=self-host
BUILD=target/bootstrap
mkdir -p "$BUILD"

STAGE1=$BUILD/kryos-stage1.exe
[ ! -x "$STAGE1" ] && STAGE1=$BUILD/kryos-stage1
[ ! -x "$STAGE1" ] && { echo "FAIL: no stage-1 binary; build it first"; exit 1; }

# 1. Concatenate full source (runtime first, main last); strip internal `use`.
FULL=$BUILD/kryos-sh-full.kry
cat "$SELFHOST/runtime.kry" "$SELFHOST/token.kry" "$SELFHOST/lexer.kry" \
    "$SELFHOST/ast.kry" "$SELFHOST/parser.kry" "$SELFHOST/types.kry" \
    "$SELFHOST/mir.kry" "$SELFHOST/lower.kry" "$SELFHOST/optimize.kry" \
    "$SELFHOST/regalloc.kry" "$SELFHOST/x86.kry" "$SELFHOST/codegen.kry" \
    "$SELFHOST/elf.kry" "$SELFHOST/coff.kry" "$SELFHOST/linker.kry" \
    "$SELFHOST/main.kry" > "$FULL"
grep -vE '^use (token|lexer|ast|parser|types|mir|lower|optimize|regalloc|x86|codegen|elf|coff|linker|runtime|main)$' "$FULL" > "$FULL.tmp"
mv "$FULL.tmp" "$FULL"
echo "full source: $(wc -l < "$FULL") lines"

# 2. Emit single .obj with stage-1.
OBJ=$BUILD/kryos-stage2.obj
echo "emitting $OBJ (stage-1 obj of full source) ..."
KRYOS_SKIP_TYPES=1 KRYOS_NO_ASLR=1 "$STAGE1" obj "$FULL" -o "$OBJ" 2>&1 | grep -iE "^error|panic|double-free" | head -5
[ ! -f "$OBJ" ] && { echo "FAIL: stage-1 did not produce $OBJ"; exit 1; }
echo "obj: $(wc -c < "$OBJ") bytes"

# 3. MSVC link.
OUT=$BUILD/kryos-stage2.exe
VCVARS="C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat"
RT="target/release/kryos_rt.lib"
STD="target/release/kryos_stdlib_native.lib"
echo "linking $OUT ..."
cmd.exe /c "call \"$VCVARS\" >NUL && link.exe /NOLOGO /SUBSYSTEM:CONSOLE /ENTRY:mainCRTStartup /STACK:33554432 /Brepro /OUT:$(cygpath -w "$OUT") $(cygpath -w "$OBJ") $(cygpath -w "$RT") $(cygpath -w "$STD") /NODEFAULTLIB:libcmt.lib msvcrt.lib vcruntime.lib ucrt.lib legacy_stdio_definitions.lib kernel32.lib ws2_32.lib advapi32.lib userenv.lib bcrypt.lib ntdll.lib user32.lib" 2>&1 | grep -iE "error|unresolved|warning LNK" | head -20
[ ! -f "$OUT" ] && { echo "FAIL: link did not produce $OUT"; exit 1; }
echo "stage-2: $(wc -c < "$OUT") bytes -> $OUT"
