# Kryos Runtime Performance Benchmarks
# Compares Kryos (debug + release) vs Rust (debug + release) vs Go
# Run from: compiler/ directory
# Usage: powershell -ExecutionPolicy Bypass -File tests/run_perf_benchmarks.ps1

$ErrorActionPreference = "Continue"
$testDir = "tests"
$runs = 3

Write-Host "=== Kryos Runtime Performance Benchmarks ===" -ForegroundColor Cyan
Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Write-Host "Runs per benchmark: $runs"
Write-Host ""

# Compiler versions
Write-Host "--- Compiler Versions ---" -ForegroundColor Yellow
rustc --version 2>&1
go version 2>&1
Write-Host ""

# Helper: time a command N times, return average ms
function Measure-Average {
    param([string]$exe, [int]$count = 3, [int]$timeoutSec = 60)
    $times = @()
    for ($i = 0; $i -lt $count; $i++) {
        $job = Start-Job -ScriptBlock {
            param($e)
            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            & $e 2>&1 | Out-Null
            $sw.Stop()
            $sw.Elapsed.TotalMilliseconds
        } -ArgumentList $exe
        $result = Wait-Job $job -Timeout $timeoutSec
        if ($result) {
            $ms = Receive-Job $job
            $times += $ms
            Remove-Job $job -Force
        } else {
            Stop-Job $job -Force
            Remove-Job $job -Force
            Write-Host "  TIMEOUT after ${timeoutSec}s" -ForegroundColor Red
            return "TIMEOUT"
        }
    }
    $avg = ($times | Measure-Object -Average).Average
    return [math]::Round($avg, 1)
}

# ============================================================
# COMPILATION
# ============================================================
Write-Host "=== Compiling Benchmarks ===" -ForegroundColor Cyan

# --- Fibonacci ---
Write-Host "`n--- Fibonacci (fib(35), recursive) ---" -ForegroundColor Yellow

Write-Host "  Kryos debug..."
cargo run -- build "$testDir/perf_fib.kry" -o "$testDir/perf_fib_k_dbg.exe" 2>&1 | Select-Object -Last 3
Write-Host "  Kryos release..."
cargo run -- build --release "$testDir/perf_fib.kry" -o "$testDir/perf_fib_k_rel.exe" 2>&1 | Select-Object -Last 3
Write-Host "  Rust debug..."
rustc "$testDir/perf_fib.rs" -o "$testDir/perf_fib_r_dbg.exe" 2>&1
Write-Host "  Rust release..."
rustc -O "$testDir/perf_fib.rs" -o "$testDir/perf_fib_r_rel.exe" 2>&1
Write-Host "  Go..."
go build -o "$testDir/perf_fib_g.exe" "$testDir/perf_fib.go" 2>&1

# --- Sum Loop ---
Write-Host "`n--- Sum Loop (0..100M) ---" -ForegroundColor Yellow

Write-Host "  Kryos debug..."
cargo run -- build "$testDir/perf_sum.kry" -o "$testDir/perf_sum_k_dbg.exe" 2>&1 | Select-Object -Last 3
Write-Host "  Kryos release..."
cargo run -- build --release "$testDir/perf_sum.kry" -o "$testDir/perf_sum_k_rel.exe" 2>&1 | Select-Object -Last 3
Write-Host "  Rust debug..."
rustc "$testDir/perf_sum.rs" -o "$testDir/perf_sum_r_dbg.exe" 2>&1
Write-Host "  Rust release..."
rustc -O "$testDir/perf_sum.rs" -o "$testDir/perf_sum_r_rel.exe" 2>&1
Write-Host "  Go..."
go build -o "$testDir/perf_sum_g.exe" "$testDir/perf_sum.go" 2>&1

# --- Nested Loops ---
Write-Host "`n--- Nested Loops (1000x1000 multiply) ---" -ForegroundColor Yellow

Write-Host "  Kryos debug..."
cargo run -- build "$testDir/perf_nested.kry" -o "$testDir/perf_nested_k_dbg.exe" 2>&1 | Select-Object -Last 3
Write-Host "  Kryos release..."
cargo run -- build --release "$testDir/perf_nested.kry" -o "$testDir/perf_nested_k_rel.exe" 2>&1 | Select-Object -Last 3
Write-Host "  Rust debug..."
rustc "$testDir/perf_nested.rs" -o "$testDir/perf_nested_r_dbg.exe" 2>&1
Write-Host "  Rust release..."
rustc -O "$testDir/perf_nested.rs" -o "$testDir/perf_nested_r_rel.exe" 2>&1
Write-Host "  Go..."
go build -o "$testDir/perf_nested_g.exe" "$testDir/perf_nested.go" 2>&1

# ============================================================
# BENCHMARKING
# ============================================================
Write-Host "`n=== Running Benchmarks ($runs runs each) ===" -ForegroundColor Cyan

$results = @{}

$benchmarks = @("fib", "sum", "nested")
$variants = @(
    @{name="Kryos debug"; suffix="_k_dbg"},
    @{name="Kryos release"; suffix="_k_rel"},
    @{name="Rust debug"; suffix="_r_dbg"},
    @{name="Rust release"; suffix="_r_rel"},
    @{name="Go"; suffix="_g"}
)

foreach ($bench in $benchmarks) {
    Write-Host "`n--- $bench ---" -ForegroundColor Yellow
    foreach ($v in $variants) {
        $exe = "$testDir/perf_${bench}$($v.suffix).exe"
        if (Test-Path $exe) {
            Write-Host "  $($v.name)... " -NoNewline
            $avg = Measure-Average -exe $exe -count $runs -timeoutSec 60
            Write-Host "${avg}ms"
            $results["${bench}_$($v.name)"] = $avg
        } else {
            Write-Host "  $($v.name)... MISSING (compilation failed)" -ForegroundColor Red
            $results["${bench}_$($v.name)"] = "FAILED"
        }
    }
}

# ============================================================
# REPORT
# ============================================================
Write-Host "`n`n=== RESULTS SUMMARY ===" -ForegroundColor Green

$header = "{0,-20} {1,15} {2,15} {3,15} {4,15} {5,15}" -f "Benchmark", "Kryos dbg", "Kryos rel", "Rust dbg", "Rust rel", "Go"
Write-Host $header
Write-Host ("-" * 95)

foreach ($bench in $benchmarks) {
    $kd = $results["${bench}_Kryos debug"]
    $kr = $results["${bench}_Kryos release"]
    $rd = $results["${bench}_Rust debug"]
    $rr = $results["${bench}_Rust release"]
    $g  = $results["${bench}_Go"]

    $row = "{0,-20} {1,15} {2,15} {3,15} {4,15} {5,15}" -f $bench, "${kd}ms", "${kr}ms", "${rd}ms", "${rr}ms", "${g}ms"
    Write-Host $row
}

Write-Host "`nDone. Use these results to populate PERF_REPORT.md" -ForegroundColor Cyan
