//! Bypass class B4: a `kryos_*` native nobody classified used to be AMBIENT, so
//! `extern { fn kryos_ffi_cstr(..) }` read arbitrary memory from a program that
//! declared no capability. The fix fails closed (an unclassified native needs
//! `ffi`); this test is what makes a FUTURE native impossible to add without a
//! decision: it enumerates every `#[no_mangle] extern "C"` symbol of
//! `kryos-rt` and `kryos-stdlib-native` from their source and every `kryos_*`
//! extern the stdlib declares, and fails if one is missing from
//! `kryos_capabilities::native_symbols`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use kryos_capabilities::model::{required_capability_for_native_symbol, spelled_native_symbol_capability};
use kryos_capabilities::native_symbols::{
    classified_native_symbols, classify_native_symbol, NativeClass, AMBIENT_NATIVES, CAPABILITY_NATIVES,
};
use kryos_capabilities::Capability;

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn stdlib_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib")
}

fn files_with_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        panic!("cannot read {}", dir.display());
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            files_with_ext(&p, ext, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some(ext) {
            out.push(p);
        }
    }
}

fn ident_at(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// Every symbol a source file exports with `#[no_mangle]`: the `extern "C" fn
/// NAME` that follows the attribute within a few lines, plus the name passed
/// as the first argument of a `macro_rules!` macro whose body carries
/// `#[no_mangle]` (`dlcall_arity!(kryos_ffi_dlcall0, ..)`).
fn exported_symbols(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let lines: Vec<&str> = src.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if !(t.starts_with("#[no_mangle]") || t.starts_with("#[unsafe(no_mangle)]")) {
            continue;
        }
        // `#[no_mangle] pub extern "C" fn NAME` on one line, or on a later one.
        for follow in lines.iter().skip(i).take(6) {
            if let Some(pos) = follow.find("extern \"C\" fn ") {
                let name = ident_at(&follow[pos + "extern \"C\" fn ".len()..]);
                if name.starts_with("kryos_") {
                    out.insert(name);
                }
                break;
            }
        }
    }
    // Macro-generated exports.
    let mut exporting_macros = BTreeSet::new();
    let mut rest = src;
    while let Some(pos) = rest.find("macro_rules!") {
        let after = &rest[pos + "macro_rules!".len()..];
        let name = ident_at(after.trim_start());
        let Some(open) = after.find('{') else { break };
        let mut depth = 0usize;
        let mut end = after.len();
        for (j, ch) in after[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + j;
                        break;
                    }
                }
                _ => {}
            }
        }
        if after[open..end].contains("no_mangle") {
            exporting_macros.insert(name);
        }
        rest = &after[end.min(after.len())..];
    }
    for m in &exporting_macros {
        let needle = format!("{m}!(");
        let mut hay = src;
        while let Some(pos) = hay.find(&needle) {
            let tail = hay[pos + needle.len()..].trim_start();
            let name = ident_at(tail);
            if name.starts_with("kryos_") {
                out.insert(name);
            }
            hay = &hay[pos + needle.len()..];
        }
    }
    out
}

/// symbol (with `kryos_`) -> a file that exports it
fn runtime_exports() -> BTreeMap<String, PathBuf> {
    let mut files = Vec::new();
    for krate in ["kryos-rt/src", "kryos-stdlib-native/src"] {
        files_with_ext(&crates_dir().join(krate), "rs", &mut files);
    }
    let mut map = BTreeMap::new();
    for f in files {
        let src = fs::read_to_string(&f).expect("read runtime source");
        for sym in exported_symbols(&src) {
            map.entry(sym).or_insert_with(|| f.clone());
        }
    }
    map
}

/// Every `kryos_*` name declared as an extern function by a stdlib module.
fn stdlib_extern_names() -> BTreeMap<String, String> {
    let mut files = Vec::new();
    files_with_ext(&stdlib_dir(), "kry", &mut files);
    let mut map = BTreeMap::new();
    for f in files {
        let src = fs::read_to_string(&f).expect("read stdlib source");
        let module = f.file_stem().unwrap().to_string_lossy().to_string();
        for line in src.lines() {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix("fn kryos_") {
                map.entry(format!("kryos_{}", ident_at(rest)))
                    .or_insert_with(|| module.clone());
            }
        }
    }
    map
}

fn strip(sym: &str) -> &str {
    sym.strip_prefix("kryos_").unwrap_or(sym)
}

#[test]
fn the_scan_finds_the_runtime() {
    // A broken scanner would make the other tests vacuously pass.
    let exports = runtime_exports();
    assert!(
        exports.len() > 550,
        "expected 600+ exported kryos_* symbols, the scan found {}",
        exports.len()
    );
    for must in [
        "kryos_ffi_cstr",
        "kryos_ffi_read_i8",
        "kryos_ffi_dlcall0",
        "kryos_ffi_dlcall8",
        "kryos_db_open",
        "kryos_tcp_connect",
        "kryos_array_new",
        "kryos_builtin_len",
    ] {
        assert!(exports.contains_key(must), "scan missed {must}");
    }
    let std_externs = stdlib_extern_names();
    assert!(std_externs.contains_key("kryos_ffi_cstr"));
    assert!(std_externs.len() > 100, "stdlib extern scan found {}", std_externs.len());
}

#[test]
fn every_exported_native_is_classified() {
    let exports = runtime_exports();
    let unclassified: Vec<String> = exports
        .iter()
        .filter(|(sym, _)| classify_native_symbol(strip(sym)).is_none())
        .map(|(sym, file)| format!("{sym}  ({})", file.display()))
        .collect();
    assert!(
        unclassified.is_empty(),
        "{} exported native(s) are not classified in kryos-capabilities/src/native_symbols.rs. \
         Decide for each: does calling it exercise authority (add it to CAPABILITY_NATIVES with \
         the capability a caller must hold; use Capability::Ffi for raw memory / pointer sources / \
         C calls), or is it authority-free (add it to AMBIENT_NATIVES)? An unclassified native \
         fails closed (requires `ffi`) but must still be decided explicitly:\n  {}",
        unclassified.len(),
        unclassified.join("\n  ")
    );
}

#[test]
fn every_stdlib_extern_is_classified() {
    let unclassified: Vec<String> = stdlib_extern_names()
        .iter()
        .filter(|(sym, _)| classify_native_symbol(strip(sym)).is_none())
        .map(|(sym, module)| format!("{sym}  (declared by std::{module})"))
        .collect();
    assert!(
        unclassified.is_empty(),
        "stdlib extern(s) without an explicit classification (they would demand `ffi` from every \
         caller of the wrapper):\n  {}",
        unclassified.join("\n  ")
    );
}

#[test]
fn no_stale_or_duplicate_entries() {
    let exports: BTreeSet<String> = runtime_exports().into_keys().collect();
    let host_imports: BTreeSet<String> = stdlib_extern_names().into_keys().collect();
    let mut seen = BTreeSet::new();
    for name in classified_native_symbols() {
        assert!(seen.insert(name), "`{name}` is classified twice");
        let full = format!("kryos_{name}");
        assert!(
            exports.contains(&full) || host_imports.contains(&full),
            "`{name}` is classified but no runtime crate exports it and no stdlib module declares it \
             (stale entry: remove it)"
        );
    }
    // The two tables must not overlap (the OnceLock table also asserts this).
    let ambient: BTreeSet<&str> = AMBIENT_NATIVES.iter().copied().collect();
    for (name, _) in CAPABILITY_NATIVES {
        assert!(!ambient.contains(name), "`{name}` is in both tables");
    }
}

#[test]
fn classification_is_what_the_resolver_returns() {
    for (name, cap) in CAPABILITY_NATIVES {
        assert_eq!(
            required_capability_for_native_symbol(name),
            Some(*cap),
            "kryos_{name}"
        );
    }
    for name in AMBIENT_NATIVES {
        assert_eq!(required_capability_for_native_symbol(name), None, "kryos_{name}");
    }
}

#[test]
fn the_table_never_loosens_what_the_spelling_already_required() {
    // Before B4 a native's capability came from its SPELLING (`_ks` / `builtin_`
    // normalisation and the verb-order table). The explicit table replaces that
    // for every runtime symbol; it must never answer with LESS authority.
    for name in classified_native_symbols() {
        let Some(spelled) = spelled_native_symbol_capability(name) else {
            continue;
        };
        let class = classify_native_symbol(name).unwrap();
        match class {
            NativeClass::Requires(c) => assert!(
                c == spelled || c.satisfies(&spelled) || spelled.satisfies(&c) && c == Capability::FsWrite,
                "kryos_{name}: spelling says {spelled}, the table says {c}"
            ),
            NativeClass::Ambient => panic!("kryos_{name}: spelling says {spelled}, the table says ambient"),
        }
    }
}

#[test]
fn unclassified_natives_fail_closed() {
    for name in [
        "this_symbol_does_not_exist_in_any_table",
        "ffi_new_thing",
        "db_new_thing",
        "anything_at_all",
    ] {
        assert_eq!(
            required_capability_for_native_symbol(name),
            Some(Capability::Ffi),
            "kryos_{name} must require ffi"
        );
    }
}

#[test]
fn spot_checks_of_the_classes_the_repros_use() {
    use Capability::*;
    for (sym, want) in [
        ("ffi_cstr", Some(Ffi)),
        ("ffi_read_i8", Some(Ffi)),
        ("ffi_dlopen", Some(Ffi)),
        ("ffi_dlsym", Some(Ffi)),
        ("ffi_dlcall0", Some(Ffi)),
        ("ffi_dlcall6", Some(Ffi)),
        ("ffi_malloc", Some(Ffi)),
        ("ptr_read_i64", Some(Ffi)),
        ("ptr_byte_at", Some(Ffi)),
        ("alloc_bytes", Some(Ffi)),
        ("str_to_ptr", Some(Ffi)),
        ("db_open", Some(Db)),
        ("db_exec", Some(Db)),
        ("tls_connect", Some(NetTcp)),
        ("tcp_connect", Some(NetTcp)),
        ("term_raw_enable", Some(Term)),
        ("fs_read", Some(FsRead)),
        ("fs_write", Some(FsWrite)),
        ("fs_rename", Some(FsWrite)),
        ("file_open", Some(FsWrite)),
        ("env_cwd", Some(Process)),
        ("env_get", Some(Process)),
        ("process_exec", Some(Process)),
        ("sha1", Some(Crypto)),
        ("http_fetch", Some(NetHttp)),
        ("path_absolute", Some(FsRead)),
        ("budget_active", None),
        ("array_new", None),
        ("string_new", None),
        ("regex_new", None),
        ("mutex_new", None),
        ("time_now_millis", None),
        ("stdout_write", None),
        ("tensor_zeros", None),
    ] {
        assert_eq!(required_capability_for_native_symbol(sym), want, "kryos_{sym}");
    }
}
