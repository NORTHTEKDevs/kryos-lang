//! `kryos test` — discover and run tests in a Kryos project.

use std::path::Path;

use kryos_test_runner::{discover_tests, format_report, run_all};

/// Execute the test command.
pub fn execute(filter: Option<&str>) -> Result<(), String> {
    // Look for a tests/ directory first, fall back to current directory.
    let test_dir = if Path::new("tests").is_dir() {
        Path::new("tests")
    } else {
        Path::new(".")
    };

    let mut tests = discover_tests(test_dir);

    if let Some(f) = filter {
        tests.retain(|t| t.name.contains(f));
    }

    if tests.is_empty() {
        eprintln!("kryos test: no test files found in `{}`", test_dir.display());
        if filter.is_some() {
            eprintln!("  hint: no tests matched the filter");
        }
        return Ok(());
    }

    eprintln!("running {} test{}", tests.len(), if tests.len() == 1 { "" } else { "s" });
    eprintln!();

    let report = run_all(&tests);
    eprint!("{}", format_report(&report));

    if report.all_passed() {
        Ok(())
    } else {
        Err(format!("{} test{} failed", report.failed, if report.failed == 1 { "" } else { "s" }))
    }
}
