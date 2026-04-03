//! Integration tests for the Kryos compiler driver.

use std::fs;
use std::path::PathBuf;

use kryos_driver::{
    BuildConfig, BuildMode, OutputType,
    compile_file, compile_source, check_source,
};

/// Write `contents` to a temporary `.kry` file and return its path.
fn temp_kry_file(name: &str, contents: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("kryos_driver_tests");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    path
}

/// Valid Kryos source that should compile through to MIR without errors.
const VALID_SOURCE: &str = r#"fn add(a: i32, b: i32) -> i32 {
    return a + b
}
"#;

/// Source with a syntax error (malformed function declaration).
const SYNTAX_ERROR_SOURCE: &str = r#"fn broken( {
"#;

/// Source with a type error (string assigned to i32 variable).
const TYPE_ERROR_SOURCE: &str = r#"fn bad() -> i32 {
    let x: i32 = "hello"
    return x
}
"#;

// ---------------------------------------------------------------------------
// compile_file tests
// ---------------------------------------------------------------------------

#[test]
fn compile_valid_source_succeeds() {
    let path = temp_kry_file("valid.kry", VALID_SOURCE);
    let config = BuildConfig {
        input: path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
    };

    let result = compile_file(&path, &config);

    assert!(
        result.success,
        "expected success but got errors: {:?}",
        result.diagnostics
    );
    // No error-level diagnostics.
    assert_eq!(
        result.error_count(),
        0,
        "expected no errors, got: {:?}",
        result.errors()
    );
    // MIR should have been produced.
    assert!(
        result.mir.is_some(),
        "expected MIR to be produced for valid source"
    );
}

#[test]
fn compile_syntax_error_fails() {
    let path = temp_kry_file("syntax_err.kry", SYNTAX_ERROR_SOURCE);
    let config = BuildConfig {
        input: path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Binary,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
    };

    let result = compile_file(&path, &config);

    assert!(!result.success, "expected failure for syntax error source");
    assert!(
        result.error_count() > 0,
        "expected at least one error diagnostic"
    );
    // MIR should NOT be produced when parsing fails.
    assert!(
        result.mir.is_none(),
        "expected no MIR for syntax error source"
    );
}

#[test]
fn compile_type_error_fails() {
    let path = temp_kry_file("type_err.kry", TYPE_ERROR_SOURCE);
    let config = BuildConfig {
        input: path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Binary,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
    };

    let result = compile_file(&path, &config);

    assert!(!result.success, "expected failure for type error source");
    assert!(
        result.error_count() > 0,
        "expected at least one type error diagnostic"
    );
    // MIR should NOT be produced when type check fails.
    assert!(
        result.mir.is_none(),
        "expected no MIR when type errors are present"
    );
}

#[test]
fn compile_nonexistent_file_fails() {
    let path = PathBuf::from("/nonexistent/path/to/file.kry");
    let config = BuildConfig::for_file(path.to_string_lossy().to_string());

    let result = compile_file(&path, &config);

    assert!(!result.success);
    assert!(result.error_count() > 0);
    // The error message should mention the file.
    let msg = &result.diagnostics[0].message;
    assert!(
        msg.contains("failed to read"),
        "expected 'failed to read' in error: {msg}"
    );
}

// ---------------------------------------------------------------------------
// check_source tests
// ---------------------------------------------------------------------------

#[test]
fn check_valid_source_no_diagnostics() {
    let (diags, _source_map) = check_source(VALID_SOURCE, "test_valid.kry");

    let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected no errors checking valid source, got: {:?}",
        errors
    );
}

#[test]
fn check_syntax_error_returns_diagnostics() {
    let (diags, _source_map) = check_source(SYNTAX_ERROR_SOURCE, "test_syntax.kry");

    let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
    assert!(
        !errors.is_empty(),
        "expected errors for syntax error source"
    );
}

#[test]
fn check_type_error_returns_diagnostics() {
    let (diags, _source_map) = check_source(TYPE_ERROR_SOURCE, "test_type.kry");

    let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
    assert!(
        !errors.is_empty(),
        "expected errors for type error source"
    );
}

// ---------------------------------------------------------------------------
// compile_source tests (string API, no file I/O)
// ---------------------------------------------------------------------------

#[test]
fn compile_source_valid() {
    let config = BuildConfig {
        input: "inline.kry".to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
    };

    let result = compile_source(VALID_SOURCE, "inline.kry", &config);

    assert!(result.success);
    assert!(result.mir.is_some());
    assert_eq!(result.error_count(), 0);
}

#[test]
fn compile_source_with_binary_output_no_backend() {
    // When requesting Binary output but no backend is available, the driver
    // should still succeed (with a warning) and produce MIR.
    let config = BuildConfig {
        input: "inline.kry".to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Binary,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
    };

    let result = compile_source(VALID_SOURCE, "inline.kry", &config);

    assert!(result.success, "should succeed even without backend");
    assert!(result.mir.is_some(), "MIR should still be produced");
    // Should have a warning about no backend.
    let warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.level == kryos_errors::Level::Warning)
        .collect();
    assert!(
        !warnings.is_empty(),
        "expected a warning about missing backend"
    );
}

// ---------------------------------------------------------------------------
// BuildConfig tests
// ---------------------------------------------------------------------------

#[test]
fn build_config_defaults() {
    let config = BuildConfig::for_file("main.kry");

    assert_eq!(config.input, "main.kry");
    assert_eq!(config.output, None);
    assert_eq!(config.mode, BuildMode::Debug);
    assert_eq!(config.output_type, OutputType::Binary);
    assert_eq!(config.target, None);
    assert!(config.capabilities.is_empty());
    assert!(!config.verbose);
}

#[test]
fn build_config_derive_output_path() {
    let config = BuildConfig {
        input: "src/main.kry".to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Object,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
    };

    let derived = config.derive_output_path();
    assert_eq!(derived, PathBuf::from("main.o"));
}

#[test]
fn build_config_derive_output_mir() {
    let config = BuildConfig {
        input: "foo.kry".to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
    };

    let derived = config.derive_output_path();
    assert_eq!(derived, PathBuf::from("foo.mir"));
}

#[test]
fn build_config_explicit_output_overrides_derived() {
    let config = BuildConfig {
        input: "main.kry".to_string(),
        output: Some("custom_output".to_string()),
        mode: BuildMode::Debug,
        output_type: OutputType::Binary,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
    };

    let derived = config.derive_output_path();
    assert_eq!(derived, PathBuf::from("custom_output"));
}

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

#[test]
fn version_is_correct() {
    assert_eq!(kryos_driver::version(), "0.1.0");
}
