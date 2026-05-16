//! Integration tests for the Kryos compiler driver.

use std::fs;
use std::path::PathBuf;

use kryos_driver::{
    check_source, compile_file, compile_source, BuildConfig, BuildMode, OutputType,
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
const VALID_SOURCE: &str = r#"fn add(a: i64, b: i64) -> i64 {
    return a + b
}

fn main() {
    let x = add(1, 2)
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
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
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
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
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
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
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
    assert!(!errors.is_empty(), "expected errors for type error source");
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
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
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
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
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
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
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
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
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
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
    };

    let derived = config.derive_output_path();
    assert_eq!(derived, PathBuf::from("custom_output"));
}

// ---------------------------------------------------------------------------
// Module import tests
// ---------------------------------------------------------------------------

#[test]
fn compile_file_with_import_succeeds() {
    // Create a main file that imports a math module.
    let dir = std::env::temp_dir().join("kryos_module_tests");
    fs::create_dir_all(&dir).unwrap();

    let math_src = r#"fn add(a: i32, b: i32) -> i32 {
    return a + b
}

fn subtract(a: i32, b: i32) -> i32 {
    return a - b
}
"#;

    let main_src = r#"use math

fn main() {
    let result = add(3, 4)
    println("module import works")
}
"#;

    fs::write(dir.join("math.kry"), math_src).unwrap();
    fs::write(dir.join("main.kry"), main_src).unwrap();

    let main_path = dir.join("main.kry");
    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: true,
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
    };

    let result = compile_file(&main_path, &config);

    assert!(
        result.success,
        "expected success but got errors: {:?}",
        result.diagnostics
    );
    assert_eq!(
        result.error_count(),
        0,
        "expected no errors, got: {:?}",
        result.errors()
    );
    assert!(result.mir.is_some(), "expected MIR to be produced");

    // The merged MIR should contain functions from both files:
    // add, subtract (from math.kry) and main (from main.kry).
    let mir = result.mir.unwrap();
    let func_names: Vec<&str> = mir.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(
        func_names.contains(&"add"),
        "expected 'add' in merged MIR, got: {func_names:?}"
    );
    assert!(
        func_names.contains(&"subtract"),
        "expected 'subtract' in merged MIR, got: {func_names:?}"
    );
    assert!(
        func_names.contains(&"main"),
        "expected 'main' in merged MIR, got: {func_names:?}"
    );
}

#[test]
fn compile_file_with_missing_import_fails() {
    let dir = std::env::temp_dir().join("kryos_module_tests_missing");
    fs::create_dir_all(&dir).unwrap();

    let main_src = r#"use nonexistent

fn main() {
    println("should fail")
}
"#;

    fs::write(dir.join("main.kry"), main_src).unwrap();

    let main_path = dir.join("main.kry");
    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
    };

    let result = compile_file(&main_path, &config);

    assert!(!result.success, "expected failure for missing import");
    assert!(
        result.error_count() > 0,
        "expected at least one error diagnostic"
    );
    let msg = &result.diagnostics[0].message;
    assert!(
        msg.contains("nonexistent") && msg.contains("not found"),
        "expected error about 'nonexistent' not found, got: {msg}"
    );
}

#[test]
fn compile_file_with_transitive_import_succeeds() {
    // Test: main imports utils, which imports helpers.
    let dir = std::env::temp_dir().join("kryos_module_tests_transitive");
    fs::create_dir_all(&dir).unwrap();

    let helpers_src = r#"fn double(x: i32) -> i32 {
    return x + x
}
"#;

    let utils_src = r#"use helpers

fn triple(x: i32) -> i32 {
    return x + x + x
}
"#;

    let main_src = r#"use utils

fn main() {
    let a = triple(5)
    let b = double(3)
    println("transitive import works")
}
"#;

    fs::write(dir.join("helpers.kry"), helpers_src).unwrap();
    fs::write(dir.join("utils.kry"), utils_src).unwrap();
    fs::write(dir.join("main.kry"), main_src).unwrap();

    let main_path = dir.join("main.kry");
    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: true,
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
    };

    let result = compile_file(&main_path, &config);

    assert!(
        result.success,
        "expected success but got errors: {:?}",
        result.diagnostics
    );

    let mir = result.mir.unwrap();
    let func_names: Vec<&str> = mir.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(
        func_names.contains(&"double"),
        "expected 'double' from transitive import, got: {func_names:?}"
    );
    assert!(
        func_names.contains(&"triple"),
        "expected 'triple' from direct import, got: {func_names:?}"
    );
    assert!(
        func_names.contains(&"main"),
        "expected 'main' in merged MIR, got: {func_names:?}"
    );
}

#[test]
fn compile_file_with_diamond_import_no_duplicates() {
    // Diamond: main imports A and B, both import common.
    // Functions from common should appear exactly once.
    let dir = std::env::temp_dir().join("kryos_module_tests_diamond");
    fs::create_dir_all(&dir).unwrap();

    let common_src = r#"fn shared_fn(x: i32) -> i32 {
    return x
}
"#;

    let a_src = r#"use common

fn a_fn(x: i32) -> i32 {
    return shared_fn(x)
}
"#;

    let b_src = r#"use common

fn b_fn(x: i32) -> i32 {
    return shared_fn(x)
}
"#;

    let main_src = r#"use a
use b

fn main() {
    let x = a_fn(1)
    let y = b_fn(2)
    println("diamond import works")
}
"#;

    fs::write(dir.join("common.kry"), common_src).unwrap();
    fs::write(dir.join("a.kry"), a_src).unwrap();
    fs::write(dir.join("b.kry"), b_src).unwrap();
    fs::write(dir.join("main.kry"), main_src).unwrap();

    let main_path = dir.join("main.kry");
    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: true,
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
    };

    let result = compile_file(&main_path, &config);

    assert!(
        result.success,
        "expected success but got errors: {:?}",
        result.diagnostics
    );

    let mir = result.mir.unwrap();
    let func_names: Vec<&str> = mir.functions.iter().map(|f| f.name.as_str()).collect();

    // shared_fn should appear exactly once despite being imported through two paths.
    let shared_count = func_names.iter().filter(|&&n| n == "shared_fn").count();
    assert_eq!(
        shared_count, 1,
        "expected 'shared_fn' exactly once in MIR, got {shared_count} in: {func_names:?}"
    );

    assert!(func_names.contains(&"a_fn"));
    assert!(func_names.contains(&"b_fn"));
    assert!(func_names.contains(&"main"));
}

// ---------------------------------------------------------------------------
// Module resolution tests
// ---------------------------------------------------------------------------

#[test]
fn resolve_module_sibling_file() {
    let dir = std::env::temp_dir().join("kryos_resolve_tests");
    fs::create_dir_all(&dir).unwrap();

    fs::write(dir.join("foo.kry"), "fn foo() {}").unwrap();
    let main_path = dir.join("main.kry");
    fs::write(&main_path, "use foo").unwrap();

    let result = kryos_driver::resolve::resolve_module_path(&["foo".to_string()], &main_path);
    assert!(result.is_ok(), "expected to find foo.kry as sibling");
    assert_eq!(result.unwrap(), dir.join("foo.kry"));
}

#[test]
fn resolve_module_dir_mod() {
    let dir = std::env::temp_dir().join("kryos_resolve_tests_dir");
    fs::create_dir_all(dir.join("bar")).unwrap();

    fs::write(dir.join("bar").join("mod.kry"), "fn bar() {}").unwrap();
    let main_path = dir.join("main.kry");
    fs::write(&main_path, "use bar").unwrap();

    let result = kryos_driver::resolve::resolve_module_path(&["bar".to_string()], &main_path);
    assert!(result.is_ok(), "expected to find bar/mod.kry");
    assert_eq!(result.unwrap(), dir.join("bar").join("mod.kry"));
}

#[test]
fn resolve_module_not_found() {
    let dir = std::env::temp_dir().join("kryos_resolve_tests_missing");
    fs::create_dir_all(&dir).unwrap();

    let main_path = dir.join("main.kry");
    fs::write(&main_path, "use nonexistent").unwrap();

    let result =
        kryos_driver::resolve::resolve_module_path(&["nonexistent".to_string()], &main_path);
    assert!(result.is_err(), "expected error for missing module");
}

// ---------------------------------------------------------------------------
// Multi-segment / selective / alias import tests
// ---------------------------------------------------------------------------

#[test]
fn compile_file_with_multi_segment_import() {
    let dir = std::env::temp_dir().join("kryos_module_tests_multi_seg");
    fs::create_dir_all(dir.join("ml")).unwrap();

    let math_src = r#"fn multiply(a: i32, b: i32) -> i32 {
    return a * b
}
"#;

    let main_src = r#"use ml::math

fn main() {
    let result = multiply(3, 4)
    println("multi-segment import works")
}
"#;

    fs::write(dir.join("ml").join("math.kry"), math_src).unwrap();
    fs::write(dir.join("main.kry"), main_src).unwrap();

    let main_path = dir.join("main.kry");
    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: true,
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
    };

    let result = compile_file(&main_path, &config);
    assert!(
        result.success,
        "expected success but got errors: {:?}",
        result.diagnostics
    );
    assert_eq!(result.error_count(), 0);
    let mir = result.mir.unwrap();
    let func_names: Vec<&str> = mir.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(
        func_names.contains(&"multiply"),
        "expected 'multiply' in merged MIR, got: {func_names:?}"
    );
    assert!(
        func_names.contains(&"main"),
        "expected 'main' in merged MIR, got: {func_names:?}"
    );
}

#[test]
fn compile_file_with_selective_import() {
    let dir = std::env::temp_dir().join("kryos_module_tests_selective");
    fs::create_dir_all(&dir).unwrap();

    let math_src = r#"fn add(a: i32, b: i32) -> i32 {
    return a + b
}
fn subtract(a: i32, b: i32) -> i32 {
    return a - b
}
fn multiply(a: i32, b: i32) -> i32 {
    return a * b
}
"#;

    let main_src = r#"use math::{add, multiply}

fn main() {
    let a = add(3, 4)
    let b = multiply(5, 6)
    println("selective import works")
}
"#;

    fs::write(dir.join("math.kry"), math_src).unwrap();
    fs::write(dir.join("main.kry"), main_src).unwrap();

    let main_path = dir.join("main.kry");
    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: true,
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
    };

    let result = compile_file(&main_path, &config);
    assert!(result.success, "errors: {:?}", result.diagnostics);
    let mir = result.mir.unwrap();
    let func_names: Vec<&str> = mir.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(
        func_names.contains(&"add"),
        "expected 'add' in merged MIR, got: {func_names:?}"
    );
    assert!(
        func_names.contains(&"multiply"),
        "expected 'multiply' in merged MIR, got: {func_names:?}"
    );
    // subtract should NOT be imported with selective import.
    assert!(
        !func_names.contains(&"subtract"),
        "subtract should not be imported, got: {func_names:?}"
    );
}

#[test]
fn compile_file_with_aliased_import() {
    let dir = std::env::temp_dir().join("kryos_module_tests_alias");
    fs::create_dir_all(&dir).unwrap();

    let math_src = r#"fn add(a: i32, b: i32) -> i32 {
    return a + b
}
"#;

    let main_src = r#"use math as m

fn main() {
    let result = add(3, 4)
    println("aliased import works")
}
"#;

    fs::write(dir.join("math.kry"), math_src).unwrap();
    fs::write(dir.join("main.kry"), main_src).unwrap();

    let main_path = dir.join("main.kry");
    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: true,
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
    };

    let result = compile_file(&main_path, &config);
    assert!(result.success, "errors: {:?}", result.diagnostics);
}

#[test]
fn resolve_module_multi_segment() {
    let dir = std::env::temp_dir().join("kryos_resolve_tests_multi");
    fs::create_dir_all(dir.join("net")).unwrap();

    fs::write(
        dir.join("net").join("http.kry"),
        "fn get() -> i32 { return 0 }",
    )
    .unwrap();
    let main_path = dir.join("main.kry");
    fs::write(&main_path, "use net::http").unwrap();

    let result = kryos_driver::resolve::resolve_module_path(
        &["net".to_string(), "http".to_string()],
        &main_path,
    );
    assert!(result.is_ok(), "expected to find net/http.kry");
    assert_eq!(result.unwrap(), dir.join("net").join("http.kry"));
}

// ---------------------------------------------------------------------------
// Stdlib resolution tests
// ---------------------------------------------------------------------------

#[test]
fn resolve_std_module_to_stdlib_dir() {
    // Verify that `use std::math` resolves to stdlib/math.kry via
    // CARGO_MANIFEST_DIR walk-up (the default path for the compiled driver).
    let dir = std::env::temp_dir().join("kryos_resolve_std_tests");
    fs::create_dir_all(&dir).unwrap();

    let main_path = dir.join("main.kry");
    fs::write(&main_path, "use std::math\nfn main() { let pi = 3.14 }\n").unwrap();

    let result = kryos_driver::resolve::resolve_module_path(
        &["std".to_string(), "math".to_string()],
        &main_path,
    );
    assert!(
        result.is_ok(),
        "expected std::math to resolve to stdlib/math.kry, got: {:?}",
        result.err()
    );
    let resolved = result.unwrap();
    assert!(
        resolved.to_string_lossy().contains("stdlib"),
        "expected resolved path to be under stdlib/, got: {}",
        resolved.display()
    );
    assert!(
        resolved.to_string_lossy().ends_with("math.kry"),
        "expected resolved path to end with math.kry, got: {}",
        resolved.display()
    );
}

#[test]
fn resolve_std_string_to_stdlib_dir() {
    // Verify that `use std::string` also resolves to stdlib/string.kry.
    let dir = std::env::temp_dir().join("kryos_resolve_std_string_test");
    fs::create_dir_all(&dir).unwrap();

    let main_path = dir.join("main.kry");
    fs::write(&main_path, "fn main() {}\n").unwrap();

    let result = kryos_driver::resolve::resolve_module_path(
        &["std".to_string(), "string".to_string()],
        &main_path,
    );
    assert!(
        result.is_ok(),
        "expected std::string to resolve, got: {:?}",
        result.err()
    );
    let resolved = result.unwrap();
    assert!(
        resolved.to_string_lossy().contains("stdlib"),
        "expected path under stdlib/, got: {}",
        resolved.display()
    );
    assert!(
        resolved.to_string_lossy().ends_with("string.kry"),
        "expected path to end with string.kry, got: {}",
        resolved.display()
    );
}

#[test]
fn resolve_std_nonexistent_fails() {
    // Verify that `use std::nonexistent` fails since there's no such stdlib module.
    let dir = std::env::temp_dir().join("kryos_resolve_std_nonexistent");
    fs::create_dir_all(&dir).unwrap();

    let main_path = dir.join("main.kry");
    fs::write(&main_path, "fn main() {}\n").unwrap();

    let result = kryos_driver::resolve::resolve_module_path(
        &["std".to_string(), "nonexistent_module".to_string()],
        &main_path,
    );
    assert!(result.is_err(), "expected std::nonexistent_module to fail");
}

#[test]
fn compile_file_with_std_import() {
    // End-to-end test: compile a file that uses `use std::mymod` where mymod
    // is a simple module in a local stdlib/ directory.
    //
    // We place the main file inside the compiler tree so the CARGO_MANIFEST_DIR
    // walk-up finds the real stdlib/, and we temporarily create a simple test
    // module there. The module is created and removed within this test.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut stdlib_dir = PathBuf::from(manifest_dir);
    loop {
        let candidate = stdlib_dir.join("stdlib");
        if candidate.is_dir() {
            stdlib_dir = candidate;
            break;
        }
        assert!(
            stdlib_dir.pop(),
            "could not find stdlib/ from CARGO_MANIFEST_DIR"
        );
    }

    // Use a unique name to avoid conflicts with other tests.
    let test_mod_name = "_driver_test_std_import";
    let test_module_path = stdlib_dir.join(format!("{test_mod_name}.kry"));
    let mymod_src = "fn helper(x: i32) -> i32 {\n    return x + 1\n}\n";
    fs::write(&test_module_path, mymod_src).unwrap();

    let dir = std::env::temp_dir().join("kryos_std_import_e2e");
    fs::create_dir_all(&dir).unwrap();

    let main_src =
        format!("use std::{test_mod_name}\n\nfn main() {{\n    let result = helper(41)\n}}\n");
    let main_path = dir.join("main.kry");
    fs::write(&main_path, &main_src).unwrap();

    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
        skip_ownership: false,
    lto: false,
    debug_info: false,
    use_cache: false,
    split_async_awaits: true,
    };

    let result = compile_file(&main_path, &config);

    // Clean up the temporary test module from stdlib/.
    let _ = fs::remove_file(&test_module_path);

    assert!(
        result.success,
        "expected std:: import to succeed, but got errors: {:?}",
        result.diagnostics
    );
    assert_eq!(
        result.error_count(),
        0,
        "expected no errors, got: {:?}",
        result.errors()
    );
    assert!(result.mir.is_some(), "expected MIR to be produced");

    // The MIR should contain functions from the stdlib module and main.
    let mir = result.mir.unwrap();
    let func_names: Vec<&str> = mir.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(
        func_names.contains(&"helper"),
        "expected 'helper' from std:: import in merged MIR, got: {func_names:?}"
    );
    assert!(
        func_names.contains(&"main"),
        "expected 'main' in merged MIR, got: {func_names:?}"
    );
}

// ---------------------------------------------------------------------------
// Async lowering integration (Item 8 Stage 3)
// ---------------------------------------------------------------------------

/// Source containing a single `async fn` plus `main`. The async pass should
/// see this, produce a plan, and stamp a `__kryos_state_compute` struct.
const ASYNC_SOURCE: &str = r#"async fn compute(x: i64) -> i64 {
    return x + 1
}

fn main() {
    let _ = compute(41)
}
"#;

#[test]
fn async_function_flagged_as_async_in_mir() {
    let path = temp_kry_file("async_flag.kry", ASYNC_SOURCE);
    let config = BuildConfig {
        input: path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
        skip_ownership: false,
        lto: false,
        debug_info: false,
        use_cache: false,
        split_async_awaits: true,
    };

    let result = compile_file(&path, &config);
    assert!(result.success, "compile failed: {:?}", result.errors());
    let mir = result.mir.expect("MIR should be produced");

    // The MIR function should carry is_async = true.
    let compute = mir
        .functions
        .iter()
        .find(|f| f.name == "compute")
        .expect("compute fn must exist in MIR");
    assert!(
        compute.attributes.is_async,
        "compute should have is_async = true in MIR attributes"
    );

    // The async_lower pass should have synthesised a state struct.
    let state_name = kryos_mir::async_lower::state_struct_name_for("compute");
    assert!(
        mir.struct_defs.contains_key(&state_name),
        "expected synthesised state struct `{state_name}`, got: {:?}",
        mir.struct_defs.keys().collect::<Vec<_>>()
    );

    // Non-async fn must remain unflagged.
    let main = mir
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("main fn must exist in MIR");
    assert!(
        !main.attributes.is_async,
        "main should NOT have is_async = true"
    );
}

#[test]
fn async_function_compiles_end_to_end() {
    // Sanity: a program containing async fn must still build cleanly
    // (the async pass should be a no-op semantically; await currently
    // lowers as a direct call).
    let path = temp_kry_file("async_e2e.kry", ASYNC_SOURCE);
    let config = BuildConfig {
        input: path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
        skip_ownership: false,
        lto: false,
        debug_info: false,
        use_cache: false,
        split_async_awaits: true,
    };

    let result = compile_file(&path, &config);
    assert!(
        result.success,
        "async program should compile: {:?}",
        result.errors()
    );
    assert_eq!(result.error_count(), 0);
}

#[test]
fn async_plus_inline_is_rejected() {
    // @inline + async is an explicit error in the async pass.
    let src = r#"@inline
async fn bad(x: i64) -> i64 { return x }

fn main() { let _ = bad(1) }
"#;
    let path = temp_kry_file("async_inline.kry", src);
    let config = BuildConfig {
        input: path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
        skip_ownership: false,
        lto: false,
        debug_info: false,
        use_cache: false,
        split_async_awaits: true,
    };

    let result = compile_file(&path, &config);
    assert!(!result.success, "expected compile failure for async+inline");
    let msgs: Vec<String> = result.errors().iter().map(|d| d.message.clone()).collect();
    assert!(
        msgs.iter().any(|m| m.contains("async lowering error")
            && m.contains("bad")
            && m.contains("inline")),
        "expected async-lowering error mentioning `bad` + inline, got: {msgs:?}"
    );
}

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

#[test]
fn version_is_correct() {
    assert_eq!(kryos_driver::version(), env!("CARGO_PKG_VERSION"));
}
