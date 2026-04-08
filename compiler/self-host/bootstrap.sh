#!/bin/bash
# ============================================================
# bootstrap.sh -- Bootstrap verification for the Kryos
# self-hosted compiler.
#
# This script verifies the self-hosting chain:
#   1. Compile the self-host with the Rust-based compiler (stage-0 -> stage-1)
#   2. Compile the self-host with stage-1 (stage-1 -> stage-2)
#   3. Compare stage-1 and stage-2 binaries (must be identical)
#
# If stage-1 == stage-2, the compiler is fully self-hosting.
#
# Usage: ./bootstrap.sh [--verbose]
# ============================================================

set -euo pipefail

SELFHOST_DIR="$(cd "$(dirname "$0")" && pwd)"
COMPILER_DIR="$(dirname "$SELFHOST_DIR")"
BUILD_DIR="$COMPILER_DIR/target/bootstrap"

VERBOSE="${1:-}"

log() {
    echo "=== $1 ==="
}

info() {
    if [ "$VERBOSE" = "--verbose" ]; then
        echo "  $1"
    fi
}

# Create build directory
mkdir -p "$BUILD_DIR"

# Concatenate all self-host source files in the right order
COMBINED="$BUILD_DIR/kryos-sh.kry"
log "Combining self-host source files"
cat \
    "$SELFHOST_DIR/token.kry" \
    "$SELFHOST_DIR/lexer.kry" \
    "$SELFHOST_DIR/ast.kry" \
    "$SELFHOST_DIR/parser.kry" \
    "$SELFHOST_DIR/types.kry" \
    "$SELFHOST_DIR/mir.kry" \
    "$SELFHOST_DIR/lower.kry" \
    "$SELFHOST_DIR/optimize.kry" \
    "$SELFHOST_DIR/regalloc.kry" \
    "$SELFHOST_DIR/x86.kry" \
    "$SELFHOST_DIR/codegen.kry" \
    "$SELFHOST_DIR/elf.kry" \
    "$SELFHOST_DIR/coff.kry" \
    "$SELFHOST_DIR/linker.kry" \
    "$SELFHOST_DIR/main.kry" \
    > "$COMBINED"

LINES=$(wc -l < "$COMBINED")
info "Combined source: $LINES lines"

# ── Stage 0: Build the Rust-based compiler ─────────────────
log "Stage 0: Building Rust-based compiler"
cd "$COMPILER_DIR"
cargo build --release -j 4 2>&1 | tail -1
KRYOS_RS="$COMPILER_DIR/target/release/kryos"
if [ ! -f "$KRYOS_RS" ] && [ -f "$KRYOS_RS.exe" ]; then
    KRYOS_RS="$KRYOS_RS.exe"
fi
info "Rust compiler: $KRYOS_RS"

# ── Stage 1: Compile self-host with Rust compiler ──────────
log "Stage 1: Compiling self-host with Rust compiler -> stage-1"
STAGE1="$BUILD_DIR/kryos-stage1"
"$KRYOS_RS" compile "$COMBINED" -o "$STAGE1" 2>&1 || {
    echo "FAIL: Stage 1 compilation failed"
    echo "The Rust-based compiler could not compile the self-host."
    echo "This is expected during early development."
    exit 1
}
info "Stage 1 binary: $STAGE1"
info "Stage 1 size: $(wc -c < "$STAGE1") bytes"

# ── Stage 2: Compile self-host with stage-1 ────────────────
log "Stage 2: Compiling self-host with stage-1 -> stage-2"
STAGE2="$BUILD_DIR/kryos-stage2"
"$STAGE1" compile "$COMBINED" -o "$STAGE2" 2>&1 || {
    echo "FAIL: Stage 2 compilation failed"
    echo "The stage-1 compiler could not compile itself."
    exit 1
}
info "Stage 2 binary: $STAGE2"
info "Stage 2 size: $(wc -c < "$STAGE2") bytes"

# ── Stage 3: Verify stage-1 == stage-2 ─────────────────────
log "Stage 3: Comparing stage-1 and stage-2 binaries"

HASH1=$(sha256sum "$STAGE1" | awk '{print $1}')
HASH2=$(sha256sum "$STAGE2" | awk '{print $1}')

info "Stage 1 SHA-256: $HASH1"
info "Stage 2 SHA-256: $HASH2"

if [ "$HASH1" = "$HASH2" ]; then
    echo ""
    echo "============================================"
    echo "  BOOTSTRAP VERIFIED"
    echo "  Stage-1 and stage-2 binaries are identical."
    echo "  The Kryos compiler is fully self-hosting."
    echo "============================================"
    echo ""
    echo "SHA-256: $HASH1"
    echo "Size:    $(wc -c < "$STAGE1") bytes"
    echo "Lines:   $LINES"
    exit 0
else
    echo ""
    echo "============================================"
    echo "  BOOTSTRAP MISMATCH"
    echo "  Stage-1 and stage-2 differ."
    echo "  This indicates a codegen determinism bug."
    echo "============================================"
    echo ""
    echo "Stage 1: $HASH1"
    echo "Stage 2: $HASH2"
    echo ""
    echo "Run with --verbose for details."

    # Optional: compile stage-3 and check if stage-2 == stage-3
    # (convergence after 2 iterations is still acceptable)
    log "Stage 3b: Compiling with stage-2 -> stage-3"
    STAGE3="$BUILD_DIR/kryos-stage3"
    "$STAGE2" compile "$COMBINED" -o "$STAGE3" 2>&1 || {
        echo "FAIL: Stage 3 compilation failed"
        exit 1
    }
    HASH3=$(sha256sum "$STAGE3" | awk '{print $1}')
    if [ "$HASH2" = "$HASH3" ]; then
        echo "Stage-2 == Stage-3: Compiler converges after 2 iterations."
        echo "This is acceptable (deterministic after first self-compile)."
        exit 0
    else
        echo "Stage-2 != Stage-3: Non-deterministic codegen. This is a bug."
        exit 1
    fi
fi
