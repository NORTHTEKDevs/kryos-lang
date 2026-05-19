//! v3.14 LSP semantic-tokens tests.

use kryos_lsp::semantic_tokens;

#[test]
fn legend_lists_token_types() {
    let v = semantic_tokens::legend();
    let types = v.get("tokenTypes").and_then(|t| t.as_array()).unwrap();
    let names: Vec<&str> = types.iter().map(|s| s.as_str().unwrap()).collect();
    for expected in &[
        "keyword", "type", "function", "variable", "parameter",
        "string", "number", "comment", "operator", "property",
        "enumMember", "macro",
    ] {
        assert!(names.contains(expected), "missing token type {expected}");
    }
}

#[test]
fn classifies_hello_world() {
    let src = "fn main() {\n    println(\"Hello\")\n}\n";
    let v = semantic_tokens::semantic_tokens(src);
    let data = v.get("data").and_then(|d| d.as_array()).unwrap();
    // Expect at least 4 tokens: `fn`, `main`, `println`, `"Hello"`. Each is
    // 5 u32 entries. So at least 20.
    assert!(data.len() >= 20, "expected ≥ 20 entries, got {}", data.len());
}

#[test]
fn function_name_gets_function_token_type() {
    let src = "fn add(a: i64, b: i64) -> i64 { return a + b }\nfn main() { add(1, 2) }";
    let v = semantic_tokens::semantic_tokens(src);
    let data = v.get("data").and_then(|d| d.as_array()).unwrap();
    let entries: Vec<u32> = data.iter().filter_map(|n| n.as_u64().map(|x| x as u32)).collect();
    // Walk in groups of 5; the 4th entry of each group is the token type.
    let mut saw_function_type = false;
    for chunk in entries.chunks(5) {
        if chunk.len() == 5 && chunk[3] == 2 {
            // T_FUNCTION = 2
            saw_function_type = true;
            break;
        }
    }
    assert!(saw_function_type, "no function-typed token emitted for `add` or `main`");
}
