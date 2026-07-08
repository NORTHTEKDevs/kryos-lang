//! End-to-end test runner: discovers all `.kry` files in `tests/e2e/` and
//! compiles them through the full pipeline (lex -> parse -> type check ->
//! ownership -> capabilities -> MIR).
//!
//! Files without annotations are expected to compile without errors.
//! Files with `// expect-error: <text>` must produce matching error diagnostics.
//! Files with `// skip` are skipped.

use std::path::Path;

use kryos_test_runner::{
    discover_tests, format_report_plain, run_all_with, RunOptions, TestOutcome,
};

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

    // These fixtures exercise the lex/parse/typecheck/codegen pipeline and
    // `// expect-error:` matching; the capability error_cases carry explicit
    // @capabilities annotations (enforced under Permissive too). Deny-by-
    // default capability policy is covered by tests/inferred_soundness.sh, so
    // pin Permissive here to avoid coupling unrelated fixtures to that policy.
    let report = run_all_with(
        &tests,
        RunOptions {
            capability_mode: kryos_driver::CapabilityMode::Permissive,
            ..RunOptions::default()
        },
    );

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
