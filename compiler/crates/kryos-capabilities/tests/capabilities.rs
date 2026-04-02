//! Integration tests for the kryos-capabilities crate.

use kryos_ast::{Annotation, Block, Decl, Expr, MessageHandler, Module, Stmt};
use kryos_capabilities::model::{
    is_escalation_action, required_capability_for_path, Budget, Sandbox,
};
use kryos_capabilities::{check_capabilities, Capability, CapabilitySet};
use kryos_errors::Span;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn span() -> Span {
    Span::DUMMY
}

fn ann(name: &str, args: Vec<&str>) -> Annotation {
    Annotation {
        name: name.to_string(),
        args: args.into_iter().map(|s| s.to_string()).collect(),
        span: span(),
    }
}

fn stdlib_call(module: &str, func: &str) -> Expr {
    Expr::FnCall {
        callee: Box::new(Expr::FieldAccess {
            object: Box::new(Expr::FieldAccess {
                object: Box::new(Expr::Identifier {
                    name: "std".into(),
                    span: span(),
                }),
                field: module.into(),
                span: span(),
            }),
            field: func.into(),
            span: span(),
        }),
        args: vec![],
        span: span(),
    }
}

fn fn_decl(name: &str, annotations: Vec<Annotation>, body_stmts: Vec<Stmt>) -> Decl {
    Decl::Function {
        name: name.to_string(),
        generics: vec![],
        params: vec![],
        ret_ty: None,
        body: Some(Block {
            stmts: body_stmts,
            span: span(),
        }),
        public: false,
        annotations,
        span: span(),
    }
}

fn module_with(decls: Vec<Decl>) -> Module {
    Module {
        name: "test".into(),
        declarations: decls,
        span: span(),
    }
}

fn errors_only(diags: &[kryos_errors::Diagnostic]) -> Vec<&kryos_errors::Diagnostic> {
    diags.iter().filter(|d| d.is_error()).collect()
}

// ── CapabilitySet tests ──────────────────────────────────────────────────────

#[test]
fn parse_capabilities_from_annotations() {
    let anns = vec![ann("capabilities", vec!["net", "io"])];
    let set = CapabilitySet::from_annotations(&anns);
    assert!(set.has(Capability::Net));
    assert!(set.has(Capability::Io));
    assert!(!set.has(Capability::Ffi));
    assert!(!set.has(Capability::Crypto));
    assert_eq!(set.len(), 2);
}

#[test]
fn empty_capabilities_empty_set() {
    let set = CapabilitySet::from_annotations(&[]);
    assert!(set.is_empty());
    assert!(!set.has(Capability::Net));
    assert!(!set.has(Capability::Io));
    assert!(!set.has(Capability::All));
}

#[test]
fn multiple_capability_annotations_on_same_function() {
    // Two separate @capabilities annotations should merge.
    let anns = vec![
        ann("capabilities", vec!["net"]),
        ann("capabilities", vec!["io", "time"]),
    ];
    let set = CapabilitySet::from_annotations(&anns);
    assert!(set.has(Capability::Net));
    assert!(set.has(Capability::Io));
    assert!(set.has(Capability::Time));
    assert_eq!(set.len(), 3);
}

#[test]
fn all_capability_grants_everything() {
    let anns = vec![ann("capabilities", vec!["all"])];
    let set = CapabilitySet::from_annotations(&anns);
    assert!(set.has(Capability::Net));
    assert!(set.has(Capability::Io));
    assert!(set.has(Capability::Ffi));
    assert!(set.has(Capability::Compute));
    assert!(set.has(Capability::Crypto));
    assert!(set.has(Capability::Process));
    assert!(set.has(Capability::Env));
    assert!(set.has(Capability::Term));
    assert!(set.has(Capability::Db));
    assert!(set.has(Capability::Time));
}

// ── Checker: stdlib usage ────────────────────────────────────────────────────

#[test]
fn function_using_net_without_capability_produces_error() {
    let module = module_with(vec![fn_decl(
        "connect_somewhere",
        vec![],
        vec![Stmt::Expr {
            expr: stdlib_call("net", "connect"),
            span: span(),
        }],
    )]);

    let diags = check_capabilities(&module);
    let errs = errors_only(&diags);
    assert_eq!(errs.len(), 1);
    assert!(errs[0].message.contains("requires `net` capability"));
    assert_eq!(errs[0].code.as_deref(), Some("E-CAP-MISSING"));
}

#[test]
fn function_using_net_with_capability_no_error() {
    let module = module_with(vec![fn_decl(
        "connect_somewhere",
        vec![ann("capabilities", vec!["net"])],
        vec![Stmt::Expr {
            expr: stdlib_call("net", "connect"),
            span: span(),
        }],
    )]);

    let diags = check_capabilities(&module);
    let errs = errors_only(&diags);
    assert!(errs.is_empty(), "expected no errors, got: {errs:?}");
}

#[test]
fn function_using_io_without_capability_produces_error() {
    let module = module_with(vec![fn_decl(
        "read_file",
        vec![],
        vec![Stmt::Expr {
            expr: stdlib_call("io", "read"),
            span: span(),
        }],
    )]);

    let diags = check_capabilities(&module);
    let errs = errors_only(&diags);
    assert_eq!(errs.len(), 1);
    assert!(errs[0].message.contains("requires `io` capability"));
}

#[test]
fn function_with_all_can_use_any_stdlib() {
    let module = module_with(vec![fn_decl(
        "god_mode",
        vec![ann("capabilities", vec!["all"])],
        vec![
            Stmt::Expr {
                expr: stdlib_call("net", "listen"),
                span: span(),
            },
            Stmt::Expr {
                expr: stdlib_call("io", "write"),
                span: span(),
            },
            Stmt::Expr {
                expr: stdlib_call("crypto", "encrypt"),
                span: span(),
            },
            Stmt::Expr {
                expr: stdlib_call("db", "query"),
                span: span(),
            },
        ],
    )]);

    let diags = check_capabilities(&module);
    let errs = errors_only(&diags);
    assert!(errs.is_empty(), "all should grant everything: {errs:?}");
}

// ── Checker: attenuation ─────────────────────────────────────────────────────

#[test]
fn attenuation_child_subset_of_parent_no_error() {
    // Actor with net+io, handler uses only net.
    let module = module_with(vec![Decl::Actor {
        name: "NetActor".into(),
        state_fields: vec![],
        handlers: vec![MessageHandler {
            name: "handle".into(),
            params: vec![],
            ret_ty: None,
            body: Block {
                stmts: vec![Stmt::Expr {
                    expr: stdlib_call("net", "connect"),
                    span: span(),
                }],
                span: span(),
            },
            span: span(),
        }],
        annotations: vec![ann("capabilities", vec!["net", "io"])],
        span: span(),
    }]);

    let diags = check_capabilities(&module);
    let errs = errors_only(&diags);
    assert!(errs.is_empty(), "subset should be valid: {errs:?}");
}

#[test]
fn spawn_with_capability_subset_is_valid() {
    // A function with net+io spawns an expression — the spawn itself is fine
    // as long as the spawned code doesn't exceed the enclosing scope.
    let module = module_with(vec![fn_decl(
        "spawner",
        vec![ann("capabilities", vec!["net", "io"])],
        vec![Stmt::Spawn {
            expr: Expr::FnCall {
                callee: Box::new(Expr::Identifier {
                    name: "worker".into(),
                    span: span(),
                }),
                args: vec![],
                span: span(),
            },
            span: span(),
        }],
    )]);

    let diags = check_capabilities(&module);
    let errs = errors_only(&diags);
    assert!(errs.is_empty(), "spawn within capability scope is fine: {errs:?}");
}

#[test]
fn spawn_stdlib_call_exceeding_parent_capabilities_is_error() {
    // Function has only `net`, but spawn body calls std.io.
    let module = module_with(vec![fn_decl(
        "spawner",
        vec![ann("capabilities", vec!["net"])],
        vec![Stmt::Spawn {
            expr: stdlib_call("io", "write"),
            span: span(),
        }],
    )]);

    let diags = check_capabilities(&module);
    let errs = errors_only(&diags);
    assert_eq!(errs.len(), 1, "spawn exceeding parent should error: {errs:?}");
    assert!(errs[0].message.contains("requires `io` capability"));
}

// ── Checker: self-heal escalation ────────────────────────────────────────────

#[test]
fn self_heal_escalation_method_call_detected() {
    let escalation = Expr::MethodCall {
        object: Box::new(Expr::Identifier {
            name: "self".into(),
            span: span(),
        }),
        method: "add_capability".into(),
        args: vec![],
        span: span(),
    };

    let module = module_with(vec![fn_decl(
        "try_escalate",
        vec![ann("capabilities", vec!["net"])],
        vec![Stmt::Expr {
            expr: escalation,
            span: span(),
        }],
    )]);

    let diags = check_capabilities(&module);
    assert!(diags.iter().any(|d| d.code.as_deref() == Some("E-CAP-ESCALATION")));
}

#[test]
fn self_heal_escalation_fn_call_detected() {
    let escalation = Expr::FnCall {
        callee: Box::new(Expr::Identifier {
            name: "escalate_tier".into(),
            span: span(),
        }),
        args: vec![],
        span: span(),
    };

    let module = module_with(vec![fn_decl(
        "try_escalate",
        vec![ann("capabilities", vec!["net"])],
        vec![Stmt::Expr {
            expr: escalation,
            span: span(),
        }],
    )]);

    let diags = check_capabilities(&module);
    assert!(
        diags.iter().any(|d| d.code.as_deref() == Some("E-CAP-ESCALATION")),
        "expected escalation error, got: {diags:?}"
    );
}

#[test]
fn all_prohibited_escalation_actions_detected() {
    let prohibited = vec![
        "add_capability",
        "widen_sandbox",
        "increase_budget",
        "remove_spawn_limit",
        "modify_annotations",
        "escalate_tier",
    ];

    for action in prohibited {
        assert!(
            is_escalation_action(action),
            "expected `{action}` to be detected as escalation"
        );
    }
}

#[test]
fn normal_method_names_not_flagged_as_escalation() {
    assert!(!is_escalation_action("handle_error"));
    assert!(!is_escalation_action("process_data"));
    assert!(!is_escalation_action("connect"));
    assert!(!is_escalation_action("read_file"));
}

// ── Budget annotation ────────────────────────────────────────────────────────

#[test]
fn budget_annotation_parsed_correctly() {
    let anns = vec![ann("budget", vec!["1000"])];
    let budget = Budget::from_annotations(&anns);
    assert_eq!(budget, Some(Budget { limit: 1000 }));
}

#[test]
fn budget_annotation_missing_returns_none() {
    let anns = vec![ann("capabilities", vec!["net"])];
    let budget = Budget::from_annotations(&anns);
    assert_eq!(budget, None);
}

#[test]
fn budget_annotation_invalid_number_returns_none() {
    let anns = vec![ann("budget", vec!["not_a_number"])];
    let budget = Budget::from_annotations(&anns);
    assert_eq!(budget, None);
}

// ── Sandbox annotation ───────────────────────────────────────────────────────

#[test]
fn sandbox_annotation_detected() {
    let anns = vec![ann("sandbox", vec![])];
    let sandbox = Sandbox::from_annotations(&anns);
    assert!(sandbox.enabled);
}

#[test]
fn sandbox_annotation_absent() {
    let anns = vec![ann("capabilities", vec!["net"])];
    let sandbox = Sandbox::from_annotations(&anns);
    assert!(!sandbox.enabled);
}

// ── CapabilitySet subset/union ───────────────────────────────────────────────

#[test]
fn subset_same_set() {
    let set = CapabilitySet::from_annotations(&[ann("capabilities", vec!["net", "io"])]);
    assert!(set.is_subset_of(&set));
}

#[test]
fn subset_strict_subset() {
    let parent = CapabilitySet::from_annotations(&[ann("capabilities", vec!["net", "io", "time"])]);
    let child = CapabilitySet::from_annotations(&[ann("capabilities", vec!["net"])]);
    assert!(child.is_subset_of(&parent));
    assert!(!parent.is_subset_of(&child));
}

#[test]
fn subset_all_is_superset() {
    let all = CapabilitySet::from_annotations(&[ann("capabilities", vec!["all"])]);
    let some = CapabilitySet::from_annotations(&[ann("capabilities", vec!["net", "io"])]);
    assert!(some.is_subset_of(&all));
    assert!(!all.is_subset_of(&some));
}

#[test]
fn union_combines_sets() {
    let a = CapabilitySet::from_annotations(&[ann("capabilities", vec!["net"])]);
    let b = CapabilitySet::from_annotations(&[ann("capabilities", vec!["io"])]);
    let combined = a.union(&b);
    assert!(combined.has(Capability::Net));
    assert!(combined.has(Capability::Io));
    assert_eq!(combined.len(), 2);
}

// ── Path-to-capability mapping ───────────────────────────────────────────────

#[test]
fn stdlib_path_mapping() {
    let mappings: Vec<(&str, Capability)> = vec![
        ("net", Capability::Net),
        ("io", Capability::Io),
        ("ffi", Capability::Ffi),
        ("compute", Capability::Compute),
        ("crypto", Capability::Crypto),
        ("process", Capability::Process),
        ("env", Capability::Env),
        ("term", Capability::Term),
        ("db", Capability::Db),
        ("time", Capability::Time),
    ];

    for (module, expected_cap) in mappings {
        let path = vec!["std".to_string(), module.to_string(), "something".to_string()];
        assert_eq!(
            required_capability_for_path(&path),
            Some(expected_cap),
            "std::{module} should require {expected_cap}"
        );
    }
}

#[test]
fn non_std_path_returns_none() {
    let path = vec!["mylib".to_string(), "net".to_string()];
    assert_eq!(required_capability_for_path(&path), None);
}

// ── Unknown capability warning ───────────────────────────────────────────────

#[test]
fn unknown_capability_name_produces_warning() {
    let module = module_with(vec![fn_decl(
        "bad_caps",
        vec![ann("capabilities", vec!["net", "banana"])],
        vec![],
    )]);

    let diags = check_capabilities(&module);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("banana"));
    assert_eq!(diags[0].code.as_deref(), Some("W-CAP-UNKNOWN"));
}

// ── Extern blocks ────────────────────────────────────────────────────────────

#[test]
fn extern_block_inside_ffi_scope_is_ok() {
    // A function with ffi capability containing an extern block (via impl or nested decl)
    // is tested indirectly — extern at module level doesn't require a scope.
    // The capability check for extern fires when there IS a scope stack.
    // This test verifies the model logic.
    let set = CapabilitySet::from_annotations(&[ann("capabilities", vec!["ffi"])]);
    assert!(set.has(Capability::Ffi));
}

// ── Complex scenarios ────────────────────────────────────────────────────────

#[test]
fn multiple_violations_reported() {
    // Function with no capabilities calls multiple gated stdlib modules.
    let module = module_with(vec![fn_decl(
        "violator",
        vec![],
        vec![
            Stmt::Expr {
                expr: stdlib_call("net", "connect"),
                span: span(),
            },
            Stmt::Expr {
                expr: stdlib_call("io", "read"),
                span: span(),
            },
            Stmt::Expr {
                expr: stdlib_call("crypto", "sign"),
                span: span(),
            },
        ],
    )]);

    let diags = check_capabilities(&module);
    let errs = errors_only(&diags);
    assert_eq!(errs.len(), 3, "should report 3 violations: {errs:?}");
}

#[test]
fn mixed_valid_and_invalid_calls() {
    // Function has `net` but not `io` — net calls should pass, io should fail.
    let module = module_with(vec![fn_decl(
        "mixed",
        vec![ann("capabilities", vec!["net"])],
        vec![
            Stmt::Expr {
                expr: stdlib_call("net", "connect"),
                span: span(),
            },
            Stmt::Expr {
                expr: stdlib_call("io", "write"),
                span: span(),
            },
        ],
    )]);

    let diags = check_capabilities(&module);
    let errs = errors_only(&diags);
    assert_eq!(errs.len(), 1);
    assert!(errs[0].message.contains("io"));
}

#[test]
fn escalation_combined_with_capability_violation() {
    // Function with no capabilities tries stdlib and escalation.
    let module = module_with(vec![fn_decl(
        "double_trouble",
        vec![],
        vec![
            Stmt::Expr {
                expr: stdlib_call("net", "connect"),
                span: span(),
            },
            Stmt::Expr {
                expr: Expr::MethodCall {
                    object: Box::new(Expr::Identifier {
                        name: "self".into(),
                        span: span(),
                    }),
                    method: "widen_sandbox".into(),
                    args: vec![],
                    span: span(),
                },
                span: span(),
            },
        ],
    )]);

    let diags = check_capabilities(&module);
    let errs = errors_only(&diags);
    assert!(errs.len() >= 2, "should have both violations: {errs:?}");
    assert!(errs.iter().any(|d| d.code.as_deref() == Some("E-CAP-MISSING")));
    assert!(errs.iter().any(|d| d.code.as_deref() == Some("E-CAP-ESCALATION")));
}
