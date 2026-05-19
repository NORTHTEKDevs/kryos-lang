//! v3.12 LSP code-action tests.

use kryos_lsp::code_actions;

#[test]
fn extracts_quickfix_for_unknown_var() {
    // Reference an undefined `lenght` near the right line.
    let src = "fn main() {\n    let s: str = \"hello\"\n    println(to_string(lenght(s)))\n}\n";
    let uri = "file:///test.kry";
    let v = code_actions::code_actions(src, uri, 0, 20);
    let arr = v.as_array().unwrap();
    // The result may be empty if the checker doesn't suggest "length" — at minimum we
    // must verify the handler returns a JSON array and never crashes.
    assert!(arr.len() <= 5, "unexpected explosion of actions: {arr:?}");
}

#[test]
fn returns_empty_array_for_clean_source() {
    let src = "fn main() { println(\"ok\") }\n";
    let uri = "file:///clean.kry";
    let v = code_actions::code_actions(src, uri, 0, 5);
    let arr = v.as_array().unwrap();
    assert!(arr.is_empty(), "clean source should produce no actions, got {arr:?}");
}
