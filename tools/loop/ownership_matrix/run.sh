#!/usr/bin/env bash
# run.sh <kryos> <srcdir> <outdir>: runs every generated program (gen1.py /
# gen2.py / gen3.py) on six legs and keeps the full outputs for compare.py:
# jd / jr = `kryos run` under KRYOS_FREE_DIAG / with real frees, ld / lr =
# LLVM AOT (`--release`), cd / cr = Cranelift AOT. Run it once with the master
# binary and once with the candidate, then `python compare.py <master_out>
# <cand_out>`. Several runs may go in parallel: each uses its own temp dir
# (`kryos run` builds into it).
K="$1"; S="$(cd "$2" && pwd)"; mkdir -p "$3"; O="$(cd "$3" && pwd)"
HERE="$(cd "$(dirname "$0")" && pwd)"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$HERE/../../../compiler/stdlib}"
mkdir -p "$O/tmp"
if command -v cygpath >/dev/null 2>&1; then
  export TEMP="$(cygpath -w "$O/tmp")" TMP="$(cygpath -w "$O/tmp")"
else
  export TMPDIR="$O/tmp"
fi
cp "$S"/*.kry "$O"/
cd "$O" || exit 1
for f in *.kry; do
  b="${f%.kry}"
  KRYOS_FREE_DIAG=1 KRYOS_BOX_DIAG=1 timeout 60 "$K" run --capabilities-mode=permissive "$f" > "$b.jd" 2> "$b.jde"; echo "rc=$?" >> "$b.jd"
  timeout 60 "$K" run --capabilities-mode=permissive "$f" > "$b.jr" 2> /dev/null; echo "rc=$?" >> "$b.jr"
  if timeout 300 "$K" build --release --capabilities-mode=permissive "$f" -o "${b}_l.exe" > "$b.lb" 2>&1; then
    KRYOS_FREE_DIAG=1 timeout 60 "./${b}_l.exe" > "$b.ld" 2> "$b.lde"; echo "rc=$?" >> "$b.ld"
    timeout 60 "./${b}_l.exe" > "$b.lr" 2> /dev/null; echo "rc=$?" >> "$b.lr"
  else
    echo BUILDFAIL > "$b.ld"; echo BUILDFAIL > "$b.lr"; : > "$b.lde"
  fi
  if timeout 300 "$K" build --backend cranelift --capabilities-mode=permissive "$f" -o "${b}_c.exe" > "$b.cb" 2>&1; then
    KRYOS_FREE_DIAG=1 timeout 60 "./${b}_c.exe" > "$b.cd" 2> "$b.cde"; echo "rc=$?" >> "$b.cd"
    timeout 60 "./${b}_c.exe" > "$b.cr" 2> /dev/null; echo "rc=$?" >> "$b.cr"
  else
    echo BUILDFAIL > "$b.cd"; echo BUILDFAIL > "$b.cr"; : > "$b.cde"
  fi
  rm -f "${b}_l.exe" "${b}_c.exe" "${b}_l.pdb" "${b}_c.pdb"
done
echo DONE > _done
