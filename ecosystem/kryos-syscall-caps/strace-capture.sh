#!/usr/bin/env bash
# strace-capture.sh -- capture a syscall trace for a Kryos binary and audit it
# against its declared capability manifest.
#
# Linux-only: depends on strace. macOS would use dtruss, Windows ETW -- both
# out of scope for the MVP (see README).
#
# Usage:
#   ./strace-capture.sh <prog> <manifest.caps.json> [prog-args...]
#
# It runs <prog> under `strace -f` restricted to the network/file/process
# syscall classes, writes the trace to a temp file, then hands the trace and
# the manifest to demo_audit.kry. The audit's exit code is propagated:
#   0 -- observed capabilities are a subset of declared
#   1 -- the program issued a capability class the manifest did not declare
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: strace-capture.sh <prog> <manifest.caps.json> [prog-args...]" >&2
  exit 2
fi

if ! command -v strace >/dev/null 2>&1; then
  echo "error: strace not found (this tool is Linux-only for the MVP)" >&2
  exit 3
fi

PROG="$1"; MANIFEST="$2"; shift 2
KRYOS="${KRYOS:-kryos}"
TRACE="$(mktemp -t syscall-caps.XXXXXX)"
trap 'rm -f "$TRACE"' EXIT

# %network, %file, %process are strace syscall-class shorthands. We discard the
# program's own stdout so only the audit report reaches the terminal.
strace -f -e trace=%network,%file,%process -o "$TRACE" -- "$PROG" "$@" >/dev/null 2>&1 || true

"$KRYOS" run demo_audit.kry "$TRACE" "$MANIFEST"
