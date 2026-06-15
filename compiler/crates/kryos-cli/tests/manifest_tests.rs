use std::process::Command;

fn run(args: &[&str]) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_kryos"))
        .args(args)
        .output()
        .expect("spawn kryos");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    (stdout, out.status.code().unwrap_or(-1))
}

/// Like `run`, but also returns stderr (for warning-emission tests).
fn run_full(args: &[&str]) -> (String, String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_kryos"))
        .args(args)
        .output()
        .expect("spawn kryos");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    (stdout, stderr, out.status.code().unwrap_or(-1))
}

/// Normalize line endings before comparing: Windows `autocrlf` can check out the
/// golden `.json` files as CRLF while the binary always emits LF.
fn norm(s: &str) -> String {
    s.replace("\r\n", "\n").trim_end().to_string()
}

fn assert_golden(fixture: &str, golden: &str) {
    let (stdout, code) = run(&["manifest", "--caps", fixture]);
    assert_eq!(code, 0, "exit code for {fixture}");
    assert_eq!(norm(&stdout), norm(golden), "golden mismatch for {fixture}");
}

#[test]
fn manifest_golden_net_only() {
    assert_golden(
        "tests/manifest/net_only.kry",
        include_str!("manifest/expected/net_only.json"),
    );
}

#[test]
fn manifest_golden_multi_cap() {
    assert_golden(
        "tests/manifest/multi_cap.kry",
        include_str!("manifest/expected/multi_cap.json"),
    );
}

#[test]
fn manifest_golden_all_cap() {
    assert_golden(
        "tests/manifest/all_cap.kry",
        include_str!("manifest/expected/all_cap.json"),
    );
}

#[test]
fn manifest_golden_no_caps() {
    assert_golden(
        "tests/manifest/no_caps.kry",
        include_str!("manifest/expected/no_caps.json"),
    );
}

#[test]
fn manifest_golden_extern_block() {
    assert_golden(
        "tests/manifest/extern_block.kry",
        include_str!("manifest/expected/extern_block.json"),
    );
}

#[test]
fn manifest_golden_impl_methods() {
    assert_golden(
        "tests/manifest/impl_methods.kry",
        include_str!("manifest/expected/impl_methods.json"),
    );
}

#[test]
fn manifest_deny_net_trips() {
    let (_, code) = run(&[
        "manifest",
        "--caps",
        "tests/manifest/net_only.kry",
        "--deny",
        "net",
    ]);
    assert_eq!(code, 1);
}

#[test]
fn manifest_deny_io_passes() {
    let (_, code) = run(&[
        "manifest",
        "--caps",
        "tests/manifest/net_only.kry",
        "--deny",
        "io",
    ]);
    assert_eq!(code, 0);
}

#[test]
fn manifest_deny_ffi_extern_trips() {
    let (_, code) = run(&[
        "manifest",
        "--caps",
        "tests/manifest/extern_block.kry",
        "--deny",
        "ffi",
    ]);
    assert_eq!(code, 1);
}

#[test]
fn manifest_deny_all_trips_any() {
    let (_, code) = run(&[
        "manifest",
        "--caps",
        "tests/manifest/all_cap.kry",
        "--deny",
        "net",
    ]);
    assert_eq!(code, 1);
}

#[test]
fn manifest_strict_includes_unannotated() {
    let (stdout, _) = run(&[
        "manifest",
        "--caps",
        "tests/manifest/no_caps.kry",
        "--strict",
    ]);
    assert!(stdout.contains("\"add\""), "stdout should contain add: {stdout}");
    assert!(stdout.contains("\"annotated\": false"), "stdout should contain annotated: false: {stdout}");
    assert!(stdout.contains("\"unannotated_count\": 2"), "stdout should contain unannotated_count: 2: {stdout}");
}

#[test]
fn manifest_output_flag_writes_file() {
    let out_path = std::env::temp_dir().join("kryos_manifest_test_out.json");
    let out_str = out_path.to_str().unwrap();
    let (stdout, _code) = run(&[
        "manifest",
        "--caps",
        "tests/manifest/net_only.kry",
        "--output",
        out_str,
    ]);
    assert!(stdout.trim().is_empty(), "stdout should be empty when --output is set");
    let content = std::fs::read_to_string(&out_path).expect("output file should exist");
    let golden = include_str!("manifest/expected/net_only.json");
    assert_eq!(norm(&content), norm(golden));
}

#[test]
fn manifest_missing_path_errors() {
    let (_, code) = run(&[
        "manifest",
        "--caps",
        "tests/manifest/__nope__.kry",
    ]);
    assert_eq!(code, 1);
}

#[test]
fn manifest_format_pretty() {
    let (stdout, code) = run(&[
        "manifest",
        "--caps",
        "tests/manifest/net_only.kry",
        "--format",
        "pretty",
    ]);
    assert_eq!(code, 0, "exit code: {stdout}");
    assert!(
        !stdout.trim_start().starts_with('{'),
        "pretty output must not start with '{{': {stdout}"
    );
    assert!(
        stdout.contains("fetch"),
        "pretty output must contain function name 'fetch': {stdout}"
    );
}

#[test]
fn manifest_format_unknown_errors() {
    let (_, code) = run(&[
        "manifest",
        "--caps",
        "tests/manifest/net_only.kry",
        "--format",
        "xml",
    ]);
    assert_eq!(code, 1, "unknown --format should exit 1");
}

#[test]
fn manifest_parse_fail_warns() {
    // A file that fails to parse must be skipped AND surface a stderr warning,
    // so silent data loss is visible. Use a temp file to keep the fixture dir clean.
    let bad_path = std::env::temp_dir().join("kryos_manifest_test_bad_syntax.kry");
    std::fs::write(&bad_path, "fn {{{ ))) not valid kryos @@@\n").expect("write temp bad file");
    let bad_str = bad_path.to_str().unwrap();
    let (stdout, stderr, code) = run_full(&["manifest", "--caps", bad_str]);
    assert_eq!(code, 0, "parse-skip is non-fatal (exit 0): {stderr}");
    assert!(
        stdout.contains("\"functions\": {}"),
        "skipped file yields empty functions: {stdout}"
    );
    assert!(
        stderr.contains("warning") && stderr.contains("skipped"),
        "parse failure must emit a stderr warning: {stderr}"
    );
    let _ = std::fs::remove_file(&bad_path);
}

#[test]
fn manifest_dir_scan_shape() {
    // Directory input uses the DirManifest shape (schema + files[]) — a distinct
    // code path from single-file. expected/ holds only .json so it is skipped.
    let (stdout, code) = run(&["manifest", "--caps", "tests/manifest"]);
    assert_eq!(code, 0, "dir scan exit code: {stdout}");
    assert!(stdout.contains("\"kryos-manifest-v1\""), "dir output has schema: {stdout}");
    assert!(stdout.contains("\"files\""), "dir output has files array: {stdout}");
    // collect_kry sorts; all_cap.kry sorts first lexicographically.
    assert!(
        stdout.contains("net_only.kry") && stdout.contains("all_cap.kry"),
        "dir output lists fixture sources: {stdout}"
    );
}

#[test]
fn manifest_deny_unknown_cap_warns_and_passes() {
    // An unrecognized --deny capability is warned about and skipped, not fatal.
    let (_stdout, stderr, code) = run_full(&[
        "manifest",
        "--caps",
        "tests/manifest/net_only.kry",
        "--deny",
        "bogus",
    ]);
    assert_eq!(code, 0, "unknown deny cap must not trip the gate: {stderr}");
    assert!(stderr.contains("warning"), "unknown deny cap must warn: {stderr}");
}
