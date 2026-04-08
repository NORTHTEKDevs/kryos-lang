//! Type checking pass — walks the AST and validates types.
//!
//! Resolves `TypeExpr` → `Type`, infers types for unannotated let bindings,
//! checks function bodies, binary ops, calls, field access, etc.
//! Reports mismatches as `Diagnostic` errors.

use kryos_ast::{
    BinOp, Block, Decl, Expr, Module, Pattern, Stmt, TypeExpr, UnOp,
};
use kryos_errors::{Diagnostic, Span};

use crate::env::{EnumDef, FunctionSig, StructDef, TypeEnv};
use crate::infer::InferenceEngine;
use crate::ty::Type;

/// The main type checker.
pub struct TypeChecker {
    pub env: TypeEnv,
    pub engine: InferenceEngine,
    pub diagnostics: Vec<Diagnostic>,
    /// The expected return type of the function currently being checked.
    current_return_type: Option<Type>,
    /// Functions marked with @deprecated — emit warnings on call.
    deprecated_functions: std::collections::HashSet<String>,
    /// Functions marked with @pure — cannot call non-pure or do I/O.
    pure_functions: std::collections::HashSet<String>,
    /// Whether we are currently inside a @pure function body.
    in_pure_function: bool,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            env: TypeEnv::new(),
            engine: InferenceEngine::new(),
            diagnostics: Vec::new(),
            current_return_type: None,
            deprecated_functions: std::collections::HashSet::new(),
            pure_functions: std::collections::HashSet::new(),
            in_pure_function: false,
        }
    }

    /// Report an error diagnostic.
    fn error(&mut self, msg: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::error(msg).with_label(span, "here"));
    }

    /// Report a warning diagnostic.
    fn warning(&mut self, msg: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::warning(msg).with_label(span, "here"));
    }

    // ── TypeExpr → Type resolution ───────────────────────────────────

    /// Resolve an AST TypeExpr to a concrete Type.
    pub fn resolve_type_expr(&mut self, te: &TypeExpr) -> Type {
        match te {
            TypeExpr::Simple { name, span } => {
                if let Some(ty) = Type::from_name(name) {
                    ty
                } else {
                    // Check if it's a known struct or enum name.
                    if self.env.lookup_struct(name).is_some() {
                        Type::Struct {
                            name: name.clone(),
                            generics: vec![],
                        }
                    } else if self.env.lookup_enum(name).is_some() {
                        Type::Enum {
                            name: name.clone(),
                            generics: vec![],
                        }
                    } else if let Some(ty) = self.env.lookup_var(name) {
                        // Type alias or generic parameter registered as a variable.
                        ty.clone()
                    } else {
                        self.error(format!("unknown type `{name}`"), *span);
                        Type::Error
                    }
                }
            }
            TypeExpr::Generic { name, args, span } => {
                let resolved_args: Vec<Type> =
                    args.iter().map(|a| self.resolve_type_expr(a)).collect();

                // Handle well-known generic types.
                match name.as_str() {
                    "Option" => {
                        if resolved_args.len() == 1 {
                            Type::Option {
                                inner: Box::new(resolved_args[0].clone()),
                            }
                        } else {
                            self.error("Option expects exactly 1 type argument", *span);
                            Type::Error
                        }
                    }
                    "Result" => {
                        if resolved_args.len() == 2 {
                            Type::Result {
                                ok: Box::new(resolved_args[0].clone()),
                                err: Box::new(resolved_args[1].clone()),
                            }
                        } else {
                            self.error("Result expects exactly 2 type arguments", *span);
                            Type::Error
                        }
                    }
                    "Map" => {
                        if resolved_args.len() == 2 {
                            Type::Map {
                                key: Box::new(resolved_args[0].clone()),
                                value: Box::new(resolved_args[1].clone()),
                            }
                        } else {
                            self.error("Map expects exactly 2 type arguments", *span);
                            Type::Error
                        }
                    }
                    // chan<T> — channels are opaque i64 handles at runtime.
                    "chan" => Type::I64,
                    "Set" => {
                        if resolved_args.len() == 1 {
                            Type::Set {
                                element: Box::new(resolved_args[0].clone()),
                            }
                        } else {
                            self.error("Set expects exactly 1 type argument", *span);
                            Type::Error
                        }
                    }
                    _ => {
                        // User-defined generic struct or enum.
                        if self.env.lookup_struct(name).is_some() {
                            Type::Struct {
                                name: name.clone(),
                                generics: resolved_args,
                            }
                        } else if self.env.lookup_enum(name).is_some() {
                            Type::Enum {
                                name: name.clone(),
                                generics: resolved_args,
                            }
                        } else {
                            // Treat as struct by default (forward reference).
                            Type::Struct {
                                name: name.clone(),
                                generics: resolved_args,
                            }
                        }
                    }
                }
            }
            TypeExpr::Array {
                element,
                size,
                span: _,
            } => Type::Array {
                element: Box::new(self.resolve_type_expr(element)),
                size: *size,
            },
            TypeExpr::Tuple {
                elements,
                span: _,
            } => Type::Tuple {
                elements: elements.iter().map(|e| self.resolve_type_expr(e)).collect(),
            },
            TypeExpr::Function {
                params,
                ret,
                span: _,
            } => Type::Function {
                params: params.iter().map(|p| self.resolve_type_expr(p)).collect(),
                ret: Box::new(self.resolve_type_expr(ret)),
            },
            TypeExpr::Optional { inner, span: _ } => Type::Option {
                inner: Box::new(self.resolve_type_expr(inner)),
            },
            TypeExpr::Reference {
                inner,
                mutable,
                span: _,
            } => Type::Reference {
                inner: Box::new(self.resolve_type_expr(inner)),
                mutable: *mutable,
            },
            TypeExpr::Shared { inner, span: _ } => Type::Shared {
                inner: Box::new(self.resolve_type_expr(inner)),
            },
            TypeExpr::Weak { inner, span: _ } => Type::Weak {
                inner: Box::new(self.resolve_type_expr(inner)),
            },
            TypeExpr::Pointer {
                inner,
                mutable,
                span: _,
            } => Type::Pointer {
                inner: Box::new(self.resolve_type_expr(inner)),
                mutable: *mutable,
            },
            TypeExpr::DynTrait { trait_name, span } => {
                // Verify the trait exists.
                if self.env.lookup_trait(trait_name).is_none() {
                    self.error(format!("unknown trait `{trait_name}`"), *span);
                    return Type::Error;
                }
                Type::DynTrait { trait_name: trait_name.clone() }
            }
            TypeExpr::Inferred { span: _ } => {
                // Create a fresh type variable to be inferred.
                self.engine.fresh_var()
            }
        }
    }

    // ── Declaration checking ─────────────────────────────────────────

    /// Type-check an entire module.
    pub fn check_module(&mut self, module: &Module) {
        // First pass: register all type and function declarations.
        for decl in &module.declarations {
            self.register_decl(decl);
            // Track annotated functions.
            if let Decl::Function { name, annotations, .. } = decl {
                for ann in annotations {
                    match ann.name.as_str() {
                        "deprecated" => { self.deprecated_functions.insert(name.clone()); }
                        "pure" => { self.pure_functions.insert(name.clone()); }
                        _ => {}
                    }
                }
            }
        }
        // Second pass: check function bodies and expressions.
        for decl in &module.declarations {
            self.check_decl(decl);
        }
    }

    /// Register a declaration in the environment (forward declaration pass).
    fn register_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Function {
                name,
                generics,
                params,
                ret_ty,
                ..
            } => {
                // Temporarily bind generic params so they resolve in param/return types.
                if !generics.is_empty() {
                    self.env.push_scope();
                    for gp in generics {
                        let tv = self.engine.fresh_var();
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }

                let param_types: Vec<(String, Type)> = params
                    .iter()
                    .map(|p| {
                        let ty = p
                            .ty
                            .as_ref()
                            .map(|t| self.resolve_type_expr(t))
                            .unwrap_or_else(|| self.engine.fresh_var());
                        (p.name.clone(), ty)
                    })
                    .collect();

                let ret = ret_ty
                    .as_ref()
                    .map(|t| self.resolve_type_expr(t))
                    .unwrap_or(Type::Void);

                if !generics.is_empty() {
                    self.env.pop_scope();
                }

                let sig = FunctionSig {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    params: param_types,
                    ret,
                };
                self.env.define_function(sig);
            }
            Decl::Struct {
                name,
                generics,
                fields,
                ..
            } => {
                // Bind generic params so they resolve in field types.
                if !generics.is_empty() {
                    self.env.push_scope();
                    for gp in generics {
                        let tv = self.engine.fresh_var();
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }
                let field_types: Vec<(String, Type)> = fields
                    .iter()
                    .map(|f| (f.name.clone(), self.resolve_type_expr(&f.ty)))
                    .collect();
                if !generics.is_empty() {
                    self.env.pop_scope();
                }
                self.env.define_struct(StructDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    fields: field_types,
                });
            }
            Decl::Enum {
                name,
                generics,
                variants,
                ..
            } => {
                // Bind generic params so they resolve in variant field types.
                if !generics.is_empty() {
                    self.env.push_scope();
                    for gp in generics {
                        let tv = self.engine.fresh_var();
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }
                let variant_types: Vec<(String, Vec<Type>)> = variants
                    .iter()
                    .map(|v| {
                        let tys = v.fields.iter().map(|t| self.resolve_type_expr(t)).collect();
                        (v.name.clone(), tys)
                    })
                    .collect();
                if !generics.is_empty() {
                    self.env.pop_scope();
                }
                self.env.define_enum(EnumDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    variants: variant_types,
                });
            }
            Decl::Impl {
                target,
                trait_name,
                methods,
                ..
            } => {
                // Resolve the impl target type once for binding `self` params.
                let impl_target_ty = if self.env.lookup_struct(target).is_some() {
                    Some(Type::Struct {
                        name: target.clone(),
                        generics: vec![],
                    })
                } else if self.env.lookup_enum(target).is_some() {
                    Some(Type::Enum {
                        name: target.clone(),
                        generics: vec![],
                    })
                } else {
                    None
                };

                let method_sigs: Vec<FunctionSig> = methods
                    .iter()
                    .filter_map(|m| {
                        if let Decl::Function {
                            name,
                            generics,
                            params,
                            ret_ty,
                            ..
                        } = m
                        {
                            let param_types: Vec<(String, Type)> = params
                                .iter()
                                .map(|p| {
                                    let ty = if p.name == "self" && p.ty.is_none() {
                                        // `self` with no annotation gets the impl target type.
                                        impl_target_ty
                                            .clone()
                                            .unwrap_or_else(|| self.engine.fresh_var())
                                    } else {
                                        p.ty
                                            .as_ref()
                                            .map(|t| self.resolve_type_expr(t))
                                            .unwrap_or_else(|| self.engine.fresh_var())
                                    };
                                    (p.name.clone(), ty)
                                })
                                .collect();
                            let ret = ret_ty
                                .as_ref()
                                .map(|t| self.resolve_type_expr(t))
                                .unwrap_or(Type::Void);
                            Some(FunctionSig {
                                name: name.clone(),
                                generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                                params: param_types,
                                ret,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                // Register each method as a standalone function so check_decl
                // can look it up and bind params (including `self`) correctly.
                for sig in &method_sigs {
                    self.env.define_function(sig.clone());
                }
                self.env
                    .define_impl(target.clone(), trait_name.clone(), method_sigs);
            }
            Decl::Trait {
                name,
                generics,
                methods,
                ..
            } => {
                let method_sigs: Vec<FunctionSig> = methods
                    .iter()
                    .filter_map(|m| {
                        if let Decl::Function {
                            name,
                            generics,
                            params,
                            ret_ty,
                            ..
                        } = m
                        {
                            let param_types: Vec<(String, Type)> = params
                                .iter()
                                .map(|p| {
                                    let ty = p
                                        .ty
                                        .as_ref()
                                        .map(|t| self.resolve_type_expr(t))
                                        .unwrap_or_else(|| self.engine.fresh_var());
                                    (p.name.clone(), ty)
                                })
                                .collect();
                            let ret = ret_ty
                                .as_ref()
                                .map(|t| self.resolve_type_expr(t))
                                .unwrap_or(Type::Void);
                            Some(FunctionSig {
                                name: name.clone(),
                                generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                                params: param_types,
                                ret,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                self.env.define_trait(crate::env::TraitDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    methods: method_sigs,
                });
            }
            Decl::TypeAlias { name, ty, .. } => {
                let resolved = self.resolve_type_expr(ty);
                // Register as a variable in the type namespace for lookup.
                self.env.define_var(name.clone(), resolved);
            }
            Decl::Const { name, ty, value, .. } => {
                let resolved_ty = if let Some(t) = ty {
                    self.resolve_type_expr(t)
                } else {
                    self.infer_expr(value)
                };
                self.env.define_var(name.clone(), resolved_ty);
            }
            Decl::Import { .. } | Decl::Actor { .. } => {
                // These don't introduce types we need to check yet.
            }
            Decl::Extern { items, .. } => {
                // Register extern function declarations so they're callable.
                for item in items {
                    self.register_decl(item);
                }
            }
        }
    }

    /// Check a declaration's body (second pass).
    fn check_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Function {
                name,
                generics,
                params,
                ret_ty,
                body: Some(body),
                span,
                ..
            } => {
                self.env.push_scope();

                // Set @pure flag for the duration of this function body.
                let was_pure = self.in_pure_function;
                if self.pure_functions.contains(name) {
                    self.in_pure_function = true;
                }

                // Bind generic type parameters as type variables so they resolve
                // in parameter types, return types, and the function body.
                for gp in generics {
                    let tv = self.engine.fresh_var();
                    self.env.define_var(gp.name.clone(), tv);
                }

                // Bind parameters in function scope.
                let sig = self.env.lookup_function(name).cloned();
                if let Some(ref sig) = sig {
                    for (pname, pty) in &sig.params {
                        self.env.define_var(pname.clone(), pty.clone());
                    }
                    self.current_return_type = Some(sig.ret.clone());
                } else {
                    // Fallback: resolve params directly.
                    for param in params {
                        let ty = param
                            .ty
                            .as_ref()
                            .map(|t| self.resolve_type_expr(t))
                            .unwrap_or_else(|| self.engine.fresh_var());
                        self.env.define_var(param.name.clone(), ty);
                    }
                    self.current_return_type = ret_ty
                        .as_ref()
                        .map(|t| self.resolve_type_expr(t));
                }

                self.check_block(body);

                // Check for missing return in non-void functions.
                if let Some(ref ret) = self.current_return_type {
                    if *ret != Type::Void && !block_returns(body) {
                        self.error(
                            format!(
                                "function `{name}` has return type but not all paths return a value"
                            ),
                            *span,
                        );
                    }
                }

                self.current_return_type = None;
                self.in_pure_function = was_pure;
                self.env.pop_scope();

                let _ = span; // suppress unused warning
            }
            Decl::Impl { target, methods, .. } => {
                // Resolve the target type so we can bind `self` in methods.
                let target_ty = if self.env.lookup_struct(target).is_some() {
                    Some(Type::Struct {
                        name: target.clone(),
                        generics: vec![],
                    })
                } else if self.env.lookup_enum(target).is_some() {
                    Some(Type::Enum {
                        name: target.clone(),
                        generics: vec![],
                    })
                } else {
                    None
                };

                for method in methods {
                    if let (Some(ref ty), Decl::Function { body: Some(_), .. }) =
                        (&target_ty, method)
                    {
                        // Push scope, bind `self` to the concrete target type,
                        // then check the method body normally.
                        self.env.push_scope();
                        self.env.define_var("self".to_string(), ty.clone());
                        self.check_decl(method);
                        self.env.pop_scope();
                    } else {
                        self.check_decl(method);
                    }
                }
            }
            _ => {}
        }
    }

    // ── Block / statement checking ───────────────────────────────────

    pub fn check_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                ty,
                value,
                span,
                ..
            } => {
                let declared_ty = ty.as_ref().map(|t| self.resolve_type_expr(t));
                let inferred_ty = value.as_ref().map(|v| self.infer_expr(v));

                let final_ty = match (declared_ty, inferred_ty) {
                    (Some(decl), Some(inferred)) => {
                        // Both declared and inferred: unify them.
                        if let Err(diag) = self.engine.unify(&decl, &inferred, *span) {
                            self.diagnostics.push(diag);
                        }
                        decl
                    }
                    (Some(decl), None) => decl,
                    (None, Some(inferred)) => inferred,
                    (None, None) => {
                        // No type info at all — create a fresh variable.
                        self.engine.fresh_var()
                    }
                };

                self.env.define_var(name.clone(), final_ty);
            }
            Stmt::Assign {
                target,
                value,
                span,
                ..
            } => {
                let target_ty = self.infer_expr(target);
                let value_ty = self.infer_expr(value);
                if let Err(diag) = self.engine.unify(&target_ty, &value_ty, *span) {
                    self.diagnostics.push(diag);
                }
            }
            Stmt::Return { value, span } => {
                let ret_ty = value
                    .as_ref()
                    .map(|v| self.infer_expr(v))
                    .unwrap_or(Type::Void);

                if let Some(ref expected) = self.current_return_type {
                    if let Err(diag) = self.engine.unify(expected, &ret_ty, *span) {
                        self.diagnostics.push(diag);
                    }
                }
            }
            Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                span,
            } => {
                let cond_ty = self.infer_expr(condition);
                if let Err(diag) = self.engine.unify(&Type::Bool, &cond_ty, *span) {
                    self.diagnostics.push(diag);
                }
                self.env.push_scope();
                self.check_block(then_block);
                self.env.pop_scope();
                for (elif_cond, elif_block) in elif_clauses {
                    let elif_ty = self.infer_expr(elif_cond);
                    if let Err(diag) = self.engine.unify(&Type::Bool, &elif_ty, *span) {
                        self.diagnostics.push(diag);
                    }
                    self.env.push_scope();
                    self.check_block(elif_block);
                    self.env.pop_scope();
                }
                if let Some(else_blk) = else_block {
                    self.env.push_scope();
                    self.check_block(else_blk);
                    self.env.pop_scope();
                }
            }
            Stmt::While {
                condition,
                body,
                span,
            } => {
                let cond_ty = self.infer_expr(condition);
                if let Err(diag) = self.engine.unify(&Type::Bool, &cond_ty, *span) {
                    self.diagnostics.push(diag);
                }
                self.env.push_scope();
                self.check_block(body);
                self.env.pop_scope();
            }
            Stmt::For {
                pattern,
                iterable,
                body,
                ..
            } => {
                // Infer the iterable type and determine element type.
                let iter_ty = self.infer_expr(iterable);
                let elem_ty = match &iter_ty {
                    Type::Array { element, .. } => *element.clone(),
                    _ => Type::I32, // default for range() and other builtins
                };

                self.env.push_scope();

                // Bind the loop variable from the pattern into scope.
                if let Pattern::Ident { name, .. } = pattern {
                    self.env.define_var(name.clone(), elem_ty);
                }

                self.check_block(body);
                self.env.pop_scope();
            }
            Stmt::Expr { expr, .. } => {
                self.infer_expr(expr);
            }
            Stmt::Throw { expr, .. } => {
                self.infer_expr(expr);
            }
            Stmt::TryCatch {
                try_block,
                catch_name,
                catch_block,
                ..
            } => {
                self.env.push_scope();
                self.check_block(try_block);
                self.env.pop_scope();
                self.env.push_scope();
                self.env.define_var(catch_name.clone(), Type::Error);
                self.check_block(catch_block);
                self.env.pop_scope();
            }
            Stmt::Spawn { expr, .. } => {
                self.infer_expr(expr);
            }
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Select { .. } => {}
        }
    }

    // ── Pattern binding ──────────────────────────────────────────────

    /// Bind variables introduced by a pattern into the current scope.
    ///
    /// For example, `Option::Some(val)` binds `val` to a fresh type variable.
    /// `Ident` patterns bind the name to the subject type.
    /// Wildcards and literals bind nothing.
    fn bind_pattern(&mut self, pattern: &Pattern, subject_ty: &Type) {
        match pattern {
            Pattern::Wildcard { .. } | Pattern::Literal { .. } => {}
            Pattern::Ident { name, .. } => {
                self.env.define_var(name.clone(), subject_ty.clone());
            }
            Pattern::Tuple { elements, .. } => {
                for elem in elements {
                    let tv = self.engine.fresh_var();
                    self.bind_pattern(elem, &tv);
                }
            }
            Pattern::Struct { fields, .. } => {
                for (_field_name, pat) in fields {
                    let tv = self.engine.fresh_var();
                    self.bind_pattern(pat, &tv);
                }
            }
            Pattern::Enum { name, fields, .. } => {
                // Look up the enum variant's field types if available.
                let field_types: Vec<Type> =
                    if let Some(edef) = self.env.lookup_enum(name).cloned() {
                        // Find variant types (already stored as Vec<Type> per variant)
                        edef.variants
                            .iter()
                            .find(|(_, _)| true) // We'd need variant name here
                            .map(|(_, tys)| tys.clone())
                            .unwrap_or_default()
                    } else {
                        vec![]
                    };

                for (i, pat) in fields.iter().enumerate() {
                    let field_ty = field_types
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| self.engine.fresh_var());
                    self.bind_pattern(pat, &field_ty);
                }
            }
            Pattern::Or { patterns, .. } => {
                for pat in patterns {
                    self.bind_pattern(pat, subject_ty);
                }
            }
        }
    }

    // ── Expression type inference ────────────────────────────────────

    /// Infer the type of an expression.
    pub fn infer_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            // Literals.
            Expr::IntLiteral { .. } => Type::I32,
            Expr::FloatLiteral { .. } => Type::F64,
            Expr::StringLiteral { .. } | Expr::InterpolatedString { .. } => Type::Str,
            Expr::CharLiteral { .. } => Type::Char,
            Expr::BoolLiteral { .. } => Type::Bool,
            Expr::NoneLiteral { .. } => Type::Option {
                inner: Box::new(self.engine.fresh_var()),
            },

            // Identifier lookup.
            Expr::Identifier { name, span } => {
                if let Some(ty) = self.env.lookup_var(name) {
                    ty.clone()
                } else if let Some(sig) = self.env.lookup_function(name) {
                    // Function used as a value — return its function type.
                    Type::Function {
                        params: sig.params.iter().map(|(_, t)| t.clone()).collect(),
                        ret: Box::new(sig.ret.clone()),
                    }
                } else if self.env.lookup_enum(name).is_some() {
                    // Enum name used as a namespace (e.g., `Color` in `Color.Red`).
                    Type::Enum {
                        name: name.clone(),
                        generics: vec![],
                    }
                } else {
                    self.error(format!("undefined variable `{name}`"), *span);
                    Type::Error
                }
            }

            // Field access: object.field.
            Expr::FieldAccess {
                object,
                field,
                span,
            } => {
                let obj_ty = self.infer_expr(object);
                let obj_ty = self.engine.resolve(&obj_ty);
                match &obj_ty {
                    Type::Struct { name, .. } => {
                        if let Some(fty) = self.env.lookup_field(name, field) {
                            fty.clone()
                        } else {
                            self.error(
                                format!("no field `{field}` on type `{name}`"),
                                *span,
                            );
                            Type::Error
                        }
                    }
                    Type::Tuple { elements } => {
                        // Tuple field access: t.0, t.1, etc.
                        if let Ok(idx) = field.parse::<usize>() {
                            if idx < elements.len() {
                                elements[idx].clone()
                            } else {
                                self.error(
                                    format!(
                                        "tuple index {idx} out of bounds (length {})",
                                        elements.len()
                                    ),
                                    *span,
                                );
                                Type::Error
                            }
                        } else {
                            self.error(
                                format!("no field `{field}` on tuple type"),
                                *span,
                            );
                            Type::Error
                        }
                    }
                    // Auto-deref: access fields through references.
                    Type::Reference { inner, .. } => {
                        if let Type::Struct { name, .. } = inner.as_ref() {
                            if let Some(fty) = self.env.lookup_field(name, field) {
                                fty.clone()
                            } else {
                                self.error(
                                    format!("no field `{field}` on type `{name}`"),
                                    *span,
                                );
                                Type::Error
                            }
                        } else {
                            self.error(
                                format!("cannot access field `{field}` on type `{obj_ty}`"),
                                *span,
                            );
                            Type::Error
                        }
                    }
                    // Enum variant access: Color.Red resolves to the enum type.
                    Type::Enum { name, generics } => {
                        if let Some(edef) = self.env.lookup_enum(name) {
                            if edef.variants.iter().any(|(vname, _)| vname == field) {
                                Type::Enum {
                                    name: name.clone(),
                                    generics: generics.clone(),
                                }
                            } else {
                                self.error(
                                    format!("no variant `{field}` on enum `{name}`"),
                                    *span,
                                );
                                Type::Error
                            }
                        } else {
                            self.error(
                                format!("cannot access field `{field}` on type `{obj_ty}`"),
                                *span,
                            );
                            Type::Error
                        }
                    }
                    Type::Error => Type::Error,
                    _ => {
                        self.error(
                            format!("cannot access field `{field}` on type `{obj_ty}`"),
                            *span,
                        );
                        Type::Error
                    }
                }
            }

            // Index access: object[index].
            Expr::IndexAccess {
                object,
                index,
                span,
            } => {
                let obj_ty = self.infer_expr(object);
                let idx_ty = self.infer_expr(index);
                let obj_ty = self.engine.resolve(&obj_ty);
                match &obj_ty {
                    Type::Array { element, .. } => {
                        // Index must be an integer.
                        if !self.engine.resolve(&idx_ty).is_integer() {
                            if let Err(diag) = self.engine.unify(&Type::I32, &idx_ty, *span) {
                                self.diagnostics.push(diag);
                            }
                        }
                        *element.clone()
                    }
                    Type::Map { key, value } => {
                        if let Err(diag) = self.engine.unify(key, &idx_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                        *value.clone()
                    }
                    Type::Error => Type::Error,
                    _ => {
                        self.error(
                            format!("type `{obj_ty}` is not indexable"),
                            *span,
                        );
                        Type::Error
                    }
                }
            }

            // Binary operations.
            Expr::BinaryOp {
                op,
                left,
                right,
                span,
            } => self.check_binary_op(*op, left, right, *span),

            // Unary operations.
            Expr::UnaryOp {
                op,
                operand,
                span,
            } => {
                let operand_ty = self.infer_expr(operand);
                match op {
                    UnOp::Neg => {
                        let resolved = self.engine.resolve(&operand_ty);
                        if resolved.is_numeric() || resolved.is_error() {
                            operand_ty
                        } else {
                            self.error(
                                format!("cannot negate type `{resolved}`"),
                                *span,
                            );
                            Type::Error
                        }
                    }
                    UnOp::Not => {
                        if let Err(diag) = self.engine.unify(&Type::Bool, &operand_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                        Type::Bool
                    }
                    UnOp::BitNot => {
                        let resolved = self.engine.resolve(&operand_ty);
                        if resolved.is_integer() || resolved.is_error() {
                            operand_ty
                        } else {
                            self.error(
                                format!("cannot bitwise-not type `{resolved}`"),
                                *span,
                            );
                            Type::Error
                        }
                    }
                }
            }

            // Function call.
            Expr::FnCall { callee, args, span } => {
                // Extract callee name for attribute checks.
                let callee_name_str = match callee.as_ref() {
                    Expr::Identifier { name, .. } => Some(name.clone()),
                    _ => None,
                };

                // @deprecated: warn when calling a deprecated function.
                if let Some(ref name) = callee_name_str {
                    if self.deprecated_functions.contains(name) {
                        self.warning(
                            format!("use of deprecated function `{name}`"),
                            *span,
                        );
                    }
                }

                // @pure enforcement: pure functions cannot call non-pure functions or I/O builtins.
                if self.in_pure_function {
                    let io_builtins = ["println", "print", "eprintln", "exit"];
                    if let Some(ref name) = callee_name_str {
                        if io_builtins.contains(&name.as_str()) {
                            self.error(
                                format!("`@pure` function cannot call I/O builtin `{name}`"),
                                *span,
                            );
                        } else if !self.pure_functions.contains(name) {
                            // Only warn for user-defined functions that are not marked @pure.
                            // Skip builtins like len, range, etc. which are side-effect free.
                            let side_effect_free_builtins = [
                                "len", "range", "to_string", "typeof", "sizeof",
                                "min", "max", "abs", "sqrt", "pow", "floor", "ceil",
                                "round", "log", "log2", "log10", "sin", "cos", "tan",
                                "push", "pop", "contains", "keys", "values",
                                "split", "trim", "starts_with", "ends_with",
                                "to_upper", "to_lower", "char_at", "substring",
                                "parse_int", "parse_float",
                            ];
                            if self.env.lookup_function(name).is_some()
                                && !side_effect_free_builtins.contains(&name.as_str())
                            {
                                self.error(
                                    format!("`@pure` function cannot call non-pure function `{name}`"),
                                    *span,
                                );
                            }
                        }
                    }
                }

                let callee_ty = self.infer_expr(callee);
                let callee_ty = self.engine.resolve(&callee_ty);

                match &callee_ty {
                    Type::Function { params, ret } => {
                        // Special handling for assert(): accept 1 or 2 args.
                        // assert(condition) uses a default message at codegen time.
                        let is_assert_1arg = matches!(&callee_name_str, Some(n) if n == "assert")
                            && args.len() == 1
                            && params.len() == 2;

                        if !is_assert_1arg && args.len() != params.len() {
                            let display_name = match callee_name_str {
                                Some(ref n) => format!(" to `{n}`"),
                                None => String::new(),
                            };
                            self.error(
                                format!(
                                    "this function{display_name} takes {} argument{} but {} {} supplied",
                                    params.len(),
                                    if params.len() == 1 { "" } else { "s" },
                                    args.len(),
                                    if args.len() == 1 { "was" } else { "were" },
                                ),
                                *span,
                            );
                        } else {
                            for (arg, param_ty) in args.iter().zip(params.iter()) {
                                let arg_ty = self.infer_expr(arg);
                                if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span())
                                {
                                    self.diagnostics.push(diag);
                                }
                            }
                        }
                        *ret.clone()
                    }
                    Type::Error => Type::Error,
                    _ => {
                        self.error(
                            format!("type `{callee_ty}` is not callable"),
                            *span,
                        );
                        Type::Error
                    }
                }
            }

            // Method call.
            Expr::MethodCall {
                object,
                method,
                args,
                span,
            } => {
                let obj_ty = self.infer_expr(object);
                let obj_ty = self.engine.resolve(&obj_ty);

                // Handle method calls on dyn Trait objects via trait definition lookup.
                if let Type::DynTrait { ref trait_name } = obj_ty {
                    if let Some(trait_def) = self.env.lookup_trait(trait_name).cloned() {
                        if let Some(sig) = trait_def.methods.iter().find(|m| m.name == *method) {
                            // Skip 'self' parameter (first param) if present.
                            let expected_params: Vec<_> = if sig.params.first().map(|(n, _)| n.as_str())
                                == Some("self")
                            {
                                sig.params[1..].to_vec()
                            } else {
                                sig.params.clone()
                            };
                            if args.len() != expected_params.len() {
                                self.error(
                                    format!(
                                        "method `{method}` expects {} arguments, found {}",
                                        expected_params.len(),
                                        args.len()
                                    ),
                                    *span,
                                );
                            } else {
                                for (arg, (_, param_ty)) in args.iter().zip(expected_params.iter()) {
                                    let arg_ty = self.infer_expr(arg);
                                    if let Err(diag) =
                                        self.engine.unify(param_ty, &arg_ty, arg.span())
                                    {
                                        self.diagnostics.push(diag);
                                    }
                                }
                            }
                            return sig.ret.clone();
                        }
                    }
                    self.error(
                        format!("no method `{method}` found on `dyn {trait_name}`"),
                        *span,
                    );
                    return Type::Error;
                }

                let type_name = match &obj_ty {
                    Type::Struct { name, .. } => Some(name.clone()),
                    Type::Enum { name, .. } => Some(name.clone()),
                    Type::Error => return Type::Error,
                    _ => None,
                };

                if let Some(ref tname) = type_name {
                    // Check if this is an enum variant constructor (e.g. Shape.Circle(3)).
                    if let Some(edef) = self.env.lookup_enum(tname).cloned() {
                        if edef.variants.iter().any(|(vname, _)| vname == method) {
                            // Infer args but accept the call as a variant constructor.
                            for arg in args.iter() {
                                self.infer_expr(arg);
                            }
                            return Type::Enum { name: tname.clone(), generics: vec![] };
                        }
                    }

                    if let Some(sig) = self.env.lookup_method(tname, method).cloned() {
                        // Skip 'self' parameter (first param) if present.
                        let expected_params: Vec<_> = if sig.params.first().map(|(n, _)| n.as_str())
                            == Some("self")
                        {
                            sig.params[1..].to_vec()
                        } else {
                            sig.params.clone()
                        };

                        if args.len() != expected_params.len() {
                            self.error(
                                format!(
                                    "method `{method}` expects {} arguments, found {}",
                                    expected_params.len(),
                                    args.len()
                                ),
                                *span,
                            );
                        } else {
                            for (arg, (_, param_ty)) in args.iter().zip(expected_params.iter()) {
                                let arg_ty = self.infer_expr(arg);
                                if let Err(diag) =
                                    self.engine.unify(param_ty, &arg_ty, arg.span())
                                {
                                    self.diagnostics.push(diag);
                                }
                            }
                        }
                        return sig.ret.clone();
                    }
                }

                self.error(
                    format!("no method `{method}` found for type `{obj_ty}`"),
                    *span,
                );
                Type::Error
            }

            // Literals: array, tuple, map, struct.
            Expr::ArrayLiteral { elements, span } => {
                if elements.is_empty() {
                    Type::Array {
                        element: Box::new(self.engine.fresh_var()),
                        size: Some(0),
                    }
                } else {
                    let first_ty = self.infer_expr(&elements[0]);
                    for elem in &elements[1..] {
                        let elem_ty = self.infer_expr(elem);
                        if let Err(diag) = self.engine.unify(&first_ty, &elem_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                    }
                    Type::Array {
                        element: Box::new(first_ty),
                        size: Some(elements.len() as u64),
                    }
                }
            }
            Expr::TupleLiteral { elements, .. } => Type::Tuple {
                elements: elements.iter().map(|e| self.infer_expr(e)).collect(),
            },
            Expr::MapLiteral { entries, span } => {
                if entries.is_empty() {
                    Type::Map {
                        key: Box::new(self.engine.fresh_var()),
                        value: Box::new(self.engine.fresh_var()),
                    }
                } else {
                    let (first_key, first_val) = &entries[0];
                    let key_ty = self.infer_expr(first_key);
                    let val_ty = self.infer_expr(first_val);
                    for (k, v) in &entries[1..] {
                        let kt = self.infer_expr(k);
                        let vt = self.infer_expr(v);
                        if let Err(diag) = self.engine.unify(&key_ty, &kt, *span) {
                            self.diagnostics.push(diag);
                        }
                        if let Err(diag) = self.engine.unify(&val_ty, &vt, *span) {
                            self.diagnostics.push(diag);
                        }
                    }
                    Type::Map {
                        key: Box::new(key_ty),
                        value: Box::new(val_ty),
                    }
                }
            }
            Expr::StructLiteral { name, fields, span } => {
                if let Some(def) = self.env.lookup_struct(name).cloned() {
                    for (fname, fexpr) in fields {
                        let expr_ty = self.infer_expr(fexpr);
                        if let Some((_, expected_ty)) =
                            def.fields.iter().find(|(n, _)| n == fname)
                        {
                            if let Err(diag) =
                                self.engine.unify(expected_ty, &expr_ty, fexpr.span())
                            {
                                self.diagnostics.push(diag);
                            }
                        } else {
                            self.error(
                                format!("no field `{fname}` on struct `{name}`"),
                                *span,
                            );
                        }
                    }
                    Type::Struct {
                        name: name.clone(),
                        generics: def
                            .generic_params
                            .iter()
                            .map(|_| self.engine.fresh_var())
                            .collect(),
                    }
                } else {
                    self.error(format!("unknown struct `{name}`"), *span);
                    Type::Error
                }
            }

            // Lambda.
            Expr::Lambda {
                params,
                ret_ty,
                body,
                ..
            } => {
                let param_types: Vec<Type> = params
                    .iter()
                    .map(|p| {
                        p.ty.as_ref()
                            .map(|t| self.resolve_type_expr(t))
                            .unwrap_or_else(|| self.engine.fresh_var())
                    })
                    .collect();

                let ret = ret_ty
                    .as_ref()
                    .map(|t| self.resolve_type_expr(t))
                    .unwrap_or_else(|| self.engine.fresh_var());

                // Set current_return_type so `return` statements inside the
                // lambda body are validated against the declared return type.
                let prev_ret = self.current_return_type.take();
                self.current_return_type = Some(ret.clone());

                self.env.push_scope();
                for (param, ty) in params.iter().zip(param_types.iter()) {
                    self.env.define_var(param.name.clone(), ty.clone());
                }
                let body_ty = self.infer_expr(body);
                self.env.pop_scope();

                self.current_return_type = prev_ret;

                // If the body evaluates to Void (e.g. block ending with `return`),
                // the return statements already validated against `ret`.
                // Only unify when body produces a non-void expression result.
                if body_ty != Type::Void {
                    if let Err(diag) = self.engine.unify(&ret, &body_ty, body.span()) {
                        self.diagnostics.push(diag);
                    }
                }

                Type::Function {
                    params: param_types,
                    ret: Box::new(ret),
                }
            }

            // If expression.
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                span,
            } => {
                let cond_ty = self.infer_expr(condition);
                if let Err(diag) = self.engine.unify(&Type::Bool, &cond_ty, *span) {
                    self.diagnostics.push(diag);
                }
                self.env.push_scope();
                self.check_block(then_branch);
                self.env.pop_scope();
                if let Some(else_blk) = else_branch {
                    self.env.push_scope();
                    self.check_block(else_blk);
                    self.env.pop_scope();
                }
                // If used as expression, type is unit for now.
                Type::Void
            }

            // Match expression.
            Expr::MatchExpr { subject, arms, span } => {
                let subject_ty = self.infer_expr(subject);
                if arms.is_empty() {
                    return Type::Void;
                }
                let mut result_ty: Option<Type> = None;
                for arm in arms {
                    self.env.push_scope();
                    self.bind_pattern(&arm.pattern, &subject_ty);
                    let arm_ty = self.infer_expr(&arm.body);
                    self.env.pop_scope();

                    if let Some(ref first) = result_ty {
                        if let Err(diag) = self.engine.unify(first, &arm_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                    } else {
                        result_ty = Some(arm_ty);
                    }
                }
                result_ty.unwrap_or(Type::Void)
            }

            // Range expression.
            Expr::RangeExpr {
                start,
                end,
                span,
                ..
            } => {
                let start_ty = start
                    .as_ref()
                    .map(|e| self.infer_expr(e))
                    .unwrap_or(Type::I32);
                let end_ty = end
                    .as_ref()
                    .map(|e| self.infer_expr(e))
                    .unwrap_or(Type::I32);
                if let Err(diag) = self.engine.unify(&start_ty, &end_ty, *span) {
                    self.diagnostics.push(diag);
                }
                // Range type is a struct-like type.
                Type::Struct {
                    name: "Range".to_string(),
                    generics: vec![start_ty],
                }
            }

            // Pipe expression (left |> right).
            Expr::PipeExpr { left, right, span } => {
                let left_ty = self.infer_expr(left);
                let right_ty = self.infer_expr(right);
                let right_ty = self.engine.resolve(&right_ty);
                match &right_ty {
                    Type::Function { params, ret } => {
                        if params.len() == 1 {
                            if let Err(diag) = self.engine.unify(&params[0], &left_ty, *span) {
                                self.diagnostics.push(diag);
                            }
                            *ret.clone()
                        } else {
                            self.error(
                                "pipe target must be a function taking exactly 1 argument",
                                *span,
                            );
                            Type::Error
                        }
                    }
                    Type::Error => Type::Error,
                    _ => {
                        self.error(
                            format!("pipe target must be a function, found `{right_ty}`"),
                            *span,
                        );
                        Type::Error
                    }
                }
            }

            // Borrow expression: &x or &mut x.
            Expr::Borrow { inner, mutable, .. } => {
                let inner_ty = self.infer_expr(inner);
                Type::Reference {
                    inner: Box::new(inner_ty),
                    mutable: *mutable,
                }
            }
            // Dereference expression: *x.
            Expr::Deref { inner, .. } => {
                let inner_ty = self.infer_expr(inner);
                match inner_ty {
                    Type::Reference { inner, .. } => *inner,
                    Type::Pointer { inner, .. } => *inner,
                    Type::Shared { inner } => *inner,
                    _ => {
                        self.error(
                            format!("cannot dereference value of type `{inner_ty}`"),
                            inner.span(),
                        );
                        Type::Error
                    }
                }
            }

            // Shared / Move / Weak expressions.
            Expr::SharedExpr { inner, .. } => {
                let inner_ty = self.infer_expr(inner);
                Type::Shared {
                    inner: Box::new(inner_ty),
                }
            }
            Expr::MoveExpr { inner, .. } => self.infer_expr(inner),
            Expr::WeakExpr { inner, .. } => {
                let inner_ty = self.infer_expr(inner);
                Type::Weak {
                    inner: Box::new(inner_ty),
                }
            }

            // Cast expression.
            Expr::Cast { expr, ty, .. } => {
                // Check the source expression, then return the target type.
                self.infer_expr(expr);
                self.resolve_type_expr(ty)
            }

            // Block expression — type is the type of the last expression.
            Expr::Block { block, .. } => {
                self.env.push_scope();
                for stmt in &block.stmts {
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                // Block type is Void unless last stmt is an expression.
                if let Some(last) = block.stmts.last() {
                    if let Stmt::Expr { expr, .. } = last {
                        return self.infer_expr(expr);
                    }
                }
                Type::Void
            }

            // Comptime block — type is the type of the last expression.
            Expr::ComptimeBlock { body, .. } => {
                self.env.push_scope();
                for stmt in &body.stmts {
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                if let Some(last) = body.stmts.last() {
                    if let Stmt::Expr { expr, .. } = last {
                        return self.infer_expr(expr);
                    }
                }
                Type::Void
            }
            // Quantum blocks — check body, return void for now.
            Expr::QuantumBlock { body, .. } => {
                self.env.push_scope();
                self.check_block(body);
                self.env.pop_scope();
                Type::Void
            }
        }
    }

    // ── Binary operator type checking ────────────────────────────────

    fn check_binary_op(&mut self, op: BinOp, left: &Expr, right: &Expr, span: Span) -> Type {
        let left_ty = self.infer_expr(left);
        let right_ty = self.infer_expr(right);

        match op {
            // Arithmetic: both sides must be the same numeric type.
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow => {
                if let Err(diag) = self.engine.unify(&left_ty, &right_ty, span) {
                    self.diagnostics.push(diag);
                    return Type::Error;
                }
                let resolved = self.engine.resolve(&left_ty);
                if !resolved.is_numeric() && !resolved.is_error() {
                    // Special case: string concatenation with +.
                    if op == BinOp::Add && resolved == Type::Str {
                        return Type::Str;
                    }
                    let op_sym = match op {
                        BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*",
                        BinOp::Div => "/", BinOp::Mod => "%", BinOp::Pow => "**",
                        _ => "?",
                    };
                    self.error(
                        format!("cannot apply `{op_sym}` to type `{resolved}`: expected a numeric type"),
                        span,
                    );
                    return Type::Error;
                }
                resolved
            }

            // Comparison: both sides same type, result is bool.
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                if let Err(diag) = self.engine.unify(&left_ty, &right_ty, span) {
                    self.diagnostics.push(diag);
                }
                Type::Bool
            }

            // Logical: both sides bool, result bool.
            BinOp::And | BinOp::Or => {
                if let Err(diag) = self.engine.unify(&Type::Bool, &left_ty, span) {
                    self.diagnostics.push(diag);
                }
                if let Err(diag) = self.engine.unify(&Type::Bool, &right_ty, span) {
                    self.diagnostics.push(diag);
                }
                Type::Bool
            }

            // Bitwise: both sides same integer type.
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                if let Err(diag) = self.engine.unify(&left_ty, &right_ty, span) {
                    self.diagnostics.push(diag);
                    return Type::Error;
                }
                let resolved = self.engine.resolve(&left_ty);
                if !resolved.is_integer() && !resolved.is_error() {
                    let op_sym = match op {
                        BinOp::BitAnd => "&", BinOp::BitOr => "|", BinOp::BitXor => "^",
                        BinOp::Shl => "<<", BinOp::Shr => ">>",
                        _ => "?",
                    };
                    self.error(
                        format!("cannot apply `{op_sym}` to type `{resolved}`: expected an integer type"),
                        span,
                    );
                    return Type::Error;
                }
                resolved
            }

            // Pipe is handled separately in infer_expr.
            BinOp::Pipe => {
                // Should not reach here — PipeExpr handles pipes.
                self.engine.resolve(&right_ty)
            }

            // Matrix multiply — both sides numeric.
            BinOp::MatMul => {
                if let Err(diag) = self.engine.unify(&left_ty, &right_ty, span) {
                    self.diagnostics.push(diag);
                }
                self.engine.resolve(&left_ty)
            }
        }
    }
}

// ── Public API ───────────────────────────────────────────────────────

/// Type-check a module, returning all diagnostics found.
pub fn type_check(module: &Module) -> Vec<Diagnostic> {
    let mut checker = TypeChecker::new();

    // Register built-in functions that are always available.
    // println/print/eprintln accept any type — codegen converts non-string
    // args to strings via kryos_builtin_to_string at call time.
    checker.env.define_function(FunctionSig {
        name: "println".to_string(),
        generic_params: vec![],
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Void,
    });

    // print(value: any) -> void  (no newline)
    checker.env.define_function(FunctionSig {
        name: "print".to_string(),
        generic_params: vec![],
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Void,
    });

    // eprintln(value: any) -> void  (stderr + newline)
    checker.env.define_function(FunctionSig {
        name: "eprintln".to_string(),
        generic_params: vec![],
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Void,
    });

    // exit(code: i32) -> void
    checker.env.define_function(FunctionSig {
        name: "exit".to_string(),
        generic_params: vec![],
        params: vec![("code".to_string(), Type::I32)],
        ret: Type::Void,
    });

    // range(start, end) -> [i32]  (conceptually returns an integer sequence;
    // the MIR lowering special-cases this into a counter loop)
    checker.env.define_function(FunctionSig {
        name: "range".to_string(),
        generic_params: vec![],
        params: vec![
            ("start".to_string(), Type::I32),
            ("end".to_string(), Type::I32),
        ],
        ret: Type::Array {
            element: Box::new(Type::I32),
            size: None,
        },
    });

    // len(collection) -> i64 — accepts any collection type (str, array, map).
    // Codegen passes the opaque handle to kryos_builtin_len which reads
    // the length field at offset 0 (shared across all collection types).
    checker.env.define_function(FunctionSig {
        name: "len".to_string(),
        generic_params: vec![],
        params: vec![("collection".to_string(), Type::Error)],
        ret: Type::I64,
    });

    // to_string(value) -> str — accepts any type.
    checker.env.define_function(FunctionSig {
        name: "to_string".to_string(),
        generic_params: vec![],
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Str,
    });

    // chan() -> i64  (create a new channel)
    checker.env.define_function(FunctionSig {
        name: "chan".to_string(),
        generic_params: vec![],
        params: vec![],
        ret: Type::I64,
    });

    // send(ch, value) -> void
    checker.env.define_function(FunctionSig {
        name: "send".to_string(),
        generic_params: vec![],
        params: vec![
            ("ch".to_string(), Type::I64),
            ("value".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // recv(ch) -> i64
    checker.env.define_function(FunctionSig {
        name: "recv".to_string(),
        generic_params: vec![],
        params: vec![("ch".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // file_read(path: str) -> str — read entire file to string
    checker.env.define_function(FunctionSig {
        name: "file_read".to_string(),
        generic_params: vec![],
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // file_write(path: str, content: str) -> i64 — write string to file (0=ok, -1=err)
    checker.env.define_function(FunctionSig {
        name: "file_write".to_string(),
        generic_params: vec![],
        params: vec![
            ("path".to_string(), Type::Str),
            ("content".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // env_get(key: str) -> str — get environment variable
    checker.env.define_function(FunctionSig {
        name: "env_get".to_string(),
        generic_params: vec![],
        params: vec![("key".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // time_now() -> i64 — Unix timestamp in seconds
    checker.env.define_function(FunctionSig {
        name: "time_now".to_string(),
        generic_params: vec![],
        params: vec![],
        ret: Type::I64,
    });

    // assert(condition: bool, msg: str) -> void — abort if condition is false
    checker.env.define_function(FunctionSig {
        name: "assert".to_string(),
        generic_params: vec![],
        params: vec![
            ("condition".to_string(), Type::Bool),
            ("msg".to_string(), Type::Str),
        ],
        ret: Type::Void,
    });

    // parse_int(s: str) -> i64 — parse string to integer (0 on failure)
    checker.env.define_function(FunctionSig {
        name: "parse_int".to_string(),
        generic_params: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // parse_float(s: str) -> f64 — parse string to float (0.0 on failure)
    checker.env.define_function(FunctionSig {
        name: "parse_float".to_string(),
        generic_params: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::F64,
    });

    // type_of(value: any) -> str — returns type name (always "i64" at runtime)
    checker.env.define_function(FunctionSig {
        name: "type_of".to_string(),
        generic_params: vec![],
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Str,
    });

    // char_code(c: str) -> i64 — Unicode code point of first character
    checker.env.define_function(FunctionSig {
        name: "char_code".to_string(),
        generic_params: vec![],
        params: vec![("c".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // char_from(n: i64) -> str — single-character string from code point
    checker.env.define_function(FunctionSig {
        name: "char_from".to_string(),
        generic_params: vec![],
        params: vec![("n".to_string(), Type::I64)],
        ret: Type::Str,
    });

    // substr(s: str, start: i64, end: i64) -> str — substring [start..end)
    checker.env.define_function(FunctionSig {
        name: "substr".to_string(),
        generic_params: vec![],
        params: vec![
            ("s".to_string(), Type::Str),
            ("start".to_string(), Type::I64),
            ("end".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // push(arr: any, val: any) -> any — append value to array
    checker.env.define_function(FunctionSig {
        name: "push".to_string(),
        generic_params: vec![],
        params: vec![
            ("arr".to_string(), Type::Error),
            ("val".to_string(), Type::Error),
        ],
        ret: Type::Error,
    });

    // pop(arr: any) -> any — remove and return last element
    checker.env.define_function(FunctionSig {
        name: "pop".to_string(),
        generic_params: vec![],
        params: vec![("arr".to_string(), Type::Error)],
        ret: Type::Error,
    });

    // int(x: any) -> i64 — convert to integer
    checker.env.define_function(FunctionSig {
        name: "int".to_string(),
        generic_params: vec![],
        params: vec![("x".to_string(), Type::Error)],
        ret: Type::I64,
    });

    // float(x: any) -> f64 — convert to float
    checker.env.define_function(FunctionSig {
        name: "float".to_string(),
        generic_params: vec![],
        params: vec![("x".to_string(), Type::Error)],
        ret: Type::F64,
    });

    // sqrt(x: f64) -> f64 — square root
    checker.env.define_function(FunctionSig {
        name: "sqrt".to_string(),
        generic_params: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // floor(x: f64) -> f64 — floor
    checker.env.define_function(FunctionSig {
        name: "floor".to_string(),
        generic_params: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // ceil(x: f64) -> f64 — ceiling
    checker.env.define_function(FunctionSig {
        name: "ceil".to_string(),
        generic_params: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // abs(x: any) -> any — absolute value
    checker.env.define_function(FunctionSig {
        name: "abs".to_string(),
        generic_params: vec![],
        params: vec![("x".to_string(), Type::Error)],
        ret: Type::Error,
    });

    // log(x: f64) -> f64 — natural logarithm
    checker.env.define_function(FunctionSig {
        name: "log".to_string(),
        generic_params: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // keys(m: any) -> [str] — get map keys
    checker.env.define_function(FunctionSig {
        name: "keys".to_string(),
        generic_params: vec![],
        params: vec![("m".to_string(), Type::Error)],
        ret: Type::Array {
            element: Box::new(Type::Str),
            size: None,
        },
    });

    // sleep_ms(ms: i64) -> void — sleep for milliseconds
    checker.env.define_function(FunctionSig {
        name: "sleep_ms".to_string(),
        generic_params: vec![],
        params: vec![("ms".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // ── Byte buffer builtins ──────────────────────────────────────

    // buf_new(capacity: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "buf_new".to_string(),
        generic_params: vec![],
        params: vec![("capacity".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // buf_write_byte(handle: i64, byte: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_byte".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("byte".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_i16_le(handle: i64, val: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_i16_le".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_i32_le(handle: i64, val: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_i32_le".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_i64_le(handle: i64, val: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_i64_le".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_bytes(dst: i64, src: i64, len: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_bytes".to_string(),
        generic_params: vec![],
        params: vec![
            ("dst".to_string(), Type::I64),
            ("src".to_string(), Type::I64),
            ("len".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_str(handle: i64, s: str) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_str".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("s".to_string(), Type::Str),
        ],
        ret: Type::Void,
    });

    // buf_write_zeros(handle: i64, count: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_zeros".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("count".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_len(handle: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "buf_len".to_string(),
        generic_params: vec![],
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // buf_get_byte(handle: i64, offset: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "buf_get_byte".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("offset".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // buf_set_byte(handle: i64, offset: i64, byte: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_set_byte".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("offset".to_string(), Type::I64),
            ("byte".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_patch_i32_le(handle: i64, offset: i64, val: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_patch_i32_le".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("offset".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_patch_i64_le(handle: i64, offset: i64, val: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_patch_i64_le".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("offset".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_to_file(handle: i64, path: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "buf_write_to_file".to_string(),
        generic_params: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("path".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // buf_free(handle: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_free".to_string(),
        generic_params: vec![],
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // exit(code: i64) -> void — terminate the process
    checker.env.define_function(FunctionSig {
        name: "exit".to_string(),
        generic_params: vec![],
        params: vec![("code".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // args() -> [str] — command-line arguments
    checker.env.define_function(FunctionSig {
        name: "args".to_string(),
        generic_params: vec![],
        params: vec![],
        ret: Type::Array {
            element: Box::new(Type::Str),
            size: None,
        },
    });

    checker.check_module(module);
    checker.diagnostics
}

// ── Missing return analysis ─────────────────────────────────────────

/// Returns `true` if the block is guaranteed to return a value on all
/// control flow paths. This is a conservative check — it may produce
/// false negatives (miss some paths) but never false positives.
fn block_returns(block: &Block) -> bool {
    if let Some(last) = block.stmts.last() {
        stmt_returns(last)
    } else {
        false
    }
}

fn stmt_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::Throw { .. } => true,
        Stmt::If {
            then_block,
            elif_clauses,
            else_block,
            ..
        } => {
            // All branches must return, including else.
            let then_ok = block_returns(then_block);
            let elifs_ok = elif_clauses.iter().all(|(_, b)| block_returns(b));
            let else_ok = else_block.as_ref().is_some_and(|b| block_returns(b));
            then_ok && elifs_ok && else_ok
        }
        Stmt::TryCatch {
            try_block,
            catch_block,
            ..
        } => block_returns(try_block) && block_returns(catch_block),
        // An expression statement at the end of a block can serve as an
        // implicit return value (expression-bodied functions).
        Stmt::Expr { .. } => true,
        _ => false,
    }
}

