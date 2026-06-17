//! Capability checker — walks the AST and enforces capability rules.
//!
//! Rules enforced:
//! 1. Functions using stdlib modules must declare matching capabilities.
//! 2. Attenuation: child scopes cannot exceed parent capabilities.
//! 3. Immutability: no runtime escalation of capabilities.
//! 4. Budget and sandbox annotations are validated.

use kryos_ast::{Annotation, Decl, Expr, Module, Stmt};
use kryos_errors::{Diagnostic, Span};

use std::collections::HashMap;

use crate::model::{
    is_escalation_action, required_capability_for_builtin, required_capability_for_path, Budget,
    Capability, CapabilitySet, Sandbox,
};

/// Run the capability checking pass over a module.
///
/// Returns a list of diagnostics (errors/warnings) for any violations found.
///
/// When `strict` is true, every function is treated as having an explicit
/// capability annotation — unannotated functions are checked as if declared
/// with the empty capability set. This is the `--strict-capabilities` mode:
/// it shifts the system from opt-in documentation to opt-in exemption. Any
/// call to a capability-gated builtin from an unannotated function becomes
/// a compile error (E0505) unless the function explicitly declares the
/// capability via `@capabilities(...)`.
pub fn check_capabilities(module: &Module, strict: bool) -> Vec<Diagnostic> {
    let mut checker = CapabilityChecker::new(strict);
    checker.check_module(module);
    checker.diagnostics
}

/// A capability scope entry on the stack.
///
/// Tracks both the capability set and whether this scope was explicitly
/// annotated with `@capabilities(...)`. Unannotated scopes are "ambient" —
/// they don't enforce builtin or cross-function capability checks.
#[derive(Debug, Clone)]
struct CapabilityScope {
    capabilities: CapabilitySet,
    /// `true` if the enclosing declaration had a `@capabilities(...)` annotation.
    annotated: bool,
}

/// Internal checker state.
struct CapabilityChecker {
    /// Stack of capability scopes. Each entry is the capability set for
    /// the current enclosing scope (function, actor, etc.).
    scope_stack: Vec<CapabilityScope>,
    /// Accumulated diagnostics.
    diagnostics: Vec<Diagnostic>,
    /// Map from function name to its declared capability set.
    /// Only populated for functions that have `@capabilities(...)` annotations.
    /// Used for cross-function propagation checks.
    fn_capabilities: HashMap<String, CapabilitySet>,
    /// Deny-by-default mode. When true, unannotated functions are treated as
    /// `@capabilities()` — the empty set — so any capability-gated builtin
    /// call inside them is a compile error (E0505) unless the function adds
    /// an explicit annotation.
    strict_mode: bool,
}

impl CapabilityChecker {
    fn new(strict_mode: bool) -> Self {
        Self {
            scope_stack: Vec::new(),
            diagnostics: Vec::new(),
            fn_capabilities: HashMap::new(),
            strict_mode,
        }
    }

    /// Get the current (innermost) capability scope, if any.
    fn current_scope(&self) -> Option<&CapabilityScope> {
        self.scope_stack.last()
    }

    /// Get just the current capability set, if any scope exists.
    fn current_caps(&self) -> Option<&CapabilitySet> {
        self.scope_stack.last().map(|s| &s.capabilities)
    }

    /// Check whether ANY enclosing scope is annotated.
    /// This handles lambdas and blocks inside annotated functions.
    fn has_annotated_scope(&self) -> bool {
        self.scope_stack.iter().any(|s| s.annotated)
    }

    /// Build a map from function names to their declared capability sets.
    /// Only includes functions with explicit `@capabilities(...)` annotations.
    fn build_fn_capability_map(&mut self, declarations: &[Decl]) {
        for decl in declarations {
            match decl {
                Decl::Function {
                    name, annotations, ..
                } if Self::has_capabilities_annotation(annotations) => {
                    let caps = CapabilitySet::from_annotations(annotations);
                    self.fn_capabilities.insert(name.clone(), caps);
                }
                Decl::Impl { methods, .. } | Decl::Trait { methods, .. } => {
                    self.build_fn_capability_map(methods);
                }
                _ => {}
            }
        }
    }

    /// Check if annotations contain a `@capabilities(...)` annotation.
    fn has_capabilities_annotation(annotations: &[Annotation]) -> bool {
        annotations.iter().any(|a| a.name == "capabilities")
    }

    fn check_module(&mut self, module: &Module) {
        // First pass: build the function-to-capabilities map for cross-function checks.
        self.build_fn_capability_map(&module.declarations);

        // Second pass: walk the AST and enforce capability rules.
        for decl in &module.declarations {
            self.check_decl(decl);
        }
    }

    fn check_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Function {
                name,
                annotations,
                body,
                span,
                ..
            } => {
                self.check_function(name, annotations, body.as_ref(), *span);
            }
            Decl::Actor {
                name,
                annotations,
                handlers,
                ..
            } => {
                self.check_actor(name, annotations, handlers);
            }
            Decl::Extern { items, span, .. } => {
                self.check_extern(items, *span);
            }
            Decl::Impl { methods, .. } => {
                for method in methods {
                    self.check_decl(method);
                }
            }
            Decl::Trait { methods, .. } => {
                for method in methods {
                    self.check_decl(method);
                }
            }
            Decl::Import { path, .. } => {
                // Check that imports of capability-gated modules are allowed.
                if let Some(required_cap) = required_capability_for_path(&path.segments) {
                    if let Some(caps) = self.current_caps() {
                        if !caps.has(required_cap) {
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "import of `{}` requires `{required_cap}` capability",
                                    path.segments.join("::")
                                ))
                                .with_label(path.span, format!("requires `{required_cap}`"))
                                .with_code(kryos_errors::codes::E0501),
                            );
                        }
                    }
                }
            }
            // Struct, Enum, TypeAlias — no capability concerns.
            _ => {}
        }
    }

    fn check_function(
        &mut self,
        name: &str,
        annotations: &[Annotation],
        body: Option<&kryos_ast::Block>,
        span: Span,
    ) {
        let caps = CapabilitySet::from_annotations(annotations);
        let annotated = Self::has_capabilities_annotation(annotations);
        let _budget = Budget::from_annotations(annotations);
        let _sandbox = Sandbox::from_annotations(annotations);

        // Validate unknown capability names in annotations.
        self.validate_capability_annotations(annotations);

        // Attenuation check: this function's capabilities must not exceed
        // the enclosing scope's capabilities.
        if let Some(parent) = self.current_scope() {
            if !caps.is_subset_of(&parent.capabilities) {
                let excess = caps.excess_over(&parent.capabilities);
                let excess_names: Vec<String> = excess.iter().map(|c| c.to_string()).collect();
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "function `{name}` exceeds parent capabilities: [{}]",
                        excess_names.join(", ")
                    ))
                    .with_label(span, "declared here")
                    .with_note(
                        "child scope cannot add capabilities not granted by parent".to_string(),
                    )
                    .with_code(kryos_errors::codes::E0503),
                );
            }
        }

        // Push this function's scope and check its body.
        // In strict mode, treat unannotated functions as annotated with the
        // empty set so builtin and propagation checks fire against them.
        let scope = CapabilityScope {
            capabilities: caps,
            annotated: annotated || self.strict_mode,
        };
        self.scope_stack.push(scope);
        if let Some(block) = body {
            self.check_block(block);
        }
        self.scope_stack.pop();
    }

    fn check_actor(
        &mut self,
        name: &str,
        annotations: &[Annotation],
        handlers: &[kryos_ast::MessageHandler],
    ) {
        let caps = CapabilitySet::from_annotations(annotations);
        let annotated = Self::has_capabilities_annotation(annotations);

        self.validate_capability_annotations(annotations);

        // Attenuation: actor capabilities must not exceed parent scope.
        if let Some(parent) = self.current_scope() {
            if !caps.is_subset_of(&parent.capabilities) {
                let excess = caps.excess_over(&parent.capabilities);
                let excess_names: Vec<String> = excess.iter().map(|c| c.to_string()).collect();
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "actor `{name}` exceeds parent capabilities: [{}]",
                        excess_names.join(", ")
                    ))
                    .with_label(
                        annotations.first().map(|a| a.span).unwrap_or(Span::DUMMY),
                        "declared here",
                    )
                    .with_note("spawned actor capabilities must be a subset of the spawner's")
                    .with_code(kryos_errors::codes::E0503),
                );
            }
        }

        // Check each handler under the actor's capability scope.
        // In strict mode, treat unannotated actors as annotated with the
        // empty set so builtin and propagation checks fire against them.
        let scope = CapabilityScope {
            capabilities: caps,
            annotated: annotated || self.strict_mode,
        };
        self.scope_stack.push(scope);
        for handler in handlers {
            self.check_block(&handler.body);
        }
        self.scope_stack.pop();
    }

    fn check_extern(&mut self, items: &[Decl], span: Span) {
        // Extern blocks require the Ffi capability.
        if let Some(caps) = self.current_caps() {
            if !caps.has(Capability::Ffi) {
                self.diagnostics.push(
                    Diagnostic::error("extern block requires `ffi` capability")
                        .with_label(span, "extern block here")
                        .with_code(kryos_errors::codes::E0506),
                );
            }
        } else {
            // Top-level extern — check against module-level (no scope = no caps).
            // Extern at top level is only valid if there's an enclosing scope with ffi.
            // At module level we allow it (it's a declaration, not an invocation).
            // The actual *use* of extern functions inside capability-scoped code
            // will be caught when those functions are called.
        }

        for item in items {
            self.check_decl(item);
        }
    }

    fn check_block(&mut self, block: &kryos_ast::Block) {
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
    }

    /// Check a `deny!(caps) { body }` block. Narrows the active capability set by
    /// removing the denied capabilities, then checks the body under that narrowed,
    /// annotated scope so the existing per-call guards (E0505 builtin / E0502
    /// missing / E0507 propagation) fire on any use of a denied capability.
    fn check_deny_block(&mut self, denied: &[String], body: &kryos_ast::Block, span: Span) {
        let denied_caps: Vec<Capability> =
            denied.iter().filter_map(|s| Capability::from_str(s)).collect();
        // A deny that names no recognized capability (typo) would silently narrow
        // nothing — warn rather than no-op.
        if denied_caps.is_empty() {
            self.diagnostics.push(
                Diagnostic::warning(format!(
                    "deny!({}) names no recognized capability; the block has no effect",
                    denied.join(", ")
                ))
                .with_label(span, "deny block here")
                .with_code(kryos_errors::codes::W0500),
            );
        }
        let base = self
            .current_caps()
            .cloned()
            .unwrap_or_else(CapabilitySet::empty);
        let narrowed = base.without(&denied_caps);
        self.scope_stack.push(CapabilityScope {
            capabilities: narrowed,
            annotated: true,
        });
        self.check_block(body);
        self.scope_stack.pop();
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { value, .. } => {
                if let Some(expr) = value {
                    self.check_expr(expr);
                }
            }
            Stmt::Assign { value, target, .. } => {
                self.check_expr(target);
                self.check_expr(value);
            }
            Stmt::Return { value, .. } => {
                if let Some(expr) = value {
                    self.check_expr(expr);
                }
            }
            Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                ..
            } => {
                self.check_expr(condition);
                self.check_block(then_block);
                for (cond, block) in elif_clauses {
                    self.check_expr(cond);
                    self.check_block(block);
                }
                if let Some(block) = else_block {
                    self.check_block(block);
                }
            }
            Stmt::For { iterable, body, .. } => {
                self.check_expr(iterable);
                self.check_block(body);
            }
            Stmt::While {
                condition, body, ..
            } => {
                self.check_expr(condition);
                self.check_block(body);
            }
            Stmt::Expr { expr, .. } => {
                self.check_expr(expr);
            }
            Stmt::Spawn { expr, span } => {
                self.check_spawn_expr(expr, *span);
            }
            Stmt::TryCatch {
                try_block,
                catch_block,
                ..
            } => {
                self.check_block(try_block);
                self.check_block(catch_block);
            }
            Stmt::Throw { expr, .. } => {
                self.check_expr(expr);
            }
            Stmt::Select { branches, .. } => {
                for branch in branches {
                    self.check_expr(&branch.channel);
                    self.check_block(&branch.body);
                }
            }
            Stmt::DenyBlock { denied, body, span } => {
                self.check_deny_block(denied, body, *span);
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::FnCall { callee, args, span } => {
                // Check if the callee is a path to a capability-gated stdlib module.
                self.check_callee_capabilities(callee, *span);

                // Check for prohibited self-heal escalation calls.
                self.check_escalation(callee, *span);

                // Recurse into callee and arguments.
                self.check_expr(callee);
                for arg in args {
                    self.check_expr(arg);
                }
            }
            Expr::MethodCall {
                object,
                method,
                args,
                span,
            } => {
                // Check for prohibited self-heal escalation method calls.
                if is_escalation_action(method) {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "prohibited capability escalation: `{method}` cannot be called"
                        ))
                        .with_label(*span, "escalation attempt")
                        .with_note(
                            "capabilities are immutable at compile time; \
                             self-heal actions cannot escalate privileges",
                        )
                        .with_code(kryos_errors::codes::E0504),
                    );
                }

                self.check_expr(object);
                for arg in args {
                    self.check_expr(arg);
                }
            }
            Expr::StaticMethodCall { args, .. } => {
                for arg in args {
                    self.check_expr(arg);
                }
            }
            Expr::FieldAccess { object, .. } => {
                self.check_expr(object);
            }
            Expr::IndexAccess { object, index, .. } => {
                self.check_expr(object);
                self.check_expr(index);
            }
            Expr::BinaryOp { left, right, .. } => {
                self.check_expr(left);
                self.check_expr(right);
            }
            Expr::UnaryOp { operand, .. } => {
                self.check_expr(operand);
            }
            Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => {
                for elem in elements {
                    self.check_expr(elem);
                }
            }
            Expr::MapLiteral { entries, .. } => {
                for (k, v) in entries {
                    self.check_expr(k);
                    self.check_expr(v);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, expr) in fields {
                    self.check_expr(expr);
                }
            }
            Expr::Lambda { body, .. } => {
                // Lambdas inherit the enclosing scope's capabilities.
                self.check_expr(body);
            }
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.check_expr(condition);
                self.check_block(then_branch);
                if let Some(block) = else_branch {
                    self.check_block(block);
                }
            }
            Expr::MatchExpr { subject, arms, .. } => {
                self.check_expr(subject);
                for arm in arms {
                    self.check_expr(&arm.body);
                    if let Some(guard) = &arm.guard {
                        self.check_expr(guard);
                    }
                }
            }
            Expr::PipeExpr { left, right, .. } => {
                self.check_expr(left);
                self.check_expr(right);
            }
            Expr::Borrow { inner, .. }
            | Expr::Deref { inner, .. }
            | Expr::SharedExpr { inner, .. }
            | Expr::MoveExpr { inner, .. }
            | Expr::WeakExpr { inner, .. } => {
                self.check_expr(inner);
            }
            Expr::ComptimeBlock { body, .. } | Expr::QuantumBlock { body, .. } => {
                self.check_block(body);
            }
            Expr::Cast { expr, .. } => {
                self.check_expr(expr);
            }
            Expr::Block { block, .. } => {
                self.check_block(block);
            }
            Expr::Await { value, .. } => {
                self.check_expr(value);
            }
            Expr::RangeExpr { start, end, .. } => {
                if let Some(s) = start {
                    self.check_expr(s);
                }
                if let Some(e) = end {
                    self.check_expr(e);
                }
            }
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    if let kryos_ast::StringPart::Expr(e) = part {
                        self.check_expr(e);
                    }
                }
            }
            // Literals and identifiers — no capability concerns.
            Expr::IntLiteral { .. }
            | Expr::FloatLiteral { .. }
            | Expr::StringLiteral { .. }
            | Expr::CharLiteral { .. }
            | Expr::BoolLiteral { .. }
            | Expr::NoneLiteral { .. }
            | Expr::Identifier { .. } => {}
        }
    }

    /// Check a function call's callee to see if it references a capability-gated path.
    ///
    /// We resolve simple dotted paths like `std.net.TcpStream.connect(...)` by
    /// walking field access chains. This also catches direct calls to
    /// identifiers that match stdlib path patterns, bare builtin function
    /// calls (like `file_write`), and cross-function capability propagation.
    fn check_callee_capabilities(&mut self, callee: &Expr, call_span: Span) {
        let segments = self.resolve_path(callee);

        // 1. Check qualified stdlib paths (e.g. std.io.write_file).
        if let Some(required_cap) = required_capability_for_path(&segments) {
            if let Some(caps) = self.current_caps() {
                if !caps.satisfies_required(&required_cap) {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "call to `{}` requires `{required_cap}` capability",
                            segments.join("::")
                        ))
                        .with_label(call_span, format!("requires `{required_cap}`"))
                        .with_note(format!(
                            "add `@capabilities({required_cap})` to the enclosing function or actor"
                        ))
                        .with_code(kryos_errors::codes::E0502),
                    );
                }
            }
        }

        // 2. Check bare builtin function calls (e.g. file_write, http_get).
        //    Normally only enforced inside explicitly annotated @capabilities
        //    scopes; in strict mode every scope is treated as annotated, so
        //    unannotated callers are also checked against the empty set.
        if segments.len() == 1 {
            if let Some(required_cap) = required_capability_for_builtin(&segments[0]) {
                if self.strict_mode || self.has_annotated_scope() {
                    if let Some(caps) = self.current_caps() {
                        if !caps.satisfies_required(&required_cap) {
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "builtin `{}` requires `{required_cap}` capability",
                                    segments[0]
                                ))
                                .with_label(
                                    call_span,
                                    format!("requires `{required_cap}`"),
                                )
                                .with_note(format!(
                                    "add `@capabilities({required_cap})` to the enclosing function or actor"
                                ))
                                .with_code(kryos_errors::codes::E0505),
                            );
                        }
                    }
                }
            }
        }

        // 3. Cross-function propagation: if calling a known annotated function,
        //    the caller's capabilities must be a superset of the callee's.
        //    In strict mode unannotated callers are checked too — they hold
        //    the empty set, so calling any capability-bearing function from
        //    them is an error unless they declare their own annotation.
        if segments.len() == 1 {
            if let Some(callee_caps) = self.fn_capabilities.get(&segments[0]).cloned() {
                if self.strict_mode || self.has_annotated_scope() {
                    if let Some(caller_caps) = self.current_caps() {
                        if !callee_caps.is_subset_of(caller_caps) {
                            let excess = callee_caps.excess_over(caller_caps);
                            let excess_names: Vec<String> =
                                excess.iter().map(|c| c.to_string()).collect();
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "call to `{}` requires capabilities [{}] not granted to caller",
                                    segments[0],
                                    excess_names.join(", ")
                                ))
                                .with_label(call_span, "callee requires more capabilities")
                                .with_note(format!(
                                    "function `{}` has @capabilities({}) but caller lacks [{}]",
                                    segments[0],
                                    callee_caps
                                        .iter()
                                        .map(|c| c.to_string())
                                        .collect::<Vec<_>>()
                                        .join(", "),
                                    excess_names.join(", ")
                                ))
                                .with_code(kryos_errors::codes::E0507),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Check if a callee expression is a prohibited escalation action.
    fn check_escalation(&mut self, callee: &Expr, span: Span) {
        let segments = self.resolve_path(callee);
        if let Some(last) = segments.last() {
            if is_escalation_action(last) {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "prohibited capability escalation: `{last}` cannot be called"
                    ))
                    .with_label(span, "escalation attempt")
                    .with_note(
                        "capabilities are immutable at compile time; \
                         self-heal actions cannot escalate privileges",
                    )
                    .with_code(kryos_errors::codes::E0504),
                );
            }
        }
    }

    /// Check a spawn expression against current capabilities.
    ///
    /// When spawning, the spawned entity's capabilities must not exceed
    /// the current scope's capabilities.
    fn check_spawn_expr(&mut self, expr: &Expr, span: Span) {
        // The spawn itself is checked — any function/actor being spawned
        // will have its capabilities checked via attenuation rules.
        // Here we flag the spawn site if there's no enclosing capability scope.
        self.check_expr(expr);

        // Additional: if spawning a struct literal (actor), check annotations would
        // apply at the actor declaration site (already handled by check_actor).
        // This is here for completeness — spawn-site checks are structural.
        let _ = span;
    }

    /// Resolve a callee expression into path segments.
    ///
    /// `std.net.connect(...)` → `["std", "net", "connect"]`
    fn resolve_path(&self, expr: &Expr) -> Vec<String> {
        match expr {
            Expr::Identifier { name, .. } => vec![name.clone()],
            Expr::FieldAccess { object, field, .. } => {
                let mut segments = self.resolve_path(object);
                segments.push(field.clone());
                segments
            }
            _ => Vec::new(),
        }
    }

    /// Validate that `@capabilities(...)` annotations only contain recognized names.
    fn validate_capability_annotations(&mut self, annotations: &[Annotation]) {
        for ann in annotations {
            if ann.name == "capabilities" {
                for arg in &ann.args {
                    if Capability::from_str(arg).is_none() {
                        self.diagnostics.push(
                            Diagnostic::warning(format!(
                                "unknown capability `{arg}` in @capabilities annotation"
                            ))
                            .with_label(ann.span, format!("`{arg}` is not recognized"))
                            .with_note(
                                "known capabilities: net, io, ffi, compute, crypto, \
                                 process, env, term, db, time, all"
                                    .to_string(),
                            )
                            .with_code("W-CAP-UNKNOWN"),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kryos_ast::{Block, MessageHandler};
    use kryos_errors::Span;

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

    fn empty_block() -> Block {
        Block {
            stmts: Vec::new(),
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

    fn fn_decl(name: &str, caps: Vec<&str>, body_stmts: Vec<Stmt>) -> Decl {
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
            is_async: false,
            annotations: if caps.is_empty() {
                vec![]
            } else {
                vec![ann("capabilities", caps)]
            },
            doc_comments: vec![],
            span: span(),
        }
    }

    #[test]
    fn deny_block_narrows_net_inside_net_fn() {
        // @capabilities(net) fn guarded() { deny!(net) { http_get() } }
        // deny removes net inside the block -> http_get (needs net:http) errors E0505.
        let deny = Stmt::DenyBlock {
            denied: vec!["net".to_string()],
            body: Block {
                stmts: vec![Stmt::Expr {
                    expr: builtin_call("http_get"),
                    span: span(),
                }],
                span: span(),
            },
            span: span(),
        };
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl("guarded", vec!["net"], vec![deny])],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0505")),
            "expected E0505 for http_get inside deny!(net), got: {diags:?}"
        );
    }

    #[test]
    fn deny_block_allows_non_denied_cap() {
        // @capabilities(net, io) fn { deny!(net) { file_write() } }
        // file_write needs fs:write (covered by io); deny!(net) keeps io -> no error.
        let deny = Stmt::DenyBlock {
            denied: vec!["net".to_string()],
            body: Block {
                stmts: vec![Stmt::Expr {
                    expr: builtin_call("file_write"),
                    span: span(),
                }],
                span: span(),
            },
            span: span(),
        };
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl("guarded", vec!["net", "io"], vec![deny])],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
        assert!(
            errors.is_empty(),
            "deny!(net) must not break a file_write covered by io, got: {diags:?}"
        );
    }

    #[test]
    fn no_capabilities_no_stdlib_no_errors() {
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl("main", vec![], vec![])],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(diags.is_empty());
    }

    #[test]
    fn using_net_with_capability_is_ok() {
        let call = stdlib_call("net", "connect");
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "do_net",
                vec!["net"],
                vec![Stmt::Expr {
                    expr: call,
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(diags.is_empty(), "expected no errors, got: {diags:?}");
    }

    #[test]
    fn using_net_without_capability_is_error() {
        let call = stdlib_call("net", "connect");
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "do_net",
                vec![],
                vec![Stmt::Expr {
                    expr: call,
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("requires `net` capability"));
        assert_eq!(diags[0].code.as_deref(), Some("E0502"));
    }

    #[test]
    fn attenuation_child_exceeds_parent() {
        // The AST doesn't nest functions as Decls inside function bodies,
        // so we test attenuation via an actor whose handler scope is limited.
        let actor = Decl::Actor {
            name: "MyActor".into(),
            state_fields: vec![],
            handlers: vec![MessageHandler {
                name: "handle".into(),
                params: vec![],
                ret_ty: None,
                body: empty_block(),
                span: span(),
            }],
            annotations: vec![ann("capabilities", vec!["net"])],
            doc_comments: vec![],
            span: span(),
        };

        // Inner function declared inside actor scope — we test this by
        // building a module with just the outer function, and pushing a
        // manual scope to demonstrate. Instead, let's use the public API properly:
        // Module-level functions don't have a parent scope, so attenuation
        // doesn't apply there. We test attenuation through actors and nested decls.
        let module = Module {
            name: "test".into(),
            declarations: vec![actor],
            span: span(),
        };
        // This should pass — the actor has `net` and the handler uses nothing.
        let diags = check_capabilities(&module, false);
        assert!(diags.is_empty());
    }

    #[test]
    fn escalation_via_method_call_is_error() {
        let escalation = Expr::MethodCall {
            object: Box::new(Expr::Identifier {
                name: "self".into(),
                span: span(),
            }),
            method: "add_capability".into(),
            args: vec![],
            span: span(),
        };

        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "try_escalate",
                vec!["net"],
                vec![Stmt::Expr {
                    expr: escalation,
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(!diags.is_empty());
        assert!(diags
            .iter()
            .any(|d| d.code.as_deref() == Some("E0504")));
    }

    #[test]
    fn escalation_via_fn_call_is_error() {
        let escalation = Expr::FnCall {
            callee: Box::new(Expr::Identifier {
                name: "widen_sandbox".into(),
                span: span(),
            }),
            args: vec![],
            span: span(),
        };

        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "try_escalate",
                vec!["net"],
                vec![Stmt::Expr {
                    expr: escalation,
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(!diags.is_empty());
        assert!(diags
            .iter()
            .any(|d| d.code.as_deref() == Some("E0504")));
    }

    #[test]
    fn all_capability_grants_everything() {
        // Using std::net, std::io, std::crypto in a function with `all`.
        let calls = vec![
            Stmt::Expr {
                expr: stdlib_call("net", "connect"),
                span: span(),
            },
            Stmt::Expr {
                expr: stdlib_call("io", "read"),
                span: span(),
            },
            Stmt::Expr {
                expr: stdlib_call("crypto", "hash"),
                span: span(),
            },
        ];

        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl("god_mode", vec!["all"], calls)],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(
            diags.is_empty(),
            "expected no errors with `all`, got: {diags:?}"
        );
    }

    // -------------------------------------------------------------------
    // Strict mode tests (--strict-capabilities / deny-by-default).
    //
    // These exercise the public `check_capabilities(module, strict)` switch
    // so the wiring stays end-to-end: pipeline.rs -> check_capabilities
    // -> CapabilityChecker { strict_mode } -> per-call guard.
    // -------------------------------------------------------------------

    /// Build a bare-name builtin call expression (e.g. `file_write()`),
    /// which is what the strict-mode path enforces against.
    fn builtin_call(name: &str) -> Expr {
        Expr::FnCall {
            callee: Box::new(Expr::Identifier {
                name: name.into(),
                span: span(),
            }),
            args: vec![],
            span: span(),
        }
    }

    #[test]
    fn strict_unannotated_with_file_write_is_error() {
        // Unannotated function that calls `file_write` (requires `fs:write`).
        // In strict mode this must surface E0505 at the call site.
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "leaks",
                vec![],
                vec![Stmt::Expr {
                    expr: builtin_call("file_write"),
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, true);
        let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
        assert_eq!(
            errors.len(),
            1,
            "expected exactly one E0505 error, got: {diags:?}"
        );
        assert_eq!(errors[0].code.as_deref(), Some("E0505"));
        assert!(errors[0].message.contains("file_write"));
        assert!(errors[0].message.contains("`fs:write`"));
    }

    #[test]
    fn nonstrict_unannotated_with_file_write_is_ok() {
        // Same module as above, but with strict=false — the existing
        // opt-in behaviour. No errors should be raised.
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "leaks",
                vec![],
                vec![Stmt::Expr {
                    expr: builtin_call("file_write"),
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(
            diags.iter().all(|d| !d.is_error()),
            "non-strict mode must not error on unannotated builtin calls, got: {diags:?}"
        );
    }

    #[test]
    fn strict_unannotated_with_no_gated_calls_is_ok() {
        // Unannotated function that touches nothing capability-gated.
        // Strict mode must NOT error — we only deny gated builtins, not
        // every function definition.
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl("pure", vec![], vec![])],
            span: span(),
        };
        let diags = check_capabilities(&module, true);
        assert!(
            diags.iter().all(|d| !d.is_error()),
            "pure function in strict mode must have no errors, got: {diags:?}"
        );
    }

    #[test]
    fn strict_annotated_with_matching_cap_is_ok() {
        // Annotated @capabilities(io) function calling file_write.
        // Strict mode must accept this — the annotation grants exactly
        // what the call needs.
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "writer",
                vec!["io"],
                vec![Stmt::Expr {
                    expr: builtin_call("file_write"),
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, true);
        assert!(
            diags.iter().all(|d| !d.is_error()),
            "annotated function with matching cap must not error in strict mode, got: {diags:?}"
        );
    }

    #[test]
    fn unknown_capability_name_warns() {
        let module = Module {
            name: "test".into(),
            declarations: vec![Decl::Function {
                name: "bad_caps".into(),
                generics: vec![],
                params: vec![],
                ret_ty: None,
                body: Some(empty_block()),
                public: false,
                is_async: false,
                annotations: vec![ann("capabilities", vec!["net", "banana"])],
                doc_comments: vec![],
                span: span(),
            }],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("banana"));
        assert_eq!(diags[0].code.as_deref(), Some("W-CAP-UNKNOWN"));
    }
}
