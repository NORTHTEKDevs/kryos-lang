#!/usr/bin/env bash
# installed_toolchain_check.sh -- the `kryos` on your PATH must agree with this repo.
#
# WHY THIS EXISTS. On 2026-08-16 three independent agents, each trying to USE the
# language rather than test it, hit the same wall: the globally-installed
# `kryos` (~/.local/bin/kryos, dated 2026-06-15) REJECTED valid programs that the
# repo's freshly-built compiler accepted -- 72 errors on one 315-line file.
#
# The trap is worse than "it is old". The stale binary reported
#     kryos 1.0.0-beta.1
# while the repo's actual compiler reported, at that time,
#     kryos 0.9.0
# so it advertised a HIGHER version than the source it came from. (The repo
# reports `kryos 1.0.0` since the 2026-08-31 v1.0.0 release; the 0.9.0 above
# is the version as of the 2026-08-16 incident, kept as the record of it.)
# Nothing about the failure looked like staleness: you got E0102 "undefined variable `None`",
# E0100 on `Dict<K,V>`, and "unknown capability `fs:read`" -- all of which read as
# language bugs, not as a two-month-old toolchain. Two months of language work
# (the fs:read/fs:write sub-capability model, Option spelling, Dict generics) did
# not exist in that binary.
#
# This is the single most likely thing to make someone conclude "the language is
# broken" when it is not, and no other gate in this repo could catch it, because
# every other gate invokes compiler/target/release/kryos.exe by path.
#
# ADVISORY, not a CI gate: CI has no global install, so absence is a clean SKIP.
# Wired into `kryos-loop.sh preflight`, which is the "run this first, always"
# step and already exists to catch exactly this class of stale-artifact trap
# (see its staticlib-freshness check).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
REPO_BIN="$ROOT/compiler/target/release/kryos.exe"
[ -x "$REPO_BIN" ] || REPO_BIN="$ROOT/compiler/target/release/kryos"

if [ ! -x "$REPO_BIN" ]; then
    echo "installed-toolchain: SKIP (no repo build yet -- run cargo build --release)"
    exit 0
fi
PATH_BIN="$(command -v kryos 2>/dev/null || true)"
if [ -z "$PATH_BIN" ]; then
    echo "installed-toolchain: SKIP (no \`kryos\` on PATH -- nothing to drift)"
    exit 0
fi

repo_ver="$("$REPO_BIN" --version 2>/dev/null | head -1)"
path_ver="$("$PATH_BIN" --version 2>/dev/null | head -1)"
echo "  repo build : $repo_ver  ($REPO_BIN)"
echo "  on PATH    : $path_ver  ($PATH_BIN)"

fail=0

# --- functional agreement, which is what actually bites -------------------
# A version string can match while the binary is still behind (or ahead, as the
# stale one was). The decisive test is behavioural: does the PATH binary accept
# a program the repo compiler accepts? Use a real showcase program, not a toy --
# the stale binary happily compiled `println("hi")` while failing every program
# that used the last two months of language surface.
PROBE=""
for cand in examples/showcase/log_analyzer.kry examples/showcase/sales_report.kry examples/hello.kry; do
    [ -f "$cand" ] && { PROBE="$cand"; break; }
done
if [ -z "$PROBE" ]; then
    echo "installed-toolchain: SKIP (no probe program found)"
    exit 0
fi

if ! "$REPO_BIN" check "$PROBE" >/dev/null 2>&1; then
    echo "  note: repo build cannot compile $PROBE either -- not a drift problem, skipping"
    exit 0
fi

if "$PATH_BIN" check "$PROBE" >/dev/null 2>&1; then
    echo "  ok   PATH \`kryos\` accepts $PROBE, same as the repo build"
else
    n="$("$PATH_BIN" check "$PROBE" 2>&1 | grep -c 'error' || true)"
    echo "  FAIL the \`kryos\` on your PATH REJECTS $PROBE ($n errors) while the"
    echo "       repo build accepts it. Your installed toolchain is STALE."
    echo "       Its errors will look like language bugs. They are not."
    echo
    echo "       Fix:"
    echo "         cargo build --release --manifest-path compiler/Cargo.toml"
    echo "         cp compiler/target/release/kryos.exe            ~/.local/bin/kryos.exe"
    echo "         cp compiler/target/release/kryos_rt.lib         ~/.local/lib/"
    echo "         cp compiler/target/release/kryos_stdlib_native.lib ~/.local/lib/"
    echo "         cp compiler/stdlib/*.kry                        ~/.local/stdlib/"
    fail=1
fi

[ "$fail" -eq 0 ] && echo "installed-toolchain: PASS" || echo "installed-toolchain: FAIL"
exit $fail
