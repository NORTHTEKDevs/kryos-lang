//! End-to-end test runner: discovers all `.kry` files in `tests/e2e/` and
//! compiles them through the full pipeline (lex -> parse -> type check ->
//! ownership -> capabilities -> MIR).
//!
//! Files without annotations are expected to compile without errors.
//! Files with `// expect-error: <text>` must produce matching error diagnostics.
//! Files with `// skip` are skipped.

use std::path::Path;

use kryos_test_runner::{discover_tests, format_report_plain, run_all, TestOutcome};

#[test]
fn e2e_test_suite() {
    let test_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("e2e");

    let tests = discover_tests(&test_dir);
    assert!(
        !tests.is_empty(),
        "no .kry test files found in {}",
        test_dir.display()
    );

    let report = run_all(&tests);

    // Print full report for visibility in cargo test output.
    eprintln!("\n{}", format_report_plain(&report));

    // Collect failures for assertion message.
    let failures: Vec<_> = report
        .results
        .iter()
        .filter_map(|r| match &r.outcome {
            TestOutcome::Failed { reason } => Some(format!("FAIL {}: {}", r.name, reason)),
            _ => None,
        })
        .collect();

    assert!(
        report.all_passed(),
        "{} test(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
