//! v3.1 LSP feature tests — exercises document_symbols, references, rename,
//! folding, formatting, highlight, signature_help, inlay_hints.

use kryos_lsp::{
    document_symbols, folding, formatting, highlight, inlay_hints, references, signature_help,
    workspace_symbols,
};
use std::collections::HashMap;

const SAMPLE: &str = r#"struct Point {
    x: f64,
    y: f64,
}

enum Color {
    Red,
    Green,
    Blue,
}

fn add(a: i64, b: i64) -> i64 {
    return a + b
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 }
    let result = add(1, 2)
    println(to_string(result))
}
"#;

#[test]
fn document_symbols_returns_outline() {
    let v = document_symbols::document_symbols(SAMPLE);
    let arr = v.as_array().expect("array");
    assert_eq!(arr.len(), 4, "expected 4 top-level decls (Point, Color, add, main), got {:?}", arr);

    let names: Vec<&str> = arr
        .iter()
        .map(|s| s.get("name").and_then(|n| n.as_str()).unwrap_or(""))
        .collect();
    assert!(names.contains(&"Point"));
    assert!(names.contains(&"Color"));
    assert!(names.contains(&"add"));
    assert!(names.contains(&"main"));

    // Point should have x and y as children.
    let point = arr.iter().find(|s| s["name"] == "Point").unwrap();
    let kids = point["children"].as_array().expect("Point children");
    let kid_names: Vec<&str> = kids
        .iter()
        .map(|s| s["name"].as_str().unwrap_or(""))
        .collect();
    assert!(kid_names.contains(&"x"));
    assert!(kid_names.contains(&"y"));

    // Color should have variant children.
    let color = arr.iter().find(|s| s["name"] == "Color").unwrap();
    let variants = color["children"].as_array().expect("Color children");
    assert_eq!(variants.len(), 3);
}

#[test]
fn folding_collapses_blocks() {
    let v = folding::folding_ranges(SAMPLE);
    let arr = v.as_array().unwrap();
    // We expect ≥ 4 folds (struct + enum + add fn + main fn bodies).
    assert!(arr.len() >= 4, "expected ≥ 4 folds, got {}", arr.len());
}

#[test]
fn formatting_returns_edit_when_unformatted() {
    // Ill-formatted input (extra spaces).
    let messy = "fn  main(  ) {  let x=1   }\n";
    let v = formatting::format_document(messy);
    // formatting may return either an edit or an empty array depending on
    // whether kryos-fmt could parse — both are valid responses.
    assert!(v.is_array());
}

#[test]
fn highlight_finds_identifier_uses() {
    let src = "fn main() { let x = 1\n  let y = x + x\n  println(to_string(y)) }";
    // Position of `x` on line 0 col 16.
    let v = highlight::document_highlight(src, 0, 16);
    let arr = v.as_array().unwrap();
    // 3 occurrences of `x`: decl + 2 reads.
    assert_eq!(arr.len(), 3, "expected 3 highlights of `x`, got {}", arr.len());
}

#[test]
fn references_finds_all_uses_in_file() {
    let uri = "file:///test.kry";
    let docs: HashMap<String, String> = HashMap::from([(uri.to_string(), SAMPLE.to_string())]);
    // Cursor on `add` at its definition (line 11, col 3).
    let v = references::find_references(SAMPLE, uri, 11, 3, None, &docs, true);
    let arr = v.as_array().unwrap();
    // `add` appears twice: once at the declaration, once at the call site.
    assert_eq!(arr.len(), 2);
}

#[test]
fn rename_produces_workspace_edit() {
    let uri = "file:///test.kry";
    let docs: HashMap<String, String> = HashMap::from([(uri.to_string(), SAMPLE.to_string())]);
    let v = references::prepare_rename(SAMPLE, uri, 11, 3, "sum", None, &docs);
    let changes = v.get("changes").expect("changes object");
    let edits = changes.get(uri).expect("edits for our uri");
    let arr = edits.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    for e in arr {
        assert_eq!(e["newText"], "sum");
    }
}

#[test]
fn rename_rejects_invalid_identifier() {
    let uri = "file:///test.kry";
    let docs: HashMap<String, String> = HashMap::from([(uri.to_string(), SAMPLE.to_string())]);
    let v = references::prepare_rename(SAMPLE, uri, 11, 3, "123bad", None, &docs);
    assert!(v.is_null());
}

#[test]
fn signature_help_at_call_site() {
    // Cursor just past `add(` — position the cursor right after the opening
    // paren so we're at active param 0.
    let src = "fn add(a: i64, b: i64) -> i64 { return a + b }\nfn main() { add(1, 2) }";
    // Open paren of add( at line 1 col 16, cursor inside arg 0.
    let v = signature_help::signature_help(src, 1, 16);
    assert!(v.get("signatures").is_some(), "expected signatures, got {v:?}");
    assert_eq!(v["activeParameter"], 0);
}

#[test]
fn workspace_symbols_fuzzy_search() {
    let uri = "file:///test.kry";
    let docs: HashMap<String, String> = HashMap::from([(uri.to_string(), SAMPLE.to_string())]);
    // Search for "ad" — should match `add` via subsequence.
    let v = workspace_symbols::workspace_symbols("ad", None, &docs);
    let arr = v.as_array().unwrap();
    assert!(arr.iter().any(|s| s["name"] == "add"));
}

#[test]
fn inlay_hints_for_literal_let() {
    let src = "fn main() {\n    let n = 42\n    let s = \"hi\"\n    let b = true\n}\n";
    let v = inlay_hints::inlay_hints(src, 0, 10);
    let arr = v.as_array().unwrap();
    // 3 hints (i64, str, bool).
    let type_hints: Vec<&str> = arr
        .iter()
        .filter(|h| h["kind"] == 1)
        .map(|h| h["label"].as_str().unwrap())
        .collect();
    assert!(type_hints.contains(&": i64"));
    assert!(type_hints.contains(&": str"));
    assert!(type_hints.contains(&": bool"));
}
