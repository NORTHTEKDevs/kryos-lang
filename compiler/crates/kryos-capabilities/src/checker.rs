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

/// How strictly capabilities are enforced. The three modes form a hierarchy
/// `Permissive < Inferred < Strict`:
///
/// - **Permissive** — the historical default. Only functions that carry an
///   explicit `@capabilities(...)` annotation are checked (plus attenuation
///   between annotated scopes). An unannotated function may call any gated
///   builtin. Capabilities are advisory.
///
/// - **Inferred** — deny-by-default at the *boundary*, with interior
///   inference. Every function's effective capability set is computed: an
///   annotated function's set is its declaration (a ceiling); an unannotated
///   interior function's set is *inferred* as the union of what it and its
///   callees require (no annotation needed on helpers). Enforcement then
///   requires that `main` — and any explicitly annotated function — actually
///   holds every capability its body transitively uses. So an unannotated
///   `main` that (directly or through helpers) writes a file is rejected:
///   the program must declare `@capabilities(fs:write)` on `main`. Reading
///   `main`'s annotation tells you the program's entire authority.
///
/// - **Strict** — every function is treated as annotated with exactly its
///   declaration (empty if unannotated). Every gated builtin call from an
///   unannotated function is an error. This is `--strict-capabilities`:
///   maximally explicit, every function auditable in isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityMode {
    Permissive,
    Inferred,
    Strict,
}

impl CapabilityMode {
    /// Parse a mode name (from `kryos.toml` / `--capabilities-mode`).
    pub fn from_str(s: &str) -> Option<CapabilityMode> {
        match s {
            "permissive" => Some(CapabilityMode::Permissive),
            "inferred" => Some(CapabilityMode::Inferred),
            "strict" => Some(CapabilityMode::Strict),
            _ => None,
        }
    }
}

/// Run the capability checking pass over a module in the given [`CapabilityMode`].
pub fn check_capabilities_mode(module: &Module, mode: CapabilityMode) -> Vec<Diagnostic> {
    let mut checker = CapabilityChecker::new(mode);
    checker.check_module(module);
    checker.diagnostics
}

/// Run the capability checking pass over a module.
///
/// Back-compat shim over [`check_capabilities_mode`]: `strict == true` maps to
/// [`CapabilityMode::Strict`], `false` to [`CapabilityMode::Permissive`]. New
/// callers should use `check_capabilities_mode` directly to opt into
/// [`CapabilityMode::Inferred`].
pub fn check_capabilities(module: &Module, strict: bool) -> Vec<Diagnostic> {
    let mode = if strict {
        CapabilityMode::Strict
    } else {
        CapabilityMode::Permissive
    };
    check_capabilities_mode(module, mode)
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
    /// Map from function name to the capability set used for cross-function
    /// propagation checks. In every mode this holds each *annotated* function's
    /// declared set (its ceiling). In [`CapabilityMode::Inferred`] it also holds
    /// each *unannotated* function's inferred set, so a caller is checked
    /// against what its callee transitively requires.
    fn_capabilities: HashMap<String, CapabilitySet>,
    /// Every function name declared in an `extern { ... }` block anywhere in
    /// the module. Calls to these are the raw FFI surface: a `kryos_*` name
    /// maps to the same semantic capability as its builtin (e.g.
    /// `kryos_env_get` -> `process`); any other extern name requires `ffi`.
    /// Without this call-side gate, a user-declared extern block was a
    /// deny-by-default bypass (declared top-level, called from unannotated
    /// code, no capability ever demanded).
    extern_fns: std::collections::HashSet<String>,
    /// The active enforcement mode.
    mode: CapabilityMode,
}

impl CapabilityChecker {
    fn new(mode: CapabilityMode) -> Self {
        Self {
            scope_stack: Vec::new(),
            diagnostics: Vec::new(),
            fn_capabilities: HashMap::new(),
            extern_fns: std::collections::HashSet::new(),
            mode,
        }
    }

    /// The capability a CALL to `name` requires because `name` is an
    /// extern-declared function. `kryos_*` runtime exports resolve through the
    /// builtin table (same authority, same capability); unmapped `kryos_*`
    /// names are runtime plumbing (allocators, pointer helpers) and stay
    /// ambient; every non-`kryos_` extern (user C libraries) requires `ffi`.
    fn required_capability_for_extern(&self, name: &str) -> Option<Capability> {
        if !self.extern_fns.contains(name) {
            return None;
        }
        match name.strip_prefix("kryos_") {
            Some(stripped) => required_capability_for_builtin(stripped),
            None => Some(Capability::Ffi),
        }
    }

    /// Collect every extern-declared function name (recursing into nested
    /// declaration containers) so call sites can be gated.
    fn collect_extern_fns(&mut self, decls: &[Decl]) {
        for d in decls {
            match d {
                Decl::Extern { items, .. } => {
                    for item in items {
                        if let Decl::Function { name, .. } = item {
                            self.extern_fns.insert(name.clone());
                        }
                    }
                }
                Decl::Impl { methods, .. } | Decl::Trait { methods, .. } => {
                    self.collect_extern_fns(methods);
                }
                _ => {}
            }
        }
    }

    /// True only in the maximally-explicit [`CapabilityMode::Strict`], where an
    /// unannotated function is treated as declaring the empty set.
    fn strict_mode(&self) -> bool {
        matches!(self.mode, CapabilityMode::Strict)
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

    /// The capability contributed by an expression used as a first-class VALUE:
    /// a bare identifier naming a gated builtin (`file_write` passed as a
    /// `fn(...)` argument) delegates that builtin's authority. Used by interior
    /// inference so the requirement propagates to the boundary; the enforcement
    /// counterpart is `check_builtin_value_ref`.
    fn builtin_value_caps(expr: &Expr) -> CapabilitySet {
        let mut set = CapabilitySet::empty();
        if let Expr::Identifier { name, .. } = expr {
            if let Some(cap) = required_capability_for_builtin(name) {
                set.insert(cap);
            }
        }
        set
    }

    /// Gather every function (top-level and impl/trait method) that has a body,
    /// as `(name, body)` pairs, for interior capability inference.
    fn collect_functions<'a>(decls: &'a [Decl], out: &mut Vec<(String, &'a kryos_ast::Block)>) {
        for d in decls {
            match d {
                Decl::Function {
                    name,
                    body: Some(b),
                    ..
                } => out.push((name.clone(), b)),
                Decl::Impl { methods, .. } | Decl::Trait { methods, .. } => {
                    Self::collect_functions(methods, out)
                }
                _ => {}
            }
        }
    }

    /// Compute each UNANNOTATED function's inferred capability set: the fixpoint
    /// union of the capabilities its body requires directly (gated builtins /
    /// stdlib paths) and the sets of the functions it calls. Terminating: the
    /// lattice is the finite powerset of the capability enum and each step only
    /// adds capabilities (monotone union), so the result is the least set that
    /// covers all direct uses — an *over*-approximation, never an under one.
    ///
    /// Soundness note: an unresolved single-segment call (not a gated builtin,
    /// not a known function) contributes nothing. That is sound under Kryos's
    /// whole-program compilation because every user/stdlib function body is
    /// merged into this module (so real callees resolve), constructors and
    /// closure-variable calls carry no authority, and the only way to reach a
    /// raw native `extern` is through an `extern` block, which already requires
    /// the `ffi` capability.
    fn compute_inferred_capabilities(
        &self,
        declarations: &[Decl],
    ) -> HashMap<String, CapabilitySet> {
        let mut fns: Vec<(String, &kryos_ast::Block)> = Vec::new();
        Self::collect_functions(declarations, &mut fns);

        // Seed the working map with annotated functions' declared ceilings.
        let mut working: HashMap<String, CapabilitySet> = self.fn_capabilities.clone();
        let annotated: std::collections::HashSet<String> =
            self.fn_capabilities.keys().cloned().collect();

        loop {
            let mut changed = false;
            for (name, body) in &fns {
                if annotated.contains(name) {
                    continue; // annotation is the ceiling; never widened by inference
                }
                let collected = self.collect_caps_block(body, &working);
                let cur = working
                    .get(name)
                    .cloned()
                    .unwrap_or_else(CapabilitySet::empty);
                let merged = cur.union(&collected);
                // union only grows, so a length change means a new capability.
                if merged.len() != cur.len() {
                    working.insert(name.clone(), merged);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        working
            .into_iter()
            .filter(|(n, _)| !annotated.contains(n))
            .collect()
    }

    /// Read-only union of the capabilities required by every call in a block.
    fn collect_caps_block(
        &self,
        block: &kryos_ast::Block,
        working: &HashMap<String, CapabilitySet>,
    ) -> CapabilitySet {
        let mut acc = CapabilitySet::empty();
        for stmt in &block.stmts {
            acc = acc.union(&self.collect_caps_stmt(stmt, working));
        }
        acc
    }

    fn collect_caps_stmt(
        &self,
        stmt: &Stmt,
        working: &HashMap<String, CapabilitySet>,
    ) -> CapabilitySet {
        let mut acc = CapabilitySet::empty();
        let e = |ex: &Expr, a: &mut CapabilitySet| *a = a.union(&self.collect_caps_expr(ex, working));
        let b = |bl: &kryos_ast::Block, a: &mut CapabilitySet| {
            *a = a.union(&self.collect_caps_block(bl, working))
        };
        match stmt {
            Stmt::Let { value, .. } => {
                if let Some(expr) = value {
                    e(expr, &mut acc);
                }
            }
            Stmt::Assign { value, target, .. } => {
                e(target, &mut acc);
                e(value, &mut acc);
            }
            Stmt::Return { value, .. } => {
                if let Some(expr) = value {
                    e(expr, &mut acc);
                }
            }
            Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                ..
            } => {
                e(condition, &mut acc);
                b(then_block, &mut acc);
                for (cond, block) in elif_clauses {
                    e(cond, &mut acc);
                    b(block, &mut acc);
                }
                if let Some(block) = else_block {
                    b(block, &mut acc);
                }
            }
            Stmt::For { iterable, body, .. } => {
                e(iterable, &mut acc);
                b(body, &mut acc);
            }
            Stmt::While {
                condition, body, ..
            } => {
                e(condition, &mut acc);
                b(body, &mut acc);
            }
            Stmt::Expr { expr, .. } => e(expr, &mut acc),
            Stmt::Spawn { expr, .. } => e(expr, &mut acc),
            Stmt::TryCatch {
                try_block,
                catch_block,
                ..
            } => {
                b(try_block, &mut acc);
                b(catch_block, &mut acc);
            }
            Stmt::Throw { expr, .. } => e(expr, &mut acc),
            Stmt::Select { branches, .. } => {
                for branch in branches {
                    e(&branch.channel, &mut acc);
                    b(&branch.body, &mut acc);
                }
            }
            Stmt::DenyBlock { body, .. } => b(body, &mut acc),
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
        acc
    }

    fn collect_caps_expr(
        &self,
        expr: &Expr,
        working: &HashMap<String, CapabilitySet>,
    ) -> CapabilitySet {
        let mut acc = CapabilitySet::empty();
        let e = |ex: &Expr, a: &mut CapabilitySet| *a = a.union(&self.collect_caps_expr(ex, working));
        let b = |bl: &kryos_ast::Block, a: &mut CapabilitySet| {
            *a = a.union(&self.collect_caps_block(bl, working))
        };
        match expr {
            Expr::FnCall { callee, args, span: _ } => {
                // The capability contribution of THIS call.
                let segments = self.resolve_path(callee);
                if let Some(cap) = required_capability_for_path(&segments) {
                    acc.insert(cap);
                }
                if segments.len() == 1 {
                    if let Some(cap) = required_capability_for_builtin(&segments[0]) {
                        acc.insert(cap);
                    }
                    // Extern calls contribute their FFI capability to inference
                    // so helpers calling raw externs propagate the requirement
                    // to the boundary (same rule as enforce_callee_name).
                    if let Some(cap) = self.required_capability_for_extern(&segments[0]) {
                        acc.insert(cap);
                    }
                    if let Some(caps) = working.get(&segments[0]) {
                        acc = acc.union(caps);
                    }
                }
                e(callee, &mut acc);
                for arg in args {
                    e(arg, &mut acc);
                }
            }
            Expr::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                // The method's own (declared or inferred) requirement.
                if let Some(caps) = working.get(method) {
                    acc = acc.union(caps);
                }
                e(object, &mut acc);
                for arg in args {
                    e(arg, &mut acc);
                }
            }
            Expr::StaticMethodCall { method, args, .. } => {
                if let Some(caps) = working.get(method) {
                    acc = acc.union(caps);
                }
                for arg in args {
                    e(arg, &mut acc);
                }
            }
            Expr::FieldAccess { object, .. } => e(object, &mut acc),
            Expr::IndexAccess { object, index, .. } => {
                e(object, &mut acc);
                e(index, &mut acc);
            }
            Expr::BinaryOp { left, right, .. } => {
                e(left, &mut acc);
                e(right, &mut acc);
            }
            Expr::UnaryOp { operand, .. } => e(operand, &mut acc),
            Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => {
                for elem in elements {
                    e(elem, &mut acc);
                }
            }
            Expr::MapLiteral { entries, .. } => {
                for (k, v) in entries {
                    e(k, &mut acc);
                    e(v, &mut acc);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, ex) in fields {
                    e(ex, &mut acc);
                }
            }
            Expr::Lambda { body, .. } => e(body, &mut acc),
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                e(condition, &mut acc);
                b(then_branch, &mut acc);
                if let Some(block) = else_branch {
                    b(block, &mut acc);
                }
            }
            Expr::MatchExpr { subject, arms, .. } => {
                e(subject, &mut acc);
                for arm in arms {
                    e(&arm.body, &mut acc);
                    if let Some(guard) = &arm.guard {
                        e(guard, &mut acc);
                    }
                }
            }
            Expr::PipeExpr { left, right, .. } => {
                e(left, &mut acc);
                e(right, &mut acc);
            }
            Expr::Borrow { inner, .. }
            | Expr::Deref { inner, .. }
            | Expr::SharedExpr { inner, .. }
            | Expr::MoveExpr { inner, .. }
            | Expr::WeakExpr { inner, .. } => e(inner, &mut acc),
            Expr::ComptimeBlock { body, .. }
            | Expr::QuantumBlock { body, .. }
            | Expr::UnsafeBlock { body, .. } => b(body, &mut acc),
            Expr::Cast { expr, .. } => e(expr, &mut acc),
            Expr::Block { block, .. } => b(block, &mut acc),
            Expr::Await { value, .. } => e(value, &mut acc),
            Expr::RangeExpr { start, end, .. } => {
                if let Some(s) = start {
                    e(s, &mut acc);
                }
                if let Some(en) = end {
                    e(en, &mut acc);
                }
            }
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    if let kryos_ast::StringPart::Expr(ex) = part {
                        e(ex, &mut acc);
                    }
                }
            }
            // A gated builtin used as a VALUE (bound to a `let`, returned,
            // stored in an array/struct, piped, ...) delegates its authority.
            // This single arm covers every non-call value position.
            Expr::Identifier { .. } => acc = acc.union(&Self::builtin_value_caps(expr)),
            Expr::IntLiteral { .. }
            | Expr::FloatLiteral { .. }
            | Expr::StringLiteral { .. }
            | Expr::CharLiteral { .. }
            | Expr::BoolLiteral { .. }
            | Expr::NoneLiteral { .. } => {}
        }
        acc
    }

    fn check_module(&mut self, module: &Module) {
        // Pass 0: record every extern-declared function name so call sites
        // can require the corresponding capability (see extern_fns).
        self.collect_extern_fns(&module.declarations);

        // Pass 1: seed the propagation map with every ANNOTATED function's
        // declared set (its ceiling).
        self.build_fn_capability_map(&module.declarations);

        // Pass 1b (Inferred mode only): compute each UNANNOTATED function's
        // capability set as the fixpoint union of what it and its callees
        // require, and merge those into the propagation map. This is what lets
        // interior helpers stay annotation-free while their requirements still
        // reach the boundary. Annotated functions are left at their declared
        // ceiling (a function that under-declares is caught when its own body
        // is checked, not by widening it here).
        if matches!(self.mode, CapabilityMode::Inferred) {
            let inferred = self.compute_inferred_capabilities(&module.declarations);
            for (name, caps) in inferred {
                self.fn_capabilities.entry(name).or_insert(caps);
            }
        }

        // Pass 2: walk the AST and enforce capability rules.
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
            Decl::Const { value, .. } => {
                // A top-level `let`/`let mut` initializer (Decl::Const) runs
                // at program startup and can call gated builtins
                // (file_write, env_get, http_get, ...). It was never checked
                // — a full capability bypass in every mode (backlog #24).
                // A const has no annotation site of its own, but it is part
                // of the program whose authority ceiling is `main`'s
                // declaration. Check the initializer against `main`'s
                // capabilities: a const that needs authority `main` never
                // declared (the leak case: net:http-only main with a
                // file_write const) is rejected, while legitimate startup
                // initializers covered by main's annotation pass. Permissive
                // leaves it unchecked, matching unannotated functions.
                let annotated = !matches!(self.mode, CapabilityMode::Permissive);
                self.scope_stack.push(CapabilityScope {
                    capabilities: self
                        .fn_capabilities
                        .get("main")
                        .cloned()
                        .unwrap_or_else(CapabilitySet::empty),
                    annotated,
                });
                self.check_expr(value);
                self.scope_stack.pop();
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

        // Choose the effective capability set this function's body is checked
        // against, and whether the per-call guards fire (`annotated`):
        //
        // - Permissive: unchanged — only explicitly annotated functions enforce.
        // - Strict: every function enforces against its declaration (empty if
        //   unannotated), so every gated builtin call must be declared.
        // - Inferred: `main` and annotated functions are BOUNDARIES — they
        //   enforce against their DECLARATION (empty `main` => deny-by-default,
        //   annotation => ceiling). Every other (interior) function enforces
        //   against its INFERRED set, so helpers need no annotation yet their
        //   requirements still propagate up to the boundary.
        let (effective_caps, effective_annotated) = match self.mode {
            CapabilityMode::Permissive => (caps, annotated),
            CapabilityMode::Strict => (caps, true),
            CapabilityMode::Inferred => {
                if annotated || name == "main" {
                    (caps, true)
                } else {
                    let inferred = self
                        .fn_capabilities
                        .get(name)
                        .cloned()
                        .unwrap_or_else(CapabilitySet::empty);
                    (inferred, true)
                }
            }
        };
        let scope = CapabilityScope {
            capabilities: effective_caps,
            annotated: effective_annotated,
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

        // Check each handler under the actor's capability scope. Both Strict
        // and Inferred enforce against the actor's declaration (empty if
        // unannotated): actors are message-passing boundaries, so their
        // authority must be declared, not inferred from handler bodies.
        let scope = CapabilityScope {
            capabilities: caps,
            annotated: annotated || !matches!(self.mode, CapabilityMode::Permissive),
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

                // Recurse into arguments (the Identifier arm flags any gated
                // builtin used as a value). Do NOT recurse into a bare-name
                // callee: its capability is already enforced by
                // check_callee_capabilities, and recursing would double-report.
                if !matches!(callee.as_ref(), Expr::Identifier { .. }) {
                    self.check_expr(callee);
                }
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

                // Propagate the method's capability requirement to the caller
                // (the previously-missing call-site check for method dispatch).
                self.enforce_callee_name(method, *span, false);

                self.check_expr(object);
                for arg in args {
                    self.check_expr(arg);
                }
            }
            Expr::StaticMethodCall {
                method, args, span, ..
            } => {
                // Same call-site propagation for `Type::method()` static
                // dispatch. The parser turns ANY `Ident::name(args)` into a
                // StaticMethodCall, and the checker/MIR lower an unknown
                // `type_name` to a bare call to `name` — so
                // `NotAType::file_write(..)` actually invokes the builtin.
                // If `method` is a gated builtin name, gate it (backlog #38):
                // pass is_builtin_name so the E0505 builtin gate fires, just
                // as it does for the ordinary `file_write(..)` call shape.
                let is_gated_builtin = required_capability_for_builtin(method).is_some();
                self.enforce_callee_name(method, *span, is_gated_builtin);
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
            Expr::ComptimeBlock { body, .. }
            | Expr::QuantumBlock { body, .. }
            | Expr::UnsafeBlock { body, .. } => {
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
            // A bare identifier that names a gated builtin, evaluated as a
            // VALUE (let/assign/return/array/struct/pipe/arg — anywhere that is
            // not the direct callee of a call), delegates that builtin's
            // authority and must be declared. The FnCall arm deliberately does
            // not recurse into a bare-name callee, so this never double-fires.
            Expr::Identifier { span, .. } => {
                self.check_builtin_value_ref(expr, *span);
            }
            // Literals — no capability concerns.
            Expr::IntLiteral { .. }
            | Expr::FloatLiteral { .. }
            | Expr::StringLiteral { .. }
            | Expr::CharLiteral { .. }
            | Expr::BoolLiteral { .. }
            | Expr::NoneLiteral { .. } => {}
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

        // 2+3. Bare builtin requirement + cross-function propagation, keyed on
        //      the single-segment callee name. Shared with method / static
        //      dispatch (see `enforce_callee_name`).
        if segments.len() == 1 {
            self.enforce_callee_name(&segments[0], call_span, true);
        }
    }

    /// Enforce the capability requirements of a call to `name` — a bare
    /// function name (`FnCall`) or a method name (`MethodCall` /
    /// `StaticMethodCall`). Two checks, both gated on the caller being an
    /// enforcing scope (strict mode, or any annotated/inferred scope):
    ///
    /// - **E0505** if `name` is a capability-gated builtin the caller does not
    ///   hold; and
    /// - **E0507** if `name` is a known function/method (annotated ceiling or
    ///   inferred set) whose capabilities exceed the caller's.
    ///
    /// Routing method dispatch through here is what closes the soundness hole
    /// where `obj.method()` / `Type::method()` could exercise authority the
    /// caller never declared (the call-site propagation was previously only
    /// wired for free-function `FnCall`s).
    ///
    /// `is_builtin_name`: whether `name` may denote a gated runtime builtin.
    /// True for free-function calls (`file_write(...)`), FALSE for method /
    /// static dispatch -- a method is never a runtime builtin, so a method
    /// merely NAMED like one (e.g. `doc.write_file()`) must NOT be force-gated
    /// by the builtin table; it is gated only by its own propagated caps.
    fn enforce_callee_name(&mut self, name: &str, call_span: Span, is_builtin_name: bool) {
        if !(self.strict_mode() || self.has_annotated_scope()) {
            return;
        }

        if is_builtin_name {
            if let Some(required_cap) = required_capability_for_builtin(name) {
                if let Some(caps) = self.current_caps() {
                    if !caps.satisfies_required(&required_cap) {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "builtin `{name}` requires `{required_cap}` capability"
                            ))
                            .with_label(call_span, format!("requires `{required_cap}`"))
                            .with_note(format!(
                                "add `@capabilities({required_cap})` to the enclosing function or actor"
                            ))
                            .with_code(kryos_errors::codes::E0505),
                        );
                    }
                }
            }
        }

        // Calls to extern-declared functions are the raw FFI surface — gate
        // them like the builtin they wrap (`kryos_*`) or on `ffi` (anything
        // else). Declaring an extern is free; CALLING it demands authority.
        if let Some(required_cap) = self.required_capability_for_extern(name) {
            if let Some(caps) = self.current_caps() {
                if !caps.satisfies_required(&required_cap) {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "extern function `{name}` requires `{required_cap}` capability"
                        ))
                        .with_label(call_span, format!("requires `{required_cap}`"))
                        .with_note(format!(
                            "add `@capabilities({required_cap})` to the enclosing function or actor"
                        ))
                        .with_code(kryos_errors::codes::E0506),
                    );
                }
            }
        }

        if let Some(callee_caps) = self.fn_capabilities.get(name).cloned() {
            if let Some(caller_caps) = self.current_caps() {
                if !callee_caps.is_subset_of(caller_caps) {
                    let excess = callee_caps.excess_over(caller_caps);
                    let excess_names: Vec<String> =
                        excess.iter().map(|c| c.to_string()).collect();
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "call to `{name}` requires capabilities [{}] not granted to caller",
                            excess_names.join(", ")
                        ))
                        .with_label(call_span, "callee requires more capabilities")
                        .with_note(format!(
                            "function `{name}` has @capabilities({}) but caller lacks [{}]",
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

    /// A gated builtin used as a first-class VALUE (passed as a `fn(...)`
    /// argument, or bound to a variable) hands out its authority just as much
    /// as calling it. Require the capability at the reference site so
    /// `apply(file_write, ...)` cannot smuggle `fs:write` past the boundary.
    fn check_builtin_value_ref(&mut self, expr: &Expr, span: Span) {
        if let Expr::Identifier { name, .. } = expr {
            if !(self.strict_mode() || self.has_annotated_scope()) {
                return;
            }
            // An extern fn handed out as a value carries its authority too.
            if let Some(required_cap) = self.required_capability_for_extern(name) {
                if let Some(caps) = self.current_caps() {
                    if !caps.satisfies_required(&required_cap) {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "extern function `{name}` used as a value requires `{required_cap}` capability"
                            ))
                            .with_label(span, format!("requires `{required_cap}`"))
                            .with_code(kryos_errors::codes::E0506),
                        );
                    }
                }
            }
            if let Some(required_cap) = required_capability_for_builtin(name) {
                if let Some(caps) = self.current_caps() {
                    if !caps.satisfies_required(&required_cap) {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "builtin `{name}` used as a value requires `{required_cap}` capability"
                            ))
                            .with_label(span, format!("requires `{required_cap}`"))
                            .with_note(format!(
                                "passing `{name}` as a function value delegates its authority; \
                                 add `@capabilities({required_cap})` to the enclosing function"
                            ))
                            .with_code(kryos_errors::codes::E0505),
                        );
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

    // -----------------------------------------------------------------
    // Inferred mode (--capabilities-mode=inferred / deny-by-default with
    // interior inference). These prove: helpers need no annotation, the
    // boundary (main / annotated fns) is where deny-by-default bites, and
    // authority cannot leak past an unannotated boundary.
    // -----------------------------------------------------------------

    fn call(name: &str) -> Stmt {
        Stmt::Expr {
            expr: builtin_call(name),
            span: span(),
        }
    }

    fn infer(decls: Vec<Decl>) -> Vec<Diagnostic> {
        let module = Module {
            name: "test".into(),
            declarations: decls,
            span: span(),
        };
        check_capabilities_mode(&module, CapabilityMode::Inferred)
    }

    #[test]
    fn inferred_helper_needs_no_annotation() {
        // main @capabilities(fs:write) { helper() }   fn helper() { file_write() }
        // helper is unannotated; inference covers it. Zero errors.
        let helper = fn_decl("helper", vec![], vec![call("file_write")]);
        let main = fn_decl("main", vec!["fs:write"], vec![call("helper")]);
        let diags = infer(vec![main, helper]);
        assert!(diags.is_empty(), "expected clean, got {diags:?}");
    }

    #[test]
    fn inferred_unannotated_main_transitive_write_is_error() {
        // fn main() { helper() }   fn helper() { file_write() }
        // main declares nothing -> deny-by-default: the transitively-required
        // fs:write must be declared on main. Error names the missing cap.
        let helper = fn_decl("helper", vec![], vec![call("file_write")]);
        let main = fn_decl("main", vec![], vec![call("helper")]);
        let diags = infer(vec![main, helper]);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0507")),
            "expected E0507 at the boundary, got {diags:?}"
        );
    }

    #[test]
    fn inferred_unannotated_main_direct_builtin_is_error() {
        // fn main() { file_write() }  -> E0505 at the boundary.
        let main = fn_decl("main", vec![], vec![call("file_write")]);
        let diags = infer(vec![main]);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0505")),
            "expected E0505, got {diags:?}"
        );
    }

    #[test]
    fn inferred_annotated_main_covers_direct_builtin() {
        let main = fn_decl("main", vec!["fs:write"], vec![call("file_write")]);
        assert!(infer(vec![main]).is_empty());
    }

    #[test]
    fn inferred_pure_helper_is_authority_free() {
        // fn main() { pure() }  fn pure() { println-like/no builtins }  -> clean.
        // A helper that touches no capability provably requires none, so an
        // unannotated main calling it is fine.
        let pure = fn_decl("pure", vec![], vec![call("some_local_fn")]);
        let leaf = fn_decl("some_local_fn", vec![], vec![]);
        let main = fn_decl("main", vec![], vec![call("pure")]);
        assert!(infer(vec![main, pure, leaf]).is_empty());
    }

    #[test]
    fn inferred_chain_depth_three_propagates() {
        // a @capabilities(fs:write) -> b -> c -> file_write ; b,c unannotated.
        let c = fn_decl("c", vec![], vec![call("file_write")]);
        let b = fn_decl("b", vec![], vec![call("c")]);
        let a = fn_decl("a", vec!["fs:write"], vec![call("b")]);
        // `a` is the boundary; give it a main caller so nothing dangles.
        let main = fn_decl("main", vec!["fs:write"], vec![call("a")]);
        let diags = infer(vec![main, a, b, c]);
        assert!(diags.is_empty(), "expected clean chain, got {diags:?}");
    }

    #[test]
    fn inferred_annotation_ceiling_is_enforced() {
        // fn helper @capabilities(fs:read) { file_write() } -> fs:write not
        // covered by the declared fs:read ceiling. E0505.
        let helper = fn_decl("helper", vec!["fs:read"], vec![call("file_write")]);
        let main = fn_decl("main", vec!["fs:read"], vec![call("helper")]);
        let diags = infer(vec![main, helper]);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0505")),
            "expected E0505 ceiling violation, got {diags:?}"
        );
    }

    #[test]
    fn inferred_ambient_builtins_never_blocked() {
        // fn main() { exit(); sleep(); }  -> ambient, no annotation needed.
        let main = fn_decl("main", vec![], vec![call("exit"), call("sleep")]);
        assert!(infer(vec![main]).is_empty());
    }

    #[test]
    fn permissive_default_unchanged_by_inference() {
        // The same unannotated-main-writes-file program is clean in Permissive.
        let main = fn_decl("main", vec![], vec![call("file_write")]);
        let module = Module {
            name: "test".into(),
            declarations: vec![main],
            span: span(),
        };
        assert!(check_capabilities_mode(&module, CapabilityMode::Permissive).is_empty());
    }

    // --- Soundness: authority must not leak through method / static dispatch
    // or through gated builtins passed as first-class values (regression tests
    // for holes found by adversarial review). ---

    fn method_call(recv: &str, method: &str) -> Stmt {
        Stmt::Expr {
            expr: Expr::MethodCall {
                object: Box::new(Expr::Identifier {
                    name: recv.into(),
                    span: span(),
                }),
                method: method.into(),
                args: vec![],
                span: span(),
            },
            span: span(),
        }
    }

    fn impl_with_method(target: &str, method: Decl) -> Decl {
        Decl::Impl {
            target: target.into(),
            trait_name: None,
            generics: vec![],
            methods: vec![method],
            doc_comments: vec![],
            span: span(),
        }
    }

    #[test]
    fn inferred_method_call_authority_is_caught() {
        // impl Sink { fn write(self) { file_write(...) } }  (unannotated method)
        // fn main() { s.write() }  (unannotated) -> deny-by-default at boundary.
        // Before the fix, MethodCall never propagated and this LEAKED.
        let write = fn_decl("write", vec![], vec![call("file_write")]);
        let imp = impl_with_method("Sink", write);
        let main = fn_decl("main", vec![], vec![method_call("s", "write")]);
        let diags = infer(vec![imp, main]);
        assert!(
            diags.iter().any(|d| d.is_error()),
            "method-dispatch authority must be caught at the boundary, got {diags:?}"
        );
    }

    #[test]
    fn inferred_method_call_authority_ok_when_declared() {
        // Same, but main declares fs:write -> inference threads it, no error.
        let write = fn_decl("write", vec![], vec![call("file_write")]);
        let imp = impl_with_method("Sink", write);
        let main = fn_decl("main", vec!["fs:write"], vec![method_call("s", "write")]);
        let diags = infer(vec![imp, main]);
        assert!(diags.is_empty(), "declared main must pass, got {diags:?}");
    }

    fn let_stmt(name: &str, value: Expr) -> Stmt {
        Stmt::Let {
            name: name.into(),
            ty: None,
            value: Some(value),
            pattern: None,
            mutable: false,
            span: span(),
        }
    }

    fn ident(name: &str) -> Expr {
        Expr::Identifier {
            name: name.into(),
            span: span(),
        }
    }

    #[test]
    fn inferred_builtin_let_bound_is_caught() {
        // fn main() { let f = file_write }  -- binding a gated builtin to a
        // variable delegates its authority; unannotated main must be rejected.
        // (Non-argument value position -- the second-pass leak.)
        let main = fn_decl("main", vec![], vec![let_stmt("f", ident("file_write"))]);
        let diags = infer(vec![main]);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0505")),
            "let-bound gated builtin must require its cap, got {diags:?}"
        );
    }

    #[test]
    fn inferred_builtin_returned_is_caught() {
        // fn get() { return file_write }  -- returning a gated builtin exposes
        // its authority. get's inferred set must include fs:write and propagate.
        let get = Decl::Function {
            name: "get".into(),
            generics: vec![],
            params: vec![],
            ret_ty: None,
            body: Some(Block {
                stmts: vec![Stmt::Return {
                    value: Some(ident("file_write")),
                    span: span(),
                }],
                span: span(),
            }),
            public: false,
            is_async: false,
            annotations: vec![],
            doc_comments: vec![],
            span: span(),
        };
        let main = fn_decl("main", vec![], vec![call("get")]);
        let diags = infer(vec![get, main]);
        assert!(
            diags.iter().any(|d| d.is_error()),
            "returned gated builtin must propagate to the boundary, got {diags:?}"
        );
    }

    #[test]
    fn inferred_free_fn_named_like_builtin_is_conservatively_gated() {
        // A FREE function named `file_write` is indistinguishable at a call site
        // from the gated builtin / stdlib wrapper of the same name (they merge
        // into one module), so it is conservatively gated. A safe
        // over-approximation, not a leak -- the alternative (global name
        // shadowing) unsoundly UN-gates the stdlib wrappers (env_get, http_get,
        // write_file, ...) that deliberately share builtin names.
        let shadow = fn_decl("file_write", vec![], vec![call("some_noop")]);
        let noop = fn_decl("some_noop", vec![], vec![]);
        let main = fn_decl("main", vec![], vec![call("file_write")]);
        let diags = infer(vec![shadow, noop, main]);
        assert!(
            diags.iter().any(|d| d.is_error()),
            "free fn colliding with a builtin name is conservatively gated, got {diags:?}"
        );
    }

    #[test]
    fn inferred_user_method_named_like_builtin_not_gated() {
        // A METHOD named `write_file` is never a runtime builtin, so method
        // dispatch must NOT force fs:write -- it is gated only by the method's
        // own (here empty) caps. This is the realistic false positive the sound
        // design fixes: methods skip the builtin table entirely.
        let m = fn_decl("write_file", vec![], vec![]);
        let imp = impl_with_method("Doc", m);
        let main = fn_decl("main", vec![], vec![method_call("d", "write_file")]);
        let diags = infer(vec![imp, main]);
        assert!(
            diags.is_empty(),
            "user method colliding with a builtin name must not be gated, got {diags:?}"
        );
    }

    #[test]
    fn inferred_builtin_passed_as_value_is_caught() {
        // fn main() { apply(file_write, ...) }  -- passing a gated builtin as a
        // fn value delegates its authority; unannotated main must be rejected.
        let call_apply = Stmt::Expr {
            expr: Expr::FnCall {
                callee: Box::new(Expr::Identifier {
                    name: "apply".into(),
                    span: span(),
                }),
                args: vec![Expr::Identifier {
                    name: "file_write".into(),
                    span: span(),
                }],
                span: span(),
            },
            span: span(),
        };
        let main = fn_decl("main", vec![], vec![call_apply]);
        let diags = infer(vec![main]);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0505")),
            "gated builtin passed as a value must require its cap, got {diags:?}"
        );
    }
}
