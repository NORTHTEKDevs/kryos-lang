#!/usr/bin/env bash
# ecosystem/kryos-embed/hosts/node/build.sh
# Compile agent_wasm.kry to WASM and place the artifact in dist/.
# Run from the repository root:
#   bash ecosystem/kryos-embed/hosts/node/build.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
cd "$ROOT"

export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="$ROOT/compiler/target/release/kryos.exe"
SRC="$ROOT/ecosystem/kryos-embed/hosts/node/agent_wasm.kry"
OUT="$ROOT/ecosystem/kryos-embed/hosts/node/dist/kryos_embed_agent.wasm"

mkdir -p "$ROOT/ecosystem/kryos-embed/hosts/node/dist"

echo "Building $SRC -> $OUT ..."
"$KRYOS" build --release --backend wasm "$SRC" -o "$OUT"
echo "Build complete: $OUT"
