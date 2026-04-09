//! Ownership analysis pass — walks the AST and tracks ownership state.
//!
//! For each variable in each scope, tracks whether it is Owned, Moved,
//! Shared, PartiallyMoved, or Uninitialized. Reports errors when:
//! - A moved value is used.
//! - An uninitialized value is used.
//! - A value is moved in one branch but not another (conditional move).

use std::collections::{HashMap, HashSet};

use kryos_ast::{Block, Decl, Expr, Module, Stmt};
use kryos_errors::{Diagnostic, Span};

use crate::{ArcInsertion, ArcReason, OwnershipState};

/// Information tracked for each variable.
#[derive(Debug, Clone)]
pub struct VarInfo {
    pub state: OwnershipState,
    /// True if the variable is mutable (`let mut`).
    pub mutable: bool,
    /// True if the variable's type is a copy type.
    pub is_copy: bool,
    /// The span where this variable was declared.
    pub decl_span: Span,
    /// The span where this variable was moved (if moved).
    pub moved_span: Option<Span>,
    /// Set of field names that have been moved out (for partial moves).
    pub moved_fields: HashSet<String>,
}

/// A single lexical scope for ownership tracking.
#[derive(Debug, Clone)]
struct OwnershipScope {
    vars: HashMap<String, VarInfo>,
}

impl OwnershipScope {
    fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }
}

/// Set of known copy type names (primitives only).
/// Strings are NOT copy — they are heap-allocated and tracked by
/// ownership.  String *literals* are still considered copy via
/// `expr_is_copy` (they are interned/static data).
fn is_primitive_copy_type(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16" | "i32" | "i64" | "i128"
            | "u8" | "u16" | "u32" | "u64" | "u128"
            | "f32" | "f64"
            | "bool" | "char"
            | "usize" | "isize"
    )
}

/// The ownership analyzer — accumulates diagnostics and ARC insertion points.
pub struct OwnershipAnalyzer {
    scopes: Vec<OwnershipScope>,
    pub diagnostics: Vec<Diagnostic>,
    pub arc_insertions: Vec<ArcInsertion>,
    /// Structs annotated with @copy.
    copy_structs: HashSet<String>,
    /// Function name → whether return type is a primitive copy type.
    fn_copy_returns: HashMap<String, bool>,
}

impl Default for OwnershipAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl OwnershipAnalyzer {
    pub fn new() -> Self {
        Self {
            scopes: vec![OwnershipScope::new()],
            diagnostics: Vec::new(),
            arc_insertions: Vec::new(),
            copy_structs: HashSet::new(),
            fn_copy_returns: HashMap::new(),
        }
    }

    // ── Scope management ────────────────────────────────────────────

    fn push_scope(&mut self) {
        self.scopes.push(OwnershipScope::new());
    }

    fn pop_scope(&mut self) {
        debug_assert!(self.scopes.len() > 1, "cannot pop the global scope");
        self.scopes.pop();
    }

    /// Define a variable in the current scope.
    fn define_var(&mut self, name: String, info: VarInfo) {
        self.current_scope_mut().vars.insert(name, info);
    }

    /// Look up a variable's info, searching from innermost scope outward.
    fn lookup_var(&self, name: &str) -> Option<&VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.vars.get(name) {
                return Some(info);
            }
        }
        None
    }

    /// Mutably look up a variable's info, searching from innermost scope outward.
    fn lookup_var_mut(&mut self, name: &str) -> Option<&mut VarInfo> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.vars.get_mut(name) {
                return Some(info);
            }
        }
        None
    }

    fn current_scope_mut(&mut self) -> &mut OwnershipScope {
        self.scopes.last_mut().expect("at least one scope")
    }

    /// Snapshot variable states for the current scope stack (for branching).
    fn snapshot_states(&self) -> HashMap<String, OwnershipState> {
        let mut states = HashMap::new();
        for scope in &self.scopes {
            for (name, info) in &scope.vars {
                states.insert(name.clone(), info.state);
            }
        }
        states
    }

    /// Restore a variable's state (used after branch analysis).
    fn restore_state(&mut self, name: &str, state: OwnershipState) {
        if let Some(info) = self.lookup_var_mut(name) {
            info.state = state;
        }
    }

    // ── Type helpers ────────────────────────────────────────────────

    /// Check if a type annotation name indicates a copy type.
    fn is_copy_type_name(&self, name: &str) -> bool {
        is_primitive_copy_type(name) || self.copy_structs.contains(name)
    }

    /// Determine if an expression's inferred type is a copy type.
    /// Returns true when the result is provably a primitive copy type
    /// (i64, f64, bool, char, etc.), even through compound expressions
    /// like binary ops, function calls, match, if-else, and casts.
    fn expr_is_copy(&self, expr: &Expr) -> bool {
        match expr {
            // Literals are always copy.
            Expr::IntLiteral { .. }
            | Expr::FloatLiteral { .. }
            | Expr::BoolLiteral { .. }
            | Expr::CharLiteral { .. }
            | Expr::StringLiteral { .. } => true,

            // Identifiers: check the variable's is_copy flag.
            Expr::Identifier { name, .. } => {
                if let Some(info) = self.lookup_var(name) {
                    info.is_copy
                } else {
                    false
                }
            }

            // Binary ops on copy operands produce copy results
            // (arithmetic, comparison, logical ops on primitives).
            Expr::BinaryOp { left, right, .. } => {
                self.expr_is_copy(left) && self.expr_is_copy(right)
            }

            // Unary ops on a copy operand produce a copy result.
            Expr::UnaryOp { operand, .. } => self.expr_is_copy(operand),

            // Function calls: check builtins, then declared function return types.
            Expr::FnCall { callee, .. } => {
                if let Expr::Identifier { name, .. } = callee.as_ref() {
                    // Known builtins that return copy types.  String-returning
                    // builtins (to_string, substr, etc.) are copy because they
                    // produce fresh allocations the caller fully owns.
                    if matches!(
                        name.as_str(),
                        "len" | "parse_int" | "parse_float" | "time_now" | "abs"
                            | "min" | "max" | "floor" | "ceil" | "round"
                            | "sqrt" | "pow" | "log" | "sin" | "cos" | "tan"
                            | "char_code" | "int" | "float"
                            | "to_string" | "char_from" | "substr"
                            | "type_of" | "env_get" | "file_read"
                            | "buf_new" | "buf_len" | "buf_get_byte" | "buf_write_to_file"
                    ) {
                        return true;
                    }
                    // Check user-defined function return type annotations.
                    if let Some(&is_copy) = self.fn_copy_returns.get(name.as_str()) {
                        return is_copy;
                    }
                    false
                } else {
                    false
                }
            }

            // Method calls: check common methods that return primitives,
            // then fall back to the function return type registry.
            Expr::MethodCall { method, .. } => {
                // Well-known methods that always return copy types.
                if matches!(
                    method.as_str(),
                    "len" | "count" | "size" | "capacity"
                        | "is_empty" | "contains" | "starts_with" | "ends_with"
                        | "parse_int" | "parse_float"
                        | "abs" | "min" | "max" | "floor" | "ceil" | "round"
                        | "sqrt" | "pow" | "log" | "sin" | "cos" | "tan"
                        | "clone"
                ) {
                    return true;
                }
                // Check if the method was declared with a copy return type.
                if let Some(&is_copy) = self.fn_copy_returns.get(method.as_str()) {
                    return is_copy;
                }
                false
            }

            Expr::StaticMethodCall { type_name, method, .. } => {
                let mangled = format!("{type_name}__{method}");
                if let Some(&is_copy) = self.fn_copy_returns.get(mangled.as_str()) {
                    return is_copy;
                }
                false
            }

            // Match expression: copy if ALL arms produce copy values.
            Expr::MatchExpr { arms, .. } => {
                !arms.is_empty() && arms.iter().all(|arm| self.expr_is_copy(&arm.body))
            }

            // If expression: copy if both branches' last expressions are copy.
            Expr::IfExpr {
                then_branch,
                else_branch,
                ..
            } => {
                let then_copy = self.block_last_expr_is_copy(then_branch);
                let else_copy = else_branch
                    .as_ref()
                    .map(|blk| self.block_last_expr_is_copy(blk))
                    .unwrap_or(true);
                then_copy && else_copy
            }

            // Cast to a copy type produces a copy value.
            Expr::Cast { ty, .. } => self.is_type_expr_copy(ty),

            // Range expressions produce a copy handle (iterator over i64).
            Expr::RangeExpr { .. } => true,

            // Index access conservatively returns copy (array elements are i64).
            Expr::IndexAccess { .. } => true,

            // Block expression: copy if the last expression in the block is copy.
            Expr::Block { block, .. } => self.block_last_expr_is_copy(block),

            // Field access: copy if the object is a copy type (all fields of
            // a copy struct are themselves copy).
            Expr::FieldAccess { object, .. } => self.expr_is_copy(object),

            // Pipe expression: the result type is determined by the right-hand
            // side (the function receiving the piped value).
            Expr::PipeExpr { right, .. } => self.expr_is_copy(right),

            // Borrow/Deref: references to copy types yield copy values.
            Expr::Borrow { inner, .. } => self.expr_is_copy(inner),
            Expr::Deref { inner, .. } => self.expr_is_copy(inner),

            // Move of a copy type is still copy.
            Expr::MoveExpr { inner, .. } => self.expr_is_copy(inner),

            _ => false,
        }
    }

    /// Check if the last expression of a block is a copy type.
    /// Blocks produce their last statement's expression value.
    fn block_last_expr_is_copy(&self, block: &Block) -> bool {
        if let Some(last) = block.stmts.last() {
            match last {
                Stmt::Expr { expr, .. } => self.expr_is_copy(expr),
                Stmt::Return { value: Some(expr), .. } => self.expr_is_copy(expr),
                _ => false,
            }
        } else {
            false
        }
    }

    // ── Error reporting ─────────────────────────────────────────────

    fn error_use_after_move(&mut self, name: &str, use_span: Span, moved_span: Span) {
        self.diagnostics.push(
            Diagnostic::error(format!("use of moved value: `{name}`"))
                .with_code("E0382")
                .with_label(use_span, format!("`{name}` used here after move"))
                .with_label(moved_span, format!("`{name}` moved here"))
                .with_note("values are moved on assignment or when passed to functions"),
        );
    }

    fn error_use_uninitialized(&mut self, name: &str, use_span: Span, decl_span: Span) {
        self.diagnostics.push(
            Diagnostic::error(format!("use of uninitialized variable: `{name}`"))
                .with_code("E0381")
                .with_label(use_span, format!("`{name}` used here but not initialized"))
                .with_label(decl_span, format!("`{name}` declared here")),
        );
    }

    fn error_partial_move(&mut self, name: &str, field: &str, use_span: Span) {
        self.diagnostics.push(
            Diagnostic::error(format!(
                "use of partially moved value: `{name}` (field `{field}` was moved)"
            ))
            .with_code("E0382")
            .with_label(use_span, format!("`{name}.{field}` was previously moved")),
        );
    }

    fn warning_conditional_move(&mut self, name: &str, span: Span) {
        self.diagnostics.push(
            Diagnostic::warning(format!(
                "variable `{name}` is moved in one branch but not another"
            ))
            .with_code("W0383")
            .with_label(span, "conditional move detected")
            .with_note("consider using `shared` or restructuring to avoid partial ownership"),
        );
    }

    // ── Analysis entry point ────────────────────────────────────────

    pub fn analyze_module(&mut self, module: &Module) {
        // First pass: collect @copy struct annotations and function return types.
        for decl in &module.declarations {
            match decl {
                Decl::Struct {
                    name, annotations, ..
                } => {
                    if annotations.iter().any(|a| a.name == "copy") {
                        self.copy_structs.insert(name.clone());
                    }
                }
                Decl::Function {
                    name, ret_ty, ..
                } => {
                    let is_copy_ret = ret_ty
                        .as_ref()
                        .map(|t| self.is_type_expr_copy(t))
                        .unwrap_or(false);
                    self.fn_copy_returns.insert(name.clone(), is_copy_ret);
                }
                _ => {}
            }
        }

        // Also collect methods from impl blocks.
        for decl in &module.declarations {
            if let Decl::Impl { methods, .. } = decl {
                for method in methods {
                    if let Decl::Function { name, ret_ty, .. } = method {
                        let is_copy_ret = ret_ty
                            .as_ref()
                            .map(|t| self.is_type_expr_copy(t))
                            .unwrap_or(false);
                        self.fn_copy_returns.insert(name.clone(), is_copy_ret);
                    }
                }
            }
        }

        // Second pass: analyze declarations.
        for decl in &module.declarations {
            self.analyze_decl(decl);
        }
    }

    fn analyze_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Function {
                params,
                body: Some(body),
                ..
            } => {
                self.push_scope();
                // Function parameters are owned by the function.
                for param in params {
                    let is_copy = param
                        .ty
                        .as_ref()
                        .map(|t| self.is_type_expr_copy(t))
                        .unwrap_or(false);
                    self.define_var(
                        param.name.clone(),
                        VarInfo {
                            state: OwnershipState::Owned,
                            mutable: false,
                            is_copy,
                            decl_span: param.span,
                            moved_span: None,
                            moved_fields: HashSet::new(),
                        },
                    );
                }
                self.analyze_block(body);
                self.pop_scope();
            }
            Decl::Actor {
                handlers,
                state_fields,
                span,
                ..
            } => {
                // Actor state fields shared across handlers → ARC boundary.
                for field in state_fields {
                    self.arc_insertions.push(ArcInsertion {
                        variable: field.name.clone(),
                        scope_span: *span,
                        reason: ArcReason::ActorBoundary,
                    });
                }
                for handler in handlers {
                    self.push_scope();
                    for param in &handler.params {
                        let is_copy = param
                            .ty
                            .as_ref()
                            .map(|t| self.is_type_expr_copy(t))
                            .unwrap_or(false);
                        self.define_var(
                            param.name.clone(),
                            VarInfo {
                                state: OwnershipState::Owned,
                                mutable: false,
                                is_copy,
                                decl_span: param.span,
                                moved_span: None,
                                moved_fields: HashSet::new(),
                            },
                        );
                    }
                    self.analyze_block(&handler.body);
                    self.pop_scope();
                }
            }
            Decl::Impl { methods, .. } => {
                for method in methods {
                    self.analyze_decl(method);
                }
            }
            // Other declarations don't need ownership analysis.
            _ => {}
        }
    }

    fn analyze_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.analyze_stmt(stmt);
        }
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                mutable,
                ty,
                value,
                span,
                ..
            } => {
                if let Some(init_expr) = value {
                    // Check if the initializer is a `shared` expression.
                    if let Expr::SharedExpr { inner, span: sh_span } = init_expr {
                        // Move the inner value into ARC.
                        self.analyze_expr_move(inner);
                        self.arc_insertions.push(ArcInsertion {
                            variable: name.clone(),
                            scope_span: *sh_span,
                            reason: ArcReason::ExplicitShared,
                        });
                        self.define_var(
                            name.clone(),
                            VarInfo {
                                state: OwnershipState::Shared,
                                mutable: *mutable,
                                is_copy: false,
                                decl_span: *span,
                                moved_span: None,
                                moved_fields: HashSet::new(),
                            },
                        );
                    } else if let Expr::WeakExpr { inner, .. } = init_expr {
                        // Weak reference: doesn't move the source.
                        self.analyze_expr_use(inner);
                        self.define_var(
                            name.clone(),
                            VarInfo {
                                state: OwnershipState::Owned,
                                mutable: *mutable,
                                is_copy: false,
                                decl_span: *span,
                                moved_span: None,
                                moved_fields: HashSet::new(),
                            },
                        );
                    } else {
                        // Determine if this is a copy.
                        let is_copy = self.expr_is_copy(init_expr)
                            || ty
                                .as_ref()
                                .map(|t| self.is_type_expr_copy(t))
                                .unwrap_or(false);

                        if is_copy {
                            // Copy: source remains owned.
                            self.analyze_expr_use(init_expr);
                        } else {
                            // Move: source becomes Moved.
                            self.analyze_expr_move(init_expr);
                        }

                        self.define_var(
                            name.clone(),
                            VarInfo {
                                state: OwnershipState::Owned,
                                mutable: *mutable,
                                is_copy,
                                decl_span: *span,
                                moved_span: None,
                                moved_fields: HashSet::new(),
                            },
                        );
                    }
                } else {
                    // Declared without initializer → Uninitialized.
                    let is_copy = ty
                        .as_ref()
                        .map(|t| self.is_type_expr_copy(t))
                        .unwrap_or(false);
                    self.define_var(
                        name.clone(),
                        VarInfo {
                            state: OwnershipState::Uninitialized,
                            mutable: *mutable,
                            is_copy,
                            decl_span: *span,
                            moved_span: None,
                            moved_fields: HashSet::new(),
                        },
                    );
                }
            }

            Stmt::Assign { target, value, span, .. } => {
                // Analyze the RHS value (move/copy).
                let rhs_copy = self.expr_is_copy(value);
                if rhs_copy {
                    self.analyze_expr_use(value);
                } else {
                    self.analyze_expr_move(value);
                }

                // Assignment to a variable resets its state to Owned.
                if let Expr::Identifier { name, .. } = target {
                    if let Some(info) = self.lookup_var_mut(name) {
                        info.state = OwnershipState::Owned;
                        info.moved_span = None;
                        info.moved_fields.clear();
                    }
                } else if let Expr::FieldAccess { object, .. } = target {
                    // Field assignment: just check the object is usable.
                    self.analyze_expr_use(object);
                } else {
                    self.analyze_expr_use(target);
                }
                let _ = span; // used by parent diagnostics
            }

            Stmt::Expr { expr, .. } => {
                self.analyze_expr_use(expr);
            }

            Stmt::Return { value, .. } => {
                if let Some(expr) = value {
                    // Return moves the value out of the function.
                    self.analyze_expr_move(expr);
                }
            }

            Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                span,
            } => {
                self.analyze_expr_use(condition);

                // Snapshot state before branching.
                let before = self.snapshot_states();

                // Analyze then branch.
                self.push_scope();
                self.analyze_block(then_block);
                self.pop_scope();
                let after_then = self.snapshot_states();

                // Restore and analyze elif branches.
                let mut branch_states: Vec<HashMap<String, OwnershipState>> = vec![after_then];
                for (elif_cond, elif_block) in elif_clauses {
                    // Restore to pre-branch state.
                    for (name, state) in &before {
                        self.restore_state(name, *state);
                    }
                    self.analyze_expr_use(elif_cond);
                    self.push_scope();
                    self.analyze_block(elif_block);
                    self.pop_scope();
                    branch_states.push(self.snapshot_states());
                }

                // Restore and analyze else branch.
                if let Some(else_blk) = else_block {
                    for (name, state) in &before {
                        self.restore_state(name, *state);
                    }
                    self.push_scope();
                    self.analyze_block(else_blk);
                    self.pop_scope();
                    branch_states.push(self.snapshot_states());
                }

                // Check for conditional moves: if a variable is Moved in some
                // branches but not all, that's a warning.
                if else_block.is_some() {
                    // Only check when all paths are covered.
                    for (name, pre_state) in &before {
                        if *pre_state != OwnershipState::Owned {
                            continue;
                        }
                        let moved_in_some = branch_states
                            .iter()
                            .any(|s| s.get(name) == Some(&OwnershipState::Moved));
                        let owned_in_some = branch_states
                            .iter()
                            .any(|s| s.get(name) == Some(&OwnershipState::Owned));
                        if moved_in_some && owned_in_some {
                            self.warning_conditional_move(name, *span);
                        }
                        // If moved in ALL branches, mark as moved.
                        let moved_in_all = branch_states
                            .iter()
                            .all(|s| s.get(name) == Some(&OwnershipState::Moved));
                        if moved_in_all {
                            self.restore_state(name, OwnershipState::Moved);
                        } else if moved_in_some {
                            // Conservatively keep as owned but warn.
                            self.restore_state(name, OwnershipState::Owned);
                        }
                    }
                } else {
                    // No else: if then branch moves something, restore to pre-branch.
                    for (name, state) in &before {
                        self.restore_state(name, *state);
                    }
                }
            }

            Stmt::For {
                iterable, body, ..
            } => {
                self.analyze_expr_use(iterable);
                self.push_scope();
                self.analyze_block(body);
                self.pop_scope();
            }

            Stmt::While {
                condition, body, ..
            } => {
                self.analyze_expr_use(condition);
                self.push_scope();
                self.analyze_block(body);
                self.pop_scope();
            }

            Stmt::Spawn { expr, .. } => {
                // Spawn moves the expression into a new task.
                self.analyze_expr_move(expr);
            }

            Stmt::Select { branches, .. } => {
                for branch in branches {
                    self.analyze_expr_use(&branch.channel);
                    self.push_scope();
                    self.analyze_block(&branch.body);
                    self.pop_scope();
                }
            }

            Stmt::TryCatch {
                try_block,
                catch_name,
                catch_block,
                span,
            } => {
                self.push_scope();
                self.analyze_block(try_block);
                self.pop_scope();
                self.push_scope();
                self.define_var(
                    catch_name.clone(),
                    VarInfo {
                        state: OwnershipState::Owned,
                        mutable: false,
                        is_copy: false,
                        decl_span: *span,
                        moved_span: None,
                        moved_fields: HashSet::new(),
                    },
                );
                self.analyze_block(catch_block);
                self.pop_scope();
            }

            Stmt::Throw { expr, .. } => {
                self.analyze_expr_move(expr);
            }

            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
    }

    /// Analyze an expression that is being used (read, not moved).
    /// Checks that the value hasn't been moved or is uninitialized.
    fn analyze_expr_use(&mut self, expr: &Expr) {
        match expr {
            Expr::Identifier { name, span } => {
                self.check_usable(name, *span);
            }
            Expr::FieldAccess { object, field, span } => {
                // Check for partial move on the field.
                if let Expr::Identifier { name, .. } = object.as_ref() {
                    if let Some(info) = self.lookup_var(name) {
                        if info.moved_fields.contains(field) {
                            self.error_partial_move(name, field, *span);
                            return;
                        }
                    }
                }
                self.analyze_expr_use(object);
            }
            Expr::FnCall { callee, args, span: _ } => {
                self.analyze_expr_use(callee);
                // Check for `send(channel, value)` pattern — moves the value.
                if let Expr::Identifier { name, .. } = callee.as_ref() {
                    if name == "send" && args.len() == 2 {
                        self.analyze_expr_use(&args[0]); // channel is used, not moved
                        self.analyze_expr_move(&args[1]); // value is moved
                        return;
                    }
                    // push(arr, val) and pop(arr): the array is borrowed
                    // (mutated in-place), not moved. Only the value is moved.
                    if name == "push" && args.len() == 2 {
                        self.analyze_expr_use(&args[0]); // array is used, not moved
                        if self.expr_is_copy(&args[1]) {
                            self.analyze_expr_use(&args[1]);
                        } else {
                            self.analyze_expr_move(&args[1]); // value is moved into array
                        }
                        return;
                    }
                    if name == "pop" && args.len() == 1 {
                        self.analyze_expr_use(&args[0]); // array is used, not moved
                        return;
                    }
                }
                // Regular function call: each argument is moved (unless copy).
                for arg in args {
                    if self.expr_is_copy(arg) {
                        self.analyze_expr_use(arg);
                    } else {
                        self.analyze_expr_move(arg);
                    }
                }
            }
            Expr::MethodCall { object, args, .. } => {
                self.analyze_expr_use(object);
                for arg in args {
                    if self.expr_is_copy(arg) {
                        self.analyze_expr_use(arg);
                    } else {
                        self.analyze_expr_move(arg);
                    }
                }
            }
            Expr::StaticMethodCall { args, .. } => {
                for arg in args {
                    if self.expr_is_copy(arg) {
                        self.analyze_expr_use(arg);
                    } else {
                        self.analyze_expr_move(arg);
                    }
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                self.analyze_expr_use(left);
                self.analyze_expr_use(right);
            }
            Expr::UnaryOp { operand, .. } => {
                self.analyze_expr_use(operand);
            }
            Expr::IndexAccess { object, index, .. } => {
                self.analyze_expr_use(object);
                self.analyze_expr_use(index);
            }
            Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => {
                for elem in elements {
                    self.analyze_expr_use(elem);
                }
            }
            Expr::MapLiteral { entries, .. } => {
                for (k, v) in entries {
                    self.analyze_expr_use(k);
                    self.analyze_expr_use(v);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, val) in fields {
                    self.analyze_expr_move(val);
                }
            }
            Expr::Lambda {
                params, body, span, ..
            } => {
                self.analyze_lambda(params, body, *span);
            }
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.analyze_expr_use(condition);
                self.push_scope();
                self.analyze_block(then_branch);
                self.pop_scope();
                if let Some(else_blk) = else_branch {
                    self.push_scope();
                    self.analyze_block(else_blk);
                    self.pop_scope();
                }
            }
            Expr::MatchExpr { subject, arms, .. } => {
                self.analyze_expr_move(subject);
                for arm in arms {
                    self.push_scope();
                    if let Some(guard) = &arm.guard {
                        self.analyze_expr_use(guard);
                    }
                    self.analyze_expr_use(&arm.body);
                    self.pop_scope();
                }
            }
            Expr::Borrow { inner, .. } => {
                // Borrowing doesn't move the source — just reads it.
                self.analyze_expr_use(inner);
            }
            Expr::Deref { inner, .. } => {
                // Dereferencing reads the reference.
                self.analyze_expr_use(inner);
            }
            Expr::SharedExpr { inner, span } => {
                self.analyze_expr_move(inner);
                // Standalone shared expression — record ARC.
                if let Expr::Identifier { name, .. } = inner.as_ref() {
                    self.arc_insertions.push(ArcInsertion {
                        variable: name.clone(),
                        scope_span: *span,
                        reason: ArcReason::ExplicitShared,
                    });
                }
            }
            Expr::MoveExpr { inner, .. } => {
                self.analyze_expr_move(inner);
            }
            Expr::WeakExpr { inner, .. } => {
                // Weak reference doesn't transfer ownership.
                self.analyze_expr_use(inner);
            }
            Expr::Cast { expr, .. } => {
                self.analyze_expr_use(expr);
            }
            Expr::Block { block, .. } => {
                self.push_scope();
                self.analyze_block(block);
                self.pop_scope();
            }
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    if let kryos_ast::StringPart::Expr(e) = part {
                        self.analyze_expr_use(e);
                    }
                }
            }
            Expr::PipeExpr { left, right, .. } => {
                self.analyze_expr_use(left);
                self.analyze_expr_use(right);
            }
            Expr::RangeExpr { start, end, .. } => {
                if let Some(s) = start {
                    self.analyze_expr_use(s);
                }
                if let Some(e) = end {
                    self.analyze_expr_use(e);
                }
            }
            Expr::ComptimeBlock { body, .. } | Expr::QuantumBlock { body, .. } => {
                self.push_scope();
                self.analyze_block(body);
                self.pop_scope();
            }
            // Literals don't involve ownership.
            Expr::IntLiteral { .. }
            | Expr::FloatLiteral { .. }
            | Expr::StringLiteral { .. }
            | Expr::CharLiteral { .. }
            | Expr::BoolLiteral { .. }
            | Expr::NoneLiteral { .. } => {}
        }
    }

    /// Analyze an expression that is being moved (consumed).
    /// Marks the source variable as Moved.
    fn analyze_expr_move(&mut self, expr: &Expr) {
        match expr {
            Expr::Identifier { name, span } => {
                self.check_usable(name, *span);
                // Mark as moved (unless it's a copy type).
                if let Some(info) = self.lookup_var(name) {
                    if info.is_copy || info.state == OwnershipState::Shared {
                        return; // Copy types and shared values don't move.
                    }
                }
                if let Some(info) = self.lookup_var_mut(name) {
                    info.state = OwnershipState::Moved;
                    info.moved_span = Some(*span);
                }
            }
            Expr::FieldAccess { object, field, span } => {
                // Partial move of a struct field.
                if let Expr::Identifier { name, .. } = object.as_ref() {
                    self.check_usable(name, *span);
                    if let Some(info) = self.lookup_var_mut(name) {
                        info.moved_fields.insert(field.clone());
                        if info.state == OwnershipState::Owned {
                            info.state = OwnershipState::PartiallyMoved;
                        }
                    }
                } else {
                    self.analyze_expr_use(object);
                }
            }
            // For complex expressions, just analyze their sub-expressions.
            _ => {
                self.analyze_expr_use(expr);
            }
        }
    }

    /// Check that a variable is usable (not moved, not uninitialized).
    fn check_usable(&mut self, name: &str, span: Span) {
        if let Some(info) = self.lookup_var(name) {
            match info.state {
                OwnershipState::Moved => {
                    let moved_span = info.moved_span.unwrap_or(info.decl_span);
                    self.error_use_after_move(name, span, moved_span);
                }
                OwnershipState::Uninitialized => {
                    let decl_span = info.decl_span;
                    self.error_use_uninitialized(name, span, decl_span);
                }
                OwnershipState::PartiallyMoved => {
                    // Using a partially moved struct is allowed (for remaining fields).
                    // Errors are raised per-field in field access analysis.
                }
                OwnershipState::Owned | OwnershipState::Shared => {
                    // OK — variable is usable.
                }
            }
        }
        // Unknown variables are not our responsibility — the type checker handles those.
    }

    /// Analyze a lambda/closure: detect captures of mutable outer variables.
    fn analyze_lambda(&mut self, params: &[kryos_ast::Param], body: &Expr, lambda_span: Span) {
        // Collect all identifiers used in the lambda body that are not parameters.
        let param_names: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
        let mut captured = Vec::new();
        self.collect_captures(body, &param_names, &mut captured);

        // Check for mutable captures → ARC insertion needed.
        for cap_name in &captured {
            if let Some(info) = self.lookup_var(cap_name) {
                if info.mutable {
                    self.arc_insertions.push(ArcInsertion {
                        variable: cap_name.to_string(),
                        scope_span: lambda_span,
                        reason: ArcReason::ClosureCaptureMut,
                    });
                }
                // Check the captured variable is usable.
                self.check_usable(cap_name, lambda_span);
            }
        }

        // Analyze the body in a new scope with params defined.
        self.push_scope();
        for param in params {
            self.define_var(
                param.name.clone(),
                VarInfo {
                    state: OwnershipState::Owned,
                    mutable: false,
                    is_copy: param
                        .ty
                        .as_ref()
                        .map(|t| self.is_type_expr_copy(t))
                        .unwrap_or(false),
                    decl_span: param.span,
                    moved_span: None,
                    moved_fields: HashSet::new(),
                },
            );
        }
        // Lambda body is an expression.
        self.analyze_expr_use(body);
        self.pop_scope();
    }

    /// Recursively collect identifiers from an expression that are not in `locals`.
    fn collect_captures<'a>(
        &self,
        expr: &'a Expr,
        locals: &HashSet<&str>,
        out: &mut Vec<&'a str>,
    ) {
        match expr {
            Expr::Identifier { name, .. } => {
                if !locals.contains(name.as_str()) && self.lookup_var(name).is_some() {
                    out.push(name);
                }
            }
            Expr::BinaryOp { left, right, .. } | Expr::PipeExpr { left, right, .. } => {
                self.collect_captures(left, locals, out);
                self.collect_captures(right, locals, out);
            }
            Expr::UnaryOp { operand, .. } => {
                self.collect_captures(operand, locals, out);
            }
            Expr::FnCall { callee, args, .. } => {
                self.collect_captures(callee, locals, out);
                for arg in args {
                    self.collect_captures(arg, locals, out);
                }
            }
            Expr::MethodCall { object, args, .. } => {
                self.collect_captures(object, locals, out);
                for arg in args {
                    self.collect_captures(arg, locals, out);
                }
            }
            Expr::StaticMethodCall { args, .. } => {
                for arg in args {
                    self.collect_captures(arg, locals, out);
                }
            }
            Expr::FieldAccess { object, .. } => {
                self.collect_captures(object, locals, out);
            }
            Expr::IndexAccess { object, index, .. } => {
                self.collect_captures(object, locals, out);
                self.collect_captures(index, locals, out);
            }
            Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => {
                for e in elements {
                    self.collect_captures(e, locals, out);
                }
            }
            Expr::MapLiteral { entries, .. } => {
                for (k, v) in entries {
                    self.collect_captures(k, locals, out);
                    self.collect_captures(v, locals, out);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, v) in fields {
                    self.collect_captures(v, locals, out);
                }
            }
            Expr::IfExpr { condition, then_branch, else_branch, .. } => {
                self.collect_captures(condition, locals, out);
                for stmt in &then_branch.stmts {
                    self.collect_captures_stmt(stmt, locals, out);
                }
                if let Some(blk) = else_branch {
                    for stmt in &blk.stmts {
                        self.collect_captures_stmt(stmt, locals, out);
                    }
                }
            }
            Expr::SharedExpr { inner, .. }
            | Expr::MoveExpr { inner, .. }
            | Expr::WeakExpr { inner, .. }
            | Expr::Cast { expr: inner, .. } => {
                self.collect_captures(inner, locals, out);
            }
            Expr::Block { block, .. } => {
                for stmt in &block.stmts {
                    self.collect_captures_stmt(stmt, locals, out);
                }
            }
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    if let kryos_ast::StringPart::Expr(e) = part {
                        self.collect_captures(e, locals, out);
                    }
                }
            }
            Expr::RangeExpr { start, end, .. } => {
                if let Some(s) = start {
                    self.collect_captures(s, locals, out);
                }
                if let Some(e) = end {
                    self.collect_captures(e, locals, out);
                }
            }
            Expr::MatchExpr { subject, arms, .. } => {
                self.collect_captures(subject, locals, out);
                for arm in arms {
                    if let Some(g) = &arm.guard {
                        self.collect_captures(g, locals, out);
                    }
                    self.collect_captures(&arm.body, locals, out);
                }
            }
            Expr::Lambda { body, .. } => {
                // Nested lambdas: their captures are our captures too.
                self.collect_captures(body, locals, out);
            }
            Expr::ComptimeBlock { body, .. } | Expr::QuantumBlock { body, .. } => {
                for stmt in &body.stmts {
                    self.collect_captures_stmt(stmt, locals, out);
                }
            }
            // Literals have no captures.
            _ => {}
        }
    }

    fn collect_captures_stmt<'a>(
        &self,
        stmt: &'a Stmt,
        locals: &HashSet<&str>,
        out: &mut Vec<&'a str>,
    ) {
        match stmt {
            Stmt::Let { value, .. } => {
                if let Some(v) = value {
                    self.collect_captures(v, locals, out);
                }
            }
            Stmt::Assign { target, value, .. } => {
                self.collect_captures(target, locals, out);
                self.collect_captures(value, locals, out);
            }
            Stmt::Expr { expr, .. } | Stmt::Spawn { expr, .. } | Stmt::Throw { expr, .. } => {
                self.collect_captures(expr, locals, out);
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.collect_captures(v, locals, out);
                }
            }
            Stmt::If { condition, then_block, elif_clauses, else_block, .. } => {
                self.collect_captures(condition, locals, out);
                for s in &then_block.stmts {
                    self.collect_captures_stmt(s, locals, out);
                }
                for (c, b) in elif_clauses {
                    self.collect_captures(c, locals, out);
                    for s in &b.stmts {
                        self.collect_captures_stmt(s, locals, out);
                    }
                }
                if let Some(blk) = else_block {
                    for s in &blk.stmts {
                        self.collect_captures_stmt(s, locals, out);
                    }
                }
            }
            Stmt::For { iterable, body, .. } => {
                self.collect_captures(iterable, locals, out);
                for s in &body.stmts {
                    self.collect_captures_stmt(s, locals, out);
                }
            }
            Stmt::While { condition, body, .. } => {
                self.collect_captures(condition, locals, out);
                for s in &body.stmts {
                    self.collect_captures_stmt(s, locals, out);
                }
            }
            Stmt::Select { branches, .. } => {
                for branch in branches {
                    self.collect_captures(&branch.channel, locals, out);
                    for s in &branch.body.stmts {
                        self.collect_captures_stmt(s, locals, out);
                    }
                }
            }
            Stmt::TryCatch { try_block, catch_block, .. } => {
                for s in &try_block.stmts {
                    self.collect_captures_stmt(s, locals, out);
                }
                for s in &catch_block.stmts {
                    self.collect_captures_stmt(s, locals, out);
                }
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
    }

    /// Check if a TypeExpr represents a copy type.
    fn is_type_expr_copy(&self, ty: &kryos_ast::TypeExpr) -> bool {
        match ty {
            kryos_ast::TypeExpr::Simple { name, .. } => self.is_copy_type_name(name),
            kryos_ast::TypeExpr::Tuple { elements, .. } => {
                elements.iter().all(|e| self.is_type_expr_copy(e))
            }
            // References, shared, weak are not copy.
            _ => false,
        }
    }
}
