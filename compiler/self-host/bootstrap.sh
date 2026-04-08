#!/bin/bash
# ============================================================
# bootstrap.sh -- Bootstrap verification for the Kryos
# self-hosted compiler.
#
# This script verifies the self-hosting chain:
#   1. Compile the self-host with the Rust-based compiler (stage-0 -> stage-1)
#   2. Compile the self-host with stage-1 (stage-1 -> stage-2)
#   3. Compile the self-host with stage-2 (stage-2 -> stage-3)
#   4. Compare stage-2 and stage-3 binaries (must be identical)
#
# Stage-1 is compiled by a different compiler (Rust/Cranelift), so
# stage-1 != stage-2. But stage-2 and stage-3 are both compiled by
# the same compiler (the self-host), so stage-2 == stage-3 proves
# the compiler faithfully reproduces itself.
#
# This is the same technique used by GCC, Rust, and Go.
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

# ── Combine source files ─────────────────────────────────────
# Stage-0 source: core compiler without runtime (Rust provides builtins)
STAGE0_SRC="$BUILD_DIR/kryos-sh.kry"
log "Combining self-host source files (stage-0, no runtime)"
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
    > "$STAGE0_SRC"

# Stage-1+ source: compiler with runtime (self-contained binary)
SELFHOST_SRC="$BUILD_DIR/kryos-sh-full.kry"
log "Combining self-host source files (stage-1+, with runtime)"
cat \
    "$SELFHOST_DIR/runtime.kry" \
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
    > "$SELFHOST_SRC"

LINES0=$(wc -l < "$STAGE0_SRC")
LINES1=$(wc -l < "$SELFHOST_SRC")
info "Stage-0 source: $LINES0 lines"
info "Full source: $LINES1 lines"

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
"$KRYOS_RS" build "$STAGE0_SRC" -o "$STAGE1" --skip-ownership 2>&1 || {
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
"$STAGE1" compile "$SELFHOST_SRC" -o "$STAGE2" 2>&1 || {
    echo "FAIL: Stage 2 compilation failed"
    echo "The stage-1 compiler could not compile itself."
    exit 1
}
info "Stage 2 binary: $STAGE2"
info "Stage 2 size: $(wc -c < "$STAGE2") bytes"

# ── Stage 3: Compile self-host with stage-2 ────────────────
log "Stage 3: Compiling self-host with stage-2 -> stage-3"
STAGE3="$BUILD_DIR/kryos-stage3"
"$STAGE2" compile "$SELFHOST_SRC" -o "$STAGE3" 2>&1 || {
    echo "FAIL: Stage 3 compilation failed"
    echo "The stage-2 compiler could not compile itself."
    exit 1
}
info "Stage 3 binary: $STAGE3"
info "Stage 3 size: $(wc -c < "$STAGE3") bytes"

# ── Verify: stage-2 == stage-3 ─────────────────────────────
log "Verification: Comparing stage-2 and stage-3 binaries"

HASH2=$(sha256sum "$STAGE2" | awk '{print $1}')
HASH3=$(sha256sum "$STAGE3" | awk '{print $1}')

info "Stage 2 SHA-256: $HASH2"
info "Stage 3 SHA-256: $HASH3"

if [ "$HASH2" = "$HASH3" ]; then
    echo ""
    echo "============================================"
    echo "  BOOTSTRAP VERIFIED"
    echo "  Stage-2 and stage-3 binaries are identical."
    echo "  The Kryos compiler is fully self-hosting."
    echo "============================================"
    echo ""
    echo "SHA-256: $HASH2"
    echo "Size:    $(wc -c < "$STAGE2") bytes"
    echo "Source:  $LINES1 lines (16 files)"
    echo ""
    echo "Chain:"
    echo "  stage-0 (Rust/Cranelift) -> stage-1  ($LINES0 lines)"
    echo "  stage-1 (Kryos)          -> stage-2  ($LINES1 lines)"
    echo "  stage-2 (Kryos)          -> stage-3  ($LINES1 lines)"
    echo "  stage-2 == stage-3  [PASS]"
    exit 0
else
    echo ""
    echo "============================================"
    echo "  BOOTSTRAP MISMATCH"
    echo "  Stage-2 and stage-3 differ."
    echo "  This indicates a codegen determinism bug."
    echo "============================================"
    echo ""
    echo "Stage 2: $HASH2"
    echo "Stage 3: $HASH3"
    echo ""
    echo "Run with --verbose for details."
    exit 1
fi
