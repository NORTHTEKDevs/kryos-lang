//! SPEC-31 capability-surface LSP features: per-function capability inlay hints
//! and the strict-mode (`--strict-capabilities`) diagnostic.

use kryos_lsp::{diagnostics, inlay_hints};

// Four top-level functions:
//   read_config  — unannotated, uses file_read   -> needs `fs:read`
//   read_logged  — @capabilities(io), uses file_read (annotated, still surfaced)
//   fetch        — unannotated, uses http_get     -> needs `net:http`
//   pure_add     — no gated builtins              -> no capability surface
const FIXTURE: &str = r#"fn read_config() -> str {
    return file_read("config.txt")
}

@capabilities(io)
fn read_logged() -> str {
    return file_read("log.txt")
}

fn fetch() -> str {
    return http_get("http://example.com")
}

fn pure_add(a: i64, b: i64) -> i64 {
    return a + b
}
"#;

#[test]
fn capability_inlay_hints_show_computed_caps() {
    let v = inlay_hints::inlay_hints(FIXTURE, 0, 100);
    let arr = v.as_array().expect("array");

    let cap_hints: Vec<String> = arr
        .iter()
        .filter_map(|h| h["label"].as_str())
        .filter(|l| l.starts_with("caps:"))
        .map(|l| l.trim_end().to_string())
        .collect();

    // read_config (fs:read), read_logged (fs:read), fetch (net:http) — pure_add gets none.
    assert_eq!(
        cap_hints.len(),
        3,
        "expected 3 capability hints, got {cap_hints:?}"
    );
    assert_eq!(
        cap_hints.iter().filter(|l| *l == "caps: fs:read").count(),
        2,
        "expected two `caps: fs:read` hints, got {cap_hints:?}"
    );
    assert!(
        cap_hints.iter().any(|l| l == "caps: net:http"),
        "expected a `caps: net:http` hint, got {cap_hints:?}"
    );
}

#[test]
fn strict_mode_warns_on_unannotated_gated_fn() {
    let (diags, _) = diagnostics::check_source(FIXTURE, "file:///caps.kry");

    let warnings: Vec<&serde_json::Value> =
        diags.iter().filter(|d| d["code"] == "W0505").collect();

    // read_config (io) and fetch (net) are unannotated + gated; read_logged is
    // annotated; pure_add is pure — so exactly two strict-mode warnings.
    assert_eq!(
        warnings.len(),
        2,
        "expected 2 strict-mode warnings, got {warnings:?}"
    );
    for w in &warnings {
        assert_eq!(w["severity"], 2, "strict-mode diagnostic must be a warning");
    }

    let messages: Vec<&str> = warnings
        .iter()
        .filter_map(|w| w["message"].as_str())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("read_config") && m.contains("fs:read")),
        "expected a warning naming read_config/fs:read, got {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("fetch") && m.contains("net:http")),
        "expected a warning naming fetch/net:http, got {messages:?}"
    );
}

#[test]
fn annotated_and_pure_functions_get_no_strict_warning() {
    let (diags, _) = diagnostics::check_source(FIXTURE, "file:///caps.kry");
    let warnings: Vec<&serde_json::Value> =
        diags.iter().filter(|d| d["code"] == "W0505").collect();
    for w in &warnings {
        let msg = w["message"].as_str().unwrap_or("");
        assert!(
            !msg.contains("read_logged"),
            "annotated function must not warn: {msg}"
        );
        assert!(
            !msg.contains("pure_add"),
            "pure function must not warn: {msg}"
        );
    }
}
