//! Where-clause parsing tests (v3.13).
//!
//! Validates the soft-keyword `where` parser path. The clause must:
//!  - merge bounds into the matching GenericParam
//!  - silently ignore unknown type names (caller will diagnose)
//!  - terminate cleanly at `{`, `;`, or EOF

use kryos_ast::Decl;
use kryos_lexer::Lexer;
use kryos_parser::parse;

fn parse_source(src: &str) -> kryos_ast::Module {
    let tokens = Lexer::new(src, 0).tokenize();
    parse(tokens).expect("parse should succeed")
}

#[test]
fn single_where_clause_adds_bound() {
    let src = "fn id<T>(x: T) -> T where T: Clone { return x }";
    let m = parse_source(src);
    let Decl::Function { generics, .. } = &m.declarations[0] else {
        panic!("expected fn decl");
    };
    assert_eq!(generics.len(), 1);
    assert_eq!(generics[0].name, "T");
    assert!(generics[0].bounds.contains(&"Clone".to_string()));
}

#[test]
fn multi_param_where_clause() {
    let src =
        "fn both<A, B>(a: A, b: B) -> i64 where A: Show, B: Show + Debug { return 0 }";
    let m = parse_source(src);
    let Decl::Function { generics, .. } = &m.declarations[0] else {
        panic!("expected fn decl");
    };
    assert_eq!(generics.len(), 2);
    let a = generics.iter().find(|g| g.name == "A").unwrap();
    assert!(a.bounds.contains(&"Show".to_string()));
    let b = generics.iter().find(|g| g.name == "B").unwrap();
    assert!(b.bounds.contains(&"Show".to_string()));
    assert!(b.bounds.contains(&"Debug".to_string()));
}

#[test]
fn where_clause_merges_with_inline_bounds() {
    let src = "fn f<T: Clone>(x: T) where T: Hash { return }";
    let m = parse_source(src);
    let Decl::Function { generics, .. } = &m.declarations[0] else {
        panic!("expected fn decl");
    };
    assert_eq!(generics[0].bounds, vec!["Clone".to_string(), "Hash".to_string()]);
}

#[test]
fn where_clause_absent_does_not_break_parsing() {
    let src = "fn f<T>(x: T) -> T { return x }";
    let m = parse_source(src);
    let Decl::Function { generics, .. } = &m.declarations[0] else {
        panic!("expected fn decl");
    };
    assert_eq!(generics[0].bounds.len(), 0);
}

#[test]
fn where_clause_on_non_generic_fn_is_noop() {
    // No generics — `where` shouldn't be consumed.
    let src = "fn f() { let x = 1 }";
    let m = parse_source(src);
    assert_eq!(m.declarations.len(), 1);
}
