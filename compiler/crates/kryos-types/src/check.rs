//! Type checking pass — walks the AST and validates types.
//!
//! Resolves `TypeExpr` → `Type`, infers types for unannotated let bindings,
//! checks function bodies, binary ops, calls, field access, etc.
//! Reports mismatches as `Diagnostic` errors.

use kryos_ast::{BinOp, Block, Decl, Expr, Module, Pattern, Stmt, TypeExpr, UnOp};
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
    /// The name of the function currently being checked (for better error messages).
    current_function_name: Option<String>,
    /// Functions marked with @deprecated — emit warnings on call.
    deprecated_functions: std::collections::HashSet<String>,
    /// Functions marked with @pure — cannot call non-pure or do I/O.
    pure_functions: std::collections::HashSet<String>,
    /// Names declared as ACTORS. Actors register as struct-like types (so
    /// `self.field` type-checks in handlers), which wrongly let the
    /// struct-literal form `Counter { count: 0 }` type-check -- the LLVM
    /// backend cannot compile it, and actor state is private by design.
    actor_names: std::collections::HashSet<String>,
    /// Whether we are currently inside a @pure function body.
    in_pure_function: bool,
    /// The current `Self` type — set when checking trait/impl blocks.
    current_self_type: Option<Type>,
    /// Maps generic type variable id → list of trait bound names. Populated
    /// when entering a generic function with bounds (e.g. `<T: Showable>`).
    /// Used in MethodCall resolution: when `obj_ty` resolves to a Type::Var
    /// that has registered bounds, the method is looked up on the bound
    /// traits' method signatures so `x.show()` typechecks inside
    /// `fn announce<T: Showable>(x: T)`.
    generic_var_bounds: std::collections::HashMap<u32, Vec<String>>,
    /// Bidirectional inference: pushed expected types for lambda args,
    /// keyed by the lambda's span. Set by `Expr::FnCall` before inferring
    /// a Lambda arg whose corresponding param is a Function type; consumed
    /// by `Expr::Lambda` to pre-unify un-annotated params and return.
    lambda_expected_types: std::collections::HashMap<Span, (Vec<Type>, Type)>,
    /// Resolved concrete types for each lambda's UN-annotated parameters,
    /// keyed by the lambda's span. Recorded after a lambda is checked so the
    /// MIR lowering can type the closure's params (otherwise it defaults them
    /// to i64, miscompiling e.g. a `str` closure passed to a higher-order fn).
    /// Per param: Some(TypeExpr) for an inferred un-annotated param, None when
    /// the param was annotated or its type stayed unresolved.
    resolved_lambda_params: std::collections::HashMap<Span, Vec<Option<TypeExpr>>>,
    /// Resolved type of an unannotated `let X = []` (empty-array) binding, keyed
    /// by the Let span. The element type is only known after later `push(X, v)`
    /// unification, so this is resolved at end-of-check. The MIR consumes it to
    /// type the local (its own inference defaults an empty array's element to
    /// i64, which mis-types `X[i].field` / aggregate elements on AOT).
    resolved_let_types: std::collections::HashMap<Span, Type>,
    /// Nesting depth of enclosing `unsafe { }` blocks. Raw-pointer dereference
    /// is only permitted when this is > 0 (else E0500).
    unsafe_depth: u32,
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
            current_function_name: None,
            deprecated_functions: std::collections::HashSet::new(),
            pure_functions: std::collections::HashSet::new(),
            actor_names: std::collections::HashSet::new(),
            in_pure_function: false,
            current_self_type: None,
            generic_var_bounds: std::collections::HashMap::new(),
            lambda_expected_types: std::collections::HashMap::new(),
            resolved_lambda_params: std::collections::HashMap::new(),
            resolved_let_types: std::collections::HashMap::new(),
            unsafe_depth: 0,
        }
    }

    /// Report an error diagnostic.
    fn error(&mut self, msg: impl Into<String>, span: Span) {
        // Uncategorized type errors still get a code so every diagnostic is
        // explainable via `kryos explain`. Specific cases use error_with_code.
        self.diagnostics.push(
            Diagnostic::error(msg)
                .with_label(span, "here")
                .with_code(kryos_errors::codes::E0110),
        );
    }

    /// Report an error diagnostic with an error code.
    fn error_with_code(&mut self, msg: impl Into<String>, span: Span, code: &str) {
        self.diagnostics.push(
            Diagnostic::error(msg)
                .with_label(span, "here")
                .with_code(code),
        );
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
                // Resolve `Self` to the current impl/trait target type.
                if name == "Self" {
                    return if let Some(ref self_ty) = self.current_self_type {
                        self_ty.clone()
                    } else {
                        self.error_with_code(
                            "`Self` used outside of impl or trait block",
                            *span,
                            kryos_errors::codes::E0109,
                        );
                        Type::Error
                    };
                }
                if name == "Map" || name == "map" {
                    // Bare `map`/`Map`: create a map with fresh type variables
                    Type::Map {
                        key: Box::new(self.engine.fresh_var()),
                        value: Box::new(self.engine.fresh_var()),
                    }
                } else if let Some(ty) = Type::from_name(name) {
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
                        self.error_with_code(
                            format!("unknown type `{name}`"),
                            *span,
                            kryos_errors::codes::E0101,
                        );
                        let known = self.env.all_type_names();
                        if let Some(suggestion) =
                            crate::suggest::closest_match(name, known.iter().map(|s| s.as_str()))
                        {
                            if let Some(diag) = self.diagnostics.last_mut() {
                                diag.notes.push(format!("did you mean `{suggestion}`?"));
                            }
                        }
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
                    // The builtin `Map`/`Set` type sugar only applies when no
                    // user-defined struct of that name is in scope. This lets
                    // std::collections ship real generic `Set<T>` / `Map<..>`
                    // structs that shadow the builtin sugar (the `_` arm below
                    // resolves them as ordinary user structs). Bare `map<K,V>`
                    // literals keep working because nobody defines a struct
                    // named `map`.
                    "Map" | "map" if self.env.lookup_struct(name).is_none() => {
                        if resolved_args.len() == 2 {
                            Type::Map {
                                key: Box::new(resolved_args[0].clone()),
                                value: Box::new(resolved_args[1].clone()),
                            }
                        } else {
                            self.error("map expects exactly 2 type arguments", *span);
                            Type::Error
                        }
                    }
                    // chan<T> — channels are opaque i64 handles at runtime.
                    "chan" => Type::I64,
                    "Set" | "set" if self.env.lookup_struct(name).is_none() => {
                        if resolved_args.len() == 1 {
                            Type::Set {
                                element: Box::new(resolved_args[0].clone()),
                            }
                        } else {
                            self.error("set expects exactly 1 type argument", *span);
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
            TypeExpr::Tuple { elements, span: _ } => Type::Tuple {
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
                    self.error_with_code(
                        format!("unknown trait `{trait_name}`"),
                        *span,
                        kryos_errors::codes::E0105,
                    );
                    return Type::Error;
                }
                Type::DynTrait {
                    trait_name: trait_name.clone(),
                }
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
        // Pass 0: pre-register all struct/enum NAMES so self-referential
        // types (e.g. `struct Expr { left: [Expr] }`) can resolve.
        for decl in &module.declarations {
            self.pre_register_type_name(decl);
        }
        // First pass: register all type and function declarations.
        for decl in &module.declarations {
            self.register_decl(decl);
            // Track annotated functions.
            if let Decl::Function {
                name, annotations, ..
            } = decl
            {
                for ann in annotations {
                    match ann.name.as_str() {
                        "deprecated" => {
                            self.deprecated_functions.insert(name.clone());
                        }
                        "pure" => {
                            self.pure_functions.insert(name.clone());
                        }
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

    /// Pre-register struct/enum names with empty fields so self-referential
    /// types resolve during the full registration pass.
    fn pre_register_type_name(&mut self, decl: &Decl) {
        match decl {
            Decl::Struct { name, generics, .. } => {
                self.env.define_struct(StructDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    generic_var_ids: vec![],
                    fields: vec![],
                });
            }
            Decl::Enum { name, generics, .. } => {
                self.env.define_enum(EnumDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    generic_var_ids: vec![],
                    variants: vec![],
                });
            }
            // An actor is, to the type system, a struct-like handle: its name is
            // a type and a zero-arg constructor (`Counter()` spawns it). Register
            // the name here so it resolves during the full pass.
            Decl::Actor { name, .. } => {
                self.actor_names.insert(name.clone());
                self.env.define_struct(StructDef {
                    name: name.clone(),
                    generic_params: vec![],
                    generic_var_ids: vec![],
                    fields: vec![],
                });
            }
            _ => {}
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
                // Capture the type variable IDs so we can instantiate fresh copies
                // at each call site (prevents generic pinning bug).
                // Also register trait bounds keyed by the *sig* var IDs — these
                // are the IDs that show up in parameter types when the function
                // body is checked, so MethodCall bound-resolution must see them.
                let mut generic_var_ids = Vec::new();
                if !generics.is_empty() {
                    self.env.push_scope();
                    for gp in generics {
                        let tv = self.engine.fresh_var();
                        if let Type::Var(id) = &tv {
                            generic_var_ids.push(*id);
                            if !gp.bounds.is_empty() {
                                self.generic_var_bounds
                                    .insert(*id, gp.bounds.clone());
                            }
                        }
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }

                let param_types: Vec<(String, Type)> = params
                    .iter()
                    .map(|p| {
                        let ty =
                            p.ty.as_ref()
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
                    generic_var_ids,
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
                let mut generic_var_ids = Vec::new();
                if !generics.is_empty() {
                    self.env.push_scope();
                    for gp in generics {
                        let tv = self.engine.fresh_var();
                        if let Type::Var(id) = &tv {
                            generic_var_ids.push(*id);
                        }
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
                    generic_var_ids,
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
                let mut generic_var_ids = Vec::new();
                if !generics.is_empty() {
                    self.env.push_scope();
                    for gp in generics {
                        let tv = self.engine.fresh_var();
                        if let Type::Var(id) = &tv {
                            generic_var_ids.push(*id);
                        }
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
                    generic_var_ids,
                    variants: variant_types,
                });
            }
            Decl::Impl {
                target,
                trait_name,
                methods,
                generics: impl_generics,
                ..
            } => {
                // Set Self type for the duration of this impl block registration.
                let prev_self = self.current_self_type.take();

                // Bring the impl's own type parameters (`impl<T> Box<T>`) into
                // scope as fresh type vars so method signatures that mention
                // them (`self: Box<T>`, `-> T`) resolve instead of raising
                // E0101. Concrete impls (`impl Box<i64>`) have no generics, so
                // this is a no-op for them.
                let scoped_impl_generics = !impl_generics.is_empty();
                // Capture the impl's generic var ids so each method signature
                // can record them. Without this the sigs carry
                // `generic_var_ids: vec![]` and are treated as non-generic:
                // `instantiate_sig` skips freshening, so the impl's return-type
                // var is SHARED across every call site. Calling `.get()` on
                // Box<str> then `let x: f64 = box_f64.get()` then failed with
                // "expected f64, found str" -- cross-instantiation
                // contamination through the shared var.
                let mut impl_generic_var_ids: Vec<u32> = Vec::new();
                if scoped_impl_generics {
                    self.env.push_scope();
                    for gp in impl_generics {
                        let tv = self.engine.fresh_var();
                        if let Type::Var(id) = &tv {
                            impl_generic_var_ids.push(*id);
                            if !gp.bounds.is_empty() {
                                self.generic_var_bounds.insert(*id, gp.bounds.clone());
                            }
                        }
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }

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

                self.current_self_type = impl_target_ty.clone();

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
                                        p.ty.as_ref()
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
                                // Record the impl's generic vars so per-call
                                // instantiation freshens them (fixes the shared-
                                // var contamination). Vars the method does not
                                // mention are a no-op during instantiation.
                                generic_var_ids: impl_generic_var_ids.clone(),
                                params: param_types,
                                ret,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                // Impl methods are NOT registered as global functions.
                // They live only in the `impls` map and are looked up via
                // `lookup_method`. The per-impl-method check site below
                // pushes a scope and defines the method signature there,
                // so `check_decl(Function)` can find the right param types
                // (including `self`) while checking the method body.
                //
                // This avoids polluting the global namespace: a stdlib impl
                // method named `push` no longer shadows the `push` builtin
                // for unrelated call sites — including call sites inside
                // sibling impl method bodies.
                let _ = &method_sigs;

                // Pop the impl-generic scope NOW: the method signatures are
                // fully resolved (their Var ids persist in the type engine),
                // and `define_impl` below registers into the *current* scope.
                // If we popped after define_impl, the registration would land
                // in this temporary scope and be discarded -- the method would
                // then be "not found" at every call site.
                if scoped_impl_generics {
                    self.env.pop_scope();
                }

                // If this is a trait impl, inherit default methods from the
                // trait that are not explicitly overridden in this impl block.
                let mut all_method_sigs = method_sigs.clone();
                if let Some(ref tname) = trait_name {
                    if let Some(trait_def) = self.env.lookup_trait(tname).cloned() {
                        let explicit_names: std::collections::HashSet<&str> =
                            method_sigs.iter().map(|s| s.name.as_str()).collect();
                        for trait_method in &trait_def.methods {
                            if !explicit_names.contains(trait_method.name.as_str()) {
                                // Rewrite the `self` parameter type to the concrete
                                // impl target type.
                                let rewritten_params: Vec<(String, Type)> = trait_method
                                    .params
                                    .iter()
                                    .map(|(pname, pty)| {
                                        if pname == "self" {
                                            (
                                                pname.clone(),
                                                impl_target_ty
                                                    .clone()
                                                    .unwrap_or_else(|| pty.clone()),
                                            )
                                        } else {
                                            (pname.clone(), pty.clone())
                                        }
                                    })
                                    .collect();
                                let default_sig = FunctionSig {
                                    name: trait_method.name.clone(),
                                    generic_params: trait_method.generic_params.clone(),
                                    generic_var_ids: trait_method.generic_var_ids.clone(),
                                    params: rewritten_params,
                                    ret: trait_method.ret.clone(),
                                };
                                // Inherited trait defaults: also kept out of
                                // the global function table; available via
                                // method/static-method lookup only.
                                all_method_sigs.push(default_sig);
                            }
                        }
                    }
                }

                self.env
                    .define_impl(target.clone(), trait_name.clone(), all_method_sigs);
                // Record the (type, trait) pair for `dyn Trait` coercion
                // verification in unify (backlog #18/#34).
                if let Some(ref tname) = trait_name {
                    self.engine
                        .register_trait_impl(target.clone(), tname.clone());
                }

                // (impl-generic scope already popped above, before define_impl)
                self.current_self_type = prev_self;
            }
            Decl::Trait {
                name,
                generics,
                methods,
                ..
            } => {
                // Set Self to DynTrait so `self: Self` in trait method
                // signatures resolves correctly.
                let prev_self = self.current_self_type.take();
                self.current_self_type = Some(Type::DynTrait {
                    trait_name: name.clone(),
                });

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
                                    let ty =
                                        p.ty.as_ref()
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
                                generic_var_ids: vec![],
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

                self.current_self_type = prev_self;
            }
            Decl::TypeAlias { name, ty, .. } => {
                let resolved = self.resolve_type_expr(ty);
                // Register as a variable in the type namespace for lookup.
                self.env.define_var(name.clone(), resolved);
            }
            Decl::Const {
                name,
                ty,
                value,
                mutable,
                span,
                ..
            } => {
                let resolved_ty = if let Some(t) = ty {
                    let decl_ty = self.resolve_type_expr(t);
                    // Check the value against the annotation (mirrors the
                    // local Stmt::Let path). Previously skipped entirely, so
                    // `let X: str = 42` passed the checker and produced a
                    // garbage value at runtime. Forward-ref guard: a value
                    // calling a function declared LATER in the file infers
                    // Error (with a spurious E0102) because registration is
                    // sequential — suppress those diagnostics and skip the
                    // unify, preserving the previously-accepted pattern (the
                    // unannotated branch has the same limitation).
                    let diags_before = self.diagnostics.len();
                    let inferred_ty = self.infer_expr(value);
                    if matches!(self.engine.resolve(&inferred_ty), Type::Error) {
                        self.diagnostics.truncate(diags_before);
                    } else if let Err(diag) = self.engine.unify(&decl_ty, &inferred_ty, *span) {
                        self.diagnostics.push(diag);
                    }
                    self.check_int_literal_range(&decl_ty, value, *span);
                    decl_ty
                } else {
                    self.infer_expr(value)
                };
                if *mutable {
                    self.env.define_var_mut(name.clone(), resolved_ty);
                } else {
                    self.env.define_var(name.clone(), resolved_ty);
                }
            }
            Decl::Actor {
                name,
                state_fields,
                handlers,
                ..
            } => {
                self.actor_names.insert(name.clone());
                // Register the actor as a struct-like type with its state fields
                // (so `self.field` in handlers type-checks).
                let field_types: Vec<(String, Type)> = state_fields
                    .iter()
                    .map(|f| (f.name.clone(), self.resolve_type_expr(&f.ty)))
                    .collect();
                self.env.define_struct(StructDef {
                    name: name.clone(),
                    generic_params: vec![],
                    generic_var_ids: vec![],
                    fields: field_types,
                });
                let actor_ty = Type::Struct {
                    name: name.clone(),
                    generics: vec![],
                };
                // `Counter()` -- zero-arg constructor spawning the actor.
                self.env.define_function(FunctionSig {
                    name: name.clone(),
                    generic_params: vec![],
                    generic_var_ids: vec![],
                    params: vec![],
                    ret: actor_ty.clone(),
                });
                // Handlers become methods `c.handler(msg_args)`; sends are
                // fire-and-forget so the observable return is unit.
                let method_sigs: Vec<FunctionSig> = handlers
                    .iter()
                    .map(|h| {
                        let params: Vec<(String, Type)> = h
                            .params
                            .iter()
                            .map(|p| {
                                if p.name == "self" {
                                    (p.name.clone(), actor_ty.clone())
                                } else {
                                    let ty = p
                                        .ty
                                        .as_ref()
                                        .map(|t| self.resolve_type_expr(t))
                                        .unwrap_or_else(|| self.engine.fresh_var());
                                    (p.name.clone(), ty)
                                }
                            })
                            .collect();
                        FunctionSig {
                            name: h.name.clone(),
                            generic_params: vec![],
                            generic_var_ids: vec![],
                            params,
                            ret: Type::Void,
                        }
                    })
                    .collect();
                self.env.define_impl(name.clone(), None, method_sigs);
            }
            Decl::Import { .. } => {}
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
                // Record trait bounds so method calls on this type var can be
                // resolved through the bound trait's method signatures.
                let mut bound_var_ids: Vec<u32> = Vec::new();
                for gp in generics {
                    let tv = self.engine.fresh_var();
                    if let Type::Var(id) = &tv {
                        if !gp.bounds.is_empty() {
                            self.generic_var_bounds.insert(*id, gp.bounds.clone());
                            bound_var_ids.push(*id);
                        }
                    }
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
                    self.current_return_type = ret_ty.as_ref().map(|t| self.resolve_type_expr(t));
                }

                self.current_function_name = Some(name.clone());
                self.check_block(body);
                self.current_function_name = None;

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
                // Clear trait bounds for this function's generic vars so they
                // don't leak into the next function's checking.
                for id in &bound_var_ids {
                    self.generic_var_bounds.remove(id);
                }
                self.env.pop_scope();

                let _ = span; // suppress unused warning
            }
            Decl::Impl {
                target,
                methods,
                generics: impl_generics,
                ..
            } => {
                // Set Self type for the duration of this impl block.
                let prev_self = self.current_self_type.take();

                // Scope the impl's type parameters so method bodies that
                // mention them (`let x: T = ...`, `-> T` in the fallback
                // path) resolve. No-op for concrete impls.
                let scoped_impl_generics = !impl_generics.is_empty();
                if scoped_impl_generics {
                    self.env.push_scope();
                    for gp in impl_generics {
                        let tv = self.engine.fresh_var();
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }

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

                self.current_self_type = target_ty.clone();

                for method in methods {
                    if let (
                        Some(ref ty),
                        Decl::Function {
                            name: mname,
                            body: Some(body),
                            params: m_params,
                            ret_ty: m_ret,
                            generics: m_generics,
                            ..
                        },
                    ) = (&target_ty, method)
                    {
                        // Check the method body inline, binding `self` and
                        // the rest of the params directly from the impl's
                        // signature — without registering the method as a
                        // global function (which would shadow same-named
                        // builtins inside the body).
                        self.env.push_scope();
                        self.env.define_var("self".to_string(), ty.clone());

                        // Bind generics so param/return types resolve.
                        for gp in m_generics {
                            let tv = self.engine.fresh_var();
                            self.env.define_var(gp.name.clone(), tv);
                        }

                        // Bind non-self params using the impl method's
                        // recorded signature when available (preserves the
                        // exact types used elsewhere), otherwise resolve
                        // them directly from the AST.
                        if let Some(sig) = self.env.lookup_method(target, mname).cloned() {
                            for (pname, pty) in &sig.params {
                                if pname == "self" {
                                    continue;
                                }
                                self.env.define_var(pname.clone(), pty.clone());
                            }
                            let prev_ret = self.current_return_type.take();
                            self.current_return_type = Some(sig.ret.clone());
                            let prev_fn = self.current_function_name.take();
                            self.current_function_name = Some(mname.clone());
                            self.check_block(body);
                            self.current_function_name = prev_fn;
                            self.current_return_type = prev_ret;
                        } else {
                            for param in m_params {
                                if param.name == "self" {
                                    continue;
                                }
                                let ty = param
                                    .ty
                                    .as_ref()
                                    .map(|t| self.resolve_type_expr(t))
                                    .unwrap_or_else(|| self.engine.fresh_var());
                                self.env.define_var(param.name.clone(), ty);
                            }
                            let prev_ret = self.current_return_type.take();
                            self.current_return_type =
                                m_ret.as_ref().map(|t| self.resolve_type_expr(t));
                            let prev_fn = self.current_function_name.take();
                            self.current_function_name = Some(mname.clone());
                            self.check_block(body);
                            self.current_function_name = prev_fn;
                            self.current_return_type = prev_ret;
                        }

                        self.env.pop_scope();
                    } else {
                        self.check_decl(method);
                    }
                }

                if scoped_impl_generics {
                    self.env.pop_scope();
                }
                self.current_self_type = prev_self;
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

    /// Like `check_block`, but returns the type of the tail expression if the
    /// last statement is an `Expr` statement (i.e. the block is used as a value).
    /// Returns `Type::Void` if the block is empty or ends with a non-expr statement.
    fn infer_block_as_expr(&mut self, block: &Block) -> Type {
        let n = block.stmts.len();
        for (i, stmt) in block.stmts.iter().enumerate() {
            if i == n - 1 {
                if let Stmt::Expr { expr, .. } = stmt {
                    return self.infer_expr(expr);
                }
                // An if-else as the last statement can produce a value just like
                // an if expression.  We infer the branch types without emitting
                // diagnostics for the stmt itself here (check_stmt does that).
                if let Stmt::If {
                    condition,
                    then_block,
                    elif_clauses,
                    else_block: Some(else_blk),
                    span,
                } = stmt
                {
                    let cond_ty = self.infer_expr(condition);
                    if let Err(diag) = self.engine.unify(&Type::Bool, &cond_ty, *span) {
                        self.diagnostics.push(diag);
                    }
                    self.env.push_scope();
                    let mut branch_ty = self.infer_block_as_expr(then_block);
                    self.env.pop_scope();
                    // Walk elif clauses; their branch type must match.
                    for (elif_cond, elif_block) in elif_clauses {
                        let ec_ty = self.infer_expr(elif_cond);
                        if let Err(diag) = self.engine.unify(&Type::Bool, &ec_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                        self.env.push_scope();
                        let elif_ty = self.infer_block_as_expr(elif_block);
                        self.env.pop_scope();
                        if let Err(diag) = self.engine.unify(&branch_ty, &elif_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                    }
                    self.env.push_scope();
                    let else_ty = self.infer_block_as_expr(else_blk);
                    self.env.pop_scope();
                    if let Err(diag) = self.engine.unify(&branch_ty, &else_ty, *span) {
                        self.diagnostics.push(diag);
                        // If unification fails return the else type so the outer
                        // expression still gets a concrete (non-void) type.
                        branch_ty = else_ty;
                    }
                    return branch_ty;
                }
            }
            self.check_stmt(stmt);
        }
        Type::Void
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                mutable,
                ty,
                value,
                span,
                pattern,
            } => {
                let declared_ty = ty.as_ref().map(|t| self.resolve_type_expr(t));
                let inferred_ty = value.as_ref().map(|v| self.infer_expr(v));

                let final_ty = match (declared_ty, inferred_ty) {
                    (Some(decl), Some(inferred)) => {
                        // Both declared and inferred: unify them.
                        if let Err(diag) = self.engine.unify(&decl, &inferred, *span) {
                            self.diagnostics.push(diag);
                        }
                        if let Some(v) = value.as_ref() {
                            self.check_int_literal_range(&decl, v, *span);
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

                // Record unannotated empty-array bindings so the MIR can type
                // them from the element type unified by later `push` calls.
                if ty.is_none() {
                    if let Some(Expr::ArrayLiteral { elements, .. }) = value.as_ref() {
                        if elements.is_empty() {
                            self.resolved_let_types.insert(*span, final_ty.clone());
                        }
                    }
                }

                if let Some(pat) = pattern {
                    // Tuple / struct destructuring: bind each variable in the pattern.
                    // Pass through the outer `let mut` so `let mut (a, b) = ...`
                    // makes both bindings mutable, and `let (mut a, b) = ...`
                    // makes only `a` mutable via the per-element `mut` flag
                    // inside bind_pattern_with_mut.
                    self.bind_pattern_with_mut(pat, &final_ty, *mutable);
                } else if *mutable {
                    self.env.define_var_mut(name.clone(), final_ty);
                } else {
                    self.env.define_var(name.clone(), final_ty);
                }
            }
            Stmt::Assign {
                target,
                value,
                span,
                ..
            } => {
                let target_ty = self.infer_expr(target);
                let value_ty = self.infer_expr(value);
                self.check_int_literal_range(&target_ty, value, *span);
                if let Err(diag) = self.engine.unify(&target_ty, &value_ty, *span) {
                    self.diagnostics.push(diag);
                }
                // Enforce immutability: only `let mut` variables can be
                // reassigned. This is an ERROR (E0302), matching CLAUDE.md and
                // `kryos explain E0302` — it was previously only a warning, so
                // immutability was silently unenforced (backlog #113).
                if let Expr::Identifier { name, .. } = target {
                    if !self.env.is_mutable(name) {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "assignment to immutable variable `{name}`"
                            ))
                            .with_label(*span, "help: consider declaring with `let mut`")
                            .with_code(kryos_errors::codes::E0302),
                        );
                    }
                }
            }
            Stmt::Return { value, span } => {
                let ret_ty = value
                    .as_ref()
                    .map(|v| self.infer_expr(v))
                    .unwrap_or(Type::Void);

                if let Some(ref expected) = self.current_return_type {
                    if let Err(mut diag) = self.engine.unify(expected, &ret_ty, *span) {
                        // Enhance error message with function context.
                        if let Some(ref fn_name) = self.current_function_name {
                            let msg = format!(
                                "function `{fn_name}` declared to return `{expected}`, but body evaluates to `{ret_ty}`"
                            );
                            diag = Diagnostic::error(msg).with_label(
                                *span,
                                format!("expected `{expected}`, found `{ret_ty}`"),
                            );
                        }
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
                    _ => Type::I64, // default for range() and other builtins
                };

                self.env.push_scope();

                // Bind the loop pattern into scope. Was Ident-only, so
                // `for (a, b) in pairs` left a/b undefined (E0102); route
                // through bind_pattern to destructure tuples/structs/enums.
                self.bind_pattern(pattern, &elem_ty);

                self.check_block(body);
                self.env.pop_scope();
            }
            Stmt::Expr { expr, span } => {
                let ty = self.infer_expr(expr);
                // must-use lint (W0400): discarding a `Tracked<T>` value as a
                // statement silently drops its provenance/lineage chain. The
                // tracked stdlib fns are functional (each returns a NEW Tracked),
                // so ignoring the result loses the audit trail. A non-void fn must
                // `return`, so a bare `Stmt::Expr` of Tracked type is always a real
                // discard, never an implicit return. `tracked_discard(t, reason)`
                // is the explicit opt-out (it returns the inner, non-Tracked value).
                //
                // Known limitation: keyed on the struct NAME only (Type::Struct has no
                // module origin), so a user struct literally named `Tracked` would also
                // trip this. Acceptable for v1 -- the name is distinctive and this is a
                // warning, not an error. A module-origin guard is the proper fix later.
                if matches!(&ty, Type::Struct { name, .. } if name == "Tracked") {
                    self.diagnostics.push(
                        Diagnostic::warning(
                            "tracked value is discarded; its provenance/lineage is lost. \
                             Use the value, or call `tracked_discard(t, reason)` to drop it explicitly.",
                        )
                        .with_label(*span, "tracked value dropped here")
                        .with_code(kryos_errors::codes::W0400),
                    );
                }
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
                self.env.define_var(catch_name.clone(), Type::Str);
                self.check_block(catch_block);
                self.env.pop_scope();
            }
            Stmt::Spawn { expr, .. } => {
                self.infer_expr(expr);
            }
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Select { .. } => {}
            // deny!(...) { body } is a compile-time capability-narrowing wrapper;
            // for type-checking it is just a scoped block of statements.
            Stmt::DenyBlock { body, .. } => {
                self.env.push_scope();
                self.check_block(body);
                self.env.pop_scope();
            }
        }
    }

    // ── Pattern binding ──────────────────────────────────────────────

    /// Bind variables introduced by a pattern into the current scope.
    ///
    /// For example, `Option::Some(val)` binds `val` to a fresh type variable.
    /// `Ident` patterns bind the name to the subject type.
    /// Wildcards and literals bind nothing.
    ///
    /// `outer_mut` is true when the binding site (`let mut (...)` /
    /// match arm under a `mut` binding) declared the whole pattern
    /// mutable.  Per-element `mut` inside the pattern
    /// (`let (mut a, b) = ...`) is also honored — the binding is
    /// mutable if either source says so.
    fn bind_pattern(&mut self, pattern: &Pattern, subject_ty: &Type) {
        self.bind_pattern_with_mut(pattern, subject_ty, false);
    }

    fn bind_pattern_with_mut(
        &mut self,
        pattern: &Pattern,
        subject_ty: &Type,
        outer_mut: bool,
    ) {
        match pattern {
            Pattern::Wildcard { .. } | Pattern::Literal { .. } => {}
            Pattern::Ident { name, mutable, .. } => {
                if outer_mut || *mutable {
                    self.env.define_var_mut(name.clone(), subject_ty.clone());
                } else {
                    self.env.define_var(name.clone(), subject_ty.clone());
                }
            }
            Pattern::Tuple { elements, .. } => {
                // Resolve the subject type so we can extract element types.
                let resolved = self.engine.resolve(subject_ty);
                let elem_tys: Vec<Type> = if let Type::Tuple { elements: ts } = &resolved {
                    ts.clone()
                } else {
                    vec![]
                };
                for (i, elem) in elements.iter().enumerate() {
                    let elem_ty = elem_tys
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| self.engine.fresh_var());
                    self.bind_pattern_with_mut(elem, &elem_ty, outer_mut);
                }
            }
            Pattern::Struct { fields, .. } => {
                for (_field_name, pat) in fields {
                    let tv = self.engine.fresh_var();
                    self.bind_pattern_with_mut(pat, &tv, outer_mut);
                }
            }
            Pattern::Enum {
                name,
                variant,
                fields,
                ..
            } => {
                // Resolve the subject first so type variables don't slip through.
                let resolved_subject = self.engine.resolve(subject_ty);
                // Option<T> / Result<T,E> are DISTINCT Type variants from
                // Type::Enum — derive their payload types directly from the
                // generic arguments. Without this they fall to the enum-name
                // lookup below with name "", binding the payload to a fresh
                // ?Tn (so `Some(u) => u.field` failed to type-check).
                let field_types: Vec<Type> = match (&resolved_subject, variant.as_str()) {
                    (Type::Option { inner }, "Some") => vec![(**inner).clone()],
                    (Type::Option { .. }, "None") => vec![],
                    (Type::Result { ok, .. }, "Ok") => vec![(**ok).clone()],
                    (Type::Result { err, .. }, "Err") => vec![(**err).clone()],
                    _ => {
                        // Bare (unqualified) variant patterns carry an empty
                        // enum name; resolve it from the matched subject type
                        // when it is an enum.
                        let resolved_name = if name.is_empty() {
                            match &resolved_subject {
                                Type::Enum { name: n, .. } => n.clone(),
                                _ => name.clone(),
                            }
                        } else {
                            name.clone()
                        };
                        // Look up the enum variant's field types by variant name.
                        if let Some(edef) = self.env.lookup_enum(&resolved_name).cloned() {
                            edef.variants
                                .iter()
                                .find(|(v, _)| v == variant)
                                .map(|(_, tys)| tys.clone())
                                .unwrap_or_default()
                        } else {
                            vec![]
                        }
                    }
                };

                for (i, pat) in fields.iter().enumerate() {
                    let field_ty = field_types
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| self.engine.fresh_var());
                    self.bind_pattern_with_mut(pat, &field_ty, outer_mut);
                }
            }
            Pattern::Or { patterns, .. } => {
                for pat in patterns {
                    self.bind_pattern_with_mut(pat, subject_ty, outer_mut);
                }
            }
        }
    }

    // ── Expression type inference ────────────────────────────────────

    /// Infer the type of an expression.
    /// If `expr` is an integer literal (or negated literal) and `declared`
    /// resolves to a narrow integer type, reject values outside the type's
    /// range. Without this, `let x: u8 = 999` silently truncated to 231 —
    /// silent data corruption at the source level. Explicit casts
    /// (`999 as u8`) keep their documented truncation semantics.
    fn check_int_literal_range(&mut self, declared: &Type, expr: &Expr, span: Span) {
        let (min, max): (i128, i128) = match self.engine.resolve(declared) {
            Type::I8 => (i8::MIN as i128, i8::MAX as i128),
            Type::I16 => (i16::MIN as i128, i16::MAX as i128),
            Type::I32 => (i32::MIN as i128, i32::MAX as i128),
            Type::U8 => (0, u8::MAX as i128),
            Type::U16 => (0, u16::MAX as i128),
            Type::U32 => (0, u32::MAX as i128),
            Type::U64 => (0, u64::MAX as i128),
            _ => return,
        };
        // Unsigned targets have min == 0; only they may hold values above
        // i64::MAX. A bare integer literal is stored as an i64 bit-pattern
        // (the parser bit-casts u64 values above i64::MAX to a negative i64),
        // so for an unsigned target reinterpret those bits as unsigned —
        // otherwise u64::MAX reads back as -1 and is wrongly rejected. A
        // NEGATED literal (`-5`) stays negative and is still rejected for
        // unsigned types, as intended.
        let unsigned_target = min == 0;
        let value: Option<i128> = match expr {
            Expr::IntLiteral { value, .. } => {
                if unsigned_target && *value < 0 {
                    Some(*value as u64 as i128)
                } else {
                    Some(*value as i128)
                }
            }
            Expr::UnaryOp {
                op: UnOp::Neg,
                operand,
                ..
            } => match operand.as_ref() {
                Expr::IntLiteral { value, .. } => Some(-(*value as i128)),
                _ => None,
            },
            _ => None,
        };
        if let Some(v) = value {
            if v < min || v > max {
                let ty_name = format!("{:?}", self.engine.resolve(declared)).to_lowercase();
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "integer literal `{v}` is out of range for `{ty_name}` (valid range: {min}..={max})"
                    ))
                    .with_label(span, "out-of-range literal")
                    .with_note(format!(
                        "use a wider type, or `as {ty_name}` if truncation is intended"
                    ))
                    .with_code(kryos_errors::codes::E0111),
                );
            }
        }
    }

    /// Substitute generic-param type variables in a struct field's type with
    /// the instance's concrete generics (StructDef.generic_var_ids are 1:1
    /// with the decl's generic params; instance generics are positional).
    /// `Boxed<str>.value` resolves to `str`, not the registration-time var.
    fn substitute_struct_generics(&mut self, name: &str, instance_generics: &[Type], fty: &Type) -> Type {
        if instance_generics.is_empty() {
            return fty.clone();
        }
        let Some(def) = self.env.lookup_struct(name) else {
            return fty.clone();
        };
        if def.generic_var_ids.is_empty() {
            return fty.clone();
        }
        let map: std::collections::HashMap<u32, Type> = def
            .generic_var_ids
            .iter()
            .copied()
            .zip(instance_generics.iter().cloned())
            .collect();
        fn walk(t: &Type, map: &std::collections::HashMap<u32, Type>) -> Type {
            match t {
                Type::Var(id) => map.get(id).cloned().unwrap_or_else(|| t.clone()),
                Type::Array { element, size } => Type::Array {
                    element: Box::new(walk(element, map)),
                    size: *size,
                },
                Type::Tuple { elements } => Type::Tuple {
                    elements: elements.iter().map(|e| walk(e, map)).collect(),
                },
                Type::Map { key, value } => Type::Map {
                    key: Box::new(walk(key, map)),
                    value: Box::new(walk(value, map)),
                },
                Type::Option { inner } => Type::Option {
                    inner: Box::new(walk(inner, map)),
                },
                Type::Result { ok, err } => Type::Result {
                    ok: Box::new(walk(ok, map)),
                    err: Box::new(walk(err, map)),
                },
                Type::Struct { name, generics } => Type::Struct {
                    name: name.clone(),
                    generics: generics.iter().map(|g| walk(g, map)).collect(),
                },
                Type::Enum { name, generics } => Type::Enum {
                    name: name.clone(),
                    generics: generics.iter().map(|g| walk(g, map)).collect(),
                },
                Type::Function { params, ret } => Type::Function {
                    params: params.iter().map(|p| walk(p, map)).collect(),
                    ret: Box::new(walk(ret, map)),
                },
                Type::Reference { inner, mutable } => Type::Reference {
                    inner: Box::new(walk(inner, map)),
                    mutable: *mutable,
                },
                Type::Shared { inner } => Type::Shared {
                    inner: Box::new(walk(inner, map)),
                },
                other => other.clone(),
            }
        }
        walk(fty, &map)
    }

    /// The stdlib bridge enums `Result`/`Option` are declared non-generic
    /// with `any` payloads; their constructors used to produce a bare
    /// `Enum{Result, []}` that unified with ANY `Result<T, E>` annotation,
    /// so `fn f() -> Result<i64, str> { return Err(42) }` type-checked
    /// clean and then segfaulted when the i64 payload was read through the
    /// str-typed match binding (backlog #13/#19). Synthesize payload-bearing
    /// generics from the constructed variant so the annotation bridge in
    /// `unify` can check them. The un-constrained slot gets a fresh var.
    fn stdlib_bridge_generics(
        &mut self,
        enum_name: &str,
        variant: &str,
        arg_tys: &[Type],
    ) -> Vec<Type> {
        let payload = arg_tys.first().cloned().unwrap_or(Type::Error);
        match (enum_name, variant) {
            ("Result", "Ok") => vec![payload, self.engine.fresh_var()],
            ("Result", "Err") => vec![self.engine.fresh_var(), payload],
            ("Option", "Some") => vec![payload],
            ("Option", "None") => vec![self.engine.fresh_var()],
            _ => Vec::new(),
        }
    }

    pub fn infer_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            // Literals.
            Expr::IntLiteral { .. } => Type::I64,
            Expr::FloatLiteral { .. } => Type::F64,
            Expr::StringLiteral { .. } => Type::Str,
            Expr::InterpolatedString { parts, .. } => {
                // Type-check every interpolated expression. Previously the
                // parts were never visited, so an undefined identifier in
                // `"{...}"` passed `kryos check` and ran as 0 (backlog #23).
                for part in parts {
                    if let kryos_ast::StringPart::Expr(e) = part {
                        let _ = self.infer_expr(e);
                    }
                }
                Type::Str
            }
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
                    // For generic functions, instantiate fresh type variables so
                    // each call site gets independent type inference (prevents
                    // generic type pinning across call sites).
                    let sig = sig.clone();
                    let (params, ret, var_map) = self.engine.instantiate_sig(&sig);
                    // Carry the sig's trait bounds onto the fresh call-site
                    // vars so unify can enforce them (backlog #89).
                    for (old_id, new_id) in &var_map {
                        if let Some(bounds) = self.generic_var_bounds.get(old_id).cloned() {
                            self.engine.set_var_bounds(*new_id, bounds);
                        }
                    }
                    Type::Function {
                        params,
                        ret: Box::new(ret),
                    }
                } else if self.env.lookup_enum(name).is_some() {
                    // Enum name used as a namespace (e.g., `Color` in `Color.Red`).
                    Type::Enum {
                        name: name.clone(),
                        generics: vec![],
                    }
                } else if let Some((ename, edef)) = name.split_once("::").and_then(|(e, v)| {
                    // Qualified NULLARY enum variant, e.g. `Opt::None`. The parser
                    // emits this as an `Identifier` named "Opt::None" (the payload
                    // form `Opt::Some(7)` is a StaticMethodCall handled elsewhere).
                    self.env
                        .lookup_enum(e)
                        .filter(|d| d.variants.iter().any(|(vn, _)| vn == v))
                        .map(|d| (e.to_string(), d.clone()))
                }) {
                    let fresh_generics: Vec<Type> = edef
                        .generic_var_ids
                        .iter()
                        .map(|_| self.engine.fresh_var())
                        .collect();
                    Type::Enum {
                        name: ename,
                        generics: fresh_generics,
                    }
                } else if let Some(edef) = self.env.find_enum_by_variant(name).cloned().filter(
                    |edef| {
                        edef.variants
                            .iter()
                            .any(|(v, ftys)| v == name && ftys.is_empty())
                    },
                ) {
                    // Bare (unqualified) NULLARY enum variant used as a value, e.g.
                    // `Nil` / `Done` / `Red`. This mirrors the bare with-args
                    // constructor path in the FnCall arm (`Cons(1, t)` already
                    // resolves by unambiguous variant name) and the MIR lowering,
                    // which already resolves bare nullary variants via
                    // `find_enum_variant`. Only NULLARY variants resolve here — a
                    // with-args variant used bare without a call is not a value.
                    // Closes the inconsistency where `Cons(1, Nil)` rejected only
                    // the `Nil`, forcing recursive/tree enums to qualify every leaf.
                    let fresh_generics: Vec<Type> = edef
                        .generic_var_ids
                        .iter()
                        .map(|_| self.engine.fresh_var())
                        .collect();
                    Type::Enum {
                        name: edef.name.clone(),
                        generics: fresh_generics,
                    }
                } else {
                    self.error_with_code(
                        format!("undefined variable `{name}`"),
                        *span,
                        kryos_errors::codes::E0102,
                    );
                    // Newcomer hints for names carried over from other
                    // languages beat a Levenshtein suggestion here.
                    let newcomer_note: Option<&str> = match name.as_str() {
                        "null" | "nil" | "undefined" | "NULL" | "nullptr" => Some(
                            "Kryos has no null -- use `Option<T>` from std::option (`Some(x)` / `None()`)",
                        ),
                        "console" => Some("there is no `console` -- print with `println(...)`"),
                        _ => None,
                    };
                    let known = self.env.all_var_names();
                    if let Some(diag) = self.diagnostics.last_mut() {
                        if let Some(note) = newcomer_note {
                            diag.notes.push(note.to_string());
                        } else if let Some(suggestion) =
                            crate::suggest::closest_match(name, known.iter().map(|s| s.as_str()))
                        {
                            diag.notes.push(format!("did you mean `{suggestion}`?"));
                        } else {
                            // No close match, show that variable is not in scope
                            diag.notes.push("this variable is not in scope".to_string());
                        }
                    }
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
                    Type::Struct { name, generics } => {
                        if let Some(fty) = self.env.lookup_field(name, field) {
                            let fty = fty.clone();
                            self.substitute_struct_generics(name, generics, &fty)
                        } else {
                            self.error_with_code(
                                format!("no field `{field}` on type `{name}`"),
                                *span,
                                kryos_errors::codes::E0106,
                            );
                            let field_names: Vec<String> = self
                                .env
                                .lookup_struct(name)
                                .map(|d| d.fields.iter().map(|(n, _)| n.clone()).collect())
                                .unwrap_or_default();
                            if let Some(s) = crate::suggest::closest_match(
                                field,
                                field_names.iter().map(|s| s.as_str()),
                            ) {
                                if let Some(diag) = self.diagnostics.last_mut() {
                                    diag.notes.push(format!("did you mean `{s}`?"));
                                }
                            }
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
                            self.error(format!("no field `{field}` on tuple type"), *span);
                            Type::Error
                        }
                    }
                    // Auto-deref: access fields through references.
                    Type::Reference { inner, .. } => {
                        if let Type::Struct { name, generics } = inner.as_ref() {
                            if let Some(fty) = self.env.lookup_field(name, field) {
                                let fty = fty.clone();
                                self.substitute_struct_generics(name, generics, &fty)
                            } else {
                                self.error(format!("no field `{field}` on type `{name}`"), *span);
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
                    // Auto-deref through shared (Rc-like) pointer.
                    Type::Shared { inner } => {
                        if let Type::Struct { name, generics } = inner.as_ref() {
                            if let Some(fty) = self.env.lookup_field(name, field) {
                                let fty = fty.clone();
                                self.substitute_struct_generics(name, generics, &fty)
                            } else {
                                self.error(format!("no field `{field}` on type `{name}`"), *span);
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
                                let variants: Vec<String> =
                                    edef.variants.iter().map(|(n, _)| n.clone()).collect();
                                self.error(format!("no variant `{field}` on enum `{name}`"), *span);
                                if let Some(s) = crate::suggest::closest_match(
                                    field,
                                    variants.iter().map(|s| s.as_str()),
                                ) {
                                    if let Some(diag) = self.diagnostics.last_mut() {
                                        diag.notes.push(format!("did you mean `{s}`?"));
                                    }
                                }
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
                            if let Err(diag) = self.engine.unify(&Type::I64, &idx_ty, *span) {
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
                    // String indexing: str[i] -> str (single character)
                    Type::Str => Type::Str,
                    Type::Error => Type::Error,
                    _ => {
                        self.error(format!("type `{obj_ty}` is not indexable"), *span);
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
            Expr::UnaryOp { op, operand, span } => {
                let operand_ty = self.infer_expr(operand);
                match op {
                    UnOp::Neg => {
                        let resolved = self.engine.resolve(&operand_ty);
                        if resolved.is_numeric() || resolved.is_error() {
                            operand_ty
                        } else {
                            self.error(format!("cannot negate type `{resolved}`"), *span);
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
                            self.error(format!("cannot bitwise-not type `{resolved}`"), *span);
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
                        self.warning(format!("use of deprecated function `{name}`"), *span);
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
                                "len",
                                "range",
                                "to_string",
                                "typeof",
                                "sizeof",
                                "min",
                                "max",
                                "min_f",
                                "max_f",
                                "abs",
                                "abs_f",
                                "sqrt",
                                "pow",
                                "floor",
                                "ceil",
                                "round",
                                "log",
                                "log2",
                                "log10",
                                "sin",
                                "cos",
                                "tan",
                                "push",
                                "pop",
                                "contains",
                                "keys",
                                "values",
                                "split",
                                "trim",
                                "starts_with",
                                "ends_with",
                                "to_upper",
                                "to_lower",
                                "char_at",
                                "substring",
                                "parse_int",
                                "parse_float",
                            ];
                            if self.env.lookup_function(name).is_some()
                                && !side_effect_free_builtins.contains(&name.as_str())
                            {
                                self.error(
                                    format!(
                                        "`@pure` function cannot call non-pure function `{name}`"
                                    ),
                                    *span,
                                );
                            }
                        }
                    }
                }

                // Bare (unqualified) enum-variant constructor: `Some(42)`,
                // `Ok(v)`, `Rect(3, 5)`. If the callee is a plain identifier
                // that is not a value/function in scope but IS an enum variant,
                // type the call as enum construction (mirrors the qualified
                // `Enum.Variant(..)` path; the MIR lowering already handles bare
                // construction via find_enum_variant).
                if let Some(ref cname) = callee_name_str {
                    if self.env.lookup_var(cname).is_none()
                        && self.env.lookup_function(cname).is_none()
                    {
                        if let Some(edef) = self.env.find_enum_by_variant(cname).cloned() {
                            if let Some((_, field_types)) =
                                edef.variants.iter().find(|(v, _)| v == cname)
                            {
                                let mut var_map = std::collections::HashMap::new();
                                let mut fresh_generics =
                                    Vec::with_capacity(edef.generic_var_ids.len());
                                for &old_id in &edef.generic_var_ids {
                                    let fresh = self.engine.fresh_var();
                                    if let Type::Var(new_id) = &fresh {
                                        var_map.insert(old_id, *new_id);
                                    }
                                    fresh_generics.push(fresh);
                                }
                                let mut arg_tys = Vec::with_capacity(args.len());
                                for (arg, expected_ty) in args.iter().zip(field_types.iter()) {
                                    let arg_ty = self.infer_expr(arg);
                                    let expected_instantiated = if var_map.is_empty() {
                                        expected_ty.clone()
                                    } else {
                                        self.engine.instantiate(expected_ty, &var_map)
                                    };
                                    if let Err(diag) =
                                        self.engine.unify(&expected_instantiated, &arg_ty, *span)
                                    {
                                        self.diagnostics.push(diag);
                                    }
                                    arg_tys.push(arg_ty);
                                }
                                let generics = if fresh_generics.is_empty() {
                                    self.stdlib_bridge_generics(&edef.name, cname, &arg_tys)
                                } else {
                                    fresh_generics
                                };
                                return Type::Enum {
                                    name: edef.name.clone(),
                                    generics,
                                };
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

                        // Opaque any-callable shape produced by bare `fn`
                        // type annotations: `fn(any) -> any`. Skip arity and
                        // parameter-type checking entirely. We still infer
                        // each argument so its own type errors surface.
                        let is_opaque_callable = params.len() == 1
                            && matches!(&params[0], Type::Error)
                            && matches!(ret.as_ref(), Type::Error);
                        if is_opaque_callable {
                            for arg in args.iter() {
                                let _ = self.infer_expr(arg);
                            }
                            return *ret.clone();
                        }

                        if !is_assert_1arg && args.len() != params.len() {
                            let fn_name = match callee_name_str {
                                Some(ref n) => format!("`{n}`"),
                                None => "this function".to_string(),
                            };
                            self.error(
                                format!(
                                    "function {} expects {} argument{}, found {}",
                                    fn_name,
                                    params.len(),
                                    if params.len() == 1 { "" } else { "s" },
                                    args.len(),
                                ),
                                *span,
                            );
                        } else {
                            for (arg, param_ty) in args.iter().zip(params.iter()) {
                                // Bidirectional inference: if arg is an un-annotated
                                // Lambda and param_ty resolves to a Function type with
                                // matching arity, push the expected param/return types
                                // down so the Lambda body can be typed against them.
                                if let Expr::Lambda {
                                    params: lparams,
                                    span: lspan,
                                    ..
                                } = arg
                                {
                                    let resolved_pty = self.engine.resolve(param_ty);
                                    if let Type::Function {
                                        params: eps,
                                        ret: er,
                                    } = &resolved_pty
                                    {
                                        if eps.len() == lparams.len() {
                                            self.lambda_expected_types.insert(
                                                *lspan,
                                                (eps.clone(), (**er).clone()),
                                            );
                                        }
                                    }
                                }
                                let arg_ty = self.infer_expr(arg);
                                if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span())
                                {
                                    self.diagnostics.push(diag);
                                }
                                self.check_int_literal_range(param_ty, arg, arg.span());
                                // The builtin `len` signature accepts any type
                                // (its param is Type::Error) because it is
                                // polymorphic over str/array/map. That let a
                                // struct/enum/number slip through and read a
                                // garbage "length" from the value's first
                                // 8 bytes at runtime (a CsvRow passed to len
                                // printed a pointer). Gate the builtin only --
                                // a user function shadowing `len` has typed
                                // params and never hits this arm.
                                if matches!(&callee_name_str, Some(n) if n == "len")
                                    && matches!(param_ty, Type::Error)
                                {
                                    let resolved = self.engine.resolve(&arg_ty);
                                    let ok = matches!(
                                        resolved,
                                        Type::Str
                                            | Type::Array { .. }
                                            | Type::Map { .. }
                                            | Type::Tuple { .. }
                                            | Type::Error
                                            | Type::Var(_)
                                    );
                                    if !ok {
                                        self.error(
                                            format!(
                                                "`len` expects a str, array, or map; found `{resolved}`"
                                            ),
                                            arg.span(),
                                        );
                                    }
                                }
                            }
                        }
                        *ret.clone()
                    }
                    Type::Error => Type::Error,
                    _ => {
                        self.error(format!("type `{callee_ty}` is not callable"), *span);
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
                // If the receiver is a bare identifier that names a struct or
                // enum (not a value in scope), treat the call as a static
                // method/associated-function call. This makes `List.new()`,
                // `Map.new()` etc. work the same as `List::new()`.
                if let Expr::Identifier { name: ident, .. } = object.as_ref() {
                    let is_var = self.env.lookup_var(ident).is_some();
                    let is_type = self.env.lookup_struct(ident).is_some()
                        || self.env.lookup_enum(ident).is_some();
                    if !is_var && is_type {
                        let static_call = Expr::StaticMethodCall {
                            type_name: ident.clone(),
                            method: method.clone(),
                            args: args.clone(),
                            span: *span,
                        };
                        return self.infer_expr(&static_call);
                    }
                }
                let obj_ty = self.infer_expr(object);
                let obj_ty = self.engine.resolve(&obj_ty);

                // Resolve method calls on a generic type variable through its
                // declared trait bounds. For `fn announce<T: Showable>(x: T)`,
                // `x.show()` typechecks because `T`'s bound is `Showable` and
                // `Showable::show(self) -> str` is in scope. Each impl resolves
                // the actual function during monomorphization in MIR lowering.
                if let Type::Var(id) = &obj_ty {
                    if let Some(bounds) = self.generic_var_bounds.get(id).cloned() {
                        for trait_name in &bounds {
                            if let Some(trait_def) =
                                self.env.lookup_trait(trait_name).cloned()
                            {
                                if let Some(sig) =
                                    trait_def.methods.iter().find(|m| m.name == *method)
                                {
                                    let expected_params: Vec<_> = if sig
                                        .params
                                        .first()
                                        .map(|(n, _)| n.as_str())
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
                                        for (arg, (_, param_ty)) in
                                            args.iter().zip(expected_params.iter())
                                        {
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
                        }
                    }
                }

                // Handle method calls on dyn Trait objects via trait definition lookup.
                if let Type::DynTrait { ref trait_name } = obj_ty {
                    if let Some(trait_def) = self.env.lookup_trait(trait_name).cloned() {
                        if let Some(sig) = trait_def.methods.iter().find(|m| m.name == *method) {
                            // Skip 'self' parameter (first param) if present.
                            let expected_params: Vec<_> =
                                if sig.params.first().map(|(n, _)| n.as_str()) == Some("self") {
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
                                for (arg, (_, param_ty)) in args.iter().zip(expected_params.iter())
                                {
                                    let arg_ty = self.infer_expr(arg);
                                    if let Err(diag) =
                                        self.engine.unify(param_ty, &arg_ty, arg.span())
                                    {
                                        self.diagnostics.push(diag);
                                    }
                                    self.check_int_literal_range(param_ty, arg, arg.span());
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
                        if let Some((_, field_types)) =
                            edef.variants.iter().find(|(vname, _)| vname == method)
                        {
                            // Per-use monomorphization: fresh vars for each generic param.
                            let mut var_map = std::collections::HashMap::new();
                            let mut fresh_generics = Vec::with_capacity(edef.generic_var_ids.len());
                            for &old_id in &edef.generic_var_ids {
                                let fresh = self.engine.fresh_var();
                                if let Type::Var(new_id) = &fresh {
                                    var_map.insert(old_id, *new_id);
                                }
                                fresh_generics.push(fresh);
                            }
                            let mut arg_tys = Vec::with_capacity(args.len());
                            for (arg, expected_ty) in args.iter().zip(field_types.iter()) {
                                let arg_ty = self.infer_expr(arg);
                                let expected_instantiated = if var_map.is_empty() {
                                    expected_ty.clone()
                                } else {
                                    self.engine.instantiate(expected_ty, &var_map)
                                };
                                if let Err(diag) =
                                    self.engine
                                        .unify(&expected_instantiated, &arg_ty, arg.span())
                                {
                                    self.diagnostics.push(diag);
                                }
                                arg_tys.push(arg_ty);
                            }
                            let generics = if fresh_generics.is_empty() {
                                self.stdlib_bridge_generics(tname, method, &arg_tys)
                            } else {
                                fresh_generics
                            };
                            return Type::Enum {
                                name: tname.clone(),
                                generics,
                            };
                        }
                    }

                    if let Some(sig) = self.env.lookup_method(tname, method).cloned() {
                        // Instantiate the method signature with FRESH type vars
                        // per call, then bind them to the receiver's concrete
                        // type arguments by unifying the freshened `self`
                        // parameter against the receiver type. This fixes two
                        // problems that the old `sig.ret.clone()` had:
                        //   1. Contamination -- the impl's return var was shared
                        //      across every call, so `.get()` on Box<str> then
                        //      `let x: f64 = box_f64.get()` errored "found str".
                        //   2. Erasure -- `to_string(box_f64.get())` reported the
                        //      i64 slot and printed raw bits; the self-unify now
                        //      resolves the return to the real f64.
                        let (inst_params, inst_ret, var_map) =
                            self.engine.instantiate_sig(&sig);
                        for (old_id, new_id) in &var_map {
                            if let Some(bounds) = self.generic_var_bounds.get(old_id).cloned() {
                                self.engine.set_var_bounds(*new_id, bounds);
                            }
                        }
                        let has_self =
                            sig.params.first().map(|(n, _)| n.as_str()) == Some("self");
                        let expected_params: Vec<Type> = if has_self {
                            // Unify only when the freshened self is a same-named
                            // struct/enum with matching generic arity -- an
                            // unannotated `self` (empty generics) or a shape
                            // mismatch must not inject a spurious error.
                            if let Some(self_ty) = inst_params.first() {
                                let compatible = match (self_ty, &obj_ty) {
                                    (
                                        Type::Struct { name: a, generics: ga },
                                        Type::Struct { name: b, generics: gb },
                                    )
                                    | (
                                        Type::Enum { name: a, generics: ga },
                                        Type::Enum { name: b, generics: gb },
                                    ) => a == b && ga.len() == gb.len() && !ga.is_empty(),
                                    _ => false,
                                };
                                if compatible {
                                    let _ = self.engine.unify(self_ty, &obj_ty, *span);
                                }
                            }
                            inst_params.iter().skip(1).cloned().collect()
                        } else {
                            inst_params.clone()
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
                            for (arg, param_ty) in args.iter().zip(expected_params.iter()) {
                                let arg_ty = self.infer_expr(arg);
                                if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span())
                                {
                                    self.diagnostics.push(diag);
                                }
                            }
                        }
                        return self.engine.resolve(&inst_ret);
                    }

                    // Check if this is a Function-typed struct field being called
                    // (e.g. `t.transform(5)` where `transform: fn(i64) -> i64`).
                    if let Some(Type::Function {
                        params: fn_params,
                        ret: fn_ret,
                    }) = self.env.lookup_field(tname, method).cloned()
                    {
                        // Opaque callable (bare `fn` type) accepts any arity.
                        let is_opaque = fn_params.len() == 1
                            && matches!(&fn_params[0], Type::Error)
                            && matches!(fn_ret.as_ref(), Type::Error);
                        if is_opaque {
                            for arg in args.iter() {
                                let _ = self.infer_expr(arg);
                            }
                            return *fn_ret;
                        }
                        if args.len() != fn_params.len() {
                            self.error(
                                format!(
                                    "closure field `{method}` expects {} arguments, found {}",
                                    fn_params.len(),
                                    args.len()
                                ),
                                *span,
                            );
                        } else {
                            for (arg, param_ty) in args.iter().zip(fn_params.iter()) {
                                let arg_ty = self.infer_expr(arg);
                                if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span())
                                {
                                    self.diagnostics.push(diag);
                                }
                            }
                        }
                        return *fn_ret;
                    }
                }

                self.error_with_code(
                    format!("no method `{method}` found for type `{obj_ty}`"),
                    *span,
                    kryos_errors::codes::E0107,
                );
                // Common builtins mistaken for methods (`s.len()`, `a.push(x)`).
                const BUILTIN_FN_HINTS: &[&str] = &[
                    "len", "push", "pop", "contains", "starts_with", "ends_with",
                    "trim", "to_upper", "to_lower", "replace", "split", "join",
                    "substr", "sort", "reverse", "abs", "to_string",
                ];
                if BUILTIN_FN_HINTS.contains(&method.as_str()) {
                    if let Some(diag) = self.diagnostics.last_mut() {
                        diag.notes.push(format!(
                            "`{method}` is a global builtin, not a method -- write `{method}(value, ...)`"
                        ));
                    }
                }
                if let Type::Struct { name, .. } | Type::Enum { name, .. } = &obj_ty {
                    let methods = self.env.all_method_names(name);
                    if let Some(s) =
                        crate::suggest::closest_match(method, methods.iter().map(|s| s.as_str()))
                    {
                        if let Some(diag) = self.diagnostics.last_mut() {
                            diag.notes.push(format!("did you mean `{s}`?"));
                        }
                    }
                }
                Type::Error
            }

            Expr::StaticMethodCall {
                type_name,
                method,
                args,
                span,
            } => {
                // Enum variant constructor via `::` syntax (e.g. `Maybe::Some(10)`).
                if let Some(edef) = self.env.lookup_enum(type_name).cloned() {
                    if let Some((_, field_types)) =
                        edef.variants.iter().find(|(vname, _)| vname == method)
                    {
                        let mut var_map = std::collections::HashMap::new();
                        let mut fresh_generics = Vec::with_capacity(edef.generic_var_ids.len());
                        for &old_id in &edef.generic_var_ids {
                            let fresh = self.engine.fresh_var();
                            if let Type::Var(new_id) = &fresh {
                                var_map.insert(old_id, *new_id);
                            }
                            fresh_generics.push(fresh);
                        }
                        let mut arg_tys = Vec::with_capacity(args.len());
                        for (arg, expected_ty) in args.iter().zip(field_types.iter()) {
                            let arg_ty = self.infer_expr(arg);
                            let expected_instantiated = if var_map.is_empty() {
                                expected_ty.clone()
                            } else {
                                self.engine.instantiate(expected_ty, &var_map)
                            };
                            if let Err(diag) =
                                self.engine
                                    .unify(&expected_instantiated, &arg_ty, arg.span())
                            {
                                self.diagnostics.push(diag);
                            }
                            arg_tys.push(arg_ty);
                        }
                        let generics = if fresh_generics.is_empty() {
                            self.stdlib_bridge_generics(type_name, method, &arg_tys)
                        } else {
                            fresh_generics
                        };
                        return Type::Enum {
                            name: type_name.clone(),
                            generics,
                        };
                    }
                }
                // Look up the method on the type (mangled as TypeName__method).
                if let Some(sig) = self.env.lookup_method(type_name, method).cloned() {
                    // Static call — skip 'self' parameter.
                    let expected_params: Vec<_> =
                        if sig.params.first().map(|(n, _)| n.as_str()) == Some("self") {
                            sig.params[1..].to_vec()
                        } else {
                            sig.params.clone()
                        };
                    if args.len() != expected_params.len() {
                        self.error(
                            format!(
                                "static method `{type_name}::{method}` expects {} arguments, found {}",
                                expected_params.len(),
                                args.len()
                            ),
                            *span,
                        );
                    } else {
                        for (arg, (_, param_ty)) in args.iter().zip(expected_params.iter()) {
                            let arg_ty = self.infer_expr(arg);
                            if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span()) {
                                self.diagnostics.push(diag);
                            }
                        }
                    }
                    sig.ret.clone()
                } else if self.env.lookup_struct(type_name).is_none()
                    && self.env.lookup_enum(type_name).is_none()
                    && self.env.lookup_function(method).is_some()
                {
                    // MODULE-qualified call: `util::add(2, 3)` (or via an
                    // alias, `u::add(..)`). `type_name` is not a type, and the
                    // module's functions are imported under their plain names
                    // -- type this as a call of the plain function. The MIR
                    // lowering already resolves it the same way; only the
                    // checker rejected it ("no method `add` on type `util`").
                    let sig = self.env.lookup_function(method).cloned().unwrap();
                    let (params, ret, var_map) = self.engine.instantiate_sig(&sig);
                    for (old_id, new_id) in &var_map {
                        if let Some(bounds) = self.generic_var_bounds.get(old_id).cloned() {
                            self.engine.set_var_bounds(*new_id, bounds);
                        }
                    }
                    if args.len() != params.len() {
                        self.error(
                            format!(
                                "function `{method}` expects {} arguments, found {}",
                                params.len(),
                                args.len()
                            ),
                            *span,
                        );
                    } else {
                        for (arg, param_ty) in args.iter().zip(params.iter()) {
                            let arg_ty = self.infer_expr(arg);
                            if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span()) {
                                self.diagnostics.push(diag);
                            }
                        }
                    }
                    ret
                } else {
                    self.error(
                        format!("no method `{method}` found on type `{type_name}`"),
                        *span,
                    );
                    let methods = self.env.all_method_names(type_name);
                    if let Some(s) =
                        crate::suggest::closest_match(method, methods.iter().map(|s| s.as_str()))
                    {
                        if let Some(diag) = self.diagnostics.last_mut() {
                            diag.notes.push(format!("did you mean `{s}`?"));
                        }
                    }
                    Type::Error
                }
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
                if self.actor_names.contains(name) {
                    self.error(
                        format!(
                            "`{name}` is an actor; construct it with `{name}()` -- actor state is private and initializes to zero"
                        ),
                        *span,
                    );
                    for (_f, e) in fields {
                        let _ = self.infer_expr(e);
                    }
                    return Type::Struct {
                        name: name.clone(),
                        generics: vec![],
                    };
                }
                if let Some(def) = self.env.lookup_struct(name).cloned() {
                    // Per-use monomorphization: build a var_map from the def's
                    // generic var IDs to fresh vars, then instantiate each
                    // field's expected type before unifying.
                    let mut var_map = std::collections::HashMap::new();
                    let mut fresh_generics = Vec::with_capacity(def.generic_var_ids.len());
                    for &old_id in &def.generic_var_ids {
                        let fresh = self.engine.fresh_var();
                        if let Type::Var(new_id) = &fresh {
                            var_map.insert(old_id, *new_id);
                        }
                        fresh_generics.push(fresh);
                    }
                    for (fname, fexpr) in fields {
                        let expr_ty = self.infer_expr(fexpr);
                        if let Some((_, expected_ty)) = def.fields.iter().find(|(n, _)| n == fname)
                        {
                            let expected_instantiated = if var_map.is_empty() {
                                expected_ty.clone()
                            } else {
                                self.engine.instantiate(expected_ty, &var_map)
                            };
                            if let Err(diag) =
                                self.engine
                                    .unify(&expected_instantiated, &expr_ty, fexpr.span())
                            {
                                self.diagnostics.push(diag);
                            }
                        } else {
                            self.error(format!("no field `{fname}` on struct `{name}`"), *span);
                        }
                    }
                    Type::Struct {
                        name: name.clone(),
                        generics: fresh_generics,
                    }
                } else {
                    self.error_with_code(
                        format!("unknown struct `{name}`"),
                        *span,
                        kryos_errors::codes::E0103,
                    );
                    let known = self.env.all_struct_names();
                    if let Some(suggestion) =
                        crate::suggest::closest_match(name, known.iter().map(|s| s.as_str()))
                    {
                        if let Some(diag) = self.diagnostics.last_mut() {
                            diag.notes.push(format!("did you mean `{suggestion}`?"));
                        }
                    }
                    Type::Error
                }
            }

            // Lambda.
            Expr::Lambda {
                params,
                ret_ty,
                body,
                span: lambda_span,
            } => {
                // Bidirectional inference: if an outer FnCall pushed expected
                // types for this lambda, use them for any un-annotated params
                // and the return type. This is what makes `|a, b| a + b` work
                // when passed to a function expecting `fn(i64, i64) -> i64`.
                let expected = self.lambda_expected_types.remove(lambda_span);

                let param_types: Vec<Type> = params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        if let Some(t) = p.ty.as_ref() {
                            self.resolve_type_expr(t)
                        } else if let Some((ref eps, _)) = expected {
                            if let Some(et) = eps.get(i) {
                                et.clone()
                            } else {
                                self.engine.fresh_var()
                            }
                        } else {
                            self.engine.fresh_var()
                        }
                    })
                    .collect();

                let ret = if let Some(t) = ret_ty.as_ref() {
                    self.resolve_type_expr(t)
                } else if let Some((_, ref er)) = expected {
                    er.clone()
                } else {
                    self.engine.fresh_var()
                };

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

                // Record resolved types for UN-annotated params (str/struct/
                // enum/array/... AND floats) so MIR can type the closure's
                // params instead of defaulting them to i64 -- the cause of a
                // `str` closure passed to a HOF (`fold(xs, "", |acc, x| acc+x)`)
                // miscompiling its concatenation, and of an f64 closure
                // (`map(xs, |x| x*2.0)`) lowering `*` to an integer `imul`.
                // Floats are i64-slot-passed through the uniform closure ABI;
                // both backends' env-thunks already bit-cast the i64 slot to/from
                // the real float param/return type, so typing the param is enough.
                {
                    let resolved: Vec<Option<TypeExpr>> = params
                        .iter()
                        .zip(param_types.iter())
                        .map(|(p, pt)| {
                            if p.ty.is_some() {
                                return None;
                            }
                            let r = self.engine.resolve(pt);
                            if is_flowable_param_type(&r) {
                                concrete_type_to_type_expr(&r)
                            } else {
                                None
                            }
                        })
                        .collect();
                    if resolved.iter().any(|x| x.is_some()) {
                        self.resolved_lambda_params.insert(*lambda_span, resolved);
                    }
                }

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
                let then_ty = self.infer_block_as_expr(then_branch);
                self.env.pop_scope();
                if let Some(else_blk) = else_branch {
                    self.env.push_scope();
                    let else_ty = self.infer_block_as_expr(else_blk);
                    self.env.pop_scope();
                    if let Err(diag) = self.engine.unify(&then_ty, &else_ty, *span) {
                        self.diagnostics.push(diag);
                    }
                    then_ty
                } else {
                    // No else branch: if-expr without else is void.
                    Type::Void
                }
            }

            // Match expression.
            Expr::MatchExpr {
                subject,
                arms,
                span,
            } => {
                let subject_ty = self.infer_expr(subject);
                if arms.is_empty() {
                    return Type::Void;
                }
                let mut result_ty: Option<Type> = None;
                for arm in arms {
                    self.env.push_scope();
                    self.bind_pattern(&arm.pattern, &subject_ty);
                    let arm_ty = self.infer_expr(&arm.body);
                    // An arm body that diverges (always returns/throws) does
                    // not contribute to the match expression's type.  This
                    // lets the `?` operator desugar — whose Err arm is
                    // `{ return Err(e) }` of type Void — coexist with an Ok
                    // arm that yields the unwrapped value.
                    let arm_diverges = arm_body_diverges(&arm.body);
                    self.env.pop_scope();

                    if arm_diverges {
                        continue;
                    }

                    if let Some(ref first) = result_ty {
                        if let Err(diag) = self.engine.unify(first, &arm_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                    } else {
                        result_ty = Some(arm_ty);
                    }
                }

                // --- Exhaustiveness check ---
                let resolved = self.engine.resolve(&subject_ty);
                let type_name = match &resolved {
                    Type::Bool => "bool".to_string(),
                    Type::I8 => "i8".to_string(),
                    Type::I16 => "i16".to_string(),
                    Type::I32 => "i32".to_string(),
                    Type::I64 => "i64".to_string(),
                    Type::I128 => "i128".to_string(),
                    Type::U8 => "u8".to_string(),
                    Type::U16 => "u16".to_string(),
                    Type::U32 => "u32".to_string(),
                    Type::U64 => "u64".to_string(),
                    Type::U128 => "u128".to_string(),
                    Type::Str => "str".to_string(),
                    Type::Char => "char".to_string(),
                    Type::Struct { name, .. } | Type::Enum { name, .. } => name.clone(),
                    _ => String::new(),
                };
                if !type_name.is_empty() {
                    let enum_def = self.env.lookup_enum(&type_name).cloned();
                    let pats: Vec<&kryos_ast::Pattern> = arms.iter().map(|a| &a.pattern).collect();
                    let warnings = crate::exhaustive::check_exhaustive(
                        &type_name,
                        &pats,
                        enum_def.as_ref(),
                        *span,
                    );
                    self.diagnostics.extend(warnings);
                }

                result_ty.unwrap_or(Type::Void)
            }

            // Range expression.
            Expr::RangeExpr {
                start, end, span, ..
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
            Expr::Deref { inner, span } => {
                let deref_span = *span;
                let inner_ty = self.infer_expr(inner);
                match inner_ty {
                    Type::Reference { inner, .. } => *inner,
                    Type::Pointer { inner, .. } => {
                        // Dereferencing a RAW pointer is an unsafe operation: it
                        // can read arbitrary memory. Require an enclosing
                        // `unsafe { }` block (E0500). References/Shared are safe.
                        if self.unsafe_depth == 0 {
                            self.error_with_code(
                                "dereference of raw pointer requires an `unsafe` block"
                                    .to_string(),
                                deref_span,
                                kryos_errors::codes::E0500,
                            );
                        }
                        *inner
                    }
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
                // The block's type is its tail expression's type, computed
                // WHILE the block scope is still live -- locals bound earlier
                // in the block (e.g. `{ let a = 5; a + 1 }`) must be in scope
                // for the tail. (Previously the tail was re-inferred AFTER
                // popping the scope, so a tail referencing a block-local
                // failed with "undefined variable" -- which also broke
                // `spawn { let x = ...; use x }`.)
                let mut block_ty = Type::Void;
                let last_idx = block.stmts.len().wrapping_sub(1);
                for (i, stmt) in block.stmts.iter().enumerate() {
                    // A value-position block's tail Stmt::Expr is the block's VALUE,
                    // not a discard -- infer it directly and bypass check_stmt, so the
                    // W0400 must-use lint doesn't false-positive on a Tracked tail
                    // (mirrors infer_block_as_expr). Doing so also avoids the
                    // double-infer that the previous check_stmt + infer_expr produced.
                    if i == last_idx {
                        if let Stmt::Expr { expr, .. } = stmt {
                            block_ty = self.infer_expr(expr);
                            continue;
                        }
                    }
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                block_ty
            }

            // Comptime block — type is the type of the last expression.
            Expr::ComptimeBlock { body, .. } => {
                self.env.push_scope();
                let last_idx = body.stmts.len().wrapping_sub(1);
                let mut ty = Type::Void;
                for (i, stmt) in body.stmts.iter().enumerate() {
                    // Tail Stmt::Expr is the block's value, not a discard (see Expr::Block).
                    if i == last_idx {
                        if let Stmt::Expr { expr, .. } = stmt {
                            ty = self.infer_expr(expr);
                            continue;
                        }
                    }
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                ty
            }
            // Quantum blocks — check body, return void for now.
            Expr::QuantumBlock { body, .. } => {
                self.env.push_scope();
                self.check_block(body);
                self.env.pop_scope();
                Type::Void
            }

            // Unsafe block — a plain block whose type is its tail expression's,
            // but while it is being checked, raw-pointer dereference is allowed
            // (E0500 is suppressed). Semantically transparent to codegen.
            Expr::UnsafeBlock { body, .. } => {
                self.env.push_scope();
                self.unsafe_depth += 1;
                let last_idx = body.stmts.len().wrapping_sub(1);
                let mut ty = Type::Void;
                for (i, stmt) in body.stmts.iter().enumerate() {
                    if i == last_idx {
                        if let Stmt::Expr { expr, .. } = stmt {
                            ty = self.infer_expr(expr);
                            continue;
                        }
                    }
                    self.check_stmt(stmt);
                }
                self.unsafe_depth -= 1;
                self.env.pop_scope();
                ty
            }

            // Await expression — for now, just pass through the inner type.
            // A full implementation would unwrap Future<T> → T.
            Expr::Await { value, .. } => self.infer_expr(value),
        }
    }

    // ── Binary operator type checking ────────────────────────────────

    fn check_binary_op(&mut self, op: BinOp, left: &Expr, right: &Expr, span: Span) -> Type {
        let left_ty = self.infer_expr(left);
        let right_ty = self.infer_expr(right);

        match op {
            // Arithmetic: both sides must be the same numeric type.
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow => {
                // Special case: array concatenation with +.
                if op == BinOp::Add {
                    let resolved_left = self.engine.resolve(&left_ty);
                    let resolved_right = self.engine.resolve(&right_ty);
                    if let (Type::Array { element: e1, .. }, Type::Array { element: e2, .. }) =
                        (&resolved_left, &resolved_right)
                    {
                        // Unify element types.
                        if let Err(diag) = self.engine.unify(e1, e2, span) {
                            self.diagnostics.push(diag);
                            return Type::Error;
                        }
                        let elem = self.engine.resolve(e1);
                        // Array concatenation always produces a dynamic array.
                        return Type::Array {
                            element: Box::new(elem),
                            size: None,
                        };
                    }
                }

                if let Err(diag) = self.engine.unify(&left_ty, &right_ty, span) {
                    self.diagnostics.push(diag);
                    return Type::Error;
                }
                let resolved = self.engine.resolve(&left_ty);
                // Deferred numeric context: both operands are unconstrained
                // type vars (e.g. `|n| |x| x + n` — neither annotation nor
                // literal pins a type). Default to i64; the operands were
                // unified above so one binding fixes both.
                if let Type::Var(_) = resolved {
                    let _ = self.engine.unify(&resolved, &Type::I64, span);
                    return Type::I64;
                }
                if !resolved.is_numeric() && !resolved.is_error() {
                    // Special case: string concatenation with +.
                    if op == BinOp::Add && resolved == Type::Str {
                        return Type::Str;
                    }
                    let op_sym = match op {
                        BinOp::Add => "+",
                        BinOp::Sub => "-",
                        BinOp::Mul => "*",
                        BinOp::Div => "/",
                        BinOp::Mod => "%",
                        BinOp::Pow => "**",
                        _ => "?",
                    };
                    self.error(
                        format!(
                            "cannot apply `{op_sym}` to type `{resolved}`: expected a numeric type"
                        ),
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
                // Deferred integer context — same Var-defaulting as the
                // arithmetic branch (`|n| |x| x & n`).
                if let Type::Var(_) = resolved {
                    let _ = self.engine.unify(&resolved, &Type::I64, span);
                    return Type::I64;
                }
                if !resolved.is_integer() && !resolved.is_error() {
                    let op_sym = match op {
                        BinOp::BitAnd => "&",
                        BinOp::BitOr => "|",
                        BinOp::BitXor => "^",
                        BinOp::Shl => "<<",
                        BinOp::Shr => ">>",
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
/// True when `ty` is a type whose un-annotated closure param we want to flow
/// into MIR rather than defaulting to i64: pointer/heap-represented types
/// (i64-register-sized, no ABI change) plus floats (passed in an i64 slot
/// through the uniform closure ABI; the env-thunk bit-casts the slot to/from
/// the real float type on both backends). Plain integers/bool already work as
/// i64 and are excluded.
fn is_flowable_param_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Str
            | Type::Struct { .. }
            | Type::Enum { .. }
            | Type::Array { .. }
            | Type::Tuple { .. }
            | Type::Map { .. }
            | Type::Option { .. }
            | Type::Result { .. }
            | Type::Reference { .. }
            | Type::Shared { .. }
            | Type::Pointer { .. }
            | Type::F32
            | Type::F64
    )
}

/// Convert a resolved concrete `Type` to an AST `TypeExpr` for annotating a
/// lambda param. Returns None for unresolved vars or shapes we can't represent.
fn concrete_type_to_type_expr(ty: &Type) -> Option<TypeExpr> {
    let sp = Span::DUMMY;
    let simple = |n: &str| TypeExpr::Simple {
        name: n.to_string(),
        span: sp,
    };
    let generic = |n: &str, args: Vec<TypeExpr>| TypeExpr::Generic {
        name: n.to_string(),
        args,
        span: sp,
    };
    Some(match ty {
        Type::I8 => simple("i8"),
        Type::I16 => simple("i16"),
        Type::I32 => simple("i32"),
        Type::I64 => simple("i64"),
        Type::I128 => simple("i128"),
        Type::U8 => simple("u8"),
        Type::U16 => simple("u16"),
        Type::U32 => simple("u32"),
        Type::U64 => simple("u64"),
        Type::U128 => simple("u128"),
        Type::F32 => simple("f32"),
        Type::F64 => simple("f64"),
        Type::Bool => simple("bool"),
        Type::Char => simple("char"),
        Type::Str => simple("str"),
        Type::USize => simple("usize"),
        Type::ISize => simple("isize"),
        Type::Struct { name, generics } if generics.is_empty() => simple(name),
        Type::Enum { name, generics } if generics.is_empty() => simple(name),
        Type::Struct { name, generics } | Type::Enum { name, generics } => generic(
            name,
            generics
                .iter()
                .map(concrete_type_to_type_expr)
                .collect::<Option<Vec<_>>>()?,
        ),
        Type::Array { element, size } => TypeExpr::Array {
            element: Box::new(concrete_type_to_type_expr(element)?),
            size: *size,
            span: sp,
        },
        Type::Tuple { elements } => TypeExpr::Tuple {
            elements: elements
                .iter()
                .map(concrete_type_to_type_expr)
                .collect::<Option<Vec<_>>>()?,
            span: sp,
        },
        Type::Map { key, value } => generic(
            "map",
            vec![
                concrete_type_to_type_expr(key)?,
                concrete_type_to_type_expr(value)?,
            ],
        ),
        Type::Option { inner } => generic("Option", vec![concrete_type_to_type_expr(inner)?]),
        Type::Result { ok, err } => generic(
            "Result",
            vec![
                concrete_type_to_type_expr(ok)?,
                concrete_type_to_type_expr(err)?,
            ],
        ),
        _ => return None,
    })
}

pub fn type_check(module: &Module) -> Vec<Diagnostic> {
    type_check_with_lambda_params(module).0
}

/// Type-check a module and also return the resolved types of un-annotated
/// lambda parameters (keyed by lambda span), for the MIR lowering to consume.
pub fn type_check_with_lambda_params(
    module: &Module,
) -> (
    Vec<Diagnostic>,
    std::collections::HashMap<Span, Vec<Option<TypeExpr>>>,
    std::collections::HashMap<Span, TypeExpr>,
) {
    let mut checker = TypeChecker::new();

    // Register built-in functions that are always available.
    // println/print/eprintln accept any type — codegen converts non-string
    // args to strings via kryos_builtin_to_string at call time.
    checker.env.define_function(FunctionSig {
        name: "println".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Void,
    });

    // print(value: any) -> void  (no newline)
    checker.env.define_function(FunctionSig {
        name: "print".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Void,
    });

    // eprintln(value: any) -> void  (stderr + newline)
    checker.env.define_function(FunctionSig {
        name: "eprintln".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Void,
    });

    // exit(code: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "exit".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("code".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // range(start, end) -> [i64]  (conceptually returns an integer sequence;
    // the MIR lowering special-cases this into a counter loop)
    checker.env.define_function(FunctionSig {
        name: "range".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("start".to_string(), Type::I64),
            ("end".to_string(), Type::I64),
        ],
        ret: Type::Array {
            element: Box::new(Type::I64),
            size: None,
        },
    });

    // len(collection) -> i64 — accepts any collection type (str, array, map).
    // Codegen passes the opaque handle to kryos_builtin_len which reads
    // the length field at offset 0 (shared across all collection types).
    checker.env.define_function(FunctionSig {
        name: "len".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("collection".to_string(), Type::Error)],
        ret: Type::I64,
    });

    // to_string(value) -> str — accepts any type.
    checker.env.define_function(FunctionSig {
        name: "to_string".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Str,
    });

    // chan() -> i64  (create a new channel)
    checker.env.define_function(FunctionSig {
        name: "chan".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![],
        ret: Type::I64,
    });

    // send(ch, value) -> void
    checker.env.define_function(FunctionSig {
        name: "send".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
        params: vec![("ch".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // chan_try_recv(ch) -> i64 status (1 data / 0 empty / -1 closed) — non-blocking.
    checker.env.define_function(FunctionSig {
        name: "chan_try_recv".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("ch".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // chan_last_recv() -> i64 value of the most recent successful try_recv on this thread.
    checker.env.define_function(FunctionSig {
        name: "chan_last_recv".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![],
        ret: Type::I64,
    });

    // file_read(path: str) -> str — read entire file to string
    checker.env.define_function(FunctionSig {
        name: "file_read".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // file_write(path: str, content: str) -> i64 — write string to file (0=ok, -1=err)
    checker.env.define_function(FunctionSig {
        name: "file_write".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("path".to_string(), Type::Str),
            ("content".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // file_exists(path: str) -> i64 — 1 if exists, 0 if not
    checker.env.define_function(FunctionSig {
        name: "file_exists".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // append_file(path: str, content: str) -> i64 — append string to file (0=ok, -1=err)
    checker.env.define_function(FunctionSig {
        name: "append_file".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("path".to_string(), Type::Str),
            ("content".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // file_size(path: str) -> i64 — byte size of file, -1 on error
    checker.env.define_function(FunctionSig {
        name: "file_size".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // Legacy file IO aliases: read_file/write_file mirror file_read/file_write.
    // These exist because the LLVM codegen and several v1.x examples used the
    // *_file form; the canonical name is the file_* form.
    checker.env.define_function(FunctionSig {
        name: "read_file".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "write_file".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("path".to_string(), Type::Str),
            ("content".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // create_dir(path: str) -> i64 — create directory recursively
    checker.env.define_function(FunctionSig {
        name: "create_dir".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // read_line() -> str — read one line from stdin
    checker.env.define_function(FunctionSig {
        name: "read_line".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![],
        ret: Type::Str,
    });

    // map_has(m, key: str) -> bool
    checker.env.define_function(FunctionSig {
        name: "map_has".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("m".to_string(), Type::Error),
            ("key".to_string(), Type::Str),
        ],
        ret: Type::Bool,
    });

    // map_delete(m, key: str) -> map
    checker.env.define_function(FunctionSig {
        name: "map_delete".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("m".to_string(), Type::Error),
            ("key".to_string(), Type::Str),
        ],
        ret: Type::Error,
    });

    // map_keys(m) -> [str]
    checker.env.define_function(FunctionSig {
        name: "map_keys".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("m".to_string(), Type::Error)],
        ret: Type::Error,
    });

    // env_get(key: str) -> str — get environment variable
    checker.env.define_function(FunctionSig {
        name: "env_get".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("key".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // time_now() -> i64 — Unix timestamp in seconds
    checker.env.define_function(FunctionSig {
        name: "time_now".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![],
        ret: Type::I64,
    });

    // sleep(ms: i64) -> void — block the current task for `ms` milliseconds
    checker.env.define_function(FunctionSig {
        name: "sleep".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("ms".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // ---------------------------------------------------------------------
    // Cooperative async executor (real interleaving via await / yield).
    //   coop_spawn(task) -> i64   register a cooperative task; returns id
    //   coop_yield()      -> void hand control to the scheduler (await point)
    //   coop_run()        -> void drive all tasks to completion
    //   coop_reset()      -> void clear the executor (queue + order log)
    //   coop_record(s)    -> void append a tag to the order log
    //   coop_order()      -> str  the recorded interleaving order
    // ---------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "coop_spawn".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        // The argument is a task expression handled specially at lowering; the
        // checker accepts any single argument type (Type::Error = wildcard).
        params: vec![("task".to_string(), Type::Error)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "coop_yield".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "coop_run".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "coop_reset".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "coop_record".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("tag".to_string(), Type::Str)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "coop_order".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![],
        ret: Type::Str,
    });

    // close_chan(ch: chan) -> void — close a channel (further sends panic, recvs drain then 0)
    checker.env.define_function(FunctionSig {
        name: "close_chan".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("ch".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // ---------------------------------------------------------------------
    // Low-level FFI helpers — used by stdlib `extern { ... }` blocks that
    // need raw byte access. All take/return i64 (handles or pointers cast
    // to i64) to avoid distinguishing between `ptr` and `i64` in the user
    // surface.
    // ---------------------------------------------------------------------

    // str_to_ptr(s: str) -> i64 — raw data pointer (as i64) of a string.
    checker.env.define_function(FunctionSig {
        name: "str_to_ptr".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // buf_to_str(ptr: i64, len: i64) -> str — copy `len` bytes at `ptr` to a new string.
    checker.env.define_function(FunctionSig {
        name: "buf_to_str".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("len".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // alloc(size: i64) -> i64 — allocate `size` zero-initialized bytes. Returns 0 on failure.
    checker.env.define_function(FunctionSig {
        name: "alloc".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("size".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // free_bytes(ptr: i64, size: i64) -> void — release memory from `alloc`.
    checker.env.define_function(FunctionSig {
        name: "free_bytes".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("size".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // ptr_byte_at(ptr: i64, i: i64) -> i64 — read byte at offset.
    checker.env.define_function(FunctionSig {
        name: "ptr_byte_at".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("i".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // ptr_set_byte(ptr: i64, i: i64, b: i64) -> void — write byte at offset.
    checker.env.define_function(FunctionSig {
        name: "ptr_set_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("i".to_string(), Type::I64),
            ("b".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // handle_to_str(handle: i64) -> str — reinterpret KryosString handle as typed str.
    checker.env.define_function(FunctionSig {
        name: "handle_to_str".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::Str,
    });

    // ptr_read_i64(ptr: i64, i: i64) -> i64 — read 8-byte slot at i*8.
    checker.env.define_function(FunctionSig {
        name: "ptr_read_i64".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("i".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // ptr_write_i64(ptr: i64, i: i64, v: i64) -> void.
    checker.env.define_function(FunctionSig {
        name: "ptr_write_i64".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("i".to_string(), Type::I64),
            ("v".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // assert(condition: bool, msg: str) -> void — abort if condition is false
    checker.env.define_function(FunctionSig {
        name: "assert".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("condition".to_string(), Type::Bool),
            ("msg".to_string(), Type::Str),
        ],
        ret: Type::Void,
    });

    // assert_eq(left, right) -> void — abort if left != right, printing both
    // values. Codegen converts each argument to its stringified form via the
    // type-aware `to_string` lowering, so the runtime always sees two strings.
    // Both args use Type::Error to accept any type (matched at codegen).
    checker.env.define_function(FunctionSig {
        name: "assert_eq".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("left".to_string(), Type::Error),
            ("right".to_string(), Type::Error),
        ],
        ret: Type::Void,
    });

    // panic(msg: str) -> void — abort the process with the given message.
    // Return type is void but the call never actually returns; control
    // flow analysis treats it like a regular call for now.
    checker.env.define_function(FunctionSig {
        name: "panic".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("msg".to_string(), Type::Str)],
        ret: Type::Void,
    });

    // parse_int(s: str) -> i64 — parse string to integer (0 on failure)
    checker.env.define_function(FunctionSig {
        name: "parse_int".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // parse_float(s: str) -> f64 — parse string to float (0.0 on failure)
    checker.env.define_function(FunctionSig {
        name: "parse_float".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::F64,
    });

    // Overflow-aware integer arithmetic. All currently operate on i64.
    // wrapping_*  : explicit 2's-complement wrap (same as default behavior)
    // checked_*   : panic on overflow with a clear message
    // saturating_*: clamp to INT64_MIN / INT64_MAX on overflow
    for op in [
        "wrapping_add",
        "wrapping_sub",
        "wrapping_mul",
        "checked_add",
        "checked_sub",
        "checked_mul",
        "saturating_add",
        "saturating_sub",
        "saturating_mul",
    ] {
        checker.env.define_function(FunctionSig {
            name: op.to_string(),
            generic_params: vec![],
            generic_var_ids: vec![],
            params: vec![
                ("a".to_string(), Type::I64),
                ("b".to_string(), Type::I64),
            ],
            ret: Type::I64,
        });
    }

    // type_of(value: any) -> str — returns type name (always "i64" at runtime)
    checker.env.define_function(FunctionSig {
        name: "type_of".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Str,
    });

    // char_code(c: str) -> i64 — Unicode code point of first character
    checker.env.define_function(FunctionSig {
        name: "char_code".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("c".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // char_from(n: i64) -> str — single-character string from code point
    checker.env.define_function(FunctionSig {
        name: "char_from".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("n".to_string(), Type::I64)],
        ret: Type::Str,
    });

    // substr(s: str, start: i64, end: i64) -> str — substring [start..end)
    checker.env.define_function(FunctionSig {
        name: "substr".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("s".to_string(), Type::Str),
            ("start".to_string(), Type::I64),
            ("end".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // push<T>(arr: [T], val: T) -> [T] — generic so the element type flows:
    // `let mut a = []; a = push(a, X)` infers `a: [X]`.
    let push_t = {
        let v = checker.engine.fresh_var();
        if let Type::Var(id) = v { id } else { unreachable!() }
    };
    checker.env.define_function(FunctionSig {
        name: "push".to_string(),
        generic_params: vec!["T".to_string()],
        generic_var_ids: vec![push_t],
        params: vec![
            (
                "arr".to_string(),
                Type::Array { element: Box::new(Type::Var(push_t)), size: None },
            ),
            ("val".to_string(), Type::Var(push_t)),
        ],
        ret: Type::Array { element: Box::new(Type::Var(push_t)), size: None },
    });

    // pop<T>(arr: [T]) -> T — remove and return last element
    let pop_t = {
        let v = checker.engine.fresh_var();
        if let Type::Var(id) = v { id } else { unreachable!() }
    };
    checker.env.define_function(FunctionSig {
        name: "pop".to_string(),
        generic_params: vec!["T".to_string()],
        generic_var_ids: vec![pop_t],
        params: vec![(
            "arr".to_string(),
            Type::Array { element: Box::new(Type::Var(pop_t)), size: None },
        )],
        ret: Type::Var(pop_t),
    });

    // sort(arr: any) -> any — in-place ascending sort
    checker.env.define_function(FunctionSig {
        name: "sort".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("arr".to_string(), Type::Error)],
        ret: Type::Error,
    });

    // reverse(arr: any) -> any — in-place reverse
    checker.env.define_function(FunctionSig {
        name: "reverse".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("arr".to_string(), Type::Error)],
        ret: Type::Error,
    });

    // int(x: any) -> i64 — convert to integer
    checker.env.define_function(FunctionSig {
        name: "int".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::Error)],
        ret: Type::I64,
    });

    // float(x: any) -> f64 — convert to float
    checker.env.define_function(FunctionSig {
        name: "float".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::Error)],
        ret: Type::F64,
    });

    // sqrt(x: f64) -> f64 — square root
    checker.env.define_function(FunctionSig {
        name: "sqrt".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // floor(x: f64) -> f64 — floor
    checker.env.define_function(FunctionSig {
        name: "floor".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // ceil(x: f64) -> f64 — ceiling
    checker.env.define_function(FunctionSig {
        name: "ceil".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // round(x: f64) -> f64 — round to nearest integer (ties to even)
    checker.env.define_function(FunctionSig {
        name: "round".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // abs(x: T) -> T — absolute value (polymorphic: i64 or f64)
    {
        let abs_tv = checker.engine.fresh_var();
        let abs_var_id = if let Type::Var(id) = &abs_tv {
            vec![*id]
        } else {
            vec![]
        };
        checker.env.define_function(FunctionSig {
            name: "abs".to_string(),
            generic_params: vec!["T".to_string()],
            generic_var_ids: abs_var_id,
            params: vec![("x".to_string(), abs_tv.clone())],
            ret: abs_tv,
        });
    }

    // min(a: T, b: T) -> T — minimum of two values (polymorphic: i64 or f64)
    {
        let min_tv = checker.engine.fresh_var();
        let min_var_id = if let Type::Var(id) = &min_tv {
            vec![*id]
        } else {
            vec![]
        };
        checker.env.define_function(FunctionSig {
            name: "min".to_string(),
            generic_params: vec!["T".to_string()],
            generic_var_ids: min_var_id,
            params: vec![
                ("a".to_string(), min_tv.clone()),
                ("b".to_string(), min_tv.clone()),
            ],
            ret: min_tv,
        });
    }

    // max(a: T, b: T) -> T — maximum of two values (polymorphic: i64 or f64)
    {
        let max_tv = checker.engine.fresh_var();
        let max_var_id = if let Type::Var(id) = &max_tv {
            vec![*id]
        } else {
            vec![]
        };
        checker.env.define_function(FunctionSig {
            name: "max".to_string(),
            generic_params: vec!["T".to_string()],
            generic_var_ids: max_var_id,
            params: vec![
                ("a".to_string(), max_tv.clone()),
                ("b".to_string(), max_tv.clone()),
            ],
            ret: max_tv,
        });
    }

    // sin(x: f64) -> f64
    checker.env.define_function(FunctionSig {
        name: "sin".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // cos(x: f64) -> f64
    checker.env.define_function(FunctionSig {
        name: "cos".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // tan(x: f64) -> f64
    checker.env.define_function(FunctionSig {
        name: "tan".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // log(x: f64) -> f64 — natural logarithm
    checker.env.define_function(FunctionSig {
        name: "log".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // log2(x: f64) -> f64
    checker.env.define_function(FunctionSig {
        name: "log2".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // log10(x: f64) -> f64
    checker.env.define_function(FunctionSig {
        name: "log10".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // pow(x: f64, y: f64) -> f64 — x to the power of y
    checker.env.define_function(FunctionSig {
        name: "pow".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64), ("y".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // abs_f(x: f64) -> f64 — absolute value for floats
    checker.env.define_function(FunctionSig {
        name: "abs_f".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // min_f(a: f64, b: f64) -> f64 — minimum of two floats
    checker.env.define_function(FunctionSig {
        name: "min_f".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("a".to_string(), Type::F64), ("b".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // max_f(a: f64, b: f64) -> f64 — maximum of two floats
    checker.env.define_function(FunctionSig {
        name: "max_f".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("a".to_string(), Type::F64), ("b".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // keys(m: any) -> [str] — get map keys
    checker.env.define_function(FunctionSig {
        name: "keys".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
        params: vec![("ms".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // ── Byte buffer builtins ──────────────────────────────────────

    // buf_new(capacity: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "buf_new".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("capacity".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // buf_write_byte(handle: i64, byte: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // buf_get_byte(handle: i64, offset: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "buf_get_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
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
        generic_var_ids: vec![],
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // args() -> [str] — command-line arguments
    checker.env.define_function(FunctionSig {
        name: "args".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![],
        ret: Type::Array {
            element: Box::new(Type::Str),
            size: None,
        },
    });

    // ── Self-host intrinsics ─────────────────────────────────────
    //
    // The self-hosted runtime (`compiler/self-host/runtime.kry`) wraps a
    // handful of low-level intrinsics. For stage-0 builds (Rust compiler →
    // stage-1) the Rust runtime provides these as C-callable symbols. For
    // stage-1+ self-compiled builds, the codegen emits inline syscall /
    // mov / load / store instructions. Either way the type checker has
    // to know about them so `runtime.kry` passes `check`.

    // Linux x86_64 syscalls. The codegen recognises these names and emits
    // the `syscall` instruction with the appropriate argument registers.
    for (name, arity) in [
        ("syscall1", 1usize),
        ("syscall2", 2),
        ("syscall3", 3),
        ("syscall6", 6),
    ] {
        let mut params = vec![("nr".to_string(), Type::I64)];
        for i in 1..=arity {
            params.push((format!("a{i}"), Type::I64));
        }
        checker.env.define_function(FunctionSig {
            name: name.to_string(),
            generic_params: vec![],
            generic_var_ids: vec![],
            params,
            ret: Type::I64,
        });
    }

    // Raw memory ops. Codegen emits inline load/store instructions.
    checker.env.define_function(FunctionSig {
        name: "mem_read_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("ptr".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "mem_write_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "mem_read_i64".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("ptr".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "mem_write_i64".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "mem_copy".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("src".to_string(), Type::I64),
            ("dst".to_string(), Type::I64),
            ("len".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // String view intrinsics. A Kryos `str` is an opaque handle; these
    // expose its byte length and data pointer for low-level work. The
    // self-hosted runtime sometimes carries the handle around as raw
    // `i64`, so the intrinsic accepts either a `str` or an `i64` handle.
    checker.env.define_function(FunctionSig {
        name: "str_byte_len".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Error)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "str_data_ptr".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Error)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "str_from_bytes".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("len".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // Numeric/process intrinsics.
    checker.env.define_function(FunctionSig {
        name: "__int_to_float".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("x".to_string(), Type::I64)],
        ret: Type::F64,
    });
    checker.env.define_function(FunctionSig {
        name: "__get_process_args".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![],
        ret: Type::Array {
            element: Box::new(Type::Str),
            size: None,
        },
    });

    // `__builtin_*` thin wrappers — the runtime's user-facing `len`,
    // `push`, `pop`, `range`, `map_*` functions delegate to these.
    // They mirror the existing high-level builtins but live under a
    // namespaced name so codegen can dispatch them differently from a
    // user-defined `len`/`push`.
    checker.env.define_function(FunctionSig {
        name: "__builtin_len".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("collection".to_string(), Type::Error)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "__builtin_push".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("arr".to_string(), Type::Error),
            ("val".to_string(), Type::Error),
        ],
        ret: Type::Error,
    });
    checker.env.define_function(FunctionSig {
        name: "__builtin_pop".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("arr".to_string(), Type::Error)],
        ret: Type::Error,
    });
    checker.env.define_function(FunctionSig {
        name: "__builtin_range".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("start".to_string(), Type::I64),
            ("end".to_string(), Type::I64),
        ],
        ret: Type::Array {
            element: Box::new(Type::I64),
            size: None,
        },
    });
    for name in [
        "__builtin_map_has",
        "__builtin_map_has_str",
        "__builtin_map_delete",
        "__builtin_map_delete_str",
    ] {
        checker.env.define_function(FunctionSig {
            name: name.to_string(),
            generic_params: vec![],
            generic_var_ids: vec![],
            params: vec![
                ("m".to_string(), Type::Error),
                ("key".to_string(), Type::Error),
            ],
            ret: Type::I64,
        });
    }
    for name in ["__builtin_map_keys", "__builtin_map_keys_str"] {
        checker.env.define_function(FunctionSig {
            name: name.to_string(),
            generic_params: vec![],
            generic_var_ids: vec![],
            params: vec![("m".to_string(), Type::Error)],
            ret: Type::Error,
        });
    }

    // ── String utility builtins ────────────────────────────────────

    // contains(haystack, needle) -> bool
    checker.env.define_function(FunctionSig {
        name: "contains".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        // BOTH params are lenient (Type::Error): contains works on a str
        // (substring search, needle: str), a str-keyed map (key membership,
        // needle: str), AND an int-keyed map (key membership, needle: i64).
        // The needle was previously hard-typed to str, which rejected the
        // documented `contains(m, k)` on map<i64, _> with E0100. MIR lowering
        // dispatches to kryos_map_has[_str] vs kryos_builtin_contains by the
        // first arg's type (and the map's KEY type), so leniency here is safe.
        params: vec![
            ("haystack".to_string(), Type::Error),
            ("needle".to_string(), Type::Error),
        ],
        ret: Type::Bool,
    });

    // starts_with(s: str, prefix: str) -> bool
    checker.env.define_function(FunctionSig {
        name: "starts_with".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("s".to_string(), Type::Str),
            ("prefix".to_string(), Type::Str),
        ],
        ret: Type::Bool,
    });

    // ends_with(s: str, suffix: str) -> bool
    checker.env.define_function(FunctionSig {
        name: "ends_with".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("s".to_string(), Type::Str),
            ("suffix".to_string(), Type::Str),
        ],
        ret: Type::Bool,
    });

    // trim(s: str) -> str
    checker.env.define_function(FunctionSig {
        name: "trim".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // to_upper(s: str) -> str
    checker.env.define_function(FunctionSig {
        name: "to_upper".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // to_lower(s: str) -> str
    checker.env.define_function(FunctionSig {
        name: "to_lower".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // replace(s: str, from: str, to: str) -> str
    checker.env.define_function(FunctionSig {
        name: "replace".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("s".to_string(), Type::Str),
            ("from".to_string(), Type::Str),
            ("to".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });

    // split(s: str, delimiter: str) -> [str]
    checker.env.define_function(FunctionSig {
        name: "split".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("s".to_string(), Type::Str),
            ("delimiter".to_string(), Type::Str),
        ],
        ret: Type::Array {
            element: Box::new(Type::Str),
            size: None,
        },
    });

    // join(arr: [str], separator: str) -> str
    checker.env.define_function(FunctionSig {
        name: "join".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            (
                "arr".to_string(),
                Type::Array {
                    element: Box::new(Type::Str),
                    size: None,
                },
            ),
            ("separator".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });

    // tcp_connect(host: str, port: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "tcp_connect".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("host".to_string(), Type::Str),
            ("port".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // tcp_listen(host: str, port: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "tcp_listen".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("host".to_string(), Type::Str),
            ("port".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // tcp_accept(fd: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "tcp_accept".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("fd".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // tcp_send(fd: i64, data: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "tcp_send".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("fd".to_string(), Type::I64),
            ("data".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // tcp_recv(fd: i64, max_bytes: i64) -> str
    checker.env.define_function(FunctionSig {
        name: "tcp_recv".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("fd".to_string(), Type::I64),
            ("max_bytes".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // tcp_close(fd: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "tcp_close".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("fd".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // -----------------------------------------------------------------------
    // Async / non-blocking primitives (Gap 3 minimum viable async)
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "tcp_set_nonblocking".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("fd".to_string(), Type::I64), ("nonblocking".to_string(), Type::Bool)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "tcp_try_accept".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("listener_fd".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "tcp_try_recv".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("fd".to_string(), Type::I64), ("max_bytes".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "sleep_ms".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("ms".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // -----------------------------------------------------------------------
    // TLS server builtins (Gap A)
    // -----------------------------------------------------------------------
    // tls_server_config(cert_path: str, key_path: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "tls_server_config".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("cert_path".to_string(), Type::Str),
            ("key_path".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // tls_accept(client_tcp_fd: i64, config_handle: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "tls_accept".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("client_tcp_fd".to_string(), Type::I64),
            ("config_handle".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // tls_send(fd: i64, data: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "tls_send".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("fd".to_string(), Type::I64),
            ("data".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // tls_recv(fd: i64, max_bytes: i64) -> str
    checker.env.define_function(FunctionSig {
        name: "tls_recv".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("fd".to_string(), Type::I64),
            ("max_bytes".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // tls_close(fd: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "tls_close".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("fd".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // -----------------------------------------------------------------------
    // PostgreSQL builtins (Gap B)
    // -----------------------------------------------------------------------
    // pg_connect(conn_str: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "pg_connect".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("conn_str".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // pg_exec(handle: i64, sql: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "pg_exec".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("sql".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // pg_query(handle: i64, sql: str) -> str
    checker.env.define_function(FunctionSig {
        name: "pg_query".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![
            ("handle".to_string(), Type::I64),
            ("sql".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });

    // pg_close(handle: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "pg_close".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // -----------------------------------------------------------------------
    // Unix domain sockets (v2.0)
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "uds_connect".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("path".to_string(), Type::Str)], ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "uds_bind".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("path".to_string(), Type::Str)], ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "uds_accept".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("fd".to_string(), Type::I64)], ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "uds_send".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("fd".to_string(), Type::I64), ("data".to_string(), Type::Str)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "uds_recv".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("fd".to_string(), Type::I64), ("max_bytes".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "uds_close".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("fd".to_string(), Type::I64)], ret: Type::I64,
    });

    // -----------------------------------------------------------------------
    // WebSocket (RFC 6455) helpers (v2.0)
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "ws_accept_key".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("key".to_string(), Type::Str)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_encode_text".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("payload".to_string(), Type::Str)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_encode_binary".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("payload".to_string(), Type::Str)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_encode_close".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("code".to_string(), Type::I64)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_encode_ping".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("payload".to_string(), Type::Str)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_encode_pong".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("payload".to_string(), Type::Str)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_unmask".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("buf".to_string(), Type::Str),
            ("payload_off".to_string(), Type::I64),
            ("payload_len".to_string(), Type::I64),
            ("mask_off".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_read_frame".to_string(), generic_params: vec![], generic_var_ids: vec![],
        params: vec![("fd".to_string(), Type::I64)], ret: Type::Str,
    });

    // -----------------------------------------------------------------------
    // JSON builtins
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "json_parse".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_stringify".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "json_object".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("keys".to_string(), Type::Array { element: Box::new(Type::Str), size: None }),
            ("vals".to_string(), Type::Array { element: Box::new(Type::I64), size: None }),
        ],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_array".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("items".to_string(), Type::Array { element: Box::new(Type::I64), size: None })],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_string".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_number".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("v".to_string(), Type::F64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_bool".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("v".to_string(), Type::Bool)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_null".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_get".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("obj".to_string(), Type::I64), ("key".to_string(), Type::Str)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_get_index".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("arr".to_string(), Type::I64), ("idx".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_to_str".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "json_to_int".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_to_float".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::F64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_is_null".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::Bool,
    });
    checker.env.define_function(FunctionSig {
        name: "json_length".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_type".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::Str,
    });

    // -----------------------------------------------------------------------
    // Crypto / hashing
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "sha256".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "sha512".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "hmac_sha256".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("key".to_string(), Type::Str), ("data".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ed25519_generate".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ed25519_public".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("pkcs8_hex".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ed25519_sign".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("pkcs8_hex".to_string(), Type::Str), ("msg".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ed25519_verify".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("pub_hex".to_string(), Type::Str),
            ("msg".to_string(), Type::Str),
            ("sig_hex".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "hex_to_base64url".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("hex".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "base64url_to_hex".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("b64url".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "pbkdf2_sha256".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("password".to_string(), Type::Str),
            ("salt_hex".to_string(), Type::Str),
            ("iters".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "random_bytes".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("n".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "sha1_hex".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "sha1_base64".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "base64_encode".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "base64_decode".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "chr".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("n".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "byte_at".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("s".to_string(), Type::Str), ("idx".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // -----------------------------------------------------------------------
    // Regex
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "regex_new".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("pattern".to_string(), Type::Str)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_match".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("re".to_string(), Type::I64), ("text".to_string(), Type::Str)],
        ret: Type::Bool,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_find".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("re".to_string(), Type::I64), ("text".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_find_pos".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("re".to_string(), Type::I64),
            ("text".to_string(), Type::Str),
            ("from".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_find_end".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("re".to_string(), Type::I64),
            ("text".to_string(), Type::Str),
            ("from".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_replace_all".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("re".to_string(), Type::I64),
            ("text".to_string(), Type::Str),
            ("replacement".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_drop".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("re".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // -----------------------------------------------------------------------
    // HTTP / HTTPS
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "http_request".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("method".to_string(), Type::Str),
            ("url".to_string(), Type::Str),
            ("headers".to_string(), Type::Str),
            ("body".to_string(), Type::Str),
            ("timeout_ms".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "https_get".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("url".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // -----------------------------------------------------------------------
    // HTTP/2 client (Gap C) — reqwest-backed, ALPN h2 with HTTP/1.1 fallback
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "http2_get".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("url".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "http2_post".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("url".to_string(), Type::Str),
            ("body".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "http2_request".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("method".to_string(), Type::Str),
            ("url".to_string(), Type::Str),
            ("headers".to_string(), Type::Str),
            ("body".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });

    // -----------------------------------------------------------------------
    // WASM v0.4 web builtins (DOM / canvas / fetch / alert)
    // These compile to host imports under --backend wasm and to runtime
    // no-ops / native equivalents under cranelift/llvm (best-effort).
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "dom_set_text".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("id".to_string(), Type::Str), ("text".to_string(), Type::Str)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "dom_get_value".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("id".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "alert".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("msg".to_string(), Type::Str)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "canvas_fill_rect".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![
            ("canvas_id".to_string(), Type::Str),
            ("x".to_string(), Type::I64),
            ("y".to_string(), Type::I64),
            ("w".to_string(), Type::I64),
            ("h".to_string(), Type::I64),
            ("color".to_string(), Type::Str),
        ],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "canvas_clear".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("canvas_id".to_string(), Type::Str)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "fetch_text".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("url".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // -----------------------------------------------------------------------
    // Time / Mutex
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "time_now_secs".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "time_now_millis".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![],
        ret: Type::I64,
    });
    // `time_millis` is the documented short alias for `time_now_millis`.
    checker.env.define_function(FunctionSig {
        name: "time_millis".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "mutex_new".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "mutex_lock".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("m".to_string(), Type::I64)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "mutex_unlock".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("m".to_string(), Type::I64)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "mutex_drop".to_string(),
        generic_params: vec![], generic_var_ids: vec![],
        params: vec![("m".to_string(), Type::I64)],
        ret: Type::Void,
    });

    checker.check_module(module);
    // Resolve recorded empty-array let bindings through the (now fully unified)
    // engine and convert to TypeExpr for the MIR. Only keep those whose element
    // resolved to a concrete (non-var) type.
    let let_types: std::collections::HashMap<Span, TypeExpr> = checker
        .resolved_let_types
        .iter()
        .filter_map(|(span, ty)| {
            let resolved = checker.engine.resolve(ty);
            concrete_type_to_type_expr(&resolved).map(|te| (*span, te))
        })
        .collect();
    (
        std::mem::take(&mut checker.diagnostics),
        std::mem::take(&mut checker.resolved_lambda_params),
        let_types,
    )
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

/// Returns true if a match-arm body expression is guaranteed to
/// diverge (return/throw on every path) and therefore should not
/// contribute its type to the match expression's unified type.
fn arm_body_diverges(body: &Expr) -> bool {
    match body {
        Expr::Block { block, .. } => block_returns(block),
        _ => false,
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
            let else_ok = else_block.as_ref().is_some_and(block_returns);
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
        // A deny block returns iff its body returns on all paths.
        Stmt::DenyBlock { body, .. } => block_returns(body),
        _ => false,
    }
}
