//! The stdlib audit of LEDGER bypass class B3, as a test.
//!
//! A stdlib function that exercises authority (a gated builtin, a gated native,
//! another function that does) must DECLARE it with `@capabilities(..)`. In the
//! default (inferred) mode an undeclared one is still charged to its caller by
//! inference, but in permissive mode an annotated caller is charged only for
//! callees that are annotated, and under `--strict-capabilities` the stdlib
//! function itself is rejected; an undeclared authority-bearing export is
//! therefore invisible exactly where the caller relies on the declaration.
//!
//! For every stdlib module this resolves `use std::<m>::*`, runs the real
//! capability checker over the merged program, and fails if a top-level
//! function of the module is unannotated yet has a non-empty effective
//! capability set, unless it is on the reviewed list below.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use kryos_ast::Decl;
use kryos_capabilities::{effective_function_capabilities, CapabilityMode};
use kryos_driver::resolve;

/// Unannotated functions that carry authority today, with the reason each is
/// not part of the B3 fix. Every entry must still be a real violation (so the
/// list can only shrink), and a NEW violation fails the test.
const KNOWN_UNDECLARED: &[(&str, &str, &str)] = &[
    (
        "io",
        "read_line",
        "reads stdin (ambient); the fs:read it shows is the bare-name union with BufReader.read_line",
    ),
    ("jwt", "sign_eddsa", "crypto, undeclared; jwt has no extern or raw memory (outside the audit set)"),
    ("jwt", "sign_hs256", "crypto, undeclared; jwt has no extern or raw memory (outside the audit set)"),
    ("jwt", "verify_eddsa", "crypto, undeclared; jwt has no extern or raw memory (outside the audit set)"),
    ("jwt", "verify_hs256", "crypto, undeclared; jwt has no extern or raw memory (outside the audit set)"),
    ("smtp", "_read_reply", "net, undeclared; smtp has no extern or raw memory (outside the audit set)"),
    ("smtp", "_send_line", "net, undeclared; smtp has no extern or raw memory (outside the audit set)"),
    ("smtp", "send_mail", "net, undeclared; smtp has no extern or raw memory (outside the audit set)"),
    ("test", "run_suite", "invokes caller-supplied closures: delegation, charged to the sender (B6, by design)"),
    ("test", "run_tests", "invokes caller-supplied closures: delegation, charged to the sender (B6, by design)"),
];

fn stdlib_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib")
}

fn parse_file(path: &Path) -> kryos_ast::Module {
    let src = kryos_driver::read_source(path).expect("read");
    let mut sm = kryos_errors::SourceMap::default();
    let id = sm.add_file(path.to_string_lossy().to_string(), src.clone());
    let toks = kryos_lexer::Lexer::new(&src, id).tokenize();
    kryos_parser::parse(toks).unwrap_or_else(|e| panic!("parse {}: {e:?}", path.display()))
}

fn has_caps(anns: &[kryos_ast::Annotation]) -> bool {
    anns.iter().any(|a| a.name == "capabilities")
}

/// (module, fn) of every unannotated top-level stdlib function with a body
/// whose effective capability set is non-empty, in `Inferred` mode. Computed
/// once per process: the tests of this file run on parallel threads, and two
/// computations would share (and delete) one probe directory.
fn undeclared_authority() -> &'static BTreeSet<(String, String)> {
    static RESULT: OnceLock<BTreeSet<(String, String)>> = OnceLock::new();
    RESULT.get_or_init(compute_undeclared_authority)
}

fn compute_undeclared_authority() -> BTreeSet<(String, String)> {
    let probe_dir = std::env::temp_dir().join(format!("kryos_stdlib_authority_{}", std::process::id()));
    fs::create_dir_all(&probe_dir).unwrap();
    let mut out = BTreeSet::new();
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
    assert!(modules.len() >= 60, "found only {} stdlib modules", modules.len());
    for m in modules {
        let probe = probe_dir.join(format!("probe_{m}.kry"));
        fs::write(&probe, format!("use std::{m}::*\nfn main() {{ }}\n")).unwrap();
        let root = parse_file(&probe);
        let mut visited = HashSet::new();
        visited.insert(fs::canonicalize(&probe).unwrap());
        let mut imported = Vec::new();
        resolve::reset_import_origins();
        resolve::resolve_imports(&root, &probe, &mut visited, &mut imported, false)
            .unwrap_or_else(|d| panic!("resolve std::{m}: {} diagnostics", d.len()));
        let mut merged = root;
        imported.append(&mut merged.declarations);
        merged.declarations = imported;

        let effective = effective_function_capabilities(&merged, CapabilityMode::Inferred);
        let own = parse_file(&stdlib_dir().join(format!("{m}.kry")));
        for d in &own.declarations {
            let Decl::Function {
                name,
                annotations,
                body: Some(_),
                ..
            } = d
            else {
                continue;
            };
            if has_caps(annotations) {
                continue;
            }
            let non_empty = effective
                .iter()
                .any(|(n, _annotated, caps)| n == name && !caps.is_empty());
            if non_empty {
                out.insert((m.clone(), name.clone()));
            }
        }
    }
    let _ = fs::remove_dir_all(&probe_dir);
    out
}

#[test]
fn no_stdlib_function_exercises_authority_without_declaring_it() {
    let found = undeclared_authority();
    let known: BTreeSet<(String, String)> = KNOWN_UNDECLARED
        .iter()
        .map(|(m, f, _)| (m.to_string(), f.to_string()))
        .collect();

    let new: Vec<String> = found
        .difference(&known)
        .map(|(m, f)| format!("std::{m}::{f}"))
        .collect();
    assert!(
        new.is_empty(),
        "stdlib function(s) exercise authority (a gated builtin, a gated native, or an annotated \
         callee) without a `@capabilities(..)` declaration; declare the capability of what each does:\n  {}",
        new.join("\n  ")
    );

    let stale: Vec<String> = known
        .difference(found)
        .map(|(m, f)| format!("std::{m}::{f}"))
        .collect();
    assert!(
        stale.is_empty(),
        "KNOWN_UNDECLARED lists function(s) that are no longer undeclared authority (remove them):\n  {}",
        stale.join("\n  ")
    );
}

#[test]
fn std_ffi_and_std_wasm_declare_everything_they_expose() {
    // The two modules B3 fixed. Spelled out so the fix cannot be reverted by
    // editing KNOWN_UNDECLARED.
    let found = undeclared_authority();
    for (m, f) in found {
        assert!(
            m != "ffi" && m != "wasm",
            "std::{m}::{f} exposes authority without declaring it (bypass B3)"
        );
    }
}
