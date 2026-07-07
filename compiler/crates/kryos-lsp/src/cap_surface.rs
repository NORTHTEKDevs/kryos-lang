//! Per-function capability surface computation for the LSP.
//!
//! Walks a function body and collects the capability set it requires, reusing
//! the compiler's capability model (`required_capability_for_builtin` /
//! `required_capability_for_path`). This mirrors the call traversal in
//! kryos-capabilities' checker, so the LSP surfaces the same per-function
//! capability set the `--strict-capabilities` driver would compute — without
//! re-running the full checker or editing the kryos-capabilities crate.
//!
//! Scope: top-level builtin/qualified-path usage inside a single function body.
//! Cross-function propagation and imports are intentionally out of scope for
//! the MVP (see SPEC-31).

use kryos_ast::{Annotation, Block, Decl, Expr, Module, Stmt, StringPart};
use kryos_capabilities::model::required_capability_for_path;
use kryos_capabilities::{required_capability_for_builtin, CapabilitySet};
use kryos_errors::{Diagnostic, Span};

/// Compute the capability set a function body requires from its direct
/// builtin / qualified-stdlib-path calls.
pub fn function_caps(body: &Block) -> CapabilitySet {
    let mut set = CapabilitySet::empty();
    walk_block(body, &mut set);
    set
}

/// Whether a declaration carries an explicit `@capabilities(...)` annotation.
pub fn has_capabilities_annotation(annotations: &[Annotation]) -> bool {
    annotations.iter().any(|a| a.name == "capabilities")
}

/// Render a capability set as a sorted, comma-separated string (e.g. `io, net`).
pub fn caps_label(set: &CapabilitySet) -> String {
    let mut names: Vec<String> = set.iter().map(|c| c.to_string()).collect();
    names.sort();
    names.join(", ")
}

/// Strict-mode diagnostics: warn on every top-level, unannotated function whose
/// computed capability surface is non-empty — i.e. the functions that
/// `--strict-capabilities` would reject for using a gated builtin without an
/// `@capabilities(...)` annotation.
pub fn strict_mode_warnings(module: &Module) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for decl in &module.declarations {
        if let Decl::Function {
            name,
            annotations,
            body: Some(body),
            span,
            ..
        } = decl
        {
            if has_capabilities_annotation(annotations) {
                continue;
            }
            let caps = function_caps(body);
            if caps.is_empty() {
                continue;
            }
            let label = caps_label(&caps);
            // Underline just the signature region (decl start → body `{`).
            let sig_end = body.span.start.max(span.start);
            let sig_span = Span::new(span.file_id, span.start, sig_end);
            out.push(
                Diagnostic::warning(format!(
                    "function `{name}` uses capabilities [{label}] but has no \
                     `@capabilities(...)` annotation; `--strict-capabilities` would reject it"
                ))
                .with_label(sig_span, format!("requires [{label}]"))
                .with_note(format!(
                    "add `@capabilities({label})` to the function to pass strict capability mode"
                ))
                .with_code("W0505"),
            );
        }
    }
    out
}

fn walk_block(block: &Block, set: &mut CapabilitySet) {
    for stmt in &block.stmts {
        walk_stmt(stmt, set);
    }
}

fn walk_stmt(stmt: &Stmt, set: &mut CapabilitySet) {
    match stmt {
        Stmt::Let { value, .. } => {
            if let Some(expr) = value {
                walk_expr(expr, set);
            }
        }
        Stmt::Assign { value, target, .. } => {
            walk_expr(target, set);
            walk_expr(value, set);
        }
        Stmt::Return { value, .. } => {
            if let Some(expr) = value {
                walk_expr(expr, set);
            }
        }
        Stmt::If {
            condition,
            then_block,
            elif_clauses,
            else_block,
            ..
        } => {
            walk_expr(condition, set);
            walk_block(then_block, set);
            for (cond, block) in elif_clauses {
                walk_expr(cond, set);
                walk_block(block, set);
            }
            if let Some(block) = else_block {
                walk_block(block, set);
            }
        }
        Stmt::For { iterable, body, .. } => {
            walk_expr(iterable, set);
            walk_block(body, set);
        }
        Stmt::While {
            condition, body, ..
        } => {
            walk_expr(condition, set);
            walk_block(body, set);
        }
        Stmt::Expr { expr, .. } => walk_expr(expr, set),
        Stmt::Spawn { expr, .. } => walk_expr(expr, set),
        Stmt::TryCatch {
            try_block,
            catch_block,
            ..
        } => {
            walk_block(try_block, set);
            walk_block(catch_block, set);
        }
        Stmt::Throw { expr, .. } => walk_expr(expr, set),
        Stmt::Select { branches, .. } => {
            for branch in branches {
                walk_expr(&branch.channel, set);
                walk_block(&branch.body, set);
            }
        }
        Stmt::DenyBlock { body, .. } => walk_block(body, set),
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
    }
}

fn walk_expr(expr: &Expr, set: &mut CapabilitySet) {
    match expr {
        Expr::FnCall { callee, args, .. } => {
            let segments = resolve_path(callee);
            if let Some(cap) = required_capability_for_path(&segments) {
                set.insert(cap);
            }
            if segments.len() == 1 {
                if let Some(cap) = required_capability_for_builtin(&segments[0]) {
                    set.insert(cap);
                }
            }
            walk_expr(callee, set);
            for arg in args {
                walk_expr(arg, set);
            }
        }
        Expr::MethodCall { object, args, .. } => {
            walk_expr(object, set);
            for arg in args {
                walk_expr(arg, set);
            }
        }
        Expr::StaticMethodCall { args, .. } => {
            for arg in args {
                walk_expr(arg, set);
            }
        }
        Expr::FieldAccess { object, .. } => walk_expr(object, set),
        Expr::IndexAccess { object, index, .. } => {
            walk_expr(object, set);
            walk_expr(index, set);
        }
        Expr::BinaryOp { left, right, .. } => {
            walk_expr(left, set);
            walk_expr(right, set);
        }
        Expr::UnaryOp { operand, .. } => walk_expr(operand, set),
        Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => {
            for elem in elements {
                walk_expr(elem, set);
            }
        }
        Expr::MapLiteral { entries, .. } => {
            for (k, v) in entries {
                walk_expr(k, set);
                walk_expr(v, set);
            }
        }
        Expr::StructLiteral { fields, .. } => {
            for (_, expr) in fields {
                walk_expr(expr, set);
            }
        }
        Expr::Lambda { body, .. } => walk_expr(body, set),
        Expr::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            walk_expr(condition, set);
            walk_block(then_branch, set);
            if let Some(block) = else_branch {
                walk_block(block, set);
            }
        }
        Expr::MatchExpr { subject, arms, .. } => {
            walk_expr(subject, set);
            for arm in arms {
                walk_expr(&arm.body, set);
                if let Some(guard) = &arm.guard {
                    walk_expr(guard, set);
                }
            }
        }
        Expr::PipeExpr { left, right, .. } => {
            walk_expr(left, set);
            walk_expr(right, set);
        }
        Expr::Borrow { inner, .. }
        | Expr::Deref { inner, .. }
        | Expr::SharedExpr { inner, .. }
        | Expr::MoveExpr { inner, .. }
        | Expr::WeakExpr { inner, .. } => walk_expr(inner, set),
        Expr::ComptimeBlock { body, .. }
        | Expr::QuantumBlock { body, .. }
        | Expr::UnsafeBlock { body, .. } => walk_block(body, set),
        Expr::Cast { expr, .. } => walk_expr(expr, set),
        Expr::Block { block, .. } => walk_block(block, set),
        Expr::Await { value, .. } => walk_expr(value, set),
        Expr::RangeExpr { start, end, .. } => {
            if let Some(s) = start {
                walk_expr(s, set);
            }
            if let Some(e) = end {
                walk_expr(e, set);
            }
        }
        Expr::InterpolatedString { parts, .. } => {
            for part in parts {
                if let StringPart::Expr(e) = part {
                    walk_expr(e, set);
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

/// Resolve a callee expression into dotted path segments.
/// `std.net.connect(...)` → `["std", "net", "connect"]`.
fn resolve_path(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Identifier { name, .. } => vec![name.clone()],
        Expr::FieldAccess { object, field, .. } => {
            let mut segments = resolve_path(object);
            segments.push(field.clone());
            segments
        }
        _ => Vec::new(),
    }
}
