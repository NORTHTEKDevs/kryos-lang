//! Integration tests for kryos-linker.

use std::path::PathBuf;
use kryos_linker::{
    Target, Arch, Os, Env,
    LinkerConfig, LinkType, LinkError,
    find_system_linker, build_command, command_to_args,
};

// ─── Target::host() ────────────────────────────────────────────────

#[test]
fn host_target_is_valid() {
    let host = Target::host();
    // Host must have a recognized arch
    assert!(
        matches!(host.arch, Arch::X86_64 | Arch::Aarch64 | Arch::Wasm32),
        "host arch should be recognized: {:?}",
        host.arch,
    );
    // Host must have a recognized OS
    assert!(
        matches!(host.os, Os::Linux | Os::Windows | Os::MacOS | Os::Wasi | Os::Unknown),
        "host OS should be recognized: {:?}",
        host.os,
    );
    // Triple string should be non-empty
    let triple = host.triple_string();
    assert!(!triple.is_empty());
    assert!(triple.contains('-'));
}

// ─── Target::from_triple() parsing ─────────────────────────────────

#[test]
fn parse_x86_64_linux_gnu() {
    let t = Target::from_triple("x86_64-unknown-linux-gnu").unwrap();
    assert_eq!(t.arch, Arch::X86_64);
    assert_eq!(t.os, Os::Linux);
    assert_eq!(t.env, Env::Gnu);
}

#[test]
fn parse_aarch64_apple_darwin() {
    let t = Target::from_triple("aarch64-apple-darwin").unwrap();
    assert_eq!(t.arch, Arch::Aarch64);
    assert_eq!(t.os, Os::MacOS);
    assert_eq!(t.env, Env::None);
}

#[test]
fn parse_x86_64_windows_msvc() {
    let t = Target::from_triple("x86_64-pc-windows-msvc").unwrap();
    assert_eq!(t.arch, Arch::X86_64);
    assert_eq!(t.os, Os::Windows);
    assert_eq!(t.env, Env::Msvc);
}

#[test]
fn parse_wasm32_unknown_unknown() {
    let t = Target::from_triple("wasm32-unknown-unknown").unwrap();
    assert_eq!(t.arch, Arch::Wasm32);
    assert_eq!(t.os, Os::Unknown);
    assert_eq!(t.env, Env::None);
}

#[test]
fn parse_x86_64_linux_musl() {
    let t = Target::from_triple("x86_64-unknown-linux-musl").unwrap();
    assert_eq!(t.arch, Arch::X86_64);
    assert_eq!(t.os, Os::Linux);
    assert_eq!(t.env, Env::Musl);
}

#[test]
fn parse_wasm32_wasi() {
    let t = Target::from_triple("wasm32-wasi").unwrap();
    assert_eq!(t.arch, Arch::Wasm32);
    assert_eq!(t.os, Os::Wasi);
    assert_eq!(t.env, Env::None);
}

// ─── Invalid triples ───────────────────────────────────────────────

#[test]
fn invalid_triple_single_component() {
    let err = Target::from_triple("x86_64");
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("too few components"));
}

#[test]
fn invalid_triple_bad_arch() {
    let err = Target::from_triple("mips64-unknown-linux-gnu");
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("unsupported architecture"));
}

#[test]
fn invalid_triple_too_many_parts() {
    let err = Target::from_triple("x86_64-unknown-linux-gnu-extra");
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("too many components"));
}

// ─── Triple roundtrip ──────────────────────────────────────────────

#[test]
fn triple_roundtrip_linux_gnu() {
    let original = "x86_64-unknown-linux-gnu";
    let t = Target::from_triple(original).unwrap();
    assert_eq!(t.triple_string(), original);
}

#[test]
fn triple_roundtrip_macos() {
    let original = "aarch64-apple-darwin";
    let t = Target::from_triple(original).unwrap();
    assert_eq!(t.triple_string(), original);
}

#[test]
fn triple_roundtrip_windows_msvc() {
    let original = "x86_64-pc-windows-msvc";
    let t = Target::from_triple(original).unwrap();
    assert_eq!(t.triple_string(), original);
}

#[test]
fn triple_roundtrip_wasm_unknown() {
    let t = Target::from_triple("wasm32-unknown-unknown").unwrap();
    assert_eq!(t.triple_string(), "wasm32-unknown-unknown");
}

// ─── is_wasm() ─────────────────────────────────────────────────────

#[test]
fn is_wasm_true_for_wasm32() {
    let t = Target::from_triple("wasm32-unknown-unknown").unwrap();
    assert!(t.is_wasm());
}

#[test]
fn is_wasm_false_for_x86_64() {
    let t = Target::from_triple("x86_64-unknown-linux-gnu").unwrap();
    assert!(!t.is_wasm());
}

#[test]
fn is_wasm_false_for_aarch64() {
    let t = Target::from_triple("aarch64-apple-darwin").unwrap();
    assert!(!t.is_wasm());
}

// ─── LinkerConfig command construction ─────────────────────────────

fn make_test_config(target: Target, link_type: LinkType) -> LinkerConfig {
    LinkerConfig {
        target,
        object_files: vec![PathBuf::from("main.o"), PathBuf::from("utils.o")],
        runtime_lib: Some(PathBuf::from("libkryos_rt.a")),
        stdlib_native: Some(PathBuf::from("libkryos_stdlib_native.a")),
        output: PathBuf::from("output_binary"),
        link_type,
        extra_libs: vec!["m".to_string(), "pthread".to_string()],
        extra_lib_dirs: vec![PathBuf::from("/usr/local/lib")],
    }
}

#[test]
fn unix_dynamic_command_line() {
    let target = Target { arch: Arch::X86_64, os: Os::Linux, env: Env::Gnu };
    let config = make_test_config(target.clone(), LinkType::Dynamic);
    let linker_path = PathBuf::from("/usr/bin/cc");
    let cmd = build_command(&linker_path, &config);
    let args = command_to_args(&cmd);

    // Should contain: cc -o output_binary main.o utils.o libs... -L... -l...
    assert!(args[0].contains("cc"));
    assert!(args.contains(&"-o".to_string()));
    assert!(args.contains(&"output_binary".to_string()));
    assert!(args.contains(&"main.o".to_string()));
    assert!(args.contains(&"utils.o".to_string()));
    assert!(args.contains(&"libkryos_rt.a".to_string()));
    assert!(args.contains(&"libkryos_stdlib_native.a".to_string()));
    assert!(args.contains(&"-lm".to_string()));
    assert!(args.contains(&"-lpthread".to_string()));
    // Dynamic should NOT have -static
    assert!(!args.contains(&"-static".to_string()));
}

#[test]
fn unix_static_command_line() {
    let target = Target { arch: Arch::X86_64, os: Os::Linux, env: Env::Gnu };
    let config = make_test_config(target, LinkType::Static);
    let linker_path = PathBuf::from("/usr/bin/cc");
    let cmd = build_command(&linker_path, &config);
    let args = command_to_args(&cmd);

    assert!(args.contains(&"-static".to_string()));
}

#[test]
fn unix_shared_lib_command_line() {
    let target = Target { arch: Arch::X86_64, os: Os::Linux, env: Env::Gnu };
    let config = make_test_config(target, LinkType::SharedLib);
    let linker_path = PathBuf::from("/usr/bin/cc");
    let cmd = build_command(&linker_path, &config);
    let args = command_to_args(&cmd);

    assert!(args.contains(&"-shared".to_string()));
}

#[test]
fn msvc_command_line() {
    let target = Target { arch: Arch::X86_64, os: Os::Windows, env: Env::Msvc };
    let config = make_test_config(target, LinkType::Dynamic);
    let linker_path = PathBuf::from("link.exe");
    let cmd = build_command(&linker_path, &config);
    let args = command_to_args(&cmd);

    assert!(args[0].contains("link.exe"));
    assert!(args.iter().any(|a| a.starts_with("/OUT:")));
    assert!(args.contains(&"main.o".to_string()));
    assert!(args.contains(&"utils.o".to_string()));
    assert!(args.iter().any(|a| a.starts_with("/LIBPATH:")));
    assert!(args.contains(&"m.lib".to_string()));
    assert!(args.contains(&"pthread.lib".to_string()));
}

#[test]
fn msvc_shared_lib_command_line() {
    let target = Target { arch: Arch::X86_64, os: Os::Windows, env: Env::Msvc };
    let config = make_test_config(target, LinkType::SharedLib);
    let linker_path = PathBuf::from("link.exe");
    let cmd = build_command(&linker_path, &config);
    let args = command_to_args(&cmd);

    assert!(args.contains(&"/DLL".to_string()));
}

#[test]
fn wasm_command_line() {
    let target = Target { arch: Arch::Wasm32, os: Os::Unknown, env: Env::None };
    let config = make_test_config(target, LinkType::Dynamic);
    let linker_path = PathBuf::from("wasm-ld");
    let cmd = build_command(&linker_path, &config);
    let args = command_to_args(&cmd);

    assert!(args[0].contains("wasm-ld"));
    assert!(args.contains(&"-o".to_string()));
    assert!(args.contains(&"--no-entry".to_string()));
    assert!(args.contains(&"--export-dynamic".to_string()));
    assert!(args.contains(&"main.o".to_string()));
}

#[test]
fn command_without_optional_libs() {
    let target = Target { arch: Arch::X86_64, os: Os::Linux, env: Env::Gnu };
    let config = LinkerConfig {
        target,
        object_files: vec![PathBuf::from("main.o")],
        runtime_lib: None,
        stdlib_native: None,
        output: PathBuf::from("a.out"),
        link_type: LinkType::Dynamic,
        extra_libs: vec![],
        extra_lib_dirs: vec![],
    };
    let linker_path = PathBuf::from("cc");
    let cmd = build_command(&linker_path, &config);
    let args = command_to_args(&cmd);

    // Should just be: cc -o a.out main.o
    assert_eq!(args.len(), 4);
    assert_eq!(args[1], "-o");
    assert_eq!(args[2], "a.out");
    assert_eq!(args[3], "main.o");
}

// ─── find_system_linker ────────────────────────────────────────────

#[test]
fn find_system_linker_on_current_host() {
    let host = Target::host();
    // On any CI or development machine, there should be at least one linker
    // available. This test verifies the search logic works end-to-end.
    match find_system_linker(&host) {
        Ok(path) => {
            assert!(!path.to_string_lossy().is_empty());
        }
        Err(msg) => {
            // On minimal systems without a C toolchain, this is acceptable
            eprintln!("note: no linker found for host target (this is OK in minimal envs): {msg}");
        }
    }
}

// ─── LinkError display ─────────────────────────────────────────────

#[test]
fn link_error_display_no_object_files() {
    let err = LinkError::NoObjectFiles;
    let msg = format!("{err}");
    assert!(msg.contains("no object files"));
}

#[test]
fn link_error_display_linker_not_found() {
    let err = LinkError::LinkerNotFound("no cc on PATH".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("linker not found"));
    assert!(msg.contains("no cc on PATH"));
}
