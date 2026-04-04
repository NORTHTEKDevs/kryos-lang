#!/bin/bash
# Kryos vs Rust vs Go — Compilation Speed Benchmark
# Measures cold compile time and binary size for equivalent programs

set -e
cd "$(dirname "$0")"

KRYOS="../target/release/kryos.exe"
RUNS=5
RESULTS_FILE="benchmark_results.txt"

echo "=============================================="
echo "  KRYOS LANGUAGE BENCHMARK SUITE"
echo "  $(date)"
echo "=============================================="
echo ""

# Version info
echo "--- Compiler Versions ---"
echo "Kryos:  $($KRYOS --version 2>&1 || echo '0.1.0')"
echo "Rust:   $(rustc --version)"
echo "Go:     $(go version)"
echo ""

# ============================================
# TEST 1: Hello World — Cold Compile Time
# ============================================
echo "=== TEST 1: Hello World — Compile Time (${RUNS} runs, best of) ==="
echo ""

# Kryos
best_kryos=999
for i in $(seq 1 $RUNS); do
    rm -f bench_out_kryos.exe
    start=$(date +%s%N)
    $KRYOS build bench_hello.kry -o bench_out_kryos.exe 2>/dev/null
    end=$(date +%s%N)
    elapsed=$(( (end - start) / 1000000 ))
    if [ $elapsed -lt $best_kryos ]; then best_kryos=$elapsed; fi
done
kryos_size=$(wc -c < bench_out_kryos.exe 2>/dev/null || echo 0)
echo "Kryos:  ${best_kryos}ms  (binary: ${kryos_size} bytes)"

# Rust (debug)
best_rust_dbg=999999
for i in $(seq 1 $RUNS); do
    rm -f bench_out_rust_dbg.exe
    start=$(date +%s%N)
    rustc bench_hello.rs -o bench_out_rust_dbg.exe 2>/dev/null
    end=$(date +%s%N)
    elapsed=$(( (end - start) / 1000000 ))
    if [ $elapsed -lt $best_rust_dbg ]; then best_rust_dbg=$elapsed; fi
done
rust_dbg_size=$(wc -c < bench_out_rust_dbg.exe 2>/dev/null || echo 0)
echo "Rust:   ${best_rust_dbg}ms  (binary: ${rust_dbg_size} bytes)"

# Go
best_go=999999
for i in $(seq 1 $RUNS); do
    rm -f bench_out_go.exe
    start=$(date +%s%N)
    go build -o bench_out_go.exe bench_hello.go 2>/dev/null
    end=$(date +%s%N)
    elapsed=$(( (end - start) / 1000000 ))
    if [ $elapsed -lt $best_go ]; then best_go=$elapsed; fi
done
go_size=$(wc -c < bench_out_go.exe 2>/dev/null || echo 0)
echo "Go:     ${best_go}ms  (binary: ${go_size} bytes)"

echo ""
echo "--- Speedup vs competitors ---"
if [ $best_kryos -gt 0 ]; then
    rust_ratio=$(echo "scale=1; $best_rust_dbg / $best_kryos" | bc 2>/dev/null || echo "N/A")
    go_ratio=$(echo "scale=1; $best_go / $best_kryos" | bc 2>/dev/null || echo "N/A")
    echo "Kryos is ${rust_ratio}x faster than Rust (debug)"
    echo "Kryos is ${go_ratio}x faster than Go"
fi

echo ""

# ============================================
# TEST 2: Binary Size Comparison
# ============================================
echo "=== TEST 2: Binary Size ==="
echo ""
echo "Kryos:  $(numfmt --to=iec $kryos_size 2>/dev/null || echo "${kryos_size} bytes")"
echo "Rust:   $(numfmt --to=iec $rust_dbg_size 2>/dev/null || echo "${rust_dbg_size} bytes")"
echo "Go:     $(numfmt --to=iec $go_size 2>/dev/null || echo "${go_size} bytes")"

echo ""

# ============================================
# TEST 3: Pipeline Stage Breakdown (Kryos only)
# ============================================
echo "=== TEST 3: Kryos Pipeline Breakdown ==="
echo ""
$KRYOS build bench_hello.kry -o bench_out_kryos2.exe --verbose 2>&1 | grep -E "^\[kryos\]"
rm -f bench_out_kryos2.exe

echo ""

# ============================================
# TEST 4: Check-only speed (no codegen)
# ============================================
echo "=== TEST 4: Type Check Only Speed (${RUNS} runs, best of) ==="
echo ""

best_kryos_check=999999
for i in $(seq 1 $RUNS); do
    start=$(date +%s%N)
    $KRYOS check bench_hello.kry 2>/dev/null
    end=$(date +%s%N)
    elapsed=$(( (end - start) / 1000000 ))
    if [ $elapsed -lt $best_kryos_check ]; then best_kryos_check=$elapsed; fi
done
echo "Kryos check: ${best_kryos_check}ms"

best_rust_check=999999
for i in $(seq 1 $RUNS); do
    start=$(date +%s%N)
    rustc --edition 2021 bench_hello.rs --crate-type lib --emit=metadata -o /dev/null 2>/dev/null
    end=$(date +%s%N)
    elapsed=$(( (end - start) / 1000000 ))
    if [ $elapsed -lt $best_rust_check ]; then best_rust_check=$elapsed; fi
done
echo "Rust check:  ${best_rust_check}ms"

best_go_vet=999999
for i in $(seq 1 $RUNS); do
    start=$(date +%s%N)
    go vet bench_hello.go 2>/dev/null
    end=$(date +%s%N)
    elapsed=$(( (end - start) / 1000000 ))
    if [ $elapsed -lt $best_go_vet ]; then best_go_vet=$elapsed; fi
done
echo "Go vet:      ${best_go_vet}ms"

echo ""

# ============================================
# Cleanup
# ============================================
rm -f bench_out_kryos.exe bench_out_rust_dbg.exe bench_out_go.exe bench_out_kryos2.exe

echo "=============================================="
echo "  BENCHMARK COMPLETE"
echo "=============================================="
