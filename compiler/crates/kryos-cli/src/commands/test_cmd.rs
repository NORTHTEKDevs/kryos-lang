//! `kryos test` — discover and run tests in a Kryos project.

use std::path::Path;

use kryos_test_runner::{discover_tests, format_report, run_all, run_annotated_tests};

/// Execute the test command.
pub fn execute(filter: Option<&str>) -> Result<(), String> {
    // Look for a tests/ directory first, fall back to current directory.
    let test_dir = if Path::new("tests").is_dir() {
        Path::new("tests")
    } else {
        Path::new(".")
    };

    // --- Phase 1: file-level annotation tests (// expect:, // run-expect:) ---
    let mut tests = discover_tests(test_dir);

    if let Some(f) = filter {
        tests.retain(|t| t.name.contains(f));
    }

    let mut total_failed = 0usize;

    if !tests.is_empty() {
        eprintln!(
            "running {} file test{}",
            tests.len(),
            if tests.len() == 1 { "" } else { "s" }
        );
        eprintln!();

        let report = run_all(&tests);
        eprint!("{}", format_report(&report));
        total_failed += report.failed;
    }

    // --- Phase 2: @test annotated function tests ---
    // Scan the same directory for .kry files with @test functions and JIT-run them.
    let annotated_report = run_annotated_tests(test_dir, filter);

    if annotated_report.total > 0 {
        if !tests.is_empty() {
            eprintln!(); // separator between phases
        }
        eprintln!(
            "running {} @test function{}",
            annotated_report.total,
            if annotated_report.total == 1 { "" } else { "s" }
        );
        eprintln!();
        eprint!("{}", format_report(&annotated_report));
        total_failed += annotated_report.failed;
    }

    if tests.is_empty() && annotated_report.total == 0 {
        eprintln!("kryos test: no tests found in `{}`", test_dir.display());
        if filter.is_some() {
            eprintln!("  hint: no tests matched the filter");
        }
        return Ok(());
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
