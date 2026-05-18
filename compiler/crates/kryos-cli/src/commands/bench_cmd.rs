//! `kryos bench` — discover and run `@bench`-annotated functions.

use std::path::Path;

use kryos_test_runner::{format_bench_report, run_benches, BenchOptions};

#[derive(Debug, Clone, Default)]
pub struct BenchCliOptions {
    /// Optional positional filter (substring or exact match).
    pub filter: Option<String>,
    /// If true, only run benches whose name equals the filter exactly.
    pub exact: bool,
    /// Warmup iterations before measurement.
    pub warmup: Option<usize>,
    /// Measurement iterations.
    pub measure: Option<usize>,
    /// Path to a `.kry` file or directory. Defaults to `benches/` or current dir.
    pub path: Option<String>,
}

pub fn execute(opts: BenchCliOptions) -> Result<(), String> {
    let dir_buf = match &opts.path {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            if Path::new("benches").is_dir() {
                std::path::PathBuf::from("benches")
            } else if Path::new("tests").is_dir() {
                std::path::PathBuf::from("tests")
            } else {
                std::path::PathBuf::from(".")
            }
        }
    };
    if !dir_buf.exists() {
        return Err(format!("kryos bench: path '{}' does not exist", dir_buf.display()));
    }

    let bench_opts = BenchOptions {
        warmup: opts.warmup.unwrap_or(10),
        measure: opts.measure.unwrap_or(100),
    };

    eprintln!(
        "kryos bench: warmup={}, measure={}, root={}",
        bench_opts.warmup,
        bench_opts.measure,
        dir_buf.display()
    );
    eprintln!();

    let report = run_benches(&dir_buf, opts.filter.as_deref(), opts.exact, bench_opts);
    print!("{}", format_bench_report(&report));

    let failed = report.results.iter().filter(|r| r.failed).count();
    if failed > 0 {
        Err(format!("{failed} benchmark(s) failed"))
    } else {
        Ok(())
    }
}
