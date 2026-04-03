#!/bin/bash
echo "=== Starting test run ==="
echo "Working directory: $(pwd)"
echo "=== Killing stuck processes ==="
taskkill //F //IM "cargo.exe" 2>/dev/null || true
taskkill //F //IM "kryos_codegen_cranelift" 2>/dev/null || true
sleep 3
echo "=== Running cargo test ==="
cd /c/Users/Krist/projects/active/kryos-lang/compiler
cargo test --workspace --exclude kryos-codegen-cranelift 2>&1
echo "=== EXIT CODE: $? ==="
echo "=== Test run complete ==="
