//! Type environment — scope-based symbol table for type checking.
//!
//! Tracks variable types, function signatures, struct/enum definitions,
//! trait methods, and impl blocks across nested lexical scopes.

use std::collections::{HashMap, HashSet};

use crate::ty::Type;

/// Information about a struct definition.
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub generic_params: Vec<String>,
    pub fields: Vec<(String, Type)>,
}

/// Information about an enum definition.
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub generic_params: Vec<String>,
    pub variants: Vec<(String, Vec<Type>)>,
}

/// Information about a function signature.
#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub name: String,
    pub generic_params: Vec<String>,
    /// Type variable IDs corresponding 1:1 with `generic_params`.
    /// Used to instantiate fresh type variables at each call site
    /// so that generic functions monomorphize correctly.
    pub generic_var_ids: Vec<u32>,
    pub params: Vec<(String, Type)>,
    pub ret: Type,
}

/// Information about a trait definition.
#[derive(Debug, Clone)]
pub struct TraitDef {
    pub name: String,
    pub generic_params: Vec<String>,
    pub methods: Vec<FunctionSig>,
}

/// A single lexical scope level.
#[derive(Debug, Clone)]
struct Scope {
    /// Variable name -> type
    variables: HashMap<String, Type>,
    /// Variables declared with `let mut` — assignment allowed.
    mutable_vars: HashSet<String>,
    /// Function name -> signature
    functions: HashMap<String, FunctionSig>,
    /// Struct name -> definition
    structs: HashMap<String, StructDef>,
    /// Enum name -> definition
    enums: HashMap<String, EnumDef>,
    /// Trait name -> definition
    traits: HashMap<String, TraitDef>,
    /// (Type name, trait name) -> method implementations
    impls: HashMap<(String, Option<String>), Vec<FunctionSig>>,
}

impl Scope {
    fn new() -> Self {
        Self {
            variables: HashMap::new(),
            mutable_vars: HashSet::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            traits: HashMap::new(),
            impls: HashMap::new(),
        }
    }
}

/// Scope-based type environment.
///
/// Supports nested scopes (function bodies, blocks, etc.) with proper
/// shadowing semantics. Inner scopes can shadow outer bindings.
#[derive(Debug)]
pub struct TypeEnv {
    scopes: Vec<Scope>,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeEnv {
    /// Create a new environment with a single global scope.
    pub fn new() -> Self {
        Self {
            scopes: vec![Scope::new()],
        }
    }

    /// Push a new nested scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    /// Pop the innermost scope.
    pub fn pop_scope(&mut self) {
        debug_assert!(self.scopes.len() > 1, "cannot pop the global scope");
        self.scopes.pop();
    }

    // ── Variables ──────────────────────────────────────────────────────

    /// Define a variable in the current scope.
    pub fn define_var(&mut self, name: String, ty: Type) {
        self.current_scope_mut().variables.insert(name, ty);
    }

    /// Define a mutable variable in the current scope.
    pub fn define_var_mut(&mut self, name: String, ty: Type) {
        let scope = self.current_scope_mut();
        scope.variables.insert(name.clone(), ty);
        scope.mutable_vars.insert(name);
    }

    /// Check whether a variable is mutable (declared with `let mut`).
    pub fn is_mutable(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if scope.variables.contains_key(name) {
                return scope.mutable_vars.contains(name);
            }
        }
        false
    }

    /// Look up a variable, searching from innermost to outermost scope.
    pub fn lookup_var(&self, name: &str) -> Option<&Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.variables.get(name) {
                return Some(ty);
            }
        }
        None
    }

    // ── Functions ─────────────────────────────────────────────────────

    /// Define a function signature in the current scope.
    pub fn define_function(&mut self, sig: FunctionSig) {
        self.current_scope_mut()
            .functions
            .insert(sig.name.clone(), sig);
    }

    /// Look up a function signature by name.
    pub fn lookup_function(&self, name: &str) -> Option<&FunctionSig> {
        for scope in self.scopes.iter().rev() {
            if let Some(sig) = scope.functions.get(name) {
                return Some(sig);
            }
        }
        None
    }

    // ── Structs ───────────────────────────────────────────────────────

    /// Define a struct in the current scope.
    pub fn define_struct(&mut self, def: StructDef) {
        self.current_scope_mut()
            .structs
            .insert(def.name.clone(), def);
    }

    /// Look up a struct definition by name.
    pub fn lookup_struct(&self, name: &str) -> Option<&StructDef> {
        for scope in self.scopes.iter().rev() {
            if let Some(def) = scope.structs.get(name) {
                return Some(def);
            }
        }
        None
    }

    // ── Enums ─────────────────────────────────────────────────────────

    /// Define an enum in the current scope.
    pub fn define_enum(&mut self, def: EnumDef) {
        self.current_scope_mut()
            .enums
            .insert(def.name.clone(), def);
    }

    /// Look up an enum definition by name.
    pub fn lookup_enum(&self, name: &str) -> Option<&EnumDef> {
        for scope in self.scopes.iter().rev() {
            if let Some(def) = scope.enums.get(name) {
                return Some(def);
            }
        }
        None
    }

    // ── Traits ────────────────────────────────────────────────────────

    /// Define a trait in the current scope.
    pub fn define_trait(&mut self, def: TraitDef) {
        self.current_scope_mut()
            .traits
            .insert(def.name.clone(), def);
    }

    /// Look up a trait definition by name.
    pub fn lookup_trait(&self, name: &str) -> Option<&TraitDef> {
        for scope in self.scopes.iter().rev() {
            if let Some(def) = scope.traits.get(name) {
                return Some(def);
            }
        }
        None
    }

    // ── Impls ─────────────────────────────────────────────────────────

    /// Register an impl block's methods.
    pub fn define_impl(
        &mut self,
        type_name: String,
        trait_name: Option<String>,
        methods: Vec<FunctionSig>,
    ) {
        self.current_scope_mut()
            .impls
            .insert((type_name, trait_name), methods);
    }

    /// Look up methods on a type (optionally from a specific trait impl).
    pub fn lookup_method(&self, type_name: &str, method_name: &str) -> Option<&FunctionSig> {
        for scope in self.scopes.iter().rev() {
            for ((ty, _trait), methods) in &scope.impls {
                if ty == type_name {
                    if let Some(sig) = methods.iter().find(|m| m.name == method_name) {
                        return Some(sig);
                    }
                }
            }
        }
        None
    }

    /// Look up a field type on a struct.
    pub fn lookup_field(&self, type_name: &str, field_name: &str) -> Option<&Type> {
        if let Some(def) = self.lookup_struct(type_name) {
            for (fname, fty) in &def.fields {
                if fname == field_name {
                    return Some(fty);
                }
            }
        }
        None
    }

    // ── Helpers ───────────────────────────────────────────────────────

    fn current_scope_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().expect("at least one scope must exist")
    }

    // ── Name collection (for "did you mean?" suggestions) ────────────

    /// Collect all variable names visible in the current scope chain.
    pub fn all_var_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for scope in self.scopes.iter().rev() {
            for name in scope.variables.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            for name in scope.functions.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names
    }

    /// Collect all type names (structs + enums) visible in scope.
    pub fn all_type_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for scope in self.scopes.iter().rev() {
            for name in scope.structs.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            for name in scope.enums.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names
    }

    /// Collect all struct names visible in scope.
    pub fn all_struct_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for scope in self.scopes.iter().rev() {
            for name in scope.structs.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names
    }
}
