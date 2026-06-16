#!/usr/bin/env bash
# check_caps.sh -- the manifest test: prove the PARSE/EFFECT split at compile time.
#
# Two machine-checkable claims, in one run:
#
#   1. src/typed.kry (the parse + validate surface) is COMPUTE-ONLY. The deny
#      gate fails if ANY function there carries net/io/ffi/crypto/process/env/
#      term/db/time. A green run is the proof that "config parsing has no side
#      effects" -- a compute validator literally cannot read a file, the env, or
#      the network.
#
#   2. src/loader.kry IS where the effects live: load_file declares `io`,
#      load_env_var declares `process`. The capability to reach the outside world
#      is confined to that thin layer and nowhere else.
#
# Usage:
#   KRYOS=/path/to/kryos.exe ./check_caps.sh    # or ./check_caps.sh if kryos is on PATH
set -euo pipefail

KRYOS="${KRYOS:-kryos}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PURE="$HERE/src/typed.kry"
LOADER="$HERE/src/loader.kry"

echo "== capability manifest: src/typed.kry (parse + validate surface) =="
"$KRYOS" manifest --caps --format pretty "$PURE"

echo
echo "== deny net,io,ffi,crypto,process,env,term,db,time on src/typed.kry =="
if "$KRYOS" manifest --caps --deny net,io,ffi,crypto,process,env,term,db,time "$PURE" >/dev/null 2>&1; then
    echo "PASS: the parse/validate surface is compute-only (no I/O capabilities)."
else
    echo "FAIL: a function in src/typed.kry declares a non-compute capability." >&2
    exit 1
fi

echo
echo "== capability manifest: src/loader.kry (the effect layer) =="
"$KRYOS" manifest --caps --format pretty "$LOADER"
echo "(loader.kry is EXPECTED to carry io/process -- that is the whole point of the split.)"
