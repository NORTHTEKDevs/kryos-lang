//! Bypass class B3: a stdlib function that hands its CALLER raw process memory
//! is the authority itself, however it is spelled. The stdlib is exempt from
//! the raw-memory gate because it is built on the primitives; that exemption
//! is only sound while no stdlib function turns a primitive back into an API.
//! These tests keep the two lists the gate reads (`STD_FFI_EXPORTS`,
//! `STD_RAW_MEMORY_HELPERS`) equal to what the stdlib source really contains.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use kryos_capabilities::model::{STD_FFI_EXPORTS, STD_RAW_MEMORY_HELPERS};

fn stdlib_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib")
}

fn read(module: &str) -> String {
    fs::read_to_string(stdlib_dir().join(format!("{module}.kry"))).expect("read stdlib module")
}

/// `src` with comments blanked and string-literal contents blanked (newlines
/// and braces outside strings preserved), so brace matching and token search
/// see code only.
fn code_only(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut block_depth = 0usize;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if block_depth == 0 && c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            block_depth += 1;
            out.push_str("  ");
            i += 2;
            continue;
        }
        if block_depth > 0 {
            if c == '*' && next == Some('/') {
                block_depth -= 1;
                out.push_str("  ");
                i += 2;
            } else {
                out.push(if c == '\n' { '\n' } else { ' ' });
                i += 1;
            }
            continue;
        }
        if c == '"' {
            out.push('"');
            i += 1;
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' {
                    out.push(' ');
                    i += 1;
                }
                if i < b.len() {
                    out.push(if b[i] == '\n' { '\n' } else { ' ' });
                    i += 1;
                }
            }
            out.push('"');
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Every function definition in `code` (comments and strings blanked):
/// (name, text of the parameter list, body text). Bodiless declarations (extern
/// items, trait method signatures) are skipped.
fn functions(code: &str) -> Vec<(String, String, String)> {
    let chars: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < chars.len() {
        let at_word_start = i == 0 || !is_ident(chars[i - 1]);
        if at_word_start && chars[i] == 'f' && chars[i + 1] == 'n' && chars[i + 2].is_whitespace() {
            let mut j = i + 3;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let ns = j;
            while j < chars.len() && is_ident(chars[j]) {
                j += 1;
            }
            let name: String = chars[ns..j].iter().collect();
            // optional generic list, then the parameter list
            while j < chars.len() && chars[j] != '(' && chars[j] != '\n' {
                j += 1;
            }
            if name.is_empty() || j >= chars.len() || chars[j] != '(' {
                i += 3;
                continue;
            }
            let ps = j + 1;
            let mut depth = 1;
            j += 1;
            while j < chars.len() && depth > 0 {
                match chars[j] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let params: String = chars[ps..j - 1].iter().collect();
            // the body must open on this line (after an optional `-> type`)
            let mut k = j;
            while k < chars.len() && chars[k] != '{' && chars[k] != '\n' {
                k += 1;
            }
            if k >= chars.len() || chars[k] != '{' {
                i = j;
                continue;
            }
            let bs = k;
            let mut d = 0;
            let mut m = k;
            while m < chars.len() {
                match chars[m] {
                    '{' => d += 1,
                    '}' => {
                        d -= 1;
                        if d == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                m += 1;
            }
            let body: String = chars[bs..m.min(chars.len() - 1) + 1].iter().collect();
            out.push((name, params, body));
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// Parameter names declared `: i64` in a parameter list.
fn i64_params(params: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in params.split(',') {
        let mut it = part.splitn(2, ':');
        let name = it.next().unwrap_or("").trim().to_string();
        let ty = it.next().unwrap_or("").trim().to_string();
        if !name.is_empty() && ty == "i64" {
            out.push(name);
        }
    }
    out
}

#[test]
fn the_scanner_sees_the_functions_it_must() {
    // A scanner that finds nothing would make the tests below vacuous.
    let tensor = functions(&code_only(&read("tensor")));
    assert!(tensor.len() > 30, "tensor: {} functions", tensor.len());
    assert!(
        tensor.iter().any(|(n, p, b)| n == "arr_data_ptr"
            && i64_params(p) == ["header"]
            && b.contains("ptr_read_i64(header")),
        "the scanner must see tensor::arr_data_ptr(header: i64) reading `header`"
    );
    let crypto = functions(&code_only(&read("crypto")));
    assert!(crypto
        .iter()
        .any(|(n, p, b)| n == "_bytes_to_hex" && i64_params(p) == ["data", "length"] && b.contains("ptr_byte_at(data")));
}

#[test]
fn std_ffi_exports_match_the_module_and_are_all_gated() {
    let src = read("ffi");
    let lines: Vec<&str> = src.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut found: BTreeSet<String> = BTreeSet::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix("pub fn ") else {
            continue;
        };
        let name: String = rest.chars().take_while(|c| is_ident(*c)).collect();
        assert!(
            i > 0 && lines[i - 1].trim() == "@capabilities(ffi)",
            "std::ffi::{name} is exported without `@capabilities(ffi)` on the line above it: \
             a caller would not be charged for raw memory / a C call (bypass B3)"
        );
        found.insert(name);
    }
    let listed: BTreeSet<String> = STD_FFI_EXPORTS.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        found, listed,
        "STD_FFI_EXPORTS (kryos-capabilities/src/model.rs) must list exactly the `pub fn`s of \
         compiler/stdlib/ffi.kry: user modules importing those names are gated by it"
    );
    assert_eq!(found.len(), 32);
}

#[test]
fn no_stdlib_function_derefs_an_address_its_caller_supplies_unlisted() {
    // Raw-address parameters: a function `f(p: i64, ..)` whose body passes `p`
    // as the pointer of ptr_read_i64 / ptr_write_i64 / ptr_byte_at /
    // ptr_set_byte / free_bytes lets any caller read or write the memory at an
    // address of its choosing. Each such function must be gated in user code
    // (STD_FFI_EXPORTS, or STD_RAW_MEMORY_HELPERS for stdlib-internal helpers
    // that the flat namespace nevertheless exposes), or be rewritten so it does
    // not deref a caller-supplied integer.
    let gated: BTreeSet<&str> = STD_FFI_EXPORTS
        .iter()
        .chain(STD_RAW_MEMORY_HELPERS.iter())
        .copied()
        .collect();
    let sinks = ["ptr_read_i64", "ptr_write_i64", "ptr_byte_at", "ptr_set_byte", "free_bytes"];
    let mut offenders = Vec::new();
    let mut found_helpers = BTreeSet::new();
    let mut modules: Vec<String> = fs::read_dir(stdlib_dir())
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension().and_then(|x| x.to_str()) == Some("kry"))
                .then(|| p.file_stem().unwrap().to_string_lossy().to_string())
        })
        .collect();
    modules.sort();
    for m in modules {
        for (name, params, body) in functions(&code_only(&read(&m))) {
            let ptr_params = i64_params(&params);
            if ptr_params.is_empty() {
                continue;
            }
            for sink in sinks {
                let mut rest = body.as_str();
                while let Some(pos) = rest.find(sink) {
                    let before_ok = pos == 0 || !is_ident(rest[..pos].chars().last().unwrap());
                    let after = rest[pos + sink.len()..].trim_start();
                    if before_ok && after.starts_with('(') {
                        let arg = after[1..].trim_start();
                        let ident: String = arg.chars().take_while(|c| is_ident(*c)).collect();
                        let next = arg[ident.len()..].trim_start().chars().next();
                        // the pointer argument is exactly a bare `i64` parameter
                        if ptr_params.contains(&ident) && matches!(next, Some(',') | Some(')')) {
                            if gated.contains(name.as_str()) {
                                found_helpers.insert(name.clone());
                            } else {
                                offenders.push(format!("std::{m}::{name}({params}) -> {sink}({ident}, ..)"));
                            }
                        }
                    }
                    rest = &rest[pos + sink.len()..];
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "stdlib function(s) dereference an address their caller supplies and are not gated in user \
         code (add to STD_RAW_MEMORY_HELPERS in kryos-capabilities/src/model.rs, or stop taking a \
         raw address):\n  {}",
        offenders.join("\n  ")
    );
    // The listed helpers must still exist and still be what the list says.
    for helper in STD_RAW_MEMORY_HELPERS {
        assert!(
            found_helpers.contains(*helper),
            "STD_RAW_MEMORY_HELPERS lists `{helper}` but no stdlib function of that name still \
             dereferences an i64 parameter (stale entry: remove it)"
        );
    }
}
