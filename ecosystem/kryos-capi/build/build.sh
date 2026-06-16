#!/usr/bin/env bash
# ============================================================
# build.sh -- the kryos-capi build wrapper.
#
# Produces, from src/csvlib.kry:
#   dist/csvlib.dll            a real loadable shared library with C exports
#   dist/csvlib.manifest.json  raw `kryos manifest --caps` output
#   dist/csvlib.caps.json      curated authority header, keyed by C symbol
#
# then drives the consumer under two allow-sets and asserts the spec's
# done-criteria. Exits non-zero on any failure.
#
# Why the dllexport/zig step: `kryos build` does not expose a --shared flag
# (the driver+linker support it via OutputType::Library, but no CLI surface).
# So we build the shared lib "on the demo's terms": emit LLVM IR, promote the
# exported symbols from `internal` to `dllexport`, and assemble+link a DLL with
# `zig cc` (a clang frontend that auto-locates the MSVC SDK), linking the same
# kryos_rt / kryos_stdlib_native libs the normal kryos linker uses.
#
# Env overrides: KRYOS (path to the kryos.exe to use).
# ============================================================
set -euo pipefail

# --- locate project root and the toolchain --------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

# Use `kryos` on PATH by default; override with KRYOS=/path/to/kryos(.exe).
# The runtime libs (kryos_rt / kryos_stdlib_native) are looked up next to the
# binary; override with KRYOS_LIBDIR if they live elsewhere.
KRYOS="${KRYOS:-kryos}"
KRYOS_BIN="$(command -v "$KRYOS" || echo "$KRYOS")"
TOOLDIR="${KRYOS_LIBDIR:-$(dirname "$KRYOS_BIN")}"
RT="$TOOLDIR/kryos_rt.lib"
ST="$TOOLDIR/kryos_stdlib_native.lib"

LIBNAME="csvlib"
EXPORTS="csv_field_count,csv_row_count,csv_to_json_file"
WINLIBS="-lntdll -luserenv -lws2_32 -ldbghelp -luser32 -lbcrypt -ladvapi32"

command -v "$KRYOS" >/dev/null || { echo "FATAL: kryos not found (set KRYOS=/path/to/kryos)" >&2; exit 1; }
command -v zig >/dev/null || { echo "FATAL: zig not on PATH (needed to assemble the DLL)" >&2; exit 1; }
for lib in "$RT" "$ST"; do
    [ -e "$lib" ] || { echo "FATAL: missing runtime lib $lib (set KRYOS_LIBDIR)" >&2; exit 1; }
done

mkdir -p dist
echo "== kryos-capi build =="
echo "kryos   : $KRYOS_BIN"
echo "exports : $EXPORTS"
echo

# --- 1. type-check (proves the @capabilities labels are honest) -----------
echo "[1/5] capability check"
"$KRYOS" check src/csvlib.kry
echo "      ok: every annotated export's declared caps match its real usage"
# negative proof: a mislabeled (compute) io export must be REJECTED.
if "$KRYOS" check fixtures/mislabel_compute.kry >/dev/null 2>dist/mislabel_err.txt; then
    echo "FATAL: a compute-labelled io export compiled -- verification is broken" >&2; exit 1
fi
grep -q "E0505" dist/mislabel_err.txt \
    && echo "      ok: mislabelling io as compute is rejected (E0505)" \
    || { echo "FATAL: expected E0505 rejecting the mislabelled fixture" >&2; cat dist/mislabel_err.txt >&2; exit 1; }
echo

# --- 2. emit LLVM IR and promote exports to dllexport ---------------------
echo "[2/5] emit IR + mark C exports dllexport"
"$KRYOS" build --release --emit-llvm src/csvlib.kry -o "dist/$LIBNAME.ll" >/dev/null
cp "dist/$LIBNAME.ll" "dist/${LIBNAME}_exp.ll"
IFS=',' read -ra SYMS <<< "$EXPORTS"
for s in "${SYMS[@]}"; do
    sed -i "/^define internal .*@${s}(/ s/define internal/define dllexport/" "dist/${LIBNAME}_exp.ll"
    grep -q "define dllexport .*@${s}(" "dist/${LIBNAME}_exp.ll" \
        || { echo "FATAL: failed to dllexport $s" >&2; exit 1; }
done
echo "      ok: ${#SYMS[@]} symbols promoted to dllexport"
echo

# --- 3. assemble + link the shared library --------------------------------
echo "[3/5] link dist/$LIBNAME.dll via zig cc (MSVC target)"
zig cc -target x86_64-windows-msvc -shared -o "dist/$LIBNAME.dll" \
    "dist/${LIBNAME}_exp.ll" "$RT" "$ST" $WINLIBS 2>/dev/null
[ -e "dist/$LIBNAME.dll" ] || { echo "FATAL: DLL not produced" >&2; exit 1; }
echo "      ok: $(ls -la dist/$LIBNAME.dll | awk '{print $5}') bytes"
echo

# --- 4. emit the capability manifest --------------------------------------
echo "[4/5] caps manifest -> dist/$LIBNAME.caps.json"
"$KRYOS" manifest --caps src/csvlib.kry -o "dist/$LIBNAME.manifest.json" >/dev/null
"$KRYOS" run gen_caps.kry "dist/$LIBNAME.manifest.json" "$EXPORTS" "dist/$LIBNAME.caps.json" "$LIBNAME"
echo
echo "      dist/$LIBNAME.caps.json:"
sed 's/^/      /' "dist/$LIBNAME.caps.json"
echo
echo

# --- 5. drive the consumer + assert the done-criteria ---------------------
echo "[5/5] consumer end-to-end checks"
PASS=0
FAIL=0
assert_grep() { # <label> <file> <pattern>
    if grep -q -- "$2" "$3"; then echo "      PASS  $1"; PASS=$((PASS+1));
    else echo "      FAIL  $1 (expected /$2/ in $3)"; FAIL=$((FAIL+1)); fi
}
refute_grep() { # <label> <file> <pattern>
    if grep -q -- "$2" "$3"; then echo "      FAIL  $1 (unexpected /$2/ in $3)"; FAIL=$((FAIL+1));
    else echo "      PASS  $1"; PASS=$((PASS+1)); fi
}

# scenario A: compute-only host -> io export refused, compute exports run
"$KRYOS" run consumer.kry "dist/$LIBNAME.caps.json" "dist/$LIBNAME.dll" "compute" > dist/out_compute.txt
echo "      --- allow-set=compute ---"; sed 's/^/        /' dist/out_compute.txt
assert_grep "compute host binds csv_field_count" "ALLOW   csv_field_count" dist/out_compute.txt
assert_grep "csv_field_count returns 3 fields"   "csv_field_count = 3"     dist/out_compute.txt
assert_grep "csv_row_count returns 2 rows"       "csv_row_count = 2"       dist/out_compute.txt
assert_grep "compute host REFUSES io export"     "REFUSE  csv_to_json_file" dist/out_compute.txt
assert_grep "refusal names the excess io"        "excess={io}"             dist/out_compute.txt
refute_grep "io export never ran"                "csv_to_json_file wrote"  dist/out_compute.txt

# scenario B: {compute,io} host -> all three run
rm -f dist/csvlib_out.json
"$KRYOS" run consumer.kry "dist/$LIBNAME.caps.json" "dist/$LIBNAME.dll" "compute,io" > dist/out_io.txt
echo "      --- allow-set=compute,io ---"; sed 's/^/        /' dist/out_io.txt
assert_grep "io host binds csv_to_json_file" "ALLOW   csv_to_json_file"   dist/out_io.txt
assert_grep "io export wrote bytes"          "csv_to_json_file wrote"     dist/out_io.txt
assert_grep "io export produced the file"    "Alice"                      dist/csvlib_out.json

echo
echo "== build wrapper: $PASS passed, $FAIL failed =="
[ "$FAIL" -eq 0 ] || exit 1
echo "BUILD OK"
