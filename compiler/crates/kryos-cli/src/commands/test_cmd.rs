//! `kryos test` — discover and run tests in a Kryos project.
//!
//! Provides cargo-test–style ergonomics: a positional filter, `--exact`
//! matching, `--nocapture` to surface stdout from `// run-expect:` tests,
//! `--format=json` for CI consumption, and `--list` to enumerate discovered
//! test names without running them.

use std::path::Path;

use kryos_test_runner::{
    discover_annotated_tests, discover_tests, format_report, format_report_json, run_all_with,
    run_annotated_tests_with, RunOptions, TestReport, TestResult,
};

/// Report format selector for `kryos test`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable colored output (the historical default).
    Pretty,
    /// One JSON object per line, suitable for CI pipelines and IDE integration.
    Json,
}

/// User-facing options for `kryos test`.
#[derive(Debug, Clone, Default)]
pub struct TestOptions {
    /// Optional filter pattern (positional FILTER or `--filter`).
    pub filter: Option<String>,
    /// If `true`, the filter must match the test name exactly.
    pub exact: bool,
    /// Forward child-process stdout/stderr (for `// run-expect:` tests).
    pub nocapture: bool,
    /// Output format.
    pub format: OutputFormat,
    /// Just list discovered test names and exit.
    pub list: bool,
}

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::Pretty
    }
}

/// Execute the test command with the given options.
pub fn execute(opts: TestOptions) -> Result<(), String> {
    // Look for a tests/ directory first, fall back to current directory.
    let test_dir = if Path::new("tests").is_dir() {
        Path::new("tests")
    } else {
        Path::new(".")
    };

    let filter_opt: Option<&str> = opts.filter.as_deref();

    // ---- --list mode: enumerate and exit ----
    if opts.list {
        return list_tests(test_dir, filter_opt, opts.exact);
    }

    // ---- Phase 1: file-level annotation tests (`// expect:` / `// run-expect:`) ----
    let mut tests = discover_tests(test_dir);
    if let Some(f) = filter_opt {
        if opts.exact {
            tests.retain(|t| t.name == f);
        } else {
            tests.retain(|t| t.name.contains(f));
        }
    }

    let run_opts = RunOptions {
        nocapture: opts.nocapture,
    };

    let file_report = if tests.is_empty() {
        empty_report()
    } else {
        if opts.format == OutputFormat::Pretty {
            eprintln!(
                "running {} file test{}",
                tests.len(),
                if tests.len() == 1 { "" } else { "s" }
            );
            eprintln!();
        }
        run_all_with(&tests, run_opts)
    };

    // ---- Phase 2: `@test`-annotated function tests ----
    let annotated_report = run_annotated_tests_with(test_dir, filter_opt, opts.exact);

    if tests.is_empty() && annotated_report.total == 0 {
        if opts.format == OutputFormat::Json {
            // Emit a zero-test JSON suite for parser-friendliness.
            print!("{}", format_report_json(&empty_report()));
            return Ok(());
        }
        eprintln!("kryos test: no tests found in `{}`", test_dir.display());
        if filter_opt.is_some() {
            eprintln!("  hint: no tests matched the filter");
        }
        return Ok(());
    }

    let total_failed = file_report.failed + annotated_report.failed;

    match opts.format {
        OutputFormat::Pretty => {
            if !tests.is_empty() {
                eprint!("{}", format_report(&file_report));
            }
            if annotated_report.total > 0 {
                if !tests.is_empty() {
                    eprintln!();
                }
                eprintln!(
                    "running {} @test function{}",
                    annotated_report.total,
                    if annotated_report.total == 1 { "" } else { "s" }
                );
                eprintln!();
                eprint!("{}", format_report(&annotated_report));
            }
        }
        OutputFormat::Json => {
            // Merge both phases into a single suite report for JSON output.
            let combined = merge_reports(&[file_report, annotated_report]);
            print!("{}", format_report_json(&combined));
        }
    }

    if total_failed > 0 {
        Err(format!(
            "{total_failed} test{} failed",
            if total_failed == 1 { "" } else { "s" }
        ))
    } else {
        Ok(())
    }
}

fn empty_report() -> TestReport {
    TestReport {
        total: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
        duration: std::time::Duration::ZERO,
        results: Vec::new(),
    }
}

fn merge_reports(reports: &[TestReport]) -> TestReport {
    let mut results: Vec<TestResult> = Vec::new();
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut total = 0;
    let mut duration = std::time::Duration::ZERO;
    for r in reports {
        results.extend(r.results.iter().cloned());
        passed += r.passed;
        failed += r.failed;
        skipped += r.skipped;
        total += r.total;
        duration += r.duration;
    }
    TestReport {
        total,
        passed,
        failed,
        skipped,
        duration,
        results,
    }
}

/// Implement `--list`: enumerate file-test names and `@test` function names.
fn list_tests(test_dir: &Path, filter: Option<&str>, exact: bool) -> Result<(), String> {
    let mut names: Vec<String> = Vec::new();

    let file_tests = discover_tests(test_dir);
    for t in &file_tests {
        names.push(t.name.clone());
    }

    let annotated = discover_annotated_tests(test_dir);
    for (_path, fns) in &annotated {
        for n in fns {
            names.push(n.clone());
        }
    }

    if let Some(f) = filter {
        names.retain(|n| if exact { n == f } else { n.contains(f) });
    }

    names.sort();
    names.dedup();

    for n in &names {
        println!("{n}");
    }

    if names.is_empty() {
        eprintln!("kryos test: no tests found in `{}`", test_dir.display());
    } else {
        eprintln!("{} test{} listed", names.len(), if names.len() == 1 { "" } else { "s" });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_default_is_pretty() {
        let f: OutputFormat = Default::default();
        assert_eq!(f, OutputFormat::Pretty);
    }

    #[test]
    fn test_options_default_is_empty() {
        let o: TestOptions = Default::default();
        assert!(o.filter.is_none());
        assert!(!o.exact);
        assert!(!o.nocapture);
        assert!(!o.list);
        assert_eq!(o.format, OutputFormat::Pretty);
    }

    #[test]
    fn merge_reports_sums_counters() {
        let a = TestReport {
            total: 3,
            passed: 2,
            failed: 1,
            skipped: 0,
            duration: std::time::Duration::from_millis(10),
            results: Vec::new(),
        };
        let b = TestReport {
            total: 2,
            passed: 2,
            failed: 0,
            skipped: 0,
            duration: std::time::Duration::from_millis(5),
            results: Vec::new(),
        };
        let merged = merge_reports(&[a, b]);
        assert_eq!(merged.total, 5);
        assert_eq!(merged.passed, 4);
        assert_eq!(merged.failed, 1);
        assert_eq!(merged.skipped, 0);
        assert_eq!(merged.duration, std::time::Duration::from_millis(15));
    }
}
