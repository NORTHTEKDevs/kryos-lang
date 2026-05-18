//! `@bench`-annotated function discovery + runner.
//!
//! Discovers functions marked `@bench` in `.kry` files, JIT-compiles each
//! module via Cranelift, then runs each bench function `warmup` times to
//! settle caches/inliner state, then `measure` times taking a wall-clock
//! sample per iteration. Reports min/median/mean/p95/max in nanoseconds.
//!
//! `kryos bench` is the user-facing entry point in `kryos-cli`; this
//! module is the engine.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kryos_codegen_cranelift::CraneliftBackend;
use kryos_driver::{compile_file, BuildConfig, OutputType};

/// One bench function's measurements.
#[derive(Debug, Clone)]
pub struct BenchResult {
    pub name: String,
    pub samples: Vec<Duration>,
    /// True if the JIT or call failed.
    pub failed: bool,
    pub failure_reason: String,
}

impl BenchResult {
    pub fn min(&self) -> Duration {
        self.samples.iter().copied().min().unwrap_or(Duration::ZERO)
    }
    pub fn max(&self) -> Duration {
        self.samples.iter().copied().max().unwrap_or(Duration::ZERO)
    }
    pub fn mean(&self) -> Duration {
        if self.samples.is_empty() {
            return Duration::ZERO;
        }
        let total: u128 = self.samples.iter().map(|d| d.as_nanos()).sum();
        Duration::from_nanos((total / self.samples.len() as u128) as u64)
    }
    pub fn median(&self) -> Duration {
        if self.samples.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted = self.samples.clone();
        sorted.sort();
        sorted[sorted.len() / 2]
    }
    pub fn p95(&self) -> Duration {
        if self.samples.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted = self.samples.clone();
        sorted.sort();
        let idx = ((sorted.len() as f64) * 0.95) as usize;
        sorted[idx.min(sorted.len() - 1)]
    }
}

/// Top-level bench report.
#[derive(Debug, Default, Clone)]
pub struct BenchReport {
    pub results: Vec<BenchResult>,
    pub total_duration: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct BenchOptions {
    pub warmup: usize,
    pub measure: usize,
}

impl Default for BenchOptions {
    fn default() -> Self {
        Self { warmup: 10, measure: 100 }
    }
}

/// Discover every `.kry` file under `dir` that contains at least one
/// `@bench`-annotated function. Returns (file, [fn_names]).
pub fn discover_annotated_benches(dir: &Path) -> Vec<(PathBuf, Vec<String>)> {
    let mut results = Vec::new();
    discover_recursive(dir, &mut results);
    results
}

fn discover_recursive(dir: &Path, results: &mut Vec<(PathBuf, Vec<String>)>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_recursive(&path, results);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("kry") {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else { continue };
        if !source.contains("@bench") {
            continue;
        }
        let mut config = BuildConfig::for_file(path.to_string_lossy().to_string());
        config.output_type = OutputType::Mir;
        let result = compile_file(&path, &config);
        if !result.success {
            continue;
        }
        if let Some(ref mir) = result.mir {
            let names: Vec<String> = mir
                .functions
                .iter()
                .filter(|f| f.attributes.bench)
                .map(|f| f.name.clone())
                .collect();
            if !names.is_empty() {
                results.push((path.to_path_buf(), names));
            }
        }
    }
}

/// Run all `@bench`-annotated functions under `dir`. Each function is
/// expected to take zero parameters and return void; the measured cost is
/// one whole call. For tighter inner-loop benching, write your loop inside
/// the function body — the engine intentionally doesn't auto-vectorize.
pub fn run_benches(
    dir: &Path,
    filter: Option<&str>,
    exact: bool,
    opts: BenchOptions,
) -> BenchReport {
    let start = Instant::now();
    let mut report = BenchReport::default();
    let discovered = discover_annotated_benches(dir);

    for (path, fn_names) in discovered {
        let mut config = BuildConfig::for_file(path.to_string_lossy().to_string());
        config.output_type = OutputType::Mir;
        let compile = compile_file(&path, &config);
        if !compile.success {
            for n in &fn_names {
                report.results.push(BenchResult {
                    name: n.clone(),
                    samples: Vec::new(),
                    failed: true,
                    failure_reason: "module compile failed".into(),
                });
            }
            continue;
        }
        let Some(mir) = &compile.mir else { continue };

        let backend = CraneliftBackend::new();
        let ptrs = match backend.jit_compile_module(mir) {
            Ok(p) => p,
            Err(e) => {
                for n in &fn_names {
                    report.results.push(BenchResult {
                        name: n.clone(),
                        samples: Vec::new(),
                        failed: true,
                        failure_reason: format!("JIT failed: {e}"),
                    });
                }
                continue;
            }
        };

        for fn_name in &fn_names {
            if let Some(f) = filter {
                let matches = if exact { fn_name == f } else { fn_name.contains(f) };
                if !matches {
                    continue;
                }
            }
            let Some(&ptr) = ptrs.get(fn_name.as_str()) else {
                report.results.push(BenchResult {
                    name: fn_name.clone(),
                    samples: Vec::new(),
                    failed: true,
                    failure_reason: "function missing from JIT output".into(),
                });
                continue;
            };

            // Safety: @bench functions are `fn()`. The MIR lowering pass
            // refuses any non-`fn()` signature, so this transmute is safe.
            let f: fn() = unsafe { std::mem::transmute(ptr) };

            // Warmup.
            for _ in 0..opts.warmup {
                f();
            }
            // Measure.
            let mut samples = Vec::with_capacity(opts.measure);
            for _ in 0..opts.measure {
                let t0 = Instant::now();
                f();
                samples.push(t0.elapsed());
            }
            report.results.push(BenchResult {
                name: fn_name.clone(),
                samples,
                failed: false,
                failure_reason: String::new(),
            });
        }
    }

    report.total_duration = start.elapsed();
    report
}

/// Pretty-print a bench report. Uses ANSI bold for the name column.
pub fn format_bench_report(report: &BenchReport) -> String {
    let mut out = String::new();
    if report.results.is_empty() {
        out.push_str("no @bench functions found\n");
        return out;
    }
    out.push_str(&format!(
        "{:<40} {:>12} {:>12} {:>12} {:>12} {:>12}\n",
        "name", "min", "median", "mean", "p95", "max"
    ));
    for r in &report.results {
        if r.failed {
            out.push_str(&format!(
                "\x1b[31m{:<40}\x1b[0m  FAILED ({})\n",
                r.name, r.failure_reason
            ));
            continue;
        }
        out.push_str(&format!(
            "\x1b[1m{:<40}\x1b[0m {:>12} {:>12} {:>12} {:>12} {:>12}\n",
            r.name,
            fmt_dur(r.min()),
            fmt_dur(r.median()),
            fmt_dur(r.mean()),
            fmt_dur(r.p95()),
            fmt_dur(r.max()),
        ));
    }
    out.push_str(&format!(
        "\n{} benchmark(s), total wall-clock {:.3}s\n",
        report.results.iter().filter(|r| !r.failed).count(),
        report.total_duration.as_secs_f64()
    ));
    out
}

fn fmt_dur(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns}ns")
    } else if ns < 1_000_000 {
        format!("{:.2}µs", ns as f64 / 1_000.0)
    } else if ns < 1_000_000_000 {
        format!("{:.2}ms", ns as f64 / 1_000_000.0)
    } else {
        format!("{:.2}s", ns as f64 / 1_000_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_dur_picks_unit() {
        assert_eq!(fmt_dur(Duration::from_nanos(42)), "42ns");
        assert_eq!(fmt_dur(Duration::from_micros(5)), "5.00µs");
        assert_eq!(fmt_dur(Duration::from_millis(2)), "2.00ms");
        assert_eq!(fmt_dur(Duration::from_secs(3)), "3.00s");
    }

    #[test]
    fn bench_result_stats() {
        let r = BenchResult {
            name: "x".into(),
            samples: vec![
                Duration::from_nanos(100),
                Duration::from_nanos(200),
                Duration::from_nanos(300),
                Duration::from_nanos(400),
                Duration::from_nanos(500),
            ],
            failed: false,
            failure_reason: String::new(),
        };
        assert_eq!(r.min(), Duration::from_nanos(100));
        assert_eq!(r.max(), Duration::from_nanos(500));
        assert_eq!(r.median(), Duration::from_nanos(300));
        assert_eq!(r.mean(), Duration::from_nanos(300));
    }
}
