//! AST -> MIR lowering pass.
//!
//! Converts a typed Kryos AST (`kryos_ast::Module`) into the MIR control-flow
//! graph representation (`MirModule`).  The lowerer walks each function body,
//! creating basic blocks, instructions, and terminators.

use std::collections::{HashMap, HashSet};

use kryos_ast::{self as ast};
use kryos_types::Type;

use crate::ir::*;

// ---------------------------------------------------------------------------
// Lowering context
// ---------------------------------------------------------------------------

/// Stateful context that drives the AST -> MIR translation.
pub struct LoweringContext {
    /// Accumulated locals for the current function being lowered.
    locals: Vec<MirLocal>,
    /// Accumulated basic blocks for the current function.
    blocks: Vec<BasicBlock>,
    /// Instructions being collected for the *current* block.
    current_instructions: Vec<Instruction>,
    /// Id of the current (open) block.
    current_block: BlockId,
    /// Next local id counter.
    next_local: u32,
    /// Next block id counter.
    next_block: u32,
    /// Stack of loop headers for `continue`.
    loop_headers: Vec<BlockId>,
    /// Stack of loop exits for `break`.
    loop_exits: Vec<BlockId>,
    /// Struct definitions: struct_name -> ordered list of (field_name, MirType).
    struct_defs: HashMap<String, Vec<(String, MirType)>>,
    /// Enum definitions: enum_name -> ordered list of variants.
    enum_defs: HashMap<String, Vec<EnumVariantDef>>,
    /// Function return types: func_name -> MirType.
    func_ret_types: HashMap<String, MirType>,
    /// Method ownership: (TypeName, method_name) -> mangled function name.
    method_owners: HashMap<(String, String), String>,
    /// Trait definitions: trait_name -> list of required method signatures.
    trait_defs: HashMap<String, Vec<TraitMethodSig>>,
    /// Trait default method ASTs: trait_name -> list of methods that have bodies.
    trait_default_methods: HashMap<String, Vec<ast::Decl>>,
    /// Impl-for-trait map: (type_name, trait_name) -> list of mangled method names.
    impl_map: HashMap<(String, String), Vec<String>>,
    /// Generic function templates: func_name -> (generic_params, AST function decl).
    /// These are not lowered immediately; they are instantiated on demand at call sites.
    generic_templates: HashMap<String, GenericTemplate>,
    /// Generic impl ASSOCIATED-function templates: (target, method) -> template.
    /// Populated for a `self`-less method inside a generic `impl<T> Target<T>`
    /// block. Mirrors `generic_templates` for free functions: an associated
    /// fn that constructs and returns the generic struct/enum BY VALUE (e.g.
    /// `Box::new(v: T) -> Box<T>`) must be lowered ONCE PER concrete
    /// instantiation, because the struct literal inside its body picks the
    /// monomorphized layout from the (now-concrete) param types. Lowering it
    /// exactly once (the old behavior) always built the FIRST call site's
    /// concrete type, so a second instantiation at a different concrete type
    /// mismatched on the strict LLVM AOT backend (Cranelift JIT tolerated it
    /// since it only checks byte-size, not nominal struct type).
    generic_impl_fn_templates: HashMap<(String, String), GenericImplFnTemplate>,
    /// Generic struct templates: struct_name -> (generic_params, AST struct decl fields).
    generic_struct_templates: HashMap<String, GenericStructTemplate>,
    /// Mangled monomorphized name -> the concrete type args it was built
    /// with (e.g. "Boxed___str" -> [Str]). Lets generic-arg extraction match
    /// a generic param TypeExpr (`Boxed<T>`) against an already-mangled
    /// concrete struct/enum name.
    mono_instance_args: HashMap<String, Vec<MirType>>,
    /// (base struct, method) -> (impl generic names, self-param TypeExpr,
    /// return TypeExpr). Lets a generic method's return be resolved to the
    /// receiver's CONCRETE type at an inference site: bind the impl generics
    /// by matching the self param against the monomorphized receiver
    /// (`extract_type_bindings`), then substitute the return. Without it,
    /// `to_string(box_of_f64.get())` inferred the erased i64 slot (the method
    /// is lowered once under the base name) and printed raw f64 bits.
    impl_method_generic_info:
        HashMap<(String, String), (Vec<String>, Option<ast::TypeExpr>, Option<ast::TypeExpr>)>,
    /// Generic enum templates: enum_name -> (generic_params, AST enum variants).
    generic_enum_templates: HashMap<String, GenericEnumTemplate>,
    /// Already-monomorphized specializations, to avoid duplicate lowering.
    monomorphized: HashMap<String, bool>,
    /// Functions produced by monomorphization (collected after lowering).
    monomorphized_functions: Vec<MirFunction>,
    /// Struct/enum type names for which a `__kryos_eq_<Type>` structural
    /// equality helper has already been synthesized (see
    /// `ensure_struct_eq_helper` / `ensure_enum_eq_helper`). Keyed by the
    /// exact (already-monomorphized) type name so a type compared with `==`
    /// at multiple call sites gets exactly one helper, mirroring the
    /// `__kryos_drop_<Type>` per-type helper the codegen backends already
    /// generate for destructors.
    synthesized_eq_helpers: HashSet<String>,
    /// Counter for anonymous lambda function names.
    lambda_counter: u32,
    /// Resolved types for un-annotated lambda params, from the type checker
    /// (keyed by the lambda's span). Used to type closure params that would
    /// otherwise default to i64 (e.g. a `str` closure passed to a HOF).
    lambda_param_types: HashMap<kryos_errors::Span, Vec<Option<ast::TypeExpr>>>,
    /// Resolved types for unannotated empty-array `let` bindings (keyed by Let
    /// span), from the type checker. Used so `let x = []; push(x, S{..})` types
    /// x as `[S]` instead of the MIR's default `[i64]`.
    let_types: HashMap<kryos_errors::Span, ast::TypeExpr>,
    /// Generic-param -> concrete-type bindings active while lowering a
    /// monomorphized generic function/method body. Consulted when resolving a
    /// local `let`'s type annotation so `let x: T` / `let x: Box<T>` inside the
    /// body substitute to the concrete instantiation (else LLVM emits `%T`).
    /// Empty outside a monomorphized body.
    active_generic_bindings: HashMap<String, MirType>,
    /// Counter for spawn wrapper function names.
    spawn_counter: u32,
    /// Type alias map: alias_name -> resolved MirType.
    type_aliases: HashMap<String, MirType>,
    /// When inside a `try` block, holds the target local and check-block for `throw`.
    try_catch_target: Option<TryCatchTarget>,
    /// Tracks locals that are closures with captures: local_name -> (func_name, capture_operands).
    closure_locals: HashMap<String, (String, Vec<Operand>)>,
    /// Underlying lambda function names (e.g. `__lambda_0`) whose body
    /// mutates at least one of its captured variables. A mutating closure
    /// captures its state BY MOVE and must own a single persistent copy
    /// across calls, so it can never use the direct-call optimization that
    /// `closure_locals` enables (that shortcut re-reads the CURRENT value of
    /// the outer captured variable at every call site, which silently
    /// resets a counter/accumulator back to its initial value instead of
    /// threading the mutation forward). Populated when a Lambda is lowered;
    /// consulted when a `let` binds that lambda to decide whether to
    /// register it in `closure_locals` at all. Global for the whole
    /// compilation (like `func_ret_types`), not per-function scratch state.
    mutating_closures: HashSet<String>,
    /// Pending closure-local re-registrations for the next `lower_function`
    /// call. Used when lowering nested lambdas that capture other closures:
    /// the outer frame stages entries here so that after the inner frame
    /// allocates its parameter locals, body lowering can find the captured
    /// closures in `closure_locals` keyed by inner-frame local IDs. Each
    /// entry is (closure_local_name, real_function_name, capture_var_names).
    pending_closure_regs: Vec<(String, String, Vec<String>)>,
    /// Actor definitions: actor_name -> ordered list of (handler_name, param_count).
    actor_defs: HashMap<String, Vec<(String, usize)>>,
    /// The current `Self` type name — set when lowering trait/impl blocks.
    current_self_type: Option<String>,
    /// Type-parameter names of the `impl<...>` block currently being lowered
    /// (e.g. `["T"]` for `impl<T> Box<T>`). While set, `resolve_type` erases
    /// these names to `i64` -- the same slot-erasure generic structs use --
    /// so a generic impl method lowers to a single concrete-sized function
    /// instead of referencing an unsized `%T` LLVM type. Empty for concrete
    /// impls, so the self-host compiler (which has none) is unaffected.
    current_impl_generics: Vec<String>,
    /// Actor state field layouts: actor_name -> ordered list of (field_name, field_index).
    /// Each field occupies one i64 slot at offset field_index * 8.
    actor_state_fields: HashMap<String, Vec<(String, u32)>>,
    /// The actor whose handler is currently being lowered, if any. `self.field`
    /// inside a handler resolves against this (actor VALUES erase to i64, so the
    /// old "self's type is Struct(actor)" check no longer identifies them).
    current_actor: Option<String>,
    /// Fields (of any struct OR actor) whose declared type names an actor:
    /// (owner_type_name, field_name) -> actor_type_name. Captured at
    /// struct/actor field-registration time, BEFORE `resolve_type`'s
    /// actor-erasure rule turns the field's MirType into a bare `I64` slot.
    /// Consulted by `infer_type_name` so a method call on an actor-typed
    /// FIELD (`self.target.bump(..)` where `target: Counter`) is still
    /// recognized as an actor receiver and routed through actor dispatch,
    /// even though the field's physical (erased) type no longer says so.
    field_actor_types: HashMap<(String, String), String>,
    /// Locals (function/closure/spawn-body PARAMS, and explicitly-annotated
    /// `let`s) whose declared type names an actor: LocalId.0 -> actor_type_name.
    /// Captured at param/let registration time, BEFORE `resolve_type`'s
    /// actor-erasure rule turns the param's MirType into a bare `I64` slot.
    /// Sibling of `field_actor_types`: that map recovers an actor's logical
    /// type for a FIELD access; this one recovers it for a plain identifier
    /// bound to an actor value that arrived via an explicit type annotation
    /// (e.g. a `spawn { }` block's captured-variable parameter, or `let c:
    /// Counter = ...`). Without it, `infer_type_name` on a bare identifier
    /// sees only the erased `I64` and a method call on the actor lowers to a
    /// plain unmangled call instead of the ActorSend/mailbox dispatch — this
    /// is exactly what broke calling an actor method on a variable CAPTURED
    /// by a `spawn { }` block (the capture is re-materialized as a typed
    /// param, which goes through `resolve_type`, unlike the outer scope's
    /// unannotated `let col = Collector()` whose type is inferred directly
    /// from the constructor call and never erased).
    local_actor_types: HashMap<u32, String>,
    /// Functions/methods whose declared RETURN type names an actor:
    /// fn_name (or `Type__method` for impl methods) -> actor_type_name.
    /// Captured at signature-registration time (alongside `func_ret_types`),
    /// BEFORE `resolve_type`'s actor-erasure rule turns the return type into
    /// a bare `I64` handle. Third facet of the same family as
    /// `field_actor_types` (an actor recovered through a FIELD) and
    /// `local_actor_types` (an actor recovered through a typed PARAM/`let`):
    /// this one recovers it when the method-call RECEIVER is itself a call
    /// expression (`get_counter(c).bump(..)`) -- consulted by
    /// `infer_type_name`'s `FnCall` fallback.
    fn_return_actor_types: HashMap<String, String>,
    /// Top-level constant definitions: const_name -> (MirType, AST expression).
    const_defs: HashMap<String, (MirType, ast::Expr)>,
    /// Top-level mutable globals: name -> (MirType, init expression).
    /// These are real process-wide slots stored in the runtime globals
    /// registry. References to these names lower to `kryos_global_get`/
    /// `kryos_global_set` calls; the initializer expression is run once at
    /// the start of `main`.
    mutable_globals: HashMap<String, (MirType, ast::Expr)>,
    /// Insertion order of mutable globals so initialization runs in source
    /// order (later globals may reference earlier ones).
    mutable_global_order: Vec<String>,
    /// Function parameter types: func_name -> ordered list of MirType.
    /// Used for dyn Trait coercion: when a concrete struct is passed to a `dyn Trait`
    /// param, the lowerer wraps it in `MakeTraitObject`.
    func_param_types: HashMap<String, Vec<MirType>>,
    /// Tracks locals that have already been dropped by an inner scope to prevent
    /// double-free when the outer scope's drop loop runs.
    dropped_locals: HashSet<u32>,
    /// Locals that are function parameters — must NOT be dropped by the callee
    /// because the caller owns them.
    param_locals: HashSet<u32>,
    /// Name of the function currently being lowered (drop-tag diagnostics).
    cur_fn_name: String,
    /// Locals that borrow from another local (e.g., struct field access into a new
    /// struct). These must not be dropped because the source local owns the memory.
    borrowed_locals: HashSet<u32>,
    /// Return type of the function currently being lowered.  Used by `throw`
    /// outside a `try` block to emit a properly-typed early return.
    current_ret_ty: MirType,
    /// Structs annotated with `@copy` — forwarded to `MirModule` for codegen.
    copy_structs: HashSet<String>,
    /// Locals hidden from name resolution after their enclosing scope exits.
    /// Prevents inner-scope variables from shadowing outer ones after the
    /// inner block ends (e.g., `let x = 1; if true { let x = 2 }; println(x)`
    /// must print 1, not 2).
    hidden_locals: HashSet<u32>,
    /// Locals from which at least one non-copy field has been moved out.
    /// These must NOT be dropped by scope cleanup — the moved fields already
    /// own (and will free) their heap data; a full struct drop would double-free.
    partial_moved_locals: HashSet<u32>,
    /// Ref-captured scalar variable name -> every heap box (`RValue::ArcAlloc`
    /// cell) created for it so far in the CURRENT function. Populated when a
    /// struct-literal-field closure literal (see `pending_box_scalar_captures`)
    /// boxes one of its non-mutated captures instead of snapshotting it by
    /// value. `Stmt::Assign` to a plain identifier consults this map and
    /// additionally writes the new value through every box on file, so a
    /// struct-field closure invoked LATER sees a mutation made to the outer
    /// variable AFTER the closure was constructed -- gotcha #11's by-reference
    /// promise, otherwise only honored for the `closure_locals` direct-call
    /// fast path (see `n01_closure_field_vs_local`-class bugs). Scoped like
    /// `closure_locals`: saved/restored around a nested lambda's own frame.
    capture_boxes: HashMap<String, Vec<LocalId>>,
    /// Set by a struct-literal field whose value is directly a `Lambda`
    /// expression, immediately before lowering that expression, and consumed
    /// (reset to `false`) at the top of the Lambda arm. Tells that ONE lambda
    /// lowering to box its non-mutated scalar captures instead of the default
    /// value-snapshot capture (see `capture_boxes`). Never sees a nested
    /// lambda: the Lambda arm resets it before lowering its own body.
    pending_box_scalar_captures: bool,
    /// Set by `Stmt::Let` just before lowering a Lambda-valued initializer
    /// (`let name = |...| { ... }` -- also what a nested/local named
    /// `fn name(...) { ... }` desugars to in the parser), immediately before
    /// the RHS is lowered. Consumed (reset to `None`) at the top of the
    /// `Expr::Lambda` arm, AFTER `save_function_state()` has swapped in a
    /// fresh `closure_locals` map for the lambda's own body frame: registers
    /// `name -> (this lambda's mangled fn name, no captures)` in that fresh
    /// map so a self-recursive call to `name` inside the body resolves to a
    /// direct call of the lambda's own hoisted function, instead of falling
    /// through to a call to a top-level function literally named `name`
    /// (which does not exist -- the hoisted function is `__lambda_N` --
    /// and previously either linked to a bogus symbol or, worse, silently
    /// allocated a fresh uninitialized local named `name`). Scoped like
    /// `closure_locals`: discarded when `restore_function_state` runs, so it
    /// never leaks past this one lambda's own body.
    pending_self_recursive_name: Option<String>,
    /// The resolved annotation of the `let` whose initializer is currently
    /// being lowered. Consumed (take()) by `monomorphize_impl_fn` when a
    /// no-argument generic static constructor leaves type params unbound --
    /// `let ls: List<str> = List.new()` has nothing to bind `T` from except
    /// this annotation (matched against the template's return type), so it
    /// defaulted to `List___i64` while the chained `.push("a")` wanted
    /// `List___str`: JIT printed raw pointers, AOT failed the build.
    pending_let_expected: Option<MirType>,
}

/// Context passed to `throw` statements inside a `try` block.
#[derive(Clone, Copy)]
struct TryCatchTarget {
    result_local: LocalId,
    check_block: BlockId,
}

/// True when this instruction is a call that can carry a cross-function
/// `throw` (a user function, an indirect closure call, or a vtable call).
/// Runtime builtins (`kryos_*`) and the pure global builtins never throw.
/// Mirrors the post-call safety-net filter in both codegen backends.
fn is_unwind_source(inst: &Instruction) -> bool {
    let Instruction::Assign { value, .. } = inst else {
        return false;
    };
    match value {
        RValue::Call { func, .. } => {
            !func.starts_with("kryos_")
                && !matches!(
                    func.as_str(),
                    "println"
                        | "print"
                        | "eprintln"
                        | "sleep"
                        | "sleep_ms"
                        | "sqrt"
                        | "floor"
                        | "ceil"
                        | "round"
                        | "abs"
                        | "min"
                        | "max"
                        | "assert"
                        | "assert_eq"
                        | "panic"
                        | "len"
                        | "range"
                        | "to_string"
                        | "exit"
                        | "malloc"
                        | "calloc"
                        | "free"
                )
        }
        RValue::CallIndirect { .. } | RValue::VtableCall { .. } => true,
        _ => false,
    }
}

/// Stores a generic function's AST for deferred monomorphization.
struct GenericTemplate {
    generic_params: Vec<String>,
    params: Vec<ast::Param>,
    ret_ty: Option<ast::TypeExpr>,
    body: ast::Block,
}

/// Stores a generic impl associated (no-`self`) function's AST for deferred
/// monomorphization, analogous to `GenericTemplate`.
///
/// Also covers a generic INSTANCE method (`has_self == true`) whose return
/// type CONSTRUCTS a generic struct/enum (e.g. `Foo<T>.to_box() -> Box<T>`,
/// or a permuted `Pair<A,B>.swap() -> Pair<B,A>` when the receiver's own
/// concrete args must be recovered rather than assumed): such a method must
/// also be lowered ONCE PER concrete instantiation, bound from the
/// RECEIVER's concrete type at the call site (there is no self-less arg to
/// bind from). A bare `-> T` instance method (returning a field of self
/// as-is) is NOT registered here -- seeing `has_self` alone is not the
/// trigger; see `instance_ret_needs_monomorphization`. That shape stays on
/// the existing single-lowered-copy fast path (an 8-byte scalar slot
/// reinterpreted at the call site), which must not be disturbed.
struct GenericImplFnTemplate {
    /// Impl-level generic parameter names (e.g. `["T"]` for `impl<T> Box<T>`).
    generic_params: Vec<String>,
    /// Includes the `self` param (with its declared `Foo<T>`-shaped type)
    /// when `has_self` is true, so the same substitution loop in
    /// `monomorphize_impl_fn` specializes it like any other param.
    params: Vec<ast::Param>,
    ret_ty: Option<ast::TypeExpr>,
    body: ast::Block,
    /// True for a generic INSTANCE method (has `self`) whose return type
    /// constructs a generic struct/enum. False for an associated (no-`self`)
    /// function, matching the original (1458ff1) meaning of this template.
    has_self: bool,
}

/// Stores a generic struct's AST for deferred monomorphization.
#[derive(Clone)]
struct GenericStructTemplate {
    generic_params: Vec<String>,
    fields: Vec<ast::decl::StructField>,
}

/// Stores a generic enum's AST for deferred monomorphization.
#[derive(Clone)]
struct GenericEnumTemplate {
    generic_params: Vec<String>,
    variants: Vec<ast::decl::EnumVariant>,
}

impl LoweringContext {
    fn new() -> Self {
        Self {
            locals: Vec::new(),
            blocks: Vec::new(),
            current_instructions: Vec::new(),
            current_block: BlockId(0),
            next_local: 0,
            next_block: 0,
            loop_headers: Vec::new(),
            loop_exits: Vec::new(),
            struct_defs: HashMap::new(),
            enum_defs: HashMap::new(),
            func_ret_types: HashMap::new(),
            method_owners: HashMap::new(),
            trait_defs: HashMap::new(),
            trait_default_methods: HashMap::new(),
            impl_map: HashMap::new(),
            generic_templates: HashMap::new(),
            generic_impl_fn_templates: HashMap::new(),
            generic_struct_templates: HashMap::new(),
            mono_instance_args: HashMap::new(),
            impl_method_generic_info: HashMap::new(),
            generic_enum_templates: HashMap::new(),
            monomorphized: HashMap::new(),
            monomorphized_functions: Vec::new(),
            synthesized_eq_helpers: HashSet::new(),
            lambda_counter: 0,
            lambda_param_types: HashMap::new(),
            let_types: HashMap::new(),
            active_generic_bindings: HashMap::new(),
            spawn_counter: 0,
            type_aliases: HashMap::new(),
            try_catch_target: None,
            closure_locals: HashMap::new(),
            mutating_closures: HashSet::new(),
            pending_closure_regs: Vec::new(),
            actor_defs: HashMap::new(),
            actor_state_fields: HashMap::new(),
            current_actor: None,
            field_actor_types: HashMap::new(),
            local_actor_types: HashMap::new(),
            fn_return_actor_types: HashMap::new(),
            const_defs: HashMap::new(),
            mutable_globals: HashMap::new(),
            mutable_global_order: Vec::new(),
            func_param_types: HashMap::new(),
            dropped_locals: HashSet::new(),
            param_locals: HashSet::new(),
            cur_fn_name: String::new(),
            borrowed_locals: HashSet::new(),
            current_ret_ty: MirType::Void,
            current_self_type: None,
            current_impl_generics: Vec::new(),
            copy_structs: HashSet::new(),
            hidden_locals: HashSet::new(),
            partial_moved_locals: HashSet::new(),
            capture_boxes: HashMap::new(),
            pending_box_scalar_captures: false,
            pending_self_recursive_name: None,
            pending_let_expected: None,
        }
    }

    // ----- type resolution -----

    /// Resolve a type, checking type aliases and enum definitions.
    ///
    /// `lower_type_expr` maps all unknown type names to `Struct(name)`.
    /// This method post-processes the result: if the name matches a known
    /// enum definition, it produces `Enum(name)` instead; if it matches a
    /// type alias, it resolves to the aliased type.
    fn resolve_type(&mut self, ty: &ast::TypeExpr) -> MirType {
        // An impl's own type parameter (`impl<T> Box<T>`) erases to an i64
        // slot, matching the generic-struct payload model. This keeps a
        // generic impl method as one concrete-sized function rather than
        // referencing an unsized `%T` type on the LLVM backend.
        if let ast::TypeExpr::Simple { name, .. } = ty {
            if self.current_impl_generics.iter().any(|g| g == name) {
                return MirType::I64;
            }
            // An actor VALUE is an opaque i64 handle (the actor_id), not its
            // state struct: `let c = Counter()` binds the handle; state lives on
            // the actor's heap and is reached only via `self.field` inside
            // handlers (which lowers to ActorStateStore/Load, not a struct GEP).
            // Erasing to i64 keeps both backends' representation consistent
            // (the LLVM backend is strict: %Counter != i64).
            if self.actor_defs.contains_key(name) {
                return MirType::I64;
            }
        }

        // Handle generic struct/enum instantiation before calling lower_type_expr.
        // The args are resolved LAZILY, only inside the branch that actually
        // needs them (struct or enum template) -- NOT unconditionally up
        // front. `map<str, Option<V>>` reaches this arm with name = "map",
        // which is neither a struct nor enum template, so the eager
        // resolution used to be pure waste; its result was discarded and
        // `ty` was re-lowered from scratch via the `lower_type_expr`
        // fallback below. But resolving an arg is NOT side-effect-free: an
        // arg like `Option<V>`, where `V` is a generic function's OWN type
        // parameter that hasn't been substituted at this point (a function
        // template's BODY is lowered as-is; only its param/return types are
        // pre-substituted per instantiation), resolves "V" as an unresolved
        // struct reference and permanently monomorphizes a garbage
        // `Option___V` instance into `enum_defs` as a side effect --
        // reachable only via this discarded, unnecessary computation. The
        // LLVM backend then emits a drop helper for every registered enum,
        // including this phantom one, whose own payload field is ALSO the
        // bogus `V` placeholder, producing a call to an undefined
        // `__kryos_drop_V`. Deferring resolution until we know it is
        // actually needed removes the wasted (and harmful) computation
        // entirely.
        if let ast::TypeExpr::Generic { name, args, .. } = ty {
            // Check if this is a generic struct.
            if self.generic_struct_templates.contains_key(name) {
                let type_args: Vec<MirType> = args.iter().map(|a| self.resolve_type(a)).collect();
                return MirType::Struct(monomorphize_struct(self, name, &type_args));
            }

            // Check if this is a generic enum.
            if self.generic_enum_templates.contains_key(name) {
                let type_args: Vec<MirType> = args.iter().map(|a| self.resolve_type(a)).collect();
                return MirType::Enum(monomorphize_enum(self, name, &type_args));
            }

            // A map must recurse its key/value through the ctx-AWARE
            // resolve_type (like Array/Tuple/Function/Shared below) so a
            // nested generic value MONOMORPHIZES correctly. Falling to the
            // ctx-less lower_type_expr named a doubly-nested value
            // `map<str, Option<Result<i64,str>>>` as a stub that did not match
            // the fully-monomorphized value `Some(Ok(9))` constructs -- so
            // reading `m["a"]` back lost the inner Result's tag+payload (the
            // outer Option matched but the inner Ok(n) read 0). Single-level
            // values happened to match the stub name, which hid the bug.
            if (name == "map" || name == "Map") && args.len() == 2 {
                return MirType::Map {
                    key: Box::new(self.resolve_type(&args[0])),
                    value: Box::new(self.resolve_type(&args[1])),
                };
            }
        }

        // Compound types must recurse through resolve_type, not fall to the
        // ctx-less lower_type_expr: `[P<str>]` went through lower_type_expr
        // wholesale, whose Generic handling erases to the bare stub `P`
        // (all-i64 fields) — monomorphized fns then loaded f64 fields as
        // i64 slots (Cranelift verifier rejected the fadd).
        match ty {
            ast::TypeExpr::Array { element, size, .. } => {
                return MirType::Array(Box::new(self.resolve_type(element)), *size);
            }
            ast::TypeExpr::Tuple { elements, .. } => {
                return MirType::Tuple(elements.iter().map(|e| self.resolve_type(e)).collect());
            }
            ast::TypeExpr::Function { params, ret, .. } => {
                return MirType::Function {
                    params: params.iter().map(|p| self.resolve_type(p)).collect(),
                    ret: Box::new(self.resolve_type(ret)),
                };
            }
            ast::TypeExpr::Shared { inner, .. } => {
                return MirType::Shared(Box::new(self.resolve_type(inner)));
            }
            _ => {}
        }

        let mir_ty = lower_type_expr(ty);
        if let MirType::Struct(ref name) = mir_ty {
            // Resolve `Self` to the current impl/trait target type.
            if name == "Self" {
                if let Some(ref self_ty) = self.current_self_type {
                    return MirType::Struct(self_ty.clone());
                }
            }
            // Check enum definitions first — enum types must be distinguished
            // from struct types so that match lowering emits tag extraction.
            if self.enum_defs.contains_key(name.as_str()) {
                return MirType::Enum(name.clone());
            }
            if let Some(aliased) = self.type_aliases.get(name) {
                return aliased.clone();
            }
        }
        mir_ty
    }

    // ----- allocation helpers -----

    fn alloc_local(&mut self, name: Option<String>, ty: MirType, mutable: bool) -> LocalId {
        let id = LocalId(self.next_local);
        self.next_local += 1;
        self.locals.push(MirLocal {
            id,
            name,
            ty,
            mutable,
        });
        id
    }

    fn alloc_temp(&mut self, ty: MirType) -> LocalId {
        self.alloc_local(None, ty, false)
    }

    fn alloc_block(&mut self) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        id
    }

    // ----- block management -----

    fn emit(&mut self, inst: Instruction) {
        // Inside a try block, every call that can carry a cross-function
        // `throw` is followed IMMEDIATELY by a pending-exception check that
        // routes to the enclosing catch. This covers calls at any nesting
        // depth (if/for/while/match bodies) — the per-statement checks in
        // lower_try_catch only cover the try body's top level, which let
        // dead code run (and later throws overwrite earlier ones) until
        // control returned to the top level. The codegen backends rely on
        // this immediacy: their per-call unwind safety net is skipped only
        // when the very next MIR instruction is this check.
        let target = if self.try_catch_target.is_some() && is_unwind_source(&inst) {
            self.try_catch_target
        } else {
            None
        };
        self.current_instructions.push(inst);
        if let Some(t) = target {
            emit_exception_check(self, t.result_local, t.check_block);
        }
    }

    /// Finish the current block with the given terminator and start a new open
    /// block at `next`.
    fn finish_block(&mut self, terminator: Terminator, next: BlockId) {
        let instructions = std::mem::take(&mut self.current_instructions);
        self.blocks.push(BasicBlock {
            id: self.current_block,
            instructions,
            terminator,
        });
        self.current_block = next;
    }

    /// Finish the current block with the given terminator without starting a
    /// new block.
    fn seal_block(&mut self, terminator: Terminator) {
        let instructions = std::mem::take(&mut self.current_instructions);
        self.blocks.push(BasicBlock {
            id: self.current_block,
            instructions,
            terminator,
        });
    }

    // ----- reset for a new function -----

    fn reset(&mut self) {
        self.locals.clear();
        self.blocks.clear();
        self.current_instructions.clear();
        self.current_block = BlockId(0);
        self.next_local = 0;
        self.next_block = 1; // 0 is already the entry block
        self.loop_headers.clear();
        self.loop_exits.clear();
        self.dropped_locals.clear();
        self.param_locals.clear();
        self.hidden_locals.clear();
        self.partial_moved_locals.clear();
        self.try_catch_target = None;
        self.local_actor_types.clear();
    }

    /// Save the per-function state so we can restore it after monomorphization.
    /// Uses `mem::take` to move data out (zero-cost) instead of cloning.
    /// The caller must call `restore_function_state` to put the data back.
    fn save_function_state(&mut self) -> FunctionState {
        FunctionState {
            locals: std::mem::take(&mut self.locals),
            blocks: std::mem::take(&mut self.blocks),
            current_instructions: std::mem::take(&mut self.current_instructions),
            current_block: self.current_block,
            next_local: self.next_local,
            next_block: self.next_block,
            loop_headers: std::mem::take(&mut self.loop_headers),
            loop_exits: std::mem::take(&mut self.loop_exits),
            hidden_locals: std::mem::take(&mut self.hidden_locals),
            closure_locals: std::mem::take(&mut self.closure_locals),
            capture_boxes: std::mem::take(&mut self.capture_boxes),
            // A nested function body (lambda/spawn/monomorphized fn) must
            // NOT inherit the enclosing function's try context — its blocks
            // and locals live in a different function.
            try_catch_target: self.try_catch_target.take(),
            local_actor_types: std::mem::take(&mut self.local_actor_types),
            // `lower_function` calls `ctx.reset()`, which unconditionally
            // clears these three sets. Local ids restart at 0 for every
            // function, so WITHOUT saving/restoring them here, a nested
            // monomorphization (triggered mid-lowering of the caller, e.g. a
            // generic method call like `bi.set(999)`) wipes any
            // already-established bookkeeping for the CALLER's own
            // already-declared locals -- e.g. a local marked
            // `partial_moved_locals` because its value was shared (aliased,
            // no ref_count) into a containing struct literal
            // (`Box{value: inner}`) loses that marker, so the caller's own
            // later scope-end drop loop wrongly re-emits a Drop for it. On
            // Cranelift (raw struct pointers, no inherent copy-on-read) that
            // re-drop double-frees the SAME block the containing struct's
            // own recursive field-drop already freed -> exit 127 at teardown
            // (LLVM is immune: a struct-typed field read there is an
            // inherent value copy, so the two drops target independent
            // blocks). Root-caused via a runtime pointer trace: the
            // aliased block was freed twice, once via the containing
            // struct's recursive field-drop and once via the source local's
            // own (should-have-stayed-suppressed) drop.
            dropped_locals: std::mem::take(&mut self.dropped_locals),
            param_locals: std::mem::take(&mut self.param_locals),
            partial_moved_locals: std::mem::take(&mut self.partial_moved_locals),
        }
    }

    /// Restore per-function state after monomorphization.
    fn restore_function_state(&mut self, state: FunctionState) {
        self.locals = state.locals;
        self.blocks = state.blocks;
        self.current_instructions = state.current_instructions;
        self.current_block = state.current_block;
        self.next_local = state.next_local;
        self.next_block = state.next_block;
        self.loop_headers = state.loop_headers;
        self.loop_exits = state.loop_exits;
        self.hidden_locals = state.hidden_locals;
        self.closure_locals = state.closure_locals;
        self.capture_boxes = state.capture_boxes;
        self.try_catch_target = state.try_catch_target;
        self.local_actor_types = state.local_actor_types;
        self.dropped_locals = state.dropped_locals;
        self.param_locals = state.param_locals;
        self.partial_moved_locals = state.partial_moved_locals;
    }
}

/// Saved per-function lowering state (used during monomorphization).
struct FunctionState {
    locals: Vec<MirLocal>,
    blocks: Vec<BasicBlock>,
    current_instructions: Vec<Instruction>,
    current_block: BlockId,
    next_local: u32,
    next_block: u32,
    loop_headers: Vec<BlockId>,
    loop_exits: Vec<BlockId>,
    hidden_locals: HashSet<u32>,
    closure_locals: HashMap<String, (String, Vec<Operand>)>,
    capture_boxes: HashMap<String, Vec<LocalId>>,
    try_catch_target: Option<TryCatchTarget>,
    local_actor_types: HashMap<u32, String>,
    dropped_locals: HashSet<u32>,
    param_locals: HashSet<u32>,
    partial_moved_locals: HashSet<u32>,
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `@budget(tokens = N, calls = M)`: wrap the function body in a runtime
/// budget frame. Entry gets `depth = kryos_budget_push(N, M)`; every return
/// is preceded by `kryos_budget_pop_to(depth)`. `std::llm` charges the
/// active frames around each model call. Pops are by-depth, so an exception
/// unwinding past a pop self-heals at the next outer pop. Missing axes are
/// unlimited (-1).
fn inject_budget_frames(func: &mut MirFunction, annotations: &[ast::Annotation]) {
    let Some(ann) = annotations.iter().find(|a| a.name == "budget") else {
        return;
    };
    let mut tokens: i64 = -1;
    let mut calls: i64 = -1;
    // USD ceiling in micro-dollars (`usd=0.05` -> 50_000). -1 = unlimited.
    let mut usd_micros: i64 = -1;
    for arg in &ann.args {
        let cleaned: String = arg.chars().filter(|c| !c.is_whitespace()).collect();
        if let Some(v) = cleaned.strip_prefix("tokens=") {
            tokens = v.parse().unwrap_or(-1);
        } else if let Some(v) = cleaned.strip_prefix("calls=") {
            calls = v.parse().unwrap_or(-1);
        } else if let Some(v) = cleaned.strip_prefix("usd=") {
            usd_micros = v
                .parse::<f64>()
                .map(|d| (d * 1_000_000.0).round() as i64)
                .unwrap_or(-1);
        }
    }
    if func.blocks.is_empty() {
        return;
    }

    let next_id = func.locals.iter().map(|l| l.id.0 + 1).max().unwrap_or(0);
    let depth = LocalId(next_id);
    let scratch = LocalId(next_id + 1);
    func.locals.push(MirLocal {
        id: depth,
        name: Some("__budget_depth".into()),
        ty: MirType::I64,
        mutable: false,
    });
    func.locals.push(MirLocal {
        id: scratch,
        name: Some("__budget_scratch".into()),
        ty: MirType::I64,
        mutable: true,
    });

    func.blocks[0].instructions.insert(
        0,
        Instruction::Assign {
            dest: depth,
            value: RValue::Call {
                func: "kryos_budget_push_usd".into(),
                args: vec![
                    Operand::Constant(Constant::Int(tokens)),
                    Operand::Constant(Constant::Int(calls)),
                    Operand::Constant(Constant::Int(usd_micros)),
                ],
            },
        },
    );
    for block in func.blocks.iter_mut() {
        if matches!(block.terminator, Terminator::Return(_)) {
            block.instructions.push(Instruction::Assign {
                dest: scratch,
                value: RValue::Call {
                    func: "kryos_budget_pop_to".into(),
                    args: vec![Operand::Local(depth)],
                },
            });
        }
    }
}

/// Convert AST annotations to MIR attribute metadata.
fn annotations_to_mir_attributes(annotations: &[ast::Annotation]) -> MirAttributes {
    let mut attrs = MirAttributes::default();
    for ann in annotations {
        match ann.name.as_str() {
            "inline" => attrs.inline = true,
            "pure" => attrs.pure_fn = true,
            "test" => attrs.test = true,
            "bench" => attrs.bench = true,
            "deprecated" => attrs.deprecated = true,
            _ => {}
        }
    }
    attrs
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Lower an entire AST module to MIR.
pub fn lower_module(module: &ast::Module) -> MirModule {
    lower_module_with_lambda_params(module, &HashMap::new(), &HashMap::new())
}

/// Lower a module to MIR, using the type checker's resolved lambda param types
/// (keyed by lambda span) to type closure params that the AST left un-annotated,
/// and resolved empty-array `let` types (keyed by Let span) so untyped arrays
/// built via `push` get their real element type.
pub fn lower_module_with_lambda_params(
    module: &ast::Module,
    lambda_param_types: &HashMap<kryos_errors::Span, Vec<Option<ast::TypeExpr>>>,
    let_types: &HashMap<kryos_errors::Span, ast::TypeExpr>,
) -> MirModule {
    let mut ctx = LoweringContext::new();
    ctx.lambda_param_types = lambda_param_types.clone();
    ctx.let_types = let_types.clone();

    // Register built-in prelude enums (Option, Result) so they're available
    // to all programs without explicit import.
    ctx.enum_defs.insert(
        "Option".to_string(),
        vec![
            EnumVariantDef {
                name: "Some".to_string(),
                fields: vec![MirType::I64],
            },
            EnumVariantDef {
                name: "None".to_string(),
                fields: vec![],
            },
        ],
    );
    ctx.enum_defs.insert(
        "Result".to_string(),
        vec![
            EnumVariantDef {
                name: "Ok".to_string(),
                fields: vec![MirType::I64],
            },
            EnumVariantDef {
                name: "Err".to_string(),
                fields: vec![MirType::I64],
            },
        ],
    );

    // Also register Option/Result as generic templates so an explicit
    // `Option<str>` / `Result<i64, str>` monomorphizes its payload field types
    // (the bare-name stubs above erase payloads to i64, which is correct for
    // the runtime [tag, slot..] layout but mis-types a directly-used non-i64
    // payload binding -- e.g. `match r { Err(e) => println(e) }` printed the
    // str handle as an int). Bare `Result`/`Option` (no type args) still
    // resolve to the i64 stubs; only `TypeExpr::Generic` uses monomorphize.
    {
        let sp = kryos_errors::Span::DUMMY;
        let simple = |n: &str| ast::TypeExpr::Simple {
            name: n.to_string(),
            span: sp,
        };
        ctx.generic_enum_templates.insert(
            "Option".to_string(),
            GenericEnumTemplate {
                generic_params: vec!["T".to_string()],
                variants: vec![
                    ast::decl::EnumVariant {
                        name: "Some".to_string(),
                        fields: vec![simple("T")],
                        span: sp,
                    },
                    ast::decl::EnumVariant {
                        name: "None".to_string(),
                        fields: vec![],
                        span: sp,
                    },
                ],
            },
        );
        ctx.generic_enum_templates.insert(
            "Result".to_string(),
            GenericEnumTemplate {
                generic_params: vec!["T".to_string(), "E".to_string()],
                variants: vec![
                    ast::decl::EnumVariant {
                        name: "Ok".to_string(),
                        fields: vec![simple("T")],
                        span: sp,
                    },
                    ast::decl::EnumVariant {
                        name: "Err".to_string(),
                        fields: vec![simple("E")],
                        span: sp,
                    },
                ],
            },
        );
    }

    // Register built-in function return types so infer_expr_type can resolve
    // temps correctly (e.g. `to_string()` returns Str, not I64).
    for (name, ret_ty) in [
        ("to_string", MirType::Str),
        ("input", MirType::Str),
        ("readline", MirType::Str),
        ("substr", MirType::Str),
        ("trim", MirType::Str),
        ("to_upper", MirType::Str),
        ("to_lower", MirType::Str),
        ("replace", MirType::Str),
        ("split", MirType::Array(Box::new(MirType::Str), None)),
        ("join", MirType::Str),
        ("type_of", MirType::Str),
        ("format", MirType::Str),
        ("len", MirType::I64),
        ("abs", MirType::I64),
        ("abs_f", MirType::F64),
        ("min", MirType::I64),
        ("max", MirType::I64),
        ("min_f", MirType::F64),
        ("max_f", MirType::F64),
        ("sqrt", MirType::F64),
        ("floor", MirType::F64),
        ("ceil", MirType::F64),
        ("pow", MirType::F64),
        ("round", MirType::F64),
        ("sin", MirType::F64),
        ("cos", MirType::F64),
        ("tan", MirType::F64),
        ("log2", MirType::F64),
        ("log10", MirType::F64),
        ("parse_int", MirType::I64),
        ("parse_float", MirType::F64),
        // Overflow-aware integer arithmetic (operate on i64).
        ("wrapping_add", MirType::I64),
        ("wrapping_sub", MirType::I64),
        ("wrapping_mul", MirType::I64),
        ("checked_add", MirType::I64),
        ("checked_sub", MirType::I64),
        ("checked_mul", MirType::I64),
        ("saturating_add", MirType::I64),
        ("saturating_sub", MirType::I64),
        ("saturating_mul", MirType::I64),
        ("file_read", MirType::Str),
        ("file_write", MirType::I64),
        ("env_get", MirType::Str),
        ("time_now", MirType::I64),
        ("assert", MirType::Void),
        ("assert_eq", MirType::Void),
        ("panic", MirType::Void),
        ("chan", MirType::I64),
        ("recv", MirType::I64),
        // Non-blocking receive: chan_try_recv(ch) -> status (1 data / 0 empty /
        // -1 closed); on status 1, chan_last_recv() returns the value.
        ("chan_try_recv", MirType::I64),
        ("chan_last_recv", MirType::I64),
        ("println", MirType::Void),
        ("print", MirType::Void),
        ("eprintln", MirType::Void),
        ("push", MirType::I64), // returns array handle
        ("pop", MirType::I64),
        ("send", MirType::Void),
        ("sleep", MirType::Void),
        ("coop_yield", MirType::Void),
        ("coop_run", MirType::Void),
        ("coop_reset", MirType::Void),
        ("coop_record", MirType::Void),
        ("coop_order", MirType::Str),
        ("coop_spawn", MirType::I64),
        ("close_chan", MirType::Void),
        ("chan_is_closed", MirType::I64),
        ("contains", MirType::Bool),
        ("starts_with", MirType::Bool),
        ("ends_with", MirType::Bool),
        ("log", MirType::F64),
        ("char_code", MirType::I64),
        ("char_from", MirType::Str),
        ("int", MirType::I64),
        ("float", MirType::F64),
        // keys/map_keys return a real array (of key handles). Typing them as a
        // bare I64 handle made `for k in keys(m)` miss the array-iteration path
        // entirely -- the loop ran zero times, silently.
        ("keys", MirType::Array(Box::new(MirType::Str), None)),
        ("map_has", MirType::Bool),
        ("map_has_str", MirType::Bool),
        ("map_delete", MirType::I64),
        ("map_delete_str", MirType::I64),
        ("map_keys", MirType::Array(Box::new(MirType::Str), None)),
        ("map_keys_str", MirType::Array(Box::new(MirType::Str), None)),
        ("sleep_ms", MirType::Void),
        ("buf_new", MirType::I64),
        ("buf_write_byte", MirType::Void),
        ("buf_write_i16_le", MirType::Void),
        ("buf_write_i32_le", MirType::Void),
        ("buf_write_i64_le", MirType::Void),
        ("buf_write_bytes", MirType::Void),
        ("buf_write_str", MirType::Void),
        ("buf_write_zeros", MirType::Void),
        ("buf_len", MirType::I64),
        ("buf_str", MirType::Str),
        ("buf_get_byte", MirType::I64),
        ("buf_set_byte", MirType::Void),
        ("buf_patch_i32_le", MirType::Void),
        ("buf_patch_i64_le", MirType::Void),
        ("buf_write_to_file", MirType::I64),
        ("buf_free", MirType::Void),
        ("exit", MirType::Void),
        ("args", MirType::Array(Box::new(MirType::Str), None)),
        ("trim_start", MirType::Str),
        ("trim_end", MirType::Str),
        ("index_of", MirType::I64),
        ("sort", MirType::Void),
        ("reverse", MirType::Void),
        ("append_file", MirType::I64),
        ("read_file", MirType::Str),
        ("write_file", MirType::I64),
        ("http_get", MirType::Str),
        ("read_line", MirType::Str),
        // file_exists returns i64 (1/0), per the type checker and the language
        // reference -- NOT bool. Typing it Bool here made `to_string` of an
        // unannotated `let e = file_exists(..)` print "true"/"false" and
        // poisoned downstream i64 arithmetic typing.
        ("file_exists", MirType::I64),
        ("file_size", MirType::I64),
        ("create_dir", MirType::Void),
        ("is_null", MirType::Bool),
        // JSON builtins
        ("json_parse", MirType::I64),
        ("json_stringify", MirType::Str),
        ("json_object", MirType::I64),
        ("json_array", MirType::I64),
        ("json_string", MirType::I64),
        ("json_number", MirType::I64),
        ("json_bool", MirType::I64),
        ("json_null", MirType::I64),
        ("json_get", MirType::I64),
        ("json_get_index", MirType::I64),
        ("json_to_str", MirType::Str),
        ("json_to_int", MirType::I64),
        ("json_to_float", MirType::F64),
        ("json_is_null", MirType::Bool),
        ("json_length", MirType::I64),
        ("json_type", MirType::Str),
        // Crypto / hashing
        ("sha256", MirType::Str),
        ("hmac_sha256", MirType::Str),
        ("ed25519_generate", MirType::Str),
        ("ed25519_public", MirType::Str),
        ("ed25519_sign", MirType::Str),
        ("ed25519_verify", MirType::I64),
        ("pbkdf2_sha256", MirType::Str),
        ("hex_to_base64url", MirType::Str),
        ("base64url_to_hex", MirType::Str),
        ("sha512", MirType::Str),
        ("random_bytes", MirType::Str),
        ("sha1_hex", MirType::Str),
        ("sha1_base64", MirType::Str),
        ("base64_encode", MirType::Str),
        ("base64_decode", MirType::Str),
        ("chr", MirType::Str),
        ("byte_at", MirType::I64),
        // Regex
        ("regex_new", MirType::I64),
        ("regex_match", MirType::Bool),
        ("regex_find", MirType::Str),
        ("regex_find_pos", MirType::I64),
        ("regex_find_end", MirType::I64),
        ("regex_replace_all", MirType::Str),
        ("regex_drop", MirType::Void),
        // HTTP / HTTPS
        ("http_request", MirType::I64),
        ("https_get", MirType::Str),
        // HTTP/2 client (Gap C)
        ("http2_get", MirType::Str),
        ("http2_post", MirType::Str),
        ("http2_request", MirType::Str),
        // Web (WASM v0.4 browser host imports)
        ("dom_set_text", MirType::Void),
        ("dom_get_value", MirType::Str),
        ("alert", MirType::Void),
        ("canvas_fill_rect", MirType::Void),
        ("canvas_clear", MirType::Void),
        ("fetch_text", MirType::Str),
        // Time / Mutex
        ("time_now_secs", MirType::I64),
        ("time_now_millis", MirType::I64),
        ("time_millis", MirType::I64),
        ("mutex_new", MirType::I64),
        ("mutex_lock", MirType::Void),
        ("mutex_unlock", MirType::Void),
        ("mutex_drop", MirType::Void),
        // TCP
        ("tcp_connect", MirType::I64),
        ("tcp_listen", MirType::I64),
        ("tcp_accept", MirType::I64),
        ("tcp_send", MirType::I64),
        ("tcp_recv", MirType::Str),
        ("tcp_close", MirType::I64),
        // Async / non-blocking
        ("tcp_set_nonblocking", MirType::I64),
        ("tcp_try_accept", MirType::I64),
        ("tcp_try_recv", MirType::Str),
        ("sleep_ms", MirType::Void),
        // TLS server (Gap A)
        ("tls_server_config", MirType::I64),
        ("tls_accept", MirType::I64),
        ("tls_send", MirType::I64),
        ("tls_recv", MirType::Str),
        ("tls_close", MirType::I64),
        // PostgreSQL (Gap B)
        ("pg_connect", MirType::I64),
        ("pg_exec", MirType::I64),
        ("pg_query", MirType::Str),
        ("pg_close", MirType::I64),
        // Unix domain sockets (v2.0)
        ("uds_connect", MirType::I64),
        ("uds_bind", MirType::I64),
        ("uds_accept", MirType::I64),
        ("uds_send", MirType::I64),
        ("uds_recv", MirType::Str),
        ("uds_close", MirType::I64),
        // WebSocket (RFC 6455) (v2.0)
        ("ws_accept_key", MirType::Str),
        ("ws_encode_text", MirType::Str),
        ("ws_encode_binary", MirType::Str),
        ("ws_encode_close", MirType::Str),
        ("ws_encode_ping", MirType::Str),
        ("ws_encode_pong", MirType::Str),
        ("ws_unmask", MirType::Str),
        ("ws_read_frame", MirType::Str),
        // Low-level FFI helpers (v2.3.4)
        ("str_to_ptr", MirType::I64),
        ("arr_to_ptr", MirType::I64),
        ("buf_to_str", MirType::Str),
        ("alloc", MirType::I64),
        ("free_bytes", MirType::Void),
        ("ptr_byte_at", MirType::I64),
        ("ptr_set_byte", MirType::Void),
        ("ptr_read_i64", MirType::I64),
        ("ptr_write_i64", MirType::Void),
        ("handle_to_str", MirType::Str),
    ] {
        ctx.func_ret_types.insert(name.to_string(), ret_ty);
    }

    // Pre-pass: collect struct definitions and function return types so the
    // lowerer can infer correct types for field accesses and call results.
    for decl in &module.declarations {
        match decl {
            ast::Decl::Struct {
                name,
                generics,
                fields,
                annotations,
                ..
            } => {
                if !generics.is_empty() {
                    // Store template for deferred monomorphization.
                    let generic_param_names: Vec<String> =
                        generics.iter().map(|g| g.name.clone()).collect();
                    ctx.generic_struct_templates.insert(
                        name.clone(),
                        GenericStructTemplate {
                            generic_params: generic_param_names,
                            fields: fields.clone(),
                        },
                    );
                    // Also register a type-erased stub under the bare name so
                    // `Box { value: ... }` / `b.value` resolve in struct-literal
                    // and field-access paths. Struct runtime layout is a heap
                    // record of i64 slots; payload types erase to i64 for ABI.
                    let stub_fields: Vec<(String, MirType)> = fields
                        .iter()
                        .map(|f| (f.name.clone(), MirType::I64))
                        .collect();
                    ctx.struct_defs.insert(name.clone(), stub_fields);
                } else {
                    let field_list: Vec<(String, MirType)> = fields
                        .iter()
                        .map(|f| (f.name.clone(), ctx.resolve_type(&f.ty)))
                        .collect();
                    // Record fields whose declared type names an actor before
                    // that information is lost: `resolve_type` above erases an
                    // actor-named field type to a bare `I64` slot (actor
                    // VALUES are opaque handles), so a later method call on
                    // such a field (`some_box.c.bump(..)`) would otherwise be
                    // invisible to actor-dispatch detection. See
                    // `field_actor_types`.
                    for f in fields {
                        if let ast::TypeExpr::Simple { name: tn, .. } = &f.ty {
                            if ctx.actor_defs.contains_key(tn) {
                                ctx.field_actor_types
                                    .insert((name.clone(), f.name.clone()), tn.clone());
                            }
                        }
                    }
                    ctx.struct_defs.insert(name.clone(), field_list);
                }
                if annotations.iter().any(|a| a.name == "copy") {
                    ctx.copy_structs.insert(name.clone());
                }
            }
            ast::Decl::Enum {
                name,
                generics,
                variants,
                ..
            } => {
                if !generics.is_empty() {
                    let generic_param_names: Vec<String> =
                        generics.iter().map(|g| g.name.clone()).collect();
                    ctx.generic_enum_templates.insert(
                        name.clone(),
                        GenericEnumTemplate {
                            generic_params: generic_param_names,
                            variants: variants.clone(),
                        },
                    );
                    // Also register a type-erased stub under the bare name so
                    // `Maybe.Some(x)` / `Maybe.None` resolve in variant-construction
                    // paths. Enum runtime layout is [tag:i64, slot0:i64, ...] —
                    // payload types erased to i64 are ABI-compatible.
                    let stub_defs: Vec<EnumVariantDef> = variants
                        .iter()
                        .map(|v| EnumVariantDef {
                            name: v.name.clone(),
                            fields: v.fields.iter().map(|_| MirType::I64).collect(),
                        })
                        .collect();
                    ctx.enum_defs.insert(name.clone(), stub_defs);
                } else {
                    let variant_defs: Vec<EnumVariantDef> = variants
                        .iter()
                        .map(|v| EnumVariantDef {
                            name: v.name.clone(),
                            fields: v.fields.iter().map(lower_type_expr).collect(),
                        })
                        .collect();
                    ctx.enum_defs.insert(name.clone(), variant_defs);
                }
            }
            ast::Decl::Function {
                name,
                generics,
                params,
                ret_ty,
                body,
                ..
            } => {
                // Generic templates must not resolve their return type here:
                // `Boxed<T>` with no substitution map monomorphizes a bogus
                // `Boxed___T = { %T, .. }` whose emission is invalid IR. The
                // real return type registers per-instantiation when the
                // function monomorphizes.
                let mir_ret = if !generics.is_empty() {
                    MirType::I64
                } else {
                    match ret_ty {
                        Some(ty) => ctx.resolve_type(ty),
                        None => MirType::Void,
                    }
                };
                ctx.func_ret_types.insert(name.clone(), mir_ret);
                // If the return type names an actor, `resolve_type` just
                // erased it to a bare `I64` handle. Record the logical actor
                // name in `fn_return_actor_types` BEFORE that's lost, so a
                // method call chained directly on this function's result
                // (`get_counter(c).bump(..)`) is still recognized as an actor
                // receiver by `infer_type_name`'s `FnCall` fallback instead of
                // falling to a plain unmangled call. Skipped for generic
                // templates (see comment above): the real return registers
                // per-instantiation at monomorphization time.
                if generics.is_empty() {
                    if let Some(ast::TypeExpr::Simple { name: tn, .. }) = ret_ty {
                        if ctx.actor_defs.contains_key(tn) {
                            ctx.fn_return_actor_types.insert(name.clone(), tn.clone());
                        }
                    }
                }

                // Store parameter types for dyn Trait coercion. Generic
                // templates must not resolve here (same reason as the return
                // type above: `Boxed<T>` with no map registers a bogus
                // `Boxed___T` type whose emission is invalid IR).
                let param_types: Vec<MirType> = params
                    .iter()
                    .map(|p| {
                        if !generics.is_empty() {
                            MirType::I64
                        } else {
                            p.ty.as_ref()
                                .map(|t| ctx.resolve_type(t))
                                .unwrap_or(MirType::I64)
                        }
                    })
                    .collect();
                ctx.func_param_types.insert(name.clone(), param_types);

                // If this function has generic params, store it as a template
                // for monomorphization instead of lowering it immediately.
                if !generics.is_empty() {
                    if let Some(body) = body {
                        ctx.generic_templates.insert(
                            name.clone(),
                            GenericTemplate {
                                generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                                params: params.clone(),
                                ret_ty: ret_ty.clone(),
                                body: body.clone(),
                            },
                        );
                    }
                }
            }
            ast::Decl::Trait { name, methods, .. } => {
                let method_sigs: Vec<TraitMethodSig> = methods
                    .iter()
                    .filter_map(|m| {
                        if let ast::Decl::Function {
                            name,
                            params,
                            ret_ty,
                            ..
                        } = m
                        {
                            let param_types: Vec<MirType> = params
                                .iter()
                                .filter(|p| p.name != "self")
                                .map(|p| {
                                    p.ty.as_ref()
                                        .map(|t| ctx.resolve_type(t))
                                        .unwrap_or(MirType::I64)
                                })
                                .collect();
                            let ret = match ret_ty {
                                Some(ty) => ctx.resolve_type(ty),
                                None => MirType::Void,
                            };
                            Some(TraitMethodSig {
                                name: name.clone(),
                                param_types,
                                ret_ty: ret,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                ctx.trait_defs.insert(name.clone(), method_sigs);
                // Store default methods (those with bodies) for later use
                // when processing impl blocks that don't override them.
                let defaults: Vec<ast::Decl> = methods
                    .iter()
                    .filter(|m| matches!(m, ast::Decl::Function { body: Some(_), .. }))
                    .cloned()
                    .collect();
                if !defaults.is_empty() {
                    ctx.trait_default_methods.insert(name.clone(), defaults);
                }
            }
            ast::Decl::Impl {
                target,
                trait_name,
                methods,
                generics: impl_generics,
                ..
            } => {
                let prev_self = ctx.current_self_type.take();
                ctx.current_self_type = Some(target.clone());
                let prev_impl_generics = std::mem::replace(
                    &mut ctx.current_impl_generics,
                    impl_generics.iter().map(|g| g.name.clone()).collect(),
                );

                // Register mangled method names in func_ret_types.
                for method in methods {
                    if let ast::Decl::Function {
                        name,
                        ret_ty,
                        params,
                        body,
                        ..
                    } = method
                    {
                        let mangled = format!("{target}__{name}");
                        let has_self = params.iter().any(|p| p.name == "self");
                        let impl_generic_names: Vec<String> =
                            impl_generics.iter().map(|g| g.name.clone()).collect();
                        // A generic INSTANCE method (has `self`) whose return
                        // type CONSTRUCTS a generic struct differing from the
                        // erased single-lowered-copy shape (e.g. `Foo<T>.to_box()
                        // -> Box<T>`) needs the SAME per-call-site
                        // monomorphization as an associated fn -- see
                        // `instance_ret_needs_monomorphization`. A bare `-> T`
                        // instance method (the common case: Box::get,
                        // Nest::get, Pair::get_first/swap/second_of) is
                        // EXCLUDED and keeps the existing single-lowered-copy
                        // fast path below/in the second pass -- it returns a
                        // uniform 8-byte scalar slot that already reinterprets
                        // correctly at any call site (`impl_method_generic_info`
                        // + `ret_is_bare_param` in `infer_expr_type`), and
                        // routing it through monomorphization too would be
                        // unnecessary churn on the hottest generic-method path
                        // (the self-host compiler leans on it heavily).
                        // A T-typed VALUE param (non-self param mentioning an
                        // impl generic, e.g. `has(self, val: T)`) means the
                        // body OPERATES on T values (stringifies them for map
                        // keys, compares them, formats them) -- the erased
                        // single copy is semantically WRONG for pointer-backed
                        // T there: `Set<str>.has("y")` stringified the str's
                        // POINTER as the lookup key (always false), and
                        // `List<str>.index_of` compared pointers instead of
                        // content. Flow-through methods (T only in the return,
                        // e.g. `get -> T`, or no T at all, e.g. `len`) keep
                        // the erased fast path the self-host leans on.
                        let has_generic_value_param = has_self
                            && params.iter().any(|p| {
                                p.name != "self"
                                    && p.ty.as_ref().is_some_and(|t| {
                                        impl_generic_names
                                            .iter()
                                            .any(|gp| type_expr_mentions_param(t, gp))
                                    })
                            });
                        // A `has_self` method whose BODY operates on `self`
                        // beyond a bare passthrough return also needs per-
                        // receiver monomorphization: `self.<generic-field>`
                        // used in `+`/`to_string`/a cast (or bound to a local
                        // then used) is computed on the i64-erased slot by the
                        // single copy -- a str field then does pointer
                        // arithmetic (SIGSEGV) and an f64 field bit arithmetic
                        // (wrong value). The bare passthrough getters the self-
                        // host leans on (`get`/`len` = a single `return
                        // self.f`) are EXCLUDED and keep the erased fast path.
                        let body_operates_on_self = has_self
                            && body
                                .as_ref()
                                .is_some_and(|b| generic_instance_method_needs_body_monomorph(b));
                        let is_ctor_instance_method = has_self
                            && (instance_ret_needs_monomorphization(ret_ty, &impl_generic_names)
                                || has_generic_value_param
                                || body_operates_on_self);
                        // A no-`self` (associated) method inside a generic
                        // impl block must be registered as a monomorphization
                        // TEMPLATE, not lowered/resolved once here: resolving
                        // `ret_ty` now would erase the impl's own type params
                        // to i64 (see `resolve_type`'s current_impl_generics
                        // rule) and permanently fix e.g. `Box::new`'s return
                        // as `Box___i64` for every call site. Mirror the
                        // free-function template registration (`generics.is_empty()`
                        // check above) instead: use a placeholder return type
                        // here (never read -- the base mangled name is never
                        // lowered or called once templated, see below and the
                        // second Decl::Impl pass) and defer the real
                        // instantiation to call sites via
                        // `monomorphize_impl_fn`. Same deferral applies to the
                        // ctor-returning instance-method case (`is_ctor_instance_method`).
                        if !impl_generics.is_empty() && (!has_self || is_ctor_instance_method) {
                            if let Some(body) = body {
                                ctx.generic_impl_fn_templates.insert(
                                    (target.clone(), name.clone()),
                                    GenericImplFnTemplate {
                                        generic_params: impl_generic_names.clone(),
                                        params: params.clone(),
                                        ret_ty: ret_ty.clone(),
                                        body: body.clone(),
                                        has_self,
                                    },
                                );
                            }
                            ctx.func_ret_types.insert(mangled, MirType::I64);
                        } else {
                            let mir_ret = match ret_ty {
                                Some(ty) => ctx.resolve_type(ty),
                                None => MirType::Void,
                            };
                            // Sibling of the free-function registration above:
                            // recover an actor-named method return before
                            // `resolve_type` erased it, keyed by the mangled
                            // `Type__method` name (how a method call resolves
                            // its callee), so a chained call on an impl
                            // method's actor return is also recognized.
                            if let Some(ast::TypeExpr::Simple { name: tn, .. }) = ret_ty {
                                if ctx.actor_defs.contains_key(tn) {
                                    ctx.fn_return_actor_types.insert(mangled.clone(), tn.clone());
                                }
                            }
                            // Store parameter types for dyn Trait coercion at
                            // method-call sites, mirroring the free-function
                            // `func_param_types` registration above. `self`
                            // is excluded so the index lines up with a
                            // MethodCall's `args` (the receiver is passed
                            // separately, not through `args`). Without this,
                            // a struct arg passed to a `dyn Trait` PARAM of
                            // an instance method (`w.combine(r)`) reached the
                            // callee's `vtable_call` as a raw struct instead
                            // of a fat pointer -> SIGSEGV.
                            let param_types: Vec<MirType> = params
                                .iter()
                                .filter(|p| p.name != "self")
                                .map(|p| {
                                    p.ty.as_ref()
                                        .map(|t| ctx.resolve_type(t))
                                        .unwrap_or(MirType::I64)
                                })
                                .collect();
                            ctx.func_param_types.insert(mangled.clone(), param_types);
                            ctx.func_ret_types.insert(mangled, mir_ret);
                        }
                        // Record the self-param + return TypeExprs so an
                        // inference site can resolve a generic return to the
                        // receiver's concrete monomorphized arg. Only useful
                        // when the impl has generics.
                        if !impl_generics.is_empty() {
                            let self_te = params
                                .iter()
                                .find(|p| p.name == "self")
                                .and_then(|p| p.ty.clone());
                            ctx.impl_method_generic_info.insert(
                                (target.clone(), name.clone()),
                                (
                                    impl_generics.iter().map(|g| g.name.clone()).collect(),
                                    self_te,
                                    ret_ty.clone(),
                                ),
                            );
                        }
                    }
                }
                // Track which methods belong to which type for method call resolution.
                for method in methods {
                    if let ast::Decl::Function { name, .. } = method {
                        ctx.method_owners
                            .insert((target.clone(), name.clone()), format!("{target}__{name}"));
                    }
                }
                // Collect explicit method names for checking default overrides.
                let explicit_names: HashSet<String> = methods
                    .iter()
                    .filter_map(|m| {
                        if let ast::Decl::Function { name, .. } = m {
                            Some(name.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                // If implementing a trait, also register default methods that
                // are NOT overridden in this impl block.
                if let Some(trait_name) = trait_name {
                    if let Some(defaults) = ctx.trait_default_methods.get(trait_name).cloned() {
                        for default_method in &defaults {
                            if let ast::Decl::Function {
                                name,
                                ret_ty,
                                params,
                                ..
                            } = default_method
                            {
                                if !explicit_names.contains(name.as_str()) {
                                    let mangled = format!("{target}__{name}");
                                    let mir_ret = match ret_ty {
                                        Some(ty) => ctx.resolve_type(ty),
                                        None => MirType::Void,
                                    };
                                    ctx.func_ret_types.insert(mangled.clone(), mir_ret);
                                    // Param types (self excluded) so the
                                    // method-call arg coercion sees a `dyn
                                    // Trait` param of a NON-overridden default
                                    // method -- without this a concrete struct
                                    // passed to it reached vtable_call as a
                                    // raw struct (SIGSEGV both backends).
                                    // Mirrors the explicit-impl registration.
                                    let param_types: Vec<MirType> = params
                                        .iter()
                                        .filter(|p| p.name != "self")
                                        .map(|p| {
                                            p.ty.as_ref()
                                                .map(|t| ctx.resolve_type(t))
                                                .unwrap_or(MirType::I64)
                                        })
                                        .collect();
                                    ctx.func_param_types
                                        .insert(mangled.clone(), param_types);
                                    ctx.method_owners
                                        .insert((target.clone(), name.clone()), mangled);
                                }
                            }
                        }
                    }
                    let mut mangled_names: Vec<String> = methods
                        .iter()
                        .filter_map(|m| {
                            if let ast::Decl::Function { name, .. } = m {
                                Some(format!("{target}__{name}"))
                            } else {
                                None
                            }
                        })
                        .collect();
                    // Add default methods to the impl_map as well.
                    if let Some(defaults) = ctx.trait_default_methods.get(trait_name).cloned() {
                        for default_method in &defaults {
                            if let ast::Decl::Function { name, .. } = default_method {
                                if !explicit_names.contains(name.as_str()) {
                                    mangled_names.push(format!("{target}__{name}"));
                                }
                            }
                        }
                    }
                    ctx.impl_map
                        .insert((target.clone(), trait_name.clone()), mangled_names);
                }

                ctx.current_self_type = prev_self;
                ctx.current_impl_generics = prev_impl_generics;
            }
            ast::Decl::TypeAlias { name, ty, .. } => {
                let mir_ty = ctx.resolve_type(ty);
                ctx.type_aliases.insert(name.clone(), mir_ty);
            }
            ast::Decl::Extern { items, .. } => {
                // Register extern function signatures so they can be called.
                for item in items {
                    if let ast::Decl::Function { name, ret_ty, .. } = item {
                        let mir_ret = match ret_ty {
                            Some(ty) => ctx.resolve_type(ty),
                            None => MirType::Void,
                        };
                        ctx.func_ret_types.insert(name.clone(), mir_ret);
                    }
                }
            }
            ast::Decl::Actor {
                name,
                state_fields,
                handlers,
                ..
            } => {
                // Register actor state as a struct def.
                let fields: Vec<(String, MirType)> = state_fields
                    .iter()
                    .map(|f| (f.name.clone(), ctx.resolve_type(&f.ty)))
                    .collect();
                ctx.struct_defs.insert(name.clone(), fields);
                // Record state fields whose declared type names ANOTHER
                // actor (e.g. `target: Counter` inside `Relay`) before that
                // information is lost: `resolve_type` above erased it to a
                // bare `I64` slot (actor VALUES are opaque handles), so
                // `self.target.bump(..)` inside a `Relay` handler would
                // otherwise be invisible to actor-dispatch detection and
                // fall through to a plain unmangled call. See
                // `field_actor_types`.
                for f in state_fields {
                    if let ast::TypeExpr::Simple { name: tn, .. } = &f.ty {
                        if ctx.actor_defs.contains_key(tn) {
                            ctx.field_actor_types
                                .insert((name.clone(), f.name.clone()), tn.clone());
                        }
                    }
                }
                // Register handler signatures and actor_defs.
                let mut handler_info = Vec::new();
                for handler in handlers {
                    let mangled = format!("{name}__{}", handler.name);
                    let mir_ret = match &handler.ret_ty {
                        Some(ty) => ctx.resolve_type(ty),
                        None => MirType::Void,
                    };
                    ctx.func_ret_types.insert(mangled.clone(), mir_ret.clone());
                    ctx.method_owners
                        .insert((name.clone(), handler.name.clone()), mangled);
                    // The dispatch loop receives only the MESSAGE arguments, not
                    // `self` (the actor's own state is threaded via state_ptr, not
                    // the mailbox). Count non-self params.
                    let msg_arg_count =
                        handler.params.iter().filter(|p| p.name != "self").count();
                    handler_info.push((handler.name.clone(), msg_arg_count));
                }
                ctx.actor_defs.insert(name.clone(), handler_info);
                // Register actor state field layout for heap allocation and field access.
                let state_field_layout: Vec<(String, u32)> = state_fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| (f.name.clone(), i as u32))
                    .collect();
                ctx.actor_state_fields
                    .insert(name.clone(), state_field_layout);
            }
            ast::Decl::Const {
                name, ty, value, mutable, ..
            } => {
                let mir_ty = if let Some(t) = ty {
                    lower_type_expr(t)
                } else {
                    infer_expr_type(&mut ctx, value)
                };
                ctx.func_ret_types.insert(name.clone(), mir_ty.clone());
                if *mutable {
                    if !ctx.mutable_globals.contains_key(name) {
                        ctx.mutable_global_order.push(name.clone());
                    }
                    ctx.mutable_globals
                        .insert(name.clone(), (mir_ty, *value.clone()));
                } else {
                    ctx.const_defs
                        .insert(name.clone(), (mir_ty, *value.clone()));
                }
            }
            _ => {}
        }
    }

    let mut functions = Vec::new();

    for decl in &module.declarations {
        match decl {
            ast::Decl::Function {
                name,
                generics,
                params,
                ret_ty,
                body: Some(body),
                annotations,
                is_async,
                ..
            } => {
                // Skip generic functions — they are lowered on demand via monomorphization.
                if !generics.is_empty() {
                    continue;
                }
                let mut func = lower_function(&mut ctx, name, params, ret_ty, body);
                func.attributes = annotations_to_mir_attributes(annotations);
                func.attributes.is_async = *is_async;
                inject_budget_frames(&mut func, annotations);
                functions.push(func);
            }
            ast::Decl::Impl {
                target,
                trait_name,
                methods,
                generics: impl_generics,
                ..
            } => {
                let prev_self = ctx.current_self_type.take();
                ctx.current_self_type = Some(target.clone());
                let prev_impl_generics = std::mem::replace(
                    &mut ctx.current_impl_generics,
                    impl_generics.iter().map(|g| g.name.clone()).collect(),
                );

                // Lower each method as a free function with mangled name.
                let mut impl_method_names = Vec::new();
                for method in methods {
                    if let ast::Decl::Function {
                        name,
                        params,
                        ret_ty,
                        body: Some(body),
                        annotations,
                        is_async,
                        ..
                    } = method
                    {
                        let m_is_async = *is_async;
                        let mangled = format!("{target}__{name}");
                        impl_method_names.push(mangled.clone());
                        let has_self = params.iter().any(|p| p.name == "self");
                        // A no-`self` method inside a generic impl block, OR a
                        // `has_self` method whose return CONSTRUCTS a generic
                        // struct (see `instance_ret_needs_monomorphization`),
                        // is registered as a `generic_impl_fn_templates` entry
                        // (see the metadata pass above) and lowered ON DEMAND
                        // per concrete instantiation by `monomorphize_impl_fn`
                        // at its call sites -- mirroring how a generic free
                        // function is skipped here (`generics.is_empty()`
                        // check for `Decl::Function`) and lowered lazily by
                        // `monomorphize`. Lowering it here too would compile
                        // an orphaned, uncallable `{target}__{name}` body
                        // whose generic params are erased to i64 (dead code,
                        // and if it were ever called by name it would be the
                        // SAME single-instantiation bug this fix removes).
                        // Checking template presence (rather than
                        // re-deriving `is_ctor_instance_method` here) keeps
                        // this pass's skip decision mechanically in sync with
                        // whatever the metadata pass just registered -- a
                        // plain `has_self` bare `-> T` method is NOT in the
                        // map and correctly falls through to the unconditional
                        // single-lowered-copy path below (unchanged fast path).
                        if !impl_generics.is_empty()
                            && ctx
                                .generic_impl_fn_templates
                                .contains_key(&(target.clone(), name.clone()))
                        {
                            continue;
                        }
                        let mut all_params = Vec::new();
                        if has_self {
                            // Ensure the `self` param has the target type even if
                            // the user wrote just `self` without `: TypeName`.
                            for p in params {
                                if p.name == "self" && p.ty.is_none() {
                                    all_params.push(ast::Param {
                                        name: "self".into(),
                                        ty: Some(ast::TypeExpr::Simple {
                                            name: target.clone(),
                                            span: kryos_errors::Span::DUMMY,
                                        }),
                                        default: None,
                                        span: p.span,
                                    });
                                } else {
                                    all_params.push(p.clone());
                                }
                            }
                        } else {
                            // Static method — no self param.
                            all_params.extend_from_slice(params);
                        }
                        let mut func =
                            lower_function(&mut ctx, &mangled, &all_params, ret_ty, body);
                        func.attributes = annotations_to_mir_attributes(annotations);
                        func.attributes.is_async = m_is_async;
                        functions.push(func);
                    }
                }
                // If this is a trait impl, also lower default methods from the
                // trait that are not overridden in this impl block.
                if let Some(trait_name) = trait_name {
                    let explicit_names: HashSet<String> = methods
                        .iter()
                        .filter_map(|m| {
                            if let ast::Decl::Function { name, .. } = m {
                                Some(name.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    if let Some(defaults) = ctx.trait_default_methods.get(trait_name).cloned() {
                        for default_method in &defaults {
                            if let ast::Decl::Function {
                                name,
                                params,
                                ret_ty,
                                body: Some(body),
                                annotations,
                                is_async,
                                ..
                            } = default_method
                            {
                                if !explicit_names.contains(name.as_str()) {
                                    let mangled = format!("{target}__{name}");
                                    impl_method_names.push(mangled.clone());
                                    // Rewrite `self` param to concrete target type.
                                    let mut all_params = Vec::new();
                                    let has_self = params.iter().any(|p| p.name == "self");
                                    if has_self {
                                        for p in params {
                                            if p.name == "self" {
                                                all_params.push(ast::Param {
                                                    name: "self".into(),
                                                    ty: Some(ast::TypeExpr::Simple {
                                                        name: target.clone(),
                                                        span: kryos_errors::Span::DUMMY,
                                                    }),
                                                    default: None,
                                                    span: p.span,
                                                });
                                            } else {
                                                all_params.push(p.clone());
                                            }
                                        }
                                    } else {
                                        all_params.push(ast::Param {
                                            name: "self".into(),
                                            ty: Some(ast::TypeExpr::Simple {
                                                name: target.clone(),
                                                span: kryos_errors::Span::DUMMY,
                                            }),
                                            default: None,
                                            span: kryos_errors::Span::DUMMY,
                                        });
                                        all_params.extend_from_slice(params);
                                    }
                                    let mut func = lower_function(
                                        &mut ctx,
                                        &mangled,
                                        &all_params,
                                        ret_ty,
                                        body,
                                    );
                                    func.attributes = annotations_to_mir_attributes(annotations);
                                    func.attributes.is_async = *is_async;
                                    functions.push(func);
                                }
                            }
                        }
                    }
                    ctx.impl_map
                        .insert((target.clone(), trait_name.clone()), impl_method_names);
                }

                ctx.current_self_type = prev_self;
                ctx.current_impl_generics = prev_impl_generics;
            }
            ast::Decl::Actor { name, handlers, .. } => {
                // Lower each message handler as a free function: ActorName__handler_name.
                for handler in handlers {
                    let mangled = format!("{name}__{}", handler.name);
                    // The handler lowers to `ActorName__handler(self, msg_args...)`.
                    // Use an actor-typed `self` (for `self.field` access) followed
                    // by the message args. The parser already puts `self` in
                    // handler.params, so drop it and re-add a properly-typed one
                    // (avoids a double-`self` / wrong arity).
                    let mut all_params = vec![ast::Param {
                        name: "self".into(),
                        ty: Some(ast::TypeExpr::Simple {
                            name: name.clone(),
                            span: kryos_errors::Span::DUMMY,
                        }),
                        default: None,
                        span: kryos_errors::Span::DUMMY,
                    }];
                    all_params.extend(
                        handler.params.iter().filter(|p| p.name != "self").cloned(),
                    );
                    ctx.current_actor = Some(name.clone());
                    functions.push(lower_function(
                        &mut ctx,
                        &mangled,
                        &all_params,
                        &handler.ret_ty,
                        &handler.body,
                    ));
                    ctx.current_actor = None;
                }
            }
            _ => {}
        }
    }

    // Generate dispatch functions for each actor.
    for (actor_name, handlers) in &ctx.actor_defs {
        functions.push(generate_actor_dispatch(actor_name, handlers));
    }

    // Collect monomorphized specializations generated during lowering.
    functions.append(&mut ctx.monomorphized_functions);

    // Erase actor VALUE types to i64 across every function. An actor binding is
    // an opaque handle (actor_id); inferred `let c = Counter()` bindings pick up
    // the checker's Struct(Counter) type, which the strict LLVM backend then
    // conflicts with the i64 the spawn produces. resolve_type erases annotated
    // occurrences; this catches the inferred ones too.
    if !ctx.actor_defs.is_empty() {
        let is_actor = |t: &MirType| matches!(t, MirType::Struct(n) if ctx.actor_defs.contains_key(n));
        for f in &mut functions {
            for p in &mut f.params {
                if is_actor(&p.ty) {
                    p.ty = MirType::I64;
                }
            }
            for l in &mut f.locals {
                if is_actor(&l.ty) {
                    l.ty = MirType::I64;
                }
            }
        }
    }

    MirModule {
        functions,
        struct_defs: ctx.struct_defs,
        enum_defs: ctx.enum_defs,
        trait_vtables: ctx.impl_map,
        copy_structs: ctx.copy_structs,
    }
}

/// Lower a single AST function declaration to a `MirFunction`.

// ---------------------------------------------------------------------------
// Debug drop-site tagging (KRYOS_MIR_DROP_TAGS=<out.tsv> at compile time).
// Every inserted Drop / reassignment-release is preceded by a
// `kryos_diag_site(id)` call; the id -> (function, purpose) table is written
// to the given TSV so a runtime double-free report names its emitting site.
// Zero cost unless the env var is set when the compiler runs.
// ---------------------------------------------------------------------------
static DROP_TAGS: std::sync::Mutex<Option<(u64, Vec<String>)>> = std::sync::Mutex::new(None);

fn drop_tags_path() -> Option<String> {
    std::env::var("KRYOS_MIR_DROP_TAGS").ok().filter(|v| !v.is_empty())
}

fn drop_tag(ctx: &mut LoweringContext, why: &str) {
    if drop_tags_path().is_none() {
        return;
    }
    let id = {
        let mut g = DROP_TAGS.lock().unwrap();
        let entry = g.get_or_insert_with(|| (0, Vec::new()));
        let id = entry.0;
        entry.0 += 1;
        entry.1.push(format!("{id}	{}	{why}", ctx.cur_fn_name));
        // Debug knob: rewrite the table on every append so it is complete
        // regardless of which pipeline path finishes the compile.
        if let Some(path) = drop_tags_path() {
            let _ = std::fs::write(path, entry.1.join("
"));
        }
        id
    };
    let sink = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: sink,
        value: RValue::Call {
            func: "kryos_diag_site".to_string(),
            args: vec![Operand::Constant(Constant::Int(id as i64))],
        },
    });
}

/// Flush the drop-site table (no-op unless tagging is active).
pub fn flush_drop_tags() {
    if let Some(path) = drop_tags_path() {
        if let Ok(g) = DROP_TAGS.lock() {
            if let Some((_, rows)) = g.as_ref() {
                let _ = std::fs::write(path, rows.join("
"));
            }
        }
    }
}

/// Borrow-to-own retain for a container value flowing OUT of a borrowed
/// source (function param) into an owned position (a local binding or a
/// return value). Params are caller-owned and never dropped by the callee,
/// so without this the receiving slot's later release/drop frees the
/// CALLER's value (shape-4 of the bootstrap heap-corruption class). The
/// retain gives the new owner its own reference; null passes through.
/// Element-ownership kind for `kryos_array_dup` (see the runtime doc):
/// 0 = scalar, 1 = refcounted container (Str/Array/Map), 2 = arc box.
fn array_elem_dup_kind(elem: &MirType) -> i64 {
    match elem {
        MirType::Str | MirType::Array(_, _) | MirType::Map { .. } => 1,
        MirType::Struct(_) | MirType::Enum(_) | MirType::Shared(_) => 2,
        _ => 0,
    }
}

/// True when the struct's whole field graph is shapes the deep-clone helpers
/// (`__kryos_struct_index_clone` in both backends) handle COMPLETELY:
/// scalars, str/array/map, and nested structs that are themselves cleanly
/// clonable. Enum/tuple/fn/shared fields disqualify -- the helpers leave an
/// enum field's heap payload SHARED, so a "clone" there creates two owners of
/// one payload and the second drop double-releases it. Cyclic type graphs
/// disqualify conservatively.
fn struct_cleanly_clonable(ctx: &LoweringContext, name: &str) -> bool {
    fn walk(
        ctx: &LoweringContext,
        name: &str,
        visiting: &mut std::collections::HashSet<String>,
    ) -> bool {
        if !visiting.insert(name.to_string()) {
            return false; // cycle: be conservative
        }
        let Some(fields) = ctx.struct_defs.get(name) else {
            return false;
        };
        for (_fname, fty) in fields {
            match fty {
                MirType::I8
                | MirType::I16
                | MirType::I32
                | MirType::I64
                | MirType::U8
                | MirType::U16
                | MirType::U32
                | MirType::U64
                | MirType::F32
                | MirType::F64
                | MirType::Bool
                | MirType::Char
                | MirType::Str
                | MirType::Array(_, _)
                | MirType::Map { .. } => {}
                MirType::Struct(inner) => {
                    if !walk(ctx, inner, visiting) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }
    let mut visiting = std::collections::HashSet::new();
    walk(ctx, name, &mut visiting)
}

fn retain_for_ty(ty: &MirType) -> Option<&'static str> {
    match ty {
        MirType::Str => Some("kryos_string_retain_opt"),
        MirType::Array(_, _) => Some("kryos_array_retain_opt"),
        MirType::Map { .. } => Some("kryos_map_retain_opt"),
        _ => None,
    }
}


fn emit_param_source_retain(ctx: &mut LoweringContext, holder: LocalId, src: LocalId) {
    if !ctx.param_locals.contains(&src.0) {
        return;
    }
    let ty = ctx
        .locals
        .iter()
        .find(|l| l.id == src)
        .map(|l| l.ty.clone())
        .unwrap_or(MirType::I64);
    if let Some(rf) = retain_for_ty(&ty) {
        let sink = ctx.alloc_temp(MirType::I64);
        ctx.emit(Instruction::Assign {
            dest: sink,
            value: RValue::Call {
                func: rf.to_string(),
                args: vec![Operand::Local(holder)],
            },
        });
    }
}

/// Give a struct-typed local its OWN independent reference to each of its
/// directly-declared str/array/map fields. Used when a struct VALUE is
/// extracted (bit-copy, no retain) from a container that still owns its
/// own drop -- e.g. an enum match-arm payload bind (`Ok(next) => ..`)
/// pulled out of a named `let result = f(..)` subject. Without this,
/// `next`'s heap-owned fields alias the exact same buffers `result`'s own
/// (un-suppressed) scope-end Drop frees, a use-after-free the next use of
/// `next` (or whatever it's reassigned into) corrupts or crashes on.
///
/// A `str`/`map` field is handled with a plain container retain
/// (`kryos_string_retain_opt`/`kryos_map_retain_opt`): both `next`'s copy
/// and `result`'s copy then share one buffer with a correctly-balanced
/// refcount, each releasing it exactly once at their own scope end.
///
/// An `array` field needs MORE than a container retain: `result`'s
/// recursive struct-drop (dropping a STRUCT field, unlike dropping a bare
/// array-typed variable) walks the array's OWN ELEMENTS and releases each
/// one -- confirmed via KRYOS_STR_TRACE/KRYOS_ARR_TRACE: an element created
/// two iterations ago accumulates one release per SUBSEQUENT iteration's
/// `result`-drop (each iteration re-clones the whole history array,
/// including old elements, into a fresh `result`), while it was only ever
/// retained once (at push time) -- net negative, so an early element (e.g.
/// history[0]) reaches refcount 0 and is freed while still live in
/// `next`/`current`, well before a LATER element (history[len-1], touched
/// by only one iteration's drop so far) shows the same symptom. A plain
/// container retain does not compensate for this because it only protects
/// the ARRAY HEADER, not each element inside it. `kryos_array_dup` (used
/// elsewhere for the identical `let mut b = a` aliasing hazard) allocates
/// an independent header+buffer AND bumps every non-null element's own
/// refcount once, fully decoupling `next`'s copy from `result`'s -- so
/// `result`'s per-iteration recursive drop only ever touches `result`'s OWN
/// (now-independent) copy and its elements, never `next`'s.
///
/// One level deep only (does not recurse into a nested struct/enum field);
/// that matches this fix's scope (a struct with scalar/str/array/map
/// fields) and keeps the blast radius small.
///
/// Implementation note: this REBUILDS the whole struct value via a fresh
/// `RValue::Struct` and reassigns `holder` in one shot, rather than
/// reading one field and writing it back in place with `StoreField`. The
/// StoreField version worked on AOT (LLVM materializes struct locals in
/// memory either way) but NOT on the Cranelift JIT: a partial field
/// mutation into an already-bound match-arm local did not reliably
/// propagate to the arm body's later use of that local (the freshly
/// duplicated array was allocated, then freed again immediately --
/// matching the ORIGINAL bug's signature, meaning the arm body still saw
/// the un-duplicated, subject-owned field). A full reassignment
/// (`Instruction::Assign` with a complete `RValue::Struct`) is the same
/// shape codegen already handles correctly for every other struct
/// construction site, so it does not depend on cross-block field-mutation
/// support this backend doesn't have.
fn retain_struct_heap_fields(ctx: &mut LoweringContext, holder: LocalId, struct_name: String) {
    let fields = ctx
        .struct_defs
        .get(struct_name.as_str())
        .cloned()
        .unwrap_or_default();
    if fields.is_empty() {
        return;
    }
    let mut new_fields: Vec<(String, Operand)> = Vec::with_capacity(fields.len());
    for (field_name, field_ty) in &fields {
        let field_val = ctx.alloc_temp(field_ty.clone());
        ctx.emit(Instruction::Assign {
            dest: field_val,
            value: RValue::Field {
                object: Operand::Local(holder),
                field: field_name.clone(),
            },
        });
        match field_ty {
            MirType::Array(elem_ty, _) => {
                // Independent header+buffer AND a per-element retain (see
                // doc comment above) -- decouples this copy's array from
                // the subject's, element-for-element.
                let kind = array_elem_dup_kind(elem_ty);
                let dup_tmp = ctx.alloc_temp(field_ty.clone());
                ctx.emit(Instruction::Assign {
                    dest: dup_tmp,
                    value: RValue::Call {
                        func: "kryos_array_dup".to_string(),
                        args: vec![Operand::Local(field_val), Operand::Constant(Constant::Int(kind))],
                    },
                });
                new_fields.push((field_name.clone(), Operand::Local(dup_tmp)));
            }
            // A nested Enum field (e.g. `PResult { expr: Expr, pos: i64 }`
            // wrapping a recursive `Expr`): `field_val` above is a bit-copy
            // alias of the subject struct's own field slot, same hazard as
            // an Array field one arm up. The subject (e.g. `let first =
            // parse_primary(..)`, or `let r0` bound one match-arm out) still
            // owns its own unconditional scope-end Drop, which recursively
            // frees this SAME field object when the struct drops. Without an
            // independent copy here, `holder`'s rebuilt struct and the
            // subject alias the identical Expr; the subject's drop frees it
            // out from under `holder`, and a later fold
            // (`node = Expr.Mul(node, r1.expr)`) either corrupts on the
            // SECOND reassignment or double-frees at teardown (matches the
            // observed exit 127/139 non-determinism: heap corruption, not a
            // deterministic panic). Fix: same `__kryos_enum_index_clone`
            // deep-copy marker used for the top-level enum-payload-bind case
            // and the push-aliasing shape.
            MirType::Enum(enum_name) if ctx.enum_defs.contains_key(enum_name.as_str()) => {
                let cloned = ctx.alloc_temp(field_ty.clone());
                ctx.emit(Instruction::Assign {
                    dest: cloned,
                    value: RValue::Call {
                        func: "__kryos_enum_index_clone".to_string(),
                        args: vec![Operand::Local(field_val)],
                    },
                });
                new_fields.push((field_name.clone(), Operand::Local(cloned)));
            }
            _ => {
                if let Some(rf) = retain_for_ty(field_ty) {
                    let sink = ctx.alloc_temp(MirType::I64);
                    ctx.emit(Instruction::Assign {
                        dest: sink,
                        value: RValue::Call {
                            func: rf.to_string(),
                            args: vec![Operand::Local(field_val)],
                        },
                    });
                }
                // Scalars (and str/map, retained above) pass through as-is:
                // the field VALUE itself (an i64/f64/bool bit pattern, or a
                // now independently-retained str/map handle) is reused
                // directly in the rebuilt struct.
                new_fields.push((field_name.clone(), Operand::Local(field_val)));
            }
        }
    }
    ctx.emit(Instruction::Assign {
        dest: holder,
        value: RValue::Struct {
            name: struct_name,
            fields: new_fields,
        },
    });
}

/// The pointer-inequality-guarded release function for a directly-owned
/// container type. Mirrors `retain_for_ty` for the release side.
fn release_if_ne_fn(ty: &MirType) -> Option<&'static str> {
    match ty {
        MirType::Str => Some("kryos_string_release_if_ne"),
        MirType::Array(..) => Some("kryos_array_release_if_ne"),
        MirType::Map { .. } => Some("kryos_map_release_if_ne"),
        _ => None,
    }
}

/// Release the OLD value's heap-owned fields (one level deep) when a
/// STRUCT-typed local is overwritten by plain reassignment (`current =
/// next`), guarded per-field by pointer inequality so an in-place mutator
/// or a field the new value happens to still share is never freed out
/// from under it.
///
/// Plain reassignment of a struct-typed local has no equivalent of the
/// existing Str/Array/Map "reassignment release" (see `release_if_ne_fn`
/// above and its call site) -- `release_fn` there is `None` for
/// `MirType::Struct`, so overwriting a struct-typed local NEVER frees its
/// previous heap-owned fields. That's an accepted pre-existing leak for a
/// program that reassigns a struct only a handful of times. It stops being
/// negligible once a fix gives each reassignment its OWN fresh
/// independent array (see `retain_struct_heap_fields`'s `kryos_array_dup`)
/// inside a hot loop: each iteration's now-orphaned independent copy (the
/// OLD `current`'s fields, superseded by `next`'s) was never reclaimed,
/// compounding into a multi-GB leak on a long-running loop (measured:
/// several GB/s of RSS growth on a 40M-iteration soak before this was
/// added). This closes that gap for the struct case specifically, using
/// the SAME guarded release primitives the Str/Array/Map case already
/// uses, so an in-place mutator or a field shared between old and new
/// (pointer-equal) is still left alone.
fn release_struct_heap_fields_if_ne(
    ctx: &mut LoweringContext,
    old_holder: LocalId,
    new_holder: LocalId,
    struct_name: &str,
) {
    let fields = ctx
        .struct_defs
        .get(struct_name)
        .cloned()
        .unwrap_or_default();
    for (field_name, field_ty) in fields {
        let Some(rf) = release_if_ne_fn(&field_ty) else {
            continue;
        };
        let old_field = ctx.alloc_temp(field_ty.clone());
        ctx.emit(Instruction::Assign {
            dest: old_field,
            value: RValue::Field {
                object: Operand::Local(old_holder),
                field: field_name.clone(),
            },
        });
        let new_field = ctx.alloc_temp(field_ty.clone());
        ctx.emit(Instruction::Assign {
            dest: new_field,
            value: RValue::Field {
                object: Operand::Local(new_holder),
                field: field_name,
            },
        });
        let sink = ctx.alloc_temp(MirType::I64);
        ctx.emit(Instruction::Assign {
            dest: sink,
            value: RValue::Call {
                func: rf.to_string(),
                args: vec![Operand::Local(old_field), Operand::Local(new_field)],
            },
        });
    }
}

pub fn lower_function(
    ctx: &mut LoweringContext,
    name: &str,
    params: &[ast::Param],
    ret_ty: &Option<ast::TypeExpr>,
    body: &ast::Block,
) -> MirFunction {
    ctx.reset();
    ctx.cur_fn_name = name.to_string();

    // Allocate entry block (id = 0).
    let _entry = ctx.alloc_block();

    // Lower return type — use resolve_type to correctly handle enum return types.
    let mir_ret_ty = match ret_ty {
        Some(ty) => ctx.resolve_type(ty),
        None => MirType::Void,
    };
    ctx.current_ret_ty = mir_ret_ty.clone();

    // Lower parameters -> locals.
    // Use resolve_type instead of lower_type_expr so that enum type names
    // (e.g. `Color`, `Day`) are correctly mapped to MirType::Enum rather
    // than MirType::Struct.  This is critical for match lowering to emit
    // tag extraction (EnumTag) on enum-typed parameters.
    let mir_params: Vec<MirParam> = params
        .iter()
        .map(|p| {
            let ty =
                p.ty.as_ref()
                    .map(|t| ctx.resolve_type(t))
                    .unwrap_or(MirType::I64);
            let local = ctx.alloc_local(Some(p.name.clone()), ty.clone(), false);
            // Mark as parameter — callee must NOT drop/free these; the caller owns them.
            ctx.param_locals.insert(local.0);
            // If the param's declared type names an actor, `resolve_type` just
            // erased `ty` to a bare `I64` handle (actor VALUES are opaque
            // handles). Record the logical actor name in `local_actor_types`
            // BEFORE that information is lost, so a method call on this param
            // inside the function body (e.g. a `spawn { }` wrapper's captured
            // actor variable) still resolves through `infer_type_name` to the
            // ActorSend/mailbox dispatch instead of a bare unmangled call.
            if let Some(ast::TypeExpr::Simple { name: tn, .. }) = p.ty.as_ref() {
                if ctx.actor_defs.contains_key(tn) {
                    ctx.local_actor_types.insert(local.0, tn.clone());
                }
            }
            MirParam { local, ty }
        })
        .collect();

    // Consume staged closure-local re-registrations from the enclosing
    // frame. The outer Lambda case populated `pending_closure_regs` with
    // (closure_name, real_func, capture_var_names). Now that this frame's
    // params are allocated, re-key those entries onto inner-frame local IDs.
    if !ctx.pending_closure_regs.is_empty() {
        let regs = std::mem::take(&mut ctx.pending_closure_regs);
        for (clos_name, real_func, cap_names) in regs {
            let cap_ops: Option<Vec<Operand>> = cap_names
                .iter()
                .map(|n| {
                    ctx.locals
                        .iter()
                        .find(|l| l.name.as_deref() == Some(n.as_str()))
                        .map(|l| Operand::Local(l.id))
                })
                .collect();
            if let Some(cap_ops) = cap_ops {
                ctx.closure_locals
                    .insert(clos_name, (real_func, cap_ops));
            }
        }
    }

    // Module-level globals initializer. For the program entry point (`main`)
    // we evaluate every `let mut NAME: TY = EXPR` top-level decl exactly once
    // before any user code runs and store the resulting i64 slot into the
    // process-wide registry via `kryos_global_set`. All subsequent reads of
    // those names lower to `kryos_global_get`.
    if name == "main" && !ctx.mutable_global_order.is_empty() {
        // Clone the order/value pairs out of ctx so we don't keep an immutable
        // borrow alive across the mutable lower_expr_to_operand calls below.
        let init_pairs: Vec<(String, ast::Expr)> = ctx
            .mutable_global_order
            .iter()
            .filter_map(|n| ctx.mutable_globals.get(n).map(|(_, e)| (n.clone(), e.clone())))
            .collect();
        for (gname, init_expr) in init_pairs {
            let val = lower_expr_to_operand(ctx, &init_expr);
            emit_global_store(ctx, &gname, val);
        }
    }

    // Lower the body statements.  If the function has a non-void return type
    // and the last statement is a bare expression OR a value-producing
    // control-flow construct (if / match), treat it as a tail expression
    // (implicit return) so that e.g. a trailing `if c { 0 } else { 1 }` or
    // `match x { ... }` becomes the function's return value rather than
    // silently lowering to `return ()` from a non-void signature.
    let last_stmt = body.stmts.last();
    let has_tail_expr = mir_ret_ty != MirType::Void
        && last_stmt.is_some_and(|s| matches!(s, ast::Stmt::Expr { .. }));
    // `match` at statement position is parsed as Stmt::Expr { MatchExpr }, so
    // it is already covered by has_tail_expr. Only `if` needs special handling
    // here because the parser commits to Stmt::If at statement position.
    let has_tail_ctrl = mir_ret_ty != MirType::Void
        && last_stmt
            .is_some_and(|s| matches!(s, ast::Stmt::If { .. } | ast::Stmt::TryCatch { .. }));

    if has_tail_ctrl && !body.stmts.is_empty() {
        let (init, last) = body.stmts.split_at(body.stmts.len() - 1);
        let scope_start = ctx.locals.len();
        for stmt in init {
            lower_stmt(ctx, stmt);
        }
        // Use the existing block-as-value lowering path: allocate a result
        // local sized for the declared return type and let the if/match
        // arms write their tail values into it via `lower_block_as_value`.
        let result_local = ctx.alloc_temp(mir_ret_ty.clone());
        lower_block_as_value(ctx, std::slice::from_ref(&last[0]), result_local);
        // Emit drops for in-scope locals before returning, matching the
        // tail-expression path's behavior.
        let scope_end = ctx.locals.len();
        for i in (scope_start..scope_end).rev() {
            if ctx.locals[i].name.is_some() {
                let local_id = ctx.locals[i].id;
                if ctx.param_locals.contains(&local_id.0)
                    || ctx.borrowed_locals.contains(&local_id.0)
                {
                    continue;
                }
                if !ctx.dropped_locals.contains(&local_id.0)
                    && local_id != result_local
                    && !ctx.partial_moved_locals.contains(&local_id.0)
                {
                    drop_tag(ctx, "block-scope-end");
                    ctx.emit(Instruction::Drop { local: local_id });
                    ctx.dropped_locals.insert(local_id.0);
                }
            }
        }
        if ctx.blocks.len() < ctx.next_block as usize {
            ctx.seal_block(Terminator::Return(Some(Operand::Local(result_local))));
        }
    } else if has_tail_expr && !body.stmts.is_empty() {
        let (init, last) = body.stmts.split_at(body.stmts.len() - 1);
        // Lower all statements except the last.
        let scope_start = ctx.locals.len();
        for stmt in init {
            lower_stmt(ctx, stmt);
        }
        // Lower the tail expression and capture its result.
        if let ast::Stmt::Expr { expr, .. } = &last[0] {
            let tail_val = lower_expr_to_operand(ctx, expr);
            // Extract the local ID of the tail value (if it's a local) so we
            // don't drop the value we're about to return.
            let tail_local_id = match &tail_val {
                Operand::Local(id) => Some(id.0),
                _ => None,
            };
            // Partial-move fix: if the tail expression is a field access on a
            // named local (e.g. `return result.value`), the struct local was
            // NOT selected as tail_local_id (the unnamed field-temp was), so
            // it would be dropped even though its field was moved out. Detect
            // this and exclude the source struct local from drops as well.
            let source_struct_local_id = if let ast::Expr::FieldAccess { object, .. } = expr {
                if let ast::Expr::Identifier { name, .. } = object.as_ref() {
                    ctx.locals
                        .iter()
                        .rev()
                        .find(|l| l.name.as_deref() == Some(name.as_str()))
                        .map(|l| l.id.0)
                } else {
                    None
                }
            } else {
                None
            };
            // Emit drops for scope locals before returning.
            let scope_end = ctx.locals.len();
            for i in (scope_start..scope_end).rev() {
                if ctx.locals[i].name.is_some() {
                    let local_id = ctx.locals[i].id;
                    // Skip function parameters — caller owns them.
                    if ctx.param_locals.contains(&local_id.0)
                        || ctx.borrowed_locals.contains(&local_id.0)
                    {
                        continue;
                    }
                    if !ctx.dropped_locals.contains(&local_id.0)
                        && tail_local_id != Some(local_id.0)
                        && source_struct_local_id != Some(local_id.0)
                        && !ctx.partial_moved_locals.contains(&local_id.0)
                    {
                        drop_tag(ctx, "block-expr-scope-end");
                        ctx.emit(Instruction::Drop { local: local_id });
                        ctx.dropped_locals.insert(local_id.0);
                    }
                }
            }
            // Seal with implicit return of the tail expression.
            if ctx.blocks.len() < ctx.next_block as usize {
                ctx.seal_block(Terminator::Return(Some(tail_val)));
            }
        }
    } else {
        lower_block_stmts(ctx, &body.stmts);

        // If the current block hasn't been sealed yet, add an implicit return.
        if ctx.blocks.len() < ctx.next_block as usize {
            ctx.seal_block(Terminator::Return(None));
        }
    }

    MirFunction {
        name: name.to_string(),
        params: mir_params,
        ret_ty: mir_ret_ty,
        blocks: ctx.blocks.clone(),
        locals: ctx.locals.clone(),
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    }
}

// ---------------------------------------------------------------------------
// Statement lowering
// ---------------------------------------------------------------------------

/// Emit drops for *named* locals declared in `ctx.locals[scope_start..]`
/// (reverse order), into whatever block is currently `ctx.current_block`.
///
/// Shared by `lower_block_stmts` (a block used as a plain statement) and the
/// `Expr::Block`/`Expr::UnsafeBlock` rvalue case (a block used as a VALUE,
/// e.g. a match-arm body or an if-as-value branch). Before this was
/// factored out, the rvalue path had NO scope-exit drop of its own at all —
/// a named `let` declared inside a match-arm block (e.g. `let r = Ok(..)`
/// inside `E.B => { let r = ..; match r { .. } }`) was never dropped at the
/// end of ITS OWN arm. The local stayed un-dropped in `ctx.locals`, so the
/// NEXT enclosing `lower_block_stmts` call that happened to cover it (e.g.
/// the `for`-loop body wrapping the whole outer `match`) swept it up
/// instead — unconditionally, in the loop body's own shared exit block,
/// on EVERY iteration, regardless of which arm of the outer match actually
/// ran that iteration. On an iteration that took a DIFFERENT arm (one that
/// never touched `r` this time), that dropped whatever STALE value `r`'s
/// slot held from a previous iteration (or zero-init on the first) — a
/// null-deref or a double-free of an already-freed prior box. This is the
/// verified root cause of the F1 struct/loop/nested-match crash (gotcha
/// #23's "BROAD, HIGH-PRIORITY crash hazard"): dropping `r` correctly at
/// the end of the E.B arm's OWN block (this function, called from the
/// block-rvalue case) means the outer loop-body scope never sees `r` left
/// over to double-drop.
///
/// Unnamed temporaries (name == None) must NOT be dropped because they
/// hold non-owning copies of values (e.g. a string handle loaded from a
/// struct field). Dropping them would free memory that the struct still
/// owns, causing heap corruption / use-after-free.
///
/// We also skip locals that were already dropped (by a nested inner scope,
/// or by a tail-expression move) to prevent double-free.
fn emit_named_scope_drops(ctx: &mut LoweringContext, scope_start: usize) {
    let scope_end = ctx.locals.len();
    for i in (scope_start..scope_end).rev() {
        if ctx.locals[i].name.is_some() {
            let local_id = ctx.locals[i].id;
            // Skip function parameters — the caller owns them, not the callee.
            if ctx.param_locals.contains(&local_id.0) || ctx.borrowed_locals.contains(&local_id.0) {
                continue;
            }
            if !ctx.dropped_locals.contains(&local_id.0)
                && !ctx.partial_moved_locals.contains(&local_id.0)
            {
                let lbl = format!(
                    "fn-exit:{}:{:?}",
                    ctx.locals[i].name.as_deref().unwrap_or("?"),
                    ctx.locals[i].ty
                );
                drop_tag(ctx, &lbl);
                ctx.emit(Instruction::Drop { local: local_id });
                ctx.dropped_locals.insert(local_id.0);
            }
        }
    }
    // Hide this scope's named locals from name resolution now that the scope
    // has exited, so an outer scope's same-named binding re-resolves to the
    // OUTER local. Without this, a value-position block (`{ let i = 5 }`), a
    // match-arm body, or any block reached only through this path leaked its
    // inner binding: `let i = 777  { let i = 5 }  i` read 5, and a
    // type-changed shadow (str outer, i64 inner) then read a raw int as a
    // string handle and SEGFAULTED. `lower_block_stmts` already hid its own
    // scope separately (that path was correct); centralizing the hide here
    // covers every scope-exit caller uniformly (its now-redundant explicit
    // hide is harmless -- HashSet inserts are idempotent).
    for i in scope_start..scope_end {
        if ctx.locals[i].name.is_some() {
            ctx.hidden_locals.insert(ctx.locals[i].id.0);
        }
    }
}

fn lower_block_stmts(ctx: &mut LoweringContext, stmts: &[ast::Stmt]) {
    let scope_start = ctx.locals.len();

    for stmt in stmts {
        lower_stmt(ctx, stmt);
    }

    emit_named_scope_drops(ctx, scope_start);

    // Hide scoped locals from name resolution so outer scopes don't
    // see inner bindings (prevents variable shadowing leaks like
    // `let x = 100; if true { let x = 999 } println(x)` printing 999).
    // We add them to hidden_locals rather than clearing names, so the
    // final MIR output still has names for debugging/introspection.
    let scope_end = ctx.locals.len();
    for i in scope_start..scope_end {
        if ctx.locals[i].name.is_some() {
            ctx.hidden_locals.insert(ctx.locals[i].id.0);
        }
    }
}

/// Lower a block's statements, treating the last statement as a value
/// that gets assigned to `result_local`. Handles `Stmt::Expr` (expressions),
/// `Stmt::If` (nested if-statements used as values), and `Stmt::Match`.
fn lower_block_as_value(ctx: &mut LoweringContext, stmts: &[ast::Stmt], result_local: LocalId) {
    if stmts.is_empty() {
        return;
    }
    // Lower all statements except the last normally.
    for stmt in &stmts[..stmts.len() - 1] {
        lower_stmt(ctx, stmt);
    }
    // Lower the last statement as a value.
    match stmts.last().unwrap() {
        ast::Stmt::Expr { expr, .. } => {
            let rv = lower_expr_to_rvalue(ctx, expr);
            ctx.emit(Instruction::Assign {
                dest: result_local,
                value: rv,
            });
            // A bare-identifier tail MOVES the local into the result (Use is
            // a bit-copy with no retain). Mark the source local dropped so
            // scope cleanup doesn't free the value the result now owns
            // (e.g. `catch e { e }` previously returned a freed string).
            if let ast::Expr::Identifier { name, .. } = expr {
                if let Some(l) = ctx
                    .locals
                    .iter()
                    .rev()
                    .find(|l| l.name.as_deref() == Some(name.as_str()))
                {
                    let id = l.id.0;
                    ctx.dropped_locals.insert(id);
                }
            }
            // (Enum-variant-wrapped tail moves like `catch e { Err(e) }` are
            // handled generally at the EnumVariant construction site, which
            // drop-suppresses bare heap-local fields in every context.)
        }
        ast::Stmt::If {
            condition,
            then_block,
            elif_clauses,
            else_block,
            ..
        } => {
            // Lower an if-statement as a value-producing expression.
            let cond_op = lower_expr_to_operand(ctx, condition);
            let then_bb = ctx.alloc_block();
            let else_bb = ctx.alloc_block();
            let merge_bb = ctx.alloc_block();

            ctx.finish_block(
                Terminator::Branch {
                    cond: cond_op,
                    then_block: then_bb,
                    else_block: else_bb,
                },
                then_bb,
            );

            // Then branch — recursively handle nested if-as-value.
            lower_block_as_value(ctx, &then_block.stmts, result_local);
            ctx.finish_block(Terminator::Goto(merge_bb), else_bb);

            // Elif/else chain.
            if !elif_clauses.is_empty() {
                for (i, (elif_cond, elif_body)) in elif_clauses.iter().enumerate() {
                    let elif_cond_op = lower_expr_to_operand(ctx, elif_cond);
                    let elif_then_bb = ctx.alloc_block();
                    let elif_else_bb = if i + 1 < elif_clauses.len() || else_block.is_some() {
                        ctx.alloc_block()
                    } else {
                        merge_bb
                    };
                    ctx.finish_block(
                        Terminator::Branch {
                            cond: elif_cond_op,
                            then_block: elif_then_bb,
                            else_block: elif_else_bb,
                        },
                        elif_then_bb,
                    );
                    lower_block_as_value(ctx, &elif_body.stmts, result_local);
                    ctx.finish_block(Terminator::Goto(merge_bb), elif_else_bb);
                }
                if let Some(else_blk) = else_block {
                    lower_block_as_value(ctx, &else_blk.stmts, result_local);
                    ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
                }
            } else if let Some(else_blk) = else_block {
                lower_block_as_value(ctx, &else_blk.stmts, result_local);
                ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
            } else {
                ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
            }
        }
        ast::Stmt::TryCatch {
            try_block,
            catch_name,
            catch_block,
            ..
        } => {
            // try/catch in value position: both the try tail (Ok payload)
            // and the catch tail write the result local.
            lower_try_catch(ctx, try_block, catch_name, catch_block, Some(result_local));
        }
        // For any other statement kind (let, return, etc.), just lower it
        // normally — it doesn't produce a value.
        other => lower_stmt(ctx, other),
    }
}

fn lower_stmt(ctx: &mut LoweringContext, stmt: &ast::Stmt) {
    let inst_mark = ctx.current_instructions.len();
    let block_mark = ctx.next_block;
    let locals_mark = ctx.locals.len();
    // Snapshot of partial-moved locals BEFORE this statement lowers, so the
    // spurious-partial-move undo below (see `drop_unescaped_str_temps`'s
    // field_source handling) can tell "this statement's own field-read just
    // marked src" apart from "an earlier statement genuinely moved a field
    // out of src" -- only the former is safe to reverse.
    let partial_moved_before = ctx.partial_moved_locals.clone();
    lower_stmt_inner(ctx, stmt);
    drop_unescaped_str_temps(ctx, inst_mark, block_mark, locals_mark, &partial_moved_before);
}

/// Calls that read a heap-typed argument and neither store nor free it --
/// shared by `drop_unescaped_str_temps` (safe to drop a temp fed only to
/// one of these) and `consume_call_args` (safe NOT to suppress the caller's
/// own scope-end Drop of an arg fed only to one of these). Deliberately
/// small and conservative: an unlisted callee (including any user-defined
/// function) keeps the existing consume-on-call-argument behavior.
const BORROWING_CALL_ARGS: &[&str] = &[
    // The deep-clone markers only READ their source aggregate (the clone's
    // result is a fresh independent value owned by the destination); the
    // source keeps its own scope-end drop. Without this, the struct/enum
    // `let copy = src` rewrite re-marked src consumed via the generic
    // call-arg-consume path -- reintroducing the exact move-suppression the
    // rewrite exists to remove (src's fields would leak once per copy).
    "__kryos_struct_index_clone",
    "__kryos_enum_index_clone",
    "len",
    "to_string",
    "println",
    "print",
    "eprintln",
    "eprint",
    "contains",
    "substr",
    "char_code",
    "parse_int",
    "parse_float",
    // Reads both operands, returns a fresh allocation; never stores or
    // frees its inputs (the binary `+` runtime path).
    "kryos_string_concat",
    // Map/string accessor calls the surface builtins (`m[k]`, `contains`,
    // `keys`, `map_delete`, `s[i]`) get rewritten to at lowering time --
    // consume_call_args/drop_unescaped_str_temps see THIS name, not the
    // surface one. All of these read or mutate-in-place the container
    // (map/string) handle passed as arg 0 -- none of them take ownership of
    // it or free it, so the container must keep its own scope-end Drop.
    // (Array reads/writes use `RValue::Index`, not a Call, so they never
    // reach consume_call_args in the first place -- no entry needed here.)
    "kryos_map_get",
    "kryos_map_get_str",
    "kryos_map_has",
    "kryos_map_has_str",
    "kryos_map_keys",
    "kryos_map_keys_str",
    "kryos_map_delete",
    "kryos_map_delete_str",
    "kryos_string_char_at",
    // `m[k] = v` (IndexAssign) lowers to kryos_map_insert(_str)(map, key, val)
    // directly -- it never reaches consume_call_args at all (a separate
    // lowering site, not the generic call path), so listing these here only
    // affects drop_unescaped_str_temps's treatment of the VALUE argument.
    // That site already compensates: it emits its OWN retain
    // (kryos_string_retain_opt/kryos_array_retain_opt) on a container-typed
    // value right after the insert call, giving the map an independent
    // share. The caller's original value temp therefore still owns its OWN
    // (un-retained) reference and must still get ITS OWN normal drop --
    // without these entries the temp-drop walk bailed on the unrecognized
    // callee and never dropped it, leaking one reference's worth of content
    // per iteration (measured: a `map<i64,str>` loop leaked ~225 MB/3M despite
    // the map's own container Drop already firing correctly). The KEY
    // argument is unconditionally safe too: `insert_str` deep-copies the key
    // into its own new allocation (see kryos_map_insert_str, kryos-rt) rather
    // than aliasing the caller's, so the caller's key temp was never at risk
    // either.
    "kryos_map_insert",
    "kryos_map_insert_str",
    // Compiler-emitted compensating retains (kryos_map_insert(_str)'s value-
    // arg retain, "let copy = container" bindings, borrow-to-own wrapping,
    // etc.) always just bump a refcount and return the SAME handle -- they
    // never store the argument elsewhere or free it, so passing a temp to
    // one is never an escape. Without this, the retain call itself (which
    // sits between a temp's definition and its otherwise-provably-safe use)
    // made the temp-drop walk bail on an "unrecognized callee", leaking the
    // caller's own share even after the container-side entry above let the
    // caller's use of the temp survive.
    "kryos_string_retain",
    "kryos_string_retain_opt",
    "kryos_array_retain",
    "kryos_array_retain_opt",
    "kryos_map_retain",
    "kryos_map_retain_opt",
];

/// Does `src`'s definition, within this instruction window, match the
/// "Borrow-to-own" container-get pattern -- `src = call kryos_map_get(_str)`
/// (a map read whose VALUE type is itself str/array/map) immediately
/// compensated by a retain call on `src`? Both codegen backends emit this
/// exact pair (`lower_expr_to_rvalue`'s Map IndexAccess arm) whenever a map's
/// value type is a container, precisely so the destination independently
/// owns a reference instead of aliasing the map's own entry. If so, whatever
/// this value is subsequently `Use`d into inherits that owned reference.
fn is_retained_container_get(instructions: &[Instruction], src: LocalId) -> bool {
    const RETAIN_FNS: &[&str] = &[
        "kryos_string_retain",
        "kryos_string_retain_opt",
        "kryos_array_retain",
        "kryos_array_retain_opt",
        "kryos_map_retain",
        "kryos_map_retain_opt",
    ];
    let mut defined_by_map_get = false;
    let mut retained = false;
    for inst in instructions {
        if let Instruction::Assign { dest, value } = inst {
            if *dest == src {
                if let RValue::Call { func, .. } = value {
                    if func == "kryos_map_get" || func == "kryos_map_get_str" {
                        defined_by_map_get = true;
                    }
                }
                continue;
            }
            if let RValue::Call { func, args } = value {
                if RETAIN_FNS.contains(&func.as_str())
                    && args.iter().any(|a| matches!(a, Operand::Local(l) if *l == src))
                {
                    retained = true;
                }
            }
        }
    }
    defined_by_map_get && retained
}

/// Statement-end cleanup for string expression TEMPORARIES (leak fix).
///
/// Named locals get scope-end drops, but subexpression temps (the result of
/// `to_string(i)` feeding a concat, the heap string a literal allocates, the
/// intermediate of `a + b + c`) had NO drop path at all: a simple string
/// churn loop grew 218MB -> 1020MB over 10 rounds. This emits `Drop` for
/// Str-typed unnamed temps created during one straight-line statement whose
/// every use is a provably borrowing operation.
///
/// Conservative by construction:
/// - Bails when the statement created control flow (SSA dominance) — so
///   if/while/match/return statements are untouched.
/// - Bails when the window contains ANY instruction other than
///   Assign/Drop/Nop/DebugLine (an instruction shape we don't model could
///   hide an escape).
/// - A temp is dropped only if its uses are exclusively: StringConcat or
///   BinOp operands, or arguments to builtins known to borrow. Appearing
///   anywhere else (Use into another slot, aggregate init, user call,
///   store, ...) disqualifies it.
///
/// Also undoes a SPURIOUS `partial_moved_locals` mark on a struct source: a
/// bare field read (`r.name`) taken by `lower_expr_to_rvalue`'s FieldAccess
/// arm unconditionally marks the struct local `r` as partially-moved (so its
/// own scope-end Drop is suppressed), on the theory that the field's heap
/// data was moved out. That theory only holds when the read result escapes
/// (is stored, returned, bound to a name); a read whose result is used
/// exclusively by a borrowing call/operator in this same straight-line
/// statement (e.g. `len(r.name)`) never escapes, so `r` was never actually
/// partially moved -- suppressing its Drop leaked the whole struct (every
/// field, not just the one read) forever after the first field read
/// anywhere in the function. See `consume_call_args` for the analogous fix
/// on the "passed as a call argument" side of the same over-eager-move bug.
fn drop_unescaped_str_temps(
    ctx: &mut LoweringContext,
    inst_mark: usize,
    block_mark: u32,
    locals_mark: usize,
    partial_moved_before: &std::collections::HashSet<u32>,
) {
    // Diagnostic kill-switch (KRYOS_NO_TEMP_DROPS=1) for bisecting runtime
    // faults to this pass without a rebuild.
    if std::env::var_os("KRYOS_NO_TEMP_DROPS").is_some() {
        return;
    }
    if ctx.next_block != block_mark {
        return; // statement created blocks: temps may not dominate this point
    }
    // Candidates were originally Str-only. Extended to Array/Map so a
    // container-VALUED map (`map<str, [i64]>`, `map<K, map<...>>`, ...)
    // gets the same unescaped-temp cleanup: the map-insert site always
    // retains a container-typed value for the map's own share (see
    // `BORROWING_CALL_ARGS`'s doc comment on `kryos_map_insert(_str)`), which
    // means the ORIGINAL value temp still owns its pre-retain reference and
    // needs its own drop here -- same requirement Str values already had,
    // just never extended to Array/Map, so `map<str, [i64]>` leaked the
    // array contents every loop iteration (measured ~260MB/2M iters) despite
    // the map's own typed free (`kryos_map_free_typed`) working correctly.
    let candidates: Vec<LocalId> = ctx.locals[locals_mark..]
        .iter()
        .filter(|l| {
            l.name.is_none()
                && matches!(l.ty, MirType::Str | MirType::Array(_, _) | MirType::Map { .. })
                && !l.mutable
        })
        .map(|l| l.id)
        .collect();
    if candidates.is_empty() {
        return;
    }
    // Whole-window guard: only instruction shapes we fully model.
    for inst in &ctx.current_instructions[inst_mark..] {
        match inst {
            Instruction::Assign { .. }
            | Instruction::Drop { .. }
            | Instruction::Nop
            | Instruction::DebugLine(_) => {}
            _ => return,
        }
    }
    let mentions = |op: &Operand, id: LocalId| matches!(op, Operand::Local(l) if *l == id);
    let mut to_drop: Vec<LocalId> = Vec::new();
    'cand: for id in candidates {
        let mut owns = false;
        // Set when `id`'s definition is `RValue::Field { object: Local(src), .. }`
        // -- a borrowed field read, not a fresh allocation. If `id` survives the
        // whole escape walk below (every use, if any, is a known-borrowing call
        // arg), the field read never escaped and `src`'s spurious partial-move
        // mark (see this function's doc comment) is safe to undo.
        let mut field_source: Option<LocalId> = None;
        for inst in &ctx.current_instructions[inst_mark..] {
            let (dest, value) = match inst {
                Instruction::Assign { dest, value } => (dest, value),
                _ => continue,
            };
            if *dest == id {
                // Temps whose DEFINITION allocates a fresh owned string may
                // be dropped directly (`owns`). A struct-FIELD read is
                // handled separately below via `field_source`: both backends
                // retain it unconditionally, so (unlike a plain alias) it is
                // safe to drop once proven non-escaping. Array/tuple INDEX
                // reads are deliberately left alone here -- dropping one
                // previously freed a still-shared element out from under the
                // container (the split_join regression) and that path's
                // retain/consume accounting has not been re-audited.
                owns = match value {
                    RValue::StringConcat(_) => true,
                    // A fresh array/map LITERAL (`[i, i+1, i+2]`, `{}`) is a
                    // brand-new allocation with exactly one reference, same
                    // as a StringConcat -- owned outright by this temp.
                    RValue::Array(_) => true,
                    RValue::Map(_) => true,
                    // Binary `+` on strings is MIR BinOp::Add with Str
                    // operands (the backend expands it to concat calls);
                    // candidates are pre-filtered to Str-typed temps, so an
                    // Add-defined one is a fresh concat allocation it owns.
                    RValue::BinOp { op: MirBinOp::Add, .. } => true,
                    RValue::Call { func, .. } => {
                        func == "to_string"
                            || func == "kryos_string_concat"
                            // `id` IS the map-get destination itself (the
                            // Borrow-to-own temp from `lower_expr_to_rvalue`'s
                            // Map IndexAccess arm), immediately followed in
                            // this same window by a retain call on `id` --
                            // e.g. reading a container-typed map value via
                            // `m["k"]` mid-expression (`m["k"][0]`,
                            // `m["k"]["k2"]`). That retain already gave `id`
                            // its own independent reference (one hop, not the
                            // `Use(Local(src))` two-hop alias case below), so
                            // it owns exactly what it holds.
                            || ((func == "kryos_map_get" || func == "kryos_map_get_str")
                                && is_retained_container_get(
                                    &ctx.current_instructions[inst_mark..],
                                    id,
                                ))
                    }
                    // `id = src` where `src`'s OWN definition in this same
                    // window is a map-get of a container-typed value,
                    // immediately compensated by a "Borrow-to-own" retain on
                    // `src` (lower_expr_to_rvalue's Map IndexAccess arm).
                    // That retain already gave `src` an independently-owned
                    // reference; this Use is `src`'s ONLY mention besides its
                    // own definition and the retain call (checked by the
                    // escape walk below, which bails `src` out of `to_drop`
                    // for exactly this reason), so `id` is now the sole
                    // holder of that retained reference, not a bare alias.
                    RValue::Use(Operand::Local(src)) => {
                        is_retained_container_get(&ctx.current_instructions[inst_mark..], *src)
                    }
                    _ => false,
                };
                if let RValue::Field { object: Operand::Local(src), .. } = value {
                    field_source = Some(*src);
                }
                continue; // its own definition consumes other values, not itself
            }
            let used_here: bool = match value {
                RValue::StringConcat(parts) => parts.iter().any(|p| mentions(p, id)),
                RValue::BinOp { left, right, .. } => mentions(left, id) || mentions(right, id),
                RValue::Call { func, args } => {
                    if args.iter().any(|a| mentions(a, id)) {
                        if BORROWING_CALL_ARGS.contains(&func.as_str()) {
                            true
                        } else {
                            continue 'cand; // unknown callee may take ownership
                        }
                    } else {
                        false
                    }
                }
                // Reading an ELEMENT out of the candidate (`tmp[0]`, the
                // array half of `m["k"][0]`) is a pure borrow: it loads one
                // element and never stores or frees the container pointer
                // itself, so a mention here is not an escape. (This is
                // distinct from the doc comment above about NOT marking an
                // Index-DEFINED temp as owning -- this arm only concerns `id`
                // appearing as the object/index of some OTHER instruction's
                // read.)
                RValue::Index { object, index } => mentions(object, id) || mentions(index, id),
                // Any other rvalue shape touching this temp is a potential
                // escape (Use copies the pointer, aggregate inits store it...).
                other => {
                    if rvalue_mentions_local(other, id) {
                        continue 'cand;
                    }
                    false
                }
            };
            let _ = used_here;
        }
        if owns {
            to_drop.push(id);
        } else if let Some(src) = field_source {
            // Both backends unconditionally RETAIN a struct-field read of a
            // heap-typed field (Cranelift's "step 44" field-read retain /
            // LLVM's "Borrow-to-own" kryos_string_retain(_opt) etc.) -- `id`
            // now holds its OWN independently-retained reference, matching
            // the source struct's. Since `id` never escaped this straight-
            // line statement (the walk above didn't bail), that retain must
            // be balanced by dropping `id` here same as any other owned
            // temp -- this is true regardless of the source's own drop
            // status, since the retain always fires.
            to_drop.push(id);
            // Separately: undo a partial-move mark THIS statement's field
            // read put on `src`, so the struct's own scope-end Drop (which
            // frees its other fields too) isn't left spuriously suppressed.
            // Only undo if this exact statement set it -- an earlier
            // statement's genuine move of a different field must stay
            // suppressed.
            if !partial_moved_before.contains(&src.0) {
                ctx.partial_moved_locals.remove(&src.0);
            }
        }
    }
    for id in to_drop {
        // Guard against a DOUBLE drop: this pass runs at the end of every
        // `lower_stmt`, and a nested statement's window overlaps its
        // enclosing statement's window. For `let bs = { let inner =
        // to_string(x) + "y"; inner + "!" }`, the inner `let inner` pass
        // drops the `to_string` temp, then the outer `let bs` pass -- whose
        // window still contains that temp -- dropped it AGAIN (a heap
        // double-free that corrupted the block value on AOT). Record each
        // drop so a later overlapping window skips it.
        if ctx.dropped_locals.contains(&id.0) {
            continue;
        }
        drop_tag(ctx, "str-temp-drop");
        ctx.emit(Instruction::Drop { local: id });
        ctx.dropped_locals.insert(id.0);
    }
}

/// Does this rvalue reference the given local anywhere in its operands?
/// Conservative helper for the temp-drop pass; must cover every operand-
/// carrying variant (unknown shapes are handled by the caller's whole-window
/// guard, which only admits Assign instructions in the first place).
fn rvalue_mentions_local(rv: &RValue, id: LocalId) -> bool {
    let m = |op: &Operand| matches!(op, Operand::Local(l) if *l == id);
    match rv {
        RValue::Use(op)
        | RValue::UnOp { operand: op, .. }
        | RValue::ArcAlloc { inner: op, .. }
        | RValue::Cast { operand: op, .. }
        | RValue::EnumTag { operand: op }
        | RValue::EnumPayload { operand: op, .. }
        | RValue::Deref { operand: op, .. }
        | RValue::MakeTraitObject { value: op, .. } => m(op),
        RValue::BinOp { left, right, .. } => m(left) || m(right),
        RValue::StringConcat(parts) | RValue::EnumVariant { fields: parts, .. } => {
            parts.iter().any(m)
        }
        RValue::Array(parts) | RValue::Tuple(parts) => parts.iter().any(m),
        RValue::Call { args, .. } => args.iter().any(m),
        RValue::CallIndirect { callee, args, .. } => m(callee) || args.iter().any(m),
        RValue::VtableCall { object, args, .. } => m(object) || args.iter().any(m),
        RValue::Struct { fields, .. } => fields.iter().any(|(_, op)| m(op)),
        RValue::Field { object, .. } => m(object),
        RValue::Index { object, index, .. } => m(object) || m(index),
        RValue::Map(entries) => entries.iter().any(|(k, v)| m(k) || m(v)),
        RValue::Closure { captures, .. } => captures.iter().any(m),
        RValue::Range { start, end, .. } => {
            start.as_ref().is_some_and(m) || end.as_ref().is_some_and(m)
        }
        RValue::AddrOf { local, .. } => *local == id,
        RValue::Comptime(inner) => rvalue_mentions_local(inner, id),
        RValue::ConstInt(_)
        | RValue::ConstFloat(_)
        | RValue::ConstBool(_)
        | RValue::ConstString(_)
        | RValue::ConstNone => false,
    }
}

fn lower_stmt_inner(ctx: &mut LoweringContext, stmt: &ast::Stmt) {
    // Debug-line instrumentation: when the DAP debugger is driving this
    // compilation, mark each statement with its source line so the backend can
    // emit a `kryos_dbg_line` hook before it. No-op (and zero cost) for normal
    // builds, where no resolver is installed.
    if let Some(line) = crate::debug_lines::resolve_debug_line(stmt.span()) {
        ctx.emit(Instruction::DebugLine(line));
    }

    match stmt {
        ast::Stmt::Let {
            name,
            mutable,
            ty,
            value,
            pattern,
            span: let_span,
            ..
        } => {
            let mir_ty = if let Some(t) = ty {
                // A generic function body monomorphized for concrete type args
                // still carries the RAW generic annotation on its locals
                // (`let inner: T` / `let inner: Box<T>`) -- resolve_type would
                // yield a placeholder `Struct("T")` and LLVM emitted `load %T`
                // (AOT build fail; the JIT tolerated the opaque type). Substitute
                // the active generic bindings (set by `monomorphize` around the
                // body lowering) into the annotation first, so `T` -> `i64`,
                // `Box<T>` -> `Box<i64>`. No-op when not in a monomorphized body.
                if ctx.active_generic_bindings.is_empty() {
                    ctx.resolve_type(t)
                } else {
                    let substituted = substitute_type_expr(t, &ctx.active_generic_bindings);
                    ctx.resolve_type(&substituted)
                }
            } else if let Some(te) = ctx.let_types.get(let_span).cloned() {
                // Type-checker-resolved type for an unannotated empty-array `let`
                // (element type came from later `push`). Without this the MIR's
                // own inference defaults the empty array's element to i64, which
                // mis-types `X[i].field` / aggregate elements on AOT.
                ctx.resolve_type(&te)
            } else if let Some(expr) = value {
                // No explicit type annotation — infer from the initializer.
                infer_expr_type(ctx, expr)
            } else {
                MirType::I64
            };

            // Lower the RHS BEFORE allocating the new local.  This ensures
            // that variable name lookups in the initializer resolve to the
            // previous binding (important for `let x = f(x)` shadowing).
            // Expose the resolved annotation to the RHS lowering so a
            // no-arg generic static ctor (`List.new()`) can bind its type
            // params from it (see pending_let_expected).
            let saved_let_expected = ctx.pending_let_expected.take();
            if ty.is_some() {
                ctx.pending_let_expected = Some(mir_ty.clone());
            }
            let rvalue_and_meta = if let Some(expr) = value {
                // Mark source locals as non-owning when the initializer
                // borrows from another value.
                match expr {
                    ast::Expr::IndexAccess { .. } | ast::Expr::FieldAccess { .. } => {
                        // The new local itself will be marked after allocation.
                    }
                    ast::Expr::StructLiteral { fields, .. } => {
                        // If struct field values come from FieldAccess on other
                        // locals, mark those sources as non-owning.
                        for (_fname, fexpr) in fields {
                            if let ast::Expr::FieldAccess { object, .. } = fexpr {
                                if let ast::Expr::Identifier { name: src_name, .. } =
                                    object.as_ref()
                                {
                                    if let Some(src_local) = find_local_by_name(ctx, src_name) {
                                        ctx.borrowed_locals.insert(src_local.0);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }

                let is_shared = matches!(expr, ast::Expr::SharedExpr { .. });
                // A plain `let name = |...| { ... }` (no destructuring
                // pattern) may recurse through its own name -- see
                // `pending_self_recursive_name` doc comment. Set it just
                // before lowering the RHS so the `Expr::Lambda` arm can
                // register the self-call mapping for its own body frame.
                if pattern.is_none() && matches!(expr, ast::Expr::Lambda { .. }) {
                    ctx.pending_self_recursive_name = Some(name.clone());
                }
                // dyn Trait coercion for an explicitly-annotated `let`: `let
                // s: dyn Shape = r` where `r` (or any struct-valued
                // initializer) is a concrete struct. Lower via the operand
                // path (same as a call argument) so a bare identifier reuses
                // its existing local and anything else materializes exactly
                // once, then wrap in MakeTraitObject -- mirrors the
                // call-argument coercion below. Without this the `dyn`-typed
                // local held a raw struct and the later vtable_call
                // dereferenced garbage (SIGSEGV on both backends). A value
                // that is already `dyn Trait`-typed (forwarding an existing
                // binding) is left untouched by `coerce_to_dyn_if_needed`.
                let rvalue = if matches!(mir_ty, MirType::DynTrait(_))
                    && matches!(infer_expr_type(ctx, expr), MirType::Struct(_))
                {
                    let operand = lower_expr_to_operand(ctx, expr);
                    RValue::Use(coerce_to_dyn_if_needed(ctx, operand, &mir_ty, expr))
                } else {
                    lower_expr_to_rvalue(ctx, expr)
                };
                let mark_non_owning = matches!(
                    expr,
                    ast::Expr::IndexAccess { .. } | ast::Expr::FieldAccess { .. }
                );

                // Track closures with captures for direct-call optimization.
                // NEVER for a closure that mutates one of its captures: it
                // captures by move and owns persistent state, so the
                // direct-call shortcut (re-read the outer variable's
                // CURRENT value at each call site) would silently reset
                // that state to the outer value every call instead of
                // threading the mutation forward (see `mutating_closures`).
                let closure_info = if let RValue::Closure {
                    ref func_name,
                    ref captures,
                } = rvalue
                {
                    if !captures.is_empty() && !ctx.mutating_closures.contains(func_name.as_str())
                    {
                        Some((func_name.clone(), captures.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                };

                Some((rvalue, mark_non_owning, closure_info, is_shared))
            } else {
                None
            };
            ctx.pending_let_expected = saved_let_expected;

            // Tuple destructuring: `let (a, b, c) = expr`
            if let Some(ast::Pattern::Tuple { elements, .. }) = pattern {
                // Assign the RHS to a temporary local, then extract each element.
                let tmp = ctx.alloc_local(None, mir_ty.clone(), false);
                if let Some((rvalue, _, _, _)) = rvalue_and_meta {
                    ctx.emit(Instruction::Assign {
                        dest: tmp,
                        value: rvalue,
                    });
                }
                for (idx, elem_pat) in elements.iter().enumerate() {
                    if let ast::Pattern::Ident {
                        name: elem_name,
                        mutable: elem_mut,
                        ..
                    } = elem_pat
                    {
                        let elem_ty = if let MirType::Tuple(ref elems) = mir_ty {
                            elems.get(idx).cloned().unwrap_or(MirType::I64)
                        } else {
                            MirType::I64
                        };
                        // Honor either the outer `let mut (...)` modifier or a
                        // per-element `mut` inside the tuple pattern
                        // (`let (mut a, b) = ...`). Without this, per-element
                        // mut was silently dropped and assignments to `a`
                        // raised "assignment to immutable variable" warnings.
                        let is_mutable = *mutable || *elem_mut;
                        let elem_local =
                            ctx.alloc_local(Some(elem_name.clone()), elem_ty, is_mutable);
                        ctx.emit(Instruction::Assign {
                            dest: elem_local,
                            value: RValue::Field {
                                object: Operand::Local(tmp),
                                field: idx.to_string(),
                            },
                        });
                    }
                }
                return;
            }

            // Now allocate the new local (after RHS is evaluated).
            let local = ctx.alloc_local(Some(name.clone()), mir_ty, *mutable);

            // If this `let`'s declared type names an actor, `resolve_type`
            // (above, for `mir_ty`) just erased it to a bare `I64` handle.
            // Record the logical actor name in `local_actor_types` before
            // that's lost -- mirrors the param-loop registration in
            // `lower_function`, for the "actor returned from a function,
            // bound via an annotated `let`" case (`let g: Counter =
            // get_counter(c)`). Without an annotation, fall back to
            // `fn_return_actor_types` when the initializer is a direct call
            // to a function with a registered actor return (`let g =
            // get_counter(c)`) -- same recovery, driven by the callee's
            // signature instead of this `let`'s own annotation.
            if let Some(ast::TypeExpr::Simple { name: tn, .. }) = ty {
                if ctx.actor_defs.contains_key(tn) {
                    ctx.local_actor_types.insert(local.0, tn.clone());
                }
            } else if let Some(ast::Expr::FnCall { callee, .. }) = value {
                if let ast::Expr::Identifier { name: callee_name, .. } = callee.as_ref() {
                    if let Some(actor_ty) = ctx.fn_return_actor_types.get(callee_name).cloned() {
                        ctx.local_actor_types.insert(local.0, actor_ty);
                    }
                }
            }

            if let Some((mut rvalue, mark_non_owning, closure_info, is_shared)) = rvalue_and_meta {
                if mark_non_owning {
                    ctx.borrowed_locals.insert(local.0);
                }
                if let Some((func_name, captures)) = closure_info {
                    ctx.closure_locals
                        .insert(name.clone(), (func_name, captures));
                }

                // `let copy = src` where src is a bare local. For refcounted
                // CONTAINERS (Str/Array/Map) this is a SHARE, not a move: both
                // bindings stay valid (Kryos's ARC model -- `let a = s; use(s)`
                // is legal). The new binding must therefore take its OWN
                // reference, or a loop-local `let copy = s` frees the buffer
                // the outer `s` still holds on the first iteration's scope-end
                // drop (differential-fuzz seed 14: `s` became empty/garbage
                // after the loop). We retain the shared container into `local`
                // and do NOT mark src consumed. For non-container non-copy
                // values (structs/enums), keep the move: mark src consumed so
                // its scope-end drop is skipped (no double-free).
                let mut retain_shared_container = false;
                // A MUTABLE binding of an ARRAY var must be an INDEPENDENT copy
                // (not a share): otherwise an in-place `push`/index-set on the
                // new binding aliases the source (value-semantics #40). Emitted
                // as kryos_array_dup below. Immutable array bindings, and str
                // bindings (strings are immutable-by-reassignment) keep the
                // cheap share-retain.
                let mut dup_array_kind: Option<i64> = None;
                // For a STRUCT/ENUM `let copy = src`, the initializer is
                // rewritten below to the deep-clone marker so the copy owns
                // independent heap-field references (the documented
                // semantics: docs/06-ownership.md "assignment deep-copies
                // heap fields"). The old model instead marked SRC consumed
                // (move) -- which broke in loops: `let hc = h` per iteration
                // re-"moved" the same value, so each iteration's scope-end
                // drop of `hc` released h's fields AGAIN -> count collapse,
                // premature free, single-threaded segfault/corrupt-header
                // (and the same collapse under `spawn`-per-iteration
                // capture, the canonical WaitGroup idiom). Use-after-move
                // was also never rejected, so the old model was UB on the
                // very next read of `src`.
                let mut aggregate_clone: Option<&'static str> = None;
                if let RValue::Use(Operand::Local(src)) = &rvalue {
                    let src_ty = ctx
                        .locals
                        .iter()
                        .find(|l| l.id == *src)
                        .map(|l| l.ty.clone())
                        .unwrap_or(MirType::I64);
                    if !is_copy_type(ctx, &src_ty) {
                        if *mutable {
                            if let MirType::Array(elem, _) = &src_ty {
                                dup_array_kind = Some(array_elem_dup_kind(elem));
                            }
                        }
                        if dup_array_kind.is_some() {
                            // handled below (independent dup)
                        } else if retain_for_ty(&src_ty).is_some() {
                            retain_shared_container = true;
                        } else {
                            match &src_ty {
                                // Only structs whose field graph the clone
                                // helpers handle COMPLETELY (scalars +
                                // str/array/map + nested such structs) take
                                // the deep-clone path. Structs with ENUM /
                                // tuple / fn-typed fields keep the old move
                                // semantics: the clone helpers leave an enum
                                // field's heap PAYLOAD shared, so cloning
                                // there gave two owners one payload -- each
                                // drop released it once (double-release; the
                                // self-host stage-1 flake, its AST structs
                                // all carry heap-payload enum fields).
                                MirType::Struct(name)
                                    if struct_cleanly_clonable(ctx, name) =>
                                {
                                    aggregate_clone =
                                        Some("__kryos_struct_index_clone");
                                }
                                _ => {
                                    // A struct/enum/tuple whose field graph the
                                    // clone helpers can't fully deep-copy (an
                                    // enum/tuple/fn-typed field). `let h2 = h1`
                                    // aliases the SAME value (non-@copy
                                    // JIT-aliases), so the OLD move semantics
                                    // (mark the SOURCE consumed, skip its drop)
                                    // only balanced a SINGLE copy: two copies
                                    // from one source (`let h2 = h1; let h3 =
                                    // h1`) left h2 AND h3 both owning the shared
                                    // value, so both dropped its heap/fn fields
                                    // -> a double-free (arc release-after-free of
                                    // the fn field / str field, exit 127). Mark
                                    // the NEW binding non-owning instead: only
                                    // the ORIGINAL source drops the shared value,
                                    // for ANY number of aliases. Safe for `let`
                                    // because the source is in scope at this
                                    // point, so it outlives the new binding's
                                    // block (the new binding can never be read
                                    // after the source is dropped).
                                    ctx.borrowed_locals.insert(local.0);
                                }
                            }
                        }
                    }
                }
                if let Some(marker) = aggregate_clone {
                    if let RValue::Use(op) = rvalue.clone() {
                        rvalue = RValue::Call {
                            func: marker.to_string(),
                            args: vec![op],
                        };
                    }
                }
                // If initializer is a call, mark non-copy args consumed.
                if let RValue::Call { ref func, ref args } = rvalue {
                    consume_call_args(ctx, local, func, args);
                }
                // Reassignment release for MUTABLE heap lets (leak fix): a
                // NOTE: a `let` binding does NOT snapshot-and-release its
                // slot. A `let mut x = ..` inside a loop reuses the same MIR
                // slot each iteration, but the previous iteration's value was
                // already freed by the block's scope-end drop -- snapshotting
                // and releasing here freed that same (dangling) pointer a
                // second time (double-free; test_csv's `let mut quoted` in a
                // loop, Linux glibc). Loop-body heap lets are the drop
                // machinery's job, not this site's.
                let param_src = if let RValue::Use(Operand::Local(src)) = &rvalue {
                    Some(*src)
                } else {
                    None
                };
                let holder_ty_for_retain = ctx
                    .locals
                    .iter()
                    .find(|l| l.id == local)
                    .map(|l| l.ty.clone());
                ctx.emit(Instruction::Assign {
                    dest: local,
                    value: rvalue,
                });
                if let Some(src) = param_src {
                    emit_param_source_retain(ctx, local, src);
                }
                if let Some(kind) = dup_array_kind {
                    // local currently shares the source array; replace it with
                    // an independent duplicate so mutations do not alias.
                    let dup_ty = holder_ty_for_retain
                        .clone()
                        .unwrap_or(MirType::Array(Box::new(MirType::I64), None));
                    let dup_tmp = ctx.alloc_temp(dup_ty);
                    ctx.emit(Instruction::Assign {
                        dest: dup_tmp,
                        value: RValue::Call {
                            func: "kryos_array_dup".to_string(),
                            args: vec![
                                Operand::Local(local),
                                Operand::Constant(Constant::Int(kind)),
                            ],
                        },
                    });
                    ctx.emit(Instruction::Assign {
                        dest: local,
                        value: RValue::Use(Operand::Local(dup_tmp)),
                    });
                } else if retain_shared_container {
                    if let Some(ref ty) = holder_ty_for_retain {
                        if let Some(rf) = retain_for_ty(ty) {
                            let sink = ctx.alloc_temp(MirType::I64);
                            ctx.emit(Instruction::Assign {
                                dest: sink,
                                value: RValue::Call {
                                    func: rf.to_string(),
                                    args: vec![Operand::Local(local)],
                                },
                            });
                        }
                    }
                }
                if is_shared {
                    ctx.emit(Instruction::ArcRetain { ptr: local });
                }
            }
        }

        ast::Stmt::Assign {
            target, op, value, ..
        } => {
            // Check if the target is an actor state field (self.field).
            let actor_field_target = if let ast::Expr::FieldAccess { object, field, .. } = target {
                if let ast::Expr::Identifier { name, .. } = object.as_ref() {
                    if name == "self" {
                        let self_local = find_local_by_name(ctx, "self")
                            .expect("internal: 'self' local not found in actor handler");
                        let actor_name = ctx
                            .locals
                            .iter()
                            .find(|l| l.id == self_local)
                            .and_then(|l| match &l.ty {
                                MirType::Struct(n) => Some(n.clone()),
                                _ => None,
                            })
                            // Actor VALUES erase to i64, so self's type is not a
                            // Struct; fall back to the actor being lowered.
                            .or_else(|| ctx.current_actor.clone());
                        if let Some(ref aname) = actor_name {
                            let fty = ctx
                                .struct_defs
                                .get(aname.as_str())
                                .and_then(|fs| {
                                    fs.iter().find(|(n, _)| n == field).map(|(_, t)| t.clone())
                                })
                                .unwrap_or(MirType::I64);
                            ctx.actor_state_fields
                                .get(aname)
                                .cloned()
                                .and_then(|fields| {
                                    fields
                                        .iter()
                                        .find(|(n, _)| n == field)
                                        .map(|(_, idx)| (self_local, *idx, fty.clone()))
                                })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else if let ast::Expr::Identifier { name, .. } = target {
                // Bare-field actor-state assignment: `count = count + 1`
                // inside a handler (the exact form docs/09-concurrency.md
                // uses). It is neither a local nor a global, so without this
                // it fell through to find_local_by_name and ICE'd
                // ("assign target local not found"). Resolve a bare name that
                // matches a field of the actor currently being lowered to the
                // same ActorStateStore path as `self.field`.
                let is_local = ctx
                    .locals
                    .iter()
                    .any(|l| l.name.as_deref() == Some(name.as_str()));
                if !is_local {
                    ctx.current_actor.clone().and_then(|aname| {
                        let fty = ctx
                            .struct_defs
                            .get(aname.as_str())
                            .and_then(|fs| {
                                fs.iter().find(|(n, _)| n == name).map(|(_, t)| t.clone())
                            })
                            .unwrap_or(MirType::I64);
                        ctx.actor_state_fields.get(&aname).cloned().and_then(|fields| {
                            fields.iter().find(|(n, _)| n == name).map(|(_, idx)| {
                                let self_local = find_local_by_name(ctx, "self")
                                    .expect("internal: 'self' local not found in actor handler");
                                (self_local, *idx, fty.clone())
                            })
                        })
                    })
                } else {
                    None
                }
            } else {
                None
            };

            if let Some((state_ptr, field_offset, field_ty)) = actor_field_target {
                // Actor state field assignment.
                match op {
                    ast::AssignOp::Assign => {
                        // ARC bookkeeping for a heap-owning field (str/array/map)
                        // being overwritten wholesale. Without this, `self.items
                        // = newitems` silently corrupted persistent actor state:
                        // ActorStateStore is a raw i64-slot store with no ARC
                        // awareness, so (a) the OLD field value was never
                        // released (a leak), and (b) if the RHS was a bare
                        // reference to a surviving named local, the actor state
                        // and that local ended up aliasing the SAME heap block
                        // with no extra retain -- the local's ordinary
                        // end-of-scope Drop then freed memory the actor's
                        // persistent state still pointed to, so the NEXT
                        // message observed freed/corrupted memory (the
                        // `reassign_fresh` -> `report` UAF). The push-chain form
                        // (`self.items = push(self.items, v)`) never hit this:
                        // its RHS is a fresh call result with no other owner,
                        // so no retain/release was needed and it "happened" to
                        // persist correctly.
                        let release_fn = release_if_ne_fn(&field_ty);
                        let old_snapshot = release_fn.map(|_| {
                            let old = ctx.alloc_temp(field_ty.clone());
                            ctx.emit(Instruction::ActorStateLoad {
                                dest: old,
                                state_ptr,
                                field_offset,
                            });
                            old
                        });
                        let rvalue = lower_expr_to_rvalue(ctx, value);
                        // Mirror the plain-local-reassignment rule: retain only
                        // when the RHS reduces to a bare reference to an
                        // existing local (so the actor state becomes a second
                        // owner alongside it); a non-container non-copy source
                        // (struct/enum) instead has its ownership transferred
                        // by marking it dropped so scope-cleanup no longer
                        // frees it. A fresh rvalue (call, literal, binop, ...)
                        // needs neither -- it has no other owner.
                        let mut retain_src: Option<&'static str> = None;
                        if let RValue::Use(Operand::Local(src)) = &rvalue {
                            let src_ty = ctx
                                .locals
                                .iter()
                                .find(|l| l.id == *src)
                                .map(|l| l.ty.clone())
                                .unwrap_or(MirType::I64);
                            if let Some(rf) = retain_for_ty(&src_ty) {
                                retain_src = Some(rf);
                            } else if !is_copy_type(ctx, &src_ty) {
                                ctx.dropped_locals.insert(src.0);
                            }
                        }
                        let val = ctx.alloc_temp(field_ty.clone());
                        ctx.emit(Instruction::Assign {
                            dest: val,
                            value: rvalue,
                        });
                        ctx.emit(Instruction::ActorStateStore {
                            state_ptr,
                            field_offset,
                            value: Operand::Local(val),
                        });
                        if let Some(rf) = retain_src {
                            let sink = ctx.alloc_temp(MirType::I64);
                            ctx.emit(Instruction::Assign {
                                dest: sink,
                                value: RValue::Call {
                                    func: rf.to_string(),
                                    args: vec![Operand::Local(val)],
                                },
                            });
                        }
                        if let (Some(rel), Some(old)) = (release_fn, old_snapshot) {
                            let sink = ctx.alloc_temp(MirType::I64);
                            ctx.emit(Instruction::Assign {
                                dest: sink,
                                value: RValue::Call {
                                    func: rel.to_string(),
                                    args: vec![Operand::Local(old), Operand::Local(val)],
                                },
                            });
                        }
                    }
                    _ => {
                        // Compound assignment (+=, -=, etc.): load current, compute, store back.
                        let mir_op = match op {
                            ast::AssignOp::AddAssign => MirBinOp::Add,
                            ast::AssignOp::SubAssign => MirBinOp::Sub,
                            ast::AssignOp::MulAssign => MirBinOp::Mul,
                            ast::AssignOp::DivAssign => MirBinOp::Div,
                            ast::AssignOp::ModAssign => MirBinOp::Mod,
                            ast::AssignOp::Assign => unreachable!(),
                        };
                        let current = ctx.alloc_temp(field_ty.clone());
                        ctx.emit(Instruction::ActorStateLoad {
                            dest: current,
                            state_ptr,
                            field_offset,
                        });
                        let rhs = lower_expr_to_operand(ctx, value);
                        let result = ctx.alloc_temp(field_ty.clone());
                        ctx.emit(Instruction::Assign {
                            dest: result,
                            value: RValue::BinOp {
                                op: mir_op,
                                left: Operand::Local(current),
                                right: rhs,
                            },
                        });
                        ctx.emit(Instruction::ActorStateStore {
                            state_ptr,
                            field_offset,
                            value: Operand::Local(result),
                        });
                    }
                }
            } else {
                match op {
                    ast::AssignOp::Assign => {
                        // For simple assignment to an identifier, find the local.
                        if let ast::Expr::Identifier { name, .. } = target {
                            // Mutable module-level global: route the write
                            // through the runtime registry instead of
                            // assigning to a (non-existent) local.
                            let is_local = ctx
                                .locals
                                .iter()
                                .any(|l| l.name.as_deref() == Some(name.as_str()));
                            if !is_local && ctx.mutable_globals.contains_key(name.as_str()) {
                                let val = lower_expr_to_operand(ctx, value);
                                emit_global_store(ctx, name, val);
                                return;
                            }
                            let dest = find_local_by_name(ctx, name)
                                .expect("internal: assign target local not found");
                            // Reassignment release (leak fix): a mutable heap
                            // local being overwritten must free its previous
                            // value -- guarded by pointer inequality at runtime
                            // so in-place mutators (push returns the same
                            // handle) stay safe. Skipped when the old value's
                            // ownership was moved out (dropped_locals) -- it is
                            // no longer ours to free.
                            let dest_ty = ctx
                                .locals
                                .iter()
                                .find(|l| l.id == dest)
                                .map(|l| l.ty.clone());
                            let release_fn = match &dest_ty {
                                Some(MirType::Str) => Some("kryos_string_release_if_ne"),
                                Some(MirType::Array(..)) => Some("kryos_array_release_if_ne"),
                                Some(MirType::Map { .. }) => Some("kryos_map_release_if_ne"),
                                _ => None,
                            };
                            // A non-copy STRUCT dest also needs its previous
                            // heap-owned fields released on reassignment (see
                            // `release_struct_heap_fields_if_ne`'s doc comment) --
                            // tracked separately from `release_fn` since it's a
                            // per-field recursive release, not a single call.
                            let dest_struct_name = match &dest_ty {
                                Some(MirType::Struct(n)) if !is_copy_type(ctx, dest_ty.as_ref().unwrap()) => {
                                    Some(n.clone())
                                }
                                _ => None,
                            };
                            let old_snapshot = match &dest_ty {
                                Some(ty) if (release_fn.is_some() || dest_struct_name.is_some())
                                    && !ctx.dropped_locals.contains(&dest.0) =>
                                {
                                    let t = ctx.alloc_temp(ty.clone());
                                    ctx.emit(Instruction::Assign {
                                        dest: t,
                                        value: RValue::Use(Operand::Local(dest)),
                                    });
                                    Some(t)
                                }
                                _ => None,
                            };
                            let mut rvalue = lower_expr_to_rvalue(ctx, value);
                            // dyn Trait coercion on REASSIGNMENT: `d = Square {
                            // .. }` where `d: dyn Shape` must wrap the concrete
                            // struct in a trait fat-pointer (MakeTraitObject),
                            // exactly like the Let-initializer / return / call-
                            // argument / struct-field-init paths already do.
                            // Without it a RAW struct pointer was stored into the
                            // dyn slot, and the next `d.method()` vtable call read
                            // offset 0/8 of raw struct memory as {data_ptr,
                            // fn_ptr} and jumped through garbage -> SIGSEGV.
                            if let Some(MirType::DynTrait(trait_name)) = dest_ty.clone() {
                                if let MirType::Struct(concrete_type) =
                                    infer_expr_type(ctx, value)
                                {
                                    let struct_tmp =
                                        ctx.alloc_temp(MirType::Struct(concrete_type.clone()));
                                    ctx.emit(Instruction::Assign {
                                        dest: struct_tmp,
                                        value: rvalue,
                                    });
                                    rvalue = RValue::MakeTraitObject {
                                        value: Operand::Local(struct_tmp),
                                        concrete_type,
                                        trait_name,
                                    };
                                }
                            }
                            // Exception-safety: `dest = fails()` where `fails`
                            // THROWS must leave `dest` holding its OLD value.
                            // `Assign { dest, Call }` is a single instruction
                            // whose store executes as part of the call, and the
                            // post-call exception check (emit()) only fires
                            // AFTER it -- so a throw left `dest` clobbered with
                            // the null sentinel (old heap value lost, then a
                            // null-deref on the next read; found by the runtime
                            // exception-safety fuzz). Only matters inside a
                            // try/catch (an uncaught throw exits anyway). Fix:
                            // materialize the throwable call into a fresh temp
                            // first (its exception check fires after `tmp =
                            // Call`, diverting to the catch with `dest`
                            // untouched), then MOVE the temp into `dest`
                            // (release the old value, no container retain --
                            // the temp is an owned fresh result, not a shared
                            // local -- and mark the temp consumed).
                            if ctx.try_catch_target.is_some()
                                && matches!(
                                    rvalue,
                                    RValue::Call { .. } | RValue::CallIndirect { .. }
                                )
                            {
                                match &rvalue {
                                    RValue::Call { func, args } => {
                                        consume_call_args(ctx, dest, func, args)
                                    }
                                    RValue::CallIndirect { args, .. } => {
                                        consume_call_args(ctx, dest, "", args)
                                    }
                                    _ => {}
                                }
                                let ty = dest_ty.clone().unwrap_or(MirType::I64);
                                let tmp = ctx.alloc_temp(ty);
                                ctx.emit(Instruction::Assign {
                                    dest: tmp,
                                    value: rvalue,
                                });
                                // Past the exception check now -- safe to touch
                                // dest. Release the old value (guarded), then
                                // move the temp in.
                                if let (Some(f), Some(old)) = (release_fn, old_snapshot) {
                                    drop_tag(ctx, "reassign-release-exc-safe");
                                    let sink = ctx.alloc_temp(MirType::I64);
                                    ctx.emit(Instruction::Assign {
                                        dest: sink,
                                        value: RValue::Call {
                                            func: f.to_string(),
                                            args: vec![
                                                Operand::Local(old),
                                                Operand::Local(tmp),
                                            ],
                                        },
                                    });
                                } else if let (Some(struct_name), Some(old)) =
                                    (&dest_struct_name, old_snapshot)
                                {
                                    drop_tag(ctx, "reassign-release-struct-exc-safe");
                                    release_struct_heap_fields_if_ne(
                                        ctx,
                                        old,
                                        tmp,
                                        struct_name,
                                    );
                                }
                                ctx.emit(Instruction::Assign {
                                    dest,
                                    value: RValue::Use(Operand::Local(tmp)),
                                });
                                ctx.dropped_locals.insert(tmp.0);
                                ctx.dropped_locals.remove(&dest.0);
                                return;
                            }
                            // Reassign from a bare local: containers SHARE
                            // (retain), mirroring the Let-binding rule. The
                            // old consume-the-source model broke when the
                            // dest lived in an INNER scope: `{ s17 = c15 }`
                            // freed c15's buffer at s17's scope end while
                            // c15 was still read outside (JIT recycled-buffer
                            // garbage; found by the throw/reassign fuzz
                            // class). Non-container non-copy values (structs,
                            // enums) keep the move.
                            let mut retain_container_src: Option<&'static str> = None;
                            if let RValue::Use(Operand::Local(src)) = &rvalue {
                                let src_ty = ctx
                                    .locals
                                    .iter()
                                    .find(|l| l.id == *src)
                                    .map(|l| l.ty.clone())
                                    .unwrap_or(MirType::I64);
                                if let Some(rf) = retain_for_ty(&src_ty) {
                                    retain_container_src = Some(rf);
                                } else if !is_copy_type(ctx, &src_ty) {
                                    ctx.dropped_locals.insert(src.0);
                                }
                            }
                            // If RHS is a direct call (e.g. `pp = push_str(pp, op_local)`),
                            // mark non-copy local args as consumed so scope cleanup
                            // doesn't double-free strings/enums the callee took ownership of.
                            // The dest local itself is skipped by consume_call_args even when
                            // it appears as an arg (self-consuming pattern).
                            if let RValue::Call { ref func, ref args } = rvalue {
                                consume_call_args(ctx, dest, func, args);
                            } else if let RValue::CallIndirect { ref args, .. } = rvalue {
                                consume_call_args(ctx, dest, "", args);
                            }
                            let param_src = if let RValue::Use(Operand::Local(src)) = &rvalue {
                                Some(*src)
                            } else {
                                None
                            };
                            ctx.emit(Instruction::Assign {
                                dest,
                                value: rvalue,
                            });
                            if let Some(rf) = retain_container_src {
                                // Container share: one retain covers every
                                // source (param or plain local) — skip the
                                // param-only retain below to avoid doubling.
                                let sink = ctx.alloc_temp(MirType::I64);
                                ctx.emit(Instruction::Assign {
                                    dest: sink,
                                    value: RValue::Call {
                                        func: rf.to_string(),
                                        args: vec![Operand::Local(dest)],
                                    },
                                });
                            } else if let Some(src) = param_src {
                                emit_param_source_retain(ctx, dest, src);
                            }
                            // Emit the guarded release AFTER the store: the RHS
                            // (which may read the old value, e.g. s = s + "x")
                            // has fully evaluated by now.
                            if let (Some(f), Some(old)) = (release_fn, old_snapshot) {
                                drop_tag(ctx, "reassign-release");
                                let sink = ctx.alloc_temp(MirType::I64);
                                ctx.emit(Instruction::Assign {
                                    dest: sink,
                                    value: RValue::Call {
                                        func: f.to_string(),
                                        args: vec![
                                            Operand::Local(old),
                                            Operand::Local(dest),
                                        ],
                                    },
                                });
                            } else if let (Some(struct_name), Some(old)) =
                                (&dest_struct_name, old_snapshot)
                            {
                                drop_tag(ctx, "reassign-release-struct");
                                release_struct_heap_fields_if_ne(ctx, old, dest, struct_name);
                            }
                            // The local now holds a fresh owned value.
                            ctx.dropped_locals.remove(&dest.0);
                            // Box-capture sync: if a struct-literal-field
                            // closure ref-captured this variable (see
                            // `capture_boxes` / `pending_box_scalar_captures`
                            // in the Lambda RValue arm), also write the new
                            // value through every box on file for this name
                            // so a LATER call to that closure observes this
                            // mutation -- gotcha #11's "sees a mutation made
                            // after construction" promise, which was
                            // otherwise only honored for the `closure_locals`
                            // direct-call fast path (n01_closure_field_vs_local
                            // -class bugs).
                            if let Some(boxes) = ctx.capture_boxes.get(name.as_str()).cloned() {
                                for box_id in boxes {
                                    ctx.emit(Instruction::StoreDeref {
                                        ptr: Operand::Local(box_id),
                                        value: Operand::Local(dest),
                                    });
                                }
                            }
                        } else if let ast::Expr::IndexAccess { object, index, .. } = target {
                            // Map/array index assignment: m["key"] = value → kryos_map_insert_str(m, key, value)
                            let obj_ty = infer_expr_type(ctx, object);
                            let map_op = lower_expr_to_operand(ctx, object);
                            let key_op = lower_expr_to_operand(ctx, index);
                            let val_op = lower_expr_to_operand(ctx, value);
                            // The container now holds a second reference to a
                            // container VALUE (the runtime stores the raw
                            // pointer; it cannot retain because only the
                            // compiler knows whether the slot is a scalar or a
                            // pointer). Without a retain, `let v = f(); m[k] = v`
                            // dangles when v's local drops — the map read
                            // returned recycled buffers. Keys are already safe
                            // (insert_str deep-copies the key).
                            let val_ty = infer_expr_type(ctx, value);
                            let val_retain = retain_for_ty(&val_ty);
                            if matches!(obj_ty, MirType::Map { .. }) {
                                let idx_ty = infer_expr_type(ctx, index);
                                let insert_fn = if idx_ty == MirType::Str {
                                    "kryos_map_insert_str"
                                } else {
                                    "kryos_map_insert"
                                };
                                let temp = ctx.alloc_temp(MirType::I64);
                                ctx.emit(Instruction::Assign {
                                    dest: temp,
                                    value: RValue::Call {
                                        func: insert_fn.to_string(),
                                        args: vec![map_op, key_op, val_op.clone()],
                                    },
                                });
                                if let (Some(rf), Operand::Local(_)) = (val_retain, &val_op) {
                                    let sink = ctx.alloc_temp(MirType::I64);
                                    ctx.emit(Instruction::Assign {
                                        dest: sink,
                                        value: RValue::Call {
                                            func: rf.to_string(),
                                            args: vec![val_op],
                                        },
                                    });
                                }
                            } else {
                                // Array index assignment.
                                let temp = ctx.alloc_temp(MirType::I64);
                                ctx.emit(Instruction::Assign {
                                    dest: temp,
                                    value: RValue::Call {
                                        func: "kryos_array_set".to_string(),
                                        args: vec![map_op, key_op, val_op.clone()],
                                    },
                                });
                                if let (Some(rf), Operand::Local(_)) = (val_retain, &val_op) {
                                    let sink = ctx.alloc_temp(MirType::I64);
                                    ctx.emit(Instruction::Assign {
                                        dest: sink,
                                        value: RValue::Call {
                                            func: rf.to_string(),
                                            args: vec![val_op],
                                        },
                                    });
                                }
                            }
                        } else if let ast::Expr::Deref { inner, .. } = target {
                            // Deref assignment: *ptr = value → store through pointer.
                            let ptr_op = lower_expr_to_operand(ctx, inner);
                            let val_op = lower_expr_to_operand(ctx, value);
                            ctx.emit(Instruction::StoreDeref {
                                ptr: ptr_op,
                                value: val_op,
                            });
                        } else if let ast::Expr::FieldAccess { object, field, .. } = target {
                            // Field assignment. Nested paths (o.a.v = 99) need
                            // read-modify-writeback — a plain StoreField on the
                            // lowered object would mutate an immutable temp COPY
                            // of the inner struct (JIT only worked by pointer
                            // aliasing; AOT emitted invalid `inttoptr %Agg`).
                            let val_op = lower_expr_to_operand(ctx, value);
                            // dyn Trait coercion when the FIELD is dyn-typed
                            // (`h.shape = Circle { .. }`): wrap the concrete
                            // struct in a trait fat-pointer, or the later
                            // `h.shape.method()` vtable-dispatches through raw
                            // struct memory -> SIGSEGV (same class as the local-
                            // reassignment case above; struct-literal FIELD INIT
                            // already coerces, reassignment did not).
                            let field_ty = infer_expr_type(ctx, target);
                            let val_op = coerce_to_dyn_if_needed(ctx, val_op, &field_ty, value);
                            lower_nested_field_assign(ctx, object, field, val_op);
                        } else {
                            // Fallback: evaluate RHS into a temp (may have side effects).
                            let temp = ctx.alloc_temp(MirType::I64);
                            let rvalue = lower_expr_to_rvalue(ctx, value);
                            ctx.emit(Instruction::Assign {
                                dest: temp,
                                value: rvalue,
                            });
                        }
                    }
                    _ => {
                        // Compound assignment (+=, -=, etc.) — desugar to bin-op + assign.
                        if let ast::Expr::Identifier { name, .. } = target {
                            // Mutable module-level global: load, op, store.
                            let is_local = ctx
                                .locals
                                .iter()
                                .any(|l| l.name.as_deref() == Some(name.as_str()));
                            if !is_local {
                                if let Some((mir_ty, _)) =
                                    ctx.mutable_globals.get(name.as_str()).cloned()
                                {
                                    let mir_op = match op {
                                        ast::AssignOp::AddAssign => MirBinOp::Add,
                                        ast::AssignOp::SubAssign => MirBinOp::Sub,
                                        ast::AssignOp::MulAssign => MirBinOp::Mul,
                                        ast::AssignOp::DivAssign => MirBinOp::Div,
                            ast::AssignOp::ModAssign => MirBinOp::Mod,
                                        ast::AssignOp::Assign => unreachable!(),
                                    };
                                    let cur = emit_global_load(ctx, name, mir_ty.clone());
                                    let rhs = lower_expr_to_operand(ctx, value);
                                    let new_val = ctx.alloc_temp(mir_ty);
                                    ctx.emit(Instruction::Assign {
                                        dest: new_val,
                                        value: RValue::BinOp {
                                            op: mir_op,
                                            left: Operand::Local(cur),
                                            right: rhs,
                                        },
                                    });
                                    emit_global_store(ctx, name, Operand::Local(new_val));
                                    return;
                                }
                            }
                            let dest = find_local_by_name(ctx, name)
                                .expect("internal: compound assign target local not found");
                            let mir_op = match op {
                                ast::AssignOp::AddAssign => MirBinOp::Add,
                                ast::AssignOp::SubAssign => MirBinOp::Sub,
                                ast::AssignOp::MulAssign => MirBinOp::Mul,
                                ast::AssignOp::DivAssign => MirBinOp::Div,
                            ast::AssignOp::ModAssign => MirBinOp::Mod,
                                ast::AssignOp::Assign => unreachable!(),
                            };
                            let rhs = lower_expr_to_operand(ctx, value);

                            // Array += : desugar to kryos_array_concat call.
                            if *op == ast::AssignOp::AddAssign {
                                let dest_ty = ctx
                                    .locals
                                    .iter()
                                    .find(|l| l.id == dest)
                                    .map(|l| l.ty.clone());
                                let rhs_ty = infer_expr_type(ctx, value);
                                if matches!(
                                    (&dest_ty, &rhs_ty),
                                    (Some(MirType::Array(_, _)), MirType::Array(_, _))
                                ) {
                                    ctx.emit(Instruction::Assign {
                                        dest,
                                        value: RValue::Call {
                                            func: "kryos_array_concat".to_string(),
                                            args: vec![Operand::Local(dest), rhs],
                                        },
                                    });
                                    return;
                                }
                            }

                            ctx.emit(Instruction::Assign {
                                dest,
                                value: RValue::BinOp {
                                    op: mir_op,
                                    left: Operand::Local(dest),
                                    right: rhs,
                                },
                            });
                        } else {
                            // Compound assignment to a NON-identifier lvalue
                            // (`a.field += n`, `arr[i] += n`, `*p += n`): the arm
                            // above matched ONLY Identifier targets, so a field /
                            // index / deref compound-assign fell through and did
                            // NOTHING -- a silent no-op on both backends. Desugar
                            // `target OP= value` into `target = target OP value`
                            // and re-lower as a plain assign, which already
                            // handles FieldAccess (lower_nested_field_assign),
                            // Index (kryos_array_set / map_set), and Deref with
                            // full ARC bookkeeping. (A side-effecting index
                            // sub-expression is evaluated twice, matching the
                            // rarity vs. the previous "does nothing" behavior.)
                            let bin_op = match op {
                                ast::AssignOp::AddAssign => ast::BinOp::Add,
                                ast::AssignOp::SubAssign => ast::BinOp::Sub,
                                ast::AssignOp::MulAssign => ast::BinOp::Mul,
                                ast::AssignOp::DivAssign => ast::BinOp::Div,
                                ast::AssignOp::ModAssign => ast::BinOp::Mod,
                                ast::AssignOp::Assign => unreachable!(),
                            };
                            let sp = target.span();
                            let new_value = ast::Expr::BinaryOp {
                                op: bin_op,
                                left: Box::new(target.clone()),
                                right: Box::new(value.clone()),
                                span: sp,
                            };
                            let synthetic = ast::Stmt::Assign {
                                target: target.clone(),
                                op: ast::AssignOp::Assign,
                                value: new_value,
                                span: sp,
                            };
                            lower_stmt_inner(ctx, &synthetic);
                        }
                    }
                }
            }
        }

        ast::Stmt::Return { value, .. } => {
            // dyn Trait coercion at the return boundary: `fn make() -> dyn
            // Shape { return Rect{..} }`. The enclosing function's declared
            // return type (`current_ret_ty`, set once per `lower_function`
            // call) is the coercion target; a concrete-struct return value
            // is wrapped in MakeTraitObject exactly like a call argument.
            // Without this the caller received a raw struct through a
            // `dyn`-typed return slot and the later vtable_call segfaulted.
            let ret_ty = ctx.current_ret_ty.clone();
            let mut operand = value.as_ref().map(|e| {
                let op = lower_expr_to_operand(ctx, e);
                coerce_to_dyn_if_needed(ctx, op, &ret_ty, e)
            });
            // Returning a param directly hands the caller a handle it will
            // treat as owned; retain so the caller's release does not free
            // its own argument (borrow-to-own at the return boundary).
            if let Some(Operand::Local(src)) = &operand {
                emit_param_source_retain(ctx, *src, *src);
            }
            // Returning a Struct/Enum VALUE READ OUT of a container the
            // callee does not own outright -- an array-element read
            // (`return rows[0]`) or a struct-field read (`return s.field`),
            // including through a borrowed bare-ident bound one `let` back
            // -- is a bit-copy ALIAS, not a fresh value (the exact shapes
            // `__kryos_struct_index_clone`/`__kryos_enum_index_clone`
            // already cover for the `push(dest, arr[i])` aliasing case).
            // The CALLER treats every return value as freshly owned and
            // gives it its own scope-end Drop; without an independent copy
            // here, that drop races the source container's own per-element
            // Drop over the SAME heap block. Confirmed via KRYOS_DUMP_IR /
            // MIR diff: `fn first(rows: [Sale]) -> Sale { return rows[0] }`
            // called twice on the SAME array handle produces two locals
            // that both alias `rows[0]` and both get an unconditional
            // `drop()` at the caller's scope end -- double-freeing the
            // shared Sale (exit 127/139 at teardown, non-deterministic --
            // the classic heap-corruption signature, not a panic).
            if let (Some(e), Some(Operand::Local(src))) = (value.as_ref(), &operand) {
                let is_direct_alias_read = matches!(
                    e,
                    ast::Expr::IndexAccess { .. } | ast::Expr::FieldAccess { .. }
                );
                let is_borrowed_bare_ident = if let ast::Expr::Identifier { name: id_name, .. } = e
                {
                    find_local_by_name(ctx, id_name)
                        .is_some_and(|l| ctx.borrowed_locals.contains(&l.0))
                } else {
                    false
                };
                if is_direct_alias_read || is_borrowed_bare_ident {
                    match infer_expr_type(ctx, e) {
                        MirType::Struct(ref sname) if ctx.struct_defs.contains_key(sname) => {
                            let cloned = ctx.alloc_temp(MirType::Struct(sname.clone()));
                            ctx.emit(Instruction::Assign {
                                dest: cloned,
                                value: RValue::Call {
                                    func: "__kryos_struct_index_clone".to_string(),
                                    args: vec![Operand::Local(*src)],
                                },
                            });
                            operand = Some(Operand::Local(cloned));
                        }
                        MirType::Enum(ref ename) if ctx.enum_defs.contains_key(ename) => {
                            let cloned = ctx.alloc_temp(MirType::Enum(ename.clone()));
                            ctx.emit(Instruction::Assign {
                                dest: cloned,
                                value: RValue::Call {
                                    func: "__kryos_enum_index_clone".to_string(),
                                    args: vec![Operand::Local(*src)],
                                },
                            });
                            operand = Some(Operand::Local(cloned));
                        }
                        _ => {}
                    }
                }
            }
            let next = ctx.alloc_block();
            ctx.finish_block(Terminator::Return(operand), next);
        }

        ast::Stmt::If {
            condition,
            then_block,
            elif_clauses,
            else_block,
            ..
        } => {
            lower_if(ctx, condition, then_block, elif_clauses, else_block);
        }

        ast::Stmt::While {
            condition, body, ..
        } => {
            lower_while(ctx, condition, body);
        }

        ast::Stmt::For {
            parallel,
            pattern,
            iterable,
            body,
            ..
        } => {
            if *parallel {
                lower_parallel_for(ctx, pattern, iterable, body);
            } else {
                lower_for(ctx, pattern, iterable, body);
            }
        }

        ast::Stmt::Break { .. } => {
            if let Some(&exit) = ctx.loop_exits.last() {
                let next = ctx.alloc_block();
                ctx.finish_block(Terminator::Goto(exit), next);
            }
        }

        ast::Stmt::Continue { .. } => {
            if let Some(&header) = ctx.loop_headers.last() {
                let next = ctx.alloc_block();
                ctx.finish_block(Terminator::Goto(header), next);
            }
        }

        ast::Stmt::Expr { expr, .. } => {
            // Lower the expression for its side effects.
            let rvalue = lower_expr_to_rvalue(ctx, expr);

            // Function calls need to be emitted as instructions even when
            // their return value is discarded, so assign to a temp.
            // This applies to both direct calls (RValue::Call) and indirect
            // calls through function pointers / closure values
            // (RValue::CallIndirect) — without the indirect arm, statements
            // like `g()` where g is a void-returning closure local would be
            // silently dropped during stmt lowering.
            match &rvalue {
                RValue::Call { func, args } => {
                    let temp = ctx.alloc_temp(MirType::Void);
                    let args_clone = args.clone();
                    let func_clone = func.clone();
                    consume_call_args(ctx, temp, &func_clone, &args_clone);
                    ctx.emit(Instruction::Assign {
                        dest: temp,
                        value: rvalue,
                    });
                }
                RValue::CallIndirect { args, .. } => {
                    let temp = ctx.alloc_temp(MirType::Void);
                    let args_clone = args.clone();
                    consume_call_args(ctx, temp, "", &args_clone);
                    ctx.emit(Instruction::Assign {
                        dest: temp,
                        value: rvalue,
                    });
                }
                _ => {}
            }

            // If the expression is a match, it was already lowered via
            // `lower_match` inside `lower_expr_to_rvalue`.
        }

        ast::Stmt::TryCatch {
            try_block,
            catch_name,
            catch_block,
            ..
        } => {
            lower_try_catch(ctx, try_block, catch_name, catch_block, None);
        }

        ast::Stmt::Throw { expr, .. } => {
            // The catch binding is statically typed `str` (check.rs), so the
            // thrown value is converted to its string representation at the
            // throw site. A single-operand StringConcat reuses the backends'
            // type-aware to_string dispatch (the same path as `"{x}"`).
            let throw_ty = infer_expr_type(ctx, expr);
            let raw = lower_expr_to_operand(ctx, expr);
            let val = if throw_ty == MirType::Str {
                raw
            } else {
                let s = ctx.alloc_temp(MirType::Str);
                ctx.emit(Instruction::Assign {
                    dest: s,
                    value: RValue::StringConcat(vec![raw]),
                });
                Operand::Local(s)
            };
            // The thrown string enters the exception slot / Err aggregate as
            // a second reference: retain it, or the throwing scope's unwind
            // drop frees the buffer the catch binding still points at
            // (double-free of "thrown-msg" under KRYOS_FREE_DIAG). The
            // retain only executes when the throw executes, so the
            // non-throwing path's refcounts are untouched.
            if let Operand::Local(_) = &val {
                let sink = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: sink,
                    value: RValue::Call {
                        func: "kryos_string_retain_opt".to_string(),
                        args: vec![val.clone()],
                    },
                });
            }
            if let Some(ref target) = ctx.try_catch_target {
                // Inside a try block: store Result::Err into the try/catch
                // result local and jump to the tag-check block.
                let result_local = target.result_local;
                let check_block = target.check_block;
                ctx.emit(Instruction::Assign {
                    dest: result_local,
                    value: RValue::EnumVariant {
                        enum_name: "Result".into(),
                        variant_idx: 1, // Err
                        fields: vec![val],
                    },
                });
                // Allocate a dead block to receive the remaining unreachable stmts.
                let dead_bb = ctx.alloc_block();
                ctx.finish_block(Terminator::Goto(check_block), dead_bb);
            } else {
                // Outside try: store the exception in the thread-local via
                // kryos_exception_throw and return from this function so the
                // caller can detect the pending exception and unwind.
                let throw_result = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: throw_result,
                    value: RValue::Call {
                        func: "kryos_exception_throw".into(),
                        args: vec![val],
                    },
                });
                // Return immediately to unwind toward the nearest try/catch.
                // Use the right return form based on the function's return type.
                let ret_operand = if ctx.current_ret_ty == MirType::Void {
                    None
                } else if ctx.current_ret_ty == MirType::F64 || ctx.current_ret_ty == MirType::F32 {
                    Some(Operand::Constant(Constant::Float(0.0)))
                } else {
                    Some(Operand::Constant(Constant::Int(0)))
                };
                let dead_bb = ctx.alloc_block();
                ctx.finish_block(Terminator::Return(ret_operand), dead_bb);
            }
        }

        ast::Stmt::Spawn { expr, .. } => {
            lower_spawn(ctx, expr);
        }

        ast::Stmt::DenyBlock { body, .. } => {
            // `deny!(...)` is a compile-time capability-narrowing wrapper; at
            // runtime it executes its body verbatim.
            lower_block_stmts(ctx, &body.stmts);
        }

        ast::Stmt::Select {
            branches, timeout, ..
        } => {
            // Lower select: sequential try_recv polling loop with closed-channel
            // detection. Each channel is probed non-blocking; first ready wins.
            // If none ready and not all closed, sleep 1ms and retry.
            // If all channels are closed, exit the select.
            // If timeout is present, exit to the timeout body after the deadline.
            let merge_bb = ctx.alloc_block();

            if branches.is_empty() {
                ctx.emit(Instruction::Nop);
                return;
            }

            let num_branches = branches.len() as i64;

            // Allocate blocks: poll, try/got per branch, check-closed, sleep.
            let bb_poll = ctx.alloc_block();
            let bb_check_closed = ctx.alloc_block();
            let bb_sleep = ctx.alloc_block();

            let mut try_bbs = Vec::new();
            let mut got_bbs = Vec::new();
            for _ in branches.iter() {
                try_bbs.push(ctx.alloc_block());
                got_bbs.push(ctx.alloc_block());
            }

            // Evaluate channel expressions ONCE before the poll loop.
            let mut ch_locals = Vec::new();
            for branch in branches.iter() {
                let ch_ty = infer_expr_type(ctx, &branch.channel);
                let ch_op = lower_expr_to_operand(ctx, &branch.channel);
                // A branch channel may be a std::chan::Channel STRUCT
                // ({raw, capacity, is_closed}) rather than a raw i64 handle.
                // `select` polls raw handles, so extract `.raw` -- otherwise the
                // aggregate value was reinterpreted as a pointer and the runtime
                // dereferenced garbage (segfault on JIT, hang on AOT). A raw i64
                // operand (from the low-level `chan()` builtin) is used as-is.
                let raw_op = match &ch_ty {
                    MirType::Struct(sname)
                        if ctx
                            .struct_defs
                            .get(sname)
                            .is_some_and(|fs| fs.iter().any(|(n, _)| n == "raw")) =>
                    {
                        let raw = ctx.alloc_temp(MirType::I64);
                        ctx.emit(Instruction::Assign {
                            dest: raw,
                            value: RValue::Field {
                                object: ch_op,
                                field: "raw".to_string(),
                            },
                        });
                        Operand::Local(raw)
                    }
                    _ => ch_op,
                };
                let ch_local = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: ch_local,
                    value: RValue::Use(raw_op),
                });
                ch_locals.push(ch_local);
            }

            // If there's a timeout, record start time and deadline before the loop.
            let timeout_deadline_local = timeout.as_ref().map(|t| {
                let start_local = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: start_local,
                    value: RValue::Call {
                        func: "kryos_time_now_millis".into(),
                        args: vec![],
                    },
                });
                let duration_op = lower_expr_to_operand(ctx, &t.duration_ms);
                let deadline_local = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: deadline_local,
                    value: RValue::BinOp {
                        op: MirBinOp::Add,
                        left: Operand::Local(start_local),
                        right: duration_op,
                    },
                });
                deadline_local
            });

            // Jump into the poll block.
            ctx.finish_block(Terminator::Goto(bb_poll), bb_poll);

            // === bb_poll: jump straight to first try block ===
            ctx.finish_block(Terminator::Goto(try_bbs[0]), try_bbs[0]);

            // === try blocks: call try_recv_status, branch on result ===
            for (i, branch) in branches.iter().enumerate() {
                // Call try_recv_status — returns 1 (data), 0 (empty), -1 (closed).
                let status_local = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: status_local,
                    value: RValue::Call {
                        func: "kryos_chan_try_recv_status_i64".into(),
                        args: vec![Operand::Local(ch_locals[i])],
                    },
                });

                // Check: status == 1 (got data)?
                let has_data = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: has_data,
                    value: RValue::BinOp {
                        op: MirBinOp::Eq,
                        left: Operand::Local(status_local),
                        right: Operand::Constant(Constant::Int(1)),
                    },
                });

                let else_bb = if i + 1 < branches.len() {
                    try_bbs[i + 1]
                } else {
                    bb_check_closed
                };
                ctx.finish_block(
                    Terminator::Branch {
                        cond: Operand::Local(has_data),
                        then_block: got_bbs[i],
                        else_block: else_bb,
                    },
                    got_bbs[i],
                );

                // === got block: retrieve value, assign to pattern, run body ===
                let recv_value = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: recv_value,
                    value: RValue::Call {
                        func: "kryos_chan_last_recv_i64".into(),
                        args: vec![],
                    },
                });

                let pattern_local =
                    ctx.alloc_local(Some(branch.pattern.clone()), MirType::I64, false);
                ctx.emit(Instruction::Assign {
                    dest: pattern_local,
                    value: RValue::Use(Operand::Local(recv_value)),
                });

                lower_block_stmts(ctx, &branch.body.stmts);

                // After body, jump to merge.
                let next_start = if i + 1 < branches.len() {
                    try_bbs[i + 1]
                } else {
                    bb_check_closed
                };
                ctx.finish_block(Terminator::Goto(merge_bb), next_start);
            }

            // === bb_check_closed: if all channels closed, exit select ===
            // Sum up is_closed for each channel; if sum == num_branches, all
            // are closed and the select should exit to merge_bb.
            let mut closed_sum = {
                let first = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: first,
                    value: RValue::Call {
                        func: "kryos_chan_is_closed_i64".into(),
                        args: vec![Operand::Local(ch_locals[0])],
                    },
                });
                first
            };
            for ch_local in ch_locals.iter().skip(1) {
                let c = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: c,
                    value: RValue::Call {
                        func: "kryos_chan_is_closed_i64".into(),
                        args: vec![Operand::Local(*ch_local)],
                    },
                });
                let new_sum = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: new_sum,
                    value: RValue::BinOp {
                        op: MirBinOp::Add,
                        left: Operand::Local(closed_sum),
                        right: Operand::Local(c),
                    },
                });
                closed_sum = new_sum;
            }

            let all_closed = ctx.alloc_temp(MirType::I64);
            ctx.emit(Instruction::Assign {
                dest: all_closed,
                value: RValue::BinOp {
                    op: MirBinOp::Eq,
                    left: Operand::Local(closed_sum),
                    right: Operand::Constant(Constant::Int(num_branches)),
                },
            });
            ctx.finish_block(
                Terminator::Branch {
                    cond: Operand::Local(all_closed),
                    then_block: merge_bb,
                    else_block: bb_sleep,
                },
                bb_sleep,
            );

            // === bb_sleep: yield 1ms, then check timeout, then retry ===
            const SELECT_POLL_INTERVAL_BITS: i64 = 0.001_f64.to_bits() as i64;
            let sleep_result = ctx.alloc_temp(MirType::I64);
            ctx.emit(Instruction::Assign {
                dest: sleep_result,
                value: RValue::Call {
                    func: "kryos_sleep".into(),
                    args: vec![Operand::Constant(Constant::Int(SELECT_POLL_INTERVAL_BITS))],
                },
            });

            if let (Some(deadline_local), Some(t)) = (timeout_deadline_local, timeout.as_ref()) {
                // Check if now >= deadline.
                let now_local = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: now_local,
                    value: RValue::Call {
                        func: "kryos_time_now_millis".into(),
                        args: vec![],
                    },
                });
                let timed_out = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: timed_out,
                    value: RValue::BinOp {
                        op: MirBinOp::GtEq,
                        left: Operand::Local(now_local),
                        right: Operand::Local(deadline_local),
                    },
                });
                let bb_timeout_body = ctx.alloc_block();
                ctx.finish_block(
                    Terminator::Branch {
                        cond: Operand::Local(timed_out),
                        then_block: bb_timeout_body,
                        else_block: bb_poll,
                    },
                    bb_timeout_body,
                );

                // === bb_timeout_body: run the timeout branch body, then merge ===
                lower_block_stmts(ctx, &t.body.stmts);
                ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
            } else {
                ctx.finish_block(Terminator::Goto(bb_poll), merge_bb);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Control flow lowering
// ---------------------------------------------------------------------------

fn lower_if(
    ctx: &mut LoweringContext,
    condition: &ast::Expr,
    then_block: &ast::Block,
    elif_clauses: &[(ast::Expr, ast::Block)],
    else_block: &Option<ast::Block>,
) {
    let cond_op = lower_expr_to_operand(ctx, condition);
    let then_bb = ctx.alloc_block();
    let else_bb = ctx.alloc_block();
    let merge_bb = ctx.alloc_block();

    ctx.finish_block(
        Terminator::Branch {
            cond: cond_op,
            then_block: then_bb,
            else_block: else_bb,
        },
        then_bb,
    );

    // Then block.
    lower_block_stmts(ctx, &then_block.stmts);
    ctx.finish_block(Terminator::Goto(merge_bb), else_bb);

    // Else / elif chain.
    if !elif_clauses.is_empty() {
        // First elif becomes the else-block, further elifs chain.
        for (i, (elif_cond, elif_body)) in elif_clauses.iter().enumerate() {
            let elif_cond_op = lower_expr_to_operand(ctx, elif_cond);
            let elif_then_bb = ctx.alloc_block();
            let elif_else_bb = if i + 1 < elif_clauses.len() || else_block.is_some() {
                ctx.alloc_block()
            } else {
                merge_bb
            };

            ctx.finish_block(
                Terminator::Branch {
                    cond: elif_cond_op,
                    then_block: elif_then_bb,
                    else_block: elif_else_bb,
                },
                elif_then_bb,
            );

            lower_block_stmts(ctx, &elif_body.stmts);
            ctx.finish_block(Terminator::Goto(merge_bb), elif_else_bb);
        }

        // Final else if present.
        if let Some(else_blk) = else_block {
            lower_block_stmts(ctx, &else_blk.stmts);
            ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
        } else if ctx.current_block != merge_bb {
            // When the last elif has no else clause, its else-branch target
            // is already merge_bb, so current_block == merge_bb and we must
            // NOT emit another block — that would create a duplicate block
            // ID and a self-loop.
            ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
        }
    } else if let Some(else_blk) = else_block {
        lower_block_stmts(ctx, &else_blk.stmts);
        ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
    } else {
        // No else branch — else block jumps straight to merge.
        ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
    }
}

fn lower_while(ctx: &mut LoweringContext, condition: &ast::Expr, body: &ast::Block) {
    let header_bb = ctx.alloc_block();
    let body_bb = ctx.alloc_block();
    let exit_bb = ctx.alloc_block();

    // Jump to loop header.
    ctx.finish_block(Terminator::Goto(header_bb), header_bb);

    // Header: evaluate condition, branch.
    let cond_op = lower_expr_to_operand(ctx, condition);
    ctx.finish_block(
        Terminator::Branch {
            cond: cond_op,
            then_block: body_bb,
            else_block: exit_bb,
        },
        body_bb,
    );

    // Body.
    ctx.loop_headers.push(header_bb);
    ctx.loop_exits.push(exit_bb);
    lower_block_stmts(ctx, &body.stmts);
    ctx.loop_headers.pop();
    ctx.loop_exits.pop();

    // Back-edge: jump to header.
    ctx.finish_block(Terminator::Goto(header_bb), exit_bb);
}

fn lower_for(
    ctx: &mut LoweringContext,
    pattern: &ast::Pattern,
    iterable: &ast::Expr,
    body: &ast::Block,
) {
    // Check if the iterable is a `range(start, end)` call — if so, emit a
    // simple counter loop instead of the general len-based iteration.
    if let ast::Expr::FnCall { callee, args, .. } = iterable {
        if let ast::Expr::Identifier { name, .. } = callee.as_ref() {
            if name == "range" && args.len() == 2 {
                lower_for_range(ctx, pattern, &args[0], &args[1], body, false);
                return;
            }
        }
    }

    // Check if the iterable is a range expression (start..end or start..=end).
    if let ast::Expr::RangeExpr {
        start,
        end,
        inclusive,
        ..
    } = iterable
    {
        // Use 0 as default start and i64::MAX as default end for open ranges.
        let default_start = ast::Expr::IntLiteral {
            value: 0,
            span: iterable.span(),
        };
        let default_end = ast::Expr::IntLiteral {
            value: i64::MAX,
            span: iterable.span(),
        };
        let s = start.as_deref().unwrap_or(&default_start);
        let e = end.as_deref().unwrap_or(&default_end);
        lower_for_range(ctx, pattern, s, e, body, *inclusive);
        return;
    }

    // General case: desugar `for x in iterable { body }` to:
    //   let _iter = iterable;
    //   let _idx  = 0;
    //   while _idx < len(_iter) {
    //       let x = _iter[_idx];
    //       body;
    //       _idx += 1;
    //   }

    // Loop var / index / iter / destructure-bind locals live in the enclosing
    // scope; hide them once the loop exits so they don't shadow an outer
    // same-named binding (see lower_for_range).
    let for_scope_start = ctx.locals.len();

    // Infer the element type for array iteration so loop variables
    // carry struct/enum type info (needed for field access codegen).
    let iter_type = infer_expr_type(ctx, iterable);
    let elem_type = match &iter_type {
        MirType::Array(elem, _) => *elem.clone(),
        _ => MirType::I64,
    };

    // Preserve the iterable's real type (Array, etc.) so downstream codegen
    // routes Index through the dynamic-array path (kryos_array_get) instead
    // of treating an opaque i64 handle as a raw i64* buffer.
    let iter_local = ctx.alloc_temp(iter_type.clone());
    let iter_rvalue = lower_expr_to_rvalue(ctx, iterable);
    ctx.emit(Instruction::Assign {
        dest: iter_local,
        value: iter_rvalue,
    });

    let idx_local = ctx.alloc_local(Some("_idx".into()), MirType::I64, true);
    ctx.emit(Instruction::Assign {
        dest: idx_local,
        value: RValue::ConstInt(0),
    });

    let header_bb = ctx.alloc_block();
    let body_bb = ctx.alloc_block();
    let increment_bb = ctx.alloc_block();
    let exit_bb = ctx.alloc_block();

    // Jump to header.
    ctx.finish_block(Terminator::Goto(header_bb), header_bb);

    // Header: _idx < len(_iter).
    let len_temp = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: len_temp,
        value: RValue::Call {
            func: "len".into(),
            args: vec![Operand::Local(iter_local)],
        },
    });
    let cond_temp = ctx.alloc_temp(MirType::Bool);
    ctx.emit(Instruction::Assign {
        dest: cond_temp,
        value: RValue::BinOp {
            op: MirBinOp::Lt,
            left: Operand::Local(idx_local),
            right: Operand::Local(len_temp),
        },
    });
    ctx.finish_block(
        Terminator::Branch {
            cond: Operand::Local(cond_temp),
            then_block: body_bb,
            else_block: exit_bb,
        },
        body_bb,
    );

    // Body: bind loop variable from pattern.
    let loop_var_name = match pattern {
        ast::Pattern::Ident { name, .. } => name.clone(),
        _ => "_anon".into(),
    };
    let elem_type_for_destructure = elem_type.clone();
    let loop_var = ctx.alloc_local(Some(loop_var_name), elem_type, false);
    // Loop variable borrows from the array — must NOT be freed on scope exit.
    ctx.borrowed_locals.insert(loop_var.0);
    ctx.emit(Instruction::Assign {
        dest: loop_var,
        value: RValue::Index {
            object: Operand::Local(iter_local),
            index: Operand::Local(idx_local),
        },
    });
    // Tuple pattern: `for (a, b) in pairs` — destructure the element into its
    // named fields (the checker binds a/b's types; the MIR must extract them,
    // else a/b were undefined temporaries reading 0).
    if let ast::Pattern::Tuple { elements, .. } = pattern {
        let elem_tys = if let MirType::Tuple(ts) = &elem_type_for_destructure {
            ts.clone()
        } else {
            Vec::new()
        };
        for (ei, epat) in elements.iter().enumerate() {
            if let ast::Pattern::Ident { name, .. } = epat {
                let ety = elem_tys.get(ei).cloned().unwrap_or(MirType::I64);
                let bind = ctx.alloc_local(Some(name.clone()), ety.clone(), false);
                if !is_copy_type(ctx, &ety) {
                    ctx.borrowed_locals.insert(bind.0);
                }
                ctx.emit(Instruction::Assign {
                    dest: bind,
                    value: RValue::Field {
                        object: Operand::Local(loop_var),
                        field: ei.to_string(),
                    },
                });
            }
        }
    }

    // `continue` must jump to increment_bb (not header), otherwise _idx
    // never advances and the loop spins forever.
    ctx.loop_headers.push(increment_bb);
    ctx.loop_exits.push(exit_bb);
    lower_block_stmts(ctx, &body.stmts);
    ctx.loop_headers.pop();
    ctx.loop_exits.pop();

    // Fall through to increment block.
    ctx.finish_block(Terminator::Goto(increment_bb), increment_bb);

    // Increment block: _idx += 1, then back-edge to header.
    ctx.emit(Instruction::Assign {
        dest: idx_local,
        value: RValue::BinOp {
            op: MirBinOp::Add,
            left: Operand::Local(idx_local),
            right: Operand::Constant(Constant::Int(1)),
        },
    });
    ctx.finish_block(Terminator::Goto(header_bb), exit_bb);
    hide_scope_locals(ctx, for_scope_start);
}

/// Lower `for i in range(start, end) { body }` into a simple counter loop:
///
/// ```text
///   let _idx = start
///   while _idx < end {
///       let i = _idx
///       body
///       _idx += 1
///   }
/// ```
///
/// This avoids needing `range()` to produce an actual collection at runtime.
fn lower_for_range(
    ctx: &mut LoweringContext,
    pattern: &ast::Pattern,
    start_expr: &ast::Expr,
    end_expr: &ast::Expr,
    body: &ast::Block,
    inclusive: bool,
) {
    // The loop variable + index are allocated in the ENCLOSING scope (before
    // the body). Record the start so they can be hidden from name resolution
    // once the loop exits -- otherwise `let i = "outer"  for i in 0..3 {}  i`
    // leaves the outer `i` resolving to the loop's i64 var (a segfault when
    // the types differ). The body's OWN locals are hidden by lower_block_stmts.
    let for_scope_start = ctx.locals.len();
    // Lower start and end bounds.
    let start_op = lower_expr_to_operand(ctx, start_expr);
    let end_op = lower_expr_to_operand(ctx, end_expr);

    // Store end in a local so it's only evaluated once.
    let end_local = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: end_local,
        value: RValue::Use(end_op),
    });

    // let _idx = start
    let idx_local = ctx.alloc_local(Some("_idx".into()), MirType::I64, true);
    ctx.emit(Instruction::Assign {
        dest: idx_local,
        value: RValue::Use(start_op),
    });

    let header_bb = ctx.alloc_block();
    let body_bb = ctx.alloc_block();
    let increment_bb = ctx.alloc_block();
    let exit_bb = ctx.alloc_block();

    // Jump to header.
    ctx.finish_block(Terminator::Goto(header_bb), header_bb);

    // Header: `_idx < end` (exclusive `..`) or `_idx <= end` (inclusive `..=`).
    // The parser records the `..=` form in RangeExpr.inclusive; dropping it
    // here made `for i in 0..=5` iterate as 0..5 -- silently wrong sums.
    let cond_temp = ctx.alloc_temp(MirType::Bool);
    ctx.emit(Instruction::Assign {
        dest: cond_temp,
        value: RValue::BinOp {
            op: if inclusive { MirBinOp::LtEq } else { MirBinOp::Lt },
            left: Operand::Local(idx_local),
            right: Operand::Local(end_local),
        },
    });
    ctx.finish_block(
        Terminator::Branch {
            cond: Operand::Local(cond_temp),
            then_block: body_bb,
            else_block: exit_bb,
        },
        body_bb,
    );

    // Body: bind loop variable (let i = _idx)
    let loop_var_name = match pattern {
        ast::Pattern::Ident { name, .. } => name.clone(),
        _ => "_anon".into(),
    };
    let loop_var = ctx.alloc_local(Some(loop_var_name), MirType::I64, false);
    ctx.emit(Instruction::Assign {
        dest: loop_var,
        value: RValue::Use(Operand::Local(idx_local)),
    });

    // `continue` must jump to increment_bb so _idx advances.
    ctx.loop_headers.push(increment_bb);
    ctx.loop_exits.push(exit_bb);
    lower_block_stmts(ctx, &body.stmts);
    ctx.loop_headers.pop();
    ctx.loop_exits.pop();

    // Fall through to increment block.
    ctx.finish_block(Terminator::Goto(increment_bb), increment_bb);

    // Increment block: _idx += 1, then back-edge to header.
    ctx.emit(Instruction::Assign {
        dest: idx_local,
        value: RValue::BinOp {
            op: MirBinOp::Add,
            left: Operand::Local(idx_local),
            right: Operand::Constant(Constant::Int(1)),
        },
    });
    ctx.finish_block(Terminator::Goto(header_bb), exit_bb);
    hide_scope_locals(ctx, for_scope_start);
}

/// Hide every named local allocated at or after `scope_start` from further
/// name resolution. Used at the exit of a scope-introducing construct whose
/// bindings live in the enclosing `ctx.locals` list (for-loop var/index,
/// match-arm pattern bindings) so an outer same-named binding re-resolves to
/// the outer local after the construct ends. Idempotent.
fn hide_scope_locals(ctx: &mut LoweringContext, scope_start: usize) {
    let end = ctx.locals.len();
    for i in scope_start..end {
        if ctx.locals[i].name.is_some() {
            ctx.hidden_locals.insert(ctx.locals[i].id.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Parallel-for lowering
// ---------------------------------------------------------------------------

/// Lower `parallel for i in range(start, end) { body }` into chunked spawns.
///
/// Generates NUM_THREADS spawn instructions, each running a slice of the range:
///
/// ```text
///   // for each thread t in 0..NUM_THREADS:
///   spawn __parallel_for_N() {
///       let __cs = start + t * chunk_size
///       let __ce = min(__cs + chunk_size, end)
///       for i in range(__cs, __ce) { body }
///   }
/// ```
///
/// For non-`range()` iterables the parallel keyword is accepted but execution
/// falls back to a sequential for-loop.
fn lower_parallel_for(
    ctx: &mut LoweringContext,
    pattern: &ast::Pattern,
    iterable: &ast::Expr,
    body: &ast::Block,
) {
    // Only optimise `range(start, end)` calls.
    let (start_expr, end_expr) = match iterable {
        ast::Expr::FnCall { callee, args, .. } => {
            if let ast::Expr::Identifier { name, .. } = callee.as_ref() {
                if name == "range" && args.len() == 2 {
                    (&args[0], &args[1])
                } else {
                    lower_for(ctx, pattern, iterable, body);
                    return;
                }
            } else {
                lower_for(ctx, pattern, iterable, body);
                return;
            }
        }
        _ => {
            lower_for(ctx, pattern, iterable, body);
            return;
        }
    };

    const NUM_THREADS: i64 = 4;

    // Evaluate start and end once.
    let start_op = lower_expr_to_operand(ctx, start_expr);
    let end_op = lower_expr_to_operand(ctx, end_expr);

    let start_local = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: start_local,
        value: RValue::Use(start_op),
    });

    let end_local = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: end_local,
        value: RValue::Use(end_op),
    });

    // __total = end - start
    let total_local = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: total_local,
        value: RValue::BinOp {
            op: MirBinOp::Sub,
            left: Operand::Local(end_local),
            right: Operand::Local(start_local),
        },
    });

    // __chunk_size = (__total + NUM_THREADS - 1) / NUM_THREADS  (ceiling division)
    let total_plus = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: total_plus,
        value: RValue::BinOp {
            op: MirBinOp::Add,
            left: Operand::Local(total_local),
            right: Operand::Constant(Constant::Int(NUM_THREADS - 1)),
        },
    });

    let chunk_size_local = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: chunk_size_local,
        value: RValue::BinOp {
            op: MirBinOp::Div,
            left: Operand::Local(total_plus),
            right: Operand::Constant(Constant::Int(NUM_THREADS)),
        },
    });

    // Name the synthetic locals so capture analysis can find them.
    ctx.locals[start_local.0 as usize].name = Some("__pf_start".into());
    ctx.locals[end_local.0 as usize].name = Some("__pf_end".into());
    ctx.locals[chunk_size_local.0 as usize].name = Some("__pf_chunk_size".into());

    // Emit NUM_THREADS spawn instructions, each running a chunk.
    let span = kryos_errors::Span::DUMMY;

    for t in 0..NUM_THREADS {
        let inner_stmts = vec![
            // let __cs = __pf_start + t * __pf_chunk_size
            ast::Stmt::Let {
                name: "__cs".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::BinaryOp {
                    op: ast::BinOp::Add,
                    left: Box::new(ast::Expr::Identifier {
                        name: "__pf_start".into(),
                        span,
                    }),
                    right: Box::new(ast::Expr::BinaryOp {
                        op: ast::BinOp::Mul,
                        left: Box::new(ast::Expr::IntLiteral { value: t, span }),
                        right: Box::new(ast::Expr::Identifier {
                            name: "__pf_chunk_size".into(),
                            span,
                        }),
                        span,
                    }),
                    span,
                }),
                pattern: None,
                span,
            },
            // let __ce_raw = __cs + __pf_chunk_size
            ast::Stmt::Let {
                name: "__ce_raw".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::BinaryOp {
                    op: ast::BinOp::Add,
                    left: Box::new(ast::Expr::Identifier {
                        name: "__cs".into(),
                        span,
                    }),
                    right: Box::new(ast::Expr::Identifier {
                        name: "__pf_chunk_size".into(),
                        span,
                    }),
                    span,
                }),
                pattern: None,
                span,
            },
            // let __ce = min(__ce_raw, __pf_end)
            ast::Stmt::Let {
                name: "__ce".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::FnCall {
                    callee: Box::new(ast::Expr::Identifier {
                        name: "min".into(),
                        span,
                    }),
                    args: vec![
                        ast::Expr::Identifier {
                            name: "__ce_raw".into(),
                            span,
                        },
                        ast::Expr::Identifier {
                            name: "__pf_end".into(),
                            span,
                        },
                    ],
                    span,
                }),
                pattern: None,
                span,
            },
            // for <pattern> in range(__cs, __ce) { body }
            ast::Stmt::For {
                parallel: false,
                pattern: pattern.clone(),
                iterable: ast::Expr::FnCall {
                    callee: Box::new(ast::Expr::Identifier {
                        name: "range".into(),
                        span,
                    }),
                    args: vec![
                        ast::Expr::Identifier {
                            name: "__cs".into(),
                            span,
                        },
                        ast::Expr::Identifier {
                            name: "__ce".into(),
                            span,
                        },
                    ],
                    span,
                },
                body: body.clone(),
                span,
            },
        ];

        lower_spawn_block(ctx, &inner_stmts);
    }
}

// ---------------------------------------------------------------------------
// Spawn lowering
// ---------------------------------------------------------------------------

/// Lower `spawn expr` into a `Spawn` instruction.
///
/// Two cases:
/// 1. `spawn some_function(a, b)` — directly emits `Spawn { func, args }`.
/// 2. `spawn { ... }` (block) — generates a wrapper function `__spawn_N`
///    that takes captured variables as parameters, then emits `Spawn`
///    pointing to the wrapper with captures as args.
fn lower_spawn(ctx: &mut LoweringContext, expr: &ast::Expr) {
    match expr {
        // Case 1: spawn a direct function call.
        ast::Expr::FnCall { callee, args, .. } => {
            let func_name = match callee.as_ref() {
                ast::Expr::Identifier { name, .. } => name.clone(),
                _ => {
                    // Complex callee — evaluate and fall through to block path.
                    lower_spawn_block(
                        ctx,
                        &[ast::Stmt::Expr {
                            expr: expr.clone(),
                            span: kryos_errors::Span::DUMMY,
                        }],
                    );
                    return;
                }
            };
            let mir_args: Vec<Operand> =
                args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
            ctx.emit(Instruction::Spawn {
                func: func_name,
                args: mir_args,
            });
        }

        // Case 2: spawn a block expression.
        ast::Expr::Block { block, .. } => {
            lower_spawn_block(ctx, &block.stmts);
        }

        // Case 3: spawn a lambda — use the lambda's body directly so the
        // closure actually executes on the spawned thread. Without this,
        // the lambda would be wrapped as a stmt-expr and merely evaluated
        // (creating a closure value) then discarded, never invoking the body.
        ast::Expr::Lambda { body, .. } => {
            // The lambda body is an Expr — usually a Block, but could be any
            // expression. Extract its stmts if it's a Block; otherwise wrap
            // the body as a single expression statement.
            match body.as_ref() {
                ast::Expr::Block { block, .. } => {
                    lower_spawn_block(ctx, &block.stmts);
                }
                other_body => {
                    lower_spawn_block(
                        ctx,
                        &[ast::Stmt::Expr {
                            expr: other_body.clone(),
                            span: kryos_errors::Span::DUMMY,
                        }],
                    );
                }
            }
        }

        // Fallback: wrap arbitrary expression in a block.
        other => {
            lower_spawn_block(
                ctx,
                &[ast::Stmt::Expr {
                    expr: other.clone(),
                    span: kryos_errors::Span::DUMMY,
                }],
            );
        }
    }
}

/// Generate a `__spawn_N` wrapper function for a spawn block body.
///
/// Uses the same save/restore pattern as lambda lowering:
/// 1. Analyze free variables in the block
/// 2. Generate a function with captures as parameters
/// 3. Emit `Spawn { func: "__spawn_N", args: [captures...] }`
fn lower_spawn_block(ctx: &mut LoweringContext, stmts: &[ast::Stmt]) {
    lower_spawn_block_prefixed(ctx, stmts, "__spawn_");
}

/// Like [`lower_spawn_block`] but with a configurable wrapper-name prefix.
/// A `__coopspawn_` prefix signals the codegen to route the emitted `Spawn`
/// to the cooperative executor (`kryos_coop_spawn`) instead of OS-thread
/// `kryos_spawn`.
fn lower_spawn_block_prefixed(ctx: &mut LoweringContext, stmts: &[ast::Stmt], prefix: &str) {
    let spawn_name = format!("{}{}", prefix, ctx.spawn_counter);
    ctx.spawn_counter += 1;

    // Find captured variables from enclosing scope.
    let captures = find_free_variables_block(ctx, stmts);

    // Build the capture operands + their MIR types before we save state
    // (need access to the enclosing frame's locals). Preserving each
    // capture's type is what lets a method call inside the spawn body
    // (e.g. `w.run()`) resolve to the mangled `Worker__run` instead of a
    // bare unmangled `run` symbol that fails to link (backlog #32).
    let capture_ops: Vec<Operand> = captures
        .iter()
        .map(|name| {
            let local =
                find_local_by_name(ctx, name).expect("internal: spawn capture local not found");
            Operand::Local(local)
        })
        .collect();
    let capture_tys: Vec<Option<ast::TypeExpr>> = captures
        .iter()
        .map(|name| {
            find_local_by_name(ctx, name)
                .and_then(|lid| ctx.locals.iter().find(|l| l.id == lid))
                .and_then(|l| mir_type_to_type_expr(&l.ty))
        })
        .collect();

    // Save current function state and lower the spawn body as a new function.
    let saved = ctx.save_function_state();

    // Build params from captures, carrying each capture's resolved type so
    // method/field resolution inside the spawn body sees the real type.
    let params: Vec<ast::Param> = captures
        .iter()
        .zip(capture_tys.into_iter())
        .map(|(name, ty)| ast::Param {
            name: name.clone(),
            ty,
            default: None,
            span: kryos_errors::Span::DUMMY,
        })
        .collect();

    let body = ast::Block {
        stmts: stmts.to_vec(),
        span: kryos_errors::Span::DUMMY,
    };

    let mir_func = lower_function(ctx, &spawn_name, &params, &None, &body);
    ctx.restore_function_state(saved);
    ctx.monomorphized_functions.push(mir_func);

    // Register the spawn function's return type.
    ctx.func_ret_types.insert(spawn_name.clone(), MirType::Void);

    // Emit the spawn instruction.
    ctx.emit(Instruction::Spawn {
        func: spawn_name,
        args: capture_ops,
    });
}

/// Lower `coop_spawn(taskExpr)` — register a cooperative task with the async
/// executor. The task body always runs through a generated `__coopspawn_N`
/// wrapper so codegen can route the `Spawn` to `kryos_coop_spawn` by name
/// prefix. Inside the task, `await` / `coop_yield()` hand control to the
/// scheduler so multiple tasks interleave (see `kryos-rt::executor`).
///
/// Accepts the same shapes as `spawn`: a function call (`coop_spawn(task())`),
/// a closure (`coop_spawn(|| { ... })`), or a block.
fn lower_coop_spawn(ctx: &mut LoweringContext, arg: &ast::Expr) {
    let stmts: Vec<ast::Stmt> = match arg {
        ast::Expr::Block { block, .. } => block.stmts.clone(),
        ast::Expr::Lambda { body, .. } => match body.as_ref() {
            ast::Expr::Block { block, .. } => block.stmts.clone(),
            other => vec![ast::Stmt::Expr {
                expr: other.clone(),
                span: kryos_errors::Span::DUMMY,
            }],
        },
        // `coop_spawn(task())` and any other expression: run it inside the
        // wrapper (the FnCall is invoked on the task thread, not eagerly).
        other => vec![ast::Stmt::Expr {
            expr: other.clone(),
            span: kryos_errors::Span::DUMMY,
        }],
    };
    lower_spawn_block_prefixed(ctx, &stmts, "__coopspawn_");
}

// ---------------------------------------------------------------------------
// Try/Catch lowering
// ---------------------------------------------------------------------------

/// Emit a check for a pending thread-local exception (set by a cross-function
/// `throw`).  If an exception is pending, take it, store it as `Result::Err`
/// in `result_local`, and jump to `check_bb` (the tag-check block of the
/// enclosing try/catch).  Otherwise, fall through to a new continuation block.
///
/// Generated MIR:
/// ```text
///   _check = call kryos_exception_check()
///   branch _check → exc_handler_bb, continue_bb
///
///   exc_handler_bb:
///     _exc_val = call kryos_exception_take()
///     result_local = Result::Err(_exc_val)
///     goto check_bb
///
///   continue_bb:
///     ... (subsequent instructions) ...
/// ```
fn emit_exception_check(ctx: &mut LoweringContext, result_local: LocalId, check_bb: BlockId) {
    let exc_flag = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: exc_flag,
        value: RValue::Call {
            func: "kryos_exception_check".into(),
            args: vec![],
        },
    });

    let exc_handler_bb = ctx.alloc_block();
    let continue_bb = ctx.alloc_block();

    ctx.finish_block(
        Terminator::Branch {
            cond: Operand::Local(exc_flag),
            then_block: exc_handler_bb,
            else_block: continue_bb,
        },
        exc_handler_bb,
    );

    // exc_handler_bb: take the exception value and store it as Result::Err.
    let exc_val = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: exc_val,
        value: RValue::Call {
            func: "kryos_exception_take".into(),
            args: vec![],
        },
    });
    ctx.emit(Instruction::Assign {
        dest: result_local,
        value: RValue::EnumVariant {
            enum_name: "Result".into(),
            variant_idx: 1, // Err
            fields: vec![Operand::Local(exc_val)],
        },
    });
    ctx.finish_block(Terminator::Goto(check_bb), continue_bb);

    // continue_bb: normal execution continues here.
}

/// Lower `try { body } catch e { handler }` into:
///
/// ```text
///   let _result = { body }          // last expr wrapped in Result::Ok
///   let _tag = enum_tag(_result)
///   if _tag == 0 goto ok_bb else goto err_bb
///   ok_bb: extract Ok payload -> continue
///   err_bb: let e = extract Err payload; handler
///   merge_bb:
/// ```
fn lower_try_catch(
    ctx: &mut LoweringContext,
    try_block: &ast::Block,
    catch_name: &str,
    catch_block: &ast::Block,
    value_dest: Option<LocalId>,
) {
    let result_local = ctx.alloc_temp(MirType::Enum("Result".into()));

    // Declare the catch binding up front and ZERO-INITIALIZE it. The
    // scope-end drop loop frees every named str local unconditionally in
    // the merge path; when the try succeeded the binding was previously
    // an UNINITIALIZED slot, so that drop freed garbage (UB: segfault or
    // LLVM deleting the caller's tail as unreachable). Null-init makes
    // the ok-path drop a no-op (kryos_string_free is null-safe) and the
    // err path overwrites it with the real thrown string.
    let err_payload = ctx.alloc_local(Some(catch_name.to_string()), MirType::Str, false);
    ctx.emit(Instruction::Assign {
        dest: err_payload,
        value: RValue::Use(Operand::Constant(Constant::Int(0))),
    });

    // Locals allocated from here on belong to the TRY body; recorded so they can
    // be hidden from name resolution before the catch block is lowered (a
    // try-body `let e = ..` otherwise shadowed the catch binding `e`).
    let try_scope_start = ctx.locals.len();

    // Pre-allocate the tag-check block so `throw` can jump to it.
    let check_bb = ctx.alloc_block();

    // Save previous try/catch context (for nesting) and install ours.
    let prev_target = ctx.try_catch_target.take();
    ctx.try_catch_target = Some(TryCatchTarget {
        result_local,
        check_block: check_bb,
    });

    // Lower the try block body. The last expression is wrapped in Result::Ok.
    // After each statement, check the thread-local exception state so that
    // `throw` from a called function is caught immediately.
    for (i, stmt) in try_block.stmts.iter().enumerate() {
        if i == try_block.stmts.len() - 1 {
            // Wrap last expression in Result::Ok.
            if let ast::Stmt::Expr { expr, .. } = stmt {
                let val = lower_expr_to_operand(ctx, expr);
                // Pin a bare constant tail into a typed temp: the LLVM
                // backend miscompiles EnumVariant("Result") construction
                // with a Constant payload (AOT segfault on `try { 7 }`);
                // a Local payload takes the proven path.
                let val = match val {
                    Operand::Constant(_) => {
                        let ty = infer_expr_type(ctx, expr);
                        let tmp = ctx.alloc_temp(ty);
                        ctx.emit(Instruction::Assign {
                            dest: tmp,
                            value: RValue::Use(val),
                        });
                        Operand::Local(tmp)
                    }
                    other => other,
                };
                // Before wrapping in Ok, check if a cross-function throw
                // set the thread-local exception during this expression.
                emit_exception_check(ctx, result_local, check_bb);
                ctx.emit(Instruction::Assign {
                    dest: result_local,
                    value: RValue::EnumVariant {
                        enum_name: "Result".into(),
                        variant_idx: 0, // Ok
                        fields: vec![val],
                    },
                });
            } else if let Some(if_expr) = stmt.as_value_if_expr() {
                // The try body's OWN tail is a value `if` (parsed as Stmt::If,
                // not Stmt::Expr): lower it to a value and wrap in Ok, exactly
                // like the bare-expr tail above. Without this it fell to the
                // side-effecting branch below and stored Ok(0) -- `try { if c
                // { 1 } else { 2 } }` silently yielded 0 on both backends.
                // (`match` at tail position is already Stmt::Expr{MatchExpr},
                // so it was covered; only `if` commits to Stmt::If.)
                let val = lower_expr_to_operand(ctx, &if_expr);
                let val = match val {
                    Operand::Constant(_) => {
                        let ty = infer_expr_type(ctx, &if_expr);
                        let tmp = ctx.alloc_temp(ty);
                        ctx.emit(Instruction::Assign {
                            dest: tmp,
                            value: RValue::Use(val),
                        });
                        Operand::Local(tmp)
                    }
                    other => other,
                };
                emit_exception_check(ctx, result_local, check_bb);
                ctx.emit(Instruction::Assign {
                    dest: result_local,
                    value: RValue::EnumVariant {
                        enum_name: "Result".into(),
                        variant_idx: 0,
                        fields: vec![val],
                    },
                });
            } else {
                lower_stmt(ctx, stmt);
                emit_exception_check(ctx, result_local, check_bb);
                ctx.emit(Instruction::Assign {
                    dest: result_local,
                    value: RValue::EnumVariant {
                        enum_name: "Result".into(),
                        variant_idx: 0,
                        fields: vec![Operand::Constant(Constant::Int(0))],
                    },
                });
            }
        } else {
            lower_stmt(ctx, stmt);
            emit_exception_check(ctx, result_local, check_bb);
        }
    }

    // If try block is empty, produce Ok(0).
    if try_block.stmts.is_empty() {
        ctx.emit(Instruction::Assign {
            dest: result_local,
            value: RValue::EnumVariant {
                enum_name: "Result".into(),
                variant_idx: 0,
                fields: vec![Operand::Constant(Constant::Int(0))],
            },
        });
    }

    // Restore previous try/catch context.
    ctx.try_catch_target = prev_target;

    // Fall through from the try block into the tag-check block.
    ctx.finish_block(Terminator::Goto(check_bb), check_bb);

    // Extract tag and branch.
    let tag_local = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: tag_local,
        value: RValue::EnumTag {
            operand: Operand::Local(result_local),
        },
    });

    let ok_bb = ctx.alloc_block();
    let err_bb = ctx.alloc_block();
    let merge_bb = ctx.alloc_block();

    // tag == 0 means Ok, tag == 1 means Err.
    ctx.finish_block(
        Terminator::Switch {
            value: Operand::Local(tag_local),
            targets: vec![(0, ok_bb)],
            default: err_bb,
        },
        ok_bb,
    );

    // Ok path: extract the Ok payload and continue.
    let ok_payload = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: ok_payload,
        value: RValue::EnumPayload {
            operand: Operand::Local(result_local),
            enum_name: "Result".into(),
            variant_idx: 0,
            field_idx: 0,
        },
    });
    // Drop the result enum shell (payload was moved out by EnumPayload).
    drop_tag(ctx, "question-op-shell");
    ctx.emit(Instruction::Drop {
        local: result_local,
    });
    // Value position: the try block's tail value (Ok payload) is the result.
    if let Some(dest) = value_dest {
        ctx.emit(Instruction::Assign {
            dest,
            value: RValue::Use(Operand::Local(ok_payload)),
        });
    }
    ctx.finish_block(Terminator::Goto(merge_bb), err_bb);

    // Err path: bind error value to catch_name (the up-front zero-init
    // local), execute handler. The thrown value is stringified at the
    // throw site (see Stmt::Throw), so the binding is a str — matching
    // its static type in check.rs.
    ctx.emit(Instruction::Assign {
        dest: err_payload,
        value: RValue::EnumPayload {
            operand: Operand::Local(result_local),
            enum_name: "Result".into(),
            variant_idx: 1,
            field_idx: 0,
        },
    });
    // Drop the result enum shell (payload was moved out by EnumPayload).
    drop_tag(ctx, "question-op-shell");
    ctx.emit(Instruction::Drop {
        local: result_local,
    });
    // Hide every local declared INSIDE the try body from name resolution before
    // lowering the catch block. The try body's locals are out of scope in the
    // catch block, but they share the function-flat locals vec with the catch
    // binding and are MORE RECENT, so `find_local_by_name` resolved the catch
    // block's `catch_name` to a try-body `let <catch_name> = ..` instead of the
    // real error binding -- the handler read the stale try value (or SEGFAULTED
    // when that local was a non-str type read as the str catch binding).
    for i in try_scope_start..ctx.locals.len() {
        ctx.hidden_locals.insert(ctx.locals[i].id.0);
    }
    // Value position: the catch block's tail expression is the result.
    if let Some(dest) = value_dest {
        lower_block_as_value(ctx, &catch_block.stmts, dest);
    } else {
        lower_block_stmts(ctx, &catch_block.stmts);
    }
    // The catch binding is scoped to this catch block. Hide it from NAME
    // resolution now so it does not leak into an ENCLOSING catch (or a later
    // sibling try/catch) that reuses the same variable name: both share the
    // function-flat locals vec, and find_local_by_name reverse-scans to the
    // MOST RECENT match, so a nested `catch e { .. }` left its stale `e`
    // shadowing the outer `catch e`'s binding -- `throw "x: " + e` built the
    // right value but the outer handler then read the INNER e (the original
    // message) instead of the rethrown one. Hiding only affects name lookup,
    // not the scope-end Drop, so the buffer is still freed exactly once.
    ctx.hidden_locals.insert(err_payload.0);
    ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
}

// ---------------------------------------------------------------------------
// Match lowering
// ---------------------------------------------------------------------------

/// Per-arm enum binding: (enum_name, variant_idx, field_patterns).
struct EnumBinding {
    enum_name: String,
    variant_idx: u32,
    field_patterns: Vec<ast::Pattern>,
}

/// Per-arm tuple binding: the element patterns of a `(a, b, ..)` match arm,
/// used to bind ident elements after the arm's comparison succeeds.
struct TupleBinding {
    element_patterns: Vec<ast::Pattern>,
}


/// True when a tuple ELEMENT sub-pattern is safe for the ordered
/// (sequential test-chain) path, at ANY nesting depth: a bare
/// literal/ident/wildcard, an enum whose own fields are all simple
/// (ident/wildcard), or a nested tuple whose own elements are all
/// (recursively) safe. Anything else -- an or-pattern, an enum with a
/// compound field, a nested struct pattern -- is excluded and the whole
/// match stays on the switch path. This recursion is what lets
/// `tuple_seq_safe` admit arbitrarily deep tuple nesting (previously it only
/// checked one level down, so `(n, (a, (c, d)), t)` was excluded and routed
/// to the switch path, which mis-executed it).
fn tuple_elem_seq_safe(pat: &ast::Pattern) -> bool {
    match pat {
        ast::Pattern::Literal { .. }
        | ast::Pattern::Ident { .. }
        | ast::Pattern::Wildcard { .. } => true,
        // Fully recursive: an enum element whose payload is itself an enum /
        // literal / tuple pattern (`(L2a(L1b(m)), n)`) is handled by the
        // sequential path's refine_pattern_acc. The old one-level gate
        // (fields all Ident/Wildcard) pushed nested shapes onto the SWITCH
        // tuple chain, which emits NO enum-element tests at all -- every
        // such match took its first arm.
        ast::Pattern::Enum { fields, .. } => fields.iter().all(tuple_elem_seq_safe),
        ast::Pattern::Tuple { elements, .. } => elements.iter().all(tuple_elem_seq_safe),
        _ => false,
    }
}

/// Recursive acc-style pattern refinement for the sequential/ordered match
/// path: emits tag-equality tests (at every nesting depth), literal-equality
/// tests, and payload/element bindings for the pattern against an already
/// EXTRACTED value local, ANDing every test into `acc`. Used by
/// `bind_tuple_pattern` for enum-shaped tuple elements and by itself for
/// nested payload sub-patterns.
fn refine_pattern_acc(
    ctx: &mut LoweringContext,
    value_local: LocalId,
    value_ty: &MirType,
    pat: &ast::Pattern,
    acc: &mut Option<Operand>,
) {
    match pat {
        ast::Pattern::Wildcard { .. } => {}
        ast::Pattern::Ident { name, mutable, .. } => {
            if let Some((_, vidx)) = ident_variant_of(ctx, name, value_ty) {
                let tag = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: tag,
                    value: RValue::EnumTag {
                        operand: Operand::Local(value_local),
                    },
                });
                let cmp = ctx.alloc_temp(MirType::Bool);
                ctx.emit(Instruction::Assign {
                    dest: cmp,
                    value: RValue::BinOp {
                        op: MirBinOp::Eq,
                        left: Operand::Local(tag),
                        right: Operand::Constant(Constant::Int(vidx as i64)),
                    },
                });
                and_into_acc(ctx, acc, Operand::Local(cmp));
            } else {
                let bound = ctx.alloc_local(Some(name.clone()), value_ty.clone(), *mutable);
                if !is_copy_type(ctx, value_ty) {
                    ctx.dropped_locals.insert(bound.0);
                }
                ctx.emit(Instruction::Assign {
                    dest: bound,
                    value: RValue::Use(Operand::Local(value_local)),
                });
            }
        }
        ast::Pattern::Literal { expr, .. } => {
            let lit = lower_expr_to_operand(ctx, expr);
            let cmp = ctx.alloc_temp(MirType::Bool);
            ctx.emit(Instruction::Assign {
                dest: cmp,
                value: RValue::BinOp {
                    op: MirBinOp::Eq,
                    left: Operand::Local(value_local),
                    right: lit,
                },
            });
            and_into_acc(ctx, acc, Operand::Local(cmp));
        }
        ast::Pattern::Enum {
            name: pname,
            variant,
            fields,
            ..
        } => {
            let enum_name = match value_ty {
                MirType::Enum(n) => n.clone(),
                MirType::Struct(n) if ctx.enum_defs.contains_key(n.as_str()) => n.clone(),
                _ if !pname.is_empty() => pname.clone(),
                _ => find_enum_variant(ctx, variant)
                    .map(|(n, _)| n)
                    .unwrap_or_default(),
            };
            let Some(vidx) = ctx
                .enum_defs
                .get(enum_name.as_str())
                .and_then(|vs| vs.iter().position(|v| v.name == *variant))
            else {
                // Unresolvable: defensively no test (matches the old
                // always-match fallback rather than emitting a wrong test).
                return;
            };
            let tag = ctx.alloc_temp(MirType::I64);
            ctx.emit(Instruction::Assign {
                dest: tag,
                value: RValue::EnumTag {
                    operand: Operand::Local(value_local),
                },
            });
            let cmp = ctx.alloc_temp(MirType::Bool);
            ctx.emit(Instruction::Assign {
                dest: cmp,
                value: RValue::BinOp {
                    op: MirBinOp::Eq,
                    left: Operand::Local(tag),
                    right: Operand::Constant(Constant::Int(vidx as i64)),
                },
            });
            and_into_acc(ctx, acc, Operand::Local(cmp));
            // Bindings are plain word reads of the (uniform-layout) payload
            // slot -- safe to extract unconditionally, and suppressed from
            // drops. REFUTABLE sub-patterns are collected for a
            // SHORT-CIRCUITED block below: an inner refinement may DEREF the
            // payload (e.g. `enum_tag` of a payload enum, a heap pointer on
            // the JIT), which is only valid when the OUTER tag matched --
            // evaluating it eagerly in the AND chain dereferenced garbage/
            // null whenever a different variant flowed in (SIGSEGV).
            let mut refutable_subs: Vec<(usize, MirType)> = Vec::new();
            for (fi, fpat) in fields.iter().enumerate() {
                if matches!(fpat, ast::Pattern::Wildcard { .. }) {
                    continue;
                }
                let field_type = ctx
                    .enum_defs
                    .get(enum_name.as_str())
                    .and_then(|vs| vs.get(vidx))
                    .and_then(|v| v.fields.get(fi))
                    .cloned()
                    .unwrap_or(MirType::I64);
                let field_type = recover_enum_types(ctx, field_type);
                // Plain binding fast path keeps the direct extract-into-named
                // local (with drop suppression, as the old inline loop did).
                let bind_name = match fpat {
                    ast::Pattern::Ident { name: bn, mutable, .. }
                        if ident_variant_of(ctx, bn, &field_type).is_none() =>
                    {
                        Some((bn.clone(), *mutable))
                    }
                    _ => None,
                };
                if let Some((bn, m)) = &bind_name {
                    let l = ctx.alloc_local(Some(bn.clone()), field_type.clone(), *m);
                    if !is_copy_type(ctx, &field_type) {
                        ctx.dropped_locals.insert(l.0);
                    }
                    ctx.emit(Instruction::Assign {
                        dest: l,
                        value: RValue::EnumPayload {
                            operand: Operand::Local(value_local),
                            enum_name: enum_name.clone(),
                            variant_idx: vidx as u32,
                            field_idx: fi as u32,
                        },
                    });
                } else {
                    refutable_subs.push((fi, field_type));
                }
            }
            if !refutable_subs.is_empty() {
                // ok = false; if <acc so far> { ok = <inner tests> }  --
                // extraction + inner refinement only run on the matched-tag
                // path. `ok` replaces acc for the arm condition.
                let ok = ctx.alloc_local(None, MirType::Bool, true);
                ctx.emit(Instruction::Assign {
                    dest: ok,
                    value: RValue::Use(Operand::Constant(Constant::Bool(false))),
                });
                let inner_bb = ctx.alloc_block();
                let join_bb = ctx.alloc_block();
                let cond = acc
                    .take()
                    .unwrap_or(Operand::Constant(Constant::Bool(true)));
                ctx.finish_block(
                    Terminator::Branch {
                        cond,
                        then_block: inner_bb,
                        else_block: join_bb,
                    },
                    inner_bb,
                );
                let mut inner_acc: Option<Operand> = None;
                for (fi, field_type) in refutable_subs {
                    let t = ctx.alloc_temp(field_type.clone());
                    if !is_copy_type(ctx, &field_type) {
                        ctx.dropped_locals.insert(t.0);
                    }
                    ctx.emit(Instruction::Assign {
                        dest: t,
                        value: RValue::EnumPayload {
                            operand: Operand::Local(value_local),
                            enum_name: enum_name.clone(),
                            variant_idx: vidx as u32,
                            field_idx: fi as u32,
                        },
                    });
                    refine_pattern_acc(ctx, t, &field_type, &fields[fi], &mut inner_acc);
                }
                let inner_op =
                    inner_acc.unwrap_or(Operand::Constant(Constant::Bool(true)));
                ctx.emit(Instruction::Assign {
                    dest: ok,
                    value: RValue::Use(inner_op),
                });
                ctx.finish_block(Terminator::Goto(join_bb), join_bb);
                *acc = Some(Operand::Local(ok));
            }
        }
        ast::Pattern::Tuple { elements, .. } => {
            let elem_tys = match value_ty {
                MirType::Tuple(e) => e.clone(),
                _ => Vec::new(),
            };
            bind_tuple_pattern(ctx, value_local, elements, &elem_tys, acc);
        }
        ast::Pattern::Struct { name, fields, .. } => {
            let sname = match value_ty {
                MirType::Struct(n) => n.clone(),
                _ => name.clone(),
            };
            bind_struct_pattern(ctx, value_local, &sname, fields, acc);
        }
        _ => {}
    }
}

/// AND a new boolean test into the running arm-condition accumulator chain
/// (shared by every element/sub-element test in a tuple pattern's ordered
/// binder so a literal or enum-tag test at any nesting depth still narrows
/// the same arm condition).
fn and_into_acc(ctx: &mut LoweringContext, acc: &mut Option<Operand>, this: Operand) {
    *acc = match acc.take() {
        None => Some(this),
        Some(prev) => {
            let combined = ctx.alloc_temp(MirType::Bool);
            ctx.emit(Instruction::Assign {
                dest: combined,
                value: RValue::BinOp {
                    op: MirBinOp::And,
                    left: prev,
                    right: this,
                },
            });
            Some(Operand::Local(combined))
        }
    };
}

/// Recursively bind/test a tuple pattern's elements against an
/// already-materialized tuple-typed local (`tuple_local`). Wildcards are
/// skipped, idents are bound via `RValue::Field`, literals are extracted and
/// AND-tested into `acc`, enum elements get a tag test + payload extraction
/// (mirroring 6bec772's nested-enum treatment), and a nested TUPLE element is
/// extracted into its own local and this function RECURSES on it -- this is
/// what makes 2+ levels of tuple nesting (`(n, (a, (c, d)), t)`) bind every
/// leaf correctly instead of silently reading zero below depth 1 (the prior
/// one-level-only version of this logic, added in 95a6cb6, only recursed
/// one level and hit a catch-all `_ => {}` for a sub-element that was itself
/// a tuple).
/// If `name` is a variant of the enum type `ety`, return (enum_name, variant_idx).
/// A bare ident element in a tuple/nested pattern whose element type IS an enum
/// and whose name matches one of its variants is a variant TEST, not a binding
/// (mirrors the single-subject switch path's `Pattern::Ident` resolution).
/// Previously `(Red, Red)` bound both elements as fresh locals named `Red`, so
/// every tuple-of-bare-variant arm matched unconditionally and the FIRST arm
/// always won. A name that matches no variant of the element's own enum type
/// stays an ordinary binding.
/// Desugar `a |> f` / `a |> f(b, c)` into the equivalent FnCall AST so both
/// the lowering AND the type inference route through the ordinary call
/// machinery (builtin overload dispatch, closure invocation, coercions).
fn desugar_pipe(left: &ast::Expr, right: &ast::Expr, span: kryos_errors::Span) -> ast::Expr {
    match right {
        ast::Expr::FnCall {
            callee,
            args,
            span: cspan,
        } => {
            // `a |> f(b, c)` -> `f(a, b, c)`
            let mut all_args = vec![left.clone()];
            all_args.extend(args.iter().cloned());
            ast::Expr::FnCall {
                callee: callee.clone(),
                args: all_args,
                span: *cspan,
            }
        }
        // `a |> f` (named fn, closure var, or inline closure literal) -> `f(a)`.
        _ => ast::Expr::FnCall {
            callee: Box::new(right.clone()),
            args: vec![left.clone()],
            span,
        },
    }
}

/// Result type of mixed-width integer arithmetic: the WIDER of the two
/// integer operands (C-style integer promotion, no narrowing). Non-integer
/// or equal-width types keep the left type (the historical behavior for
/// same-typed operands). Signedness follows the wider operand; on a
/// same-width signed/unsigned mix the left wins (rare, and the value bits
/// are identical either way).
/// True for the sub-i64 integer types (their arithmetic must be masked to
/// width, unlike i64/u64/i128/u128 which fill the register).
fn is_narrow_int_ty(t: &MirType) -> bool {
    matches!(
        t,
        MirType::I8 | MirType::U8 | MirType::I16 | MirType::U16 | MirType::I32 | MirType::U32
    )
}

fn wider_int_type(lty: &MirType, rty: &MirType) -> MirType {
    fn rank(t: &MirType) -> Option<u8> {
        match t {
            MirType::I8 | MirType::U8 => Some(8),
            MirType::I16 | MirType::U16 => Some(16),
            MirType::I32 | MirType::U32 => Some(32),
            MirType::I64 | MirType::U64 => Some(64),
            MirType::I128 | MirType::U128 => Some(128),
            _ => None,
        }
    }
    match (rank(lty), rank(rty)) {
        (Some(lr), Some(rr)) if rr > lr => rty.clone(),
        _ => lty.clone(),
    }
}

fn ident_variant_of(ctx: &LoweringContext, name: &str, ety: &MirType) -> Option<(String, u32)> {
    // Accept the Struct(name) spelling of an enum type too: lower_type_expr
    // records every non-builtin named type as Struct (it can't know which
    // names are enums), and not every path normalizes before getting here.
    let en = match ety {
        MirType::Enum(n) => n,
        MirType::Struct(n) if ctx.enum_defs.contains_key(n.as_str()) => n,
        _ => return None,
    };
    ctx.enum_defs
        .get(en.as_str())
        .and_then(|vs| vs.iter().position(|v| v.name == name))
        .map(|i| (en.clone(), i as u32))
}

/// Recover enum types recorded as Struct(name), recursing through tuple
/// elements. lower_type_expr maps every non-builtin named type to
/// Struct(name); the existing single-level recoveries fix a DIRECT enum
/// payload, but a payload like `(Light, i64)` kept the Struct("Light") label
/// inside the tuple -- the LLVM backend then renders the local as
/// `{ %Light, i64 }` where %Light is an opaque forward decl, and clang
/// rejects the alloca/load ("unsized type"). Normalizing to Enum makes the
/// tuple render `{ i64, i64 }`, matching the construction-side layout.
fn recover_enum_types(ctx: &LoweringContext, ty: MirType) -> MirType {
    match ty {
        MirType::Struct(n) if ctx.enum_defs.contains_key(&n) => MirType::Enum(n),
        MirType::Tuple(elems) => MirType::Tuple(
            elems
                .into_iter()
                .map(|e| recover_enum_types(ctx, e))
                .collect(),
        ),
        other => other,
    }
}

fn bind_tuple_pattern(
    ctx: &mut LoweringContext,
    tuple_local: LocalId,
    elements: &[ast::Pattern],
    elem_tys: &[MirType],
    acc: &mut Option<Operand>,
) {
    for (ei, epat) in elements.iter().enumerate() {
        let ety = elem_tys.get(ei).cloned().unwrap_or(MirType::I64);
        match epat {
            ast::Pattern::Wildcard { .. } => {}
            ast::Pattern::Ident { name, mutable, .. } => {
                if let Some((_, vidx)) = ident_variant_of(ctx, name, &ety) {
                    // Bare variant element, e.g. `(Red, Red)`: tag test, no binding.
                    let elem = ctx.alloc_temp(ety.clone());
                    if !is_copy_type(ctx, &ety) {
                        ctx.dropped_locals.insert(elem.0);
                    }
                    ctx.emit(Instruction::Assign {
                        dest: elem,
                        value: RValue::Field {
                            object: Operand::Local(tuple_local),
                            field: ei.to_string(),
                        },
                    });
                    let tag = ctx.alloc_temp(MirType::I64);
                    ctx.emit(Instruction::Assign {
                        dest: tag,
                        value: RValue::EnumTag {
                            operand: Operand::Local(elem),
                        },
                    });
                    let cmp = ctx.alloc_temp(MirType::Bool);
                    ctx.emit(Instruction::Assign {
                        dest: cmp,
                        value: RValue::BinOp {
                            op: MirBinOp::Eq,
                            left: Operand::Local(tag),
                            right: Operand::Constant(Constant::Int(vidx as i64)),
                        },
                    });
                    and_into_acc(ctx, acc, Operand::Local(cmp));
                } else {
                    let bound = ctx.alloc_local(Some(name.clone()), ety.clone(), *mutable);
                    ctx.emit(Instruction::Assign {
                        dest: bound,
                        value: RValue::Field {
                            object: Operand::Local(tuple_local),
                            field: ei.to_string(),
                        },
                    });
                }
            }
            ast::Pattern::Literal { expr, .. } => {
                let elem = ctx.alloc_temp(ety.clone());
                ctx.emit(Instruction::Assign {
                    dest: elem,
                    value: RValue::Field {
                        object: Operand::Local(tuple_local),
                        field: ei.to_string(),
                    },
                });
                let lit = lower_expr_to_operand(ctx, expr);
                let cmp = ctx.alloc_temp(MirType::Bool);
                ctx.emit(Instruction::Assign {
                    dest: cmp,
                    value: RValue::BinOp {
                        op: MirBinOp::Eq,
                        left: Operand::Local(elem),
                        right: lit,
                    },
                });
                and_into_acc(ctx, acc, Operand::Local(cmp));
            }
            ast::Pattern::Enum { .. } => {
                // Nested enum element, possibly with its OWN nested
                // sub-patterns (`(L2a(L1b(m)), n)`): extract the element and
                // run the recursive acc-refinement (tag tests at EVERY depth,
                // literal payload tests, payload bindings). The old inline
                // version tested one tag level and bound only Ident payload
                // fields -- a bare-variant payload leaf (`L2a(L1a)`) became a
                // BINDING with no inner tag test, so sibling arms sharing the
                // outer variant never discriminated.
                let elem = ctx.alloc_temp(ety.clone());
                if !is_copy_type(ctx, &ety) {
                    ctx.dropped_locals.insert(elem.0);
                }
                ctx.emit(Instruction::Assign {
                    dest: elem,
                    value: RValue::Field {
                        object: Operand::Local(tuple_local),
                        field: ei.to_string(),
                    },
                });
                refine_pattern_acc(ctx, elem, &ety, epat, acc);
            }
            ast::Pattern::Tuple {
                elements: sub_elements,
                ..
            } => {
                // Nested tuple element, e.g.
                // `match (n, (a, (c, d)), tag) { (n, (a, (c, d)), tag) => .. }`.
                // Extract the element (a boxed-aggregate slot -- same codegen
                // path as a nested enum/struct element; see RValue::Field's
                // anon-struct/tuple unboxing in kryos-codegen-llvm), then
                // RECURSE into its own sub-elements against the SAME acc
                // chain used for the outer tuple's tests -- this is what
                // makes arbitrarily deep nesting bind correctly instead of
                // stopping at one level.
                let elem = ctx.alloc_temp(ety.clone());
                ctx.emit(Instruction::Assign {
                    dest: elem,
                    value: RValue::Field {
                        object: Operand::Local(tuple_local),
                        field: ei.to_string(),
                    },
                });
                let sub_elem_tys = match &ety {
                    MirType::Tuple(e) => e.clone(),
                    _ => Vec::new(),
                };
                bind_tuple_pattern(ctx, elem, sub_elements, &sub_elem_tys, acc);
            }
            // Any other nested sub-pattern: excluded by tuple_seq_safe;
            // treat defensively as always-match.
            _ => {}
        }
    }
}

/// Bind/test a struct pattern `Name { field: subpat, .. }` against a pinned
/// struct local. Mirrors bind_tuple_pattern but addresses fields BY NAME via
/// RValue::Field. Ident subpatterns bind the field; Literal subpatterns AND a
/// field-equality test into `acc`; nested Struct/Tuple subpatterns recurse. A
/// struct has no tag, so an all-ident/wildcard struct pattern is irrefutable
/// (acc stays None). Struct patterns previously never reached the lowering
/// (routed to the tagless enum switch path), so field bindings read 0 and
/// literal-field arms never matched -- a silent both-backend miscompile.
fn bind_struct_pattern(
    ctx: &mut LoweringContext,
    struct_local: LocalId,
    struct_name: &str,
    fields: &[(String, ast::Pattern)],
    acc: &mut Option<Operand>,
) {
    for (fname, fpat) in fields {
        let fty = ctx
            .struct_defs
            .get(struct_name)
            .and_then(|fs| fs.iter().find(|(n, _)| n == fname).map(|(_, t)| t.clone()))
            .unwrap_or(MirType::I64);
        // A field declared as a Struct whose name is actually an enum reads
        // back as an enum value (same remap the enum-payload path uses).
        let fty = match fty {
            MirType::Struct(n) if ctx.enum_defs.contains_key(&n) => MirType::Enum(n),
            other => other,
        };
        match fpat {
            ast::Pattern::Wildcard { .. } => {}
            ast::Pattern::Ident { name, mutable, .. } => {
                let bound = ctx.alloc_local(Some(name.clone()), fty.clone(), *mutable);
                if !is_copy_type(ctx, &fty) {
                    ctx.borrowed_locals.insert(bound.0);
                }
                ctx.emit(Instruction::Assign {
                    dest: bound,
                    value: RValue::Field {
                        object: Operand::Local(struct_local),
                        field: fname.clone(),
                    },
                });
            }
            ast::Pattern::Literal { expr, .. } => {
                let elem = ctx.alloc_temp(fty.clone());
                ctx.emit(Instruction::Assign {
                    dest: elem,
                    value: RValue::Field {
                        object: Operand::Local(struct_local),
                        field: fname.clone(),
                    },
                });
                let lit = lower_expr_to_operand(ctx, expr);
                let cmp = ctx.alloc_temp(MirType::Bool);
                ctx.emit(Instruction::Assign {
                    dest: cmp,
                    value: RValue::BinOp {
                        op: MirBinOp::Eq,
                        left: Operand::Local(elem),
                        right: lit,
                    },
                });
                and_into_acc(ctx, acc, Operand::Local(cmp));
            }
            ast::Pattern::Struct {
                name: sub_name,
                fields: sub_fields,
                ..
            } => {
                let elem = ctx.alloc_temp(fty.clone());
                ctx.emit(Instruction::Assign {
                    dest: elem,
                    value: RValue::Field {
                        object: Operand::Local(struct_local),
                        field: fname.clone(),
                    },
                });
                let resolved = if !sub_name.is_empty() {
                    sub_name.clone()
                } else if let MirType::Struct(n) = &fty {
                    n.clone()
                } else {
                    String::new()
                };
                bind_struct_pattern(ctx, elem, &resolved, sub_fields, acc);
            }
            ast::Pattern::Tuple { elements, .. } => {
                let elem = ctx.alloc_temp(fty.clone());
                ctx.emit(Instruction::Assign {
                    dest: elem,
                    value: RValue::Field {
                        object: Operand::Local(struct_local),
                        field: fname.clone(),
                    },
                });
                let sub_elem_tys = match &fty {
                    MirType::Tuple(e) => e.clone(),
                    _ => Vec::new(),
                };
                bind_tuple_pattern(ctx, elem, elements, &sub_elem_tys, acc);
            }
            // Nested enum / or-pattern inside a struct field is rare; treat
            // defensively as always-match (bind nothing) rather than emit a
            // wrong test.
            _ => {}
        }
    }
}

/// Sequential lowering for scalar matches with guards and/or binding arms:
/// each arm becomes test -> (guard ->) body, falling through to the next
/// arm's test. Non-exhaustive matches fall through to a zero result, the
/// same default the switch path uses.
fn lower_match_sequential(
    ctx: &mut LoweringContext,
    subj_op: Operand,
    subj_ty: MirType,
    arms: &[ast::MatchArm],
    result_local: LocalId,
    merge_bb: BlockId,
) -> Operand {
    // Pin the subject so every test reads the same value.
    let subj_local = ctx.alloc_temp(subj_ty);
    ctx.emit(Instruction::Assign {
        dest: subj_local,
        value: RValue::Use(subj_op),
    });

    for arm in arms {
        let body_bb = ctx.alloc_block();
        let next_bb = ctx.alloc_block();

        // 1. Pattern test / binding in the current block.
        let matched: Option<Operand> = match &arm.pattern {
            ast::Pattern::Literal { expr, .. } => {
                let lit_op = lower_expr_to_operand(ctx, expr);
                let cmp = ctx.alloc_temp(MirType::Bool);
                ctx.emit(Instruction::Assign {
                    dest: cmp,
                    value: RValue::BinOp {
                        op: MirBinOp::Eq,
                        left: Operand::Local(subj_local),
                        right: lit_op,
                    },
                });
                Some(Operand::Local(cmp))
            }
            ast::Pattern::Ident { name, mutable, .. } => {
                let bound = ctx.alloc_local(
                    Some(name.clone()),
                    ctx.locals
                        .iter()
                        .find(|l| l.id == subj_local)
                        .map(|l| l.ty.clone())
                        .unwrap_or(MirType::I64),
                    *mutable,
                );
                ctx.emit(Instruction::Assign {
                    dest: bound,
                    value: RValue::Use(Operand::Local(subj_local)),
                });
                None // always matches
            }
            ast::Pattern::Or { patterns, .. } => {
                // Build `subj == alt1 || subj == alt2 || ...` for the (non-binding,
                // validated) alternatives. Previously an or-pattern fell through to
                // the `_ => None` "always matches" case below, so `1 | 2 | 3` (in a
                // match that also had a guard arm, forcing this sequential path)
                // matched EVERY value -- a real miscompile. The comparisons are
                // pure, so eager (non-short-circuit) OR is correct.
                let mut acc: Option<Operand> = None;
                let mut any_always = false;
                for sub in patterns {
                    match sub {
                        ast::Pattern::Literal { expr, .. } => {
                            let lit_op = lower_expr_to_operand(ctx, expr);
                            let cmp = ctx.alloc_temp(MirType::Bool);
                            ctx.emit(Instruction::Assign {
                                dest: cmp,
                                value: RValue::BinOp {
                                    op: MirBinOp::Eq,
                                    left: Operand::Local(subj_local),
                                    right: lit_op,
                                },
                            });
                            let this = Operand::Local(cmp);
                            match acc.take() {
                                None => acc = Some(this),
                                Some(prev) => {
                                    let combined = ctx.alloc_temp(MirType::Bool);
                                    ctx.emit(Instruction::Assign {
                                        dest: combined,
                                        value: RValue::BinOp {
                                            op: MirBinOp::Or,
                                            left: prev,
                                            right: this,
                                        },
                                    });
                                    acc = Some(Operand::Local(combined));
                                }
                            }
                        }
                        // A wildcard/ident alternative matches anything, so the
                        // whole or-pattern always matches.
                        _ => any_always = true,
                    }
                }
                if any_always {
                    None
                } else {
                    acc
                }
            }
            ast::Pattern::Tuple { elements, .. } => {
                // Tuple pattern in the ordered path: recursively extract each
                // element (including through arbitrarily deep nested tuples),
                // bind idents, AND together literal/enum-tag equality tests.
                // Bindings are emitted in this block so a following guard sees
                // them. Returns None (always structurally matches) when every
                // element is an ident/wildcard -- the guard then discriminates.
                let elem_tys = ctx
                    .locals
                    .iter()
                    .find(|l| l.id == subj_local)
                    .and_then(|l| match &l.ty {
                        MirType::Tuple(e) => Some(e.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let mut acc: Option<Operand> = None;
                bind_tuple_pattern(ctx, subj_local, elements, &elem_tys, &mut acc);
                acc
            }
            ast::Pattern::Struct { name, fields, .. } => {
                // Struct pattern in the ordered path: extract each named field,
                // bind idents, AND together literal-field equality tests. An
                // all-ident/wildcard struct pattern is irrefutable (acc None).
                let sname = if !name.is_empty() {
                    name.clone()
                } else {
                    ctx.locals
                        .iter()
                        .find(|l| l.id == subj_local)
                        .and_then(|l| match &l.ty {
                            MirType::Struct(n) => Some(n.clone()),
                            _ => None,
                        })
                        .unwrap_or_default()
                };
                let mut acc: Option<Operand> = None;
                bind_struct_pattern(ctx, subj_local, &sname, fields, &mut acc);
                acc
            }
            // Wildcard and anything else: always matches, binds nothing.
            _ => None,
        };

        // 2. Combine with the guard.
        let cond: Option<Operand> = match (&matched, &arm.guard) {
            (Some(m), None) => Some(m.clone()),
            (None, None) => None,
            (None, Some(g)) => Some(lower_expr_to_operand(ctx, g)),
            (Some(m), Some(g)) => {
                // Pattern test first, then the guard in its own block so the
                // guard only evaluates when the pattern matched.
                let guard_bb = ctx.alloc_block();
                ctx.finish_block(
                    Terminator::Branch {
                        cond: m.clone(),
                        then_block: guard_bb,
                        else_block: next_bb,
                    },
                    guard_bb,
                );
                Some(lower_expr_to_operand(ctx, g))
            }
        };

        match cond {
            Some(c) => {
                ctx.finish_block(
                    Terminator::Branch {
                        cond: c,
                        then_block: body_bb,
                        else_block: next_bb,
                    },
                    body_bb,
                );
            }
            None => {
                ctx.finish_block(Terminator::Goto(body_bb), body_bb);
            }
        }

        // 3. Arm body.
        let body_val = lower_expr_to_operand(ctx, &arm.body);
        // If the arm body MOVES a non-copy local into the result (e.g. a block
        // tail `{ let b = "x"  b }`), mark the source consumed so scope cleanup
        // doesn't double-drop the buffer the result now owns. Mirrors the
        // switch-path arm handling in `lower_match`; without it, once the result
        // is correctly str-typed both the tail local and the result free the
        // same pointer (Pass-18 latent double-free behind the type bug).
        if let Operand::Local(src) = &body_val {
            let src_ty = ctx
                .locals
                .iter()
                .find(|l| l.id == *src)
                .map(|l| l.ty.clone())
                .unwrap_or(MirType::I64);
            if !is_copy_type(ctx, &src_ty) {
                ctx.dropped_locals.insert(src.0);
            }
        }
        ctx.emit(Instruction::Assign {
            dest: result_local,
            value: RValue::Use(body_val),
        });
        ctx.finish_block(Terminator::Goto(merge_bb), next_bb);
    }

    // Fallthrough: nothing matched.
    ctx.emit(Instruction::Assign {
        dest: result_local,
        value: RValue::Use(Operand::Constant(Constant::Int(0))),
    });
    ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
    Operand::Local(result_local)
}

/// True when a sub-pattern inside an enum-variant pattern can FAIL to match
/// (so the arm needs a runtime refinement check, not just payload bindings).
fn is_refutable_subpattern(ctx: &LoweringContext, pat: &ast::Pattern) -> bool {
    match pat {
        ast::Pattern::Literal { .. } => true,
        // A nested variant pattern refines which inner variant matches.
        ast::Pattern::Enum { .. } => true,
        // A bare ident that names a known enum variant is a variant TEST, not
        // a binding (see ident_variant_of). For ROUTING, over-approximating
        // across all enums is safe: if the ident turns out to be a plain
        // binding at emission (type-gated there), the arm simply always
        // matches and the allocated fail target goes unused.
        ast::Pattern::Ident { name, .. } => find_enum_variant(ctx, name).is_some(),
        ast::Pattern::Tuple { elements, .. } => {
            elements.iter().any(|e| is_refutable_subpattern(ctx, e))
        }
        _ => false,
    }
}

/// Recursively destructure `value_op` (of MIR type `value_ty`) against `pat`,
/// emitting payload/field extractions for bindings and refinement checks
/// (inner enum tags, literal equality) that branch to `fail_bb` on mismatch.
///
/// This is what makes NESTED match patterns work: `Wrap(X(v))`, `Some(Some(v))`,
/// `P((a, b))`, `Some(5)`. Previously only top-level `Ident` sub-patterns were
/// bound; nested patterns were silently skipped, so their names resolved to
/// fresh UNINITIALIZED locals (binding 0/garbage) and no inner tag was checked.
fn lower_refutable_bind(
    ctx: &mut LoweringContext,
    value_op: Operand,
    value_ty: &MirType,
    pat: &ast::Pattern,
    fail_bb: BlockId,
) {
    match pat {
        ast::Pattern::Wildcard { .. } => {}
        ast::Pattern::Ident { name, .. } => {
            if let Some((_, vidx)) = ident_variant_of(ctx, name, value_ty) {
                // Bare variant in nested position: inner tag refinement, no binding.
                let tag = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: tag,
                    value: RValue::EnumTag {
                        operand: value_op.clone(),
                    },
                });
                let cmp = ctx.alloc_temp(MirType::Bool);
                ctx.emit(Instruction::Assign {
                    dest: cmp,
                    value: RValue::BinOp {
                        op: MirBinOp::Eq,
                        left: Operand::Local(tag),
                        right: Operand::Constant(Constant::Int(vidx as i64)),
                    },
                });
                let cont = ctx.alloc_block();
                ctx.finish_block(
                    Terminator::Branch {
                        cond: Operand::Local(cmp),
                        then_block: cont,
                        else_block: fail_bb,
                    },
                    cont,
                );
                return;
            }
            let local = ctx.alloc_local(Some(name.clone()), value_ty.clone(), false);
            // Extracted views alias the subject's payload; scope cleanup must
            // not drop them (mirrors the top-level Ident binding path).
            if !is_copy_type(ctx, value_ty) {
                ctx.dropped_locals.insert(local.0);
            }
            ctx.emit(Instruction::Assign {
                dest: local,
                value: RValue::Use(value_op),
            });
        }
        ast::Pattern::Literal { expr, .. } => {
            let lit = match expr.as_ref() {
                ast::Expr::IntLiteral { value, .. } => Some(Constant::Int(*value)),
                ast::Expr::BoolLiteral { value, .. } => Some(Constant::Bool(*value)),
                ast::Expr::FloatLiteral { value, .. } => Some(Constant::Float(*value)),
                ast::Expr::StringLiteral { value, .. } => Some(Constant::Str(value.clone())),
                _ => None,
            };
            if let Some(c) = lit {
                let cmp = ctx.alloc_temp(MirType::Bool);
                ctx.emit(Instruction::Assign {
                    dest: cmp,
                    value: RValue::BinOp {
                        op: MirBinOp::Eq,
                        left: value_op,
                        right: Operand::Constant(c),
                    },
                });
                let cont = ctx.alloc_block();
                ctx.finish_block(
                    Terminator::Branch {
                        cond: Operand::Local(cmp),
                        then_block: cont,
                        else_block: fail_bb,
                    },
                    cont,
                );
            }
        }
        ast::Pattern::Enum {
            name,
            variant,
            fields,
            ..
        } => {
            // Resolve the enum def: prefer the value's (possibly monomorphized)
            // type name, then the pattern's explicit name.
            let ty_name = match value_ty {
                MirType::Enum(n) => Some(n.clone()),
                _ => None,
            };
            let resolved = ty_name
                .clone()
                .filter(|n| ctx.enum_defs.contains_key(n.as_str()))
                .or_else(|| {
                    if !name.is_empty() && ctx.enum_defs.contains_key(name.as_str()) {
                        Some(name.clone())
                    } else {
                        None
                    }
                });
            let Some(resolved) = resolved else { return };
            let Some(idx) = ctx
                .enum_defs
                .get(resolved.as_str())
                .and_then(|vs| vs.iter().position(|v| v.name == *variant))
            else {
                return;
            };
            // Inner tag refinement.
            let tag = ctx.alloc_temp(MirType::I64);
            ctx.emit(Instruction::Assign {
                dest: tag,
                value: RValue::EnumTag {
                    operand: value_op.clone(),
                },
            });
            let cmp = ctx.alloc_temp(MirType::Bool);
            ctx.emit(Instruction::Assign {
                dest: cmp,
                value: RValue::BinOp {
                    op: MirBinOp::Eq,
                    left: Operand::Local(tag),
                    right: Operand::Constant(Constant::Int(idx as i64)),
                },
            });
            let cont = ctx.alloc_block();
            ctx.finish_block(
                Terminator::Branch {
                    cond: Operand::Local(cmp),
                    then_block: cont,
                    else_block: fail_bb,
                },
                cont,
            );
            // Extract + recurse into each payload field.
            for (field_idx, fpat) in fields.iter().enumerate() {
                if matches!(fpat, ast::Pattern::Wildcard { .. }) {
                    continue;
                }
                let field_type = ctx
                    .enum_defs
                    .get(resolved.as_str())
                    .and_then(|vs| vs.get(idx))
                    .and_then(|v| v.fields.get(field_idx))
                    .cloned()
                    .unwrap_or(MirType::I64);
                // Recover enum-typed payloads recorded as Struct(name) (see the
                // identical recovery in the top-level binding path); recursive
                // so a `(Enum, i64)` tuple payload doesn't keep an opaque
                // Struct label inside the tuple (unsized %Name on AOT).
                let field_type = recover_enum_types(ctx, field_type);
                // A bare ident naming a variant of the field's own enum type
                // is a variant TEST (recursed below), not a binding. Without
                // this, a depth-3 nested bare leaf (`L3a(L2a(L1a))`) became a
                // binding, so a sibling arm sharing the 2-level prefix never
                // discriminated -- the bare arm swallowed the payload arm.
                let bind_name = match fpat {
                    ast::Pattern::Ident { name: bn, .. }
                        if ident_variant_of(ctx, bn, &field_type).is_none() =>
                    {
                        Some(bn.clone())
                    }
                    _ => None,
                };
                let dest = if let Some(bn) = &bind_name {
                    let l = ctx.alloc_local(Some(bn.clone()), field_type.clone(), false);
                    if !is_copy_type(ctx, &field_type) {
                        ctx.dropped_locals.insert(l.0);
                    }
                    l
                } else {
                    let t = ctx.alloc_temp(field_type.clone());
                    if !is_copy_type(ctx, &field_type) {
                        ctx.dropped_locals.insert(t.0);
                    }
                    t
                };
                ctx.emit(Instruction::Assign {
                    dest,
                    value: RValue::EnumPayload {
                        operand: value_op.clone(),
                        enum_name: resolved.clone(),
                        variant_idx: idx as u32,
                        field_idx: field_idx as u32,
                    },
                });
                if bind_name.is_none() {
                    lower_refutable_bind(ctx, Operand::Local(dest), &field_type, fpat, fail_bb);
                }
            }
        }
        ast::Pattern::Tuple { elements, .. } => {
            let elem_tys = match value_ty {
                MirType::Tuple(e) => e.clone(),
                _ => Vec::new(),
            };
            for (elem_idx, epat) in elements.iter().enumerate() {
                if matches!(epat, ast::Pattern::Wildcard { .. }) {
                    continue;
                }
                let ety = elem_tys.get(elem_idx).cloned().unwrap_or(MirType::I64);
                // A bare ident that names a variant of the element's enum type
                // is a variant test, not a binding: recurse so the Ident arm
                // above emits the tag refinement.
                let bind_name = match epat {
                    ast::Pattern::Ident { name: bn, .. }
                        if ident_variant_of(ctx, bn, &ety).is_none() =>
                    {
                        Some(bn.clone())
                    }
                    _ => None,
                };
                let dest = if let Some(bn) = &bind_name {
                    let l = ctx.alloc_local(Some(bn.clone()), ety.clone(), false);
                    if !is_copy_type(ctx, &ety) {
                        ctx.dropped_locals.insert(l.0);
                    }
                    l
                } else {
                    let t = ctx.alloc_temp(ety.clone());
                    if !is_copy_type(ctx, &ety) {
                        ctx.dropped_locals.insert(t.0);
                    }
                    t
                };
                ctx.emit(Instruction::Assign {
                    dest,
                    value: RValue::Field {
                        object: value_op.clone(),
                        field: elem_idx.to_string(),
                    },
                });
                if bind_name.is_none() {
                    lower_refutable_bind(ctx, Operand::Local(dest), &ety, epat, fail_bb);
                }
            }
        }
        // Or / Struct sub-patterns: unsupported in nested position (kept at
        // today's bind-nothing behavior; the checker constrains these).
        _ => {}
    }
}

/// Infer the value-type a `match` expression produces, from its first arm's
/// body expression.
///
/// For enum arms where the body is a simple identifier that will be bound
/// from a field extraction, look up the field type directly -- the local
/// does not exist yet when `infer_expr_type` runs on the body alone, so it
/// would fall back to I64 even for a non-i64 field (e.g. `JsonValue::Number(n)
/// => n`, or a match-bound `str` payload used in a chained concatenation).
///
/// Shared by `lower_match` (which needs it to size the real result local)
/// and `infer_expr_type`'s own `MatchExpr` arm (used when a match expression
/// is nested inside a larger expression, e.g. `let name = match u { .. }`
/// sizes the `name` local via this same path) -- both must agree, or the
/// `name` local and the match's own result local can end up with different
/// declared types for the same value, one of which is used as an operand of
/// a chained string concat that discovers only at LLVM codegen time it grew
/// a `ptr` value under an `i64` label.
/// A match/if arm body that DIVERGES (its value position is a `throw` or
/// `return`) contributes no value type to the result. The parser lowers a
/// `throw`/`return` arm body into a `Block` whose final statement is
/// `Stmt::Throw`/`Stmt::Return`, so `infer_expr_type` on it yields `Void` --
/// which must NOT become the whole match's result type when a sibling arm
/// produces a real value (`let r = match x { 0 => throw .., _ => "s" }` is a
/// `str`, not void). Skip such arms when picking the type-carrying arm.
fn arm_body_diverges(body: &ast::Expr) -> bool {
    match body {
        ast::Expr::Block { block, .. } => matches!(
            block.stmts.last(),
            Some(ast::Stmt::Throw { .. }) | Some(ast::Stmt::Return { .. })
        ),
        _ => false,
    }
}

/// Temporarily register an arm PATTERN's ident bindings as synthetic locals
/// (typed from the subject/payload) so pre-lowering inference can resolve an
/// arm BODY that COMPUTES on a binding -- `match s { v => v + "!" }` and
/// `t if len(t) > 5 => t + "-long"` typed the whole match i64 (the str result
/// was stored into an i64 slot and printed as a raw pointer, both backends).
/// Mirrors the block_tail_let_type synthetic-local overlay; the caller
/// truncates ctx.locals and restores ctx.next_local afterwards.
fn overlay_arm_bindings(ctx: &mut LoweringContext, ty: &MirType, pat: &ast::Pattern) {
    match pat {
        ast::Pattern::Ident { name, .. } => {
            // A bare variant name is a tag test, not a binding.
            if ident_variant_of(ctx, name, ty).is_none() {
                ctx.alloc_local(Some(name.clone()), ty.clone(), false);
            }
        }
        ast::Pattern::Tuple { elements, .. } => {
            let elem_tys = match ty {
                MirType::Tuple(e) => e.clone(),
                _ => Vec::new(),
            };
            for (i, ep) in elements.iter().enumerate() {
                let ety = elem_tys.get(i).cloned().unwrap_or(MirType::I64);
                overlay_arm_bindings(ctx, &ety, ep);
            }
        }
        ast::Pattern::Enum {
            name,
            variant,
            fields,
            ..
        } => {
            let resolved = match ty {
                MirType::Enum(n) => Some(n.clone()),
                _ if !name.is_empty() => Some(name.clone()),
                _ => find_enum_variant(ctx, variant).map(|(en, _)| en),
            };
            let Some(vs) = resolved.and_then(|en| ctx.enum_defs.get(en.as_str()).cloned()) else {
                return;
            };
            let Some(idx) = vs.iter().position(|v| v.name == *variant) else {
                return;
            };
            for (fi, fp) in fields.iter().enumerate() {
                let fty = vs[idx].fields.get(fi).cloned().unwrap_or(MirType::I64);
                let fty = recover_enum_types(ctx, fty);
                overlay_arm_bindings(ctx, &fty, fp);
            }
        }
        ast::Pattern::Struct { name, fields, .. } => {
            let sname = match ty {
                MirType::Struct(n) => n.clone(),
                _ => name.clone(),
            };
            let field_defs = ctx.struct_defs.get(&sname).cloned().unwrap_or_default();
            for (fname, fpat) in fields {
                let fty = field_defs
                    .iter()
                    .find(|(n, _)| n == fname)
                    .map(|(_, t)| t.clone())
                    .unwrap_or(MirType::I64);
                overlay_arm_bindings(ctx, &fty, fpat);
            }
        }
        _ => {}
    }
}

fn infer_match_result_type(
    ctx: &mut LoweringContext,
    subject: &ast::Expr,
    arms: &[ast::MatchArm],
) -> MirType {
    arms.iter()
        .find(|a| !arm_body_diverges(&a.body))
        .or_else(|| arms.first())
        .map(|arm| {
            if let ast::Pattern::Enum {
                name: enum_name,
                variant,
                fields,
                ..
            } = &arm.pattern
            {
                if let ast::Expr::Identifier {
                    name: body_name, ..
                } = &*arm.body
                {
                    // Resolve the enum definition to consult. A QUALIFIED
                    // pattern (`Option.Some(n)` / `Opt::Some(n)`) carries the
                    // enum name directly. A BARE pattern (`Some(n)`) leaves
                    // `enum_name` empty by parser convention (parser.rs:
                    // "the enum is left empty and resolved from the matched
                    // value's type during lowering") -- looking that empty
                    // name up in `ctx.enum_defs` silently misses, falling
                    // through to `infer_expr_type(&arm.body)` below, which
                    // can't see `body_name` (not bound yet) and defaults to
                    // I64. Prefer the SUBJECT's own inferred type when it
                    // names a (possibly monomorphized, e.g. `Option___str`)
                    // enum, so the payload field type is the real concrete
                    // type rather than the generic stub's i64 placeholder;
                    // fall back to the pattern's name, then to a bare-variant
                    // lookup.
                    let resolved_enum_name = match infer_expr_type(ctx, subject) {
                        MirType::Enum(n) => Some(n),
                        _ if !enum_name.is_empty() => Some(enum_name.clone()),
                        _ => find_enum_variant(ctx, variant).map(|(en, _)| en),
                    };
                    if let Some(variants) =
                        resolved_enum_name.and_then(|en| ctx.enum_defs.get(en.as_str()).cloned())
                    {
                        if let Some(idx) = variants.iter().position(|v| v.name == *variant) {
                            for (field_idx, pat) in fields.iter().enumerate() {
                                if let ast::Pattern::Ident {
                                    name: field_name, ..
                                } = pat
                                {
                                    if field_name == body_name {
                                        if let Some(ft) = variants[idx].fields.get(field_idx) {
                                            return ft.clone();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Overlay the arm pattern's bindings (typed from the subject) so a
            // body that COMPUTES on a binding resolves -- without this,
            // `v => v + "!"` over a str subject fell to i64 and the match
            // result slot stored the str as a raw pointer (both backends).
            let subj_ty = infer_expr_type(ctx, subject);
            let saved_len = ctx.locals.len();
            let saved_next = ctx.next_local;
            overlay_arm_bindings(ctx, &subj_ty, &arm.pattern);
            let inferred = infer_expr_type(ctx, &arm.body);
            ctx.locals.truncate(saved_len);
            ctx.next_local = saved_next;
            inferred
        })
        .unwrap_or(MirType::I64)
}

fn lower_match(ctx: &mut LoweringContext, subject: &ast::Expr, arms: &[ast::MatchArm]) -> Operand {
    let subj_op = lower_expr_to_operand(ctx, subject);
    let result_ty = infer_match_result_type(ctx, subject, arms);
    let result_local = ctx.alloc_temp(result_ty);
    let merge_bb = ctx.alloc_block();

    // Scalar matches containing guards or binding arms cannot go through the
    // switch-table path below: it silently turned binding arms into a
    // "default" that a later wildcard OVERWROTE, and it never lowered guards
    // at all (`x if x % 5 == 0` fell to the wildcard). Lower those as a
    // sequential test chain instead. Enum/struct/tuple matches keep the
    // switch path (guards there are still unsupported and now rejected by
    // exhaustiveness of this gate going first).
    {
        let subj_ty_early = infer_expr_type(ctx, subject);
        // Enum/struct matches must stay on the tag-dispatch switch path.
        let is_enum_or_struct = matches!(subj_ty_early, MirType::Enum(_))
            || arms.iter().any(|a| {
                matches!(
                    &a.pattern,
                    ast::Pattern::Enum { .. } | ast::Pattern::Struct { .. }
                )
            });
        // A tuple match is sequential-safe when every element sub-pattern is
        // safe at ANY nesting depth (literal / ident / wildcard, an enum with
        // simple fields, or a nested tuple whose own elements are all
        // recursively safe) -- see tuple_elem_seq_safe. A nested refutable
        // element that isn't (e.g. `(Some(x), y)` with a compound payload, or
        // an or-pattern anywhere) still needs the switch path's refinement
        // machinery.
        let tuple_seq_safe = arms.iter().all(|a| match &a.pattern {
            ast::Pattern::Tuple { elements, .. } => {
                elements.iter().all(tuple_elem_seq_safe)
            }
            _ => true,
        });
        // A tuple match with a nested-enum OR nested-tuple element (at ANY
        // depth) MUST use the sequential path even without guards: the switch
        // path only binds top-level Ident elements (see the tuple_binding
        // loop below) -- it tested neither a nested enum's tag (`No` took the
        // `Yes` arm; payload bound 0) nor extracted a nested tuple's own
        // elements (`a`, `b` bound 0) -- silent wrong output on both backends.
        // Checking only the immediate elements is depth-agnostic by
        // construction: a sub-element that is itself a Tuple (however deeply
        // IT nests further) already matches `Pattern::Tuple { .. }` here, so
        // `(n, (a, (c, d)), t)` is flagged by its immediate middle element
        // without needing to recurse into `(a, (c, d))`.
        let has_nested_refutable_tuple = arms.iter().any(|a| match &a.pattern {
            ast::Pattern::Tuple { elements, .. } => elements.iter().any(|e| {
                matches!(e, ast::Pattern::Enum { .. } | ast::Pattern::Tuple { .. })
            }),
            _ => false,
        });
        let needs_sequential = arms.iter().any(|a| {
            a.guard.is_some() || matches!(&a.pattern, ast::Pattern::Ident { .. })
        }) || has_nested_refutable_tuple;
        // Route to the sequential (ordered test-chain) path for scalar AND
        // simple-tuple matches. Previously tuple matches were forced onto the
        // switch path, which could not chain multiple same-shape guarded arms:
        // `match (x,y) { (a,b) if a>0 && b>0 => .., (a,b) if a<0 && b>0 => .. }`
        // routed every `(a,b)` to the first arm's block, so a failed guard fell
        // to the default instead of the next guarded arm (silent wrong output).
        if needs_sequential && !is_enum_or_struct && tuple_seq_safe {
            return lower_match_sequential(ctx, subj_op, subj_ty_early, arms, result_local, merge_bb);
        }
        // A STRUCT match has no tag to switch on. Route it to the sequential
        // path, which extracts and binds/tests struct fields. Struct patterns
        // were previously forced onto the enum switch path (subj_enum_name=None
        // -> degenerate switch), so field bindings read 0 and literal-field arms
        // never matched -- a silent both-backend miscompile of a core feature.
        let has_struct_arm = arms
            .iter()
            .any(|a| matches!(&a.pattern, ast::Pattern::Struct { .. }));
        let has_enum_arm = arms
            .iter()
            .any(|a| matches!(&a.pattern, ast::Pattern::Enum { .. }));
        if has_struct_arm && !has_enum_arm && !matches!(subj_ty_early, MirType::Enum(_)) {
            return lower_match_sequential(ctx, subj_op, subj_ty_early, arms, result_local, merge_bb);
        }
    }

    // Detect enum match: either explicit Pattern::Enum, or bare ident patterns
    // where the subject type is an enum (e.g., `match c { Red => ... }`).
    let subj_ty = infer_expr_type(ctx, subject);
    let subj_enum_name = match &subj_ty {
        MirType::Enum(name) => Some(name.clone()),
        _ => None,
    }
    .or_else(|| {
        // The subject type may not surface as MirType::Enum even when it IS an
        // enum -- e.g. `match m[k] { Some(v) => .. }` where m: map<_, Option<_>>.
        // infer_expr_type returns the map's value type, which the enum-arm target
        // collection could not key on, so the switch was emitted with ZERO cases
        // and every value fell to the default arm (a map-stored Some/Ok read back
        // as None/Err). Fall back to the enum named by the first enum-pattern arm.
        arms.iter().find_map(|a| match &a.pattern {
            ast::Pattern::Enum { name, variant, .. } => {
                if !name.is_empty() {
                    Some(name.clone())
                } else {
                    find_enum_variant(ctx, variant).map(|(en, _)| en)
                }
            }
            _ => None,
        })
    });
    let is_enum_match = arms.iter().any(|a| match &a.pattern {
        ast::Pattern::Enum { .. } => true,
        // Or-patterns whose alternatives are enum variants must also trigger
        // tag extraction — otherwise the raw enum pointer is used as the
        // switch value instead of the loaded discriminant, causing a SIGILL
        // when comparing a heap address against small integer constants.
        ast::Pattern::Or { patterns, .. } => {
            patterns.iter().any(|p| matches!(p, ast::Pattern::Enum { .. }))
        }
        _ => false,
    }) || (subj_enum_name.is_some()
        && arms.iter().any(|a| match &a.pattern {
            ast::Pattern::Ident { .. } => true,
            ast::Pattern::Or { patterns, .. } => {
                patterns.iter().any(|p| matches!(p, ast::Pattern::Ident { .. }))
            }
            _ => false,
        }));

    // For enum matches, extract the tag first and switch on that.
    let switch_op = if is_enum_match {
        let tag_local = ctx.alloc_temp(MirType::I64);
        ctx.emit(Instruction::Assign {
            dest: tag_local,
            value: RValue::EnumTag {
                operand: subj_op.clone(),
            },
        });
        Operand::Local(tag_local)
    } else {
        subj_op.clone()
    };

    // Collect arms into switch targets.
    let mut targets: Vec<(i64, BlockId)> = Vec::new();
    let mut string_targets: Vec<(String, BlockId)> = Vec::new();
    let mut tuple_targets: Vec<(Vec<ast::Pattern>, BlockId)> = Vec::new();
    let mut arm_blocks: Vec<(BlockId, &ast::Expr, Option<EnumBinding>, Option<TupleBinding>)> =
        Vec::new();
    let mut default_arm: Option<(BlockId, &ast::Expr)> = None;
    // Enum being matched, used to decide exhaustiveness for the switch default.
    let mut enum_for_exhaustiveness: Option<String> = subj_enum_name.clone();
    // Guards on STRUCTURED arms (enum/tuple/struct patterns). The switch path
    // dispatches purely on tag, so a guard on such an arm was silently dropped
    // (`match e { A(x) if x>5 => .., A(x) => .. }` took the first A arm even for
    // x<=5). Keyed by arm block; evaluated after binding, and a false guard
    // falls through to the next same-tag arm / default via the fail-chain.
    let mut arm_guards: std::collections::HashMap<u32, &ast::Expr> =
        std::collections::HashMap::new();

    for arm in arms {
        let arm_bb = ctx.alloc_block();
        if let Some(g) = arm.guard.as_deref() {
            arm_guards.insert(arm_bb.0, g);
        }
        match &arm.pattern {
            ast::Pattern::Enum {
                name,
                variant,
                fields,
                ..
            } => {
                // Bare (unqualified) variant patterns carry an empty enum name;
                // resolve it from the subject's enum type.
                let resolved_name = if name.is_empty() {
                    subj_enum_name.clone().unwrap_or_else(|| name.clone())
                } else {
                    name.clone()
                };
                if let Some(variants) = ctx.enum_defs.get(resolved_name.as_str()) {
                    if let Some(idx) = variants.iter().position(|v| v.name == *variant) {
                        // One switch case per tag: a second arm with the same
                        // outer variant (e.g. `Wrap(X(v))` then `Wrap(Y(w))`)
                        // is reached via the refinement fail-chain, not a
                        // duplicate switch entry (which is invalid in codegen).
                        if !targets.iter().any(|(t, _)| *t == idx as i64) {
                            targets.push((idx as i64, arm_bb));
                        }
                        enum_for_exhaustiveness = Some(resolved_name.clone());
                        arm_blocks.push((
                            arm_bb,
                            &arm.body,
                            Some(EnumBinding {
                                enum_name: resolved_name.clone(),
                                variant_idx: idx as u32,
                                field_patterns: fields.clone(),
                            }),
                            None,
                        ));
                    } else {
                        default_arm = Some((arm_bb, &arm.body));
                    }
                } else {
                    default_arm = Some((arm_bb, &arm.body));
                }
            }
            ast::Pattern::Literal { expr, .. } => {
                if let ast::Expr::IntLiteral { value, .. } = expr.as_ref() {
                    targets.push((*value, arm_bb));
                    arm_blocks.push((arm_bb, &arm.body, None, None));
                } else if let ast::Expr::StringLiteral { value, .. } = expr.as_ref() {
                    string_targets.push((value.clone(), arm_bb));
                    arm_blocks.push((arm_bb, &arm.body, None, None));
                } else if let ast::Expr::BoolLiteral { value, .. } = expr.as_ref() {
                    // Bool patterns: compile as integer switch where true=1, false=0.
                    // The subject is already i8 (Cranelift's bool repr); the codegen's
                    // Switch terminator sizes the case constants to the subject's type.
                    targets.push((if *value { 1 } else { 0 }, arm_bb));
                    arm_blocks.push((arm_bb, &arm.body, None, None));
                } else {
                    default_arm = Some((arm_bb, &arm.body));
                }
            }
            ast::Pattern::Ident { name, .. } => {
                // Check if this ident matches an enum variant of the subject type.
                let mut matched = false;
                if let Some(ref enum_name) = subj_enum_name {
                    if let Some(variants) = ctx.enum_defs.get(enum_name.as_str()) {
                        if let Some(idx) = variants.iter().position(|v| v.name == *name) {
                            if !targets.iter().any(|(t, _)| *t == idx as i64) {
                                targets.push((idx as i64, arm_bb));
                            }
                            arm_blocks.push((arm_bb, &arm.body, None, None));
                            matched = true;
                        }
                    }
                }
                if !matched {
                    default_arm = Some((arm_bb, &arm.body));
                }
            }
            ast::Pattern::Or { patterns, .. } => {
                // Or-pattern: every alternative routes to the same arm block.
                // Only non-binding alternatives (int/bool/string literals and
                // bare enum variants) are supported; anything that binds, or an
                // unresolvable alternative, makes the whole arm a default.
                let mut handled = !patterns.is_empty();
                for sub in patterns {
                    match sub {
                        ast::Pattern::Literal { expr, .. } => match expr.as_ref() {
                            ast::Expr::IntLiteral { value, .. } => {
                                targets.push((*value, arm_bb))
                            }
                            ast::Expr::BoolLiteral { value, .. } => {
                                targets.push((if *value { 1 } else { 0 }, arm_bb))
                            }
                            ast::Expr::StringLiteral { value, .. } => {
                                string_targets.push((value.clone(), arm_bb))
                            }
                            _ => handled = false,
                        },
                        ast::Pattern::Enum {
                            name,
                            variant,
                            fields,
                            ..
                        } if fields.is_empty() => {
                            let resolved = if name.is_empty() {
                                subj_enum_name.clone().unwrap_or_default()
                            } else {
                                name.clone()
                            };
                            match ctx
                                .enum_defs
                                .get(resolved.as_str())
                                .and_then(|vs| vs.iter().position(|v| v.name == *variant))
                            {
                                Some(idx) => {
                                    targets.push((idx as i64, arm_bb));
                                    enum_for_exhaustiveness = Some(resolved.clone());
                                }
                                None => handled = false,
                            }
                        }
                        ast::Pattern::Ident { name, .. } => {
                            match subj_enum_name
                                .as_deref()
                                .and_then(|en| ctx.enum_defs.get(en))
                                .and_then(|vs| vs.iter().position(|v| v.name == *name))
                            {
                                Some(idx) => targets.push((idx as i64, arm_bb)),
                                None => handled = false,
                            }
                        }
                        _ => handled = false,
                    }
                }
                if handled {
                    arm_blocks.push((arm_bb, &arm.body, None, None));
                } else {
                    default_arm = Some((arm_bb, &arm.body));
                }
            }
            ast::Pattern::Tuple { elements, .. } => {
                // Tuple-literal match arm `(a, b, ..) => ...`. The terminator
                // emission builds a comparison chain over the literal elements;
                // ident elements are bound in the per-arm emission loop.
                tuple_targets.push((elements.clone(), arm_bb));
                arm_blocks.push((
                    arm_bb,
                    &arm.body,
                    None,
                    Some(TupleBinding {
                        element_patterns: elements.clone(),
                    }),
                ));
            }
            ast::Pattern::Wildcard { .. } => {
                default_arm = Some((arm_bb, &arm.body));
            }
            _ => {
                default_arm = Some((arm_bb, &arm.body));
            }
        }
    }

    // When the match has no explicit default/wildcard arm AND it exhaustively
    // covers an enum's variants, the switch's default case is unreachable.
    // Routing it to `merge_bb` (the old behaviour) makes merge a successor of
    // the switch block, so any value an arm binds and merge reads no longer
    // dominates its use under LLVM's strict SSA (e.g. the `?`-operator desugar,
    // or a function whose body is a fully-diverging match returning an enum).
    // Send it to a dedicated `Unreachable` block instead.
    let is_exhaustive_enum = default_arm.is_none()
        && enum_for_exhaustiveness
            .as_deref()
            .and_then(|n| ctx.enum_defs.get(n))
            .map(|vs| targets.len() >= vs.len())
            .unwrap_or(false);
    let unreachable_default = if is_exhaustive_enum {
        Some(ctx.alloc_block())
    } else {
        None
    };
    // A match with NO wildcard/default arm that is NOT statically exhaustive
    // (an int/str match, or an enum match missing variants) must PANIC at
    // runtime if no arm matches. Routing the switch/compare default to
    // `merge_bb` read back an UNINITIALIZED `result_local` -- a silent
    // non-exhaustive miss that yielded garbage (0 / empty string) instead of
    // failing loud (`match n { 1 => .., 2 => .. }` on 99 returned 0). Route it
    // to a dedicated panic block (sealed below like `refut_nomatch_block`).
    let nonexhaustive_nomatch_block = if default_arm.is_none() && unreachable_default.is_none() {
        Some(ctx.alloc_block())
    } else {
        None
    };
    let default_bb = default_arm
        .map(|(bb, _)| bb)
        .or(unreachable_default)
        .or(nonexhaustive_nomatch_block)
        .unwrap_or(merge_bb);

    // Fail targets for arms with REFUTABLE sub-patterns (nested variants,
    // literal payloads, tuple payloads with literals). A refuted refinement
    // falls to the next arm with the same outer tag, else the default arm,
    // else a synthetic no-match block. It must never fall to the switch's
    // unreachable-default: outer-tag exhaustiveness does not make nested
    // refinements exhaustive.
    let mut refut_nomatch_block: Option<BlockId> = None;
    let mut arm_fail_targets: Vec<Option<BlockId>> = vec![None; arm_blocks.len()];
    for i in 0..arm_blocks.len() {
        let Some(eb) = &arm_blocks[i].2 else { continue };
        // An arm needs a fail target if a nested sub-pattern can be refuted OR
        // it carries a guard (a false guard falls through like a refutation).
        let has_guard = arm_guards.contains_key(&arm_blocks[i].0 .0);
        let has_refutable = eb
            .field_patterns
            .iter()
            .any(|p| is_refutable_subpattern(ctx, p));
        if !has_guard && !has_refutable {
            continue;
        }
        let mut fail = None;
        for arm_j in arm_blocks.iter().skip(i + 1) {
            if let Some(eb2) = &arm_j.2 {
                if eb2.enum_name == eb.enum_name && eb2.variant_idx == eb.variant_idx {
                    fail = Some(arm_j.0);
                    break;
                }
            }
        }
        let fail = fail.or_else(|| default_arm.map(|(db, _)| db)).unwrap_or_else(|| {
            *refut_nomatch_block.get_or_insert_with(|| ctx.alloc_block())
        });
        arm_fail_targets[i] = Some(fail);
    }

    // Emit terminator: string patterns use an equality-comparison chain
    // (strings can't go through integer Switch), integer patterns use Switch.
    if !string_targets.is_empty() {
        // Chain of BinOp::Eq comparisons with Branch terminators.
        for (i, (ref pat_str, arm_bb)) in string_targets.iter().enumerate() {
            let cmp_local = ctx.alloc_temp(MirType::Bool);
            ctx.emit(Instruction::Assign {
                dest: cmp_local,
                value: RValue::BinOp {
                    op: MirBinOp::Eq,
                    left: switch_op.clone(),
                    right: Operand::Constant(Constant::Str(pat_str.clone())),
                },
            });
            if i + 1 < string_targets.len() {
                // More string patterns to check — allocate a continuation block.
                let nb = ctx.alloc_block();
                ctx.finish_block(
                    Terminator::Branch {
                        cond: Operand::Local(cmp_local),
                        then_block: *arm_bb,
                        else_block: nb,
                    },
                    nb,
                );
            } else {
                // Last string pattern — fall through to default on mismatch.
                let first_arm = arm_blocks
                    .first()
                    .map(|(bb, _, _, _)| *bb)
                    .unwrap_or(default_bb);
                ctx.finish_block(
                    Terminator::Branch {
                        cond: Operand::Local(cmp_local),
                        then_block: *arm_bb,
                        else_block: default_bb,
                    },
                    first_arm,
                );
            }
        }
    } else if !tuple_targets.is_empty() {
        // Tuple-literal patterns: extract each element of the subject tuple and
        // compare the literal elements (ident/wildcard elements impose no test).
        // Each arm ANDs its element equalities and branches to the arm on a full
        // match, else to the next arm's test (or the default). Modeled on the
        // string-equality chain above.
        let subj_ty = infer_expr_type(ctx, subject);
        let elem_tys = if let MirType::Tuple(elems) = subj_ty {
            elems
        } else {
            Vec::new()
        };
        let n = tuple_targets.len();
        for (i, (elem_pats, arm_bb)) in tuple_targets.iter().enumerate() {
            let mut cond: Option<LocalId> = None;
            for (idx, pat) in elem_pats.iter().enumerate() {
                let lit = if let ast::Pattern::Literal { expr, .. } = pat {
                    match expr.as_ref() {
                        ast::Expr::IntLiteral { value, .. } => Some(Constant::Int(*value)),
                        ast::Expr::BoolLiteral { value, .. } => Some(Constant::Bool(*value)),
                        ast::Expr::FloatLiteral { value, .. } => Some(Constant::Float(*value)),
                        ast::Expr::StringLiteral { value, .. } => {
                            Some(Constant::Str(value.clone()))
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                let elem_ty = elem_tys.get(idx).cloned().unwrap_or(MirType::I64);
                // A bare ident naming a variant of the element's enum type is a
                // tag test, not an unconditional binding (else `(Red, Red)` vs
                // `(Green, Green)` never discriminated and arm 1 always won).
                let variant = if let ast::Pattern::Ident { name, .. } = pat {
                    ident_variant_of(ctx, name, &elem_ty)
                } else {
                    None
                };
                if lit.is_some() || variant.is_some() {
                    let field_local = ctx.alloc_temp(elem_ty.clone());
                    if !is_copy_type(ctx, &elem_ty) {
                        ctx.dropped_locals.insert(field_local.0);
                    }
                    ctx.emit(Instruction::Assign {
                        dest: field_local,
                        value: RValue::Field {
                            object: subj_op.clone(),
                            field: idx.to_string(),
                        },
                    });
                    let cmp_local = ctx.alloc_temp(MirType::Bool);
                    if let Some(lit_const) = lit {
                        ctx.emit(Instruction::Assign {
                            dest: cmp_local,
                            value: RValue::BinOp {
                                op: MirBinOp::Eq,
                                left: Operand::Local(field_local),
                                right: Operand::Constant(lit_const),
                            },
                        });
                    } else if let Some((_, vidx)) = variant {
                        let tag = ctx.alloc_temp(MirType::I64);
                        ctx.emit(Instruction::Assign {
                            dest: tag,
                            value: RValue::EnumTag {
                                operand: Operand::Local(field_local),
                            },
                        });
                        ctx.emit(Instruction::Assign {
                            dest: cmp_local,
                            value: RValue::BinOp {
                                op: MirBinOp::Eq,
                                left: Operand::Local(tag),
                                right: Operand::Constant(Constant::Int(vidx as i64)),
                            },
                        });
                    }
                    cond = Some(match cond {
                        None => cmp_local,
                        Some(prev) => {
                            let anded = ctx.alloc_temp(MirType::Bool);
                            ctx.emit(Instruction::Assign {
                                dest: anded,
                                value: RValue::BinOp {
                                    op: MirBinOp::And,
                                    left: Operand::Local(prev),
                                    right: Operand::Local(cmp_local),
                                },
                            });
                            anded
                        }
                    });
                }
            }
            // No literal elements (all idents/wildcards) -> matches unconditionally.
            let cond_op = match cond {
                Some(c) => Operand::Local(c),
                None => Operand::Constant(Constant::Bool(true)),
            };
            if i + 1 < n {
                let nb = ctx.alloc_block();
                ctx.finish_block(
                    Terminator::Branch {
                        cond: cond_op,
                        then_block: *arm_bb,
                        else_block: nb,
                    },
                    nb,
                );
            } else {
                let first_arm = arm_blocks
                    .first()
                    .map(|(bb, _, _, _)| *bb)
                    .unwrap_or(default_bb);
                ctx.finish_block(
                    Terminator::Branch {
                        cond: cond_op,
                        then_block: *arm_bb,
                        else_block: default_bb,
                    },
                    first_arm,
                );
            }
        }
    } else {
        ctx.finish_block(
            Terminator::Switch {
                value: switch_op,
                targets,
                default: default_bb,
            },
            if let Some((bb, _, _, _)) = arm_blocks.first() {
                *bb
            } else {
                default_bb
            },
        );
    }

    // Emit each arm block.
    for (i, (arm_bb, body, enum_binding, tuple_binding)) in arm_blocks.iter().enumerate() {
        if i > 0 {
            ctx.current_block = *arm_bb;
        }

        // Pattern bindings (enum payload, tuple elements) for THIS arm are
        // allocated in the enclosing scope below; hide them once the arm body
        // is lowered so they don't shadow an outer same-named binding after
        // the match (`let x = 999  match v { Circle(x) => .. }  x` must still
        // read 999, and a type-changed shadow must not reinterpret memory).
        let arm_scope_start = ctx.locals.len();

        // Str/array/map Ident-bound payload locals that got their OWN
        // independent retain (below) and so must get an EXPLICIT Drop
        // scoped to precisely THIS arm's own block -- see the long comment
        // at the retain/suppress site for why a retain happens, and why
        // this can't just rely on the generic named-local scope-end Drop
        // (lower_block_stmts): that mechanism fires unconditionally once
        // per local, but a payload binding is only ASSIGNED when ITS OWN
        // arm is taken. A match with multiple arms sharing one merge block
        // (e.g. `Tag(s) => .., Tag2(s2, arr) => .., _ => {}`) reaches that
        // merge on EVERY iteration regardless of which arm ran, so an
        // unconditional drop(s) there fires even on an iteration that took
        // Tag2 or the wildcard -- freeing `s`'s STALE value from a PRIOR
        // iteration a second time (confirmed double-free via
        // KRYOS_FREE_DIAG: `Tag(s)|Tag2(s2,arr)|_` alternating in a loop,
        // and `Option<str>` `Some(v)|None()` alternating in a loop, both
        // reported str/array DOUBLE-FREE once the naive fix let the
        // generic mechanism drop them at the shared merge point). Emitting
        // the drop here instead -- inside the SAME arm block that did the
        // retain, right after the arm body runs -- guarantees it only
        // fires on the exact control-flow path that owns a real retained
        // reference this iteration.
        let mut arm_owned_payload_locals: Vec<LocalId> = Vec::new();

        // For tuple arms, bind ident elements to the corresponding tuple fields.
        if let Some(binding) = tuple_binding {
            let subj_ty = infer_expr_type(ctx, subject);
            let elem_tys = if let MirType::Tuple(elems) = subj_ty {
                elems
            } else {
                Vec::new()
            };
            for (elem_idx, pat) in binding.element_patterns.iter().enumerate() {
                if let ast::Pattern::Ident { name, .. } = pat {
                    let elem_ty = elem_tys.get(elem_idx).cloned().unwrap_or(MirType::I64);
                    // A bare variant element was already tag-tested in the
                    // comparison chain; it binds nothing.
                    if ident_variant_of(ctx, name, &elem_ty).is_some() {
                        continue;
                    }
                    let local = ctx.alloc_local(Some(name.clone()), elem_ty.clone(), false);
                    if !is_copy_type(ctx, &elem_ty) {
                        ctx.dropped_locals.insert(local.0);
                    }
                    ctx.emit(Instruction::Assign {
                        dest: local,
                        value: RValue::Field {
                            object: subj_op.clone(),
                            field: elem_idx.to_string(),
                        },
                    });
                }
                // Literal/wildcard elements: no binding needed.
            }
        }

        // For enum arms, extract payload fields and bind to locals.
        if let Some(binding) = enum_binding {
            // Prefer the SUBJECT's (possibly monomorphized) enum type for payload
            // field types. `binding.enum_name` is the bare generic name ("Result")
            // whose def erases payloads to i64, but the monomorphized def
            // ("Result___i64_str") carries the real types (Err: str). Without this,
            // a directly-used payload binding mis-typed to i64 -> e.g.
            // `match r { Err(e) => println(e) }` printed the str handle as a number.
            let subj_enum_name = match infer_expr_type(ctx, subject) {
                MirType::Enum(n) => n,
                _ => binding.enum_name.clone(),
            };
            for (field_idx, pat) in binding.field_patterns.iter().enumerate() {
                if matches!(pat, ast::Pattern::Wildcard { .. }) {
                    continue;
                }
                // Look up the actual field type from enum_defs so
                // the local has the correct type (e.g. f64 not i64).
                let field_type = ctx
                    .enum_defs
                    .get(subj_enum_name.as_str())
                    .or_else(|| ctx.enum_defs.get(binding.enum_name.as_str()))
                    .and_then(|variants| variants.get(binding.variant_idx as usize))
                    .and_then(|variant| variant.fields.get(field_idx))
                    .cloned()
                    .unwrap_or(MirType::I64);
                // lower_type_expr maps every non-builtin named type to
                // Struct(name) (it has no context to know which names are
                // enums), so a variant whose payload is itself an enum --
                // e.g. `Move.Go(Dir)` -- records the field as Struct("Dir").
                // Recover the real Enum type here, else the binding `d` is
                // typed Struct and a following `match d { Up => .. }` is not
                // detected as an enum match: it silently falls through to the
                // first arm (and AOT crashes when `d` is passed onward).
                // Layout is identical ({i64, ..}); only the type label changes.
                // Recursive: a `(Enum, i64)` tuple payload must not keep an
                // opaque Struct label inside the tuple (unsized %Name on AOT).
                let field_type = recover_enum_types(ctx, field_type);
                // A Struct payload gets its array/map fields REPLACED in
                // place below (retain_struct_heap_fields -> StoreField), so
                // its binding must be a MUTABLE local: an immutable local
                // may be kept purely in an SSA value on the Cranelift JIT
                // (no backing memory slot), silently dropping a StoreField
                // into one of its fields, or letting the write land on a
                // copy that the SUBJECT's payload (still aliased) also
                // sees -- confirmed by JIT-only corruption (a slot that
                // should hold an INDEPENDENT array clone was freed
                // immediately after creation, matching the SUBJECT's own
                // drop rather than staying owned by this binding).
                let needs_mutable = matches!(&field_type, MirType::Struct(n) if !is_copy_type(ctx, &MirType::Struct(n.clone())));
                // A bare ident that names a variant of the field's own enum
                // type (`Some(Red)`) is a variant TEST dispatched through
                // lower_refutable_bind below, not a payload binding.
                let ident_bind_name: Option<String> = match pat {
                    ast::Pattern::Ident { name, .. }
                        if ident_variant_of(ctx, name, &field_type).is_none() =>
                    {
                        Some(name.clone())
                    }
                    _ => None,
                };
                let dest = if let Some(name) = &ident_bind_name {
                    let local = ctx.alloc_local(Some(name.clone()), field_type.clone(), needs_mutable);
                    // Pre-mark non-copy payload bindings as consumed -- EXCEPT
                    // for str/array/map payloads, which get their OWN
                    // independently-retained reference below (see the retain
                    // match on `field_type` further down): those bindings
                    // become a genuine second owner of the payload, mirroring
                    // the `let copy = <bare heap local>` share rule used
                    // elsewhere in this file (Stmt::Let / Stmt::Assign). That
                    // retain must be balanced by exactly one release. It is
                    // NOT safe to rely on the generic named-local scope-end
                    // Drop for this (as a first attempt here did): that
                    // mechanism fires unconditionally once per local at the
                    // enclosing block's exit, but a payload binding is only
                    // ASSIGNED on the control-flow path where ITS OWN arm
                    // runs. A multi-arm match's arms share one merge block,
                    // reached every iteration regardless of which arm fired,
                    // so an unconditional drop there double-frees a STALE
                    // prior-iteration value on any iteration that took a
                    // DIFFERENT arm (confirmed via KRYOS_FREE_DIAG on a
                    // `Tag(s) | Tag2(s2, arr) | _` and an alternating
                    // `Some(v) | None()` loop). Instead, this local is queued
                    // in `arm_owned_payload_locals` and given an EXPLICIT
                    // Drop scoped to THIS arm's own block, right after the
                    // arm body runs (see below) -- that only executes on the
                    // exact path that did the retain. Pre-suppressing it
                    // unconditionally (the ORIGINAL, pre-existing behavior)
                    // left the retain permanently unbalanced whenever the arm
                    // body didn't move the binding elsewhere -- confirmed
                    // leak: a str-payload enum matched in a loop grew RSS
                    // without bound because the retained string was never
                    // dropped. The tail-expr / call-arg consumption checks
                    // (and the general assignment/let "share" rule) still
                    // correctly mark this local dropped when it DOES escape
                    // the arm (returned, assigned to an outer var, pushed
                    // into a container, ...), so the per-arm explicit drop
                    // below skips it and cannot double-free an escaping
                    // payload. Struct/enum payloads (no retain here, or a
                    // freshly rebuilt independent box via
                    // `retain_struct_heap_fields`) keep the previous
                    // suppress-always behavior unchanged.
                    let field_gets_independent_ref = matches!(
                        &field_type,
                        MirType::Str | MirType::Array(_, _) | MirType::Map { .. }
                    );
                    if !is_copy_type(ctx, &field_type) {
                        if field_gets_independent_ref {
                            arm_owned_payload_locals.push(local);
                        } else {
                            ctx.dropped_locals.insert(local.0);
                        }
                    }
                    local
                } else {
                    // Nested pattern (enum / tuple / literal): extract the
                    // payload into a temp, then recursively refine + bind.
                    let t = ctx.alloc_temp(field_type.clone());
                    if !is_copy_type(ctx, &field_type) {
                        ctx.dropped_locals.insert(t.0);
                    }
                    t
                };
                ctx.emit(Instruction::Assign {
                    dest,
                    value: RValue::EnumPayload {
                        operand: subj_op.clone(),
                        enum_name: binding.enum_name.clone(),
                        variant_idx: binding.variant_idx,
                        field_idx: field_idx as u32,
                    },
                });
                // The payload bind above is a bit-copy (RValue::EnumPayload
                // does not retain anything -- confirmed no retain/clone call
                // in either backend's codegen). The match SUBJECT (e.g. a
                // named `let result = f(..)`) still gets its own normal
                // scope-end Drop, which -- when the subject is declared
                // inside a loop body -- fires at the end of EVERY iteration
                // and frees the payload's heap-owned data, including a
                // STRUCT payload's nested str/array/map fields. `dest` (and
                // whatever it's later reassigned into, e.g. `current =
                // next`) still points at that same data: a genuine
                // use-after-free the following iteration's mutation
                // corrupts or crashes on (confirmed via KRYOS_ARR_TRACE
                // instrumentation: a struct's array field is allocated,
                // freed almost immediately by the subject's drop, then
                // retained+pushed as a zombie/freed-sentinel header on the
                // next iteration -- AOT panics inside kryos_array_push with
                // ref_count=1 data=null; JIT segfaults at teardown).
                //
                // Fix: give `dest` its OWN independent reference to every
                // heap-owned field it (transitively, one level, via a
                // Struct payload) now holds, by retaining each one here.
                // This must be a RETAIN (not a "suppress the subject's
                // drop") because some fields (e.g. a plain `str` field
                // sourced from a FieldAccess/bare-ident at the struct
                // literal's OWN construction site) are already correctly,
                // independently retained there -- blanket-suppressing the
                // subject's drop instead would leak those (measured: ~1GB
                // RSS growth per 2s over a 40M-iteration loop before this
                // was corrected to a per-field retain).
                match &field_type {
                    MirType::Str | MirType::Array(_, _) | MirType::Map { .. } => {
                        if let Some(rf) = retain_for_ty(&field_type) {
                            let sink = ctx.alloc_temp(MirType::I64);
                            ctx.emit(Instruction::Assign {
                                dest: sink,
                                value: RValue::Call {
                                    func: rf.to_string(),
                                    args: vec![Operand::Local(dest)],
                                },
                            });
                        }
                    }
                    MirType::Struct(struct_name) if !is_copy_type(ctx, &field_type) => {
                        retain_struct_heap_fields(ctx, dest, struct_name.clone());
                    }
                    // Enum payload bind (e.g. `Ok(new_state) => { state =
                    // new_state }` where new_state: OrderState): the
                    // EnumPayload read above is a bit-copy alias of the
                    // subject's own payload slot -- the subject (e.g. a
                    // `let result = f(..)` declared inside a loop) still
                    // gets its own unconditional scope-end Drop, which
                    // recursively releases ITS payload too. Without an
                    // independent copy here, that drop frees the exact
                    // object `dest` now aliases: a use-after-free the next
                    // allocation silently corrupts (reads back as a garbage
                    // enum tag), surfacing as a SIGILL trap wherever the
                    // stale value is later matched/tag-dispatched (verified
                    // via KRYOS_DUMP_IR: the post-loop `enum_tag` read landed
                    // on freed/reused memory and fell through to the
                    // `unreachable` arm). Fix mirrors the Struct case one
                    // line up: give `dest` its own deep copy via the same
                    // `__kryos_enum_index_clone` marker/emit_enum_deep_copy
                    // codegen already used for the push-aliasing shape.
                    MirType::Enum(enum_name)
                        if !is_copy_type(ctx, &field_type)
                            && ctx.enum_defs.contains_key(enum_name.as_str()) =>
                    {
                        ctx.emit(Instruction::Assign {
                            dest,
                            value: RValue::Call {
                                func: "__kryos_enum_index_clone".to_string(),
                                args: vec![Operand::Local(dest)],
                            },
                        });
                    }
                    _ => {}
                }
                if ident_bind_name.is_none() {
                    // Fail target: next same-tag arm / default / synthetic
                    // no-match (computed above; present whenever the arm has
                    // a refutable sub-pattern).
                    let fail_bb = arm_fail_targets[i].unwrap_or(merge_bb);
                    lower_refutable_bind(
                        ctx,
                        Operand::Local(dest),
                        &field_type,
                        pat,
                        fail_bb,
                    );
                }
            }
        }

        // Structured-arm guard: the payload is now bound, so evaluate the
        // guard and, on false, fall through to this arm's fail target (the
        // next same-tag arm, else the default, else the synthetic no-match).
        // The guard-true path continues into a fresh block that emits the body.
        if let Some(guard) = arm_guards.get(&arm_bb.0) {
            let cond = lower_expr_to_operand(ctx, guard);
            let body_bb = ctx.alloc_block();
            let fail_bb = arm_fail_targets[i].unwrap_or(merge_bb);
            ctx.finish_block(
                Terminator::Branch {
                    cond,
                    then_block: body_bb,
                    else_block: fail_bb,
                },
                body_bb,
            );
        }

        let arm_rvalue = lower_expr_to_rvalue(ctx, body);
        // If the arm body moves a non-copy local into the result, mark the
        // source as consumed so the scope cleanup won't double-drop it.
        if let RValue::Use(Operand::Local(src)) = &arm_rvalue {
            let src_ty = ctx
                .locals
                .iter()
                .find(|l| l.id == *src)
                .map(|l| l.ty.clone())
                .unwrap_or(MirType::I64);
            if !is_copy_type(ctx, &src_ty) {
                ctx.dropped_locals.insert(src.0);
            }
        }
        // Release this arm's independently-retained str/array/map payload
        // bindings (see `arm_owned_payload_locals`'s doc comment above) --
        // scoped to precisely this arm's own block, so it only runs on the
        // control-flow path that actually did the retain. Skips any binding
        // already marked dropped by the tail-return check just above, or by
        // an escape inside the arm body itself (assignment-share retain,
        // `push`'s retain, a consuming call, ...) -- those already gave the
        // payload a new independent owner or transferred it onward.
        for payload_local in &arm_owned_payload_locals {
            if !ctx.dropped_locals.contains(&payload_local.0) {
                drop_tag(ctx, "match-arm-payload");
                ctx.emit(Instruction::Drop {
                    local: *payload_local,
                });
                ctx.dropped_locals.insert(payload_local.0);
            }
        }
        ctx.emit(Instruction::Assign {
            dest: result_local,
            value: arm_rvalue,
        });
        let next_bb = if i + 1 < arm_blocks.len() {
            arm_blocks[i + 1].0
        } else if let Some((db, _)) = default_arm {
            db
        } else {
            merge_bb
        };
        ctx.finish_block(Terminator::Goto(merge_bb), next_bb);
        hide_scope_locals(ctx, arm_scope_start);
    }

    // Default arm.
    if let Some((_, body)) = default_arm {
        let arm_rvalue = lower_expr_to_rvalue(ctx, body);
        if let RValue::Use(Operand::Local(src)) = &arm_rvalue {
            let src_ty = ctx
                .locals
                .iter()
                .find(|l| l.id == *src)
                .map(|l| l.ty.clone())
                .unwrap_or(MirType::I64);
            if !is_copy_type(ctx, &src_ty) {
                ctx.dropped_locals.insert(src.0);
            }
        }
        ctx.emit(Instruction::Assign {
            dest: result_local,
            value: arm_rvalue,
        });
        ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
    }

    // Synthetic no-match block for refuted NESTED patterns with no other
    // same-tag arm and no default: semantically a non-exhaustive-match miss,
    // so panic at runtime (like division by zero). Deliberately does NOT
    // touch result_local: when every arm returns, merge is unreachable and a
    // store here would reference an alloca the LLVM backend never emits.
    if let Some(nm_bb) = refut_nomatch_block {
        ctx.current_block = nm_bb;
        let sink = ctx.alloc_temp(MirType::Void);
        ctx.emit(Instruction::Assign {
            dest: sink,
            value: RValue::Call {
                func: "panic".to_string(),
                args: vec![Operand::Constant(Constant::Str(
                    "match: no arm matched (nested pattern refuted)".to_string(),
                ))],
            },
        });
        ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
    }

    // Seal the non-exhaustive no-match block: a runtime panic (like division by
    // zero), so a non-exhaustive int/str match that misses fails loud instead
    // of reading uninitialized memory. Does NOT touch result_local (panic
    // diverges; a store here would reference an alloca the arms may not emit).
    if let Some(nm_bb) = nonexhaustive_nomatch_block {
        ctx.current_block = nm_bb;
        let sink = ctx.alloc_temp(MirType::Void);
        ctx.emit(Instruction::Assign {
            dest: sink,
            value: RValue::Call {
                func: "panic".to_string(),
                args: vec![Operand::Constant(Constant::Str(
                    "match: no arm matched (non-exhaustive match)".to_string(),
                ))],
            },
        });
        ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
    }

    // Seal the unreachable default block (exhaustive enum match), restoring the
    // cursor to merge_bb so subsequent lowering continues from the join.
    if let Some(unreach_bb) = unreachable_default {
        ctx.current_block = unreach_bb;
        ctx.finish_block(Terminator::Unreachable, merge_bb);
    }

    Operand::Local(result_local)
}

// ---------------------------------------------------------------------------
// Expression type inference
// ---------------------------------------------------------------------------

/// Mark non-copy (str / enum) call arguments as consumed so scope cleanup
/// won't emit a double-free after the callee takes ownership.
///
/// Skips args where arg.id == dest.id (self-consuming: `x = f(x)` or
/// `x = f(x, y)`). In that pattern, the old `x` is moved into the callee,
/// but `dest` is reassigned with the call's return value, which is a fresh
/// owned value that still needs to be dropped at scope end.
fn consume_call_args(ctx: &mut LoweringContext, dest: LocalId, func: &str, args: &[Operand]) {
    // Calls that only READ a heap-typed argument (never store or free it)
    // must not suppress the caller's own scope-end Drop. Before this check,
    // ANY call -- including e.g. `len(s)` -- marked its non-copy args
    // consumed, so a loop-body `let s: str = ...` that was ever read by such
    // a call (an extremely common pattern) never got its per-iteration Drop
    // at all: unbounded leak on every backend, worst on AOT where nothing
    // else masked it (measured: a 3M-iteration `len(s)` loop leaked ~215MB).
    // BORROWING_CALL_ARGS is the same conservative, already-audited allowlist
    // `drop_unescaped_str_temps` trusts for the equivalent temp-drop case.
    if BORROWING_CALL_ARGS.contains(&func) {
        return;
    }
    // `push` transfers ownership of its value argument into the array: the
    // array stores the (pointer-sized) value and later drops it when the
    // array itself is dropped. This includes @copy STRUCTS, which despite
    // being "copy" still own a heap body — if scope cleanup also drops the
    // source local, the body the array points at is freed (use-after-free,
    // observed as non-deterministic garbage in array elements). So for push
    // we consume @copy struct args too, not just non-copy args.
    let push_like = func == "push";
    for arg in args {
        if let Operand::Local(local_id) = arg {
            if *local_id == dest {
                continue;
            }
            let local_ty = ctx
                .locals
                .iter()
                .find(|l| l.id == *local_id)
                .map(|l| l.ty.clone())
                .unwrap_or(MirType::I64);
            // A container element pushed into an array is SHARED, not moved:
            // the language allows reading the source after the push (the
            // checker does not flag it), and the Cranelift backend's array
            // Drop releases elements. Under the old consume model the
            // element was never retained, so `push(a, s)` inside a loop (or
            // any read of `s` after the owning array dropped) was a
            // use-after-free on the JIT — and a divergence, because the LLVM
            // backend's Drop does not release elements. Retain restores the
            // balance: JIT array-drop releases what push retained; on AOT
            // the array's reference leaks (memory-safe, consistent with the
            // documented leak-on-copy model).
            if push_like {
                if let Some(rf) = retain_for_ty(&local_ty) {
                    let sink = ctx.alloc_temp(MirType::I64);
                    ctx.emit(Instruction::Assign {
                        dest: sink,
                        value: RValue::Call {
                            func: rf.to_string(),
                            args: vec![Operand::Local(*local_id)],
                        },
                    });
                    continue;
                }
            }
            let is_copy = is_copy_type(ctx, &local_ty);
            let consume = if push_like {
                // Consume heap-owning args (non-copy) AND @copy structs.
                !is_copy || matches!(local_ty, MirType::Struct(_))
            } else {
                !is_copy
            };
            if consume {
                ctx.dropped_locals.insert(local_id.0);
            }
        }
    }
}

/// Returns true when `ty` is a copy type (no heap ownership).
/// Primitives and @copy structs are copy; everything else (str, enum,
/// non-@copy struct, array, shared, ptr) is non-copy and owns heap memory.
fn is_copy_type(ctx: &LoweringContext, ty: &MirType) -> bool {
    match ty {
        MirType::I64
        | MirType::I32
        | MirType::U8
        | MirType::F64
        | MirType::F32
        | MirType::Bool
        | MirType::Void => true,
        MirType::Struct(name) => ctx.copy_structs.contains(name.as_str()),
        _ => false,
    }
}

/// Best-effort inference of a MIR type for an AST expression.
///
/// Uses struct definitions and function return types collected during the
/// pre-pass to resolve field accesses and call results.  Falls back to Void
/// for unknown function calls and the terminal catch-all; falls back to I64
/// for literals, identifiers, and structural sub-expression types.
/// Infer the MIR type of the value produced by a block when used as a value
/// (the type of its tail expression). Returns `None` when the block does not
/// produce a value (empty, or last stmt is not an expression).
/// If a block's tail is a bare identifier bound by an earlier `let` in the
/// SAME block, resolve its type from that binding. Needed because
/// `infer_match_result_type` / `infer_branch_value_type` run BEFORE the block
/// is lowered, so the block-local isn't in `ctx.locals` yet and a bare-identifier
/// tail (`{ let b = "x"  b }`) would fall through to `infer_expr_type`'s I64
/// default -- mis-typing a str/heap match/if result, which then printed the
/// value's raw pointer as an integer (Pass-18 critical). Returns None when the
/// tail isn't such an identifier, so the caller keeps its existing inference.
fn block_tail_let_type(ctx: &mut LoweringContext, block: &ast::Block) -> Option<MirType> {
    let last = block.stmts.last()?;
    let tail_expr = match last {
        ast::Stmt::Expr { expr, .. } => expr,
        _ => return None,
    };
    // If the block declares no `let`s, its tail can't reference a block-local,
    // so no overlay is needed -- infer directly.
    let has_let = block.stmts[..block.stmts.len() - 1]
        .iter()
        .any(|s| matches!(s, ast::Stmt::Let { pattern: None, .. }));
    if !has_let {
        return Some(infer_expr_type(ctx, tail_expr));
    }
    // This inference runs BEFORE the block is lowered, so the block's own `let`
    // locals aren't in `ctx.locals` yet. Overlay them as SYNTHETIC locals so
    // the tail expression -- an identifier, a binop, a call, a field access,
    // anything referencing a block-local -- resolves to the right type. Without
    // this, a value-position block/if/match arm whose tail is a HEAP-producing
    // expression over a block-local (`{ let outer = "x"  outer + "-done" }`)
    // mis-inferred as I64, so the if/match result slot was I64 while the branch
    // stored a str pointer -> garbage output on both backends and an AOT
    // `'ptr' but expected 'i64'` codegen failure on further use. (Generalizes
    // the old identifier-only tail resolution.) Lookup in `infer_expr_type` is
    // by NAME (reverse scan), so the synthetic id is irrelevant; truncated off
    // afterward so nothing leaks into the real local table.
    let mark = ctx.locals.len();
    for st in &block.stmts[..block.stmts.len() - 1] {
        if let ast::Stmt::Let {
            name,
            ty,
            value,
            pattern: None,
            ..
        } = st
        {
            let t = match (ty, value) {
                (Some(te), _) => lower_type_expr(te),
                (None, Some(v)) => infer_expr_type(ctx, v),
                (None, None) => continue,
            };
            ctx.locals.push(MirLocal {
                id: LocalId(u32::MAX),
                name: Some(name.clone()),
                ty: t,
                mutable: false,
            });
        }
    }
    let result = infer_expr_type(ctx, tail_expr);
    ctx.locals.truncate(mark);
    Some(result)
}

fn infer_branch_value_type(ctx: &mut LoweringContext, block: &ast::Block) -> Option<MirType> {
    if let Some(t) = block_tail_let_type(ctx, block) {
        return Some(t);
    }
    let last = block.stmts.last()?;
    match last {
        ast::Stmt::Expr { expr, .. } => Some(infer_expr_type(ctx, expr)),
        // `return`/`throw` DIVERGE -- they exit the function/unwind and never
        // yield the block's VALUE, so they contribute no type. An if/match whose
        // one branch diverges must take its result type from the LIVE sibling
        // branch (`let x = if c { throw } else { "s" }` is a str, not void/i64).
        _ => None,
    }
}

fn infer_expr_type(ctx: &mut LoweringContext, expr: &ast::Expr) -> MirType {
    match expr {
        ast::Expr::IntLiteral { .. } => MirType::I64,
        ast::Expr::FloatLiteral { .. } => MirType::F64,
        ast::Expr::BoolLiteral { .. } => MirType::Bool,
        ast::Expr::StringLiteral { .. } | ast::Expr::InterpolatedString { .. } => MirType::Str,
        ast::Expr::CharLiteral { .. } => MirType::Char,
        ast::Expr::NoneLiteral { .. } => MirType::I64,

        // `await e` has the type of `e` (the awaited async call's return
        // type). Without this arm the binding `let r = await foo()` fell to
        // the default and was typed Void, so the LLVM backend emitted
        // `call void @foo` and discarded the result (r read back as 0).
        ast::Expr::Await { value, .. } => infer_expr_type(ctx, value),

        ast::Expr::Identifier { name, .. } => {
            // Check if it's an enum variant first.
            if let Some((enum_name, _)) = find_enum_variant(ctx, name) {
                return MirType::Enum(enum_name);
            }
            // Check if it's a mutable module-level global.
            if let Some((mir_ty, _)) = ctx.mutable_globals.get(name.as_str()) {
                return mir_ty.clone();
            }
            // Check if it's a top-level constant.
            if let Some((mir_ty, _)) = ctx.const_defs.get(name.as_str()) {
                return mir_ty.clone();
            }
            // Look up the local's MIR type.
            if let Some(local_ty) = ctx
                .locals
                .iter()
                .rev()
                .find(|l| l.name.as_deref() == Some(name))
                .map(|l| l.ty.clone())
            {
                return local_ty;
            }
            // Check if it's a function name used as a value.
            if let Some(ret_ty) = ctx.func_ret_types.get(name.as_str()) {
                let params = ctx
                    .func_param_types
                    .get(name.as_str())
                    .cloned()
                    .unwrap_or_default();
                return MirType::Function {
                    params,
                    ret: Box::new(ret_ty.clone()),
                };
            }
            MirType::I64
        }

        ast::Expr::Borrow { inner, mutable, .. } => {
            let inner_ty = infer_expr_type(ctx, inner);
            MirType::Ref {
                inner: Box::new(inner_ty),
                mutable: *mutable,
            }
        }

        ast::Expr::Deref { inner, .. } => {
            let inner_ty = infer_expr_type(ctx, inner);
            match inner_ty {
                MirType::Ref { inner, .. } => *inner,
                MirType::Ptr(inner) => *inner,
                // Deref of shared<T> (arc-managed pointer) yields T.
                MirType::Shared(inner) => *inner,
                _ => MirType::I64,
            }
        }

        ast::Expr::FieldAccess { object, field, .. } => {
            // Resolve the object's type, then look up the field in struct_defs.
            let obj_ty = infer_expr_type(ctx, object);
            // Auto-deref: if the object is a reference or shared pointer to a struct,
            // dereference first.
            let resolved_ty = match &obj_ty {
                MirType::Ref { inner, .. } => inner.as_ref().clone(),
                MirType::Shared(inner) => inner.as_ref().clone(),
                other => other.clone(),
            };
            if let MirType::Struct(name) = &resolved_ty {
                if let Some(fields) = ctx.struct_defs.get(name) {
                    if let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == field.as_str()) {
                        return field_ty.clone();
                    }
                }
            }
            // Tuple field access: `t.0`, `t.1` -> the element's type. Without
            // this a non-i64 tuple field (e.g. a str at .1) defaulted to I64,
            // so to_string printed the string handle as an int (JIT) and the
            // LLVM backend hit a ptr-vs-i64 mismatch.
            if let MirType::Tuple(elems) = &resolved_ty {
                if let Ok(idx) = field.parse::<usize>() {
                    if let Some(field_ty) = elems.get(idx) {
                        return field_ty.clone();
                    }
                }
            }
            // Enum variant access: Color.Red → Enum("Color")
            if let MirType::Enum(name) = &resolved_ty {
                if ctx
                    .enum_defs
                    .get(name.as_str())
                    .is_some_and(|vs| vs.iter().any(|v| v.name == field.as_str()))
                {
                    return MirType::Enum(name.clone());
                }
            }
            // Check if object is an identifier matching an enum name.
            if let ast::Expr::Identifier { name, .. } = object.as_ref() {
                if ctx.enum_defs.contains_key(name.as_str()) {
                    return MirType::Enum(name.clone());
                }
            }
            // `self.field` inside an ACTOR handler: the actor value erases to
            // i64 so obj_ty is not Struct; resolve the field type from the
            // actor's registered state layout. Without this an f64/str state
            // field inferred as I64 (iadd on f64 operands; ptr slots on AOT).
            if let ast::Expr::Identifier { name, .. } = object.as_ref() {
                if name == "self" {
                    if let Some(aname) = ctx.current_actor.clone() {
                        if let Some(fields) = ctx.struct_defs.get(aname.as_str()) {
                            if let Some((_, fty)) =
                                fields.iter().find(|(n, _)| n == field.as_str())
                            {
                                return fty.clone();
                            }
                        }
                    }
                }
            }
            MirType::I64
        }

        ast::Expr::BinaryOp {
            left, right, op, ..
        } => {
            // Comparison operators always produce bool.
            match op {
                ast::BinOp::Eq
                | ast::BinOp::Neq
                | ast::BinOp::Lt
                | ast::BinOp::Gt
                | ast::BinOp::LtEq
                | ast::BinOp::GtEq
                | ast::BinOp::And
                | ast::BinOp::Or => return MirType::Bool,
                _ => {}
            }
            // For arithmetic, propagate the type of the left operand; if
            // either side is float, the result is float.
            let lty = infer_expr_type(ctx, left);
            let rty = infer_expr_type(ctx, right);

            // Array concatenation with + produces a dynamic array.
            if *op == ast::BinOp::Add {
                if let (MirType::Array(e1, _), MirType::Array(_, _)) = (&lty, &rty) {
                    return MirType::Array(e1.clone(), None);
                }
            }

            match (&lty, &rty) {
                (MirType::F64, _) | (_, MirType::F64) => MirType::F64,
                (MirType::F32, _) | (_, MirType::F32) => MirType::F32,
                // A narrow-int operand combined with a bare integer LITERAL
                // keeps the NARROW width: the literal has no inherent width (it
                // defaults to i64 only for lack of context), so it adapts to the
                // sized variable. Ranking it i64 (via wider_int_type) typed
                // `u8_var + 1` as an i64 EXPRESSION, so its overflow (256) leaked
                // UNMASKED whenever the result was consumed DIRECTLY -- in a
                // comparison, a call argument, or to_string -- instead of stored
                // to a narrow slot (the ONLY place the width mask fired). That
                // corrupted control flow: `if (g + 1) > 100` branched on 256,
                // not the wrapped 0. Keeping the narrow type makes the result
                // temp narrow, so the store-time ireduce masks it consistently,
                // matching the documented narrow-int truncation-on-overflow.
                _ if is_narrow_int_ty(&lty)
                    && matches!(right.as_ref(), ast::Expr::IntLiteral { .. }) =>
                {
                    lty.clone()
                }
                _ if is_narrow_int_ty(&rty)
                    && matches!(left.as_ref(), ast::Expr::IntLiteral { .. }) =>
                {
                    rty.clone()
                }
                // Mixed-width integer arithmetic PROMOTES to the wider type:
                // `i8 + i64` is an i64 operation. Taking the LEFT type made
                // the result local i8-sized, so the (correctly i64-computed)
                // sum was truncated back on store -- `127i8 + 1000000i64`
                // stored -65 on both backends. Reversed operand order
                // happened to work only because the wide type was on the
                // left.
                _ => wider_int_type(&lty, &rty),
            }
        }

        ast::Expr::UnaryOp { operand, .. } => infer_expr_type(ctx, operand),

        ast::Expr::FnCall { callee, args, .. } => {
            // If the callee is a simple identifier, look up the return type.
            if let ast::Expr::Identifier { name, .. } = callee.as_ref() {
                // Bare (unqualified) enum-variant construction: `Circle(2)` /
                // `Some(x)` types as the variant's enum. Must agree with the
                // lowering (find_enum_variant -> RValue::EnumVariant), else the
                // temp holding the constructed value gets the wrong MIR type and
                // is mis-passed (LLVM emitted `0` for it as a call argument).
                if let Some((enum_name, _)) = find_enum_variant(ctx, name) {
                    return MirType::Enum(enum_name);
                }
                // Actor construction returns a handle typed as the actor struct.
                if ctx.actor_defs.contains_key(name.as_str()) {
                    return MirType::Struct(name.clone());
                }
                // For generic functions, the return type depends on argument types.
                if let Some(template) = ctx.generic_templates.get(name.as_str()) {
                    let generic_params = template.generic_params.clone();
                    let template_params = template.params.clone();
                    let template_ret_ty = template.ret_ty.clone();
                    // Build type map by recursively matching each parameter's
                    // declared TypeExpr against the argument EXPRESSION (not
                    // just its inferred type -- see
                    // extract_type_bindings_from_arg). Handles `[T]`, `(A,
                    // B)`, `fn(T) -> U`, `&T`, and an enum-variant-constructor
                    // argument like `Some(x)` whose static type alone would
                    // erase the payload type.
                    let mut type_map: HashMap<String, MirType> = HashMap::new();
                    for (i, param) in template_params.iter().enumerate() {
                        if let (Some(param_ty), Some(arg)) = (&param.ty, args.get(i)) {
                            extract_type_bindings_from_arg(
                                ctx,
                                param_ty,
                                arg,
                                &generic_params,
                                &mut type_map,
                            );
                        }
                    }
                    if let Some(ret_ty) = &template_ret_ty {
                        return substitute_type_expr_to_mir(ctx, ret_ty, &type_map);
                    }
                    return MirType::Void;
                }
                if let Some(ret_ty) = ctx.func_ret_types.get(name.as_str()).cloned() {
                    // Polymorphic builtins: return type matches argument type
                    if matches!(name.as_str(), "min" | "max" | "abs") {
                        if let Some(first_arg) = args.first() {
                            let arg_ty = infer_expr_type(ctx, first_arg);
                            if arg_ty == MirType::F64 {
                                return MirType::F64;
                            }
                        }
                    }
                    // map_keys/keys(m: map<K, V>) -> [K] — the KEY-typed
                    // array, not the static [str] table entry. An int-keyed
                    // map's unannotated `let ikeys = map_keys(mi)` typed the
                    // keys [str], so `for k in ikeys { mi[k] }` dispatched the
                    // str map-get on i64 keys and str-dropped raw integers
                    // (JIT segfault). Falls through for non-map args (the
                    // self-host's own helpers).
                    if matches!(name.as_str(), "map_keys" | "keys") {
                        if let Some(first_arg) = args.first() {
                            if let MirType::Map { key, .. } = infer_expr_type(ctx, first_arg) {
                                return MirType::Array(key, None);
                            }
                        }
                    }
                    // pop(arr: [T]) -> T — element-typed result so aggregate/
                    // float elements keep their real type (the i64 table entry
                    // mis-typed `let last = pop(items); last.field` on AOT).
                    // Only fires when the argument is statically an array; the
                    // self-host's own `fn pop(arr: i64) -> i64` falls through.
                    if name.as_str() == "pop" {
                        if let Some(first_arg) = args.first() {
                            if let MirType::Array(elem, _) = infer_expr_type(ctx, first_arg) {
                                return *elem;
                            }
                        }
                    }
                    // push(arr: [T], v) -> [T] — array-typed result. The i64
                    // table entry mis-typed `let mut out = push(a, x)`: with
                    // `out` typed as a bare handle, `out[i]` on AOT indexed
                    // relative to the array HEADER (reads returned len/cap,
                    // e.g. "2,4") and element writes vanished — std::heap's
                    // push_min was silently insertion-ordered on release
                    // builds. The self-form `a = push(a, x)` never hit it
                    // because `a` keeps its declared Array type. Same
                    // fall-through guard as pop for the self-host's own push.
                    if name.as_str() == "push" {
                        if let Some(first_arg) = args.first() {
                            if let arr_ty @ MirType::Array(..) = infer_expr_type(ctx, first_arg) {
                                return arr_ty;
                            }
                        }
                    }
                    return ret_ty;
                }
                // Check if callee is a function-typed local (indirect call).
                if let Some(local) = ctx
                    .locals
                    .iter()
                    .rev()
                    .find(|l| l.name.as_deref() == Some(name.as_str()))
                {
                    if let MirType::Function { ret, .. } = &local.ty {
                        return *ret.clone();
                    }
                }
            }
            // Non-identifier callee (e.g. `arr[i](x)`, `(pick())(x)`): the call
            // yields the callee's function return type. Fall back to I64 (the
            // uniform closure-thunk return), NOT Void -- a Void result made the
            // LLVM backend discard the call result into a dead temp (silent 0).
            if let MirType::Function { ret, .. } = infer_expr_type(ctx, callee) {
                return *ret;
            }
            MirType::I64
        }

        ast::Expr::MethodCall { object, method, args, .. } => {
            // Check if this is an enum variant constructor.
            if let ast::Expr::Identifier { name, .. } = object.as_ref() {
                if ctx.enum_defs.contains_key(name.as_str()) {
                    return MirType::Enum(name.clone());
                }
                // Static method call via dot syntax: `Type.method(args)`.
                // A generic associated fn (no `self`, e.g. `Box.new(v)`)
                // must resolve its return type PER instantiation from the
                // call's own args -- the base mangled name's registered
                // type is a placeholder (see `generic_impl_fn_templates`),
                // not the real per-call concrete type.
                if let Some(mir_ty) =
                    infer_generic_impl_fn_ret(ctx, name.as_str(), method.as_str(), args, None)
                {
                    return mir_ty;
                }
                // Static method call via dot syntax: `Type.method(args)`.
                // Resolve the mangled function's return type.
                if ctx.struct_defs.contains_key(name.as_str())
                    || ctx.enum_defs.contains_key(name.as_str())
                {
                    let mangled = format!("{name}__{method}");
                    if let Some(ret_ty) = ctx.func_ret_types.get(&mangled) {
                        return ret_ty.clone();
                    }
                }
            }
            // Check dyn Trait — look up method return type from trait definition.
            let obj_ty = infer_expr_type(ctx, object);
            if let MirType::DynTrait(ref trait_name) = obj_ty {
                if let Some(methods) = ctx.trait_defs.get(trait_name.as_str()) {
                    if let Some(m) = methods.iter().find(|m| m.name == *method) {
                        return m.ret_ty.clone();
                    }
                }
            }
            // Try mangled name first (TypeName__method), then bare method name.
            if let Some(type_name) = infer_type_name_with(ctx, object, obj_ty.clone()) {
                // A generic INSTANCE method whose return type CONSTRUCTS a
                // generic struct (e.g. `Foo<T>.to_box() -> Box<T>`) must
                // resolve its return type PER RECEIVER instantiation -- see
                // `monomorphize_impl_fn` / `GenericImplFnTemplate::has_self`.
                // Must run before the plain mangled-name lookup just below,
                // which always resolves to the base struct's single (erased)
                // lowered copy's placeholder return type.
                let base_for_template =
                    type_name.split("___").next().unwrap_or(type_name.as_str()).to_string();
                let receiver_ty = MirType::Struct(type_name.clone());
                if let Some(mir_ty) = infer_generic_impl_fn_ret(
                    ctx,
                    &base_for_template,
                    method.as_str(),
                    args,
                    Some(&receiver_ty),
                ) {
                    return mir_ty;
                }
                let mangled = format!("{type_name}__{method}");
                if let Some(ret_ty) = ctx.func_ret_types.get(&mangled) {
                    return ret_ty.clone();
                }
                // Generic-struct instance: `type_name` is a monomorphized name
                // (`Cell___i64`), but the method's return type is registered
                // under the base struct name (`Cell__read`). Fall back to it so
                // the call result is typed correctly (the LLVM backend is strict;
                // a Void result mis-types the value).
                let base = type_name.split("___").next().unwrap_or(type_name.as_str());
                if base != type_name {
                    // If the method returns a bare/compound generic parameter,
                    // resolve it to the receiver's CONCRETE monomorphized arg by
                    // binding the impl generics from the self param (the same
                    // machinery monomorphization uses), then substituting the
                    // return. Fixes `to_string(box_of_f64.get())` reporting the
                    // erased i64 slot; field access already sees the concrete
                    // type via the monomorphized struct def.
                    if let Some((gp_names, self_te, ret_te)) = ctx
                        .impl_method_generic_info
                        .get(&(base.to_string(), method.clone()))
                        .cloned()
                    {
                        // Only resolve a return that is a BARE generic parameter
                        // (`-> T`). Such a value is a single i64 slot that
                        // reinterprets safely as its concrete type (f64 bits,
                        // or a str/array pointer). A COMPOUND return that merely
                        // MENTIONS the parameter -- `(T, i64)`, `[T]`,
                        // `Option<T>` -- must NOT be substituted: its value is
                        // built with erased i64-slot layout, so typing the inner
                        // as f64 makes the AOT aggregate (`{double, i64}`)
                        // mismatch the constructed `{i64, i64}` and clang
                        // rejects the module. Those keep the erased type.
                        let ret_is_bare_param = matches!(
                            &ret_te,
                            Some(ast::TypeExpr::Simple { name, .. })
                                if gp_names.iter().any(|g| g == name)
                        );
                        if ret_is_bare_param {
                            if let (Some(self_te), Some(ret_te)) = (self_te, ret_te) {
                                let receiver = MirType::Struct(type_name.clone());
                                let mut bindings: HashMap<String, MirType> = HashMap::new();
                                extract_type_bindings(
                                    ctx, &self_te, &receiver, &gp_names, &mut bindings,
                                );
                                if !bindings.is_empty() {
                                    let resolved =
                                        substitute_type_expr_to_mir(ctx, &ret_te, &bindings);
                                    // Override for a FLOAT concrete type (the
                                    // erased i64 slot holds f64 bits, and
                                    // without the real f64 type `to_string`
                                    // inlined the integer dispatch and printed
                                    // raw bits), a Struct/Enum concrete type
                                    // (chained generic method calls / an
                                    // un-annotated `let` of a generic method's
                                    // result -- CLAUDE.md gotcha #17(b)), or a
                                    // Str concrete type (the erased i64 slot
                                    // holds the string's heap pointer bits,
                                    // and without the real Str type println/
                                    // to_string/concat all mis-dispatched the
                                    // integer path and printed/operated on the
                                    // raw pointer value instead of the string
                                    // -- worst-class silent miscompile, same
                                    // root cause as the float/struct case).
                                    // The generic method body, the
                                    // monomorphized struct layout, AND a str
                                    // value all represent an 8-byte slot
                                    // uniformly (scalar bits or a pointer), so
                                    // retyping the call result here changes
                                    // nothing about the runtime bytes -- it
                                    // only lets the result (and any FOLLOW-ON
                                    // `.method()`/`.field`) resolve against
                                    // the real monomorphized type instead of
                                    // falling back to an unmangled/erased
                                    // lookup.
                                    // (A compound return merely mentioning T --
                                    // `(T, i64)`, `[T]` -- is excluded above by
                                    // `ret_is_bare_param` and keeps the erased
                                    // type; only a bare `-> T` reaches here.)
                                    // `MirType::Array`/`MirType::Map` are
                                    // likewise pointer/handle-shaped bare-T
                                    // returns (a heap array/map is a runtime
                                    // handle, same 8-byte-slot shape as a str
                                    // pointer) -- confirmed the identical
                                    // latent bug via `Box<[str]>.get()`
                                    // (element read printed pointer bits) and
                                    // `Box<map<str,i64>>.get()` (index lookup
                                    // panicked on the erased type), so both
                                    // are included here too.
                                    if matches!(
                                        resolved,
                                        MirType::F64
                                            | MirType::F32
                                            | MirType::Struct(_)
                                            | MirType::Enum(_)
                                            | MirType::Str
                                            | MirType::Array(_, _)
                                            | MirType::Map { .. }
                                    ) {
                                        return resolved;
                                    }
                                }
                            }
                        }
                    }
                    let base_mangled = format!("{base}__{method}");
                    if let Some(ret_ty) = ctx.func_ret_types.get(&base_mangled) {
                        return ret_ty.clone();
                    }
                }
            }
            if let Some(ret_ty) = ctx.func_ret_types.get(method.as_str()) {
                return ret_ty.clone();
            }
            // Function-typed struct field: e.g. t.transform(5) where
            // `transform: fn(i64) -> i64`. Return the closure's declared ret ty.
            if let Some(type_name) = infer_type_name_with(ctx, object, obj_ty.clone()) {
                if let Some(fields) = ctx.struct_defs.get(type_name.as_str()) {
                    if let Some((_, ty)) = fields.iter().find(|(n, _)| n == method) {
                        if let MirType::Function { ret, .. } = ty {
                            return (**ret).clone();
                        }
                    }
                }
            }
            MirType::Void
        }

        ast::Expr::StaticMethodCall {
            type_name, method, args, ..
        } => {
            // `Enum::Variant(..)` constructs an enum value (see the matching branch
            // in lower_expr_to_rvalue); its type is the enum, not Void. Returning
            // Void here made AOT emit a `store void` for `let x = Opt::Some(7)`.
            if let Some(variants) = ctx.enum_defs.get(type_name.as_str()) {
                if variants.iter().any(|v| v.name == *method) {
                    return MirType::Enum(type_name.clone());
                }
            }
            // Rust-style spelling of the same generic-associated-fn case
            // handled in the `MethodCall` (dotted) arm above.
            if let Some(mir_ty) =
                infer_generic_impl_fn_ret(ctx, type_name.as_str(), method.as_str(), args, None)
            {
                return mir_ty;
            }
            let mangled = format!("{type_name}__{method}");
            if let Some(ret_ty) = ctx.func_ret_types.get(&mangled) {
                return ret_ty.clone();
            }
            // If type_name is not a known struct/enum, it's a module alias.
            // Module-level functions are registered with their plain name.
            if !ctx.struct_defs.contains_key(type_name.as_str())
                && !ctx.enum_defs.contains_key(type_name.as_str())
            {
                if let Some(ret_ty) = ctx.func_ret_types.get(method.as_str()) {
                    return ret_ty.clone();
                }
            }
            MirType::Void
        }

        ast::Expr::StructLiteral { name, fields, .. } => {
            MirType::Struct(resolve_struct_literal_name(ctx, name, fields))
        }
        ast::Expr::ArrayLiteral { elements, .. } => {
            // Infer element type from the first element.
            let elem_ty = elements
                .first()
                .map(|e| infer_expr_type(ctx, e))
                .unwrap_or(MirType::I64);
            MirType::Array(Box::new(elem_ty), Some(elements.len() as u64))
        }
        ast::Expr::TupleLiteral { elements, .. } => {
            // Infer the tuple's type element-wise. Returning a scalar here
            // (the old stub) gave destructure temporaries the wrong type, so
            // the Cranelift field-access guard (`match l.ty { Tuple(_) => .. }`)
            // failed into the unknown-struct fallback and `let (a,b) = ..`
            // miscompiled to 0 on the JIT backend.
            MirType::Tuple(elements.iter().map(|e| infer_expr_type(ctx, e)).collect())
        }

        ast::Expr::Cast { ty, .. } => ctx.resolve_type(ty),

        ast::Expr::Lambda {
            params,
            body,
            ret_ty,
            span: lambda_span,
        } => {
            // A lambda expression's type is Function. Its return type drives how
            // a call to the closure variable is typed (the FnCall arm above reads
            // `Function { ret }`). Infer the body's return type for POINTER-sized
            // results (str / struct / array / map / enum / tuple / ptr) so a
            // closure like `|a: str, b: str| a + b` is not mis-typed as i64 --
            // which printed the returned string handle as an int. Pointer types
            // are i64-register-compatible, so no calling-convention change is
            // needed. Floats keep i64 here (their closure ABI is handled
            // separately); i64/bool/etc. already work.
            let resolved_params = ctx.lambda_param_types.get(lambda_span).cloned();
            // Resolve each param's type: explicit annotation, else the type
            // checker's resolved type (so e.g. a `str` closure param isn't faked
            // as i64), else i64. Used both for body inference and for the
            // Function type's params -- the latter lets a generic HOF bind its
            // type vars correctly (e.g. `map`'s element type T from the closure).
            let param_tys: Vec<MirType> = params
                .iter()
                .enumerate()
                .map(|(i, p)| match p.ty.as_ref() {
                    Some(t) => ctx.resolve_type(t),
                    None => resolved_params
                        .as_ref()
                        .and_then(|rp| rp.get(i))
                        .and_then(|o| o.as_ref())
                        .map(|te| ctx.resolve_type(te))
                        .unwrap_or(MirType::I64),
                })
                .collect();
            let ret = match ret_ty {
                Some(ty) => ctx.resolve_type(ty),
                None => {
                    // Register params on top of the current scope (captures stay
                    // visible) so the body type resolves, then truncate.
                    let saved_len = ctx.locals.len();
                    let saved_next = ctx.next_local;
                    for (p, pty) in params.iter().zip(param_tys.iter()) {
                        ctx.alloc_local(Some(p.name.clone()), pty.clone(), false);
                    }
                    let inferred = infer_expr_type(ctx, body);
                    ctx.locals.truncate(saved_len);
                    ctx.next_local = saved_next;
                    match inferred {
                        MirType::Str
                        | MirType::Ptr(_)
                        | MirType::Ref { .. }
                        | MirType::Shared(_)
                        | MirType::Array(..)
                        | MirType::Tuple(_)
                        | MirType::Struct(_)
                        | MirType::Enum(_)
                        | MirType::DynTrait(_)
                        | MirType::Map { .. }
                        // Floats too: a float-returning closure (`|x| x*2.0`)
                        // must report ret=f64 so a generic HOF that derives its
                        // result type var from the closure's return (e.g. `map`
                        // -> [U]) binds U=f64, giving the caller a `[f64]` whose
                        // elements read as floats rather than raw i64 slots. The
                        // closure fn still returns its value in the uniform i64
                        // slot (bits preserved); only the static type changes.
                        | MirType::F32
                        | MirType::F64
                        // A lambda RETURNING a lambda (`|n| |x| x + n`): the
                        // outer's ret must be Function so the call result is
                        // function-typed and the call site emits CallIndirect
                        // (an i64-typed result emitted a direct call to a
                        // symbol named after the variable -> link error).
                        | MirType::Function { .. } => inferred,
                        _ => MirType::I64,
                    }
                }
            };
            MirType::Function {
                params: param_tys,
                ret: Box::new(ret),
            }
        }

        ast::Expr::PipeExpr { left, right, span } => {
            // Infer as the DESUGARED call (must match the lowering): the old
            // "RHS callable's return type" shortcut missed the polymorphic
            // builtin dispatch, so `x |> abs` on f64 typed i64 and the float
            // printed as its raw bit pattern.
            let call = desugar_pipe(left, right, *span);
            infer_expr_type(ctx, &call)
        }

        ast::Expr::IndexAccess { object, .. } => {
            // Infer element type from the array/tuple/map type.
            let obj_ty = infer_expr_type(ctx, object);
            match obj_ty {
                MirType::Array(elem, _) => *elem,
                MirType::Tuple(elems) => elems.into_iter().next().unwrap_or(MirType::I64),
                MirType::Str => MirType::Str,
                MirType::Map { value, .. } => *value,
                _ => MirType::I64,
            }
        }

        ast::Expr::MapLiteral { entries, .. } => {
            // Infer key/value types from the first entry; fall back to I64 for empty maps.
            let (key_ty, val_ty) = entries
                .first()
                .map(|(k, v)| (infer_expr_type(ctx, k), infer_expr_type(ctx, v)))
                .unwrap_or((MirType::I64, MirType::I64));
            MirType::Map {
                key: Box::new(key_ty),
                value: Box::new(val_ty),
            }
        }
        ast::Expr::MoveExpr { inner, .. } => infer_expr_type(ctx, inner),
        ast::Expr::WeakExpr { inner, .. } => {
            let inner_ty = infer_expr_type(ctx, inner);
            MirType::Shared(Box::new(inner_ty))
        }
        ast::Expr::RangeExpr { .. } => MirType::I64,

        ast::Expr::MatchExpr { subject, arms, .. } => {
            infer_match_result_type(ctx, subject, arms)
        }

        ast::Expr::IfExpr { then_branch, else_branch, .. } => {
            // Type from the first branch that PRODUCES a value; a divergent
            // (throw/return) branch yields no type and is skipped. This MUST
            // match the if-expression lowering (which uses the same
            // infer_branch_value_type + else fallback) -- otherwise the `let`
            // binding and the if's own result local disagree, e.g.
            // `let f = if c { throw } else { "s" }` typed `f` as I64 (the throw
            // arm) while the str "s" was stored, so println(f) printed the raw
            // pointer bits. (Was: looked only at the then-branch's Stmt::Expr.)
            infer_branch_value_type(ctx, then_branch)
                .or_else(|| else_branch.as_ref().and_then(|b| infer_branch_value_type(ctx, b)))
                .unwrap_or(MirType::I64)
        }

        ast::Expr::ComptimeBlock { body, .. } => {
            // Infer from the last expression in the comptime body.
            body.stmts
                .last()
                .and_then(|s| {
                    if let ast::Stmt::Expr { expr, .. } = s {
                        Some(infer_expr_type(ctx, expr))
                    } else {
                        None
                    }
                })
                .unwrap_or(MirType::I64)
        }

        ast::Expr::Block { block, .. } | ast::Expr::UnsafeBlock { body: block, .. } => {
            // A block expression's value is its last expression. Falling to
            // the Void catch-all typed `let x = { 40 + 2 }` (and the
            // `unsafe { ... }` form) as a void slot -- a `store void` codegen
            // error on AOT.
            // A bare-identifier tail bound by an earlier `let` in this block
            // resolves via its binding (block-locals aren't in `ctx.locals`
            // during pre-lowering inference).
            if let Some(t) = block_tail_let_type(ctx, block) {
                return t;
            }
            match block.stmts.last() {
                Some(ast::Stmt::Expr { expr, .. }) => infer_expr_type(ctx, expr),
                // A trailing `try { .. } catch e { .. }` is the block's tail
                // VALUE too (mirrors the `lower_expr_to_rvalue` Expr::Block
                // path and `infer_try_catch_value_type` in kryos-types). Without
                // this, this TYPE-ONLY inference (used to size a match arm's
                // result local via `infer_match_result_type`, and a binary-op
                // operand's temp via `lower_expr_to_operand`'s fallback) typed
                // a try/catch-tail block as `void` even though the ACTUAL
                // lowering of that same block correctly produced an i64/str/..
                // value -- the value got stored into a void-typed slot, and
                // the LLVM backend's void-coercion silently substituted `0`
                // (or emitted an invalid `icmp ... void` for a `==` operand).
                Some(ast::Stmt::TryCatch {
                    try_block,
                    catch_block,
                    ..
                }) => infer_branch_value_type(ctx, try_block)
                    .or_else(|| infer_branch_value_type(ctx, catch_block))
                    .unwrap_or(MirType::I64),
                // A trailing `if ... else ...` is the block's tail value.
                Some(s) => s
                    .as_value_if_expr()
                    .map(|e| infer_expr_type(ctx, &e))
                    .unwrap_or(MirType::Void),
                None => MirType::Void,
            }
        }

        _ => MirType::Void,
    }
}

// ---------------------------------------------------------------------------
// Expression lowering
// ---------------------------------------------------------------------------

/// Lower `object.field = value` using read-modify-writeback when `object` is
/// itself a FieldAccess (nested mutation like `o.a.v = 99`). Direct local
/// targets (base case) emit a plain StoreField exactly as before.
fn lower_nested_field_assign(
    ctx: &mut LoweringContext,
    object: &ast::Expr,
    field: &str,
    value: Operand,
) {
    match object {
        // Base case: `local_var.field = value`. Locals store through directly;
        // anything else (mutable module-level global, const) falls back to the
        // operand lowering this arm always used.
        ast::Expr::Identifier { name, .. } => {
            if let Some(obj_local) = find_local_by_name(ctx, name) {
                ctx.emit(Instruction::StoreField {
                    object: Operand::Local(obj_local),
                    field: field.to_string(),
                    value,
                });
            } else {
                let obj_op = lower_expr_to_operand(ctx, object);
                ctx.emit(Instruction::StoreField {
                    object: obj_op,
                    field: field.to_string(),
                    value,
                });
            }
        }
        // Recursive case: `(parent.mid).field = value`.
        //   (1) load a MUTABLE copy of the intermediate struct,
        //   (2) mutate the target field on the copy,
        //   (3) write the copy back into the parent field (recurse).
        // mutable=true gives the temp an alloca (%_N.addr) on LLVM so the
        // StoreField mutable-aggregate path applies instead of `inttoptr %Agg`.
        ast::Expr::FieldAccess {
            object: parent,
            field: mid_field,
            ..
        } => {
            let mid_ty = infer_expr_type(ctx, object);
            let tmp = ctx.alloc_local(None, mid_ty, true);
            let load_rvalue = lower_expr_to_rvalue(ctx, object);
            ctx.emit(Instruction::Assign {
                dest: tmp,
                value: load_rvalue,
            });
            ctx.emit(Instruction::StoreField {
                object: Operand::Local(tmp),
                field: field.to_string(),
                value,
            });
            lower_nested_field_assign(ctx, parent, mid_field, Operand::Local(tmp));
        }
        // Array/map index: `arr[i].field = v` or `map[k].field = v`.
        // Array elements are heap-boxed; kryos_array_get returns the box pointer as i64.
        // If we let lower_expr_to_operand infer the struct element type, the LLVM
        // backend allocates a local alloca copy of the struct, does the field store
        // into the copy, and never writes back — the mutation is lost on AOT.
        // Fix: allocate the temp as Ptr(elem_ty). The LLVM codegen emits
        //   `inttoptr i64 %raw to ptr` (via the Index dest_ty=="ptr" path),
        // giving a `ptr`-typed SSA value. StoreField then inttoptr-bypasses the
        // alloca branch and resolve_struct_name can unwrap Ptr(Struct(name)) to
        // get the correct struct layout for field-indexed GEP.
        ast::Expr::IndexAccess {
            object: coll,
            index,
            ..
        } => {
            let coll_ty = infer_expr_type(ctx, coll);
            if let MirType::Array(elem_ty, _) = coll_ty {
                let arr_op = lower_expr_to_operand(ctx, coll);
                let idx_op = lower_expr_to_operand(ctx, index);
                // Ptr(elem_ty) so LLVM emits inttoptr + knows the struct type for GEP.
                let ptr_ty = MirType::Ptr(elem_ty);
                let elem_ptr = ctx.alloc_temp(ptr_ty);
                ctx.emit(Instruction::Assign {
                    dest: elem_ptr,
                    value: RValue::Index {
                        object: arr_op,
                        index: idx_op,
                    },
                });
                ctx.emit(Instruction::StoreField {
                    object: Operand::Local(elem_ptr),
                    field: field.to_string(),
                    value,
                });
            } else {
                // Map: kryos_map_get returns the element as a box pointer (i64).
                let coll_ty2 = infer_expr_type(ctx, coll);
                let map_op = lower_expr_to_operand(ctx, coll);
                let key_op = lower_expr_to_operand(ctx, index);
                let idx_ty = infer_expr_type(ctx, index);
                let get_fn = if idx_ty == MirType::Str {
                    "kryos_map_get_str"
                } else {
                    "kryos_map_get"
                };
                // Carry the value type for field resolution, same as the array case.
                let elem_ty: Box<MirType> = match coll_ty2 {
                    MirType::Map { value, .. } => value,
                    _ => Box::new(MirType::I64),
                };
                let ptr_ty = MirType::Ptr(elem_ty);
                let elem_ptr = ctx.alloc_temp(ptr_ty);
                ctx.emit(Instruction::Assign {
                    dest: elem_ptr,
                    value: RValue::Call {
                        func: get_fn.to_string(),
                        args: vec![map_op, key_op],
                    },
                });
                ctx.emit(Instruction::StoreField {
                    object: Operand::Local(elem_ptr),
                    field: field.to_string(),
                    value,
                });
            }
        }
        // Fallback for any other exotic object expression.
        _ => {
            let obj_op = lower_expr_to_operand(ctx, object);
            ctx.emit(Instruction::StoreField {
                object: obj_op,
                field: field.to_string(),
                value,
            });
        }
    }
}

/// If `target_ty` is `MirType::DynTrait` and `source_expr`'s inferred type is
/// a concrete `MirType::Struct`, wrap `operand` in `RValue::MakeTraitObject`
/// so the later `vtable_call` site sees a fat pointer instead of a bare
/// struct value. A value that is ALREADY `dyn Trait`-typed (e.g. forwarding
/// an existing `dyn Trait` binding into another `dyn Trait`-typed position)
/// is left untouched -- `infer_expr_type` returns `MirType::DynTrait` for it,
/// not `MirType::Struct`, so only a genuine concrete-struct -> dyn-Trait
/// coercion is ever wrapped here. Enum/other sources are out of scope: only
/// structs implement traits in this language.
///
/// Shared by every dyn-Trait coercion site added alongside the original
/// call-argument coercion (`ast::Expr::FnCall`'s normal-call and
/// generic-call arms, which keep their own inline copies of this same
/// wrap -- left as-is to avoid touching the already-verified working path):
/// an explicitly `dyn Trait`-annotated `let`, a `return` from a function
/// whose declared return type is `dyn Trait`, a `dyn Trait`-typed struct
/// field initializer, and a `dyn Trait` parameter of a resolved method call.
fn coerce_to_dyn_if_needed(
    ctx: &mut LoweringContext,
    operand: Operand,
    target_ty: &MirType,
    source_expr: &ast::Expr,
) -> Operand {
    if let MirType::DynTrait(ref trait_name) = target_ty {
        if let MirType::Struct(ref concrete_type) = infer_expr_type(ctx, source_expr) {
            let tmp = ctx.alloc_temp(MirType::DynTrait(trait_name.clone()));
            ctx.emit(Instruction::Assign {
                dest: tmp,
                value: RValue::MakeTraitObject {
                    value: operand,
                    concrete_type: concrete_type.clone(),
                    trait_name: trait_name.clone(),
                },
            });
            return Operand::Local(tmp);
        }
    }
    operand
}

fn lower_expr_to_operand(ctx: &mut LoweringContext, expr: &ast::Expr) -> Operand {
    match expr {
        ast::Expr::IntLiteral { value, .. } => Operand::Constant(Constant::Int(*value)),
        ast::Expr::FloatLiteral { value, .. } => Operand::Constant(Constant::Float(*value)),
        ast::Expr::BoolLiteral { value, .. } => Operand::Constant(Constant::Bool(*value)),
        ast::Expr::StringLiteral { value, .. } => Operand::Constant(Constant::Str(value.clone())),
        ast::Expr::NoneLiteral { .. } => Operand::Constant(Constant::None),
        ast::Expr::Identifier { name, .. } => {
            // Built-in `null` constant — lowers to integer 0 (raw pointer/handle sentinel).
            if name == "null" {
                return Operand::Constant(Constant::Int(0));
            }
            let is_local = ctx
                .locals
                .iter()
                .any(|l| l.name.as_deref() == Some(name.as_str()));
            // Mutable module-level global: emit a real runtime load.
            if !is_local {
                if let Some((mir_ty, _)) = ctx.mutable_globals.get(name.as_str()).cloned() {
                    return Operand::Local(emit_global_load(ctx, name, mir_ty));
                }
            }
            // Immutable constant: inline its value expression at the use site.
            if !is_local {
                if let Some((_, const_expr)) = ctx.const_defs.get(name.as_str()).cloned() {
                    return lower_expr_to_operand(ctx, &const_expr);
                }
            }
            // Function name used as a value (function pointer).
            if !is_local && ctx.func_ret_types.contains_key(name.as_str()) {
                let rvalue = RValue::Closure {
                    func_name: name.clone(),
                    captures: vec![],
                };
                let temp = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: temp,
                    value: rvalue,
                });
                return Operand::Local(temp);
            }
            // Nullary enum variant used directly as an operand: bare `None`/`Red`
            // or qualified `Opt::None`. Without this, an inline `describe(Opt::None)`
            // fell through to the fallback below, which allocates a fresh
            // UNINITIALIZED i64 local -- crashing the JIT and mis-dispatching AOT.
            // (The let-bound form went through the rvalue path and was fine.)
            if !is_local {
                if let Some((enum_name, variant_idx)) = find_enum_variant(ctx, name) {
                    let temp = ctx.alloc_temp(MirType::Enum(enum_name.clone()));
                    ctx.emit(Instruction::Assign {
                        dest: temp,
                        value: RValue::EnumVariant {
                            enum_name,
                            variant_idx,
                            fields: vec![],
                        },
                    });
                    return Operand::Local(temp);
                }
            }
            // Bare-field actor-state READ inside a handler (`count + amount`,
            // `return count`): route to ActorStateLoad. The fallback below
            // would allocate a fresh i64 local that silently reads 0 (backlog
            // #25; docs/09-concurrency.md uses bare-field state throughout).
            if !is_local {
                if let Some(aname) = ctx.current_actor.clone() {
                    if let Some(fields) = ctx.actor_state_fields.get(&aname).cloned() {
                        if let Some((_fname, field_idx)) =
                            fields.iter().find(|(n, _)| n == name.as_str())
                        {
                            if let Some(self_local) = find_local_by_name(ctx, "self") {
                                let fty = ctx
                                    .struct_defs
                                    .get(aname.as_str())
                                    .and_then(|fs| {
                                        fs.iter()
                                            .find(|(n, _)| n == name.as_str())
                                            .map(|(_, t)| t.clone())
                                    })
                                    .unwrap_or(MirType::I64);
                                let dest = ctx.alloc_temp(fty);
                                ctx.emit(Instruction::ActorStateLoad {
                                    dest,
                                    state_ptr: self_local,
                                    field_offset: *field_idx,
                                });
                                return Operand::Local(dest);
                            }
                        }
                    }
                }
            }
            let local = find_local_by_name(ctx, name)
                .unwrap_or_else(|| ctx.alloc_local(Some(name.to_string()), MirType::I64, false));
            Operand::Local(local)
        }
        _ => {
            // Complex expression — lower to rvalue, store in temp.
            // Infer the type so the temp has the correct MIR type instead
            // of defaulting to I64 for everything.
            let inferred_ty = infer_expr_type(ctx, expr);
            let rvalue = lower_expr_to_rvalue(ctx, expr);
            let temp = ctx.alloc_temp(inferred_ty);
            // If this call's result is being embedded directly in a larger
            // expression (array/tuple/struct literal element, binary-op
            // operand, nested call argument, ...) rather than bound by a
            // top-level `let`, mark its non-copy args consumed exactly as the
            // `let`-binding path does below. Without this, a by-value method
            // receiver used as e.g. `[recv.method(), ...]` was never added to
            // `dropped_locals`: the receiver got its own ordinary scope-end
            // Drop AND the field the callee returned/extracted from it (which
            // aliases the same box on the Cranelift backend, since a non-
            // `@copy` struct receiver is passed by the caller's own pointer,
            // not a defensive copy) was independently owned and dropped a
            // second time by whatever now holds it -- a double-free of the
            // same box (JIT-only: `pp.first_of()` embedded in an array
            // literal froze then double-freed `pp`'s boxed field, silent
            // heap corruption -> bare exit 127 with correct stdout).
            if let RValue::Call { ref func, ref args } = rvalue {
                consume_call_args(ctx, temp, func, args);
            }
            ctx.emit(Instruction::Assign {
                dest: temp,
                value: rvalue,
            });
            Operand::Local(temp)
        }
    }
}

fn lower_expr_to_rvalue(ctx: &mut LoweringContext, expr: &ast::Expr) -> RValue {
    match expr {
        ast::Expr::IntLiteral { value, .. } => RValue::ConstInt(*value),
        ast::Expr::FloatLiteral { value, .. } => RValue::ConstFloat(*value),
        ast::Expr::BoolLiteral { value, .. } => RValue::ConstBool(*value),
        ast::Expr::StringLiteral { value, .. } => RValue::ConstString(value.clone()),
        ast::Expr::NoneLiteral { .. } => RValue::ConstNone,

        ast::Expr::Identifier { name, .. } => {
            // Built-in `null` constant — lowers to integer 0 (raw pointer/handle sentinel).
            if name == "null" {
                return RValue::ConstInt(0);
            }
            // Check if this is a unit enum variant (e.g., `None`, `Red`).
            if let Some((enum_name, variant_idx)) = find_enum_variant(ctx, name) {
                return RValue::EnumVariant {
                    enum_name,
                    variant_idx,
                    fields: vec![],
                };
            }
            let is_local = ctx.locals.iter().any(|l| l.name.as_deref() == Some(name));
            // Mutable module-level global: emit a real runtime load.
            if !is_local {
                if let Some((mir_ty, _)) = ctx.mutable_globals.get(name.as_str()).cloned() {
                    let local = emit_global_load(ctx, name, mir_ty);
                    return RValue::Use(Operand::Local(local));
                }
            }
            // Immutable constant: inline its value expression at the use site.
            if !is_local {
                if let Some((_, const_expr)) = ctx.const_defs.get(name.as_str()).cloned() {
                    return lower_expr_to_rvalue(ctx, &const_expr);
                }
            }
            // Check if this is a function name used as a value (function pointer).
            // If the name matches a known function but is NOT a local variable,
            // emit a Closure with no captures to get the function's address.
            if !is_local && ctx.func_ret_types.contains_key(name.as_str()) {
                return RValue::Closure {
                    func_name: name.clone(),
                    captures: vec![],
                };
            }
            // Bare-field actor-state READ inside a handler (`return count`,
            // `count + amount`): route to ActorStateLoad, mirroring the
            // `self.field` path. Without this, a bare field name fell through
            // to the fresh-i64 fallback and silently read 0 (docs/09-
            // concurrency.md uses bare-field state throughout — backlog #25).
            if !is_local {
                if let Some(aname) = ctx.current_actor.clone() {
                    if let Some(fields) = ctx.actor_state_fields.get(&aname).cloned() {
                        if let Some((_fname, field_idx)) =
                            fields.iter().find(|(n, _)| n == name)
                        {
                            if let Some(self_local) = find_local_by_name(ctx, "self") {
                                let fty = ctx
                                    .struct_defs
                                    .get(aname.as_str())
                                    .and_then(|fs| {
                                        fs.iter().find(|(n, _)| n == name).map(|(_, t)| t.clone())
                                    })
                                    .unwrap_or(MirType::I64);
                                let dest = ctx.alloc_temp(fty);
                                ctx.emit(Instruction::ActorStateLoad {
                                    dest,
                                    state_ptr: self_local,
                                    field_offset: *field_idx,
                                });
                                return RValue::Use(Operand::Local(dest));
                            }
                        }
                    }
                }
            }
            let local = find_local_by_name(ctx, name)
                .unwrap_or_else(|| ctx.alloc_local(Some(name.to_string()), MirType::I64, false));
            RValue::Use(Operand::Local(local))
        }

        ast::Expr::BinaryOp {
            op, left, right, ..
        } => {
            // Short-circuit evaluation for logical and/or:
            //   a and b  →  let _r = a; if _r { _r = b }; use _r
            //   a or  b  →  let _r = a; if !_r { _r = b }; use _r
            if *op == ast::BinOp::And {
                let result = ctx.alloc_temp(MirType::Bool);
                let lhs = lower_expr_to_operand(ctx, left);
                ctx.emit(Instruction::Assign {
                    dest: result,
                    value: RValue::Use(lhs),
                });
                let then_bb = ctx.alloc_block();
                let merge_bb = ctx.alloc_block();
                ctx.finish_block(
                    Terminator::Branch {
                        cond: Operand::Local(result),
                        then_block: then_bb,
                        else_block: merge_bb,
                    },
                    then_bb,
                );
                // LHS was truthy — evaluate RHS.
                let rhs = lower_expr_to_operand(ctx, right);
                ctx.emit(Instruction::Assign {
                    dest: result,
                    value: RValue::Use(rhs),
                });
                ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
                return RValue::Use(Operand::Local(result));
            }
            if *op == ast::BinOp::Or {
                let result = ctx.alloc_temp(MirType::Bool);
                let lhs = lower_expr_to_operand(ctx, left);
                ctx.emit(Instruction::Assign {
                    dest: result,
                    value: RValue::Use(lhs),
                });
                let else_bb = ctx.alloc_block();
                let merge_bb = ctx.alloc_block();
                ctx.finish_block(
                    Terminator::Branch {
                        cond: Operand::Local(result),
                        then_block: merge_bb,
                        else_block: else_bb,
                    },
                    else_bb,
                );
                // LHS was falsy — evaluate RHS.
                let rhs = lower_expr_to_operand(ctx, right);
                ctx.emit(Instruction::Assign {
                    dest: result,
                    value: RValue::Use(rhs),
                });
                ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
                return RValue::Use(Operand::Local(result));
            }

            // Array concatenation: a + b → kryos_array_concat(a, b)
            if *op == ast::BinOp::Add {
                let lty = infer_expr_type(ctx, left);
                let rty = infer_expr_type(ctx, right);
                if matches!((&lty, &rty), (MirType::Array(_, _), MirType::Array(_, _))) {
                    let lhs = lower_expr_to_operand(ctx, left);
                    let rhs = lower_expr_to_operand(ctx, right);
                    return RValue::Call {
                        func: "kryos_array_concat".to_string(),
                        args: vec![lhs, rhs],
                    };
                }
            }

            // Structural equality: `==`/`!=` on a struct or enum operand.
            // Neither backend can compare two aggregate VALUES directly (see
            // the doc comment on `ensure_struct_eq_helper`), so route through
            // a synthesized per-type `__kryos_eq_<Type>` helper instead of
            // falling into the generic scalar BinOp path below.
            if matches!(op, ast::BinOp::Eq | ast::BinOp::Neq) {
                let lty = infer_expr_type(ctx, left);
                let helper_name = match &lty {
                    MirType::Struct(name) => Some(ensure_struct_eq_helper(ctx, name)),
                    MirType::Enum(name) => Some(ensure_enum_eq_helper(ctx, name)),
                    MirType::Tuple(elems) => {
                        Some(ensure_tuple_eq_helper(ctx, &elems.clone()))
                    }
                    _ => None,
                };
                if let Some(helper_name) = helper_name {
                    let lhs = lower_expr_to_operand(ctx, left);
                    let rhs = lower_expr_to_operand(ctx, right);
                    let call = RValue::Call {
                        func: helper_name,
                        args: vec![lhs, rhs],
                    };
                    if *op == ast::BinOp::Neq {
                        let eq_result = ctx.alloc_temp(MirType::Bool);
                        ctx.emit(Instruction::Assign {
                            dest: eq_result,
                            value: call,
                        });
                        return RValue::UnOp {
                            op: MirUnOp::Not,
                            operand: Operand::Local(eq_result),
                        };
                    }
                    return call;
                }
            }

            let mut lhs = lower_expr_to_operand(ctx, left);
            let mut rhs = lower_expr_to_operand(ctx, right);
            // Narrow UNSIGNED operands in comparisons: values are stored in
            // sign-extended i64 slots, so `let b: u8 = 255; b == 255` compared
            // -1 to 255 and was false (printing was fixed in step 198, the
            // compare path was not). Mask each unsigned-narrow side back to
            // its value range before the compare; the masked values are
            // nonnegative, so the signed compare is then correct for every
            // operator including < and >.
            if matches!(
                op,
                ast::BinOp::Eq
                    | ast::BinOp::Neq
                    | ast::BinOp::Lt
                    | ast::BinOp::Gt
                    | ast::BinOp::LtEq
                    | ast::BinOp::GtEq
            ) {
                let unsigned_mask = |t: &MirType| -> Option<i64> {
                    match t {
                        MirType::U8 => Some(0xFF),
                        MirType::U16 => Some(0xFFFF),
                        MirType::U32 => Some(0xFFFF_FFFF),
                        _ => None,
                    }
                };
                let lty = infer_expr_type(ctx, left);
                let rty = infer_expr_type(ctx, right);
                if let Some(m) = unsigned_mask(&lty) {
                    let t = ctx.alloc_temp(MirType::I64);
                    ctx.emit(Instruction::Assign {
                        dest: t,
                        value: RValue::BinOp {
                            op: MirBinOp::BitAnd,
                            left: lhs,
                            right: Operand::Constant(Constant::Int(m)),
                        },
                    });
                    lhs = Operand::Local(t);
                }
                if let Some(m) = unsigned_mask(&rty) {
                    let t = ctx.alloc_temp(MirType::I64);
                    ctx.emit(Instruction::Assign {
                        dest: t,
                        value: RValue::BinOp {
                            op: MirBinOp::BitAnd,
                            left: rhs,
                            right: Operand::Constant(Constant::Int(m)),
                        },
                    });
                    rhs = Operand::Local(t);
                }
            }
            RValue::BinOp {
                op: lower_binop(*op),
                left: lhs,
                right: rhs,
            }
        }

        ast::Expr::UnaryOp { op, operand, .. } => {
            let inner = lower_expr_to_operand(ctx, operand);
            RValue::UnOp {
                op: lower_unop(*op),
                operand: inner,
            }
        }

        ast::Expr::FnCall { callee, args, .. } => {
            let func_name = match callee.as_ref() {
                ast::Expr::Identifier { name, .. } => name.clone(),
                _ => {
                    // Callee is an arbitrary expression that evaluates to a
                    // function value (e.g. `arr[i](x)`, `(pick())(x)`,
                    // `tbl.field(x)`). Lower it to an operand and emit an
                    // indirect call through the function pointer. None of the
                    // identifier-keyed dispatch below (actor / enum-variant /
                    // generic / map-builtin / direct call) can apply when the
                    // callee is not a name. Previously this fell through to a
                    // direct call to the bogus symbol `<closure>`, which the
                    // linker rejected (LNK2001 unresolved external `<closure>`).
                    let callee_op = lower_expr_to_operand(ctx, callee);
                    let mir_args: Vec<Operand> =
                        args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                    return RValue::CallIndirect {
                        callee: callee_op,
                        args: mir_args,
                    };
                }
            };

            // Cooperative async executor surface. `coop_spawn(taskExpr)` is a
            // dedicated form (it must NOT eagerly evaluate its argument), so we
            // intercept it before the generic call path. It evaluates to the
            // task id (0 for now — the wrapper machinery doesn't thread it back).
            if func_name == "coop_spawn" && args.len() == 1 {
                lower_coop_spawn(ctx, &args[0]);
                return RValue::Use(Operand::Constant(Constant::Int(0)));
            }

            // `to_string(aggregate)` has no string representation and printed a
            // raw pointer (JIT) / garbage (AOT), same class as interpolating one.
            // Substitute a safe placeholder for a non-scalar aggregate arg (the
            // arg is still lowered for side effects). The concrete case is also
            // rejected by the type checker; this covers explicit `to_string(x)`
            // and the generic-monomorphized case. A user `to_string` METHOD is
            // called via `x.to_string()` (a MethodCall, a different path) and is
            // unaffected -- only the free-function builtin is intercepted.
            if func_name == "to_string" && args.len() == 1 {
                let aty = infer_expr_type(ctx, &args[0]);
                // A char is stored as its i64 code point; to_string must produce
                // the CHARACTER (via char_from), not the numeric code -- the
                // generic to_string treated it as an integer ('x' -> "120").
                if matches!(aty, MirType::Char) {
                    let code = lower_expr_to_operand(ctx, &args[0]);
                    return RValue::Call {
                        func: "char_from".into(),
                        args: vec![code],
                    };
                }
                // A struct/enum with its OWN `to_string` method: dispatch the
                // bare `to_string(x)` to that method (`x.to_string()`), not the
                // safe placeholder. Without this, `to_string(structVal)`
                // returned "<Type>" even when the type defined a real
                // to_string -- and std::collections `Set<T>`/`group_by` key off
                // `to_string(elem)`, so every distinct struct value collided on
                // "<Type>" (dedup under-counted, membership false-positived).
                if let MirType::Struct(n) | MirType::Enum(n) = &aty {
                    let base = n.split("___").next().unwrap_or(n).to_string();
                    let mangled = ctx
                        .method_owners
                        .get(&(n.clone(), "to_string".to_string()))
                        .or_else(|| {
                            ctx.method_owners
                                .get(&(base.clone(), "to_string".to_string()))
                        })
                        .cloned();
                    if let Some(m) = mangled {
                        let self_op = lower_expr_to_operand(ctx, &args[0]);
                        return RValue::Call {
                            func: m,
                            args: vec![self_op],
                        };
                    }
                }
                // `to_string(str)` is the identity, but the RESULT is a
                // DISTINCT owned str the caller drops (like any str-returning
                // call). Returning the argument's handle UNRETAINED made the
                // result alias the argument, so both drops freed the same
                // buffer -- a double-free of any rc=1 string reaching
                // to_string (a freshly computed/returned value, or a caught
                // exception's owned copy; a string LITERAL's rc masked it).
                // Retain so the result carries its own reference, matching the
                // `throw` site's retain when a str enters a second owner.
                if matches!(aty, MirType::Str) {
                    let s = lower_expr_to_operand(ctx, &args[0]);
                    if let Operand::Local(_) = &s {
                        let sink = ctx.alloc_temp(MirType::I64);
                        ctx.emit(Instruction::Assign {
                            dest: sink,
                            value: RValue::Call {
                                func: "kryos_string_retain_opt".to_string(),
                                args: vec![s.clone()],
                            },
                        });
                    }
                    return RValue::Use(s);
                }
                let placeholder = match &aty {
                    MirType::Struct(n) | MirType::Enum(n) => Some(format!("<{n}>")),
                    MirType::Array(..) => Some("<array>".to_string()),
                    MirType::Tuple(_) => Some("<tuple>".to_string()),
                    MirType::Map { .. } => Some("<map>".to_string()),
                    MirType::Function { .. } => Some("<fn>".to_string()),
                    _ => None,
                };
                if let Some(ph) = placeholder {
                    let _ = lower_expr_to_operand(ctx, &args[0]);
                    return RValue::ConstString(ph);
                }
            }

            // Check if this is an actor construction (e.g., `Counter()`).
            if ctx.actor_defs.contains_key(&func_name) {
                let dispatch_fn = format!("{func_name}__dispatch");
                let num_fields = ctx
                    .actor_state_fields
                    .get(&func_name)
                    .map(|f| f.len())
                    .unwrap_or(0);

                let state_operand = if num_fields > 0 {
                    // Allocate heap memory for actor state: num_fields * 8 bytes.
                    let alloc_size = (num_fields as i64) * 8;
                    let state_ptr = ctx.alloc_temp(MirType::I64);
                    ctx.emit(Instruction::Assign {
                        dest: state_ptr,
                        value: RValue::Call {
                            func: "kryos_arc_alloc_i64".into(),
                            args: vec![Operand::Constant(Constant::Int(alloc_size))],
                        },
                    });
                    // Initialize each state field to its default value.
                    // Clone the field layout to avoid borrow conflict with ctx.
                    let fields = ctx
                        .actor_state_fields
                        .get(&func_name)
                        .cloned()
                        .unwrap_or_default();
                    // Field types (same declaration order/names as `fields`), used to
                    // give collection-typed fields ([T] / map<K,V>) a valid EMPTY
                    // handle instead of a raw 0. A 0 there is a NULL array/map
                    // pointer: push/len/index/contains on it then either silently
                    // no-op on the JIT (kryos_builtin_push short-circuits `arr_handle
                    // == 0` and returns it unchanged, so the field never grows and
                    // len() always reads 0) or panic "array is null" on AOT (LLVM
                    // calls kryos_array_push directly, which null-checks and
                    // diverges). Scalar fields (i64/f64/bool/str) are correctly
                    // zero/null-safe by default, so only Array/Map get special init.
                    let field_types = ctx
                        .struct_defs
                        .get(&func_name)
                        .cloned()
                        .unwrap_or_default();
                    for (field_name, field_idx) in &fields {
                        let field_ty = field_types.iter().find(|(n, _)| n == field_name).map(|(_, t)| t.clone());
                        let init_value = match field_ty {
                            Some(ty @ MirType::Array(..)) => {
                                let tmp = ctx.alloc_temp(ty);
                                ctx.emit(Instruction::Assign {
                                    dest: tmp,
                                    value: RValue::Array(vec![]),
                                });
                                Operand::Local(tmp)
                            }
                            Some(ty @ MirType::Map { .. }) => {
                                let tmp = ctx.alloc_temp(ty);
                                ctx.emit(Instruction::Assign {
                                    dest: tmp,
                                    value: RValue::Map(vec![]),
                                });
                                Operand::Local(tmp)
                            }
                            // Aggregate state (struct/enum/tuple field): a raw 0
                            // is a NULL aggregate pointer -- reading the field
                            // before its first store dereferences null (JIT
                            // segfault; AOT unbox crash). Give it a ZEROED box
                            // of the right size instead (kryos_arc_alloc_i64
                            // returns zeroed memory), mirroring the empty-handle
                            // init for Array/Map. Field words: struct = field
                            // count, enum = 1 (tag) + max payload fields,
                            // tuple = element count.
                            Some(MirType::Struct(ref sn)) | Some(MirType::Enum(ref sn)) => {
                                let words = if let Some(fs) = ctx.struct_defs.get(sn) {
                                    fs.len().max(1)
                                } else if let Some(vs) = ctx.enum_defs.get(sn) {
                                    1 + vs.iter().map(|v| v.fields.len()).max().unwrap_or(0)
                                } else {
                                    1
                                };
                                let tmp = ctx.alloc_temp(MirType::I64);
                                ctx.emit(Instruction::Assign {
                                    dest: tmp,
                                    value: RValue::Call {
                                        func: "kryos_arc_alloc_i64".into(),
                                        args: vec![Operand::Constant(Constant::Int(
                                            (words as i64) * 8,
                                        ))],
                                    },
                                });
                                Operand::Local(tmp)
                            }
                            Some(MirType::Tuple(ref elems)) => {
                                // A real zero-valued TUPLE, not a raw zeroed
                                // buffer: on the JIT a tuple is a KryosArray,
                                // so reading a raw box as one panics "index 0
                                // but length is 0" on the first field access.
                                // Elements zero-init per slot (heap elements
                                // get a 0 handle -- a null str reads as "",
                                // matching scalar state fields).
                                let ty = MirType::Tuple(elems.clone());
                                let zeros: Vec<Operand> = elems
                                    .iter()
                                    .map(|_| Operand::Constant(Constant::Int(0)))
                                    .collect();
                                let tmp = ctx.alloc_temp(ty);
                                ctx.emit(Instruction::Assign {
                                    dest: tmp,
                                    value: RValue::Tuple(zeros),
                                });
                                Operand::Local(tmp)
                            }
                            _ => Operand::Constant(Constant::Int(0)),
                        };
                        ctx.emit(Instruction::ActorStateStore {
                            state_ptr,
                            field_offset: *field_idx,
                            value: init_value,
                        });
                    }
                    Operand::Local(state_ptr)
                } else {
                    // No state fields — pass 0 as state pointer.
                    Operand::Constant(Constant::Int(0))
                };

                let result = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::ActorSpawn {
                    dest: result,
                    dispatch_fn,
                    state: state_operand,
                });
                return RValue::Use(Operand::Local(result));
            }

            // Check if this is an enum variant constructor (e.g., `Some(42)`).
            if let Some((enum_name, variant_idx)) = find_enum_variant(ctx, &func_name) {
                let mir_args: Vec<Operand> =
                    args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                // A bare heap-local passed directly as a variant field is moved
                // into the enum -- the field is a bit-copy with no retain. Mark
                // the local drop-suppressed so its scope-end drop does not free
                // the payload the returned Result/Option now owns. Without this,
                // `Err(e)` / `Some(s)` / `Ok(v)` built from a named local (most
                // visibly the catch binding in `catch e { Err(e) }`, in tail,
                // `let`, and return positions) returned a freed string: empty on
                // AOT, corrupt on JIT.
                //
                // NOTE: the ownership pass currently treats enum-variant/call
                // args as a use (borrow), not a move, so `Err(e)` followed by a
                // second `Ok(e)` (aliasing one local into two enums) is not
                // rejected. Suppressing the drop there LEAKS the extra reference
                // rather than double-freeing it -- strictly safer than the
                // pre-fix behaviour (a use-after-free of the single owner). A
                // fully-refcounted fix (retain str/array/map fields on enum
                // construction) would also plug the leak; tracked separately.
                suppress_enum_field_arg_drops(ctx, args);
                return RValue::EnumVariant {
                    enum_name,
                    variant_idx,
                    fields: mir_args,
                };
            }

            // Map builtins: when the first arg is a map, dispatch to the
            // kryos_map_* runtime by the map's KEY type. Otherwise these names
            // fall through to an undefined symbol (`keys`, `map_has`) or the
            // string `contains`. The str-key variants are already covered by
            // runtime_param_types so the key is coerced to an i64 handle.
            if matches!(
                func_name.as_str(),
                "contains" | "map_has" | "keys" | "map_keys" | "map_delete"
            ) && !args.is_empty()
            {
                let obj_ty = infer_expr_type(ctx, &args[0]);
                if let MirType::Map { key, .. } = &obj_ty {
                    let key_is_str = matches!(key.as_ref(), MirType::Str);
                    let rt = match func_name.as_str() {
                        "contains" | "map_has" => {
                            if key_is_str { "kryos_map_has_str" } else { "kryos_map_has" }
                        }
                        "keys" | "map_keys" => {
                            if key_is_str { "kryos_map_keys_str" } else { "kryos_map_keys" }
                        }
                        "map_delete" => {
                            if key_is_str { "kryos_map_delete_str" } else { "kryos_map_delete" }
                        }
                        _ => unreachable!(),
                    };
                    let mir_args: Vec<Operand> =
                        args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                    return RValue::Call {
                        func: rt.to_string(),
                        args: mir_args,
                    };
                }
            }

            // Check if this is a call to a generic function — monomorphize.
            if ctx.generic_templates.contains_key(&func_name) {
                let mangled = monomorphize(ctx, &func_name, args);
                // Apply the dyn-Trait coercion here too: a concrete struct arg
                // passed to a `dyn Trait` param must be wrapped in a fat pointer
                // (MakeTraitObject), exactly as the non-generic call path does
                // below. This branch previously lowered every arg RAW, so a
                // struct passed to a generic fn's `dyn Trait` param reached the
                // monomorphized body's `vtable_call` as a plain struct -> the
                // vtable deref segfaulted on BOTH backends. A `dyn Trait` param
                // type is concrete (no dependence on the generic type params),
                // so the template's declared param types carry it directly.
                let template_param_tys: Vec<Option<ast::TypeExpr>> = ctx
                    .generic_templates
                    .get(&func_name)
                    .map(|t| t.params.iter().map(|p| p.ty.clone()).collect())
                    .unwrap_or_default();
                let mir_args: Vec<Operand> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let operand = lower_expr_to_operand(ctx, a);
                        if let Some(Some(ast::TypeExpr::DynTrait { trait_name, .. })) =
                            template_param_tys.get(i)
                        {
                            if let MirType::Struct(ref concrete_type) = infer_expr_type(ctx, a) {
                                let tmp =
                                    ctx.alloc_temp(MirType::DynTrait(trait_name.clone()));
                                ctx.emit(Instruction::Assign {
                                    dest: tmp,
                                    value: RValue::MakeTraitObject {
                                        value: operand,
                                        concrete_type: concrete_type.clone(),
                                        trait_name: trait_name.clone(),
                                    },
                                });
                                return Operand::Local(tmp);
                            }
                        }
                        operand
                    })
                    .collect();
                return RValue::Call {
                    func: mangled,
                    args: mir_args,
                };
            }

            // Check if the callee is a local with function type (indirect call).
            if func_name != "<closure>" {
                let is_fn_local = ctx
                    .locals
                    .iter()
                    .rev()
                    .find(|l| l.name.as_deref() == Some(&func_name))
                    .map(|l| {
                        // A local whose MIR type is Function is obviously an
                        // indirect callee. But a lambda param whose function type
                        // flowed through a generic HOF over an array of functions
                        // (`map(fs, |f| f(x))`, element type `fn(i64)->i64`) gets
                        // its type ERASED to i64 -- yet it is still a closure value.
                        // If a local with this name exists and it shadows no
                        // top-level function, calling it MUST be an indirect call
                        // through that local (the type checker already proved the
                        // value is callable); a direct call emitted a reference to
                        // an undefined external symbol named after the param
                        // (LNK2001 unresolved external `f`).
                        matches!(l.ty, MirType::Function { .. })
                            || !ctx.func_ret_types.contains_key(&func_name)
                    })
                    .unwrap_or(false)
                    // A self-recursive call from INSIDE a `let name = |...| {..}`
                    // lambda's own body: `name` has no local yet at this point
                    // (the enclosing `let` only allocates it AFTER the RHS is
                    // fully lowered), but the `Expr::Lambda` arm pre-registered
                    // `name` in `closure_locals` for exactly this case. Without
                    // this, the call fell through to the plain-function-call
                    // fallback below and referenced a top-level symbol literally
                    // named `name`, which does not exist (the hoisted function
                    // is `__lambda_N`).
                    || ctx.closure_locals.contains_key(&func_name);
                if is_fn_local {
                    // If this local is a tracked closure with captures,
                    // emit a direct call with captures prepended.
                    if let Some((real_func, capture_ops)) =
                        ctx.closure_locals.get(&func_name).cloned()
                    {
                        let mut mir_args: Vec<Operand> = capture_ops;
                        for a in args {
                            mir_args.push(lower_expr_to_operand(ctx, a));
                        }
                        return RValue::Call {
                            func: real_func,
                            args: mir_args,
                        };
                    }

                    let callee_local = find_local_by_name(ctx, &func_name)
                        .expect("internal: indirect call callee local not found");
                    let mir_args: Vec<Operand> =
                        args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                    return RValue::CallIndirect {
                        callee: Operand::Local(callee_local),
                        args: mir_args,
                    };
                }
            }

            // Check for dyn Trait coercion: if a parameter type is DynTrait
            // and the argument is a concrete struct, wrap with MakeTraitObject.
            let param_types = ctx.func_param_types.get(&func_name).cloned();
            let mir_args: Vec<Operand> = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let operand = lower_expr_to_operand(ctx, a);
                    // push(dest_arr, struct_index_or_field_read): on the
                    // Cranelift backend, structs AND enums are uniformly raw
                    // heap pointers with no ref_count of their own (no
                    // by-value SSA aggregate like LLVM's `load %S`), so
                    // handing this read straight to push aliases the SAME
                    // heap block between the SOURCE container (still holding
                    // it, e.g. `students`) and the new destination array
                    // (`passing`). Both containers' scope-end drops then
                    // free it once each -> double-free (Cranelift-only exit
                    // 127 at teardown; LLVM is immune because reading a
                    // struct/enum-typed element there is inherently a value
                    // copy). Insert an explicit clone call between the read
                    // and the push so the destination owns an independent
                    // instance.
                    //
                    // Two aliasing shapes reach push() this way:
                    //   (1) the arg IS the aliasing read directly:
                    //       `push(dest, arr[i])` / `push(dest, s.field)`.
                    //   (2) the arg is a bare identifier bound one level back
                    //       via `let s = arr[i]` -- `s` is already tracked in
                    //       `borrowed_locals` (it never took ownership, see
                    //       the Let-lowering above) and then handed to push
                    //       unmodified: `push(dest, s)`. This is the common
                    //       match/filter-into-a-second-array shape (`let s =
                    //       shapes[i]; if cond { big = push(big, s) }`) and
                    //       was NOT covered by shape (1) alone.
                    // Gated to exactly these aliasing shapes, NOT fresh
                    // struct/enum literals or constructor-call results: those
                    // already own their sole allocation, and
                    // `consume_call_args` below unconditionally suppresses a
                    // pushed struct/enum arg's own scope-end drop, so cloning
                    // a fresh value here would leak the original (nothing
                    // else would ever free it).
                    let is_direct_alias_read = matches!(
                        a,
                        ast::Expr::IndexAccess { .. } | ast::Expr::FieldAccess { .. }
                    );
                    let is_borrowed_bare_ident = if let ast::Expr::Identifier {
                        name: id_name, ..
                    } = a
                    {
                        find_local_by_name(ctx, id_name)
                            .is_some_and(|l| ctx.borrowed_locals.contains(&l.0))
                    } else {
                        false
                    };
                    let operand = if func_name == "push"
                        && i == 1
                        && (is_direct_alias_read || is_borrowed_bare_ident)
                    {
                        match infer_expr_type(ctx, a) {
                            MirType::Struct(ref sname) if ctx.struct_defs.contains_key(sname) => {
                                let cloned = ctx.alloc_temp(MirType::Struct(sname.clone()));
                                ctx.emit(Instruction::Assign {
                                    dest: cloned,
                                    value: RValue::Call {
                                        func: "__kryos_struct_index_clone".to_string(),
                                        args: vec![operand],
                                    },
                                });
                                Operand::Local(cloned)
                            }
                            MirType::Enum(ref ename) if ctx.enum_defs.contains_key(ename) => {
                                let cloned = ctx.alloc_temp(MirType::Enum(ename.clone()));
                                ctx.emit(Instruction::Assign {
                                    dest: cloned,
                                    value: RValue::Call {
                                        func: "__kryos_enum_index_clone".to_string(),
                                        args: vec![operand],
                                    },
                                });
                                Operand::Local(cloned)
                            }
                            _ => operand,
                        }
                    } else {
                        operand
                    };
                    // Check if this param expects a dyn Trait.
                    if let Some(ref ptypes) = param_types {
                        if let Some(MirType::DynTrait(ref trait_name)) = ptypes.get(i) {
                            let arg_type = infer_expr_type(ctx, a);
                            if let MirType::Struct(ref concrete_type) = arg_type {
                                // Emit MakeTraitObject to wrap the struct into a fat pointer.
                                let tmp = ctx.alloc_temp(MirType::DynTrait(trait_name.clone()));
                                ctx.emit(Instruction::Assign {
                                    dest: tmp,
                                    value: RValue::MakeTraitObject {
                                        value: operand,
                                        concrete_type: concrete_type.clone(),
                                        trait_name: trait_name.clone(),
                                    },
                                });
                                return Operand::Local(tmp);
                            }
                        }
                    }
                    operand
                })
                .collect();
            RValue::Call {
                func: func_name,
                args: mir_args,
            }
        }

        ast::Expr::MethodCall {
            object,
            method,
            args,
            ..
        } => {
            // Check if this is an enum variant constructor with data (e.g. Shape.Circle(3)).
            if let ast::Expr::Identifier { name, .. } = object.as_ref() {
                if let Some(variants) = ctx.enum_defs.get(name.as_str()) {
                    if let Some((idx, _)) =
                        variants.iter().enumerate().find(|(_, v)| v.name == *method)
                    {
                        let fields: Vec<Operand> =
                            args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                        // See suppress_enum_field_arg_drops: a bare heap-local
                        // arg is moved into the enum; without this, dotted
                        // `Enum.Variant(args)` double-frees a heap-owning
                        // (e.g. recursive enum) field at scope exit.
                        suppress_enum_field_arg_drops(ctx, args);
                        return RValue::EnumVariant {
                            enum_name: name.clone(),
                            variant_idx: idx as u32,
                            fields,
                        };
                    }
                }

                // Static method call via dot syntax: `TypeName.method(args)`.
                // A generic associated fn (no `self`) inside a generic impl
                // block must monomorphize a fresh specialization per call --
                // see `monomorphize_impl_fn`. Must run BEFORE the plain
                // `method_owners` lookup below, which always resolves to the
                // single base-name copy (the bug this fixes).
                if ctx
                    .generic_impl_fn_templates
                    .get(&(name.clone(), method.clone()))
                    .is_some_and(|t| !t.has_self)
                {
                    let mangled = monomorphize_impl_fn(ctx, name, method, args, None);
                    let mir_args: Vec<Operand> =
                        args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                    return RValue::Call {
                        func: mangled,
                        args: mir_args,
                    };
                }
                // If the receiver is an identifier naming a struct or enum
                // (not a value), treat the call as a static/associated
                // function call. This mirrors the checker behavior and lets
                // `List.new()`, `Dict.new()`, etc. work like `List::new()`.
                if ctx.struct_defs.contains_key(name.as_str())
                    || ctx.enum_defs.contains_key(name.as_str())
                {
                    let mir_args: Vec<Operand> =
                        args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                    let func_name = ctx
                        .method_owners
                        .get(&(name.clone(), method.clone()))
                        .cloned()
                        .unwrap_or_else(|| format!("{name}__{method}"));
                    return RValue::Call {
                        func: func_name,
                        args: mir_args,
                    };
                }
            }

            // Check if this is a method call on an actor (message send).
            let type_name = infer_type_name(ctx, object);

            // A generic INSTANCE method (has `self`) whose return type
            // CONSTRUCTS a generic struct (e.g. `Foo<T>.to_box() -> Box<T>`)
            // must monomorphize a fresh specialization bound from the
            // RECEIVER's concrete type -- see `monomorphize_impl_fn` and
            // `GenericImplFnTemplate::has_self`. Must run BEFORE the plain
            // `method_owners` lookup at the bottom of this arm, which always
            // resolves to the base struct's SINGLE lowered copy (the bug
            // this fixes). The common bare `-> T` instance method (e.g.
            // `Box<T>::get`) is never registered as a `has_self` template
            // (see `instance_ret_needs_monomorphization`), so it falls
            // through unaffected to the existing single-lowered-copy path.
            if let Some(ref tn) = type_name {
                let base = tn.split("___").next().unwrap_or(tn.as_str()).to_string();
                if ctx
                    .generic_impl_fn_templates
                    .get(&(base.clone(), method.clone()))
                    .is_some_and(|t| t.has_self)
                {
                    let receiver_ty = MirType::Struct(tn.clone());
                    let obj = lower_expr_to_operand(ctx, object);
                    let mangled = monomorphize_impl_fn(ctx, &base, method, args, Some(&receiver_ty));
                    let mut mir_args: Vec<Operand> = vec![obj];
                    for a in args {
                        mir_args.push(lower_expr_to_operand(ctx, a));
                    }
                    return RValue::Call {
                        func: mangled,
                        args: mir_args,
                    };
                }
            }

            if let Some(ref tn) = type_name {
                if let Some(handlers) = ctx.actor_defs.get(tn.as_str()).cloned() {
                    if let Some((idx, _)) =
                        handlers.iter().enumerate().find(|(_, (h, _))| h == method)
                    {
                        let obj = lower_expr_to_operand(ctx, object);
                        // Ensure actor is in a local for ActorSend.
                        let actor_local = match obj {
                            Operand::Local(id) => id,
                            _ => {
                                let tmp = ctx.alloc_temp(MirType::I64);
                                ctx.emit(Instruction::Assign {
                                    dest: tmp,
                                    value: RValue::Use(obj),
                                });
                                tmp
                            }
                        };
                        let send_args: Vec<Operand> =
                            args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                        ctx.emit(Instruction::ActorSend {
                            actor: actor_local,
                            handler_tag: (idx as u32) + 1,
                            args: send_args,
                        });
                        return RValue::ConstInt(0); // fire-and-forget
                    }
                }
            }

            // Check if this is a method call on a dyn Trait value (dynamic dispatch).
            let obj_type = infer_expr_type(ctx, object);
            if let MirType::DynTrait(ref trait_name) = obj_type {
                // Look up the method index in the trait definition.
                if let Some(methods) = ctx.trait_defs.get(trait_name.as_str()).cloned() {
                    if let Some(method_idx) = methods.iter().position(|m| m.name == *method) {
                        let ret_ty = methods[method_idx].ret_ty.clone();
                        let param_tys = methods[method_idx].param_types.clone();
                        let obj = lower_expr_to_operand(ctx, object);
                        // dyn Trait coercion on DYNAMIC-dispatch arguments: the
                        // callee is unknown at compile time, so the coercion
                        // targets come from the TRAIT method signature (self
                        // excluded, aligning with args). Without this, a
                        // concrete struct passed to a dyn param of a method
                        // called ON a dyn receiver (`boxed.heavier(other)`)
                        // reached the vtable body raw -> SIGSEGV both backends.
                        let mut call_args: Vec<Operand> = Vec::new();
                        for (i, a) in args.iter().enumerate() {
                            let operand = lower_expr_to_operand(ctx, a);
                            let operand = match param_tys.get(i) {
                                Some(target_ty) => {
                                    coerce_to_dyn_if_needed(ctx, operand, target_ty, a)
                                }
                                None => operand,
                            };
                            call_args.push(operand);
                        }
                        return RValue::VtableCall {
                            object: obj,
                            method_index: method_idx as u32,
                            args: call_args,
                            return_ty: ret_ty,
                        };
                    }
                }
            }

            // Check if this is a Function-typed struct field being called
            // (e.g. `t.transform(5)` where `transform: fn(i64) -> i64`).
            if let Some(ref tn) = type_name {
                let is_fn_field = ctx
                    .struct_defs
                    .get(tn.as_str())
                    .and_then(|fields| fields.iter().find(|(n, _)| n == method))
                    .map(|(_, ty)| matches!(ty, MirType::Function { .. }))
                    .unwrap_or(false);
                if is_fn_field {
                    let obj_val = lower_expr_to_operand(ctx, object);
                    // Load the closure (Function) from the struct field.
                    let fn_ptr_temp = ctx.alloc_temp(MirType::Function {
                        params: vec![],
                        ret: Box::new(MirType::I64),
                    });
                    ctx.emit(Instruction::Assign {
                        dest: fn_ptr_temp,
                        value: RValue::Field {
                            object: obj_val,
                            field: method.clone(),
                        },
                    });
                    let mir_args: Vec<Operand> =
                        args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                    return RValue::CallIndirect {
                        callee: Operand::Local(fn_ptr_temp),
                        args: mir_args,
                    };
                }
            }

            // A bare `-> T` generic INSTANCE method (`Nest<T>.get() -> T`,
            // `Box<T>.get() -> T`) is compiled ONCE as a single erased body
            // (see `instance_ret_needs_monomorphization`'s doc) that does a
            // bare `extractvalue self, 0; ret` -- it cannot retain the field
            // it hands out because it does not know, at that single shared
            // compile site, whether T is a heap type. `self` is passed byval
            // (caller-owned; the callee never drops it), so returning the
            // field bit-for-bit hands the CALLER's own reference out a
            // second time with no compensating increment: when T resolves to
            // a container (str/[T]/map) at THIS call site, the receiver's
            // own eventual drop AND the destination's eventual drop each
            // free the SAME handle -- an AOT-only double-free (JIT/Cranelift
            // boxes every field as a pointer uniformly, so this class of bug
            // never surfaces there; confirmed via `KRYOS_FREE_DIAG=1` on
            // `Nest<map<str,i64>>.get()` -- "map DOUBLE-FREE rc=0"). Resolve
            // the receiver's concrete binding the same way `infer_expr_type`
            // already does to TYPE this call's result correctly, and where
            // that resolves to a container type, give the destination its
            // own "borrow-to-own" reference -- mirrors
            // `emit_param_source_retain`/`retain_for_ty`, used for the
            // analogous param-source and match-bind cases.
            if let Some(ref tn) = type_name {
                let base = tn.split("___").next().unwrap_or(tn.as_str()).to_string();
                if let Some((gp_names, self_te, ret_te)) = ctx
                    .impl_method_generic_info
                    .get(&(base.clone(), method.clone()))
                    .cloned()
                {
                    let ret_is_bare_param = matches!(
                        &ret_te,
                        Some(ast::TypeExpr::Simple { name, .. })
                            if gp_names.iter().any(|g| g == name)
                    );
                    if ret_is_bare_param {
                        if let (Some(self_te), Some(ret_te)) = (self_te, ret_te) {
                            let receiver = MirType::Struct(tn.clone());
                            let mut bindings: HashMap<String, MirType> = HashMap::new();
                            extract_type_bindings(ctx, &self_te, &receiver, &gp_names, &mut bindings);
                            if !bindings.is_empty() {
                                let resolved =
                                    substitute_type_expr_to_mir(ctx, &ret_te, &bindings);
                                if let Some(rf) = retain_for_ty(&resolved) {
                                    let obj = lower_expr_to_operand(ctx, object);
                                    let mut mir_args: Vec<Operand> = vec![obj];
                                    for a in args {
                                        mir_args.push(lower_expr_to_operand(ctx, a));
                                    }
                                    let func_name = ctx
                                        .method_owners
                                        .get(&(tn.clone(), method.clone()))
                                        .cloned()
                                        .or_else(|| {
                                            if base != *tn {
                                                ctx.method_owners
                                                    .get(&(base.clone(), method.clone()))
                                                    .cloned()
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or_else(|| method.clone());
                                    let call_tmp = ctx.alloc_temp(resolved.clone());
                                    ctx.emit(Instruction::Assign {
                                        dest: call_tmp,
                                        value: RValue::Call {
                                            func: func_name,
                                            args: mir_args,
                                        },
                                    });
                                    let sink = ctx.alloc_temp(MirType::I64);
                                    ctx.emit(Instruction::Assign {
                                        dest: sink,
                                        value: RValue::Call {
                                            func: rf.to_string(),
                                            args: vec![Operand::Local(call_tmp)],
                                        },
                                    });
                                    return RValue::Use(Operand::Local(call_tmp));
                                }
                            }
                        }
                    }
                }
            }

            // Resolve mangled method name: infer the object's type and look up
            // the impl method as TypeName__method. Resolved BEFORE lowering
            // the call args (below) so the dyn-Trait-param coercion can look
            // up this method's declared param types by its final name.
            let func_name = if let Some(tn) = type_name {
                ctx.method_owners
                    .get(&(tn.clone(), method.clone()))
                    .cloned()
                    .or_else(|| {
                        // Generic-struct instance: `tn` is a monomorphized name
                        // like `Wrap___i64` (mono_mangled_name uses `___`), but
                        // impl methods are lowered once under the base struct
                        // name (`Wrap__get`) with the uniform i64-slot `self`
                        // layout. Fall back to the base name so methods on
                        // generic structs link and dispatch correctly.
                        let base = tn.split("___").next().unwrap_or(tn.as_str());
                        if base != tn {
                            ctx.method_owners
                                .get(&(base.to_string(), method.clone()))
                                .cloned()
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| method.clone())
            } else {
                method.clone()
            };

            let obj = lower_expr_to_operand(ctx, object);
            // dyn Trait coercion on method-call arguments: mirrors the
            // free-function FnCall path -- a concrete struct arg passed to a
            // `dyn Trait` param of the resolved method must be wrapped in
            // MakeTraitObject, or the callee's vtable_call derefs a raw
            // struct instead of a fat pointer (SIGSEGV on both backends).
            let param_types = ctx.func_param_types.get(&func_name).cloned();
            let mut mir_args: Vec<Operand> = vec![obj];
            for (i, a) in args.iter().enumerate() {
                let operand = lower_expr_to_operand(ctx, a);
                let operand = match param_types.as_ref().and_then(|p| p.get(i)) {
                    Some(target_ty) => coerce_to_dyn_if_needed(ctx, operand, target_ty, a),
                    None => operand,
                };
                mir_args.push(operand);
            }

            RValue::Call {
                func: func_name,
                args: mir_args,
            }
        }

        ast::Expr::StaticMethodCall {
            type_name,
            method,
            args,
            ..
        } => {
            // `Enum::Variant(args)` (Rust-style path) constructs an enum value, not
            // a static method call. Mirror the `Enum.Variant(args)` MethodCall path
            // above. Without this, `Opt::Some(7)` lowered to a call of the
            // nonexistent function `Opt__Some` (unresolved symbol on the JIT, a
            // `store void` codegen error on AOT) even though the checker already
            // type-checks it as an enum construction.
            if let Some(variants) = ctx.enum_defs.get(type_name.as_str()) {
                if let Some((idx, _)) =
                    variants.iter().enumerate().find(|(_, v)| v.name == *method)
                {
                    let fields: Vec<Operand> =
                        args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                    // See suppress_enum_field_arg_drops: a bare heap-local arg
                    // is moved into the enum; without this, Rust-style
                    // `Enum::Variant(args)` double-frees a heap-owning (e.g.
                    // recursive enum) field at scope exit.
                    suppress_enum_field_arg_drops(ctx, args);
                    return RValue::EnumVariant {
                        enum_name: type_name.clone(),
                        variant_idx: idx as u32,
                        fields,
                    };
                }
            }
            // Rust-style spelling of the generic-associated-fn call handled
            // in the `MethodCall` (dotted) arm above -- see
            // `monomorphize_impl_fn`. Must run BEFORE the plain
            // `method_owners` lookup below.
            if ctx
                .generic_impl_fn_templates
                .get(&(type_name.clone(), method.clone()))
                .is_some_and(|t| !t.has_self)
            {
                let mangled = monomorphize_impl_fn(ctx, type_name, method, args, None);
                let mir_args: Vec<Operand> =
                    args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
                return RValue::Call {
                    func: mangled,
                    args: mir_args,
                };
            }
            let func_name = ctx
                .method_owners
                .get(&(type_name.clone(), method.clone()))
                .cloned()
                .unwrap_or_else(|| {
                    // If type_name is not a known struct/enum, it's a module alias.
                    // Module-level functions are registered with their plain name.
                    if !ctx.struct_defs.contains_key(type_name.as_str())
                        && !ctx.enum_defs.contains_key(type_name.as_str())
                    {
                        method.clone()
                    } else {
                        format!("{type_name}__{method}")
                    }
                });
            // dyn Trait coercion on Rust-style `Type::method(args)` arguments:
            // this arm lowered args RAW, so a concrete struct passed to a
            // `dyn Trait` param reached vtable_call unwrapped (JIT segfault /
            // AOT crash). Mirrors the dotted MethodCall path. func_param_types
            // excludes `self`, which aligns with a no-self static method's
            // args; a UFCS receiver arg gets no entry (None) and is unchanged.
            let param_types = ctx.func_param_types.get(&func_name).cloned();
            let mir_args: Vec<Operand> = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let operand = lower_expr_to_operand(ctx, a);
                    match param_types.as_ref().and_then(|p| p.get(i)) {
                        Some(target_ty) => coerce_to_dyn_if_needed(ctx, operand, target_ty, a),
                        None => operand,
                    }
                })
                .collect();
            RValue::Call {
                func: func_name,
                args: mir_args,
            }
        }

        ast::Expr::ArrayLiteral { elements, .. } => {
            let ops: Vec<Operand> = elements
                .iter()
                .map(|e| {
                    let op = lower_expr_to_operand(ctx, e);
                    retain_or_move_literal_element(ctx, e, &op);
                    op
                })
                .collect();
            RValue::Array(ops)
        }

        ast::Expr::TupleLiteral { elements, .. } => {
            let ops: Vec<Operand> = elements
                .iter()
                .map(|e| {
                    let op = lower_expr_to_operand(ctx, e);
                    retain_or_move_literal_element(ctx, e, &op);
                    op
                })
                .collect();
            RValue::Tuple(ops)
        }

        ast::Expr::StructLiteral { name, fields, .. } => {
            let effective_name = resolve_struct_literal_name(ctx, name, fields);
            let mir_fields: Vec<(String, Operand)> = fields
                .iter()
                .map(|(n, e)| {
                    // A closure literal stored DIRECTLY in a struct field
                    // (`Holder { f: |x| x * k }`) is never `let`-bound, so
                    // `closure_locals`'s direct-call optimization (the ONLY
                    // thing currently giving non-mutated captures correct
                    // by-reference semantics -- see gotcha #11) can never
                    // apply to it. Flag it so the Lambda RValue arm boxes its
                    // non-mutated scalar captures instead of the default
                    // value-snapshot-at-construction capture.
                    if matches!(e, ast::Expr::Lambda { .. }) {
                        ctx.pending_box_scalar_captures = true;
                    }
                    let op = lower_expr_to_operand(ctx, e);
                    // A refcounted value (str/array/map) read out of ANOTHER
                    // struct's field and stored into this literal creates a
                    // second independent owner: both drops decrement a count
                    // only one of them holds. Retain at construction.
                    // Repro: majority_vote's `Probable { value: winner.value }`
                    // -- the array element drop AND the result drop freed the
                    // same string (STATUS_HEAP_CORRUPTION at exit). Fresh
                    // temps (literals, calls, concats) are NOT retained --
                    // they hand over their own +1.
                    //
                    // A BARE IDENTIFIER field (`Rec { tag: tag, .. }`) is the
                    // same hazard: the field's value is the source local's
                    // pointer, shallow-copied with no retain, while the
                    // source local ALSO gets an ordinary scope-end `Drop` (in
                    // a loop, every iteration). `push`ing the literal into an
                    // array boxes that shallow copy, so the array element and
                    // the source local end up decrementing the same
                    // refcount: a premature free that either corrupts the
                    // element on next reuse of the freed slot, or -- if nothing
                    // reallocates that exact memory before it's read back --
                    // silently "looks correct" by allocator-timing luck.
                    // Repro: `while i < n { let tag = ..; arr = push(arr,
                    // Rec{tag: tag, n: i}) }` double-frees `tag`'s buffer on
                    // AOT (kryos.dev heap-struct-array-str double-free hunt).
                    // Skip the retain for a PARAM/borrowed source: those never
                    // get a scope-end Drop, so retaining would leak instead
                    // (the ownership already transferred once at the call
                    // boundary / borrow site).
                    let source_undropped = matches!(&op, Operand::Local(id)
                        if ctx.param_locals.contains(&id.0) || ctx.borrowed_locals.contains(&id.0));
                    let is_bare_ident = matches!(e, ast::Expr::Identifier { .. }) && !source_undropped;
                    let aliases = matches!(
                        e,
                        ast::Expr::FieldAccess { .. } | ast::Expr::IndexAccess { .. }
                    ) || is_bare_ident;
                    if aliases {
                        let ty = infer_expr_type(ctx, e);
                        let retain_fn = match ty {
                            MirType::Str => Some("kryos_string_retain"),
                            // NOT Array here for the bare-identifier case: the
                            // LLVM backend's `emit_aggregate_struct` ALREADY
                            // unconditionally clones every Array-typed struct
                            // field (`kryos_array_clone`, a pre-existing fix
                            // for a DIFFERENT bug -- two fields sharing one
                            // buffer). The struct ends up holding an
                            // independent copy, never this operand, so
                            // retaining `op` here has no consumer and leaks
                            // (verified: retain added, then only ONE matching
                            // release ever fires -- confirmed via a
                            // repeated build+drop stress run ballooning to
                            // 100s of MB where the pre-fix / str-field
                            // equivalent stayed flat). FieldAccess/IndexAccess
                            // aliasing (the pre-existing branch below) is left
                            // as-is; only the new bare-identifier path skips
                            // Array to avoid compounding with that clone.
                            MirType::Array(_, _) if !is_bare_ident => Some("kryos_array_retain"),
                            MirType::Map { .. } => Some("kryos_map_retain"),
                            _ => None,
                        };
                        if let Some(f) = retain_fn {
                            let scratch = ctx.alloc_temp(MirType::I64);
                            ctx.emit(Instruction::Assign {
                                dest: scratch,
                                value: RValue::Call {
                                    func: f.into(),
                                    args: vec![op.clone()],
                                },
                            });
                        } else if is_bare_ident && matches!(ty, MirType::Struct(_) | MirType::Enum(_))
                        {
                            // Non-@copy struct/enum sourced from a bare
                            // identifier: there's no refcounted box to retain
                            // (structs are raw aggregate values whose heap-
                            // owning subfields get shallow-copied by value),
                            // so mirror the array-literal-of-struct-
                            // identifiers fix (`partial_moved_locals`
                            // elsewhere in this file): mark the source
                            // local's own scope-end drop as already
                            // accounted for -- the value's ownership (and
                            // that of any heap fields it carries) moved into
                            // this new struct literal.
                            if let Operand::Local(id) = &op {
                                ctx.partial_moved_locals.insert(id.0);
                            }
                        }
                    }
                    // dyn Trait field coercion: if the struct's declared
                    // field type is `dyn Trait` and the provided value is a
                    // concrete struct, wrap it in MakeTraitObject. Applied
                    // LAST (after the aliasing/retain/partial-move handling
                    // above) so that bookkeeping still targets the ORIGINAL
                    // source operand, matching the call-argument coercion's
                    // ordering. Without this, `Holder{ inner: r }.inner` held
                    // a raw struct behind a `dyn Shape` field and the later
                    // vtable_call segfaulted.
                    let field_ty = ctx.struct_defs.get(effective_name.as_str()).and_then(|fs| {
                        fs.iter().find(|(fname, _)| fname == n).map(|(_, t)| t.clone())
                    });
                    let op = if let Some(ref fty) = field_ty {
                        coerce_to_dyn_if_needed(ctx, op, fty, e)
                    } else {
                        op
                    };
                    (n.clone(), op)
                })
                .collect();
            RValue::Struct {
                name: effective_name,
                fields: mir_fields,
            }
        }

        ast::Expr::FieldAccess { object, field, .. } => {
            // Check if this is an enum variant construction (e.g., `Color.Red`).
            if let ast::Expr::Identifier { name, .. } = object.as_ref() {
                if let Some(variants) = ctx.enum_defs.get(name.as_str()) {
                    if let Some(idx) = variants.iter().position(|v| v.name == field.as_str()) {
                        return RValue::EnumVariant {
                            enum_name: name.clone(),
                            variant_idx: idx as u32,
                            fields: vec![],
                        };
                    }
                }
            }

            // Check if this is an actor state field access (self.field in a handler).
            if let ast::Expr::Identifier { name, .. } = object.as_ref() {
                if name == "self" {
                    // Determine the actor type from the self param's struct type.
                    let self_local = find_local_by_name(ctx, "self")
                        .expect("internal: 'self' local not found in field access");
                    let actor_name = ctx
                        .locals
                        .iter()
                        .find(|l| l.id == self_local)
                        .and_then(|l| match &l.ty {
                            MirType::Struct(n) => Some(n.clone()),
                            _ => None,
                        })
                        // Actor VALUES erase to i64; fall back to the actor
                        // whose handler is currently being lowered.
                        .or_else(|| ctx.current_actor.clone());
                    if let Some(ref aname) = actor_name {
                        if let Some(fields) = ctx.actor_state_fields.get(aname).cloned() {
                            if let Some((_fname, field_idx)) =
                                fields.iter().find(|(n, _)| n == field)
                            {
                                // Type the load by the FIELD's declared type
                                // (recorded in struct_defs at actor
                                // registration). An i64-typed dest made both
                                // backends mis-select ops on f64/str state
                                // (iadd on f64; ptr stored as i64 on AOT).
                                let fty = ctx
                                    .struct_defs
                                    .get(aname.as_str())
                                    .and_then(|fs| {
                                        fs.iter().find(|(n, _)| n == field).map(|(_, t)| t.clone())
                                    })
                                    .unwrap_or(MirType::I64);
                                let dest = ctx.alloc_temp(fty);
                                ctx.emit(Instruction::ActorStateLoad {
                                    dest,
                                    state_ptr: self_local,
                                    field_offset: *field_idx,
                                });
                                return RValue::Use(Operand::Local(dest));
                            }
                        }
                    }
                }
            }

            let obj_ty = infer_expr_type(ctx, object);
            let obj = lower_expr_to_operand(ctx, object);

            // Partial-move tracking: if a non-copy field is moved out of a
            // non-@copy struct local, mark the source local so scope cleanup
            // does NOT emit a full drop for it later (that would double-free
            // the heap memory the moved field already owns).
            if let MirType::Struct(struct_name) = &obj_ty {
                if !ctx.copy_structs.contains(struct_name.as_str()) {
                    if let Operand::Local(source_id) = &obj {
                        if let Some(fields) = ctx.struct_defs.get(struct_name.as_str()) {
                            if let Some((_, field_ty)) =
                                fields.iter().find(|(n, _)| n == field.as_str())
                            {
                                let field_ty = field_ty.clone();
                                if !is_copy_type(ctx, &field_ty) {
                                    ctx.partial_moved_locals.insert(source_id.0);
                                }
                            }
                        }
                    }
                }
            }

            // Auto-deref: if the object is a reference or shared pointer, dereference first.
            let obj = if matches!(obj_ty, MirType::Ref { .. }) {
                let deref_temp = ctx.alloc_temp(match &obj_ty {
                    MirType::Ref { inner, .. } => *inner.clone(),
                    _ => MirType::I64,
                });
                ctx.emit(Instruction::Assign {
                    dest: deref_temp,
                    value: RValue::Deref { operand: obj },
                });
                Operand::Local(deref_temp)
            } else if let MirType::Shared(inner) = &obj_ty {
                // Shared<T> auto-deref: emit a Deref so the LLVM backend loads the
                // struct inline from the arc block, and the Cranelift backend loads
                // the struct pointer stored at offset 0 of the arc block.
                let inner_ty = *inner.clone();
                let deref_temp = ctx.alloc_temp(inner_ty);
                ctx.emit(Instruction::Assign {
                    dest: deref_temp,
                    value: RValue::Deref { operand: obj },
                });
                Operand::Local(deref_temp)
            } else {
                obj
            };

            RValue::Field {
                object: obj,
                field: field.clone(),
            }
        }

        ast::Expr::IndexAccess { object, index, .. } => {
            let obj_ty = infer_expr_type(ctx, object);
            let obj = lower_expr_to_operand(ctx, object);
            let idx = lower_expr_to_operand(ctx, index);

            // Maps use runtime lookup instead of pointer arithmetic.
            if let MirType::Map { ref value, .. } = obj_ty {
                let idx_ty = infer_expr_type(ctx, index);
                let get_fn = if idx_ty == MirType::Str {
                    "kryos_map_get_str"
                } else {
                    "kryos_map_get"
                };
                // Borrow-to-own: kryos_map_get* returns the map's OWN entry
                // handle, not a copy. A container-typed result must be
                // retained so the receiving slot owns its reference —
                // otherwise the slot's later reassignment-release/drop frees
                // the map's entry and the next lookup hands out a dangling
                // pointer (heap corruption once frees became real, fe79f1b).
                let retain_fn = match **value {
                    MirType::Str => Some("kryos_string_retain_opt"),
                    MirType::Array(_, _) => Some("kryos_array_retain_opt"),
                    MirType::Map { .. } => Some("kryos_map_retain_opt"),
                    _ => None,
                };
                if let Some(rf) = retain_fn {
                    let val_ty = (**value).clone();
                    let tmp = ctx.alloc_temp(val_ty);
                    ctx.emit(Instruction::Assign {
                        dest: tmp,
                        value: RValue::Call {
                            func: get_fn.to_string(),
                            args: vec![obj, idx],
                        },
                    });
                    let discard = ctx.alloc_temp(MirType::I64);
                    ctx.emit(Instruction::Assign {
                        dest: discard,
                        value: RValue::Call {
                            func: rf.to_string(),
                            args: vec![Operand::Local(tmp)],
                        },
                    });
                    return RValue::Use(Operand::Local(tmp));
                }
                return RValue::Call {
                    func: get_fn.to_string(),
                    args: vec![obj, idx],
                };
            }

            // String indexing uses kryos_string_char_at (not kryos_array_get).
            if matches!(obj_ty, MirType::Str) {
                return RValue::Call {
                    func: "kryos_string_char_at".to_string(),
                    args: vec![obj, idx],
                };
            }

            RValue::Index {
                object: obj,
                index: idx,
            }
        }

        ast::Expr::Borrow { inner, mutable, .. } => {
            // &x → take address of local
            if let ast::Expr::Identifier { name, .. } = inner.as_ref() {
                let local =
                    find_local_by_name(ctx, name).expect("internal: borrow target local not found");
                RValue::AddrOf {
                    local,
                    mutable: *mutable,
                }
            } else {
                // For non-identifier expressions, lower to a temp first,
                // then take its address.
                let inner_ty = infer_expr_type(ctx, inner);
                let rvalue = lower_expr_to_rvalue(ctx, inner);
                let temp = ctx.alloc_local(None, inner_ty, true);
                ctx.emit(Instruction::Assign {
                    dest: temp,
                    value: rvalue,
                });
                RValue::AddrOf {
                    local: temp,
                    mutable: *mutable,
                }
            }
        }

        ast::Expr::Deref { inner, .. } => {
            // *x → load from reference
            let inner_op = lower_expr_to_operand(ctx, inner);
            RValue::Deref { operand: inner_op }
        }

        ast::Expr::SharedExpr { inner, .. } => {
            let inner_op = lower_expr_to_operand(ctx, inner);
            RValue::ArcAlloc { inner: inner_op }
        }

        ast::Expr::Cast { expr, ty, .. } => {
            let inner = lower_expr_to_operand(ctx, expr);
            let mir_ty = ctx.resolve_type(ty);
            RValue::Cast {
                operand: inner,
                ty: mir_ty,
            }
        }

        ast::Expr::MatchExpr { subject, arms, .. } => {
            let result = lower_match(ctx, subject, arms);
            RValue::Use(result)
        }

        ast::Expr::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            // Lower if-expression: both branches assign to a result local.
            //
            // The result local must be sized for the type the branches actually
            // produce, not unconditionally I64 — otherwise a function whose tail
            // expression is `if c { 0i32 } else { 1i32 }` allocates a 32-bit
            // return slot in the caller while we write 64 bits into it, which
            // silently corrupts the caller's frame and crashes after return.
            //
            // Inference mirrors the rule used by `infer_expr_type` for IfExpr:
            // look at the tail expression of the then-branch, fall back to the
            // else-branch's tail, then to I64 as a last resort.
            let result_ty = infer_branch_value_type(ctx, then_branch)
                .or_else(|| else_branch.as_ref().and_then(|b| infer_branch_value_type(ctx, b)))
                .unwrap_or(MirType::I64);
            let result_local = ctx.alloc_temp(result_ty);
            let cond_op = lower_expr_to_operand(ctx, condition);
            let then_bb = ctx.alloc_block();
            let else_bb = ctx.alloc_block();
            let merge_bb = ctx.alloc_block();

            ctx.finish_block(
                Terminator::Branch {
                    cond: cond_op,
                    then_block: then_bb,
                    else_block: else_bb,
                },
                then_bb,
            );

            // Then.
            lower_block_as_value(ctx, &then_branch.stmts, result_local);
            ctx.finish_block(Terminator::Goto(merge_bb), else_bb);

            // Else.
            if let Some(else_blk) = else_branch {
                lower_block_as_value(ctx, &else_blk.stmts, result_local);
            }
            ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);

            RValue::Use(Operand::Local(result_local))
        }

        // `unsafe { ... }` is semantically transparent: lower it exactly like a
        // plain block. The safety marker is enforced by the type-checker (E0500).
        ast::Expr::Block { block, .. } | ast::Expr::UnsafeBlock { body: block, .. } => {
            // Lower all statements, return last expression.
            //
            // Scope-exit drop: a block used as a VALUE (a match-arm body, an
            // if-as-value branch, ...) previously had NO drop cleanup of its
            // own for named locals it declares (unlike `lower_block_stmts`,
            // used when a block appears as a plain statement). A `let`
            // declared here (e.g. `let r = Ok(..)` inside one arm of an
            // enum match) stayed un-dropped in `ctx.locals`; the next
            // ENCLOSING `lower_block_stmts` scope that happened to cover it
            // (e.g. a `for`-loop body wrapping the whole match) swept it up
            // instead -- unconditionally, in that outer scope's own exit
            // block, on every iteration regardless of which arm actually
            // ran. On an iteration that took a DIFFERENT arm, that dropped
            // stale data left over from a previous iteration (or zero-init
            // on the first) instead of this arm's own value -- a null-deref
            // or a double-free of an already-freed prior allocation. This
            // was the verified root cause of the F1 struct/loop/nested-match
            // crash (gotcha #23). Track this block's own locals and drop
            // them here, at the end of ITS OWN control-flow path, exactly
            // like `lower_block_stmts` does for a statement-position block.
            let scope_start = ctx.locals.len();
            for (i, stmt) in block.stmts.iter().enumerate() {
                if i == block.stmts.len() - 1 {
                    if let ast::Stmt::Expr { expr, .. } = stmt {
                        // A bare-identifier tail MOVES the local into the
                        // result (Use is a bit-copy with no retain) -- mark
                        // it dropped so the scope cleanup below doesn't free
                        // the value the result now owns, then drop the OTHER
                        // locals and return Use(local) directly (mirrors
                        // `lower_block_as_value`'s identical guard).
                        if let ast::Expr::Identifier { name, .. } = expr {
                            let rv = lower_expr_to_rvalue(ctx, expr);
                            if let Some(l) = ctx
                                .locals
                                .iter()
                                .rev()
                                .find(|l| l.name.as_deref() == Some(name.as_str()))
                            {
                                ctx.dropped_locals.insert(l.id.0);
                            }
                            emit_named_scope_drops(ctx, scope_start);
                            return rv;
                        }
                        // Any OTHER tail (a call, binop, ...) is returned as an
                        // UN-EMITTED rvalue for the CALLER to store -- but it
                        // may READ this block's named locals, so it must be
                        // MATERIALIZED into a temp HERE, before the scope
                        // drops, or the drops free those locals first: a tail
                        // like `println(tag)` (a match-arm body `{ let tag =
                        // ..  println(tag) }`) freed `tag` before the call ran
                        // and printed blank -- a regression from the F1
                        // arm-local-drop fix. (match/if tails already emit
                        // their reads during lowering; the extra temp is
                        // harmless there.) Mirrors `lower_block_as_value`,
                        // which materializes its tail into `result_local`.
                        let rv = lower_expr_to_rvalue(ctx, expr);
                        let ty = infer_expr_type(ctx, expr);
                        let tmp = ctx.alloc_temp(ty);
                        ctx.emit(Instruction::Assign {
                            dest: tmp,
                            value: rv,
                        });
                        emit_named_scope_drops(ctx, scope_start);
                        return RValue::Use(Operand::Local(tmp));
                    }
                    // A trailing `if ... else ...` is the block's tail VALUE
                    // (parsed as Stmt::If). Without this it lowered as a
                    // side-effecting statement and the block yielded
                    // ConstNone -- `let x = { if c { a } else { b } }` and a
                    // match arm `x => { if ... }` produced empty.
                    if let Some(if_expr) = stmt.as_value_if_expr() {
                        let rv = lower_expr_to_rvalue(ctx, &if_expr);
                        emit_named_scope_drops(ctx, scope_start);
                        return rv;
                    }
                    // A trailing `try { .. } catch e { .. }` is the block's
                    // tail VALUE too. There is no `Expr::TryCatch` node --
                    // try/catch is statement-only in the parser by design --
                    // so without this branch a value-position block whose
                    // last statement is a try/catch fell through to the
                    // side-effecting `lower_stmt` call below and the block
                    // yielded ConstNone: `let x = { try { 1 } catch e { 2 } }`
                    // silently produced 0 instead of the executed arm's
                    // value. Mirrors the if-as-value branch just above and
                    // the function-body-tail try/catch path (~line 2310),
                    // both of which route through `lower_try_catch` with an
                    // explicit result local rather than discarding the arms'
                    // tail values.
                    if let ast::Stmt::TryCatch {
                        try_block,
                        catch_name,
                        catch_block,
                        ..
                    } = stmt
                    {
                        let result_ty = infer_branch_value_type(ctx, try_block)
                            .or_else(|| infer_branch_value_type(ctx, catch_block))
                            .unwrap_or(MirType::I64);
                        let result_local = ctx.alloc_temp(result_ty);
                        lower_try_catch(ctx, try_block, catch_name, catch_block, Some(result_local));
                        emit_named_scope_drops(ctx, scope_start);
                        return RValue::Use(Operand::Local(result_local));
                    }
                }
                lower_stmt(ctx, stmt);
            }
            emit_named_scope_drops(ctx, scope_start);
            RValue::ConstNone
        }

        ast::Expr::Lambda {
            params,
            ret_ty,
            body,
            span: lambda_span,
        } => {
            // Create an anonymous function name.
            let lambda_name = format!("__lambda_{}", ctx.lambda_counter);
            ctx.lambda_counter += 1;

            // Type-checker-resolved types for this lambda's un-annotated params
            // (so a `str`/struct/array closure param isn't defaulted to i64).
            let resolved_params = ctx.lambda_param_types.get(lambda_span).cloned();

            // Analyze free variables in the lambda body (captures from enclosing scope).
            let captures = find_free_variables(ctx, body, params);

            // Which of those captures does this lambda mutate? A mutating
            // closure captures BY MOVE (gotcha #11) and must own a single
            // persistent copy across calls, so it can never use the
            // direct-call optimization in `closure_locals` -- see
            // `find_mutated_captures` for why. Recorded now (before the
            // capture-derived params below get built) so the check runs
            // against the ORIGINAL capture list.
            let mutated_captures = find_mutated_captures(body, &captures);

            // Consume the "box read-only scalar captures" request set by a
            // struct-literal field whose value is this exact Lambda (see the
            // StructLiteral RValue arm). Reset immediately so a NESTED lambda
            // inside THIS one's body does not inherit the request.
            let box_scalar_captures = ctx.pending_box_scalar_captures;
            ctx.pending_box_scalar_captures = false;

            // Stage closure_locals re-registrations for the inner frame.
            // For each captured variable that is itself a tracked closure
            // with captures, record (closure_name, real_func, capture_names).
            // `find_free_variables` above already added the transitive
            // captures (e.g. `n` from `add_n`'s capture list) as free vars
            // of this lambda, so those names will exist as params in the
            // inner frame. After `lower_function` allocates inner-frame
            // params, it consumes `pending_closure_regs` and rebuilds
            // `closure_locals` entries pointing at the inner-frame locals.
            for capname in &captures {
                if let Some((real_func, capture_ops)) =
                    ctx.closure_locals.get(capname).cloned()
                {
                    let mut inner_capture_names: Vec<String> = Vec::new();
                    let mut ok = true;
                    for op in capture_ops {
                        if let Operand::Local(lid) = op {
                            let lname = ctx
                                .locals
                                .iter()
                                .find(|l| l.id == lid)
                                .and_then(|l| l.name.clone());
                            match lname {
                                Some(n) => inner_capture_names.push(n),
                                None => {
                                    ok = false;
                                    break;
                                }
                            }
                        } else {
                            ok = false;
                            break;
                        }
                    }
                    if ok {
                        ctx.pending_closure_regs.push((
                            capname.clone(),
                            real_func,
                            inner_capture_names,
                        ));
                    }
                }
            }

            // Build params BEFORE saving state — save_function_state() takes ctx.locals,
            // so type lookups must happen while the enclosing scope is still live.
            let mut all_params: Vec<ast::Param> = captures
                .iter()
                .map(|name| {
                    let ty = ctx
                        .locals
                        .iter()
                        .rev()
                        .find(|l| l.name.as_deref() == Some(name.as_str()))
                        .and_then(|l| mir_type_to_type_expr(&l.ty));
                    ast::Param {
                        name: name.clone(),
                        ty,
                        default: None,
                        span: kryos_errors::Span::DUMMY,
                    }
                })
                .collect();
            // Append the lambda's own params, filling in the type checker's
            // resolved type for any param the source left un-annotated.
            for (i, p) in params.iter().enumerate() {
                if p.ty.is_none() {
                    if let Some(Some(te)) = resolved_params.as_ref().and_then(|rp| rp.get(i)) {
                        all_params.push(ast::Param {
                            name: p.name.clone(),
                            ty: Some(te.clone()),
                            default: p.default.clone(),
                            span: p.span,
                        });
                        continue;
                    }
                }
                all_params.push(p.clone());
            }

            // Detect whether the body is a void-returning expression (e.g.
            // `println(...)`, a method call to a void method, or a Block
            // statement). When no explicit ret_ty is given and the body is
            // void, emit the body as an expression statement instead of
            // wrapping it in `return body` and defaulting the return type
            // to i64 — which would silently discard the call and produce a
            // closure that does nothing observable.
            let body_is_void_call = if ret_ty.is_none() {
                match body.as_ref() {
                    ast::Expr::FnCall { callee, .. } => {
                        if let ast::Expr::Identifier { name: cname, .. } = callee.as_ref() {
                            matches!(
                                ctx.func_ret_types.get(cname),
                                Some(MirType::Void)
                            ) || matches!(
                                cname.as_str(),
                                "println" | "print" | "eprintln" | "eprint" | "sleep_ms" | "sleep"
                            )
                        } else {
                            false
                        }
                    }
                    ast::Expr::Block { block, .. } => {
                        // A block is void only if it has no trailing
                        // expression AND contains no `return` statements.
                        // A block like `{ if cond { return 1 } return 0 }`
                        // returns i64 even though the last stmt is a Return.
                        // A trailing try/catch or value-`if` commits to a
                        // STATEMENT form but is a VALUE tail (the Expr::Block
                        // rvalue path materializes both) -- treating them as
                        // void made `|x| { try { x+1 } catch e { -1 } }` and
                        // `|x| { if x > 0 { 100 } else { -100 } }` closures
                        // silently return 0.
                        let trailing_is_expr = match block.stmts.last() {
                            Some(ast::Stmt::Expr { .. }) | Some(ast::Stmt::TryCatch { .. }) => {
                                true
                            }
                            Some(s) => s.as_value_if_expr().is_some(),
                            None => false,
                        };
                        let has_return = block
                            .stmts
                            .iter()
                            .any(|s| matches!(s, ast::Stmt::Return { .. }));
                        !trailing_is_expr && !has_return
                    }
                    _ => false,
                }
            } else {
                false
            };

            // Save state, lower the lambda as a standalone function.
            let saved = ctx.save_function_state();

            // If a `Stmt::Let` just above flagged this lambda as the value of
            // `let name = |...| { ... }` (see `pending_self_recursive_name`),
            // register `name` in the FRESH `closure_locals` map that
            // `save_function_state` just swapped in for this lambda's own
            // body frame -- so a self-recursive call to `name` inside the
            // body (which cannot see the enclosing `let`'s not-yet-allocated
            // local) resolves via the direct-call path in the `FnCall` arm
            // below to THIS lambda's own hoisted function, instead of
            // falling through to an undefined top-level symbol literally
            // named `name`. No captures: this only covers non-capturing
            // self-recursion (a captured variable would need to be
            // re-threaded through the recursive call as well, which is not
            // attempted here).
            if let Some(self_name) = ctx.pending_self_recursive_name.take() {
                ctx.closure_locals
                    .insert(self_name, (lambda_name.clone(), Vec::new()));
            }

            // Create a block from the body expression.
            let body_block = ast::Block {
                stmts: vec![if body_is_void_call {
                    ast::Stmt::Expr {
                        expr: body.as_ref().clone(),
                        span: kryos_errors::Span::DUMMY,
                    }
                } else {
                    ast::Stmt::Return {
                        value: Some(body.as_ref().clone()),
                        span: kryos_errors::Span::DUMMY,
                    }
                }],
                span: kryos_errors::Span::DUMMY,
            };

            // If no explicit return type, default to i64 — except when the
            // body is a void-returning call, in which case the lambda has
            // no return value and we pass None so `lower_function` treats
            // the function as returning void.
            let inferred_ret: Option<ast::TypeExpr>;
            let effective_ret = match ret_ty {
                Some(_) => ret_ty,
                None if body_is_void_call => &None,
                None => {
                    inferred_ret = Some(ast::TypeExpr::Simple {
                        name: "i64".to_string(),
                        span: kryos_errors::Span::DUMMY,
                    });
                    &inferred_ret
                }
            };

            let mut mir_func =
                lower_function(ctx, &lambda_name, &all_params, effective_ret, &body_block);
            ctx.restore_function_state(saved);

            // Box non-mutated SCALAR captures behind an ARC-managed heap
            // cell (reusing the SAME `RValue::ArcAlloc`/`RValue::Deref`
            // machinery that already backs `&x` borrows) when this lambda
            // literal is a struct-literal field's direct value. Rationale:
            // a struct-field closure's env is populated ONCE at construction
            // and never re-read, so (unlike a `let`-bound closure invoked by
            // name, which gets a direct-call substitution that re-reads the
            // outer variable's CURRENT value at every call site --
            // `closure_locals`) it silently sees a STALE snapshot of any
            // capture mutated after construction, even though it was never
            // mutated INSIDE the closure -- gotcha #11 promises by-reference
            // semantics unconditionally for those. A heap cell (not a raw
            // stack address) is required because a struct literal holding
            // this closure can itself be returned from the defining function
            // (`make_handler` in t06_closure_struct_field.kry) -- the
            // variable's OWN stack slot would then be popped/reused while
            // the closure still holds a pointer to it.
            //
            // Deliberately narrow: only struct-literal-field lambdas (a
            // `let`-bound closure keeps the existing, already-correct
            // `closure_locals` path -- boxing it too would require that fast
            // path to ALSO pass a pointer, a wider change this fix avoids);
            // only NON-mutated captures (a mutated capture keeps the
            // existing move + `mutated_capture_slot` write-back machinery
            // from commit 150ea0e untouched -- seperate bug, see
            // `t05_closure_mutable_hof`); only scalar types (str/array/map/
            // struct captures are not boxed here -- their existing capture
            // behavior is left exactly as before).
            let mut boxed_capture_names: HashSet<String> = HashSet::new();
            if box_scalar_captures {
                for (idx, cap_name) in captures.iter().enumerate() {
                    if mutated_captures.contains(cap_name) {
                        continue;
                    }
                    let Some(param) = mir_func.params.get(idx) else {
                        continue;
                    };
                    let orig_id = param.local;
                    let orig_ty = param.ty.clone();
                    if !is_boxable_scalar(&orig_ty) {
                        continue;
                    }
                    let ptr_id = LocalId(
                        mir_func
                            .locals
                            .iter()
                            .map(|l| l.id.0)
                            .max()
                            .map(|m| m + 1)
                            .unwrap_or(0),
                    );
                    mir_func.locals.push(MirLocal {
                        id: ptr_id,
                        name: None,
                        ty: MirType::Shared(Box::new(orig_ty.clone())),
                        mutable: false,
                    });
                    mir_func.params[idx] = MirParam {
                        local: ptr_id,
                        ty: MirType::Shared(Box::new(orig_ty)),
                    };
                    // Prologue: dereference the box once at function entry
                    // into the ORIGINAL param local -- the body already
                    // reads that id normally everywhere, so no substitution
                    // is needed anywhere else in the function.
                    mir_func.blocks[0].instructions.insert(
                        0,
                        Instruction::Assign {
                            dest: orig_id,
                            value: RValue::Deref {
                                operand: Operand::Local(ptr_id),
                            },
                        },
                    );
                    boxed_capture_names.insert(cap_name.clone());
                }
            }

            // Record mutation info for the codegen backends. ANY mutated
            // capture disables the direct-call optimization for this
            // lambda (see `mutating_closures` doc comment) since it is
            // never safe once the closure owns mutable state by move. When
            // there is EXACTLY ONE mutated capture, additionally mark its
            // env-slot index so the env-thunk can write the call's return
            // value back into persistent storage (the standard
            // mutate-then-return-it counter/accumulator idiom). Closures
            // mutating more than one capture keep working exactly as
            // before this fix (no worse), they just don't get the
            // persistence fix -- a documented residual limitation rather
            // than risking a wrong value written to the wrong slot.
            if !mutated_captures.is_empty() {
                ctx.mutating_closures.insert(lambda_name.clone());
                if mutated_captures.len() == 1
                    && tail_value_is_identifier(body, &mutated_captures[0])
                {
                    if let Some(idx) = captures.iter().position(|c| c == &mutated_captures[0]) {
                        mir_func.attributes.mutated_capture_slot = Some(idx as u32);
                    }
                }
                // Aggregate (struct) mutated-capture case: no tail-shape
                // restriction needed here (unlike the scalar slot above) --
                // see `mutated_capture_ptr_slots`' doc comment. Passing the
                // capture BY POINTER makes any field mutation on ANY path
                // through the body land directly in the persistent env block,
                // so there is no "does the tail equal the mutated value"
                // write-back-correctness concern. EACH mutated struct capture
                // gets its own pointer slot independently, so a closure that
                // mutates two or more struct captures persists all of them.
                for cap in &mutated_captures {
                    if let Some(idx) = captures.iter().position(|c| c == cap) {
                        let cap_ty = mir_func.params.get(idx).map(|p| p.ty.clone());
                        if matches!(cap_ty, Some(MirType::Struct(_))) {
                            mir_func.attributes.mutated_capture_ptr_slots.push(idx as u32);
                        }
                    }
                }
            }
            ctx.monomorphized_functions.push(mir_func);

            // Register the lambda's return type.
            let mir_ret = match ret_ty {
                Some(ty) => ctx.resolve_type(ty),
                None if body_is_void_call => MirType::Void,
                None => MirType::I64,
            };
            ctx.func_ret_types.insert(lambda_name.clone(), mir_ret);

            // Emit the closure RValue with captured variable operands.
            let mut capture_ops: Vec<Operand> = Vec::with_capacity(captures.len());
            for name in &captures {
                let local = find_local_by_name(ctx, name)
                    .expect("internal: lambda capture local not found");
                if boxed_capture_names.contains(name) {
                    // Ref-captured scalar: allocate the box HERE (at
                    // construction time, in the ENCLOSING function's own
                    // instruction stream -- `ctx` was already restored above)
                    // seeded with the capture's CURRENT value, and remember it
                    // so a later plain assignment to `name` in this same
                    // function (see the `Stmt::Assign` arm) also writes
                    // through it.
                    let local_ty = ctx
                        .locals
                        .iter()
                        .find(|l| l.id == local)
                        .map(|l| l.ty.clone())
                        .unwrap_or(MirType::I64);
                    let box_id = ctx.alloc_temp(MirType::Shared(Box::new(local_ty)));
                    ctx.emit(Instruction::Assign {
                        dest: box_id,
                        value: RValue::ArcAlloc {
                            inner: Operand::Local(local),
                        },
                    });
                    ctx.capture_boxes
                        .entry(name.clone())
                        .or_default()
                        .push(box_id);
                    capture_ops.push(Operand::Local(box_id));
                } else {
                    capture_ops.push(Operand::Local(local));
                }
            }

            RValue::Closure {
                func_name: lambda_name,
                captures: capture_ops,
            }
        }

        ast::Expr::PipeExpr { left, right, span } => {
            // Desugar to a REAL FnCall AST and recurse through the ordinary
            // call lowering. The old direct RValue::Call bypassed the entire
            // FnCall machinery: builtin overload dispatch (`x |> abs` on f64
            // picked the i64 builtin and printed the float's raw bit
            // pattern), closure invocation (a closure-valued RHS emitted a
            // direct call to an undefined symbol -> LNK2001), dyn coercion,
            // and call-arg consumption bookkeeping.
            let call = desugar_pipe(left, right, *span);
            return lower_expr_to_rvalue(ctx, &call);
        }

        ast::Expr::InterpolatedString { parts, .. } => {
            // Lower each part to an operand: literal strings become ConstString,
            // expressions are lowered normally (caller must convert to string at runtime).
            let ops: Vec<Operand> = parts
                .iter()
                .map(|part| match part {
                    ast::StringPart::Literal(s) => {
                        let tmp = ctx.alloc_temp(MirType::Str);
                        ctx.emit(Instruction::Assign {
                            dest: tmp,
                            value: RValue::ConstString(s.clone()),
                        });
                        Operand::Local(tmp)
                    }
                    ast::StringPart::Expr(e) => {
                        // A non-scalar AGGREGATE (struct/enum/array/tuple/map/fn)
                        // has no string form; StringConcat would treat it as an
                        // i64 (pointer) -> garbage on JIT, blank/SEGFAULT on AOT.
                        // The type checker rejects CONCRETE aggregate
                        // interpolation, but a GENERIC `T` that monomorphizes to
                        // an aggregate slips past (e.g. std::tracked's
                        // `"{t.value}"`). Lower `e` for its side effects, then
                        // substitute a safe placeholder so it never derefs an
                        // aggregate as a string.
                        let ety = infer_expr_type(ctx, e);
                        // A char interpolates as the CHARACTER (via char_from),
                        // not its numeric code point ('x' -> "x", not "120").
                        if matches!(ety, MirType::Char) {
                            let code = lower_expr_to_operand(ctx, e);
                            let tmp = ctx.alloc_temp(MirType::Str);
                            ctx.emit(Instruction::Assign {
                                dest: tmp,
                                value: RValue::Call {
                                    func: "char_from".into(),
                                    args: vec![code],
                                },
                            });
                            return Operand::Local(tmp);
                        }
                        let placeholder = match &ety {
                            MirType::Struct(n) | MirType::Enum(n) => Some(format!("<{n}>")),
                            MirType::Array(..) => Some("<array>".to_string()),
                            MirType::Tuple(_) => Some("<tuple>".to_string()),
                            MirType::Map { .. } => Some("<map>".to_string()),
                            MirType::Function { .. } => Some("<fn>".to_string()),
                            _ => None,
                        };
                        match placeholder {
                            Some(ph) => {
                                let _ = lower_expr_to_operand(ctx, e);
                                let tmp = ctx.alloc_temp(MirType::Str);
                                ctx.emit(Instruction::Assign {
                                    dest: tmp,
                                    value: RValue::ConstString(ph),
                                });
                                Operand::Local(tmp)
                            }
                            None => lower_expr_to_operand(ctx, e),
                        }
                    }
                })
                .collect();
            RValue::StringConcat(ops)
        }

        ast::Expr::MapLiteral { entries, .. } => {
            let mir_entries: Vec<(Operand, Operand)> = entries
                .iter()
                .map(|(k, v)| (lower_expr_to_operand(ctx, k), lower_expr_to_operand(ctx, v)))
                .collect();
            RValue::Map(mir_entries)
        }

        ast::Expr::CharLiteral { value, .. } => RValue::ConstInt(*value as i64),

        ast::Expr::MoveExpr { inner, .. } => {
            // Move is an ownership marker — at MIR level, just lower the inner expr.
            lower_expr_to_rvalue(ctx, inner)
        }

        ast::Expr::WeakExpr { inner, .. } => {
            // Weak reference — lower inner, ARC alloc is tracked separately.
            let inner_op = lower_expr_to_operand(ctx, inner);
            RValue::ArcAlloc { inner: inner_op }
        }

        ast::Expr::RangeExpr {
            start,
            end,
            inclusive,
            ..
        } => {
            let s = start.as_ref().map(|e| lower_expr_to_operand(ctx, e));
            let e = end.as_ref().map(|e| lower_expr_to_operand(ctx, e));
            RValue::Range {
                start: s,
                end: e,
                inclusive: *inclusive,
            }
        }

        ast::Expr::ComptimeBlock { body, .. } => {
            // Lower the body block, wrapping the result in Comptime.
            for (i, stmt) in body.stmts.iter().enumerate() {
                if i == body.stmts.len() - 1 {
                    if let ast::Stmt::Expr { expr, .. } = stmt {
                        let inner = lower_expr_to_rvalue(ctx, expr);
                        return RValue::Comptime(Box::new(inner));
                    }
                }
                lower_stmt(ctx, stmt);
            }
            RValue::Comptime(Box::new(RValue::ConstNone))
        }

        ast::Expr::QuantumBlock { body, .. } => {
            // Quantum blocks: lower body normally (placeholder — future quantum backend).
            for (i, stmt) in body.stmts.iter().enumerate() {
                if i == body.stmts.len() - 1 {
                    if let ast::Stmt::Expr { expr, .. } = stmt {
                        return lower_expr_to_rvalue(ctx, expr);
                    }
                }
                lower_stmt(ctx, stmt);
            }
            RValue::ConstNone
        }

        // Await — real cooperative suspension point. Evaluate the awaited
        // expression, then hand control to the scheduler via `coop_yield` so
        // other tasks interleave. On a non-coop thread `kryos_coop_yield` is a
        // no-op, so a direct (non-spawned) async call degrades to an ordinary
        // synchronous call (back-compat). This replaces the previous
        // run-straight-through behavior where `await` was a plain direct call.
        ast::Expr::Await { value, .. } => {
            let v = lower_expr_to_operand(ctx, value);
            let yield_tmp = ctx.alloc_temp(MirType::Void);
            ctx.emit(Instruction::Assign {
                dest: yield_tmp,
                value: RValue::Call {
                    func: "coop_yield".into(),
                    args: vec![],
                },
            });
            RValue::Use(v)
        }
    }
}

// ---------------------------------------------------------------------------
// Operator mapping
// ---------------------------------------------------------------------------

fn lower_binop(op: ast::BinOp) -> MirBinOp {
    match op {
        ast::BinOp::Add => MirBinOp::Add,
        ast::BinOp::Sub => MirBinOp::Sub,
        ast::BinOp::Mul => MirBinOp::Mul,
        ast::BinOp::Div => MirBinOp::Div,
        ast::BinOp::Mod => MirBinOp::Mod,
        ast::BinOp::Pow => MirBinOp::Pow,
        ast::BinOp::Eq => MirBinOp::Eq,
        ast::BinOp::Neq => MirBinOp::Neq,
        ast::BinOp::Lt => MirBinOp::Lt,
        ast::BinOp::Gt => MirBinOp::Gt,
        ast::BinOp::LtEq => MirBinOp::LtEq,
        ast::BinOp::GtEq => MirBinOp::GtEq,
        ast::BinOp::And => MirBinOp::And,
        ast::BinOp::Or => MirBinOp::Or,
        ast::BinOp::BitAnd => MirBinOp::BitAnd,
        ast::BinOp::BitOr => MirBinOp::BitOr,
        ast::BinOp::BitXor => MirBinOp::BitXor,
        ast::BinOp::Shl => MirBinOp::Shl,
        ast::BinOp::Shr => MirBinOp::Shr,
        // Pipe is handled as PipeExpr before reaching this path; if we get
        // here it means the parser unexpectedly created a BinOp::Pipe.
        ast::BinOp::Pipe => MirBinOp::Add,
        // MatMul (@) is reserved for future matrix operations.
        ast::BinOp::MatMul => MirBinOp::Mul,
    }
}

fn lower_unop(op: ast::UnOp) -> MirUnOp {
    match op {
        ast::UnOp::Neg => MirUnOp::Neg,
        ast::UnOp::Not => MirUnOp::Not,
        ast::UnOp::BitNot => MirUnOp::BitNot,
    }
}

// ---------------------------------------------------------------------------
// Type lowering
// ---------------------------------------------------------------------------

/// Convert an AST `TypeExpr` to a MIR `MirType`.
pub fn lower_type_expr(ty: &ast::TypeExpr) -> MirType {
    match ty {
        ast::TypeExpr::Simple { name, .. } => match name.as_str() {
            "i8" => MirType::I8,
            "i16" => MirType::I16,
            "i32" => MirType::I32,
            "i64" => MirType::I64,
            "i128" => MirType::I128,
            "u8" => MirType::U8,
            "u16" => MirType::U16,
            "u32" => MirType::U32,
            "u64" => MirType::U64,
            "u128" => MirType::U128,
            // Pointer-sized integers are i64 in Kryos's MIR. Without these they
            // fell to Struct("usize")/Struct("isize") -> undefined %usize on AOT.
            "usize" | "isize" => MirType::I64,
            "f32" => MirType::F32,
            "f64" => MirType::F64,
            "bool" => MirType::Bool,
            "char" => MirType::Char,
            "str" | "string" | "String" => MirType::Str,
            "void" => MirType::Void,
            // The `!` (never) type denotes a function that never returns
            // normally (it diverges via exit/throw/loop). At the MIR level
            // we represent it as Void so the ABI matches `() -> ()`.
            // Both `never` and `Never` are accepted by the type checker, so
            // both must lower here (else `Never` fell through to Struct("Never")
            // -> undefined `%Never` LLVM type).
            "never" | "Never" => MirType::Void,
            // A bare `ptr` annotation is a raw opaque pointer (used by extern
            // FFI decls + std::sync). Without this it fell through to
            // Struct("ptr") -> LLVM `%ptr` (undefined, clashes with the opaque
            // pointer keyword).
            "ptr" => MirType::Ptr(Box::new(MirType::Void)),
            // `any` is the dynamic top type. It is carried as an i64 handle
            // (matching the typechecker's Type::Error -> I64 fallback and the
            // Cranelift backend, which treats all aggregates as i64 handles).
            // Without this it fell through to Struct("any"), which the LLVM
            // backend emitted as an undefined `%any` named type — first-class
            // load failure on AOT (e.g. passing `[any]` to a function).
            "any" | "Any" => MirType::I64,
            other => MirType::Struct(other.to_string()),
        },
        ast::TypeExpr::Array { element, size, .. } => {
            MirType::Array(Box::new(lower_type_expr(element)), *size)
        }
        ast::TypeExpr::Tuple { elements, .. } => {
            MirType::Tuple(elements.iter().map(lower_type_expr).collect())
        }
        ast::TypeExpr::Function { params, ret, .. } => MirType::Function {
            params: params.iter().map(lower_type_expr).collect(),
            ret: Box::new(lower_type_expr(ret)),
        },
        ast::TypeExpr::Shared { inner, .. } => MirType::Shared(Box::new(lower_type_expr(inner))),
        ast::TypeExpr::Pointer { inner, .. } => MirType::Ptr(Box::new(lower_type_expr(inner))),
        ast::TypeExpr::Generic { name, args, .. } => {
            if (name == "Map" || name == "map") && args.len() == 2 {
                MirType::Map {
                    key: Box::new(lower_type_expr(&args[0])),
                    value: Box::new(lower_type_expr(&args[1])),
                }
            } else {
                MirType::Struct(name.clone())
            }
        }
        ast::TypeExpr::Optional { inner, .. } => {
            // Lower Optional<T> as Struct("Option") — codegen decides representation.
            let _ = lower_type_expr(inner);
            MirType::Struct("Option".to_string())
        }
        ast::TypeExpr::Reference { inner, mutable, .. } => MirType::Ref {
            inner: Box::new(lower_type_expr(inner)),
            mutable: *mutable,
        },
        ast::TypeExpr::Weak { inner, .. } => {
            // Lower Weak as Ptr — codegen adds weak-ref bookkeeping.
            MirType::Ptr(Box::new(lower_type_expr(inner)))
        }
        ast::TypeExpr::DynTrait { trait_name, .. } => MirType::DynTrait(trait_name.clone()),
        ast::TypeExpr::Inferred { .. } => MirType::I64, // default unresolved
    }
}

/// Convert a `kryos_types::Type` to `MirType`.
pub fn lower_resolved_type(ty: &Type) -> MirType {
    match ty {
        Type::I8 => MirType::I8,
        Type::I16 => MirType::I16,
        Type::I32 => MirType::I32,
        Type::I64 => MirType::I64,
        Type::I128 => MirType::I128,
        Type::U8 => MirType::U8,
        Type::U16 => MirType::U16,
        Type::U32 => MirType::U32,
        Type::U64 => MirType::U64,
        Type::U128 => MirType::U128,
        Type::F32 => MirType::F32,
        Type::F64 => MirType::F64,
        Type::Bool => MirType::Bool,
        Type::Char => MirType::Char,
        Type::Str => MirType::Str,
        Type::Void | Type::Never => MirType::Void,
        Type::USize | Type::ISize => MirType::I64,
        Type::Array { element, size } => {
            MirType::Array(Box::new(lower_resolved_type(element)), *size)
        }
        Type::Tuple { elements } => {
            MirType::Tuple(elements.iter().map(lower_resolved_type).collect())
        }
        Type::Struct { name, .. } => MirType::Struct(name.clone()),
        Type::Enum { name, .. } => MirType::Enum(name.clone()),
        Type::Function { params, ret } => MirType::Function {
            params: params.iter().map(lower_resolved_type).collect(),
            ret: Box::new(lower_resolved_type(ret)),
        },
        Type::Shared { inner } => MirType::Shared(Box::new(lower_resolved_type(inner))),
        Type::Reference { inner, mutable } => MirType::Ref {
            inner: Box::new(lower_resolved_type(inner)),
            mutable: *mutable,
        },
        Type::Pointer { inner, .. } | Type::Weak { inner } => {
            MirType::Ptr(Box::new(lower_resolved_type(inner)))
        }
        Type::Option { inner } => {
            let _ = lower_resolved_type(inner);
            MirType::Struct("Option".to_string())
        }
        Type::Result { ok, err } => {
            let _ = lower_resolved_type(ok);
            let _ = lower_resolved_type(err);
            MirType::Struct("Result".to_string())
        }
        Type::Map { key, value } => MirType::Map {
            key: Box::new(lower_resolved_type(key)),
            value: Box::new(lower_resolved_type(value)),
        },
        Type::Set { .. } => MirType::Struct("Set".to_string()),
        Type::DynTrait { trait_name } => MirType::DynTrait(trait_name.clone()),
        Type::Var(_) | Type::Error => MirType::I64, // fallback
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a MirType back to an AST TypeExpr for use in synthesized parameters
/// (e.g. lambda captures). Returns `None` for simple i64 (the default).
fn mir_type_to_type_expr(ty: &MirType) -> Option<ast::TypeExpr> {
    let span = kryos_errors::Span::DUMMY;
    match ty {
        MirType::I64 => None, // default, no annotation needed
        MirType::F64 => Some(ast::TypeExpr::Simple {
            name: "f64".to_string(),
            span,
        }),
        MirType::Bool => Some(ast::TypeExpr::Simple {
            name: "bool".to_string(),
            span,
        }),
        MirType::Str => Some(ast::TypeExpr::Simple {
            name: "str".to_string(),
            span,
        }),
        MirType::Void => Some(ast::TypeExpr::Simple {
            name: "void".to_string(),
            span,
        }),
        MirType::Struct(name) | MirType::Enum(name) => Some(ast::TypeExpr::Simple {
            name: name.clone(),
            span,
        }),
        MirType::Function { params, ret } => {
            let param_tys: Vec<ast::TypeExpr> = params
                .iter()
                .map(|p| {
                    mir_type_to_type_expr(p).unwrap_or_else(|| ast::TypeExpr::Simple {
                        name: "i64".to_string(),
                        span,
                    })
                })
                .collect();
            let ret_ty = mir_type_to_type_expr(ret).unwrap_or_else(|| ast::TypeExpr::Simple {
                name: "i64".to_string(),
                span,
            });
            Some(ast::TypeExpr::Function {
                params: param_tys,
                ret: Box::new(ret_ty),
                span,
            })
        }
        MirType::Array(elem, size) => {
            let elem_ty = mir_type_to_type_expr(elem).unwrap_or_else(|| ast::TypeExpr::Simple {
                name: "i64".to_string(),
                span,
            });
            Some(ast::TypeExpr::Array {
                element: Box::new(elem_ty),
                size: *size,
                span,
            })
        }
        MirType::DynTrait(name) => Some(ast::TypeExpr::DynTrait {
            trait_name: name.clone(),
            span,
        }),
        // Map/Tuple were missing here, so a closure that CAPTURES a map (or
        // tuple) reconstructed its param type as None (-> i64 default). Inside
        // the closure `m[k]` then mis-lowered to ARRAY indexing (string key
        // pointer used as an index -> out-of-bounds crash). Round-trip the real
        // type so index/field access dispatches correctly (resolve_type accepts
        // Generic{"map",[k,v]} and Tuple{..}).
        MirType::Map { key, value } => {
            let key_ty = mir_type_to_type_expr(key).unwrap_or_else(|| ast::TypeExpr::Simple {
                name: "i64".to_string(),
                span,
            });
            let value_ty = mir_type_to_type_expr(value).unwrap_or_else(|| ast::TypeExpr::Simple {
                name: "i64".to_string(),
                span,
            });
            Some(ast::TypeExpr::Generic {
                name: "map".to_string(),
                args: vec![key_ty, value_ty],
                span,
            })
        }
        MirType::Tuple(elems) => {
            let elem_tys: Vec<ast::TypeExpr> = elems
                .iter()
                .map(|e| {
                    mir_type_to_type_expr(e).unwrap_or_else(|| ast::TypeExpr::Simple {
                        name: "i64".to_string(),
                        span,
                    })
                })
                .collect();
            Some(ast::TypeExpr::Tuple {
                elements: elem_tys,
                span,
            })
        }
        _ => None, // fall back to default i64
    }
}

/// Emit a runtime load of a mutable module-level global named `name`.
///
/// Lowers to `let tmp: <mir_ty> = kryos_global_get("<name>")` — the name is
/// passed as a Kryos string handle (the runtime decodes it on the fly).
///
/// For `f64` globals the slot stores the raw i64 bit pattern; callers should
/// transmute via `bitcast` at the use site if needed. In practice everything
/// reads back at MIR type i64/handle and the rest of the pipeline already
/// handles the value as a uniform 64-bit slot, so this helper just returns a
/// local of the global's declared MirType and lets the codegen pick the
/// right ABI moves.
fn emit_global_load(ctx: &mut LoweringContext, name: &str, mir_ty: MirType) -> LocalId {
    let temp = ctx.alloc_temp(mir_ty);
    ctx.emit(Instruction::Assign {
        dest: temp,
        value: RValue::Call {
            func: "kryos_global_get".to_string(),
            args: vec![Operand::Constant(Constant::Str(name.to_string()))],
        },
    });
    temp
}

/// Emit a runtime store to a mutable module-level global named `name`.
///
/// Lowers to `kryos_global_set("<name>", <value>)`. The set function has a
/// void return at the ABI level, but MIR call instructions always assign to
/// a destination — we allocate a throwaway i64 temp and let the JIT discard
/// the (non-existent) return value. The codegen layer already tolerates
/// void-return externs called this way (see jit.rs `kryos_global_set_void`).
fn emit_global_store(ctx: &mut LoweringContext, name: &str, value: Operand) {
    let throwaway = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: throwaway,
        value: RValue::Call {
            func: "kryos_global_set".to_string(),
            args: vec![
                Operand::Constant(Constant::Str(name.to_string())),
                value,
            ],
        },
    });
}

/// Look up a local by name. Returns `Some(id)` if found, `None` otherwise.
fn find_local_by_name(ctx: &LoweringContext, name: &str) -> Option<LocalId> {
    ctx.locals
        .iter()
        .rev()
        .find(|l| l.name.as_deref() == Some(name) && !ctx.hidden_locals.contains(&l.id.0))
        .map(|l| l.id)
}

/// Infer the type name of an expression (for method call resolution).
/// Returns the struct/enum name if resolvable, None otherwise.
fn infer_type_name(ctx: &mut LoweringContext, expr: &ast::Expr) -> Option<String> {
    let ty = infer_expr_type(ctx, expr);
    infer_type_name_with(ctx, expr, ty)
}

/// `infer_type_name` but with the expression's MIR type ALREADY computed.
/// The caller often just ran `infer_expr_type(object)` itself (e.g. the
/// MethodCall inference/lowering arms), and letting `infer_type_name`
/// re-infer it double-recurses into the receiver -- for a method CHAIN
/// (`b.inc().inc()...`) that is O(2^depth), so a 30-deep chain took minutes.
/// Reusing the precomputed type makes chain inference linear.
fn infer_type_name_with(
    ctx: &mut LoweringContext,
    expr: &ast::Expr,
    ty: MirType,
) -> Option<String> {
    match ty {
        MirType::Struct(name) | MirType::Enum(name) => Some(name),
        _ => {
            // An actor-typed FIELD erases to a bare `I64` (actor VALUES are
            // opaque handles -- see `resolve_type`'s actor-erasure rule), so
            // a method-call receiver that is itself a field access holding
            // another actor (`self.target.bump(..)` where `target: Counter`
            // is a field of the enclosing actor, or `some_box.c.bump(..)`
            // where `c: Counter` is a field of an ordinary struct) is
            // invisible to the Struct/Enum match above and would otherwise
            // fall through to a plain unmangled call instead of the
            // actor-dispatch mailbox send. Recover the logical actor type
            // from `field_actor_types`, captured at field-registration time
            // before the erasure. `self` inside an actor handler ALSO erases
            // to I64 (same rule, applied to the handler's own `self` param),
            // so its owner type is recovered via `current_actor` -- the same
            // fallback `self.field` reads/writes already use.
            if let ast::Expr::FieldAccess { object, field, .. } = expr {
                let owner = if let ast::Expr::Identifier { name, .. } = object.as_ref() {
                    if name == "self" {
                        ctx.current_actor.clone()
                    } else {
                        infer_type_name(ctx, object)
                    }
                } else {
                    infer_type_name(ctx, object)
                };
                if let Some(owner) = owner {
                    if let Some(actor_ty) =
                        ctx.field_actor_types.get(&(owner, field.clone())).cloned()
                    {
                        return Some(actor_ty);
                    }
                }
            }
            // A bare identifier bound to an actor value via an explicit type
            // annotation (a function/closure/spawn-body PARAM, or an
            // annotated `let c: Counter = ...`) erases the same way through
            // `resolve_type`. Recover the logical actor type from
            // `local_actor_types`, captured at param/let registration time
            // before the erasure -- sibling of the field recovery above, for
            // the "actor captured by a `spawn { }` block" case (backlog:
            // spawn-captured-actor dispatch).
            if let ast::Expr::Identifier { name, .. } = expr {
                if let Some(local_id) = find_local_by_name(ctx, name) {
                    if let Some(actor_ty) = ctx.local_actor_types.get(&local_id.0).cloned() {
                        return Some(actor_ty);
                    }
                }
            }
            // A method-call receiver that is itself a CALL expression
            // (`get_counter(c).bump(..)`) whose callee's declared return type
            // names an actor erases the same way -- the return-type
            // annotation goes through `resolve_type` at signature-registration
            // time (see `fn_return_actor_types`), so the FnCall's static
            // MirType here is a bare `I64` handle, not `Struct(actor)`.
            // Recover the logical actor name from that registry -- third
            // facet of the family, sibling of the field/local recoveries
            // above (this one for a function's RETURN type instead of a
            // field's or a param/let's declared type).
            if let ast::Expr::FnCall { callee, .. } = expr {
                if let ast::Expr::Identifier { name: callee_name, .. } = callee.as_ref() {
                    if let Some(actor_ty) = ctx.fn_return_actor_types.get(callee_name).cloned() {
                        return Some(actor_ty);
                    }
                }
            }
            None
        }
    }
}

/// Check if `name` is an enum variant. Returns (enum_name, variant_index) if found.
fn find_enum_variant(ctx: &LoweringContext, name: &str) -> Option<(String, u32)> {
    // Qualified `Enum::Variant` (Rust-style path) for a NULLARY variant, e.g.
    // `Opt::None`. The parser emits this as an `Identifier` named "Opt::None"
    // (the with-payload form `Opt::Some(7)` is a StaticMethodCall instead).
    // Resolve strictly within the named enum. A non-enum `head::tail` (a module
    // path like `math::PI`) returns None here and falls through to global
    // handling.
    if let Some((enum_name, variant)) = name.split_once("::") {
        if let Some(variants) = ctx.enum_defs.get(enum_name) {
            if let Some((idx, _)) =
                variants.iter().enumerate().find(|(_, v)| v.name == variant)
            {
                return Some((enum_name.to_string(), idx as u32));
            }
        }
        return None;
    }
    // DETERMINISTIC bare-variant resolution. `ctx.enum_defs` is a HashMap, so
    // iterating it returns the first name-match in per-process-randomized order;
    // a user enum sharing a variant name with std Option/Result made the SAME
    // source lower (and compile) differently across runs. Collect all matches,
    // then prefer stdlib Option/Result on a collision, else the
    // lexicographically-first enum name. (Mirrors env.rs find_enum_by_variant.)
    let mut matches: Vec<(String, u32)> = Vec::new();
    for (enum_name, variants) in &ctx.enum_defs {
        if let Some((idx, _)) = variants.iter().enumerate().find(|(_, v)| v.name == name) {
            matches.push((enum_name.clone(), idx as u32));
        }
    }
    if matches.is_empty() {
        return None;
    }
    matches.sort_by(|a, b| {
        let rank = |n: &str| if n == "Option" || n == "Result" { 0 } else { 1 };
        rank(&a.0).cmp(&rank(&b.0)).then(a.0.cmp(&b.0))
    });
    Some(matches[0].clone())
}

/// A bare heap-local passed directly as an enum-variant field is moved into
/// the enum -- the field is a bit-copy with no retain. Mark the local
/// drop-suppressed so its scope-end drop does not free the payload the
/// constructed enum now owns.
///
/// Without this, constructing an enum from a named local holding a heap
/// value (str/array/map/struct/enum/shared) double-frees at scope exit: the
/// enum's own recursive drop frees the embedded field pointer, AND the
/// untouched source local is separately dropped since it was never marked
/// consumed. For a recursive enum (e.g. a tree node built from child enum
/// locals) this corrupts the heap (Windows: STATUS_HEAP_CORRUPTION fail-fast,
/// surfaced as a silent abnormal exit) even though program output up to that
/// point is correct.
///
/// This must run for EVERY syntax that constructs an enum variant with
/// arguments: bare `Variant(args)` (FnCall), dotted `Enum.Variant(args)`
/// (MethodCall), and Rust-style `Enum::Variant(args)` (StaticMethodCall).
/// Historically only the FnCall path had this; the other two leaked the
/// suppression, causing the double-free above for any recursive/heap-payload
/// enum built via those spellings.
fn suppress_enum_field_arg_drops(ctx: &mut LoweringContext, args: &[ast::Expr]) {
    for a in args {
        if let ast::Expr::Identifier { name: an, .. } = a {
            if let Some(l) = ctx
                .locals
                .iter()
                .rev()
                .find(|l| l.name.as_deref() == Some(an.as_str()))
            {
                if matches!(
                    l.ty,
                    MirType::Str
                        | MirType::Array(..)
                        | MirType::Map { .. }
                        | MirType::Struct(_)
                        | MirType::Enum(_)
                        | MirType::Shared(_)
                ) {
                    let id = l.id.0;
                    ctx.dropped_locals.insert(id);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Lambda / Closure helpers
// ---------------------------------------------------------------------------

/// Returns the subset of `capture_names` that the lambda `body` reassigns
/// via a bare `Stmt::Assign` (`name = ...`) anywhere inside its own body --
/// including inside `if`/`while`/`for`/`match`/`try` blocks, but NOT
/// descending into a nested `Lambda` (a nested closure's captures follow
/// its own independent capture-by-move/by-ref rule; see gotcha #11).
///
/// A closure that mutates a captured variable captures it BY MOVE: it owns
/// a single persistent copy of that variable across calls (the standard
/// counter/accumulator idiom). The direct-call optimization in
/// `closure_locals` is only correct for captures read (never written)
/// inside the closure -- it works by re-reading the CURRENT value of the
/// outer variable at each call site, which is exactly wrong for a mutated
/// capture (it silently resets the closure's state to the outer variable's
/// value every call instead of threading the mutation forward). Callers use
/// this to withhold that optimization whenever it would apply.
///
/// This is a deliberate over-approximation: it does not track lexical
/// shadowing, so a nested `let mut` that reuses a capture's name also
/// counts as "mutated". That only costs the direct-call optimization for a
/// rare name collision -- never a correctness issue.
/// True for the small set of scalar `MirType`s that both codegen backends'
/// `RValue::ArcAlloc` lower via the simple "allocate 8 bytes, store the
/// value at offset 0" path (see `kryos-codegen-{llvm,cranelift}::codegen.rs`)
/// rather than the aggregate/sizeof(T) path used for structs/arrays/etc.
/// Used to decide which struct-literal-field closure captures are safe to
/// box (see `box_scalar_captures` in the Lambda RValue arm) -- restricted to
/// this set to keep the fix's blast radius small; str/array/map/struct/enum
/// captures keep their existing (unboxed) capture behavior untouched.
fn is_boxable_scalar(ty: &MirType) -> bool {
    matches!(
        ty,
        MirType::I8
            | MirType::I16
            | MirType::I32
            | MirType::I64
            | MirType::U8
            | MirType::U16
            | MirType::U32
            | MirType::U64
            | MirType::F64
            | MirType::Bool
            | MirType::Char
    )
}

fn find_mutated_captures(body: &ast::Expr, capture_names: &[String]) -> Vec<String> {
    if capture_names.is_empty() {
        return Vec::new();
    }
    let names: std::collections::HashSet<&str> =
        capture_names.iter().map(|s| s.as_str()).collect();
    let mut mutated: std::collections::HashSet<String> = std::collections::HashSet::new();
    expr_mutates_names(body, &names, &mut mutated);
    // Preserve the original capture order for deterministic env-slot indexing.
    capture_names
        .iter()
        .filter(|n| mutated.contains(n.as_str()))
        .cloned()
        .collect()
}

/// True if the lambda body's effective return value is exactly a bare
/// reference to `name` -- the "mutate then return the same variable" idiom
/// (`|| { count = count + 1  count }`, `|| { total = total + x  total }`).
///
/// Used to gate `MirAttributes::mutated_capture_slot`: it is only safe for
/// the env-thunk to write a closure call's return value back into a
/// mutated capture's persistent env slot when the return value on every
/// path IS that capture's current value. Without this check, a closure
/// that mutates a capture but returns something unrelated (`|| { count =
/// count + 1  return 99 }`) would have 99 silently written into count's
/// slot instead of the incremented value -- a real miscompile, not just an
/// unoptimized case. Deliberately conservative: only the flat "last
/// statement is the bare identifier" and "single bare-identifier body"
/// shapes are recognized; anything branchier (if/match tails) returns
/// false and falls back to the pre-fix behavior (no write-back).
fn tail_value_is_identifier(body: &ast::Expr, name: &str) -> bool {
    match body {
        ast::Expr::Identifier { name: n, .. } => n == name,
        ast::Expr::Block { block, .. } => match block.stmts.last() {
            Some(ast::Stmt::Expr { expr, .. }) => tail_value_is_identifier(expr, name),
            Some(ast::Stmt::Return {
                value: Some(expr), ..
            }) => tail_value_is_identifier(expr, name),
            _ => false,
        },
        _ => false,
    }
}

fn expr_mutates_names(
    expr: &ast::Expr,
    names: &std::collections::HashSet<&str>,
    out: &mut std::collections::HashSet<String>,
) {
    match expr {
        ast::Expr::Block { block, .. } => block_mutates_names(&block.stmts, names, out),
        ast::Expr::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            expr_mutates_names(condition, names, out);
            block_mutates_names(&then_branch.stmts, names, out);
            if let Some(eb) = else_branch {
                block_mutates_names(&eb.stmts, names, out);
            }
        }
        ast::Expr::MatchExpr { subject, arms, .. } => {
            expr_mutates_names(subject, names, out);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    expr_mutates_names(guard, names, out);
                }
                expr_mutates_names(&arm.body, names, out);
            }
        }
        ast::Expr::ComptimeBlock { body, .. }
        | ast::Expr::QuantumBlock { body, .. }
        | ast::Expr::UnsafeBlock { body, .. } => block_mutates_names(&body.stmts, names, out),
        // A nested lambda has its own independent capture rule; whether IT
        // mutates something is that closure's own concern, not this one's.
        ast::Expr::Lambda { .. } => {}
        ast::Expr::BinaryOp { left, right, .. } | ast::Expr::PipeExpr { left, right, .. } => {
            expr_mutates_names(left, names, out);
            expr_mutates_names(right, names, out);
        }
        ast::Expr::UnaryOp { operand, .. } => expr_mutates_names(operand, names, out),
        ast::Expr::FnCall { callee, args, .. } => {
            expr_mutates_names(callee, names, out);
            for a in args {
                expr_mutates_names(a, names, out);
            }
        }
        ast::Expr::MethodCall { object, args, .. } => {
            expr_mutates_names(object, names, out);
            for a in args {
                expr_mutates_names(a, names, out);
            }
        }
        ast::Expr::StaticMethodCall { args, .. } => {
            for a in args {
                expr_mutates_names(a, names, out);
            }
        }
        ast::Expr::FieldAccess { object, .. } => expr_mutates_names(object, names, out),
        ast::Expr::IndexAccess { object, index, .. } => {
            expr_mutates_names(object, names, out);
            expr_mutates_names(index, names, out);
        }
        ast::Expr::Borrow { inner, .. }
        | ast::Expr::Deref { inner, .. }
        | ast::Expr::SharedExpr { inner, .. }
        | ast::Expr::MoveExpr { inner, .. }
        | ast::Expr::WeakExpr { inner, .. } => expr_mutates_names(inner, names, out),
        ast::Expr::Cast { expr, .. } => expr_mutates_names(expr, names, out),
        ast::Expr::StructLiteral { fields, .. } => {
            for (_, v) in fields {
                expr_mutates_names(v, names, out);
            }
        }
        ast::Expr::ArrayLiteral { elements, .. } | ast::Expr::TupleLiteral { elements, .. } => {
            for e in elements {
                expr_mutates_names(e, names, out);
            }
        }
        ast::Expr::MapLiteral { entries, .. } => {
            for (k, v) in entries {
                expr_mutates_names(k, names, out);
                expr_mutates_names(v, names, out);
            }
        }
        ast::Expr::RangeExpr { start, end, .. } => {
            if let Some(s) = start {
                expr_mutates_names(s, names, out);
            }
            if let Some(e) = end {
                expr_mutates_names(e, names, out);
            }
        }
        ast::Expr::Await { value, .. } => expr_mutates_names(value, names, out),
        ast::Expr::InterpolatedString { parts, .. } => {
            for part in parts {
                if let ast::StringPart::Expr(e) = part {
                    expr_mutates_names(e, names, out);
                }
            }
        }
        // Leaf / no-sub-expression nodes -- nothing to recurse into.
        ast::Expr::IntLiteral { .. }
        | ast::Expr::FloatLiteral { .. }
        | ast::Expr::StringLiteral { .. }
        | ast::Expr::CharLiteral { .. }
        | ast::Expr::BoolLiteral { .. }
        | ast::Expr::NoneLiteral { .. }
        | ast::Expr::Identifier { .. } => {}
    }
}

/// Find the base identifier an assignment target ultimately writes through,
/// walking any chain of field/index accesses: `c.base = ..` and `arr[i] = ..`
/// both mutate their ROOT variable (`c`, `arr`) even though the target
/// expression itself is a `FieldAccess`/`IndexAccess`, not a bare
/// `Identifier`. Used by `block_mutates_names` to detect a captured
/// STRUCT/ENUM/array whose FIELD (or element) is mutated inside a closure
/// body -- without this, `find_mutated_captures` only recognized a bare
/// `name = ..` target, so a closure that only ever wrote through a field
/// (`c.base = c.base + 1`) was never flagged as mutating its capture `c` at
/// all, silently keeping it eligible for the `closure_locals` direct-call
/// optimization meant only for READ-only captures (see the `mutating_closures`
/// doc comment) -- the root cause of the struct/enum mutating-closure
/// JIT/AOT divergence (gotcha #11 follow-up).
fn assign_target_root_name(expr: &ast::Expr) -> Option<&str> {
    match expr {
        ast::Expr::Identifier { name, .. } => Some(name.as_str()),
        ast::Expr::FieldAccess { object, .. } => assign_target_root_name(object),
        ast::Expr::IndexAccess { object, .. } => assign_target_root_name(object),
        _ => None,
    }
}

fn block_mutates_names(
    stmts: &[ast::Stmt],
    names: &std::collections::HashSet<&str>,
    out: &mut std::collections::HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            ast::Stmt::Assign { target, value, .. } => {
                if let Some(root) = assign_target_root_name(target) {
                    if names.contains(root) {
                        out.insert(root.to_string());
                    }
                }
                expr_mutates_names(target, names, out);
                expr_mutates_names(value, names, out);
            }
            ast::Stmt::Let { value: Some(v), .. } => expr_mutates_names(v, names, out),
            ast::Stmt::Let { value: None, .. } => {}
            ast::Stmt::Return { value: Some(v), .. } => expr_mutates_names(v, names, out),
            ast::Stmt::Return { value: None, .. } => {}
            ast::Stmt::Expr { expr, .. } | ast::Stmt::Spawn { expr, .. } => {
                expr_mutates_names(expr, names, out)
            }
            ast::Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                ..
            } => {
                expr_mutates_names(condition, names, out);
                block_mutates_names(&then_block.stmts, names, out);
                for (cond, blk) in elif_clauses {
                    expr_mutates_names(cond, names, out);
                    block_mutates_names(&blk.stmts, names, out);
                }
                if let Some(eb) = else_block {
                    block_mutates_names(&eb.stmts, names, out);
                }
            }
            ast::Stmt::While { condition, body, .. } => {
                expr_mutates_names(condition, names, out);
                block_mutates_names(&body.stmts, names, out);
            }
            ast::Stmt::For { iterable, body, .. } => {
                expr_mutates_names(iterable, names, out);
                block_mutates_names(&body.stmts, names, out);
            }
            ast::Stmt::TryCatch {
                try_block,
                catch_block,
                ..
            } => {
                block_mutates_names(&try_block.stmts, names, out);
                block_mutates_names(&catch_block.stmts, names, out);
            }
            ast::Stmt::Throw { expr, .. } => expr_mutates_names(expr, names, out),
            ast::Stmt::Select { branches, .. } => {
                for b in branches {
                    expr_mutates_names(&b.channel, names, out);
                    block_mutates_names(&b.body.stmts, names, out);
                }
            }
            ast::Stmt::DenyBlock { body, .. } => block_mutates_names(&body.stmts, names, out),
            ast::Stmt::Break { .. } | ast::Stmt::Continue { .. } => {}
        }
    }
}

/// Find free variables in a lambda body that refer to locals in the enclosing scope.
///
/// Returns the names of captured variables (excluding the lambda's own parameters).
fn find_free_variables(
    ctx: &LoweringContext,
    body: &ast::Expr,
    params: &[ast::Param],
) -> Vec<String> {
    let param_names: std::collections::HashSet<String> =
        params.iter().map(|p| p.name.clone()).collect();
    let mut free_vars = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_identifiers(body, &param_names, &mut free_vars, &mut seen, ctx);

    // Transitive closure-capture expansion: if any free var is a tracked
    // closure local (i.e. its initializer was a Closure RValue with captures),
    // those captures' source locals must also be free vars of the enclosing
    // lambda. Without this, the synthesized inner-lambda function would
    // re-emit the direct-call optimization for the captured closure using
    // local IDs from the outer frame that don't exist in the inner frame.
    let mut i = 0;
    while i < free_vars.len() {
        let name = free_vars[i].clone();
        if let Some((_, capture_ops)) = ctx.closure_locals.get(&name).cloned() {
            for op in capture_ops {
                if let Operand::Local(lid) = op {
                    let lname = ctx
                        .locals
                        .iter()
                        .find(|l| l.id == lid)
                        .and_then(|l| l.name.clone());
                    if let Some(lname) = lname {
                        if !param_names.contains(&lname) && !seen.contains(&lname) {
                            seen.insert(lname.clone());
                            free_vars.push(lname);
                        }
                    }
                }
            }
        }
        i += 1;
    }

    free_vars
}

/// Recursively collect identifier names used in an expression that are:
/// 1. Not in `bound` (lambda params)
/// 2. Exist as locals in the enclosing scope
fn collect_identifiers(
    expr: &ast::Expr,
    bound: &std::collections::HashSet<String>,
    free_vars: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    ctx: &LoweringContext,
) {
    match expr {
        ast::Expr::Identifier { name, .. } => {
            if !bound.contains(name)
                && !seen.contains(name)
                && ctx
                    .locals
                    .iter()
                    .any(|l| l.name.as_deref() == Some(name.as_str()))
            {
                seen.insert(name.clone());
                free_vars.push(name.clone());
            }
        }
        ast::Expr::BinaryOp { left, right, .. } => {
            collect_identifiers(left, bound, free_vars, seen, ctx);
            collect_identifiers(right, bound, free_vars, seen, ctx);
        }
        ast::Expr::UnaryOp { operand, .. } => {
            collect_identifiers(operand, bound, free_vars, seen, ctx);
        }
        ast::Expr::FnCall { callee, args, .. } => {
            collect_identifiers(callee, bound, free_vars, seen, ctx);
            for a in args {
                collect_identifiers(a, bound, free_vars, seen, ctx);
            }
        }
        ast::Expr::MethodCall { object, args, .. } => {
            collect_identifiers(object, bound, free_vars, seen, ctx);
            for a in args {
                collect_identifiers(a, bound, free_vars, seen, ctx);
            }
        }
        ast::Expr::StaticMethodCall { args, .. } => {
            for a in args {
                collect_identifiers(a, bound, free_vars, seen, ctx);
            }
        }
        ast::Expr::FieldAccess { object, .. } => {
            collect_identifiers(object, bound, free_vars, seen, ctx);
        }
        ast::Expr::IndexAccess { object, index, .. } => {
            collect_identifiers(object, bound, free_vars, seen, ctx);
            collect_identifiers(index, bound, free_vars, seen, ctx);
        }
        ast::Expr::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_identifiers(condition, bound, free_vars, seen, ctx);
            collect_identifiers_block(&then_branch.stmts, bound, free_vars, seen, ctx);
            if let Some(eb) = else_branch {
                collect_identifiers_block(&eb.stmts, bound, free_vars, seen, ctx);
            }
        }
        ast::Expr::Borrow { inner, .. } | ast::Expr::Deref { inner, .. } => {
            collect_identifiers(inner, bound, free_vars, seen, ctx);
        }
        ast::Expr::Cast { expr, .. } => {
            collect_identifiers(expr, bound, free_vars, seen, ctx);
        }
        ast::Expr::Block { block, .. } => {
            collect_identifiers_block(&block.stmts, bound, free_vars, seen, ctx);
        }
        ast::Expr::Lambda { body, params, .. } => {
            let mut inner_bound = bound.clone();
            for p in params {
                inner_bound.insert(p.name.clone());
            }
            collect_identifiers(body, &inner_bound, free_vars, seen, ctx);
        }
        ast::Expr::StructLiteral { fields, .. } => {
            for (_, val) in fields {
                collect_identifiers(val, bound, free_vars, seen, ctx);
            }
        }
        ast::Expr::ArrayLiteral { elements, .. } | ast::Expr::TupleLiteral { elements, .. } => {
            for e in elements {
                collect_identifiers(e, bound, free_vars, seen, ctx);
            }
        }
        ast::Expr::MapLiteral { entries, .. } => {
            for (k, v) in entries {
                collect_identifiers(k, bound, free_vars, seen, ctx);
                collect_identifiers(v, bound, free_vars, seen, ctx);
            }
        }
        ast::Expr::MatchExpr { subject, arms, .. } => {
            collect_identifiers(subject, bound, free_vars, seen, ctx);
            for arm in arms {
                let mut arm_bound = bound.clone();
                collect_pattern_names(&arm.pattern, &mut arm_bound);
                if let Some(guard) = &arm.guard {
                    collect_identifiers(guard, &arm_bound, free_vars, seen, ctx);
                }
                collect_identifiers(&arm.body, &arm_bound, free_vars, seen, ctx);
            }
        }
        ast::Expr::RangeExpr { start, end, .. } => {
            if let Some(s) = start {
                collect_identifiers(s, bound, free_vars, seen, ctx);
            }
            if let Some(e) = end {
                collect_identifiers(e, bound, free_vars, seen, ctx);
            }
        }
        ast::Expr::Await { value, .. } => {
            collect_identifiers(value, bound, free_vars, seen, ctx);
        }
        ast::Expr::SharedExpr { inner, .. }
        | ast::Expr::MoveExpr { inner, .. }
        | ast::Expr::WeakExpr { inner, .. } => {
            collect_identifiers(inner, bound, free_vars, seen, ctx);
        }
        ast::Expr::PipeExpr { left, right, .. } => {
            collect_identifiers(left, bound, free_vars, seen, ctx);
            collect_identifiers(right, bound, free_vars, seen, ctx);
        }
        ast::Expr::InterpolatedString { parts, .. } => {
            for part in parts {
                if let ast::StringPart::Expr(e) = part {
                    collect_identifiers(e, bound, free_vars, seen, ctx);
                }
            }
        }
        ast::Expr::ComptimeBlock { body, .. }
        | ast::Expr::QuantumBlock { body, .. }
        | ast::Expr::UnsafeBlock { body, .. } => {
            collect_identifiers_block(&body.stmts, bound, free_vars, seen, ctx);
        }
        // Leaf literals — no sub-expressions to recurse into.
        ast::Expr::IntLiteral { .. }
        | ast::Expr::FloatLiteral { .. }
        | ast::Expr::StringLiteral { .. }
        | ast::Expr::CharLiteral { .. }
        | ast::Expr::BoolLiteral { .. }
        | ast::Expr::NoneLiteral { .. } => {}
    }
}

/// Collect free variables from a list of statements (used for spawn blocks).
fn collect_identifiers_block(
    stmts: &[ast::Stmt],
    bound: &std::collections::HashSet<String>,
    free_vars: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    ctx: &LoweringContext,
) {
    let mut local_bound = bound.clone();
    for stmt in stmts {
        match stmt {
            ast::Stmt::Let { name, value, .. } => {
                if let Some(val) = value {
                    collect_identifiers(val, &local_bound, free_vars, seen, ctx);
                }
                local_bound.insert(name.clone());
            }
            ast::Stmt::Expr { expr, .. }
            | ast::Stmt::Spawn { expr, .. }
            | ast::Stmt::Return {
                value: Some(expr), ..
            } => {
                collect_identifiers(expr, &local_bound, free_vars, seen, ctx);
            }
            ast::Stmt::Assign { target, value, .. } => {
                collect_identifiers(target, &local_bound, free_vars, seen, ctx);
                collect_identifiers(value, &local_bound, free_vars, seen, ctx);
            }
            ast::Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                ..
            } => {
                collect_identifiers(condition, &local_bound, free_vars, seen, ctx);
                collect_identifiers_block(&then_block.stmts, &local_bound, free_vars, seen, ctx);
                for (cond, block) in elif_clauses {
                    collect_identifiers(cond, &local_bound, free_vars, seen, ctx);
                    collect_identifiers_block(&block.stmts, &local_bound, free_vars, seen, ctx);
                }
                if let Some(eb) = else_block {
                    collect_identifiers_block(&eb.stmts, &local_bound, free_vars, seen, ctx);
                }
            }
            ast::Stmt::While {
                condition, body, ..
            } => {
                collect_identifiers(condition, &local_bound, free_vars, seen, ctx);
                collect_identifiers_block(&body.stmts, &local_bound, free_vars, seen, ctx);
            }
            ast::Stmt::For {
                iterable,
                body,
                pattern,
                ..
            } => {
                collect_identifiers(iterable, &local_bound, free_vars, seen, ctx);
                let mut for_bound = local_bound.clone();
                // Extract bound names from the pattern.
                collect_pattern_names(pattern, &mut for_bound);
                collect_identifiers_block(&body.stmts, &for_bound, free_vars, seen, ctx);
            }
            ast::Stmt::TryCatch {
                try_block,
                catch_name,
                catch_block,
                ..
            } => {
                collect_identifiers_block(&try_block.stmts, &local_bound, free_vars, seen, ctx);
                let mut catch_bound = local_bound.clone();
                catch_bound.insert(catch_name.clone());
                collect_identifiers_block(&catch_block.stmts, &catch_bound, free_vars, seen, ctx);
            }
            ast::Stmt::Throw { expr, .. } => {
                collect_identifiers(expr, &local_bound, free_vars, seen, ctx);
            }
            ast::Stmt::Select { branches, .. } => {
                for branch in branches {
                    collect_identifiers(&branch.channel, &local_bound, free_vars, seen, ctx);
                    let mut branch_bound = local_bound.clone();
                    branch_bound.insert(branch.pattern.clone());
                    collect_identifiers_block(
                        &branch.body.stmts,
                        &branch_bound,
                        free_vars,
                        seen,
                        ctx,
                    );
                }
            }
            ast::Stmt::DenyBlock { body, .. } => {
                collect_identifiers_block(&body.stmts, &local_bound, free_vars, seen, ctx);
            }
            // Leaf statements with no sub-expressions.
            ast::Stmt::Return { value: None, .. }
            | ast::Stmt::Break { .. }
            | ast::Stmt::Continue { .. } => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Actor dispatch function generation
// ---------------------------------------------------------------------------

/// Generate a `ActorName__dispatch` function that loops receiving messages
/// from the actor mailbox and dispatches to the appropriate handler.
///
/// Layout:
///   bb0 (poll):   tag = kryos_actor_recv_i64()
///                 Branch(tag == 0, bb_exit, bb_switch)
///   bb_switch:    Switch(tag, [(1, bb_h1), (2, bb_h2), ...], bb0)
///   bb_h_N:       arg0 = kryos_actor_recv_i64()  // for each handler param
///                 _ = Call("ActorName__handler", [state, arg0, ...])
///                 Goto(bb0)
///   bb_exit:      Return(None)
fn generate_actor_dispatch(actor_name: &str, handlers: &[(String, usize)]) -> MirFunction {
    let dispatch_name = format!("{actor_name}__dispatch");

    let mut locals = Vec::new();
    let mut next_local: u32 = 0;
    let mut next_block: u32 = 0;

    let mut alloc_local = |name: Option<&str>, ty: MirType, mutable: bool| -> LocalId {
        let id = LocalId(next_local);
        locals.push(MirLocal {
            id,
            name: name.map(|s| s.to_string()),
            ty,
            mutable,
        });
        next_local += 1;
        id
    };

    let mut alloc_block = || -> BlockId {
        let id = BlockId(next_block);
        next_block += 1;
        id
    };

    // Parameter: state_ptr (i64).
    let state_local = alloc_local(Some("state"), MirType::I64, false);
    // Tag variable (mutable — assigned each iteration).
    let tag_local = alloc_local(Some("__tag"), MirType::I64, true);
    // Comparison result.
    let cmp_local = alloc_local(Some("__cmp"), MirType::Bool, false);

    // Post-handler exception plumbing (reused across every handler block —
    // see the recovery comment below).
    let exc_flag_local = alloc_local(Some("__exc_flag"), MirType::I64, true);
    let exc_val_local = alloc_local(Some("__exc_val"), MirType::I64, true);
    let exc_report_local = alloc_local(Some("__exc_report"), MirType::I64, false);

    // Pre-allocate block IDs.
    let bb_poll = alloc_block(); // bb0
    let bb_switch = alloc_block(); // bb1
    let bb_exit = alloc_block(); // bb2

    // Allocate handler blocks (one per handler), plus one exception-recovery
    // block per handler (one per handler so the diagnostic can name the
    // specific handler that threw).
    let handler_blocks: Vec<BlockId> = handlers.iter().map(|_| alloc_block()).collect();
    let exc_blocks: Vec<BlockId> = handlers.iter().map(|_| alloc_block()).collect();

    // Pre-allocate argument locals for the handler with the most params.
    let max_args = handlers.iter().map(|(_, n)| *n).max().unwrap_or(0);
    let arg_locals: Vec<LocalId> = (0..max_args)
        .map(|i| alloc_local(Some(&format!("__arg{i}")), MirType::I64, true))
        .collect();
    // Discard local for void handler results.
    let discard_local = alloc_local(Some("__discard"), MirType::I64, false);

    let mut blocks = Vec::new();

    // bb_poll: tag = kryos_actor_recv_i64(); if tag == 0 goto exit else switch
    blocks.push(BasicBlock {
        id: bb_poll,
        instructions: vec![
            Instruction::Assign {
                dest: tag_local,
                value: RValue::Call {
                    func: "kryos_actor_recv_i64".into(),
                    args: vec![],
                },
            },
            Instruction::Assign {
                dest: cmp_local,
                value: RValue::BinOp {
                    op: MirBinOp::Eq,
                    left: Operand::Local(tag_local),
                    right: Operand::Constant(Constant::Int(0)),
                },
            },
        ],
        terminator: Terminator::Branch {
            cond: Operand::Local(cmp_local),
            then_block: bb_exit,
            else_block: bb_switch,
        },
    });

    // bb_switch: Switch(tag, [(1, bb_h1), (2, bb_h2), ...], default=bb_poll)
    let targets: Vec<(i64, BlockId)> = handler_blocks
        .iter()
        .enumerate()
        .map(|(i, &bb)| ((i as i64) + 1, bb))
        .collect();
    blocks.push(BasicBlock {
        id: bb_switch,
        instructions: vec![],
        terminator: Terminator::Switch {
            value: Operand::Local(tag_local),
            targets,
            default: bb_poll, // unknown tag → just loop back
        },
    });

    // bb_exit: return
    blocks.push(BasicBlock {
        id: bb_exit,
        instructions: vec![],
        terminator: Terminator::Return(None),
    });

    // Handler blocks: recv args, call handler, check for an uncaught throw.
    //
    // A `throw` inside a handler that isn't caught by an internal try/catch
    // sets the thread-local exception flag and returns early from the
    // handler function (kryos-rt exception.rs). Left unchecked, the codegen
    // backends' *automatic* post-call exception check would fire on this
    // call instead (it fires after every user-function call) and return
    // from THIS dispatch function — silently killing the actor's mailbox
    // loop for good, with no diagnostic (the actor thread just exits).
    //
    // Explicitly checking here — with the check call placed immediately
    // after the handler call — suppresses the backends' automatic check
    // (both backends special-case "next instruction is a call to
    // `kryos_exception_check`", the same signal `emit_exception_check`
    // uses for try/catch) and lets us recover instead: take the exception,
    // report it to stderr, and loop back to `bb_poll` for the next message.
    // One bad message aborts only itself; the actor and its state survive.
    for (i, (handler_name, param_count)) in handlers.iter().enumerate() {
        let mut instructions = Vec::new();

        // Receive each argument.
        for &dest in arg_locals.iter().take(*param_count) {
            instructions.push(Instruction::Assign {
                dest,
                value: RValue::Call {
                    func: "kryos_actor_recv_i64".into(),
                    args: vec![],
                },
            });
        }

        // Call the handler: ActorName__handler(state, arg0, arg1, ...)
        let mangled = format!("{actor_name}__{handler_name}");
        let mut call_args: Vec<Operand> = vec![Operand::Local(state_local)];
        for &local in arg_locals.iter().take(*param_count) {
            call_args.push(Operand::Local(local));
        }
        instructions.push(Instruction::Assign {
            dest: discard_local,
            value: RValue::Call {
                func: mangled,
                args: call_args,
            },
        });

        // Check for a pending exception left by the handler call above.
        instructions.push(Instruction::Assign {
            dest: exc_flag_local,
            value: RValue::Call {
                func: "kryos_exception_check".into(),
                args: vec![],
            },
        });

        blocks.push(BasicBlock {
            id: handler_blocks[i],
            instructions,
            terminator: Terminator::Branch {
                cond: Operand::Local(exc_flag_local),
                then_block: exc_blocks[i],
                else_block: bb_poll,
            },
        });

        // Exception-recovery block: take + report the exception (naming
        // the actor and handler), then continue the loop.
        let label = format!("{actor_name}.{handler_name}");
        blocks.push(BasicBlock {
            id: exc_blocks[i],
            instructions: vec![
                Instruction::Assign {
                    dest: exc_val_local,
                    value: RValue::Call {
                        func: "kryos_exception_take".into(),
                        args: vec![],
                    },
                },
                Instruction::Assign {
                    dest: exc_report_local,
                    value: RValue::Call {
                        func: "kryos_actor_report_exception".into(),
                        args: vec![
                            Operand::Constant(Constant::Str(label)),
                            Operand::Local(exc_val_local),
                        ],
                    },
                },
            ],
            terminator: Terminator::Goto(bb_poll),
        });
    }

    MirFunction {
        name: dispatch_name,
        params: vec![MirParam {
            local: state_local,
            ty: MirType::I64,
        }],
        ret_ty: MirType::Void,
        blocks,
        locals,
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    }
}

/// Extract all bound names from a pattern into a set.
fn collect_pattern_names(pattern: &ast::Pattern, names: &mut std::collections::HashSet<String>) {
    match pattern {
        ast::Pattern::Ident { name, .. } => {
            names.insert(name.clone());
        }
        ast::Pattern::Tuple { elements, .. } => {
            for p in elements {
                collect_pattern_names(p, names);
            }
        }
        ast::Pattern::Struct { fields, .. } => {
            for (_, p) in fields {
                collect_pattern_names(p, names);
            }
        }
        ast::Pattern::Enum { fields, .. } => {
            for p in fields {
                collect_pattern_names(p, names);
            }
        }
        ast::Pattern::Or { patterns, .. } => {
            for p in patterns {
                collect_pattern_names(p, names);
            }
        }
        ast::Pattern::Wildcard { .. } | ast::Pattern::Literal { .. } => {}
    }
}

/// Find free variables in a block's statements that refer to enclosing scope locals.
fn find_free_variables_block(ctx: &LoweringContext, stmts: &[ast::Stmt]) -> Vec<String> {
    let bound = std::collections::HashSet::new();
    let mut free_vars = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_identifiers_block(stmts, &bound, &mut free_vars, &mut seen, ctx);
    free_vars
}

// ---------------------------------------------------------------------------
// Monomorphization
// ---------------------------------------------------------------------------

/// Walk a parameter's TypeExpr against a concrete MirType, binding any
/// generic type-param names encountered (those listed in `generic_params`)
/// to the matching position in the concrete type. Recurses into Array,
/// Tuple, Function, and Reference shapes so `[T]`, `(A, B)`, `fn(T) -> U`,
/// and `&T` all contribute bindings.
fn extract_type_bindings(
    ctx: &LoweringContext,
    param_ty: &ast::TypeExpr,
    concrete: &MirType,
    generic_params: &[String],
    out: &mut HashMap<String, MirType>,
) {
    match (param_ty, concrete) {
        // `Boxed<T>` matched against an already-monomorphized instance name
        // ("Boxed___str"): recover the concrete args from the instance
        // registry and recurse positionally. Without this, T stayed unbound
        // and the instantiated function's params resolved to bogus
        // `Boxed___T` types (invalid IR on AOT).
        (ast::TypeExpr::Generic { name, args, .. }, MirType::Struct(mangled))
        | (ast::TypeExpr::Generic { name, args, .. }, MirType::Enum(mangled)) => {
            if let Some(inst_args) = ctx.mono_instance_args.get(mangled) {
                if mangled.starts_with(name.as_str()) {
                    for (pe, ce) in args.iter().zip(inst_args.iter()) {
                        extract_type_bindings(ctx, pe, ce, generic_params, out);
                    }
                }
            }
        }
        (ast::TypeExpr::Simple { name, .. }, c) => {
            if generic_params.iter().any(|gp| gp == name) {
                out.entry(name.clone()).or_insert_with(|| c.clone());
            }
        }
        (ast::TypeExpr::Array { element, .. }, MirType::Array(elem_ty, _)) => {
            extract_type_bindings(ctx, element, elem_ty, generic_params, out);
        }
        (ast::TypeExpr::Tuple { elements, .. }, MirType::Tuple(c_elems)) => {
            for (pe, ce) in elements.iter().zip(c_elems.iter()) {
                extract_type_bindings(ctx, pe, ce, generic_params, out);
            }
        }
        (
            ast::TypeExpr::Function { params, ret, .. },
            MirType::Function {
                params: c_params,
                ret: c_ret,
            },
        ) => {
            for (pe, ce) in params.iter().zip(c_params.iter()) {
                extract_type_bindings(ctx, pe, ce, generic_params, out);
            }
            extract_type_bindings(ctx, ret, c_ret, generic_params, out);
        }
        (ast::TypeExpr::Reference { inner, .. }, MirType::Ref { inner: c_inner, .. }) => {
            extract_type_bindings(ctx, inner, c_inner, generic_params, out);
        }
        (ast::TypeExpr::Pointer { inner, .. }, MirType::Ptr(c_inner)) => {
            extract_type_bindings(ctx, inner, c_inner, generic_params, out);
        }
        (ast::TypeExpr::Shared { inner, .. }, MirType::Shared(c_inner)) => {
            extract_type_bindings(ctx, inner, c_inner, generic_params, out);
        }
        // Optional<T> lowers to Struct("Option") -- we can't recover T from it,
        // so just skip. Generic structs / enums likewise can't be recovered
        // from a bare MirType::Struct(name) without auxiliary tracking.
        _ => {}
    }
}

/// Like `extract_type_bindings`, but takes the call-site ARGUMENT EXPRESSION
/// instead of its already-inferred `MirType`. This recovers bindings that
/// `extract_type_bindings` cannot: a generic function parameter typed
/// `Option<V>` (or `Result<V, E>`, or any user generic enum) matched against
/// an argument that is an enum-variant CONSTRUCTOR CALL (`Some(42)`, `Ok(x)`)
/// -- `infer_expr_type` types that call as the bare, un-monomorphized enum
/// name (`Option`, not `Option___i64`), which loses the concrete payload type
/// entirely, so the ordinary type-based matching in `extract_type_bindings`
/// has nothing to recover `V` from and silently leaves it unbound. Without
/// this, an unbound `V` propagated into `substitute_type_expr`/
/// `resolve_type`, which treat the raw, still-unsubstituted parameter name
/// as an unresolved struct reference (`Struct("V")`) -- corrupting any
/// struct/enum monomorphized from that binding (leaking an opaque `%V` LLVM
/// type on the AOT backend). Falls back to the ordinary type-based
/// extraction for every shape it doesn't specifically recognize, so existing
/// behavior for non-enum-constructor arguments is unchanged.
fn extract_type_bindings_from_arg(
    ctx: &mut LoweringContext,
    param_ty: &ast::TypeExpr,
    arg_expr: &ast::Expr,
    generic_params: &[String],
    out: &mut HashMap<String, MirType>,
) {
    if let (ast::TypeExpr::Generic { name: pname, args: pargs, .. }, ast::Expr::FnCall { callee, args: cargs, .. }) =
        (param_ty, arg_expr)
    {
        if let ast::Expr::Identifier { name: vname, .. } = callee.as_ref() {
            if let Some((enum_name, variant_idx)) = find_enum_variant(ctx, vname) {
                // Only trust this recovery when the param's declared generic
                // name actually matches the constructed enum (or is itself a
                // known generic enum template) -- avoids misfiring on an
                // unrelated same-shaped call.
                if &enum_name == pname || ctx.generic_enum_templates.contains_key(pname) {
                    if let Some(template) = ctx.generic_enum_templates.get(&enum_name).cloned() {
                        if let Some(variant) = template.variants.get(variant_idx as usize).cloned() {
                            let enum_generic_params = template.generic_params.clone();
                            let mut local_map: HashMap<String, MirType> = HashMap::new();
                            for (field_ty, cexpr) in variant.fields.iter().zip(cargs.iter()) {
                                extract_type_bindings_from_arg(
                                    ctx,
                                    field_ty,
                                    cexpr,
                                    &enum_generic_params,
                                    &mut local_map,
                                );
                            }
                            // Unify the enum's own generic params (in order)
                            // against `pargs` (the param type's own type
                            // args, e.g. the `V` in `Option<V>`).
                            for (egp, pty_arg) in enum_generic_params.iter().zip(pargs.iter()) {
                                if let Some(bound) = local_map.get(egp) {
                                    if let ast::TypeExpr::Simple { name, .. } = pty_arg {
                                        if generic_params.iter().any(|g| g == name) {
                                            out.entry(name.clone()).or_insert_with(|| bound.clone());
                                            continue;
                                        }
                                    }
                                    extract_type_bindings(ctx, pty_arg, bound, generic_params, out);
                                }
                            }
                            return;
                        }
                    }
                }
            }
        }
    }
    // No enum-constructor shape recognized -- fall back to the ordinary
    // type-based extraction using the argument's inferred (possibly erased)
    // MirType.
    let arg_ty = infer_expr_type(ctx, arg_expr);
    extract_type_bindings(ctx, param_ty, &arg_ty, generic_params, out);
}

/// Apply a generic-parameter type map to a TypeExpr-shaped return type
/// and produce a concrete MirType. Recurses through compound shapes so that
/// `[T]`, `(A, B)`, `fn(T) -> U`, and `&T` are all substituted, not just
/// bare `Simple T`. Anything that cannot be bound falls back to
/// `resolve_type`.
fn substitute_type_expr_to_mir(
    ctx: &mut LoweringContext,
    ty: &ast::TypeExpr,
    type_map: &HashMap<String, MirType>,
) -> MirType {
    match ty {
        ast::TypeExpr::Simple { name, .. } => {
            if let Some(concrete) = type_map.get(name) {
                return concrete.clone();
            }
            let resolved = ctx.resolve_type(ty);
            // `type_map` may be missing this name because the caller's own
            // argument-based inference couldn't recover it (e.g. a generic
            // fn's type param bound only through an enum-variant-constructor
            // argument like `Some(x)`, whose static type erases to the bare
            // `Option` tag rather than a monomorphized `Option___i64` --
            // extract_type_bindings then has nothing to match against and
            // leaves the param unbound). `ctx.resolve_type` has no notion of
            // "unbound generic param" and its catch-all (`lower_type_expr`)
            // echoes an unrecognized Simple name back as `Struct(name)`,
            // treating the raw parameter identifier as if it were a real
            // struct name. If nothing in the program actually defines a
            // struct/enum by this name, it can only be a leaked, unbound
            // type parameter -- fall back to the same erased-i64-slot model
            // used everywhere else in this file for an unresolved generic
            // param, instead of propagating a bogus `Struct("V")` that the
            // LLVM backend then emits as an undefined opaque type.
            if let MirType::Struct(ref resolved_name) = resolved {
                if resolved_name == name
                    && !ctx.struct_defs.contains_key(name)
                    && !ctx.generic_struct_templates.contains_key(name)
                    && !ctx.enum_defs.contains_key(name)
                    && !ctx.generic_enum_templates.contains_key(name)
                {
                    return MirType::I64;
                }
            }
            resolved
        }
        ast::TypeExpr::Array { element, size, .. } => MirType::Array(
            Box::new(substitute_type_expr_to_mir(ctx, element, type_map)),
            *size,
        ),
        ast::TypeExpr::Tuple { elements, .. } => MirType::Tuple(
            elements
                .iter()
                .map(|e| substitute_type_expr_to_mir(ctx, e, type_map))
                .collect(),
        ),
        ast::TypeExpr::Function { params, ret, .. } => MirType::Function {
            params: params
                .iter()
                .map(|p| substitute_type_expr_to_mir(ctx, p, type_map))
                .collect(),
            ret: Box::new(substitute_type_expr_to_mir(ctx, ret, type_map)),
        },
        ast::TypeExpr::Reference { inner, mutable, .. } => MirType::Ref {
            inner: Box::new(substitute_type_expr_to_mir(ctx, inner, type_map)),
            mutable: *mutable,
        },
        ast::TypeExpr::Pointer { inner, .. } => {
            MirType::Ptr(Box::new(substitute_type_expr_to_mir(ctx, inner, type_map)))
        }
        ast::TypeExpr::Shared { inner, .. } => MirType::Shared(Box::new(
            substitute_type_expr_to_mir(ctx, inner, type_map),
        )),
        ast::TypeExpr::Generic { name, args, .. } => {
            // Substitute the generic ARGS through the map first: a generic
            // fn returning `Boxed<T>` with T=str must monomorphize
            // Boxed<str>, not resolve the raw template (where `T` fell back
            // to i64 — Boxed<str>.value then printed as a handle).
            let concrete: Vec<MirType> = args
                .iter()
                .map(|a| substitute_type_expr_to_mir(ctx, a, type_map))
                .collect();
            // The built-in `map<K, V>` is not a user template, so it must be
            // reconstructed from the SUBSTITUTED args here. Falling through to
            // `resolve_type(ty)` would re-resolve the ORIGINAL (unsubstituted)
            // `map<str, V>` and leak a bare `%V` into the monomorphized struct
            // def's field type (`store: Map<str, V>`), which then propagated
            // through every method body and emitted invalid `extractvalue %V`.
            if name == "map" && concrete.len() == 2 {
                return MirType::Map {
                    key: Box::new(concrete[0].clone()),
                    value: Box::new(concrete[1].clone()),
                };
            }
            if ctx.generic_struct_templates.contains_key(name) {
                return MirType::Struct(monomorphize_struct(ctx, name, &concrete));
            }
            if ctx.generic_enum_templates.contains_key(name) {
                return MirType::Enum(monomorphize_enum(ctx, name, &concrete));
            }
            ctx.resolve_type(ty)
        }
        _ => ctx.resolve_type(ty),
    }
}

/// Produce a mangled name for a monomorphized specialization.
/// e.g., `id` with `[I64]` → `id___i64`.
fn mono_mangled_name(base: &str, concrete_types: &[MirType]) -> String {
    // Sanitize to identifier-safe chars: a Tuple displays as "(i64, str)",
    // and parens/commas/spaces leak into LLVM symbol names (e.g. the
    // generated drop helper), which clang then mis-parses as a param list.
    let suffix: Vec<String> = concrete_types
        .iter()
        .map(|t| {
            format!("{t}")
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
                .collect::<String>()
        })
        .collect();
    format!("{base}___{}", suffix.join("_"))
}

/// Resolve a struct-literal's effective name: for generic templates, infer
/// type args from the field expression types and monomorphize into a mangled
/// name; for concrete structs, return the name unchanged.
fn resolve_struct_literal_name(
    ctx: &mut LoweringContext,
    name: &str,
    field_exprs: &[(String, ast::Expr)],
) -> String {
    if !ctx.generic_struct_templates.contains_key(name) {
        return name.to_string();
    }
    let template = ctx
        .generic_struct_templates
        .get(name)
        .expect("template exists")
        .clone();

    // If the enclosing function's declared return type is ALREADY a
    // monomorphized instance of THIS SAME struct template (e.g. this literal
    // is the direct `return` value of a generic fn `-> Registry<V>`), keep
    // its recorded concrete type args on hand as a per-param FALLBACK.
    // Field-based inference below cannot recover a param nested inside a
    // generic container it doesn't specifically special-case (its `Generic`
    // arm only unwraps `map<K, V>`; a param nested one level deeper, e.g.
    // `entries: map<str, Option<V>>`, is invisible to it and silently
    // resolves to `None` for every instantiation). Falling back to the
    // default I64 binding in that case is wrong whenever the real V isn't
    // i64: a second instantiation (e.g. `Registry<str>` alongside an
    // existing `Registry<i64>`) then collided with the first, sharing its
    // struct layout/drop glue and corrupting memory on the AOT backend.
    // This hint is consulted ONLY per-param, after field-based inference has
    // already failed to bind that param, so it changes nothing for the
    // (already-working) cases where field inference succeeds.
    let ret_hint: Option<Vec<MirType>> = if let MirType::Struct(ref ret_name) = ctx.current_ret_ty {
        if ret_name.starts_with(&format!("{name}___")) {
            ctx.mono_instance_args.get(ret_name).cloned()
        } else {
            None
        }
    } else {
        None
    };

    let type_args: Vec<MirType> = template
        .generic_params
        .iter()
        .enumerate()
        .map(|(idx, gp)| {
            // Find a field whose declared type mentions this param, infer the
            // type of the corresponding field expression, then structurally
            // peel the param's concrete binding out of that inferred type. A
            // param nested inside a constructor (`data: [T]`, `store: map<str,
            // V>`) MUST be unwrapped: using the whole field type as the binding
            // mangles `List<i64>` as `List____i64_` (derived from the field type
            // `[i64]`), while the method's `-> List<T>` return type mangles the
            // same instantiation as `List___i64`. The mismatch made the LLVM
            // backend store `undef` into the constructor's sret slot (empty
            // arrays/maps came back null), and leaked an unsized `%V` for maps.
            template
                .fields
                .iter()
                .find(|f| type_expr_mentions_param(&f.ty, gp))
                .and_then(|f| {
                    field_exprs
                        .iter()
                        .find(|(fn_, _)| fn_ == &f.name)
                        .and_then(|(_, fexpr)| {
                            let inferred = infer_expr_type(ctx, fexpr);
                            extract_param_binding(ctx, &f.ty, &inferred, gp)
                        })
                })
                .or_else(|| ret_hint.as_ref().and_then(|args| args.get(idx).cloned()))
                .unwrap_or(MirType::I64)
        })
        .collect();
    monomorphize_struct(ctx, name, &type_args)
}

/// Structurally extract the concrete binding for one generic param from an
/// inferred field type. The field's declared `TypeExpr` says WHERE the param
/// sits (the element of `[T]`, the value of `map<str, V>`, an element of a
/// tuple, ...); we walk the inferred `MirType` in lock-step and return the
/// matching component. Returns `None` when the param is absent or the shapes
/// do not line up, in which case the caller falls back to `i64` (the uniform
/// erased slot). This keeps the struct-literal mangling identical to the
/// `-> Type<T>` return-type mangling, which the LLVM backend requires.
fn extract_param_binding(
    ctx: &LoweringContext,
    field_ty: &ast::TypeExpr,
    inferred: &MirType,
    param: &str,
) -> Option<MirType> {
    match field_ty {
        ast::TypeExpr::Simple { name, .. } => {
            if name == param {
                // `inferred` came from `infer_expr_type` on the field
                // EXPRESSION (e.g. the local `m` in `let mut m: map<str, V>
                // = {}` inside a generic fn body). A generic function
                // template's body is lowered as-is -- only its param/return
                // types are pre-substituted per instantiation -- so a local
                // variable's OWN declared type that mentions the enclosing
                // function's type parameter is never substituted and still
                // resolves (via resolve_type's lower_type_expr catch-all) to
                // the literal placeholder `MirType::Struct(param)`. Trusting
                // that placeholder as if it were a real concrete binding
                // mangles a SEPARATE, bogus struct instance (e.g.
                // `Reg___V`) distinct from the correctly-derived instance
                // the call site already registered (`Reg___i64`) -- the
                // struct literal's LLVM type then mismatches the function's
                // declared sret return type, and the codegen's type-mismatch
                // guard silently stores `undef` (reads back as 0 on AOT).
                // Treat this specific self-referential, unresolved shape as
                // "no binding found" so the caller's `ret_hint` fallback
                // (the enclosing fn's already-correct return-type
                // instantiation) supplies the real concrete type instead.
                if let MirType::Struct(ref inferred_name) = inferred {
                    if inferred_name == name
                        && !ctx.struct_defs.contains_key(name)
                        && !ctx.generic_struct_templates.contains_key(name)
                        && !ctx.enum_defs.contains_key(name)
                        && !ctx.generic_enum_templates.contains_key(name)
                    {
                        return None;
                    }
                }
                Some(inferred.clone())
            } else {
                None
            }
        }
        ast::TypeExpr::Array { element, .. } => {
            if let MirType::Array(elem, _) = inferred {
                extract_param_binding(ctx, element, elem, param)
            } else {
                None
            }
        }
        ast::TypeExpr::Tuple { elements, .. } => {
            if let MirType::Tuple(items) = inferred {
                elements
                    .iter()
                    .zip(items.iter())
                    .find_map(|(te, mt)| extract_param_binding(ctx, te, mt, param))
            } else {
                None
            }
        }
        ast::TypeExpr::Generic { name, args, .. } => {
            if name == "map" {
                if let MirType::Map { key, value } = inferred {
                    return args
                        .first()
                        .and_then(|a| extract_param_binding(ctx, a, key, param))
                        .or_else(|| {
                            args.get(1)
                                .and_then(|a| extract_param_binding(ctx, a, value, param))
                        });
                }
            }
            None
        }
        ast::TypeExpr::Reference { inner, .. } => {
            if let MirType::Ref { inner: mi, .. } = inferred {
                extract_param_binding(ctx, inner, mi, param)
            } else {
                None
            }
        }
        ast::TypeExpr::Shared { inner, .. } => {
            if let MirType::Shared(mi) = inferred {
                extract_param_binding(ctx, inner, mi, param)
            } else {
                None
            }
        }
        ast::TypeExpr::Pointer { inner, .. } => {
            if let MirType::Ptr(mi) = inferred {
                extract_param_binding(ctx, inner, mi, param)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check whether a TypeExpr mentions a particular type parameter name.
fn type_expr_mentions_param(ty: &ast::TypeExpr, param: &str) -> bool {
    match ty {
        ast::TypeExpr::Simple { name, .. } => name == param,
        ast::TypeExpr::Generic { args, .. } => {
            args.iter().any(|a| type_expr_mentions_param(a, param))
        }
        ast::TypeExpr::Array { element, .. } => type_expr_mentions_param(element, param),
        ast::TypeExpr::Reference { inner, .. }
        | ast::TypeExpr::Shared { inner, .. }
        | ast::TypeExpr::Weak { inner, .. }
        | ast::TypeExpr::Pointer { inner, .. }
        | ast::TypeExpr::Optional { inner, .. } => type_expr_mentions_param(inner, param),
        ast::TypeExpr::Tuple { elements, .. } => {
            elements.iter().any(|e| type_expr_mentions_param(e, param))
        }
        ast::TypeExpr::Function { params, ret, .. } => {
            params.iter().any(|p| type_expr_mentions_param(p, param))
                || type_expr_mentions_param(ret, param)
        }
        _ => false,
    }
}

/// Does a generic INSTANCE method's return type require per-call-site
/// monomorphization? True only for a return type that CONSTRUCTS a generic
/// struct/enum (`TypeExpr::Generic` whose args mention an impl generic
/// param) -- e.g. `-> Box<T>` or `-> Pair<B, A>`.
///
/// Deliberately false for a BARE `-> T` (`TypeExpr::Simple` naming an impl
/// generic directly): that shape returns a single 8-byte scalar slot that
/// already reinterprets safely at any call site via the existing
/// `impl_method_generic_info` fast path (see `ret_is_bare_param` in
/// `infer_expr_type`) -- routing it through monomorphization too would be
/// unnecessary work on the hottest generic-method path (Box::get, Nest::get,
/// Pair::get_first/second_of/swap's OWN bare-field-return siblings, the
/// self-host compiler's heavy use of this shape).
///
/// Also deliberately false for `map<K, V>` (the builtin, not a user
/// struct/enum template) and any other compound return that merely MENTIONS
/// a generic param without being a constructed struct/enum (`[T]`, `(T,
/// i64)`) -- those already keep the erased i64-slot shape by design (see the
/// `ret_is_bare_param` comment block in `infer_expr_type`), and enums are
/// UNAFFECTED by the bug this targets in the first place: every enum lowers
/// to the SAME anonymous/unnamed LLVM literal struct `{ i64, i64, ... }`
/// (`enum_llvm_type`) regardless of which enum or which monomorphization --
/// there is no nominal-type mismatch possible for enums, only for named
/// struct types. Restricting to `name != "map"` here is a conservative
/// no-op filter; it costs nothing to also register a (harmless, unused)
/// template for a generic-enum return, but there is no bug to fix there, so
/// this stays scoped to what has an observed AOT failure: a constructed
/// STRUCT differing from the erased single-lowered-copy shape.
fn instance_ret_needs_monomorphization(
    ret_ty: &Option<ast::TypeExpr>,
    impl_generics: &[String],
) -> bool {
    match ret_ty {
        Some(ast::TypeExpr::Generic { name, args, .. }) if name != "map" => args
            .iter()
            .any(|a| impl_generics.iter().any(|gp| type_expr_mentions_param(a, gp))),
        _ => false,
    }
}

/// True if the expression references `self` (the receiver) anywhere. Used to
/// decide whether a generic INSTANCE method's body must be monomorphized per
/// concrete receiver type. A body that OPERATES on `self` (reads a field and
/// does arithmetic / to_string / a cast on it, binds it to a local, passes it
/// along) can touch a generic-typed field, which the single erased-to-i64 copy
/// mis-handles: a `str` field flows through `+`/`to_string` as its raw pointer
/// (SIGSEGV on concat, garbage on print) and an `f64` field as its bit pattern.
/// See [`generic_instance_method_needs_body_monomorph`].
fn expr_mentions_self(e: &ast::Expr) -> bool {
    use ast::Expr::*;
    match e {
        Identifier { name, .. } => name == "self",
        IntLiteral { .. }
        | FloatLiteral { .. }
        | StringLiteral { .. }
        | CharLiteral { .. }
        | BoolLiteral { .. }
        | NoneLiteral { .. } => false,
        InterpolatedString { parts, .. } => parts.iter().any(|p| match p {
            ast::StringPart::Expr(inner) => expr_mentions_self(inner),
            ast::StringPart::Literal(_) => false,
        }),
        FieldAccess { object, .. } => expr_mentions_self(object),
        IndexAccess { object, index, .. } => {
            expr_mentions_self(object) || expr_mentions_self(index)
        }
        BinaryOp { left, right, .. } => expr_mentions_self(left) || expr_mentions_self(right),
        UnaryOp { operand, .. } => expr_mentions_self(operand),
        FnCall { callee, args, .. } => {
            expr_mentions_self(callee) || args.iter().any(expr_mentions_self)
        }
        MethodCall { object, args, .. } => {
            expr_mentions_self(object) || args.iter().any(expr_mentions_self)
        }
        StaticMethodCall { args, .. } => args.iter().any(expr_mentions_self),
        ArrayLiteral { elements, .. } | TupleLiteral { elements, .. } => {
            elements.iter().any(expr_mentions_self)
        }
        MapLiteral { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_mentions_self(k) || expr_mentions_self(v)),
        StructLiteral { fields, .. } => fields.iter().any(|(_, v)| expr_mentions_self(v)),
        Lambda { body, .. } => expr_mentions_self(body),
        IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            expr_mentions_self(condition)
                || block_mentions_self(then_branch)
                || else_branch.as_ref().is_some_and(block_mentions_self)
        }
        MatchExpr { subject, arms, .. } => {
            expr_mentions_self(subject)
                || arms.iter().any(|a| {
                    a.guard.as_ref().is_some_and(|g| expr_mentions_self(g))
                        || expr_mentions_self(&a.body)
                })
        }
        RangeExpr { start, end, .. } => {
            start.as_ref().is_some_and(|s| expr_mentions_self(s))
                || end.as_ref().is_some_and(|e| expr_mentions_self(e))
        }
        PipeExpr { left, right, .. } => expr_mentions_self(left) || expr_mentions_self(right),
        Borrow { inner, .. }
        | Deref { inner, .. }
        | SharedExpr { inner, .. }
        | MoveExpr { inner, .. }
        | WeakExpr { inner, .. } => expr_mentions_self(inner),
        ComptimeBlock { body, .. } | QuantumBlock { body, .. } | UnsafeBlock { body, .. } => {
            block_mentions_self(body)
        }
        Block { block, .. } => block_mentions_self(block),
        Cast { expr, .. } => expr_mentions_self(expr),
        Await { value, .. } => expr_mentions_self(value),
    }
}

/// True if any statement in the block references `self`. See [`expr_mentions_self`].
fn block_mentions_self(b: &ast::Block) -> bool {
    b.stmts.iter().any(stmt_mentions_self)
}

fn stmt_mentions_self(s: &ast::Stmt) -> bool {
    use ast::Stmt::*;
    match s {
        Let { value, .. } => value.as_ref().is_some_and(|v| expr_mentions_self(v)),
        Assign { target, value, .. } => expr_mentions_self(target) || expr_mentions_self(value),
        Return { value, .. } => value.as_ref().is_some_and(|v| expr_mentions_self(v)),
        If {
            condition,
            then_block,
            elif_clauses,
            else_block,
            ..
        } => {
            expr_mentions_self(condition)
                || block_mentions_self(then_block)
                || elif_clauses
                    .iter()
                    .any(|(c, b)| expr_mentions_self(c) || block_mentions_self(b))
                || else_block.as_ref().is_some_and(block_mentions_self)
        }
        For { iterable, body, .. } => expr_mentions_self(iterable) || block_mentions_self(body),
        While { condition, body, .. } => expr_mentions_self(condition) || block_mentions_self(body),
        Break { .. } | Continue { .. } => false,
        Expr { expr, .. } | Spawn { expr, .. } | Throw { expr, .. } => expr_mentions_self(expr),
        // Select is rare in a generic method body; conservatively monomorphize.
        Select { .. } => true,
        TryCatch {
            try_block,
            catch_block,
            ..
        } => block_mentions_self(try_block) || block_mentions_self(catch_block),
        DenyBlock { body, .. } => block_mentions_self(body),
    }
}

/// True when the block is exactly one bare passthrough of a `self` field --
/// `return self.<f>` or a tail `self.<f>`. These (the self-host's hot generic
/// getters `get`/`len`/...) stay on the erased single-lowered-copy fast path:
/// the returned field is a uniform 8-byte slot that reinterprets correctly at
/// each call site (see `impl_method_generic_info`/`ret_is_bare_param`), so
/// monomorphizing them would be needless churn on the hottest generic path.
fn is_single_bare_self_passthrough(b: &ast::Block) -> bool {
    if b.stmts.len() != 1 {
        return false;
    }
    let e = match &b.stmts[0] {
        ast::Stmt::Return { value: Some(e), .. } => e,
        ast::Stmt::Expr { expr, .. } => expr,
        _ => return false,
    };
    matches!(e, ast::Expr::FieldAccess { object, .. }
        if matches!(object.as_ref(), ast::Expr::Identifier { name, .. } if name == "self"))
}

/// Decide whether a generic INSTANCE method's body must be monomorphized per
/// concrete receiver type. TRUE when the body operates on `self` beyond a bare
/// passthrough return -- that is where a generic-typed `self` field would be
/// mis-computed by the single erased copy (SIGSEGV / wrong value for str/f64
/// fields). A bare passthrough getter keeps the erased fast path; a body that
/// never touches `self` (returns a literal / only its non-self params) also
/// stays erased (nothing generic to specialize). Monomorphizing is always
/// SOUND inside a generic impl -- the i64 instantiation equals the erased body
/// -- so this errs toward specializing whenever `self` is used non-trivially.
fn generic_instance_method_needs_body_monomorph(body: &ast::Block) -> bool {
    !is_single_bare_self_passthrough(body) && block_mentions_self(body)
}

/// Balance ownership for a BARE-IDENTIFIER heap element placed into an array or
/// tuple LITERAL. Both backends store the element as a raw i64 slot (the box
/// POINTER for a str/array/map, an inline aggregate for a struct/enum) with NO
/// clone or refcount bump (see `RValue::Array` in both codegens), so a source
/// local placed into the literal ends up sharing its box with the container:
/// the container's teardown AND the source local's own scope-end Drop both free
/// it -- a double-free (`let outer = [inner1]` then inner1 dropped freed inner1's
/// buffer twice). Fix, mirroring the struct-literal path:
///   - str/array/map -> RETAIN the box (kryos_string/array/map_retain) so the
///     container's reference is its own and both drops balance;
///   - struct/enum -> no refcounted box to retain (raw aggregate value), so mark
///     the source local's scope-end drop as already accounted for (its ownership
///     moved into the literal).
/// Only a BARE IDENTIFIER is handled: a fresh temp (literal / call / concat)
/// already hands over its own +1. A PARAM or BORROWED source is skipped -- it
/// gets no scope-end Drop, so retaining/moving it would leak or double-account.
fn retain_or_move_literal_element(ctx: &mut LoweringContext, e: &ast::Expr, op: &Operand) {
    let Operand::Local(id) = op else { return };
    if !matches!(e, ast::Expr::Identifier { .. }) {
        return;
    }
    if ctx.param_locals.contains(&id.0) || ctx.borrowed_locals.contains(&id.0) {
        return;
    }
    let ty = ctx
        .locals
        .iter()
        .find(|l| l.id == *id)
        .map(|l| l.ty.clone());
    let Some(ty) = ty else { return };
    let retain_fn = match ty {
        MirType::Str => Some("kryos_string_retain"),
        MirType::Array(_, _) => Some("kryos_array_retain"),
        MirType::Map { .. } => Some("kryos_map_retain"),
        MirType::Struct(_) | MirType::Enum(_) => {
            ctx.partial_moved_locals.insert(id.0);
            None
        }
        _ => None,
    };
    if let Some(f) = retain_fn {
        let scratch = ctx.alloc_temp(MirType::I64);
        ctx.emit(Instruction::Assign {
            dest: scratch,
            value: RValue::Call {
                func: f.into(),
                args: vec![op.clone()],
            },
        });
    }
}

/// Monomorphize a generic struct template with concrete type arguments.
///
/// Infers type parameter bindings from the generic type arguments,
/// substitutes them in the field types, and inserts the monomorphized struct def.
/// Returns the mangled struct name.
fn monomorphize_struct(
    ctx: &mut LoweringContext,
    struct_name: &str,
    type_args: &[MirType],
) -> String {
    // Retrieve the generic struct template.
    let template = ctx
        .generic_struct_templates
        .get(struct_name)
        .expect("struct template exists")
        .clone();

    let generic_params = template.generic_params.clone();
    let fields = template.fields.clone();

    // Build type param → concrete type mapping by position.
    let mut type_map: HashMap<String, MirType> = HashMap::new();
    for (i, param_name) in generic_params.iter().enumerate() {
        if let Some(concrete) = type_args.get(i) {
            type_map.insert(param_name.clone(), concrete.clone());
        }
    }

    // Build the list of concrete types in generic_params order for the mangled name.
    let concrete_ordered: Vec<MirType> = generic_params
        .iter()
        .map(|gp| type_map.get(gp).cloned().unwrap_or(MirType::I64))
        .collect();
    let mangled = mono_mangled_name(struct_name, &concrete_ordered);
    ctx.mono_instance_args
        .entry(mangled.clone())
        .or_insert_with(|| concrete_ordered.clone());

    // If already monomorphized, just return the name.
    if ctx.struct_defs.contains_key(&mangled) {
        return mangled;
    }

    // Insert a PLACEHOLDER before substituting field types. A self-referential
    // generic struct (`struct Node<T> { next: Option<Node<T>> }`) resolves a
    // field whose type mentions THIS same instantiation, which recurses back
    // into monomorphize_struct for the same mangled name. Without the
    // placeholder present, the "already monomorphized" check above never fired
    // (the entry is inserted only at the END), so the recursion was unbounded
    // -> stack overflow at runtime (`kryos run` exit 253) on the mere
    // construction of the base case, while `kryos check` passed. Mirrors the
    // identical placeholder guard in monomorphize_enum; overwritten with the
    // real field list below once substitution finishes.
    ctx.struct_defs.insert(mangled.clone(), Vec::new());

    // Substitute type parameters in the field types.
    let field_list: Vec<(String, MirType)> = fields
        .iter()
        .map(|f| {
            // Substitute straight to MirType — the AST round-trip
            // (substitute_type_expr + resolve_type) collapses compound
            // concrete types (Tuple/Array/Function) to "i64" via
            // mir_type_to_name's catch-all, mistyping e.g. Wrap<(i64, str)>.
            (
                f.name.clone(),
                substitute_type_expr_to_mir(ctx, &f.ty, &type_map),
            )
        })
        .collect();

    // Insert the monomorphized struct definition (overwrites the placeholder).
    ctx.struct_defs.insert(mangled.clone(), field_list);

    mangled
}

/// Monomorphize a generic enum template with concrete type arguments.
///
/// Similar to monomorphize_struct, but for enum variants.
fn monomorphize_enum(ctx: &mut LoweringContext, enum_name: &str, type_args: &[MirType]) -> String {
    // Retrieve the generic enum template.
    let template = ctx
        .generic_enum_templates
        .get(enum_name)
        .expect("enum template exists")
        .clone();

    let generic_params = template.generic_params.clone();
    let variants = template.variants.clone();

    // Build type param → concrete type mapping by position.
    let mut type_map: HashMap<String, MirType> = HashMap::new();
    for (i, param_name) in generic_params.iter().enumerate() {
        if let Some(concrete) = type_args.get(i) {
            type_map.insert(param_name.clone(), concrete.clone());
        }
    }

    // Build the list of concrete types in generic_params order for the mangled name.
    let concrete_ordered: Vec<MirType> = generic_params
        .iter()
        .map(|gp| type_map.get(gp).cloned().unwrap_or(MirType::I64))
        .collect();
    let mangled = mono_mangled_name(enum_name, &concrete_ordered);
    // Mirror monomorphize_struct: register this instance's concrete type args
    // so extract_type_bindings can recover a generic fn's type param from an
    // argument whose static type is this monomorphized enum (e.g. binding V
    // from an `Option<V>`-typed param matched against an `Option___i64`
    // argument). Without this, the enum branch of extract_type_bindings's
    // lookup into mono_instance_args always misses, the param silently stays
    // unbound, and callers fall through resolve_type's ctx-less path, which
    // treats the bare param name as an unresolved struct (`MirType::Struct("V")`)
    // -- corrupting any struct monomorphized from that binding (e.g. leaking
    // an opaque `%V` into `Registry<V>`'s LLVM type on the AOT backend).
    ctx.mono_instance_args
        .entry(mangled.clone())
        .or_insert_with(|| concrete_ordered.clone());

    // If already monomorphized, just return the name.
    if ctx.enum_defs.contains_key(&mangled) {
        return mangled;
    }

    // Insert a placeholder BEFORE substituting variant field types. A
    // self-referential generic enum (e.g. `List<T> { Nil, Cons(T, List<T>) }`)
    // has a variant field that is itself `List<T>` — substituting it below
    // calls back into `monomorphize_enum(ctx, "List", [i64])` for the SAME
    // mangled name while this outer call is still running. Without an entry
    // present yet, the "already monomorphized" check above never short-
    // circuits that reentrant call, which recurses the same way again, ad
    // infinitum, overflowing the stack (in both the JIT and AOT pipelines,
    // since both lower through this fn). Reserving the slot first lets the
    // reentrant call see it and return immediately, exactly as it does for a
    // second, wholly separate `monomorphize_enum` call after this one
    // finishes. The placeholder is overwritten with the real definition
    // once field substitution completes.
    ctx.enum_defs.insert(mangled.clone(), Vec::new());

    // Substitute type parameters in the variant field types.
    let variant_defs: Vec<EnumVariantDef> = variants
        .iter()
        .map(|v| EnumVariantDef {
            name: v.name.clone(),
            fields: v
                .fields
                .iter()
                .map(|f| {
                    // Straight to MirType — see monomorphize_struct: the AST
                    // round-trip collapses Tuple/Array/Function payloads to
                    // i64 (Option<(i64, str)> bound its payload as i64).
                    substitute_type_expr_to_mir(ctx, f, &type_map)
                })
                .collect(),
        })
        .collect();

    // Insert the monomorphized enum definition (overwrites the placeholder).
    ctx.enum_defs.insert(mangled.clone(), variant_defs);

    mangled
}

// ---------------------------------------------------------------------------
// Structural equality helpers for `==` / `!=` on struct/enum operands.
//
// The type checker permits `BinOp::Eq`/`BinOp::Neq` on any two operands of
// the same type (it just unifies both sides and returns `Bool`), but neither
// codegen backend can compare two aggregate (struct/enum) VALUES directly:
// Cranelift's icmp/fcmp require scalar operands (comparing a struct/enum
// there silently compares the two HANDLES/pointers, not their content --
// `Pt{1,2} == Pt{1,2}` came back `false`), and LLVM's textual `icmp`
// rejects an aggregate type outright (a hard AOT build failure).
//
// `ensure_struct_eq_helper` / `ensure_enum_eq_helper` synthesize a per-type
// `__kryos_eq_<Type>(a, b) -> bool` MIR function (mirroring the existing
// `__kryos_drop_<Type>` per-type helper both backends already generate for
// destructors) the FIRST time a type is compared with `==`/`!=`, and cache
// it in `ctx.synthesized_eq_helpers` so later comparisons of the same type
// reuse the one helper. The helper's body is built as ordinary Kryos AST
// (field access / match, exactly the shape a hand-written `eq` method would
// take) and lowered through the normal `lower_function`/`lower_match` path,
// so nested struct/enum fields recurse into their OWN helper automatically
// via the very same call-site rewrite in `lower_expr_to_rvalue`'s
// `BinaryOp` arm -- no bespoke codegen is needed in either backend.
//
// Scope: the type checker (`kryos-types::check::contains_array_or_map`)
// rejects `==`/`!=` on any struct/enum that has an array/map field, directly
// or through a nested struct/enum, with a clear compile error -- so by the
// time MIR lowering reaches this code the operand type is guaranteed to
// contain only scalar/str/nested-struct/enum fields. The `Array`/`Map` arms
// below are unreachable in a program that passed type-checking; they are
// defensive (skip the field rather than emit something that could crash a
// backend) in case that invariant is ever violated.

/// Ensure a `__kryos_eq_<StructName>(a, b) -> bool` helper exists for
/// `struct_name`, synthesizing it on first use. Returns the helper's name.
fn ensure_struct_eq_helper(ctx: &mut LoweringContext, struct_name: &str) -> String {
    let helper_name = format!("__kryos_eq_{struct_name}");
    if ctx.synthesized_eq_helpers.contains(&helper_name) {
        return helper_name;
    }
    // Insert before building the body: a field that is itself (recursively)
    // the same struct type would otherwise recurse forever. Real Kryos
    // structs are value types (no indirection), so this cannot happen in
    // practice, but the guard is cheap and matches the monomorphization
    // placeholder-before-recursing pattern used elsewhere in this file.
    ctx.synthesized_eq_helpers.insert(helper_name.clone());

    let span = kryos_errors::Span::DUMMY;
    let fields = ctx.struct_defs.get(struct_name).cloned().unwrap_or_default();
    let self_ty = ast::TypeExpr::Simple {
        name: struct_name.to_string(),
        span,
    };
    let params = vec![
        ast::Param {
            name: "__eq_a".to_string(),
            ty: Some(self_ty.clone()),
            default: None,
            span,
        },
        ast::Param {
            name: "__eq_b".to_string(),
            ty: Some(self_ty),
            default: None,
            span,
        },
    ];

    let body_expr: ast::Expr = {
        // Array/map fields cannot reach here in a program that passed
        // type-checking (see module doc comment above) -- filtered
        // defensively rather than trusted blindly.
        let comparable: Vec<&(String, MirType)> = fields
            .iter()
            .filter(|(_, fty)| !matches!(fty, MirType::Array(..) | MirType::Map { .. }))
            .collect();
        if comparable.is_empty() {
            ast::Expr::BoolLiteral { value: true, span }
        } else {
            let mut iter = comparable
                .into_iter()
                .map(|(fname, _)| eq_field_access_expr("__eq_a", "__eq_b", fname, span));
            let mut acc = iter.next().expect("non-empty");
            for next in iter {
                acc = ast::Expr::BinaryOp {
                    op: ast::BinOp::And,
                    left: Box::new(acc),
                    right: Box::new(next),
                    span,
                };
            }
            acc
        }
    };

    let body = ast::Block {
        stmts: vec![ast::Stmt::Return {
            value: Some(body_expr),
            span,
        }],
        span,
    };
    let ret_ty = Some(ast::TypeExpr::Simple {
        name: "bool".to_string(),
        span,
    });

    let saved = ctx.save_function_state();
    let mir_func = lower_function(ctx, &helper_name, &params, &ret_ty, &body);
    ctx.restore_function_state(saved);
    ctx.func_ret_types
        .insert(helper_name.clone(), MirType::Bool);
    ctx.monomorphized_functions.push(mir_func);

    helper_name
}

/// Ensure a `__kryos_eq_tuple_<elemtypes>(a, b) -> bool` helper exists for a
/// tuple type, comparing element-by-element (`a.0 == b.0 and a.1 == b.1 ...`).
/// Tuples are structurally typed (no name), so the helper name is mangled from
/// the element types; a nested tuple/struct/enum element recurses through the
/// same `==` rewrite. A tuple with an array/map element is rejected at
/// type-check (contains_array_or_map covers Type::Tuple), so those elements are
/// filtered defensively here, exactly like the struct helper.
fn ensure_tuple_eq_helper(ctx: &mut LoweringContext, elems: &[MirType]) -> String {
    let mangle: String = elems
        .iter()
        .map(|e| {
            format!("{e}")
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("_");
    let helper_name = format!("__kryos_eq_tuple_{}_{}", elems.len(), mangle);
    if ctx.synthesized_eq_helpers.contains(&helper_name) {
        return helper_name;
    }
    ctx.synthesized_eq_helpers.insert(helper_name.clone());

    let span = kryos_errors::Span::DUMMY;
    let tuple_ty = ast::TypeExpr::Tuple {
        elements: elems
            .iter()
            .map(|e| mir_type_to_type_expr_spanned(e, span))
            .collect(),
        span,
    };
    let params = vec![
        ast::Param {
            name: "__eq_a".to_string(),
            ty: Some(tuple_ty.clone()),
            default: None,
            span,
        },
        ast::Param {
            name: "__eq_b".to_string(),
            ty: Some(tuple_ty),
            default: None,
            span,
        },
    ];

    let body_expr: ast::Expr = {
        let comparable: Vec<usize> = (0..elems.len())
            .filter(|&i| !matches!(elems[i], MirType::Array(..) | MirType::Map { .. }))
            .collect();
        if comparable.is_empty() {
            ast::Expr::BoolLiteral { value: true, span }
        } else {
            let mut iter = comparable
                .into_iter()
                .map(|i| eq_field_access_expr("__eq_a", "__eq_b", &i.to_string(), span));
            let mut acc = iter.next().expect("non-empty");
            for next in iter {
                acc = ast::Expr::BinaryOp {
                    op: ast::BinOp::And,
                    left: Box::new(acc),
                    right: Box::new(next),
                    span,
                };
            }
            acc
        }
    };

    let body = ast::Block {
        stmts: vec![ast::Stmt::Return {
            value: Some(body_expr),
            span,
        }],
        span,
    };
    let ret_ty = Some(ast::TypeExpr::Simple {
        name: "bool".to_string(),
        span,
    });

    let saved = ctx.save_function_state();
    let mir_func = lower_function(ctx, &helper_name, &params, &ret_ty, &body);
    ctx.restore_function_state(saved);
    ctx.func_ret_types
        .insert(helper_name.clone(), MirType::Bool);
    ctx.monomorphized_functions.push(mir_func);

    helper_name
}

/// `a.<field> == b.<field>`, where `a_name`/`b_name` name the two params.
fn eq_field_access_expr(a_name: &str, b_name: &str, field: &str, span: kryos_errors::Span) -> ast::Expr {
    let a_field = ast::Expr::FieldAccess {
        object: Box::new(ast::Expr::Identifier {
            name: a_name.to_string(),
            span,
        }),
        field: field.to_string(),
        span,
    };
    let b_field = ast::Expr::FieldAccess {
        object: Box::new(ast::Expr::Identifier {
            name: b_name.to_string(),
            span,
        }),
        field: field.to_string(),
        span,
    };
    ast::Expr::BinaryOp {
        op: ast::BinOp::Eq,
        left: Box::new(a_field),
        right: Box::new(b_field),
        span,
    }
}

/// Ensure a `__kryos_eq_<EnumName>(a, b) -> bool` helper exists for
/// `enum_name`, synthesizing it on first use. Returns the helper's name.
///
/// Body shape (identical to hand-written stdlib code like
/// `std::option::is_some`, just nested for the second operand):
/// ```text
/// return match a {
///     Variant0(a0, a1) => match b {
///         Variant0(b0, b1) => a0 == b0 && a1 == b1,
///         _ => false,
///     },
///     Empty => match b { Empty => true, _ => false },
///     ...
/// }
/// ```
fn ensure_enum_eq_helper(ctx: &mut LoweringContext, enum_name: &str) -> String {
    let helper_name = format!("__kryos_eq_{enum_name}");
    if ctx.synthesized_eq_helpers.contains(&helper_name) {
        return helper_name;
    }
    ctx.synthesized_eq_helpers.insert(helper_name.clone());

    let span = kryos_errors::Span::DUMMY;
    let variants = ctx.enum_defs.get(enum_name).cloned().unwrap_or_default();
    let self_ty = ast::TypeExpr::Simple {
        name: enum_name.to_string(),
        span,
    };
    let params = vec![
        ast::Param {
            name: "__eq_a".to_string(),
            ty: Some(self_ty.clone()),
            default: None,
            span,
        },
        ast::Param {
            name: "__eq_b".to_string(),
            ty: Some(self_ty),
            default: None,
            span,
        },
    ];

    let mut outer_arms: Vec<ast::MatchArm> = Vec::new();
    for (variant_idx, variant) in variants.iter().enumerate() {
        // Array/map payload fields cannot reach here in a program that
        // passed type-checking (see module doc comment above); a field of
        // one of those types is skipped (treated as always-equal) rather
        // than trusted blindly.
        let field_is_comparable: Vec<bool> = variant
            .fields
            .iter()
            .map(|fty| !matches!(fty, MirType::Array(..) | MirType::Map { .. }))
            .collect();
        let a_names: Vec<String> = (0..variant.fields.len())
            .map(|i| format!("__eq_a_{variant_idx}_{i}"))
            .collect();
        let b_names: Vec<String> = (0..variant.fields.len())
            .map(|i| format!("__eq_b_{variant_idx}_{i}"))
            .collect();
        let a_field_patterns: Vec<ast::Pattern> = a_names
            .iter()
            .map(|n| ast::Pattern::Ident {
                name: n.clone(),
                mutable: false,
                span,
            })
            .collect();
        let b_field_patterns: Vec<ast::Pattern> = b_names
            .iter()
            .map(|n| ast::Pattern::Ident {
                name: n.clone(),
                mutable: false,
                span,
            })
            .collect();

        let comparable_pairs: Vec<(&String, &String)> = a_names
            .iter()
            .zip(b_names.iter())
            .zip(field_is_comparable.iter())
            .filter(|(_, comparable)| **comparable)
            .map(|(pair, _)| pair)
            .collect();
        let inner_body: ast::Expr = if comparable_pairs.is_empty() {
            ast::Expr::BoolLiteral { value: true, span }
        } else {
            let mut it = comparable_pairs.into_iter().map(|(an, bn)| ast::Expr::BinaryOp {
                op: ast::BinOp::Eq,
                left: Box::new(ast::Expr::Identifier {
                    name: an.clone(),
                    span,
                }),
                right: Box::new(ast::Expr::Identifier {
                    name: bn.clone(),
                    span,
                }),
                span,
            });
            let mut acc = it.next().expect("non-empty");
            for next in it {
                acc = ast::Expr::BinaryOp {
                    op: ast::BinOp::And,
                    left: Box::new(acc),
                    right: Box::new(next),
                    span,
                };
            }
            acc
        };

        let inner_match = ast::Expr::MatchExpr {
            subject: Box::new(ast::Expr::Identifier {
                name: "__eq_b".to_string(),
                span,
            }),
            arms: vec![
                ast::MatchArm {
                    pattern: ast::Pattern::Enum {
                        name: enum_name.to_string(),
                        variant: variant.name.clone(),
                        fields: b_field_patterns,
                        span,
                    },
                    guard: None,
                    body: Box::new(inner_body),
                    span,
                },
                ast::MatchArm {
                    pattern: ast::Pattern::Wildcard { span },
                    guard: None,
                    body: Box::new(ast::Expr::BoolLiteral { value: false, span }),
                    span,
                },
            ],
            span,
        };

        outer_arms.push(ast::MatchArm {
            pattern: ast::Pattern::Enum {
                name: enum_name.to_string(),
                variant: variant.name.clone(),
                fields: a_field_patterns,
                span,
            },
            guard: None,
            body: Box::new(inner_match),
            span,
        });
    }

    let match_expr = ast::Expr::MatchExpr {
        subject: Box::new(ast::Expr::Identifier {
            name: "__eq_a".to_string(),
            span,
        }),
        arms: outer_arms,
        span,
    };
    let body = ast::Block {
        stmts: vec![ast::Stmt::Return {
            value: Some(match_expr),
            span,
        }],
        span,
    };
    let ret_ty = Some(ast::TypeExpr::Simple {
        name: "bool".to_string(),
        span,
    });

    let saved = ctx.save_function_state();
    let mir_func = lower_function(ctx, &helper_name, &params, &ret_ty, &body);
    ctx.restore_function_state(saved);
    ctx.func_ret_types
        .insert(helper_name.clone(), MirType::Bool);
    ctx.monomorphized_functions.push(mir_func);

    helper_name
}

/// Monomorphize a generic function template with concrete call-site argument
/// expressions.
///
/// Infers type parameter bindings from the argument expressions (not just
/// their already-inferred types -- see `extract_type_bindings_from_arg`),
/// substitutes them in the parameter/return type annotations, lowers the
/// specialized copy, and returns the mangled name.
fn monomorphize(ctx: &mut LoweringContext, func_name: &str, args: &[ast::Expr]) -> String {
    // Build type param → concrete type mapping by matching args to params.
    let template = ctx
        .generic_templates
        .get(func_name)
        .expect("template exists");
    let generic_params = template.generic_params.clone();
    let template_params = template.params.clone();
    let template_ret_ty = template.ret_ty.clone();
    let template_body = template.body.clone();

    let mut type_map: HashMap<String, MirType> = HashMap::new();
    for (i, param) in template_params.iter().enumerate() {
        if let (Some(param_ty), Some(arg_expr)) = (&param.ty, args.get(i)) {
            extract_type_bindings_from_arg(ctx, param_ty, arg_expr, &generic_params, &mut type_map);
        }
    }

    // Build the list of concrete types in generic_params order for the mangled name.
    let concrete_ordered: Vec<MirType> = generic_params
        .iter()
        .map(|gp| type_map.get(gp).cloned().unwrap_or(MirType::I64))
        .collect();
    let mangled = mono_mangled_name(func_name, &concrete_ordered);

    // If already monomorphized, just return the name.
    if ctx.monomorphized.contains_key(&mangled) {
        return mangled;
    }
    ctx.monomorphized.insert(mangled.clone(), true);

    // Register the return type for the specialized function. Substitute
    // generic params recursively (handles `-> T`, `-> [T]`, `-> (A, B)`).
    let specialized_ret = if let Some(ret_ty) = &template_ret_ty {
        substitute_type_expr_to_mir(ctx, ret_ty, &type_map)
    } else {
        MirType::Void
    };
    ctx.func_ret_types.insert(mangled.clone(), specialized_ret);

    // Substitute type params in the parameter list.
    let specialized_params: Vec<ast::Param> = template_params
        .iter()
        .map(|p| {
            let new_ty =
                p.ty.as_ref()
                    .map(|ty_expr| substitute_type_expr(ty_expr, &type_map));
            ast::Param {
                name: p.name.clone(),
                ty: new_ty,
                default: p.default.clone(),
                span: p.span,
            }
        })
        .collect();

    let specialized_ret_ty = template_ret_ty
        .as_ref()
        .map(|ty| substitute_type_expr(ty, &type_map));

    // Save the current function state — lower_function will call reset().
    let saved = ctx.save_function_state();
    // Expose the concrete type bindings so the body's local `let` annotations
    // (`let x: T` / `let x: Box<T>`) substitute to the instantiation. Saved and
    // restored to survive NESTED monomorphizations triggered while lowering.
    let saved_bindings =
        std::mem::replace(&mut ctx.active_generic_bindings, type_map.clone());

    // Lower the specialized function.
    let mir_func = lower_function(
        ctx,
        &mangled,
        &specialized_params,
        &specialized_ret_ty,
        &template_body,
    );

    // Restore the caller's function state.
    ctx.active_generic_bindings = saved_bindings;
    ctx.restore_function_state(saved);

    // Store the monomorphized function for collection by lower_module.
    ctx.monomorphized_functions.push(mir_func);

    mangled
}

/// Peek at a generic impl associated-function call's per-instantiation
/// return type WITHOUT lowering/monomorphizing it -- for `infer_expr_type`
/// sites (the dotted `MethodCall` and `::`-style `StaticMethodCall` arms),
/// which only need the correctly-specialized MirType to type the call's
/// result (e.g. so a chained `.field` access or an outer struct-literal
/// field infers correctly). Returns `None` when `(target, method)` is not a
/// templated generic associated fn, so the caller falls through to the
/// ordinary (non-generic) lookup.
///
/// Mirrors the free-function generic-template peek a few lines up in
/// `infer_expr_type`'s `FnCall` arm (`ctx.generic_templates.get(name)` +
/// `substitute_type_expr_to_mir`), but keyed by `(target, method)` and
/// sourced from `generic_impl_fn_templates` instead.
///
/// `receiver_ty` is `Some` for a dotted call on a VALUE receiver whose
/// static type is already known (`fs.to_box()`) -- used only when the
/// template is a `has_self` (instance-method) entry, to bind the impl's
/// generics from the receiver's concrete monomorphized type instead of from
/// `args` (which excludes `self`). `None` for a static/associated call
/// (`Box.new(v)` / `Box::new(v)`), matching the original behavior exactly.
fn infer_generic_impl_fn_ret(
    ctx: &mut LoweringContext,
    target: &str,
    method: &str,
    args: &[ast::Expr],
    receiver_ty: Option<&MirType>,
) -> Option<MirType> {
    let key = (target.to_string(), method.to_string());
    let template = ctx.generic_impl_fn_templates.get(&key)?;
    if template.has_self != receiver_ty.is_some() {
        // A `has_self` template needs a receiver type to bind from; a
        // no-`self` (associated) template must not be fed one. Mismatch
        // means this isn't the right call shape for this template.
        return None;
    }
    let generic_params = template.generic_params.clone();
    let template_params = template.params.clone();
    let template_ret_ty = template.ret_ty.clone();

    let mut type_map: HashMap<String, MirType> = HashMap::new();
    if let Some(rty) = receiver_ty {
        if let Some(self_ty) = template_params.iter().find(|p| p.name == "self").and_then(|p| p.ty.as_ref()) {
            extract_type_bindings(ctx, self_ty, rty, &generic_params, &mut type_map);
        }
    }
    let non_self_params: Vec<&ast::Param> = template_params.iter().filter(|p| p.name != "self").collect();
    for (i, param) in non_self_params.iter().enumerate() {
        if let (Some(param_ty), Some(arg)) = (&param.ty, args.get(i)) {
            extract_type_bindings_from_arg(ctx, param_ty, arg, &generic_params, &mut type_map);
        }
    }
    Some(match &template_ret_ty {
        Some(ret_ty) => substitute_type_expr_to_mir(ctx, ret_ty, &type_map),
        None => MirType::Void,
    })
}

/// Monomorphize a generic impl ASSOCIATED function (no `self` receiver, e.g.
/// `Box::new`) with concrete call-site argument expressions. Mirrors
/// `monomorphize` (the free-function version) exactly -- infers the impl's
/// type-parameter bindings from the argument expressions, substitutes them
/// into the specialized params/return, lowers ONE specialized copy per
/// distinct concrete instantiation (cached in `ctx.monomorphized` under a
/// mangled name derived from the base `{target}__{method}`), and returns the
/// mangled name for the caller to invoke.
///
/// Without this, an associated fn that constructs and returns the generic
/// struct/enum BY VALUE was lowered exactly once (see the `Decl::Impl`
/// pass): its struct-literal body always bound the impl's type params from
/// whichever concrete type reached it first (or i64 if none), so a second
/// call at a different concrete type built the WRONG monomorphized struct
/// layout -- functionally harmless on the byte-size-only Cranelift JIT, but
/// a hard LLVM AOT type mismatch (the two instantiations are distinct named
/// struct types, e.g. `Box___i64 = { i64 }` vs `Box___str = { ptr }`).
///
/// Also handles a `has_self` (instance-method) template exactly the same
/// way, EXCEPT the impl's generics are bound from `receiver_ty` (the
/// receiver's concrete monomorphized type, e.g. `Foo___str`) via
/// `extract_type_bindings` against the template's own `self: Foo<T>` param
/// type, rather than from `args` -- an instance-method call's AST `args`
/// never includes the receiver. The `self` param is a normal entry in
/// `template_params` for this shape, so it flows through the SAME
/// substitution loop as every other param below (`substitute_type_expr`),
/// specializing it to the concrete `self: Foo<str>` the same way `v: T`
/// specializes to `v: str` for an associated fn.
fn monomorphize_impl_fn(
    ctx: &mut LoweringContext,
    target: &str,
    method: &str,
    args: &[ast::Expr],
    receiver_ty: Option<&MirType>,
) -> String {
    let key = (target.to_string(), method.to_string());
    let template = ctx
        .generic_impl_fn_templates
        .get(&key)
        .expect("impl fn template exists");
    let generic_params = template.generic_params.clone();
    let template_params = template.params.clone();
    let template_ret_ty = template.ret_ty.clone();
    let template_body = template.body.clone();

    let mut type_map: HashMap<String, MirType> = HashMap::new();
    if let Some(rty) = receiver_ty {
        if let Some(self_ty) = template_params.iter().find(|p| p.name == "self").and_then(|p| p.ty.as_ref()) {
            extract_type_bindings(ctx, self_ty, rty, &generic_params, &mut type_map);
        }
    }
    let non_self_params: Vec<&ast::Param> = template_params.iter().filter(|p| p.name != "self").collect();
    for (i, param) in non_self_params.iter().enumerate() {
        if let (Some(param_ty), Some(arg_expr)) = (&param.ty, args.get(i)) {
            extract_type_bindings_from_arg(ctx, param_ty, arg_expr, &generic_params, &mut type_map);
        }
    }
    // A no-argument generic static ctor (`List.new()`) leaves its type
    // params unbound -- bind them from the enclosing annotated `let`'s
    // resolved type (matched against the template's RETURN type, which
    // extract_type_bindings demangles against mono_instance_args). Without
    // this, `let ls: List<str> = List.new()` monomorphized `List___i64`
    // while the chained `.push("a")` wanted `List___str` -- JIT printed
    // raw pointers, AOT failed the build. Consumed once; falls back to the
    // historical i64 default when there is no annotation.
    if generic_params.iter().any(|gp| !type_map.contains_key(gp)) {
        if let Some(expected) = ctx.pending_let_expected.take() {
            if let Some(ret_te) = &template_ret_ty {
                extract_type_bindings(ctx, ret_te, &expected, &generic_params, &mut type_map);
            }
        }
    }

    // Build the list of concrete types in generic_params order for the
    // mangled name, matching the free-function scheme exactly (e.g.
    // "Box__new" + [Str] -> "Box__new___str").
    let concrete_ordered: Vec<MirType> = generic_params
        .iter()
        .map(|gp| type_map.get(gp).cloned().unwrap_or(MirType::I64))
        .collect();
    let base_mangled = format!("{target}__{method}");
    let mangled = mono_mangled_name(&base_mangled, &concrete_ordered);

    // If already monomorphized, just return the name.
    if ctx.monomorphized.contains_key(&mangled) {
        return mangled;
    }
    ctx.monomorphized.insert(mangled.clone(), true);

    // Register the return type for the specialized function.
    let specialized_ret = if let Some(ret_ty) = &template_ret_ty {
        substitute_type_expr_to_mir(ctx, ret_ty, &type_map)
    } else {
        MirType::Void
    };
    ctx.func_ret_types.insert(mangled.clone(), specialized_ret);

    // Substitute type params in the parameter list.
    let specialized_params: Vec<ast::Param> = template_params
        .iter()
        .map(|p| {
            let new_ty =
                p.ty.as_ref()
                    .map(|ty_expr| substitute_type_expr(ty_expr, &type_map));
            ast::Param {
                name: p.name.clone(),
                ty: new_ty,
                default: p.default.clone(),
                span: p.span,
            }
        })
        .collect();

    let specialized_ret_ty = template_ret_ty
        .as_ref()
        .map(|ty| substitute_type_expr(ty, &type_map));

    // Set impl context (Self / current_impl_generics) for the duration of
    // lowering this specialization, matching what the original single-compile
    // `Decl::Impl` pass set while processing this impl block's methods (a
    // local `let x: T = ..` inside the body -- unsubstituted, since only
    // params/return are pre-substituted per instantiation, same as the
    // free-function template -- relies on `current_impl_generics` to erase
    // `T` to i64 via `resolve_type` rather than leak an unresolved `Struct("T")`).
    let prev_self = ctx.current_self_type.take();
    ctx.current_self_type = Some(target.to_string());
    let prev_impl_generics =
        std::mem::replace(&mut ctx.current_impl_generics, generic_params.clone());

    // Save the current function state — lower_function will call reset().
    let saved = ctx.save_function_state();
    // Expose the concrete bindings to the body lowering, exactly like the
    // free-function monomorphize does: a `let d: [T] = []` inside the body
    // must resolve to the INSTANTIATION type ([str]), or the struct literal
    // `List { data: d, .. }` field-infers T=i64 and builds a %List___i64
    // value inside List__new___str -- the sret store then emitted `store
    // %List___str undef` (AOT: corrupt-array-header panic on first use;
    // erased-to-i64 was only correct for the single-compile erased copy).
    let saved_bindings =
        std::mem::replace(&mut ctx.active_generic_bindings, type_map.clone());

    // Lower the specialized function.
    let mir_func = lower_function(
        ctx,
        &mangled,
        &specialized_params,
        &specialized_ret_ty,
        &template_body,
    );

    // Restore the caller's function AND impl state.
    ctx.active_generic_bindings = saved_bindings;
    ctx.restore_function_state(saved);
    ctx.current_self_type = prev_self;
    ctx.current_impl_generics = prev_impl_generics;

    // Store the monomorphized function for collection by lower_module.
    ctx.monomorphized_functions.push(mir_func);

    mangled
}

/// Substitute generic type parameters in a TypeExpr based on a type map.
/// Convert a concrete MirType back into a TypeExpr. Compound types
/// (Tuple/Array/Function) get REAL compound TypeExprs — `mir_type_to_name`
/// collapses them to "i64" via its catch-all, which mistyped generic
/// instantiations with tuple/array/function type args (step 202/209).
fn mir_type_to_type_expr_spanned(ty: &MirType, span: kryos_errors::Span) -> ast::TypeExpr {
    match ty {
        MirType::Tuple(elems) => ast::TypeExpr::Tuple {
            elements: elems
                .iter()
                .map(|e| mir_type_to_type_expr_spanned(e, span))
                .collect(),
            span,
        },
        MirType::Array(elem, size) => ast::TypeExpr::Array {
            element: Box::new(mir_type_to_type_expr_spanned(elem, span)),
            size: *size,
            span,
        },
        MirType::Function { params, ret } => ast::TypeExpr::Function {
            params: params
                .iter()
                .map(|p| mir_type_to_type_expr_spanned(p, span))
                .collect(),
            ret: Box::new(mir_type_to_type_expr_spanned(ret, span)),
            span,
        },
        other => ast::TypeExpr::Simple {
            name: mir_type_to_name(other),
            span,
        },
    }
}

fn substitute_type_expr(ty: &ast::TypeExpr, type_map: &HashMap<String, MirType>) -> ast::TypeExpr {
    match ty {
        ast::TypeExpr::Simple { name, span } => {
            if let Some(concrete) = type_map.get(name) {
                // Convert the concrete MirType back to a TypeExpr, keeping
                // compound shapes intact.
                mir_type_to_type_expr_spanned(concrete, *span)
            } else {
                ty.clone()
            }
        }
        // For compound types, recurse.
        ast::TypeExpr::Array {
            element,
            size,
            span,
        } => ast::TypeExpr::Array {
            element: Box::new(substitute_type_expr(element, type_map)),
            size: *size,
            span: *span,
        },
        ast::TypeExpr::Tuple { elements, span } => ast::TypeExpr::Tuple {
            elements: elements
                .iter()
                .map(|e| substitute_type_expr(e, type_map))
                .collect(),
            span: *span,
        },
        ast::TypeExpr::Function { params, ret, span } => ast::TypeExpr::Function {
            params: params
                .iter()
                .map(|p| substitute_type_expr(p, type_map))
                .collect(),
            ret: Box::new(substitute_type_expr(ret, type_map)),
            span: *span,
        },
        ast::TypeExpr::Generic { name, args, span } => {
            if let Some(concrete) = type_map.get(name) {
                let concrete_name = mir_type_to_name(concrete);
                ast::TypeExpr::Simple {
                    name: concrete_name,
                    span: *span,
                }
            } else {
                ast::TypeExpr::Generic {
                    name: name.clone(),
                    args: args
                        .iter()
                        .map(|a| substitute_type_expr(a, type_map))
                        .collect(),
                    span: *span,
                }
            }
        }
        ast::TypeExpr::Shared { inner, span } => ast::TypeExpr::Shared {
            inner: Box::new(substitute_type_expr(inner, type_map)),
            span: *span,
        },
        ast::TypeExpr::Pointer {
            inner,
            mutable,
            span,
        } => ast::TypeExpr::Pointer {
            inner: Box::new(substitute_type_expr(inner, type_map)),
            mutable: *mutable,
            span: *span,
        },
        ast::TypeExpr::Optional { inner, span } => ast::TypeExpr::Optional {
            inner: Box::new(substitute_type_expr(inner, type_map)),
            span: *span,
        },
        ast::TypeExpr::Reference {
            inner,
            mutable,
            span,
        } => ast::TypeExpr::Reference {
            inner: Box::new(substitute_type_expr(inner, type_map)),
            mutable: *mutable,
            span: *span,
        },
        ast::TypeExpr::Weak { inner, span } => ast::TypeExpr::Weak {
            inner: Box::new(substitute_type_expr(inner, type_map)),
            span: *span,
        },
        ast::TypeExpr::DynTrait { .. } => ty.clone(),
        ast::TypeExpr::Inferred { .. } => ty.clone(),
    }
}

/// Convert a MirType to a simple type name string for TypeExpr substitution.
fn mir_type_to_name(ty: &MirType) -> String {
    match ty {
        MirType::I8 => "i8".into(),
        MirType::I16 => "i16".into(),
        MirType::I32 => "i32".into(),
        MirType::I64 => "i64".into(),
        MirType::I128 => "i128".into(),
        MirType::U8 => "u8".into(),
        MirType::U16 => "u16".into(),
        MirType::U32 => "u32".into(),
        MirType::U64 => "u64".into(),
        MirType::U128 => "u128".into(),
        MirType::F32 => "f32".into(),
        MirType::F64 => "f64".into(),
        MirType::Bool => "bool".into(),
        MirType::Char => "char".into(),
        MirType::Str => "str".into(),
        MirType::Void => "void".into(),
        MirType::Struct(name) | MirType::Enum(name) => name.clone(),
        _ => "i64".into(),
    }
}
