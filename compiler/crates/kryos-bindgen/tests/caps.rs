//! Integration tests for capability inference in kryos-bindgen.
//!
//! Every tag is SUGGESTED (heuristic, verify), never authoritative: these tests
//! pin the heuristic, the emitted artifacts, and the human-override path.

use std::collections::BTreeMap;

use kryos_bindgen::c_parser::parse_c_header;
use kryos_bindgen::{
    apply_overrides, classify, generate_caps_bindings, generate_caps_toml, generate_wrappers,
    parse_overrides, CapTag, Capability,
};

/// socket -> net, fread -> io, strlen -> compute (the three canonical symbols).
const HEADER: &str = r#"
int socket(int domain, int kind, int protocol);
unsigned long fread(void* ptr, unsigned long size, unsigned long nmemb, void* stream);
unsigned long strlen(const char* s);
"#;

fn cap_for(tags: &[CapTag], sym: &str) -> Capability {
    tags.iter()
        .find(|t| t.symbol == sym)
        .unwrap_or_else(|| panic!("no tag for {sym}"))
        .capability
}

#[test]
fn classifies_canonical_symbols() {
    let items = parse_c_header(HEADER).unwrap();
    let tags = classify(&items);
    assert_eq!(cap_for(&tags, "socket"), Capability::Net);
    assert_eq!(cap_for(&tags, "fread"), Capability::Io);
    assert_eq!(cap_for(&tags, "strlen"), Capability::Compute);
}

#[test]
fn caps_toml_is_reviewable_with_confidence_notes() {
    let items = parse_c_header(HEADER).unwrap();
    let tags = classify(&items);
    let toml = generate_caps_toml(&tags, &BTreeMap::new());

    // Framed as suggested, never authoritative.
    assert!(toml.contains("HEURISTIC GUESS"));
    assert!(toml.contains("NOT authoritative"));

    // Each function gets a suggestion table + a "heuristic, verify" note.
    assert!(toml.contains("[suggestions.socket]"));
    assert!(toml.contains("capability = \"net\""));
    assert!(toml.contains("[suggestions.fread]"));
    assert!(toml.contains("capability = \"io\""));
    assert!(toml.contains("[suggestions.strlen]"));
    assert!(toml.contains("capability = \"compute\""));
    assert!(toml.contains("heuristic, verify"));

    // An overrides table the developer can promote corrections into.
    assert!(toml.contains("[overrides]"));
}

#[test]
fn wrapper_stub_carries_suggested_annotation() {
    let items = parse_c_header(HEADER).unwrap();
    let tags = classify(&items);
    let wrappers = generate_wrappers(&items, &tags);

    // Each wrapper fn carries its suggested @capabilities tag, directly above
    // the fn signature.
    assert!(wrappers.contains("@capabilities(net)\nfn socket("));
    assert!(wrappers.contains("@capabilities(io)\nfn fread("));
    assert!(wrappers.contains("@capabilities(compute)\nfn strlen("));

    // Stub bodies only -- no real implementation.
    assert!(wrappers.contains("stub"));
    assert!(wrappers.contains("panic("));
    // Framed as suggested, verify.
    assert!(wrappers.contains("SUGGESTED"));
}

#[test]
fn override_wins_over_heuristic() {
    let items = parse_c_header(HEADER).unwrap();
    let mut tags = classify(&items);
    assert_eq!(cap_for(&tags, "socket"), Capability::Net); // heuristic guess

    // A human deliberately overrides socket to a different capability.
    let toml = r#"
[overrides]
socket = "compute"
"#;
    let overrides = parse_overrides(toml);
    assert_eq!(overrides.get("socket"), Some(&Capability::Compute));

    apply_overrides(&mut tags, &overrides);
    assert_eq!(cap_for(&tags, "socket"), Capability::Compute); // override wins

    // ...and the regenerated wrapper reflects the override.
    let wrappers = generate_wrappers(&items, &tags);
    assert!(wrappers.contains("@capabilities(compute)\nfn socket("));
    assert!(!wrappers.contains("@capabilities(net)\nfn socket("));
}

#[test]
fn unknown_symbol_defaults_to_low_confidence_compute() {
    let items = parse_c_header("int frobnicate(int x);").unwrap();
    let tags = classify(&items);
    let tag = tags.iter().find(|t| t.symbol == "frobnicate").unwrap();
    assert_eq!(tag.capability, Capability::Compute);
    assert!(tag.note().contains("low confidence"));
    assert!(tag.note().contains("verify"));
}

#[test]
fn deliberately_misclassified_symbol_is_overridable() {
    // `audit_log` does I/O, but no pattern matches it -> low-confidence compute.
    let header = "void audit_log(const char* msg);";
    let out = generate_caps_bindings(header, None).unwrap();
    let tag = out.tags.iter().find(|t| t.symbol == "audit_log").unwrap();
    assert_eq!(tag.capability, Capability::Compute); // misclassified guess

    // The developer corrects it in the toml; the override flows through to the
    // effective tags, the wrapper, and the re-emitted toml.
    let corrected = r#"
[overrides]
audit_log = "io"
"#;
    let out = generate_caps_bindings(header, Some(corrected)).unwrap();
    let tag = out.tags.iter().find(|t| t.symbol == "audit_log").unwrap();
    assert_eq!(tag.capability, Capability::Io);
    assert!(out.wrappers.contains("@capabilities(io)\nfn audit_log("));
    // The override is preserved in the re-emitted toml so the file round-trips.
    assert!(out.caps_toml.contains("audit_log = \"io\""));
}
