//! Integration tests for the Kryos parser.
#![allow(clippy::approx_constant)]

use kryos_ast::*;
use kryos_lexer::Lexer;
use kryos_parser::parse;

/// Helper: lex and parse source code, panicking on error.
fn parse_ok(src: &str) -> Module {
    let tokens = Lexer::new(src, 0).tokenize();
    parse(tokens).unwrap_or_else(|diags| {
        for d in &diags {
            eprintln!(
                "{}: {}",
                if d.is_error() { "ERROR" } else { "WARN" },
                d.message
            );
        }
        panic!("parse failed with {} error(s)", diags.len());
    })
}

/// Helper: lex and parse, expecting failure.
fn parse_err(src: &str) -> Vec<kryos_errors::Diagnostic> {
    let tokens = Lexer::new(src, 0).tokenize();
    parse(tokens).unwrap_err()
}

// ======================== Function declarations ========================

#[test]
fn test_simple_function() {
    let m = parse_ok("fn main() { }");
    assert_eq!(m.declarations.len(), 1);
    match &m.declarations[0] {
        Decl::Function {
            name,
            params,
            ret_ty,
            body,
            public,
            ..
        } => {
            assert_eq!(name, "main");
            assert!(params.is_empty());
            assert!(ret_ty.is_none());
            assert!(body.is_some());
            assert!(!public);
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_function_with_params_and_return() {
    let m = parse_ok("fn add(x: i32, y: i32) -> i32 { x }");
    match &m.declarations[0] {
        Decl::Function {
            name,
            params,
            ret_ty,
            ..
        } => {
            assert_eq!(name, "add");
            assert_eq!(params.len(), 2);
            assert_eq!(params[0].name, "x");
            assert_eq!(params[1].name, "y");
            assert!(ret_ty.is_some());
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_pub_function() {
    let m = parse_ok("pub fn greet() { }");
    match &m.declarations[0] {
        Decl::Function { public, .. } => assert!(*public),
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_function_with_generics() {
    let m = parse_ok("fn identity<T>(x: T) -> T { x }");
    match &m.declarations[0] {
        Decl::Function { generics, .. } => {
            assert_eq!(generics.len(), 1);
            assert_eq!(generics[0].name, "T");
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_function_with_annotation() {
    let m = parse_ok("@test\nfn my_test() { }");
    match &m.declarations[0] {
        Decl::Function {
            annotations, name, ..
        } => {
            assert_eq!(name, "my_test");
            assert_eq!(annotations.len(), 1);
            assert_eq!(annotations[0].name, "test");
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_annotation_with_args() {
    let m = parse_ok("@export(wasm)\nfn run() { }");
    match &m.declarations[0] {
        Decl::Function { annotations, .. } => {
            assert_eq!(annotations[0].name, "export");
            assert_eq!(annotations[0].args, vec!["wasm"]);
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_annotation_multiple_args() {
    let m = parse_ok("@capabilities(net, io)\nfn run() { }");
    match &m.declarations[0] {
        Decl::Function { annotations, .. } => {
            assert_eq!(annotations[0].name, "capabilities");
            assert_eq!(annotations[0].args, vec!["net", "io"]);
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_annotation_nested_not() {
    // @cfg(not(windows)) should parse as a single arg "not(windows)".
    let m = parse_ok("@cfg(not(windows))\nfn run() { }");
    match &m.declarations[0] {
        Decl::Function { annotations, .. } => {
            assert_eq!(annotations[0].name, "cfg");
            assert_eq!(annotations[0].args, vec!["not(windows)"]);
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_annotation_nested_all_any() {
    let m = parse_ok("@cfg(all(linux, not(release)))\nfn run() { }");
    match &m.declarations[0] {
        Decl::Function { annotations, .. } => {
            assert_eq!(
                annotations[0].args,
                vec!["all(linux,not(release))"]
            );
        }
        other => panic!("expected Function, got {:?}", other),
    }
    let m = parse_ok("@cfg(any(windows, all(linux, debug)))\nfn run() { }");
    match &m.declarations[0] {
        Decl::Function { annotations, .. } => {
            assert_eq!(
                annotations[0].args,
                vec!["any(windows,all(linux,debug))"]
            );
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_annotation_multiple_top_level_combinators() {
    let m = parse_ok("@cfg(linux, not(release))\nfn run() { }");
    match &m.declarations[0] {
        Decl::Function { annotations, .. } => {
            assert_eq!(
                annotations[0].args,
                vec!["linux", "not(release)"]
            );
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Struct declarations ========================

#[test]
fn test_struct_decl() {
    let m = parse_ok("struct Point { x: f64, y: f64 }");
    match &m.declarations[0] {
        Decl::Struct { name, fields, .. } => {
            assert_eq!(name, "Point");
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name, "x");
            assert_eq!(fields[1].name, "y");
        }
        other => panic!("expected Struct, got {:?}", other),
    }
}

#[test]
fn test_struct_with_generics() {
    let m = parse_ok("struct Pair<A, B> { first: A, second: B }");
    match &m.declarations[0] {
        Decl::Struct {
            name,
            generics,
            fields,
            ..
        } => {
            assert_eq!(name, "Pair");
            assert_eq!(generics.len(), 2);
            assert_eq!(fields.len(), 2);
        }
        other => panic!("expected Struct, got {:?}", other),
    }
}

// ======================== Enum declarations ========================

#[test]
fn test_enum_decl() {
    let m = parse_ok("enum Color { Red, Green, Blue }");
    match &m.declarations[0] {
        Decl::Enum { name, variants, .. } => {
            assert_eq!(name, "Color");
            assert_eq!(variants.len(), 3);
            assert_eq!(variants[0].name, "Red");
            assert_eq!(variants[1].name, "Green");
            assert_eq!(variants[2].name, "Blue");
        }
        other => panic!("expected Enum, got {:?}", other),
    }
}

#[test]
fn test_enum_with_fields() {
    let m = parse_ok("enum Shape { Circle(f64), Rect(f64, f64) }");
    match &m.declarations[0] {
        Decl::Enum { variants, .. } => {
            assert_eq!(variants[0].name, "Circle");
            assert_eq!(variants[0].fields.len(), 1);
            assert_eq!(variants[1].name, "Rect");
            assert_eq!(variants[1].fields.len(), 2);
        }
        other => panic!("expected Enum, got {:?}", other),
    }
}

// ======================== Trait declarations ========================

#[test]
fn test_trait_decl() {
    let m = parse_ok("trait Display { fn fmt(self) -> str; }");
    match &m.declarations[0] {
        Decl::Trait { name, methods, .. } => {
            assert_eq!(name, "Display");
            assert_eq!(methods.len(), 1);
            match &methods[0] {
                Decl::Function { name, body, .. } => {
                    assert_eq!(name, "fmt");
                    assert!(body.is_none()); // signature only
                }
                other => panic!("expected Function in trait, got {:?}", other),
            }
        }
        other => panic!("expected Trait, got {:?}", other),
    }
}

// ======================== Impl blocks ========================

#[test]
fn test_impl_block() {
    let m = parse_ok("impl Point { fn new(x: f64) -> Point { x } }");
    match &m.declarations[0] {
        Decl::Impl {
            target,
            trait_name,
            methods,
            ..
        } => {
            assert_eq!(target, "Point");
            assert!(trait_name.is_none());
            assert_eq!(methods.len(), 1);
        }
        other => panic!("expected Impl, got {:?}", other),
    }
}

#[test]
fn test_impl_trait_for_type() {
    let m = parse_ok("impl Display for Point { fn fmt(self) -> str { self } }");
    match &m.declarations[0] {
        Decl::Impl {
            target, trait_name, ..
        } => {
            assert_eq!(target, "Point");
            assert_eq!(trait_name.as_deref(), Some("Display"));
        }
        other => panic!("expected Impl, got {:?}", other),
    }
}

// ======================== Actor declarations ========================

#[test]
fn test_actor_decl() {
    let m = parse_ok("actor Counter { count: i32 fn increment(self) { self } }");
    match &m.declarations[0] {
        Decl::Actor {
            name,
            state_fields,
            handlers,
            ..
        } => {
            assert_eq!(name, "Counter");
            assert_eq!(state_fields.len(), 1);
            assert_eq!(state_fields[0].name, "count");
            assert_eq!(handlers.len(), 1);
            assert_eq!(handlers[0].name, "increment");
        }
        other => panic!("expected Actor, got {:?}", other),
    }
}

// ======================== Type alias ========================

#[test]
fn test_type_alias() {
    let m = parse_ok("type Id = i64");
    match &m.declarations[0] {
        Decl::TypeAlias { name, ty, .. } => {
            assert_eq!(name, "Id");
            match ty {
                TypeExpr::Simple { name, .. } => assert_eq!(name, "i64"),
                other => panic!("expected Simple type, got {:?}", other),
            }
        }
        other => panic!("expected TypeAlias, got {:?}", other),
    }
}

// ======================== Import declarations ========================

#[test]
fn test_simple_import() {
    let m = parse_ok("use std::io");
    match &m.declarations[0] {
        Decl::Import { path, .. } => {
            assert_eq!(path.segments, vec!["std", "io"]);
            assert!(path.items.is_empty());
        }
        other => panic!("expected Import, got {:?}", other),
    }
}

#[test]
fn test_grouped_import() {
    let m = parse_ok("use std::io::{File, BufReader}");
    match &m.declarations[0] {
        Decl::Import { path, .. } => {
            assert_eq!(path.segments, vec!["std", "io"]);
            assert_eq!(path.items, vec!["File", "BufReader"]);
        }
        other => panic!("expected Import, got {:?}", other),
    }
}

// ======================== Extern blocks ========================

#[test]
fn test_extern_block() {
    let m = parse_ok(r#"extern "C" { fn puts(s: *u8) -> i32; }"#);
    match &m.declarations[0] {
        Decl::Extern { abi, items, .. } => {
            assert_eq!(abi, "C");
            assert_eq!(items.len(), 1);
            match &items[0] {
                Decl::Function { name, .. } => assert_eq!(name, "puts"),
                other => panic!("expected Function in extern, got {:?}", other),
            }
        }
        other => panic!("expected Extern, got {:?}", other),
    }
}

// ======================== Let binding ========================

#[test]
fn test_let_binding() {
    let m = parse_ok("fn f() { let x: i32 = 42 }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => {
            assert_eq!(block.stmts.len(), 1);
            match &block.stmts[0] {
                Stmt::Let {
                    name,
                    mutable,
                    ty,
                    value,
                    ..
                } => {
                    assert_eq!(name, "x");
                    assert!(!mutable);
                    assert!(ty.is_some());
                    assert!(value.is_some());
                }
                other => panic!("expected Let, got {:?}", other),
            }
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_let_mut() {
    let m = parse_ok("fn f() { let mut count = 0 }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Let { name, mutable, .. } => {
                assert_eq!(name, "count");
                assert!(*mutable);
            }
            other => panic!("expected Let, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Assignment ========================

#[test]
fn test_assignment() {
    let m = parse_ok("fn f() { x = 10 }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Assign { op, .. } => {
                assert_eq!(*op, AssignOp::Assign);
            }
            other => panic!("expected Assign, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_compound_assignment() {
    let m = parse_ok("fn f() { x += 1 }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Assign { op, .. } => {
                assert_eq!(*op, AssignOp::AddAssign);
            }
            other => panic!("expected Assign, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== If/elif/else ========================

#[test]
fn test_if_elif_else() {
    let m = parse_ok("fn f() { if x { y } elif z { w } else { v } }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::If {
                elif_clauses,
                else_block,
                ..
            } => {
                assert_eq!(elif_clauses.len(), 1);
                assert!(else_block.is_some());
            }
            other => panic!("expected If, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== For loop ========================

#[test]
fn test_for_loop() {
    let m = parse_ok("fn f() { for i in items { i } }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::For { pattern, .. } => match pattern {
                Pattern::Ident { name, .. } => assert_eq!(name, "i"),
                other => panic!("expected Ident pattern, got {:?}", other),
            },
            other => panic!("expected For, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Parallel for loop ========================

#[test]
fn test_parallel_for_loop() {
    let m = parse_ok("fn f() { parallel for i in range(0, 100) { i } }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::For {
                parallel, pattern, ..
            } => {
                assert!(*parallel, "expected parallel: true");
                match pattern {
                    Pattern::Ident { name, .. } => assert_eq!(name, "i"),
                    other => panic!("expected Ident pattern, got {:?}", other),
                }
            }
            other => panic!("expected For, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_regular_for_is_not_parallel() {
    let m = parse_ok("fn f() { for x in items { x } }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::For { parallel, .. } => {
                assert!(!*parallel, "regular for should have parallel: false");
            }
            other => panic!("expected For, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== While loop ========================

#[test]
fn test_while_loop() {
    let m = parse_ok("fn f() { while running { x } }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::While { .. } => {}
            other => panic!("expected While, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Match expression ========================

#[test]
fn test_match_expr() {
    let m = parse_ok("fn f() { match color { Color::Red => 1, Color::Blue => 2 } }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::MatchExpr { arms, .. } => {
                    assert_eq!(arms.len(), 2);
                }
                other => panic!("expected MatchExpr, got {:?}", other),
            },
            other => panic!("expected Expr stmt, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_match_negative_int_pattern() {
    // Regression: `-1` as a match arm pattern was a parse error before the fix.
    // It must lower to a single IntLiteral pattern with value -1.
    let m = parse_ok(
        "fn f() { match n { -1 => 0, 0 => 1, _ => 2 } }",
    );
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::MatchExpr { arms, .. } => {
                    assert_eq!(arms.len(), 3, "expected 3 arms");
                    match &arms[0].pattern {
                        Pattern::Literal { expr, .. } => match expr.as_ref() {
                            Expr::IntLiteral { value: -1, .. } => {}
                            other => panic!(
                                "expected first arm pattern IntLiteral(-1), got {:?}",
                                other
                            ),
                        },
                        other => panic!(
                            "expected first arm Pattern::Literal, got {:?}",
                            other
                        ),
                    }
                }
                other => panic!("expected MatchExpr, got {:?}", other),
            },
            other => panic!("expected Expr stmt, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Expression precedence ========================

#[test]
fn test_binary_precedence_add_mul() {
    // `1 + 2 * 3` should parse as `1 + (2 * 3)`
    let m = parse_ok("fn f() { 1 + 2 * 3 }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::BinaryOp {
                    op: BinOp::Add,
                    right,
                    ..
                } => match right.as_ref() {
                    Expr::BinaryOp { op: BinOp::Mul, .. } => {}
                    other => panic!("expected Mul on right, got {:?}", other),
                },
                other => panic!("expected Add, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_unary_negation() {
    let m = parse_ok("fn f() { -x }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::UnaryOp { op: UnOp::Neg, .. } => {}
                other => panic!("expected UnaryOp Neg, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_function_call() {
    let m = parse_ok("fn f() { add(1, 2) }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::FnCall { args, .. } => {
                    assert_eq!(args.len(), 2);
                }
                other => panic!("expected FnCall, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_field_access() {
    let m = parse_ok("fn f() { point.x }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::FieldAccess { field, .. } => {
                    assert_eq!(field, "x");
                }
                other => panic!("expected FieldAccess, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_method_call() {
    let m = parse_ok("fn f() { list.push(42) }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::MethodCall { method, args, .. } => {
                    assert_eq!(method, "push");
                    assert_eq!(args.len(), 1);
                }
                other => panic!("expected MethodCall, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_index_access() {
    let m = parse_ok("fn f() { arr[0] }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::IndexAccess { .. } => {}
                other => panic!("expected IndexAccess, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Type annotation parsing ========================

#[test]
fn test_generic_type() {
    let m = parse_ok("fn f(v: Vec<i32>) { v }");
    match &m.declarations[0] {
        Decl::Function { params, .. } => match &params[0].ty {
            Some(TypeExpr::Generic { name, args, .. }) => {
                assert_eq!(name, "Vec");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Generic type, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_reference_type() {
    let m = parse_ok("fn f(x: &mut i32) { x }");
    match &m.declarations[0] {
        Decl::Function { params, .. } => match &params[0].ty {
            Some(TypeExpr::Reference { mutable, .. }) => {
                assert!(*mutable);
            }
            other => panic!("expected Reference type, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_array_type() {
    let m = parse_ok("fn f(a: [i32; 10]) { a }");
    match &m.declarations[0] {
        Decl::Function { params, .. } => match &params[0].ty {
            Some(TypeExpr::Array { size, .. }) => {
                assert_eq!(*size, Some(10));
            }
            other => panic!("expected Array type, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_function_type() {
    let m = parse_ok("fn f(cb: fn(i32) -> i32) { cb }");
    match &m.declarations[0] {
        Decl::Function { params, .. } => match &params[0].ty {
            Some(TypeExpr::Function {
                params: fn_params, ..
            }) => {
                assert_eq!(fn_params.len(), 1);
            }
            other => panic!("expected Function type, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Literals ========================

#[test]
fn test_array_literal() {
    let m = parse_ok("fn f() { [1, 2, 3] }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::ArrayLiteral { elements, .. } => {
                    assert_eq!(elements.len(), 3);
                }
                other => panic!("expected ArrayLiteral, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn test_hex_and_binary_literals() {
    let m = parse_ok("fn f() { let a = 0xFF\n let b = 0b1010 }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => {
            match &block.stmts[0] {
                Stmt::Let {
                    value: Some(Expr::IntLiteral { value, .. }),
                    ..
                } => {
                    assert_eq!(*value, 255);
                }
                other => panic!("expected Let with int, got {:?}", other),
            }
            match &block.stmts[1] {
                Stmt::Let {
                    value: Some(Expr::IntLiteral { value, .. }),
                    ..
                } => {
                    assert_eq!(*value, 10);
                }
                other => panic!("expected Let with int, got {:?}", other),
            }
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Lambda ========================

#[test]
fn test_lambda() {
    let m = parse_ok("fn f() { let add = fn(x: i32, y: i32) -> i32 { x + y } }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Let {
                value: Some(Expr::Lambda { params, ret_ty, .. }),
                ..
            } => {
                assert_eq!(params.len(), 2);
                assert!(ret_ty.is_some());
            }
            other => panic!("expected Let with Lambda, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Try/Catch and Throw ========================

#[test]
fn test_try_catch() {
    let m = parse_ok("fn f() { try { x } catch err { y } }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::TryCatch { catch_name, .. } => {
                assert_eq!(catch_name, "err");
            }
            other => panic!("expected TryCatch, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Logical operators ========================

#[test]
fn test_and_or_precedence() {
    // `a or b and c` should parse as `a or (b and c)` because `and` binds tighter
    let m = parse_ok("fn f() { a or b and c }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::BinaryOp {
                    op: BinOp::Or,
                    right,
                    ..
                } => match right.as_ref() {
                    Expr::BinaryOp { op: BinOp::And, .. } => {}
                    other => panic!("expected And on right, got {:?}", other),
                },
                other => panic!("expected Or, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Power (right-associative) ========================

#[test]
fn test_power_right_assoc() {
    // `2 ** 3 ** 4` should parse as `2 ** (3 ** 4)` (right-assoc)
    let m = parse_ok("fn f() { 2 ** 3 ** 4 }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::BinaryOp {
                    op: BinOp::Pow,
                    right,
                    ..
                } => match right.as_ref() {
                    Expr::BinaryOp { op: BinOp::Pow, .. } => {}
                    other => panic!("expected Pow on right, got {:?}", other),
                },
                other => panic!("expected Pow, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Cast expression ========================

#[test]
fn test_cast() {
    let m = parse_ok("fn f() { x as i64 }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::Cast { ty, .. } => match ty {
                    TypeExpr::Simple { name, .. } => assert_eq!(name, "i64"),
                    other => panic!("expected Simple type, got {:?}", other),
                },
                other => panic!("expected Cast, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Return statement ========================

#[test]
fn test_return_value() {
    let m = parse_ok("fn f() -> i32 { return 42 }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Return {
                value: Some(Expr::IntLiteral { value: 42, .. }),
                ..
            } => {}
            other => panic!("expected Return 42, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Error recovery ========================

#[test]
fn test_error_recovery() {
    // Missing function body should error
    let diags = parse_err("fn f(");
    assert!(!diags.is_empty());
}

// ======================== Bool, none, string, char literals ========================

#[test]
fn test_literals() {
    let m = parse_ok(
        r#"fn f() { let a = true
let b = false
let c = none
let d = "hello"
let e = 3.14 }"#,
    );
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => {
            assert_eq!(block.stmts.len(), 5);
            match &block.stmts[0] {
                Stmt::Let {
                    value: Some(Expr::BoolLiteral { value: true, .. }),
                    ..
                } => {}
                other => panic!("expected true, got {:?}", other),
            }
            match &block.stmts[1] {
                Stmt::Let {
                    value: Some(Expr::BoolLiteral { value: false, .. }),
                    ..
                } => {}
                other => panic!("expected false, got {:?}", other),
            }
            match &block.stmts[2] {
                Stmt::Let {
                    value: Some(Expr::NoneLiteral { .. }),
                    ..
                } => {}
                other => panic!("expected none, got {:?}", other),
            }
            match &block.stmts[3] {
                Stmt::Let {
                    value: Some(Expr::StringLiteral { value, .. }),
                    ..
                } => {
                    assert_eq!(value, "hello");
                }
                other => panic!("expected string, got {:?}", other),
            }
            match &block.stmts[4] {
                Stmt::Let {
                    value: Some(Expr::FloatLiteral { value, .. }),
                    ..
                } => {
                    assert!((value - 3.14).abs() < 0.001);
                }
                other => panic!("expected float, got {:?}", other),
            }
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Multiple declarations ========================

#[test]
fn test_multiple_declarations() {
    let m = parse_ok("struct Point { x: f64, y: f64 }\nfn main() { }");
    assert_eq!(m.declarations.len(), 2);
    assert!(matches!(&m.declarations[0], Decl::Struct { .. }));
    assert!(matches!(&m.declarations[1], Decl::Function { .. }));
}

// ======================== Shared/Move/Weak expressions ========================

#[test]
fn test_shared_expr() {
    let m = parse_ok("fn f() { shared x }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::SharedExpr { .. } => {}
                other => panic!("expected SharedExpr, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Struct literal ========================

#[test]
fn test_struct_literal() {
    let m = parse_ok("fn f() { Point { x: 1, y: 2 } }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::Expr { expr, .. } => match expr {
                Expr::StructLiteral { name, fields, .. } => {
                    assert_eq!(name, "Point");
                    assert_eq!(fields.len(), 2);
                }
                other => panic!("expected StructLiteral, got {:?}", other),
            },
            other => panic!("expected Expr, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Break / Continue ========================

#[test]
fn test_break_continue() {
    let m = parse_ok("fn f() { while true { break\n continue } }");
    match &m.declarations[0] {
        Decl::Function {
            body: Some(block), ..
        } => match &block.stmts[0] {
            Stmt::While { body, .. } => {
                assert_eq!(body.stmts.len(), 2);
                assert!(matches!(&body.stmts[0], Stmt::Break { .. }));
                assert!(matches!(&body.stmts[1], Stmt::Continue { .. }));
            }
            other => panic!("expected While, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Pointer type ========================

#[test]
fn test_pointer_type() {
    let m = parse_ok("fn f(p: *u8) { p }");
    match &m.declarations[0] {
        Decl::Function { params, .. } => match &params[0].ty {
            Some(TypeExpr::Pointer { mutable, .. }) => {
                assert!(!mutable);
            }
            other => panic!("expected Pointer type, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

// ======================== Generic bound ========================

#[test]
fn test_generic_with_bound() {
    let m = parse_ok("fn print<T: Display>(x: T) { x }");
    match &m.declarations[0] {
        Decl::Function { generics, .. } => {
            assert_eq!(generics.len(), 1);
            assert_eq!(generics[0].name, "T");
            assert_eq!(generics[0].bounds, vec!["Display"]);
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

// ==========================================================================
// Fuzz regressions — malformed top-level input must terminate
// ==========================================================================

#[test]
fn fuzz_regression_stray_rbrace_at_top_level_terminates() {
    // fuzz_parser OOM finding: "}:" — synchronize() stops AT `}` without
    // consuming it, so the module loop re-errored on the same token forever.
    for src in ["}:", "}", "}}}}", ":", "return", "break continue", "} fn f() {}"] {
        let tokens = Lexer::new(src, 0).tokenize();
        let _ = parse(tokens); // must terminate (errors are fine)
    }
}

#[test]
fn fuzz_regression_map_or_block_stray_rbracket_terminates() {
    // fuzz_parser OOM finding (CI: "out-of-memory (used: 2100Mb; limit:
    // 2048Mb)"). Minimized 7-byte reproducer: `let]\x0e{]`.
    //
    // Root cause: `let ]` fails name/`=` parsing and recovers up to the `{`
    // of `... = { ... }`. `parse_map_or_block_expr`'s "otherwise parse as a
    // block" loop then hits a bare `]`: `parse_statement` -> `parse_primary`
    // deliberately does NOT consume a stray `)`/`]`/`}`/`,` (it trusts an
    // enclosing call/array/struct-literal to consume it during recovery),
    // so `parse_statement()` returns `Some(stmt)` having advanced the
    // cursor by zero tokens. Unlike `parse_block_stmts`/`parse_module`
    // (which already guard the identical zero-progress case), this loop had
    // no such guard, so it re-parsed the SAME statement every iteration,
    // growing `stmts` (and the diagnostics list) without bound until the
    // process was killed by the OS/allocator — a real DoS vector for any
    // build service or editor plugin that parses untrusted source.
    let src = "let]\x0e{]";
    let tokens = Lexer::new(src, 0).tokenize();
    let result = parse(tokens);
    // Must terminate (this line alone would hang/OOM pre-fix) and produce a
    // clean, BOUNDED diagnostic list rather than one entry per infinite
    // iteration -- a handful of "unexpected token" errors, not thousands.
    let diags = result.unwrap_err();
    assert!(
        diags.len() < 20,
        "expected a small, bounded diagnostic count, got {} (runaway loop?)",
        diags.len()
    );
}

// ======================== Nesting depth guard (E0010) ========================

#[test]
fn test_nesting_guard_deep_parens() {
    // 10k nested parens must produce a single clean E0010, not a stack
    // overflow (ICE class).
    let src = format!("fn main() {{ let x = {}1{} }}", "(".repeat(10_000), ")".repeat(10_000));
    let diags = {
        let tokens = kryos_lexer::Lexer::new(&src, 0).tokenize();
        kryos_parser::parse(tokens).unwrap_err()
    };
    let e0010: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some("E0010"))
        .collect();
    assert_eq!(e0010.len(), 1, "expected exactly one E0010, got {diags:?}");
}

#[test]
fn test_nesting_guard_long_chain() {
    // A 200k-term `1+1+...` chain builds a deep LEFT spine without parser
    // recursion — it must also be rejected with E0010 (the checker would
    // otherwise recurse per node and overflow).
    let src = format!("fn main() {{ let x = {}1 }}", "1+".repeat(200_000));
    let diags = {
        let tokens = kryos_lexer::Lexer::new(&src, 0).tokenize();
        kryos_parser::parse(tokens).unwrap_err()
    };
    assert!(
        diags.iter().any(|d| d.code.as_deref() == Some("E0010")),
        "expected E0010 in {diags:?}"
    );
}

#[test]
fn test_nesting_guard_allows_reasonable_depth() {
    // 200 nested parens and a 500-term chain are legitimate generated code —
    // they must stay well inside the limit.
    let deep = format!("fn main() {{ let x = {}1{} }}", "(".repeat(200), ")".repeat(200));
    parse_ok(&deep);
    let chain = format!("fn main() {{ let x = {}1 }}", "1+".repeat(500));
    parse_ok(&chain);
}

// ======================== Newcomer-mistake diagnostics ========================

#[test]
fn test_hint_rust_macro_call() {
    let diags = parse_err("fn main() { println!(\"hi\") }");
    assert!(
        diags.iter().any(|d| d.message.contains("has no macros")),
        "expected macro hint in {diags:?}"
    );
}

#[test]
fn test_hint_arrow_closure() {
    let diags = parse_err("fn main() { let f = (x) => x + 1 }");
    assert!(
        diags
            .iter()
            .any(|d| d.notes.iter().any(|n| n.contains("closures are written"))),
        "expected closure-syntax note in {diags:?}"
    );
}

#[test]
fn test_hint_assign_in_condition() {
    let diags = parse_err("fn main() { let mut x = 1\n if x = 5 { println(\"y\") } }");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("assignment `=` is not allowed in a condition")),
        "expected assign-in-condition error in {diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|d| d.notes.iter().any(|n| n.contains("use `==` to compare"))),
        "expected == note in {diags:?}"
    );
}
