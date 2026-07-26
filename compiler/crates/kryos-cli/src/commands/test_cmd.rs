//! `kryos test` — discover and run tests in a Kryos project.
//!
//! Provides cargo-test–style ergonomics: a positional filter, `--exact`
//! matching, `--nocapture` to surface stdout from `// run-expect:` tests,
//! `--format=json` for CI consumption, and `--list` to enumerate discovered
//! test names without running them.

use std::path::Path;

use kryos_test_runner::{
    discover_annotated_tests, discover_annotated_tests_in_file, discover_tests,
    discover_tests_in_file, format_report, format_report_json, format_summary, run_all_with,
    run_annotated_tests_in_file, run_annotated_tests_in_file_streaming, run_annotated_tests_with,
    run_annotated_tests_with_streaming, Expectation, RunOptions, TestReport, TestResult,
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
#[derive(Debug, Clone)]
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
    /// Optional path to a `.kry` file or directory. When `None`, defaults to
    /// `tests/` if it exists, otherwise the current directory.
    pub path: Option<String>,
    /// Capability enforcement mode for the test compile. Defaults to
    /// `Inferred` so `kryos test` is a real capability-regression gate.
    pub capability_mode: kryos_driver::CapabilityMode,
}

impl Default for TestOptions {
    fn default() -> Self {
        Self {
            filter: None,
            exact: false,
            nocapture: false,
            format: OutputFormat::default(),
            list: false,
            path: None,
            capability_mode: kryos_driver::CapabilityMode::Inferred,
        }
    }
}

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::Pretty
    }
}

/// Execute the test command with the given options.
pub fn execute(opts: TestOptions) -> Result<(), String> {
    // Resolve the discovery root.
    //
    // Precedence:
    //   1. `opts.path` if provided — may be a file or directory.
    //   2. `tests/` if it exists in the current directory.
    //   3. current directory.
    //
    // When the user passes a single `.kry` file, we use file-specific
    // discovery so we don't pick up unrelated tests in sibling files.
    let (test_dir_buf, single_file): (std::path::PathBuf, Option<std::path::PathBuf>) =
        match &opts.path {
            Some(p) => {
                let pb = std::path::PathBuf::from(p);
                if pb.is_file() {
                    let parent = pb.parent().map(|q| q.to_path_buf()).unwrap_or_else(|| {
                        std::path::PathBuf::from(".")
                    });
                    (parent, Some(pb))
                } else if pb.is_dir() {
                    (pb, None)
                } else {
                    return Err(format!(
                        "kryos test: path '{}' does not exist",
                        pb.display()
                    ));
                }
            }
            None => {
                if Path::new("tests").is_dir() {
                    (std::path::PathBuf::from("tests"), None)
                } else {
                    (std::path::PathBuf::from("."), None)
                }
            }
        };
    let test_dir: &Path = test_dir_buf.as_path();

    let filter_opt: Option<&str> = opts.filter.as_deref();

    // ---- --list mode: enumerate and exit ----
    if opts.list {
        return list_tests(test_dir, single_file.as_deref(), filter_opt, opts.exact);
    }

    // ---- Phase 1: file-level annotation tests (`// expect:` / `// run-expect:`) ----
    let mut tests = match &single_file {
        Some(f) => discover_tests_in_file(f),
        None => discover_tests(test_dir),
    };
    if let Some(f) = filter_opt {
        if opts.exact {
            tests.retain(|t| t.name == f);
        } else {
            tests.retain(|t| t.name.contains(f));
        }
    }

    // Drop phantom "file tests" that verify nothing. A `.kry` file with no
    // `// expect:` / `// run-expect:` / `// expect-error:` directive parses
    // into an empty `Output` expectation that "passes" merely by compiling —
    // the documented false-green (see ecosystem/kryos-rag/README.md, where a
    // file with no @test functions reports a vacuous "1 passed" even when an
    // assertion fails). Such a file asserts nothing, so it must not count as a
    // discovered test or keep the exit code green.
    tests.retain(|t| match &t.expectation {
        Expectation::Output(lines) => !lines.is_empty(),
        Expectation::Error(_) | Expectation::RunOutput(_) => true,
    });

    let run_opts = RunOptions {
        nocapture: opts.nocapture,
        capability_mode: opts.capability_mode,
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
        let report = run_all_with(&tests, run_opts);
        // Print file-test results now, before the in-process @test phase runs:
        // a runtime panic inside a @test function is process-fatal and would
        // otherwise swallow this phase's output along with its own.
        if opts.format == OutputFormat::Pretty {
            eprint!("{}", format_report(&report));
        }
        report
    };

    // ---- Phase 2: `@test`-annotated function tests ----
    // In Pretty mode, stream each result as it runs (panic-safe): the header
    // and per-test lines are emitted by the runner; only the summary is printed
    // below. JSON mode buffers (it emits one merged suite object at the end).
    let stream_annotated = opts.format == OutputFormat::Pretty;
    let sep_before = stream_annotated && !tests.is_empty();
    let annotated_report = match (&single_file, stream_annotated) {
        (Some(f), true) => {
            run_annotated_tests_in_file_streaming(f, filter_opt, opts.exact, sep_before)
        }
        (Some(f), false) => run_annotated_tests_in_file(f, filter_opt, opts.exact),
        (None, true) => run_annotated_tests_with_streaming(test_dir, filter_opt, opts.exact, sep_before),
        (None, false) => run_annotated_tests_with(test_dir, filter_opt, opts.exact),
    };

    if tests.is_empty() && annotated_report.total == 0 {
        // Nothing was verified. A zero-test invocation must NOT be mistaken
        // for a pass: warn and return a non-zero exit so CI fails loudly
        // instead of treating "it compiled" as a green test run.
        let target = single_file
            .as_deref()
            .map(|f| f.display().to_string())
            .unwrap_or_else(|| test_dir.display().to_string());
        let noun = if single_file.is_some() {
            "@test functions"
        } else {
            "tests"
        };

        if opts.format == OutputFormat::Json {
            // Still emit a parseable empty suite for tooling, then fail.
            print!("{}", format_report_json(&empty_report()));
        } else {
            if kryos_test_runner::discovery_hit_compile_error() {
                // The real cause was already printed by discovery; do not also
                // claim the file contains no tests.
                return Err("nothing was verified -- fix the compile error above".to_string());
            }
            eprintln!("warning: no {noun} found in `{target}`; nothing was verified");
            if filter_opt.is_some() {
                eprintln!("  note: a filter was active and matched nothing");
            }
        }
        return Err(format!(
            "no {noun} discovered in `{target}`; nothing was verified"
        ));
    }

    let total_failed = file_report.failed + annotated_report.failed;

    match opts.format {
        OutputFormat::Pretty => {
            // File-test results were printed after Phase 1; the @test header
            // and per-test lines were streamed by the runner as each ran. Only
            // the @test summary tail remains to print.
            if annotated_report.total > 0 {
                eprint!("{}", format_summary(&annotated_report));
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
fn list_tests(
    test_dir: &Path,
    single_file: Option<&Path>,
    filter: Option<&str>,
    exact: bool,
) -> Result<(), String> {
    let mut names: Vec<String> = Vec::new();

    let file_tests = match single_file {
        Some(f) => discover_tests_in_file(f),
        None => discover_tests(test_dir),
    };
    for t in &file_tests {
        names.push(t.name.clone());
    }

    let annotated = match single_file {
        Some(f) => discover_annotated_tests_in_file(f),
        None => discover_annotated_tests(test_dir),
    };
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

    #[test]
    fn zero_tests_in_file_returns_error() {
        // A `.kry` file with no `@test` functions and no `// expect:` directive
        // must NOT be a vacuous pass: `execute` has to return Err so the CLI
        // exits non-zero and CI cannot mistake "it compiled" for a green run.
        // (This path never invokes the compiler — discovery short-circuits on
        // the absence of `@test`, so the test is fast and hermetic.)
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("kryos_zero_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let file = dir.join("no_tests.kry");
        {
            let mut f = std::fs::File::create(&file).expect("create temp .kry");
            writeln!(f, "fn main() {{").unwrap();
            writeln!(f, "    println(\"no tests here\")").unwrap();
            writeln!(f, "}}").unwrap();
        }

        let result = execute(TestOptions {
            path: Some(file.to_string_lossy().into_owned()),
            ..Default::default()
        });

        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            result.is_err(),
            "a file with zero @test functions must return Err, got: {result:?}"
        );
    }
}
