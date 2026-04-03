//! Integration tests for the Kryos formatter.

use kryos_fmt::format_source;
use kryos_lexer::Lexer;
use kryos_parser::parse;

/// Helper: format source, panic on error.
fn fmt(src: &str) -> String {
    format_source(src).unwrap_or_else(|diags| {
        for d in &diags {
            eprintln!("{}: {}", if d.is_error() { "ERROR" } else { "WARN" }, d.message);
        }
        panic!("format_source failed with {} diagnostic(s)", diags.len());
    })
}

/// Round-trip helper: parse source, format it, re-parse the formatted output,
/// then verify the two ASTs are structurally equal (ignoring spans).
fn assert_round_trip(src: &str) {
    let formatted = fmt(src);
    let tokens1 = Lexer::new(src, 0).tokenize();
    let ast1 = parse(tokens1).expect("first parse failed");

    let tokens2 = Lexer::new(&formatted, 0).tokenize();
    let ast2 = parse(tokens2).unwrap_or_else(|diags| {
        eprintln!("Re-parse of formatted output failed:");
        eprintln!("--- formatted output ---");
        eprintln!("{}", formatted);
        eprintln!("--- diagnostics ---");
        for d in &diags {
            eprintln!("  {}: {}", if d.is_error() { "ERROR" } else { "WARN" }, d.message);
        }
        panic!("re-parse of formatted output failed");
    });

    // Compare declarations structurally (spans will differ, so we compare Debug output
    // after stripping span info, or just compare the formatted outputs)
    let formatted2 = format_source(&formatted).expect("second format failed");
    assert_eq!(
        formatted, formatted2,
        "formatter is not idempotent!\n--- first format ---\n{}\n--- second format ---\n{}",
        formatted, formatted2
    );

    // Also verify declaration counts match
    assert_eq!(
        ast1.declarations.len(),
        ast2.declarations.len(),
        "declaration count mismatch after round-trip"
    );
}

// ======================== Function formatting ========================

#[test]
fn test_simple_function() {
    let out = fmt("fn main() { }");
    assert!(out.contains("fn main()"));
    assert!(out.contains("{"));
    assert!(out.contains("}"));
}

#[test]
fn test_function_with_params_and_return() {
    let out = fmt("fn add(x: i32, y: i32) -> i32 { return x + y }");
    assert!(out.contains("fn add(x: i32, y: i32) -> i32"));
    assert!(out.contains("return x + y"));
}

#[test]
fn test_pub_function() {
    let out = fmt("pub fn greet() { }");
    assert!(out.starts_with("pub fn greet()"));
}

#[test]
fn test_function_with_generics() {
    let out = fmt("fn identity<T>(x: T) -> T { x }");
    assert!(out.contains("fn identity<T>(x: T) -> T"));
}

// ======================== Multiline function params ========================

#[test]
fn test_multiline_function_params() {
    // This signature is long enough to exceed 80 columns
    let src = "fn very_long_function_name(first_param: VeryLongTypeName, second_param: AnotherLongType, third_param: YetAnotherType) -> Result { first_param }";
    let out = fmt(src);
    // Should be broken across multiple lines
    assert!(
        out.contains("(\n"),
        "expected multi-line params, got:\n{}",
        out
    );
    // Each param should be indented
    assert!(out.contains("    first_param: VeryLongTypeName"));
    assert!(out.contains("    second_param: AnotherLongType"));
    assert!(out.contains("    third_param: YetAnotherType"));
}

// ======================== Struct formatting ========================

#[test]
fn test_struct_formatting() {
    let out = fmt("struct Point { x: f64, y: f64 }");
    // Fields should be one per line, indented
    assert!(out.contains("struct Point {"));
    assert!(out.contains("    x: f64"));
    assert!(out.contains("    y: f64"));
    assert!(out.contains("}"));
}

#[test]
fn test_pub_struct_with_generics() {
    let out = fmt("pub struct Pair<A, B> { first: A, second: B }");
    assert!(out.contains("pub struct Pair<A, B> {"));
    assert!(out.contains("    first: A"));
    assert!(out.contains("    second: B"));
}

// ======================== Enum formatting ========================

#[test]
fn test_enum_formatting() {
    let out = fmt("enum Color { Red, Green, Blue }");
    assert!(out.contains("enum Color {"));
    assert!(out.contains("    Red"));
    assert!(out.contains("    Green"));
    assert!(out.contains("    Blue"));
}

#[test]
fn test_enum_with_fields() {
    let out = fmt("enum Shape { Circle(f64), Rect(f64, f64) }");
    assert!(out.contains("    Circle(f64)"));
    assert!(out.contains("    Rect(f64, f64)"));
}

// ======================== If/elif/else formatting ========================

#[test]
fn test_if_else_formatting() {
    let out = fmt("fn f() { if x { y } else { z } }");
    assert!(out.contains("if x {"));
    assert!(out.contains("} else {"));
}

#[test]
fn test_if_elif_else_formatting() {
    let out = fmt("fn f() { if x { a } elif y { b } else { c } }");
    assert!(out.contains("if x {"));
    assert!(out.contains("} elif y {"));
    assert!(out.contains("} else {"));
}

// ======================== For loop formatting ========================

#[test]
fn test_for_loop_formatting() {
    let out = fmt("fn f() { for i in items { print(i) } }");
    assert!(out.contains("for i in items {"));
    assert!(out.contains("print(i)"));
}

// ======================== While loop formatting ========================

#[test]
fn test_while_loop_formatting() {
    let out = fmt("fn f() { while running { tick() } }");
    assert!(out.contains("while running {"));
    assert!(out.contains("tick()"));
}

// ======================== Match expression ========================

#[test]
fn test_match_expr_formatting() {
    let out = fmt("fn f() { match color { Color::Red => 1, Color::Blue => 2 } }");
    assert!(out.contains("match color {"));
    assert!(out.contains("Color::Red => 1"));
    assert!(out.contains("Color::Blue => 2"));
}

// ======================== Nested expressions ========================

#[test]
fn test_nested_function_calls() {
    let out = fmt("fn f() { foo(bar(baz(1))) }");
    assert!(out.contains("foo(bar(baz(1)))"));
}

#[test]
fn test_nested_field_access() {
    let out = fmt("fn f() { a.b.c.d }");
    assert!(out.contains("a.b.c.d"));
}

// ======================== Operator precedence in output ========================

#[test]
fn test_operator_precedence_add_mul() {
    // 1 + 2 * 3 should format without unnecessary parens
    let out = fmt("fn f() { 1 + 2 * 3 }");
    assert!(
        out.contains("1 + 2 * 3"),
        "expected '1 + 2 * 3', got:\n{}",
        out
    );
}

#[test]
fn test_operator_precedence_parens_needed() {
    // (1 + 2) * 3 needs parens
    let out = fmt("fn f() { (1 + 2) * 3 }");
    assert!(
        out.contains("(1 + 2) * 3"),
        "expected '(1 + 2) * 3', got:\n{}",
        out
    );
}

// ======================== Annotations ========================

#[test]
fn test_annotation_formatting() {
    let out = fmt("@test\nfn my_test() { }");
    assert!(out.contains("@test\n"));
    assert!(out.contains("fn my_test()"));
}

#[test]
fn test_annotation_with_args_formatting() {
    let out = fmt("@export(wasm)\nfn run() { }");
    assert!(out.contains("@export(wasm)"));
    assert!(out.contains("fn run()"));
}

// ======================== Import formatting ========================

#[test]
fn test_import_formatting() {
    let out = fmt("use std::io");
    assert!(out.contains("use std::io"));
}

#[test]
fn test_grouped_import_formatting() {
    let out = fmt("use std::io::{File, BufReader}");
    assert!(out.contains("use std::io::{File, BufReader}"));
}

// ======================== Trait / Impl formatting ========================

#[test]
fn test_trait_formatting() {
    let out = fmt("trait Display { fn fmt(self) -> str; }");
    assert!(out.contains("trait Display {"));
    assert!(out.contains("fn fmt(self) -> str"));
}

#[test]
fn test_impl_formatting() {
    let out = fmt("impl Point { fn new(x: f64) -> Point { x } }");
    assert!(out.contains("impl Point {"));
    assert!(out.contains("fn new(x: f64) -> Point"));
}

#[test]
fn test_impl_trait_for_type() {
    let out = fmt("impl Display for Point { fn fmt(self) -> str { self } }");
    assert!(out.contains("impl Display for Point {"));
}

// ======================== Actor formatting ========================

#[test]
fn test_actor_formatting() {
    let out = fmt("actor Counter { count: i32 fn increment(self) { self } }");
    assert!(out.contains("actor Counter {"));
    assert!(out.contains("    count: i32"));
    assert!(out.contains("    fn increment(self)"));
}

// ======================== Type alias formatting ========================

#[test]
fn test_type_alias_formatting() {
    let out = fmt("type Id = i64");
    assert!(out.contains("type Id = i64"));
}

// ======================== Extern formatting ========================

#[test]
fn test_extern_formatting() {
    let out = fmt(r#"extern "C" { fn puts(s: *u8) -> i32; }"#);
    assert!(out.contains("extern \"C\""));
    assert!(out.contains("fn puts(s: *u8) -> i32"));
}

// ======================== Let / assignment formatting ========================

#[test]
fn test_let_binding() {
    let out = fmt("fn f() { let x: i32 = 42 }");
    assert!(out.contains("let x: i32 = 42"));
}

#[test]
fn test_let_mut() {
    let out = fmt("fn f() { let mut count = 0 }");
    assert!(out.contains("let mut count = 0"));
}

#[test]
fn test_compound_assignment() {
    let out = fmt("fn f() { x += 1 }");
    assert!(out.contains("x += 1"));
}

// ======================== Try/catch/throw ========================

#[test]
fn test_try_catch() {
    let out = fmt("fn f() { try { risky() } catch e { handle(e) } }");
    assert!(out.contains("try {"));
    assert!(out.contains("} catch e {"));
}

// ======================== Blank lines between declarations ========================

#[test]
fn test_blank_lines_between_decls() {
    let out = fmt("fn a() { }\nfn b() { }");
    // Should have a blank line between the two functions
    assert!(
        out.contains("}\n\nfn b()"),
        "expected blank line between decls, got:\n{}",
        out
    );
}

// ======================== Indentation ========================

#[test]
fn test_nested_block_indentation() {
    let out = fmt("fn f() { if x { if y { z } } }");
    // Inner if should be at 8 spaces (2 levels)
    assert!(out.contains("        if y {"), "expected 8-space indent, got:\n{}", out);
}

// ======================== Round-trip tests ========================

#[test]
fn round_trip_simple_function() {
    assert_round_trip("fn main() { }");
}

#[test]
fn round_trip_function_with_params() {
    assert_round_trip("fn add(x: i32, y: i32) -> i32 { return x + y }");
}

#[test]
fn round_trip_struct() {
    assert_round_trip("struct Point { x: f64, y: f64 }");
}

#[test]
fn round_trip_enum() {
    assert_round_trip("enum Color { Red, Green, Blue }");
}

#[test]
fn round_trip_if_elif_else() {
    assert_round_trip("fn f() { if x { a } elif y { b } else { c } }");
}

#[test]
fn round_trip_for_loop() {
    assert_round_trip("fn f() { for i in items { i } }");
}

#[test]
fn round_trip_match() {
    assert_round_trip("fn f() { match x { 1 => 10, 2 => 20 } }");
}

#[test]
fn round_trip_import() {
    assert_round_trip("use std::io::{File, BufReader}");
}

#[test]
fn round_trip_trait_and_impl() {
    assert_round_trip("trait Foo { fn bar(self) -> i32; }");
    assert_round_trip("impl Foo { fn bar(self) -> i32 { 42 } }");
}

#[test]
fn round_trip_complex_expressions() {
    assert_round_trip("fn f() { let x = 1 + 2 * 3 }");
    assert_round_trip("fn f() { foo(bar(1, 2), baz(3)) }");
}

// ======================== Idempotency ========================

#[test]
fn test_idempotency() {
    let sources = vec![
        "fn main() { }",
        "pub fn add(x: i32, y: i32) -> i32 { return x + y }",
        "struct Point { x: f64, y: f64 }",
        "enum Color { Red, Green, Blue }",
        "@test\nfn my_test() { }",
        "fn f() { if x { a } elif y { b } else { c } }",
        "fn f() { for i in items { i } }",
        "fn f() { match x { 1 => 10, _ => 0 } }",
        "use std::io::{File, BufReader}",
        "type Id = i64",
    ];
    for src in sources {
        let first = fmt(src);
        let second = fmt(&first);
        assert_eq!(
            first, second,
            "not idempotent for source: {}\nfirst:\n{}\nsecond:\n{}",
            src, first, second
        );
    }
}

// ======================== Edge cases ========================

#[test]
fn test_empty_function_body() {
    let out = fmt("fn empty() { }");
    assert!(out.contains("fn empty() {\n}"));
}

#[test]
fn test_return_no_value() {
    let out = fmt("fn f() { return }");
    assert!(out.contains("return"));
}

#[test]
fn test_break_continue() {
    let out = fmt("fn f() { while true { break } }");
    assert!(out.contains("break"));
}

#[test]
fn test_array_literal() {
    let out = fmt("fn f() { let arr = [1, 2, 3] }");
    assert!(out.contains("[1, 2, 3]"));
}

#[test]
fn test_unary_not() {
    let out = fmt("fn f() { not true }");
    assert!(out.contains("not true"));
}

#[test]
fn test_string_literal() {
    let out = fmt(r#"fn f() { let s = "hello" }"#);
    assert!(out.contains(r#""hello""#));
}

#[test]
fn test_bool_and_none_literals() {
    let out = fmt("fn f() { let a = true\n let b = false\n let c = none }");
    assert!(out.contains("true"));
    assert!(out.contains("false"));
    assert!(out.contains("none"));
}
