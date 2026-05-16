//! DWARF integration test.
//!
//! Verifies that compiling a `.kry` file with `--release -g` produces a
//! native executable that:
//!   1. Runs and returns the expected exit code (the `-g` flag must not
//!      change program semantics).
//!   2. Contains a `.debug_info` ELF section (i.e. clang actually emitted
//!      DWARF, not just an empty `.debug_frame`).
//!   3. References the original `.kry` source file path in the compile
//!      unit, so `addr2line --functions` and stack traces can attribute
//!      frames back to Kryos source.
//!
//! This is the lightweight tier of DWARF support documented in
//! `kryos-codegen-llvm/src/codegen.rs::emit_dwarf_metadata` — full
//! per-function `DISubprogram` + per-instruction `!dbg` line info is a
//! v2.3 item gated on MIR-side span plumbing. This test guards what we
//! *do* emit today so it does not regress.
//!
//! Honors `KRYOS_SKIP_DWARF=1` (e.g. when `readelf` / `gdb` are missing
//! on the host) and runs only on Linux ELF targets.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn kryos_binary() -> PathBuf {
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
    panic!("kryos binary not found. Build with `cargo build --release -p kryos-cli` first.");
}

/// Returns true when `cmd` is in PATH (used to skip on hosts without
/// `readelf` or `addr2line`).
fn has_tool(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn dwarf_g_emits_debug_info_and_source_ref() {
    if std::env::var("KRYOS_SKIP_DWARF").is_ok() {
        eprintln!("KRYOS_SKIP_DWARF set — skipping DWARF integration test");
        return;
    }

    // ELF-only: macOS uses Mach-O + dSYM, Windows uses PDB. The current
    // emitter only targets the ELF/DWARF pair.
    if !cfg!(target_os = "linux") {
        eprintln!("not Linux — skipping DWARF integration test");
        return;
    }

    if !has_tool("readelf") {
        eprintln!("readelf not available — skipping DWARF integration test");
        return;
    }

    let kryos = kryos_binary();
    let out_dir = std::env::temp_dir().join("kryos_dwarf_test");
    fs::create_dir_all(&out_dir).expect("create temp dir");

    // Distinctive source name so we can find it in .debug_str / .debug_info.
    let src_path = out_dir.join("kryos_dwarf_probe.kry");
    let src = r#"
fn kryos_dwarf_probe_fn(x: i64) -> i64 {
    let mut sum: i64 = 0
    for i in range(0, x) {
        sum = sum + i
    }
    return sum
}

fn main() -> i64 {
    return kryos_dwarf_probe_fn(10)
}
"#;
    fs::write(&src_path, src).expect("write probe source");

    let exe_path = out_dir.join(if cfg!(windows) {
        "kryos_dwarf_probe.exe"
    } else {
        "kryos_dwarf_probe"
    });

    // Compile with --release -g.
    let compile = Command::new(&kryos)
        .args([
            std::ffi::OsStr::new("build"),
            std::ffi::OsStr::new("--release"),
            std::ffi::OsStr::new("-g"),
            std::ffi::OsStr::new("-o"),
            exe_path.as_os_str(),
            src_path.as_os_str(),
        ])
        .output()
        .expect("run kryos build");

    assert!(
        compile.status.success(),
        "compilation failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
    assert!(exe_path.exists(), "executable was not produced");

    // (1) Program semantics with -g unchanged: sum(0..10) = 45.
    let run = Command::new(&exe_path)
        .output()
        .expect("run produced executable");
    assert_eq!(
        run.status.code(),
        Some(45),
        "-g changed program semantics (expected exit 45):\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );

    // (2) Binary must contain a real `.debug_info` ELF section. A bare
    // `.debug_frame` from the runtime archive is not enough — we want
    // proof that clang ingested our `!DICompileUnit` and produced full
    // DWARF.
    let sections = Command::new("readelf")
        .args(["-S", &exe_path.to_string_lossy()])
        .output()
        .expect("run readelf -S");
    let sections_text = String::from_utf8_lossy(&sections.stdout);
    assert!(
        sections_text.contains(".debug_info"),
        ".debug_info section missing from binary (only got: {})",
        sections_text
            .lines()
            .filter(|l| l.contains("debug"))
            .collect::<Vec<_>>()
            .join(" | "),
    );

    // (3) The DWARF compile-unit (or .debug_str) must mention our source
    // file by name. Searching the raw binary is the most reliable check
    // — readelf's hex/text dump wraps text every 16 columns and would
    // split a long filename. The user source path lives verbatim in
    // either `.debug_str` or `.debug_line_str`.
    let bytes = fs::read(&exe_path).expect("read produced executable");
    let needle = b"kryos_dwarf_probe.kry";
    let has_source_name = bytes.windows(needle.len()).any(|w| w == needle);
    assert!(
        has_source_name,
        "binary does not contain user source name 'kryos_dwarf_probe.kry' \
         anywhere. This means !DIFile / !DICompileUnit was not emitted for \
         the user file.",
    );

    // (4) Producer tag should identify kryos. We emit
    // `producer: "kryos <version>"` in !DICompileUnit.
    let info_dump = Command::new("readelf")
        .args(["--debug-dump=info", "-N", &exe_path.to_string_lossy()])
        .output()
        .expect("run readelf --debug-dump=info");
    let info_text = String::from_utf8_lossy(&info_dump.stdout);
    let has_kryos_producer = info_text
        .lines()
        .any(|l| l.contains("DW_AT_producer") && l.contains("kryos "));
    assert!(
        has_kryos_producer,
        "no DW_AT_producer with 'kryos ' prefix found in .debug_info"
    );

    // Cleanup.
    fs::remove_file(&exe_path).ok();
    fs::remove_file(&src_path).ok();
}
