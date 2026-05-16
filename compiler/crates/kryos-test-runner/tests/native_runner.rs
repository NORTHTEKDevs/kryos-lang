//! Native build integration tests: compiles `.kry` files to native executables
//! and verifies stdout output and exit codes.
//!
//! Test annotations:
//! - `// expect-stdout: <text>` — stdout must contain this line
//! - `// expect-exit: <code>` — process must exit with this code
//! - `// skip` — skip this test

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct NativeTest {
    name: String,
    source: PathBuf,
    expected_stdout: Vec<String>,
    expected_exit: Option<i32>,
    skip: bool,
}

fn parse_native_test(path: &Path) -> NativeTest {
    let source = fs::read_to_string(path).expect("read test file");
    let name = path.file_stem().unwrap().to_string_lossy().to_string();

    let mut expected_stdout = Vec::new();
    let mut expected_exit = None;
    let mut skip = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed == "// skip" {
            skip = true;
        } else if let Some(rest) = trimmed.strip_prefix("// expect-stdout:") {
            expected_stdout.push(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("// expect-exit:") {
            expected_exit = rest.trim().parse().ok();
        }
    }

    NativeTest {
        name,
        source: path.to_path_buf(),
        expected_stdout,
        expected_exit,
        skip,
    }
}

fn kryos_binary() -> PathBuf {
    // Locate the kryos binary in the workspace target directory.
    // Prefer release binary (always built with --release due to memory
    // constraints), fall back to debug.
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for profile in &["release", "debug"] {
        let mut path = base.join("target").join(profile).join("kryos");
        if cfg!(windows) {
            path.set_extension("exe");
        }
        if path.exists() {
            return path;
        }
    }
    panic!("kryos binary not found. Run `cargo build --release -p kryos-cli -j 4` first.");
}

fn run_native_test(test: &NativeTest) -> Result<(), String> {
    run_native_test_inner(test, false)
}

fn run_native_test_release(test: &NativeTest) -> Result<(), String> {
    run_native_test_inner(test, true)
}

fn run_native_test_inner(test: &NativeTest, release: bool) -> Result<(), String> {
    let kryos = kryos_binary();
    let subdir = if release { "kryos_native_release_tests" } else { "kryos_native_tests" };
    let out_dir = std::env::temp_dir().join(subdir);
    fs::create_dir_all(&out_dir).ok();

    let exe_name = if cfg!(windows) {
        format!("{}.exe", test.name)
    } else {
        test.name.clone()
    };
    let exe_path = out_dir.join(&exe_name);

    // Compile .kry to native executable.
    let mut args: Vec<&std::ffi::OsStr> = vec!["build".as_ref()];
    if release {
        args.push("--release".as_ref());
    }
    args.push("-o".as_ref());
    args.push(exe_path.as_os_str());
    args.push(test.source.as_os_str());
    let compile = Command::new(&kryos)
        .args(&args)
        .output()
        .map_err(|e| format!("failed to run kryos: {e}"))?;

    if !compile.status.success() {
        let stderr = String::from_utf8_lossy(&compile.stderr);
        let stdout = String::from_utf8_lossy(&compile.stdout);
        return Err(format!(
            "compilation failed (exit {}):\nstdout: {stdout}\nstderr: {stderr}",
            compile.status.code().unwrap_or(-1)
        ));
    }

    // Run the compiled executable.
    let run = Command::new(&exe_path)
        .output()
        .map_err(|e| format!("failed to run executable: {e}"))?;

    let stdout = String::from_utf8_lossy(&run.stdout);
    let exit_code = run.status.code().unwrap_or(-1);

    // Verify exit code.
    if let Some(expected) = test.expected_exit {
        if exit_code != expected {
            return Err(format!(
                "expected exit code {expected}, got {exit_code}\nstdout: {stdout}"
            ));
        }
    }

    // Verify stdout lines.
    let stdout_lines: Vec<&str> = stdout.lines().collect();
    for (i, expected_line) in test.expected_stdout.iter().enumerate() {
        if let Some(actual) = stdout_lines.get(i) {
            if actual.trim() != expected_line.as_str() {
                return Err(format!(
                    "stdout line {}: expected '{}', got '{}'",
                    i + 1,
                    expected_line,
                    actual.trim()
                ));
            }
        } else {
            return Err(format!(
                "stdout line {} missing: expected '{}'",
                i + 1,
                expected_line
            ));
        }
    }

    // Clean up executable.
    fs::remove_file(&exe_path).ok();

    Ok(())
}

#[test]
fn native_build_tests() {
    let test_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("native");

    let mut entries: Vec<_> = fs::read_dir(&test_dir)
        .expect("read native test dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "kry"))
        .collect();
    entries.sort_by_key(|e| e.path());

    assert!(
        !entries.is_empty(),
        "no .kry test files found in {}",
        test_dir.display()
    );

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();

    for entry in &entries {
        let test = parse_native_test(&entry.path());

        if test.skip {
            eprintln!("  SKIP  {}", test.name);
            skipped += 1;
            continue;
        }

        match run_native_test(&test) {
            Ok(()) => {
                eprintln!("  PASS  {}", test.name);
                passed += 1;
            }
            Err(reason) => {
                eprintln!("  FAIL  {}: {}", test.name, reason);
                failures.push(format!("{}: {}", test.name, reason));
                failed += 1;
            }
        }
    }

    eprintln!(
        "\n  Native build tests: {} passed, {} failed, {} skipped\n",
        passed, failed, skipped
    );

    assert!(
        failures.is_empty(),
        "{} native build test(s) failed:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

// ---------------------------------------------------------------------------
// Release-mode (LLVM backend) suite
//
// This runs the same fixture corpus (plus the dedicated release_builtins/
// directory) through `kryos build --release`. The 2.0 LLVM blocker that
// hid for an entire minor version would have been caught immediately by
// this suite — it specifically validates user-facing-name → runtime-symbol
// translation in the LLVM codegen.
//
// Honors KRYOS_SKIP_RELEASE_NATIVE=1 for environments without clang.
// ---------------------------------------------------------------------------

fn collect_kry_tests(dir: &Path) -> Vec<NativeTest> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "kry"))
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths.iter().map(|p| parse_native_test(p)).collect()
}

#[test]
fn native_build_release_tests() {
    if std::env::var("KRYOS_SKIP_RELEASE_NATIVE").is_ok() {
        eprintln!("KRYOS_SKIP_RELEASE_NATIVE set — skipping --release native suite");
        return;
    }

    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let native_dir = base.join("native");
    let release_dir = base.join("release_builtins");

    let mut tests: Vec<NativeTest> = Vec::new();
    tests.extend(collect_kry_tests(&native_dir));
    tests.extend(collect_kry_tests(&release_dir));

    assert!(
        !tests.is_empty(),
        "no .kry test files found in {} or {}",
        native_dir.display(),
        release_dir.display()
    );

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();

    for test in &tests {
        if test.skip {
            eprintln!("  SKIP  {} [--release]", test.name);
            skipped += 1;
            continue;
        }
        match run_native_test_release(test) {
            Ok(()) => {
                eprintln!("  PASS  {} [--release]", test.name);
                passed += 1;
            }
            Err(reason) => {
                eprintln!("  FAIL  {} [--release]: {}", test.name, reason);
                failures.push(format!("{} [--release]: {}", test.name, reason));
                failed += 1;
            }
        }
    }

    eprintln!(
        "\n  Native build --release tests: {} passed, {} failed, {} skipped\n",
        passed, failed, skipped
    );

    assert!(
        failures.is_empty(),
        "{} --release native build test(s) failed:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
