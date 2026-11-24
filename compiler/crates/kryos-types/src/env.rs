//! Type environment — scope-based symbol table for type checking.
//!
//! Tracks variable types, function signatures, struct/enum definitions,
//! trait methods, and impl blocks across nested lexical scopes.

use std::collections::{HashMap, HashSet};

use crate::ty::{CapVarId, Type};

/// Information about a struct definition.
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub generic_params: Vec<String>,
    /// Type variable IDs for generic params, 1:1 with `generic_params`.
    /// Field types reference these IDs; per-use monomorphization substitutes
    /// them with fresh vars via `InferenceEngine::instantiate`.
    pub generic_var_ids: Vec<u32>,
    pub fields: Vec<(String, Type)>,
}

/// Information about an enum definition.
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub generic_params: Vec<String>,
    /// Type variable IDs for generic params (see `StructDef::generic_var_ids`).
    pub generic_var_ids: Vec<u32>,
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
    /// Capability-row variable IDs for every UNANNOTATED fn-typed parameter
    /// or return position in this declaration's OWN signature (not counting
    /// row vars nested further inside a called/returned function's own
    /// signature). Populated independently of `generic_var_ids` — a
    /// type-monomorphic HOF (`fn apply(f: fn()->str) -> str`) gets a fresh
    /// row var here even with `generic_var_ids` empty, per
    /// `docs/capability-effects-spec.md` §2.3's generalization rule (the
    /// annotation-burden win over tying row-var creation to `<T, U>`).
    /// Freshened at each call site by `InferenceEngine::instantiate_sig`,
    /// exactly like `generic_var_ids`.
    pub generic_cap_var_ids: Vec<CapVarId>,
    /// A SEPARATE capability-row var representing this declaration's OWN
    /// total inferred capability requirement (the union of its direct
    /// gated-builtin calls plus the rows of everything it calls,
    /// including — for a HOF — its own `generic_cap_var_ids` entries).
    /// Bound exactly once, by `check_decl`, after the declaration's body
    /// has been walked; every REFERENCE to this function as a value reads
    /// it (freshening any `generic_cap_var_ids` it happens to mention,
    /// via `InferenceEngine::instantiate_row`) rather than the raw
    /// template value, so two different call sites of a row-polymorphic
    /// HOF never share a binding. Deliberately NOT itself a member of
    /// `generic_cap_var_ids` — see `check.rs`'s `Expr::Identifier` arm for
    /// the resolve-then-instantiate sequencing this requires.
    pub own_cap_var: CapVarId,
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
        self.current_scope_mut().enums.insert(def.name.clone(), def);
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

    /// Find the enum that declares a variant with the given name (searching all
    /// scopes). Resolves bare/unqualified variant constructors like `Some(x)`
    /// (-> Option) and `Ok(v)` (-> Result). Returns the first match; the
    /// qualified `Enum.Variant` form disambiguates name collisions.
    pub fn find_enum_by_variant(&self, variant: &str) -> Option<&EnumDef> {
        // DETERMINISTIC resolution. `scope.enums` is a HashMap, so `.values()`
        // iterates in per-process-randomized order. An ambiguous bare variant --
        // a user enum sharing a variant name with std `Option`/`Result` (e.g.
        // `enum MyBox { Some(str) }` alongside `Option::Some`) -- previously
        // resolved to whichever enum the hasher visited first, so the SAME source
        // compiled differently (and to a permanently-wrong binary) across runs.
        // Within each scope (innermost first) collect every candidate, then prefer
        // stdlib Option/Result on a collision, else the lexicographically-first
        // enum name. Fully deterministic; qualified `Enum.Variant` still resolves
        // exactly and bypasses this bare-name path.
        for scope in self.scopes.iter().rev() {
            let mut matches: Vec<&EnumDef> = scope
                .enums
                .values()
                .filter(|def| def.variants.iter().any(|(v, _)| v == variant))
                .collect();
            if matches.is_empty() {
                continue;
            }
            matches.sort_by(|a, b| {
                let rank = |n: &str| if n == "Option" || n == "Result" { 0 } else { 1 };
                rank(&a.name).cmp(&rank(&b.name)).then(a.name.cmp(&b.name))
            });
            return Some(matches[0]);
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

    /// Collect every method name visible for the given type (across all impls
    /// in scope). Used by the "did you mean?" diagnostics.
    pub fn all_method_names(&self, type_name: &str) -> Vec<String> {
        let mut names = Vec::new();
        for scope in self.scopes.iter().rev() {
            for ((ty, _trait), methods) in &scope.impls {
                if ty == type_name {
                    for m in methods {
                        if !names.contains(&m.name) {
                            names.push(m.name.clone());
                        }
                    }
                }
            }
        }
        names
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
        self.scopes
            .last_mut()
            .expect("at least one scope must exist")
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
