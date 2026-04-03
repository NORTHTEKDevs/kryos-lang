//! Kryos test runner — discovers and runs `// expect:` test programs.
//!
//! Scans directories for `.kry` files containing `// expect:` annotations,
//! compiles them using the Kryos driver, and verifies the compiler output
//! matches the expected results.
//!
//! Supported annotations:
//! - `// expect: <text>` — the compiler should succeed and output should contain `<text>`
//! - `// expect-error: <text>` — the compiler should produce an error containing `<text>`
//! - `// skip` — skip this test file entirely

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kryos_driver::{BuildConfig, compile_source, render_diagnostics};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// The expected outcome of a test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expectation {
    /// The compiler should succeed. Expected output lines (from `// expect:`).
    Output(Vec<String>),
    /// The compiler should produce errors containing these strings (from `// expect-error:`).
    Error(Vec<String>),
}

/// A single test case discovered from a `.kry` file.
#[derive(Debug, Clone)]
pub struct TestCase {
    /// Human-readable test name (derived from file path).
    pub name: String,
    /// Path to the `.kry` source file.
    pub source_path: PathBuf,
    /// The source code contents.
    pub source: String,
    /// What we expect the compiler to produce.
    pub expectation: Expectation,
    /// Whether this test should be skipped (`// skip` annotation).
    pub skip: bool,
}

/// The outcome of running a single test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    Passed,
    Failed { reason: String },
    Skipped,
}

/// Result of running a single test case.
#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub outcome: TestOutcome,
    pub duration: Duration,
    pub actual_output: String,
}

/// Aggregate report over a test run.
#[derive(Debug, Clone)]
pub struct TestReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration: Duration,
    pub results: Vec<TestResult>,
}

impl TestReport {
    /// Returns `true` if all non-skipped tests passed.
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

// ---------------------------------------------------------------------------
// Test discovery
// ---------------------------------------------------------------------------

/// Parse test annotations from Kryos source code.
///
/// Looks for lines starting with `// expect:`, `// expect-error:`, and `// skip`.
fn parse_annotations(source: &str) -> (Expectation, bool) {
    let mut expect_lines: Vec<String> = Vec::new();
    let mut expect_error_lines: Vec<String> = Vec::new();
    let mut skip = false;

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed == "// skip" {
            skip = true;
        } else if let Some(rest) = trimmed.strip_prefix("// expect-error:") {
            expect_error_lines.push(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("// expect:") {
            expect_lines.push(rest.trim().to_string());
        }
    }

    let expectation = if !expect_error_lines.is_empty() {
        Expectation::Error(expect_error_lines)
    } else {
        Expectation::Output(expect_lines)
    };

    (expectation, skip)
}

/// Derive a test name from a file path relative to a base directory.
fn derive_test_name(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches(".kry")
        .to_string()
}

/// Discover all `.kry` test files in a directory tree.
///
/// Recursively walks `dir`, reads each `.kry` file, and parses its
/// `// expect:` / `// expect-error:` / `// skip` annotations into `TestCase`s.
pub fn discover_tests(dir: &Path) -> Vec<TestCase> {
    let mut tests = Vec::new();
    discover_tests_recursive(dir, dir, &mut tests);
    tests.sort_by(|a, b| a.name.cmp(&b.name));
    tests
}

fn discover_tests_recursive(dir: &Path, base: &Path, tests: &mut Vec<TestCase>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_tests_recursive(&path, base, tests);
        } else if path.extension().and_then(|e| e.to_str()) == Some("kry") {
            if let Ok(source) = fs::read_to_string(&path) {
                let (expectation, skip) = parse_annotations(&source);
                let name = derive_test_name(&path, base);
                tests.push(TestCase {
                    name,
                    source_path: path,
                    source,
                    expectation,
                    skip,
                });
            }
        }
    }
}

/// Discover test cases from source strings (useful for testing the runner itself).
pub fn test_case_from_source(name: &str, source: &str) -> TestCase {
    let (expectation, skip) = parse_annotations(source);
    TestCase {
        name: name.to_string(),
        source_path: PathBuf::from(format!("{}.kry", name)),
        source: source.to_string(),
        expectation,
        skip,
    }
}

// ---------------------------------------------------------------------------
// Test execution
// ---------------------------------------------------------------------------

/// Run a single test case through the compiler and check the result.
pub fn run_test(test: &TestCase) -> TestResult {
    if test.skip {
        return TestResult {
            name: test.name.clone(),
            outcome: TestOutcome::Skipped,
            duration: Duration::ZERO,
            actual_output: String::new(),
        };
    }

    let start = Instant::now();
    let config = BuildConfig::for_file(&test.source_path.to_string_lossy().to_string());
    let result = compile_source(&test.source, &test.name, &config);
    let duration = start.elapsed();

    let diagnostics_text = render_diagnostics(&result);
    let has_errors = result.diagnostics.iter().any(|d| d.is_error());

    let outcome = match &test.expectation {
        Expectation::Output(expected_lines) => {
            if has_errors {
                TestOutcome::Failed {
                    reason: format!(
                        "expected success but got errors:\n{}",
                        diagnostics_text
                    ),
                }
            } else if expected_lines.is_empty() {
                // No specific output expected, just that it compiles
                TestOutcome::Passed
            } else {
                // Check that all expected lines appear in output
                let mut missing: Vec<String> = Vec::new();
                for expected in expected_lines {
                    if !diagnostics_text.contains(expected) {
                        missing.push(expected.clone());
                    }
                }
                if missing.is_empty() {
                    TestOutcome::Passed
                } else {
                    TestOutcome::Failed {
                        reason: format!(
                            "expected output containing {:?}, got:\n{}",
                            missing, diagnostics_text
                        ),
                    }
                }
            }
        }
        Expectation::Error(expected_errors) => {
            if !has_errors {
                TestOutcome::Failed {
                    reason: "expected compile error but compilation succeeded".to_string(),
                }
            } else {
                let mut missing: Vec<String> = Vec::new();
                for expected in expected_errors {
                    if !diagnostics_text.contains(expected) {
                        missing.push(expected.clone());
                    }
                }
                if missing.is_empty() {
                    TestOutcome::Passed
                } else {
                    TestOutcome::Failed {
                        reason: format!(
                            "expected errors containing {:?}, got:\n{}",
                            missing, diagnostics_text
                        ),
                    }
                }
            }
        }
    };

    TestResult {
        name: test.name.clone(),
        outcome,
        duration,
        actual_output: diagnostics_text,
    }
}

/// Run all test cases and produce an aggregate report.
pub fn run_all(tests: &[TestCase]) -> TestReport {
    let start = Instant::now();
    let mut results = Vec::with_capacity(tests.len());
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    for test in tests {
        let result = run_test(test);
        match result.outcome {
            TestOutcome::Passed => passed += 1,
            TestOutcome::Failed { .. } => failed += 1,
            TestOutcome::Skipped => skipped += 1,
        }
        results.push(result);
    }

    TestReport {
        total: tests.len(),
        passed,
        failed,
        skipped,
        duration: start.elapsed(),
        results,
    }
}

// ---------------------------------------------------------------------------
// Report formatting
// ---------------------------------------------------------------------------

/// Format a test report for terminal output.
///
/// Uses ANSI escape codes for colored output:
/// - Green for passed tests
/// - Red for failed tests
/// - Yellow for skipped tests
pub fn format_report(report: &TestReport) -> String {
    let mut out = String::new();

    // Individual results
    for result in &report.results {
        match &result.outcome {
            TestOutcome::Passed => {
                out.push_str(&format!(
                    "\x1b[32m  PASS\x1b[0m {} ({:.1}ms)\n",
                    result.name,
                    result.duration.as_secs_f64() * 1000.0
                ));
            }
            TestOutcome::Failed { reason } => {
                out.push_str(&format!(
                    "\x1b[31m  FAIL\x1b[0m {} ({:.1}ms)\n",
                    result.name,
                    result.duration.as_secs_f64() * 1000.0
                ));
                // Indent the failure reason
                for line in reason.lines() {
                    out.push_str(&format!("       {}\n", line));
                }
            }
            TestOutcome::Skipped => {
                out.push_str(&format!("\x1b[33m  SKIP\x1b[0m {}\n", result.name));
            }
        }
    }

    // Summary line
    out.push('\n');
    let status_color = if report.failed > 0 { "\x1b[31m" } else { "\x1b[32m" };
    out.push_str(&format!(
        "{}Tests: {} passed, {} failed, {} skipped, {} total\x1b[0m\n",
        status_color, report.passed, report.failed, report.skipped, report.total
    ));
    out.push_str(&format!(
        "Time:  {:.3}s\n",
        report.duration.as_secs_f64()
    ));

    out
}

/// Format a test report without ANSI color codes (for piping / CI).
pub fn format_report_plain(report: &TestReport) -> String {
    let mut out = String::new();

    for result in &report.results {
        match &result.outcome {
            TestOutcome::Passed => {
                out.push_str(&format!(
                    "  PASS {} ({:.1}ms)\n",
                    result.name,
                    result.duration.as_secs_f64() * 1000.0
                ));
            }
            TestOutcome::Failed { reason } => {
                out.push_str(&format!(
                    "  FAIL {} ({:.1}ms)\n",
                    result.name,
                    result.duration.as_secs_f64() * 1000.0
                ));
                for line in reason.lines() {
                    out.push_str(&format!("       {}\n", line));
                }
            }
            TestOutcome::Skipped => {
                out.push_str(&format!("  SKIP {}\n", result.name));
            }
        }
    }

    out.push('\n');
    out.push_str(&format!(
        "Tests: {} passed, {} failed, {} skipped, {} total\n",
        report.passed, report.failed, report.skipped, report.total
    ));
    out.push_str(&format!(
        "Time:  {:.3}s\n",
        report.duration.as_secs_f64()
    ));

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Annotation parsing tests --

    #[test]
    fn test_parse_expect_annotation() {
        let source = "// expect: hello\nfn main() {}\n";
        let (expectation, skip) = parse_annotations(source);
        assert!(!skip);
        assert_eq!(expectation, Expectation::Output(vec!["hello".to_string()]));
    }

    #[test]
    fn test_parse_expect_error_annotation() {
        let source = "// expect-error: undefined variable\nfn main() { x }\n";
        let (expectation, skip) = parse_annotations(source);
        assert!(!skip);
        assert_eq!(
            expectation,
            Expectation::Error(vec!["undefined variable".to_string()])
        );
    }

    #[test]
    fn test_parse_skip_annotation() {
        let source = "// skip\nfn main() {}\n";
        let (_, skip) = parse_annotations(source);
        assert!(skip);
    }

    #[test]
    fn test_parse_multiple_expect_lines() {
        let source = "// expect: line1\n// expect: line2\nfn main() {}\n";
        let (expectation, _) = parse_annotations(source);
        assert_eq!(
            expectation,
            Expectation::Output(vec!["line1".to_string(), "line2".to_string()])
        );
    }

    #[test]
    fn test_parse_no_annotations() {
        let source = "fn main() {}\n";
        let (expectation, skip) = parse_annotations(source);
        assert!(!skip);
        assert_eq!(expectation, Expectation::Output(vec![]));
    }

    #[test]
    fn test_parse_mixed_annotations_error_wins() {
        // When both expect and expect-error are present, expect-error takes precedence
        let source = "// expect: hello\n// expect-error: type mismatch\nfn main() {}\n";
        let (expectation, _) = parse_annotations(source);
        match expectation {
            Expectation::Error(errors) => {
                assert_eq!(errors, vec!["type mismatch".to_string()]);
            }
            _ => panic!("expected Error variant"),
        }
    }

    // -- Test case construction tests --

    #[test]
    fn test_test_case_from_source() {
        let tc = test_case_from_source("my_test", "// expect: ok\nfn main() {}\n");
        assert_eq!(tc.name, "my_test");
        assert!(!tc.skip);
        assert_eq!(
            tc.expectation,
            Expectation::Output(vec!["ok".to_string()])
        );
    }

    #[test]
    fn test_test_case_from_source_skip() {
        let tc = test_case_from_source("skipped", "// skip\nfn main() {}\n");
        assert!(tc.skip);
    }

    // -- Test name derivation --

    #[test]
    fn test_derive_test_name_simple() {
        let base = Path::new("/tests");
        let path = Path::new("/tests/hello.kry");
        assert_eq!(derive_test_name(path, base), "hello");
    }

    #[test]
    fn test_derive_test_name_nested() {
        let base = Path::new("/tests");
        let path = Path::new("/tests/subdir/hello.kry");
        assert_eq!(derive_test_name(path, base), "subdir/hello");
    }

    // -- Report formatting tests --

    #[test]
    fn test_format_report_all_passed() {
        let report = TestReport {
            total: 2,
            passed: 2,
            failed: 0,
            skipped: 0,
            duration: Duration::from_millis(42),
            results: vec![
                TestResult {
                    name: "test_a".to_string(),
                    outcome: TestOutcome::Passed,
                    duration: Duration::from_millis(20),
                    actual_output: String::new(),
                },
                TestResult {
                    name: "test_b".to_string(),
                    outcome: TestOutcome::Passed,
                    duration: Duration::from_millis(22),
                    actual_output: String::new(),
                },
            ],
        };
        let output = format_report(&report);
        assert!(output.contains("PASS"));
        assert!(output.contains("test_a"));
        assert!(output.contains("test_b"));
        assert!(output.contains("2 passed"));
        assert!(output.contains("0 failed"));
    }

    #[test]
    fn test_format_report_with_failure() {
        let report = TestReport {
            total: 1,
            passed: 0,
            failed: 1,
            skipped: 0,
            duration: Duration::from_millis(10),
            results: vec![TestResult {
                name: "bad_test".to_string(),
                outcome: TestOutcome::Failed {
                    reason: "expected success but got errors".to_string(),
                },
                duration: Duration::from_millis(10),
                actual_output: String::new(),
            }],
        };
        let output = format_report(&report);
        assert!(output.contains("FAIL"));
        assert!(output.contains("bad_test"));
        assert!(output.contains("1 failed"));
    }

    #[test]
    fn test_format_report_with_skipped() {
        let report = TestReport {
            total: 1,
            passed: 0,
            failed: 0,
            skipped: 1,
            duration: Duration::from_millis(0),
            results: vec![TestResult {
                name: "skip_me".to_string(),
                outcome: TestOutcome::Skipped,
                duration: Duration::ZERO,
                actual_output: String::new(),
            }],
        };
        let output = format_report(&report);
        assert!(output.contains("SKIP"));
        assert!(output.contains("skip_me"));
        assert!(output.contains("1 skipped"));
    }

    #[test]
    fn test_format_report_plain_no_ansi() {
        let report = TestReport {
            total: 1,
            passed: 1,
            failed: 0,
            skipped: 0,
            duration: Duration::from_millis(5),
            results: vec![TestResult {
                name: "ok_test".to_string(),
                outcome: TestOutcome::Passed,
                duration: Duration::from_millis(5),
                actual_output: String::new(),
            }],
        };
        let output = format_report_plain(&report);
        // Should not contain ANSI escape codes
        assert!(!output.contains("\x1b["));
        assert!(output.contains("PASS"));
        assert!(output.contains("ok_test"));
    }

    #[test]
    fn test_report_all_passed_check() {
        let report = TestReport {
            total: 3,
            passed: 2,
            failed: 0,
            skipped: 1,
            duration: Duration::ZERO,
            results: Vec::new(),
        };
        assert!(report.all_passed());

        let failing_report = TestReport {
            total: 3,
            passed: 1,
            failed: 1,
            skipped: 1,
            duration: Duration::ZERO,
            results: Vec::new(),
        };
        assert!(!failing_report.all_passed());
    }

    // -- Skipped test execution --

    #[test]
    fn test_run_test_skipped() {
        let tc = test_case_from_source("skip_test", "// skip\nfn main() {}\n");
        let result = run_test(&tc);
        assert_eq!(result.outcome, TestOutcome::Skipped);
        assert_eq!(result.duration, Duration::ZERO);
    }
}
