#!/bin/sh
# Run kryos-bench-governed with a @budget-capable toolchain.
#
# WHY THIS WRAPPER EXISTS:
# The installed kryos at ~/.local/bin (v4.43.0-rc.4, May 2026) PREDATES the
# @budget feature (landed v4.46.0). Its kryos_rt.lib has no kryos_budget_*
# symbols, so `kryos run` fails to link any program that calls chat(). This
# wrapper uses the current HEAD build in the kryos-lang repo plus that build's
# runtime static libs (which DO export the budget symbols).
#
# Canonical long-term fix: rebuild + reinstall the toolchain from kryos-lang HEAD
# (cargo build --release in compiler/, copy kryos.exe + kryos_rt.lib +
# kryos_stdlib_native.lib into ~/.local). Until then, this wrapper works.
#
# USAGE:
#   ./run.sh                  # offline dry-run (no creds): prints ceiling + cases, exits 0
#   OPENAI_API_KEY=sk-... ./run.sh
#   ANTHROPIC_API_KEY=sk-ant-... ./run.sh
#   BENCH_BASE_URL=http://127.0.0.1:11434/v1 BENCH_MODEL=llama3.2:3b ./run.sh   # local Ollama
set -e

# Override with KRYOS_LANG_DIR if the repo lives elsewhere.
KLANG="${KRYOS_LANG_DIR:-/c/Users/Krist/projects/active/kryos-lang}/compiler"
KB="$KLANG/target/debug/kryos.exe"           # HEAD debug build: `run` uses the Cranelift backend
RT="$KLANG/target/release/kryos_rt.lib"        # HEAD runtime: exports kryos_budget_*
SN="$KLANG/target/release/kryos_stdlib_native.lib"

if [ ! -x "$KB" ]; then
    echo "error: HEAD kryos build not found at $KB" >&2
    echo "       build it: (cd $KLANG && cargo build -p kryos-cli) and (cargo build --release -p kryos-rt -p kryos-stdlib-native)" >&2
    exit 1
fi

export KRYOS_RT_LIB="$(cygpath -w "$RT")"
export KRYOS_STDLIB_NATIVE_LIB="$(cygpath -w "$SN")"

HERE="$(cd "$(dirname "$0")" && pwd)"
exec "$KB" run "$HERE/bench.kry" "$@"
