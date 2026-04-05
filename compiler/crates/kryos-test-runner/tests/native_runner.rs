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
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/kryos");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    assert!(
        path.exists(),
        "kryos binary not found at {}. Run `cargo build -p kryos-cli` first.",
        path.display()
    );
    path
}

fn run_native_test(test: &NativeTest) -> Result<(), String> {
    let kryos = kryos_binary();
    let out_dir = std::env::temp_dir().join("kryos_native_tests");
    fs::create_dir_all(&out_dir).ok();

    let exe_name = if cfg!(windows) {
        format!("{}.exe", test.name)
    } else {
        test.name.clone()
    };
    let exe_path = out_dir.join(&exe_name);

    // Compile .kry to native executable.
    let compile = Command::new(&kryos)
        .args(["build", "-o"])
        .arg(&exe_path)
        .arg(&test.source)
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
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "kry"))
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
