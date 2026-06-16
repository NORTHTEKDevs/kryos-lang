#!/usr/bin/env bash
# check_caps.sh -- the manifest test: prove the validator surface is compute-only.
#
# Emits the per-function capability manifest for src/ and fails (exit 1) if ANY
# function carries a non-compute capability (net, io, ffi, crypto, process, env,
# term, db, time). A green run is a machine-checkable guarantee that nothing on
# the validation path can perform I/O -- the security property this library
# exists to provide.
#
# Usage:
#   KRYOS=/path/to/kryos.exe ./check_caps.sh      # or just ./check_caps.sh if kryos is on PATH
set -euo pipefail

KRYOS="${KRYOS:-kryos}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$HERE/src"

echo "== capability manifest (src/) =="
"$KRYOS" manifest --caps --format pretty "$SRC"

echo
echo "== deny: net,io,ffi,crypto,process,env,term,db,time =="
if "$KRYOS" manifest --caps --deny net,io,ffi,crypto,process,env,term,db,time "$SRC" >/dev/null 2>&1; then
    echo "PASS: every function in src/ is compute-only (no I/O capabilities)."
else
    echo "FAIL: a function in src/ declares a non-compute capability." >&2
    exit 1
fi
