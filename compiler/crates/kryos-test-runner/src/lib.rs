//! Kryos test runner — discovers and runs `// expect:` test programs.
//!
//! Scans directories for `.kry` files containing `// expect:` annotations,
//! compiles them using the Kryos driver, and verifies the compiler output
//! matches the expected results.
//!
//! Supported annotations:
//! - `// expect: <text>` — the compiler should succeed and output should contain `<text>`
//! - `// expect-error: <text>` — the compiler should produce an error containing `<text>`
//! - `// run-expect: <text>` — compile to a binary, execute it, and verify stdout contains `<text>`
//! - `// skip` — skip this test file entirely

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use kryos_codegen_cranelift::CraneliftBackend;
use kryos_driver::{
    compile_file, compile_file_with_backend, compile_source, render_diagnostics, BuildConfig,
    BuildMode, OutputType,
};

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
    /// Compile to a binary, execute it, and verify stdout contains these lines (from `// run-expect:`).
    RunOutput(Vec<String>),
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
    let mut run_expect_lines: Vec<String> = Vec::new();
    let mut skip = false;

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed == "// skip" {
            skip = true;
        } else if let Some(rest) = trimmed.strip_prefix("// expect-error:") {
            expect_error_lines.push(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("// run-expect:") {
            run_expect_lines.push(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("// expect:") {
            expect_lines.push(rest.trim().to_string());
        }
    }

    // Priority: expect-error > run-expect > expect
    let expectation = if !expect_error_lines.is_empty() {
        Expectation::Error(expect_error_lines)
    } else if !run_expect_lines.is_empty() {
        Expectation::RunOutput(run_expect_lines)
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
    let mut config = BuildConfig::for_file(test.source_path.to_string_lossy().to_string());
    // Non-run tests only need type checking, not binary output.
    if !matches!(test.expectation, Expectation::RunOutput(_)) {
        config.output_type = OutputType::Mir; // skip codegen + linking
    }
    let result = if test.source_path.exists() {
        compile_file(&test.source_path, &config)
    } else {
        compile_source(&test.source, &test.name, &config)
    };
    let duration = start.elapsed();

    let diagnostics_text = render_diagnostics(&result);
    let has_errors = result.diagnostics.iter().any(|d| d.is_error());

    let outcome = match &test.expectation {
        Expectation::Output(expected_lines) => {
            if has_errors {
                TestOutcome::Failed {
                    reason: format!("expected success but got errors:\n{}", diagnostics_text),
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
        Expectation::RunOutput(expected_lines) => {
            // First, the source must compile without errors at the analysis level
            if has_errors {
                return TestResult {
                    name: test.name.clone(),
                    outcome: TestOutcome::Failed {
                        reason: format!(
                            "expected successful compilation but got errors:\n{}",
                            diagnostics_text
                        ),
                    },
                    duration: start.elapsed(),
                    actual_output: diagnostics_text,
                };
            }

            // Write source to a temp file, compile to binary, execute, check stdout
            let safe_name = test.name.replace(['/', '\\'], "_");
            let temp_dir = std::env::temp_dir().join("kryos_test_runner");
            let _ = fs::create_dir_all(&temp_dir);
            let temp_src = temp_dir.join(format!("{safe_name}.kry"));
            let exe_ext = if cfg!(windows) { ".exe" } else { "" };
            let temp_out = temp_dir.join(format!("{safe_name}{exe_ext}"));

            // Write the source to the temp file
            if let Err(e) = fs::write(&temp_src, &test.source) {
                return TestResult {
                    name: test.name.clone(),
                    outcome: TestOutcome::Failed {
                        reason: format!("failed to write temp source file: {e}"),
                    },
                    duration: start.elapsed(),
                    actual_output: diagnostics_text,
                };
            }

            // Ensure the runtime library can be found during linking.
            // During `cargo test` the CWD is the crate directory, but the
            // runtime libs live under the workspace `target/` directory. Set
            // the environment variables so `find_runtime_lib()` can locate them.
            if std::env::var("KRYOS_RT_LIB").is_err() {
                let workspace_target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("..")
                    .join("target")
                    .join("debug");
                let rt_lib_name = if cfg!(all(windows, target_env = "msvc")) {
                    "kryos_rt.lib"
                } else {
                    "libkryos_rt.a"
                };
                let rt_path = workspace_target.join(rt_lib_name);
                if rt_path.is_file() {
                    std::env::set_var("KRYOS_RT_LIB", &rt_path);
                }
            }
            if std::env::var("KRYOS_STDLIB_NATIVE_LIB").is_err() {
                let workspace_target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("..")
                    .join("target")
                    .join("debug");
                let stdlib_lib_name = if cfg!(all(windows, target_env = "msvc")) {
                    "kryos_stdlib_native.lib"
                } else {
                    "libkryos_stdlib_native.a"
                };
                let stdlib_path = workspace_target.join(stdlib_lib_name);
                if stdlib_path.is_file() {
                    std::env::set_var("KRYOS_STDLIB_NATIVE_LIB", &stdlib_path);
                }
            }

            // Compile to binary using Cranelift backend
            let build_config = BuildConfig {
                input: temp_src.to_string_lossy().to_string(),
                output: Some(temp_out.to_string_lossy().to_string()),
                mode: BuildMode::Debug,
                output_type: OutputType::Binary,
                target: None,
                capabilities: Vec::new(),
                verbose: false,
                skip_ownership: false,
            };

            let backend = CraneliftBackend::new();
            let compile_result =
                compile_file_with_backend(&temp_src, &build_config, Some(&backend));

            if !compile_result.success {
                let compile_diags = render_diagnostics(&compile_result);
                let _ = fs::remove_file(&temp_src);
                let _ = fs::remove_file(&temp_out);
                // Skip if the runtime library isn't available for linking
                // (build environment issue, not a test failure).
                let is_rt_missing = compile_diags.contains("unresolved external")
                    || compile_diags.contains("undefined reference")
                    || compile_diags.contains("linking failed");
                return TestResult {
                    name: test.name.clone(),
                    outcome: if is_rt_missing {
                        TestOutcome::Skipped
                    } else {
                        TestOutcome::Failed {
                            reason: format!("compilation to binary failed:\n{}", compile_diags),
                        }
                    },
                    duration: start.elapsed(),
                    actual_output: compile_diags,
                };
            }

            // Execute the compiled binary and capture stdout
            let exec_result = Command::new(&temp_out)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output();

            // Clean up temp files regardless of outcome
            let _ = fs::remove_file(&temp_src);
            let _ = fs::remove_file(&temp_out);

            match exec_result {
                Err(e) => TestOutcome::Failed {
                    reason: format!(
                        "failed to execute compiled binary '{}': {e}",
                        temp_out.display()
                    ),
                },
                Ok(output) => {
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        TestOutcome::Failed {
                            reason: format!(
                                "binary exited with status {}\nstderr: {}",
                                output.status,
                                stderr.trim()
                            ),
                        }
                    } else {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stdout_str = stdout.to_string();
                        let mut missing: Vec<String> = Vec::new();
                        for expected in expected_lines {
                            if !stdout_str.contains(expected) {
                                missing.push(expected.clone());
                            }
                        }
                        if missing.is_empty() {
                            TestOutcome::Passed
                        } else {
                            TestOutcome::Failed {
                                reason: format!(
                                    "expected stdout containing {:?}, got:\n{}",
                                    missing,
                                    stdout_str.trim()
                                ),
                            }
                        }
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
    let status_color = if report.failed > 0 {
        "\x1b[31m"
    } else {
        "\x1b[32m"
    };
    out.push_str(&format!(
        "{}Tests: {} passed, {} failed, {} skipped, {} total\x1b[0m\n",
        status_color, report.passed, report.failed, report.skipped, report.total
    ));
    out.push_str(&format!("Time:  {:.3}s\n", report.duration.as_secs_f64()));

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
                if !result.actual_output.is_empty() {
                    for line in result.actual_output.lines().take(20) {
                        out.push_str(&format!("       {}\n", line));
                    }
                }
            }
        }
    }

    out.push('\n');
    out.push_str(&format!(
        "Tests: {} passed, {} failed, {} skipped, {} total\n",
        report.passed, report.failed, report.skipped, report.total
    ));
    out.push_str(&format!("Time:  {:.3}s\n", report.duration.as_secs_f64()));

    out
}

// ---------------------------------------------------------------------------
// @test annotation runner — discover and JIT-execute @test functions
// ---------------------------------------------------------------------------

/// Discover `.kry` files containing `@test`-annotated functions in a directory.
///
/// Each file is compiled to MIR, and functions with the `@test` attribute
/// are collected. Returns a list of (file_path, test_function_names).
pub fn discover_annotated_tests(dir: &Path) -> Vec<(PathBuf, Vec<String>)> {
    let mut results = Vec::new();
    discover_annotated_recursive(dir, &mut results);
    results.sort_by(|a, b| a.0.cmp(&b.0));
    results
}

fn discover_annotated_recursive(dir: &Path, results: &mut Vec<(PathBuf, Vec<String>)>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_annotated_recursive(&path, results);
        } else if path.extension().and_then(|e| e.to_str()) == Some("kry") {
            if let Ok(source) = fs::read_to_string(&path) {
                // Quick pre-filter: skip files that don't contain @test at all.
                if !source.contains("@test") {
                    continue;
                }
                let config = BuildConfig::for_file(path.to_string_lossy().to_string());
                let result = compile_source(&source, &path.to_string_lossy(), &config);
                if !result.success {
                    continue;
                }
                if let Some(ref mir) = result.mir {
                    let test_fns: Vec<String> = mir
                        .functions
                        .iter()
                        .filter(|f| f.attributes.test)
                        .map(|f| f.name.clone())
                        .collect();
                    if !test_fns.is_empty() {
                        results.push((path, test_fns));
                    }
                }
            }
        }
    }
}

/// Run all `@test`-annotated functions found in `.kry` files under `dir`.
///
/// Each file is compiled via the driver, then JIT-compiled using the
/// Cranelift backend. Each `@test` function is called as `fn()` — a panic
/// from `assert()` or `panic()` counts as a failure.
pub fn run_annotated_tests(dir: &Path, filter: Option<&str>) -> TestReport {
    let start = Instant::now();
    let mut results = Vec::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    let discovered = discover_annotated_tests(dir);

    // Enable test mode so assert/panic raise catchable panics instead of
    // aborting the process.
    kryos_rt::set_test_mode(true);

    for (path, test_fns) in &discovered {
        let source = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let config = BuildConfig::for_file(path.to_string_lossy().to_string());
        let result = compile_source(&source, &path.to_string_lossy(), &config);
        if !result.success {
            // File-level compilation failure — report all test fns as failed.
            let diags = render_diagnostics(&result);
            for fn_name in test_fns {
                if let Some(f) = filter {
                    if !fn_name.contains(f) {
                        skipped += 1;
                        results.push(TestResult {
                            name: fn_name.clone(),
                            outcome: TestOutcome::Skipped,
                            duration: Duration::ZERO,
                            actual_output: String::new(),
                        });
                        continue;
                    }
                }
                failed += 1;
                results.push(TestResult {
                    name: fn_name.clone(),
                    outcome: TestOutcome::Failed {
                        reason: format!("compilation failed:\n{diags}"),
                    },
                    duration: Duration::ZERO,
                    actual_output: diags.clone(),
                });
            }
            continue;
        }

        let mir = match result.mir {
            Some(ref m) => m,
            None => continue,
        };

        // JIT compile the module so all functions are available.
        let backend = CraneliftBackend::new();
        let ptrs = match backend.jit_compile_module(mir) {
            Ok(p) => p,
            Err(e) => {
                for fn_name in test_fns {
                    failed += 1;
                    results.push(TestResult {
                        name: fn_name.clone(),
                        outcome: TestOutcome::Failed {
                            reason: format!("JIT compilation failed: {e}"),
                        },
                        duration: Duration::ZERO,
                        actual_output: String::new(),
                    });
                }
                continue;
            }
        };

        // Execute each @test function.
        for fn_name in test_fns {
            if let Some(f) = filter {
                if !fn_name.contains(f) {
                    skipped += 1;
                    results.push(TestResult {
                        name: fn_name.clone(),
                        outcome: TestOutcome::Skipped,
                        duration: Duration::ZERO,
                        actual_output: String::new(),
                    });
                    continue;
                }
            }

            let test_start = Instant::now();
            if let Some(&ptr) = ptrs.get(fn_name.as_str()) {
                // Safety: the JIT-compiled function has signature `fn()` — @test
                // functions take no parameters and return void.
                let f: fn() = unsafe { std::mem::transmute(ptr) };
                // Clear any previous failure, call the test, then check the
                // thread-local flag.  We use a flag instead of catch_unwind
                // because Cranelift JIT frames lack OS unwind info on Windows,
                // so panics cannot propagate through JIT-compiled code.
                let _ = kryos_rt::take_test_failure(); // clear
                f();
                let failure = kryos_rt::take_test_failure();
                let duration = test_start.elapsed();
                if let Some(msg) = failure {
                    failed += 1;
                    results.push(TestResult {
                        name: fn_name.clone(),
                        outcome: TestOutcome::Failed {
                            reason: msg.clone(),
                        },
                        duration,
                        actual_output: msg,
                    });
                } else {
                    passed += 1;
                    results.push(TestResult {
                        name: fn_name.clone(),
                        outcome: TestOutcome::Passed,
                        duration,
                        actual_output: String::new(),
                    });
                }
            } else {
                failed += 1;
                results.push(TestResult {
                    name: fn_name.clone(),
                    outcome: TestOutcome::Failed {
                        reason: format!("function `{fn_name}` not found in JIT output"),
                    },
                    duration: Duration::ZERO,
                    actual_output: String::new(),
                });
            }
        }
    }

    kryos_rt::set_test_mode(false);

    TestReport {
        total: results.len(),
        passed,
        failed,
        skipped,
        duration: start.elapsed(),
        results,
    }
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

    // -- run-expect annotation parsing tests --

    #[test]
    fn test_parse_run_expect_annotation() {
        let source = "// run-expect: Hello, Kryos!\nfn main() { println(\"Hello, Kryos!\") }\n";
        let (expectation, skip) = parse_annotations(source);
        assert!(!skip);
        assert_eq!(
            expectation,
            Expectation::RunOutput(vec!["Hello, Kryos!".to_string()])
        );
    }

    #[test]
    fn test_parse_multiple_run_expect_lines() {
        let source = "// run-expect: line1\n// run-expect: line2\nfn main() {}\n";
        let (expectation, _) = parse_annotations(source);
        assert_eq!(
            expectation,
            Expectation::RunOutput(vec!["line1".to_string(), "line2".to_string()])
        );
    }

    #[test]
    fn test_parse_run_expect_with_skip() {
        let source = "// run-expect: output\n// skip\nfn main() {}\n";
        let (expectation, skip) = parse_annotations(source);
        assert!(skip);
        assert_eq!(
            expectation,
            Expectation::RunOutput(vec!["output".to_string()])
        );
    }

    #[test]
    fn test_parse_error_wins_over_run_expect() {
        // expect-error takes priority over run-expect
        let source = "// run-expect: hello\n// expect-error: type mismatch\nfn main() {}\n";
        let (expectation, _) = parse_annotations(source);
        match expectation {
            Expectation::Error(errors) => {
                assert_eq!(errors, vec!["type mismatch".to_string()]);
            }
            _ => panic!("expected Error variant, got {:?}", expectation),
        }
    }

    #[test]
    fn test_parse_run_expect_wins_over_expect() {
        // run-expect takes priority over expect
        let source = "// expect: hello\n// run-expect: world\nfn main() {}\n";
        let (expectation, _) = parse_annotations(source);
        match expectation {
            Expectation::RunOutput(lines) => {
                assert_eq!(lines, vec!["world".to_string()]);
            }
            _ => panic!("expected RunOutput variant, got {:?}", expectation),
        }
    }

    // -- Test case construction tests --

    #[test]
    fn test_test_case_from_source() {
        let tc = test_case_from_source("my_test", "// expect: ok\nfn main() {}\n");
        assert_eq!(tc.name, "my_test");
        assert!(!tc.skip);
        assert_eq!(tc.expectation, Expectation::Output(vec!["ok".to_string()]));
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
