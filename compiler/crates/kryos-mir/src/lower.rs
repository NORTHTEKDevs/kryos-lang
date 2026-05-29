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
    /// Generic struct templates: struct_name -> (generic_params, AST struct decl fields).
    generic_struct_templates: HashMap<String, GenericStructTemplate>,
    /// Generic enum templates: enum_name -> (generic_params, AST enum variants).
    generic_enum_templates: HashMap<String, GenericEnumTemplate>,
    /// Already-monomorphized specializations, to avoid duplicate lowering.
    monomorphized: HashMap<String, bool>,
    /// Functions produced by monomorphization (collected after lowering).
    monomorphized_functions: Vec<MirFunction>,
    /// Counter for anonymous lambda function names.
    lambda_counter: u32,
    /// Counter for spawn wrapper function names.
    spawn_counter: u32,
    /// Type alias map: alias_name -> resolved MirType.
    type_aliases: HashMap<String, MirType>,
    /// When inside a `try` block, holds the target local and check-block for `throw`.
    try_catch_target: Option<TryCatchTarget>,
    /// Tracks locals that are closures with captures: local_name -> (func_name, capture_operands).
    closure_locals: HashMap<String, (String, Vec<Operand>)>,
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
    /// Actor state field layouts: actor_name -> ordered list of (field_name, field_index).
    /// Each field occupies one i64 slot at offset field_index * 8.
    actor_state_fields: HashMap<String, Vec<(String, u32)>>,
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
}

/// Context passed to `throw` statements inside a `try` block.
struct TryCatchTarget {
    result_local: LocalId,
    check_block: BlockId,
}

/// Stores a generic function's AST for deferred monomorphization.
struct GenericTemplate {
    generic_params: Vec<String>,
    params: Vec<ast::Param>,
    ret_ty: Option<ast::TypeExpr>,
    body: ast::Block,
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
            generic_struct_templates: HashMap::new(),
            generic_enum_templates: HashMap::new(),
            monomorphized: HashMap::new(),
            monomorphized_functions: Vec::new(),
            lambda_counter: 0,
            spawn_counter: 0,
            type_aliases: HashMap::new(),
            try_catch_target: None,
            closure_locals: HashMap::new(),
            pending_closure_regs: Vec::new(),
            actor_defs: HashMap::new(),
            actor_state_fields: HashMap::new(),
            const_defs: HashMap::new(),
            mutable_globals: HashMap::new(),
            mutable_global_order: Vec::new(),
            func_param_types: HashMap::new(),
            dropped_locals: HashSet::new(),
            param_locals: HashSet::new(),
            borrowed_locals: HashSet::new(),
            current_ret_ty: MirType::Void,
            current_self_type: None,
            copy_structs: HashSet::new(),
            hidden_locals: HashSet::new(),
            partial_moved_locals: HashSet::new(),
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
        // Handle generic struct/enum instantiation before calling lower_type_expr.
        if let ast::TypeExpr::Generic { name, args, .. } = ty {
            let type_args: Vec<MirType> = args.iter().map(|a| self.resolve_type(a)).collect();

            // Check if this is a generic struct.
            if self.generic_struct_templates.contains_key(name) {
                return MirType::Struct(monomorphize_struct(self, name, &type_args));
            }

            // Check if this is a generic enum.
            if self.generic_enum_templates.contains_key(name) {
                return MirType::Enum(monomorphize_enum(self, name, &type_args));
            }
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
        self.current_instructions.push(inst);
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
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
    let mut ctx = LoweringContext::new();

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
        ("println", MirType::Void),
        ("print", MirType::Void),
        ("eprintln", MirType::Void),
        ("push", MirType::I64), // returns array handle
        ("pop", MirType::I64),
        ("send", MirType::Void),
        ("sleep", MirType::Void),
        ("close_chan", MirType::Void),
        ("contains", MirType::Bool),
        ("starts_with", MirType::Bool),
        ("ends_with", MirType::Bool),
        ("log", MirType::F64),
        ("char_code", MirType::I64),
        ("char_from", MirType::Str),
        ("int", MirType::I64),
        ("float", MirType::F64),
        ("keys", MirType::I64), // returns array handle
        ("map_has", MirType::Bool),
        ("map_has_str", MirType::Bool),
        ("map_delete", MirType::I64),
        ("map_delete_str", MirType::I64),
        ("map_keys", MirType::I64), // returns array handle
        ("map_keys_str", MirType::I64),
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
        ("file_exists", MirType::Bool),
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
                let mir_ret = match ret_ty {
                    Some(ty) => ctx.resolve_type(ty),
                    None => MirType::Void,
                };
                ctx.func_ret_types.insert(name.clone(), mir_ret);

                // Store parameter types for dyn Trait coercion.
                let param_types: Vec<MirType> = params
                    .iter()
                    .map(|p| {
                        p.ty.as_ref()
                            .map(|t| ctx.resolve_type(t))
                            .unwrap_or(MirType::I64)
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
                ..
            } => {
                let prev_self = ctx.current_self_type.take();
                ctx.current_self_type = Some(target.clone());

                // Register mangled method names in func_ret_types.
                for method in methods {
                    if let ast::Decl::Function { name, ret_ty, .. } = method {
                        let mangled = format!("{target}__{name}");
                        let mir_ret = match ret_ty {
                            Some(ty) => ctx.resolve_type(ty),
                            None => MirType::Void,
                        };
                        ctx.func_ret_types.insert(mangled, mir_ret);
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
                            if let ast::Decl::Function { name, ret_ty, .. } = default_method {
                                if !explicit_names.contains(name.as_str()) {
                                    let mangled = format!("{target}__{name}");
                                    let mir_ret = match ret_ty {
                                        Some(ty) => ctx.resolve_type(ty),
                                        None => MirType::Void,
                                    };
                                    ctx.func_ret_types.insert(mangled.clone(), mir_ret);
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
                    handler_info.push((handler.name.clone(), handler.params.len()));
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
                functions.push(func);
            }
            ast::Decl::Impl {
                target,
                trait_name,
                methods,
                ..
            } => {
                let prev_self = ctx.current_self_type.take();
                ctx.current_self_type = Some(target.clone());

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
                        let mut all_params = Vec::new();
                        let has_self = params.iter().any(|p| p.name == "self");
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
            }
            ast::Decl::Actor { name, handlers, .. } => {
                // Lower each message handler as a free function: ActorName__handler_name.
                for handler in handlers {
                    let mangled = format!("{name}__{}", handler.name);
                    // Prepend implicit `self` param for actor state.
                    let mut all_params = vec![ast::Param {
                        name: "self".into(),
                        ty: Some(ast::TypeExpr::Simple {
                            name: name.clone(),
                            span: kryos_errors::Span::DUMMY,
                        }),
                        default: None,
                        span: kryos_errors::Span::DUMMY,
                    }];
                    all_params.extend_from_slice(&handler.params);
                    functions.push(lower_function(
                        &mut ctx,
                        &mangled,
                        &all_params,
                        &handler.ret_ty,
                        &handler.body,
                    ));
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

    MirModule {
        functions,
        struct_defs: ctx.struct_defs,
        enum_defs: ctx.enum_defs,
        trait_vtables: ctx.impl_map,
        copy_structs: ctx.copy_structs,
    }
}

/// Lower a single AST function declaration to a `MirFunction`.
pub fn lower_function(
    ctx: &mut LoweringContext,
    name: &str,
    params: &[ast::Param],
    ret_ty: &Option<ast::TypeExpr>,
    body: &ast::Block,
) -> MirFunction {
    ctx.reset();

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
        && last_stmt.is_some_and(|s| matches!(s, ast::Stmt::If { .. }));

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

fn lower_block_stmts(ctx: &mut LoweringContext, stmts: &[ast::Stmt]) {
    let scope_start = ctx.locals.len();

    for stmt in stmts {
        lower_stmt(ctx, stmt);
    }

    // Emit drops for *named* locals declared in this scope (reverse order).
    // Unnamed temporaries (name == None) must NOT be dropped because they
    // hold non-owning copies of values (e.g. a string handle loaded from a
    // struct field).  Dropping them would free memory that the struct still
    // owns, causing heap corruption / use-after-free.
    //
    // We also skip locals that were already dropped by a nested inner scope
    // to prevent double-free.
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
                ctx.emit(Instruction::Drop { local: local_id });
                ctx.dropped_locals.insert(local_id.0);
            }
        }
    }

    // Hide scoped locals from name resolution so outer scopes don't
    // see inner bindings (prevents variable shadowing leaks like
    // `let x = 100; if true { let x = 999 } println(x)` printing 999).
    // We add them to hidden_locals rather than clearing names, so the
    // final MIR output still has names for debugging/introspection.
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
        // For any other statement kind (let, return, etc.), just lower it
        // normally — it doesn't produce a value.
        other => lower_stmt(ctx, other),
    }
}

fn lower_stmt(ctx: &mut LoweringContext, stmt: &ast::Stmt) {
    match stmt {
        ast::Stmt::Let {
            name,
            mutable,
            ty,
            value,
            pattern,
            ..
        } => {
            let mir_ty = if let Some(t) = ty {
                ctx.resolve_type(t)
            } else if let Some(expr) = value {
                // No explicit type annotation — infer from the initializer.
                infer_expr_type(ctx, expr)
            } else {
                MirType::I64
            };

            // Lower the RHS BEFORE allocating the new local.  This ensures
            // that variable name lookups in the initializer resolve to the
            // previous binding (important for `let x = f(x)` shadowing).
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
                let rvalue = lower_expr_to_rvalue(ctx, expr);
                let mark_non_owning = matches!(
                    expr,
                    ast::Expr::IndexAccess { .. } | ast::Expr::FieldAccess { .. }
                );

                // Track closures with captures for direct-call optimization.
                let closure_info = if let RValue::Closure {
                    ref func_name,
                    ref captures,
                } = rvalue
                {
                    if !captures.is_empty() {
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

            if let Some((rvalue, mark_non_owning, closure_info, is_shared)) = rvalue_and_meta {
                if mark_non_owning {
                    ctx.borrowed_locals.insert(local.0);
                }
                if let Some((func_name, captures)) = closure_info {
                    ctx.closure_locals
                        .insert(name.clone(), (func_name, captures));
                }

                // If initializer moves a non-copy local, mark it consumed.
                if let RValue::Use(Operand::Local(src)) = &rvalue {
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
                // If initializer is a call, mark non-copy args consumed.
                if let RValue::Call { ref func, ref args } = rvalue {
                    consume_call_args(ctx, local, func, args);
                }
                ctx.emit(Instruction::Assign {
                    dest: local,
                    value: rvalue,
                });
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
                        let actor_name =
                            ctx.locals
                                .iter()
                                .find(|l| l.id == self_local)
                                .and_then(|l| match &l.ty {
                                    MirType::Struct(n) => Some(n.clone()),
                                    _ => None,
                                });
                        if let Some(ref aname) = actor_name {
                            ctx.actor_state_fields
                                .get(aname)
                                .cloned()
                                .and_then(|fields| {
                                    fields
                                        .iter()
                                        .find(|(n, _)| n == field)
                                        .map(|(_, idx)| (self_local, *idx))
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
            } else {
                None
            };

            if let Some((state_ptr, field_offset)) = actor_field_target {
                // Actor state field assignment.
                match op {
                    ast::AssignOp::Assign => {
                        let val = lower_expr_to_operand(ctx, value);
                        ctx.emit(Instruction::ActorStateStore {
                            state_ptr,
                            field_offset,
                            value: val,
                        });
                    }
                    _ => {
                        // Compound assignment (+=, -=, etc.): load current, compute, store back.
                        let mir_op = match op {
                            ast::AssignOp::AddAssign => MirBinOp::Add,
                            ast::AssignOp::SubAssign => MirBinOp::Sub,
                            ast::AssignOp::MulAssign => MirBinOp::Mul,
                            ast::AssignOp::DivAssign => MirBinOp::Div,
                            ast::AssignOp::Assign => unreachable!(),
                        };
                        let current = ctx.alloc_temp(MirType::I64);
                        ctx.emit(Instruction::ActorStateLoad {
                            dest: current,
                            state_ptr,
                            field_offset,
                        });
                        let rhs = lower_expr_to_operand(ctx, value);
                        let result = ctx.alloc_temp(MirType::I64);
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
                            let rvalue = lower_expr_to_rvalue(ctx, value);
                            if let RValue::Use(Operand::Local(src)) = &rvalue {
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
                            ctx.emit(Instruction::Assign {
                                dest,
                                value: rvalue,
                            });
                        } else if let ast::Expr::IndexAccess { object, index, .. } = target {
                            // Map/array index assignment: m["key"] = value → kryos_map_insert_str(m, key, value)
                            let obj_ty = infer_expr_type(ctx, object);
                            let map_op = lower_expr_to_operand(ctx, object);
                            let key_op = lower_expr_to_operand(ctx, index);
                            let val_op = lower_expr_to_operand(ctx, value);
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
                                        args: vec![map_op, key_op, val_op],
                                    },
                                });
                            } else {
                                // Array index assignment.
                                let temp = ctx.alloc_temp(MirType::I64);
                                ctx.emit(Instruction::Assign {
                                    dest: temp,
                                    value: RValue::Call {
                                        func: "kryos_array_set".to_string(),
                                        args: vec![map_op, key_op, val_op],
                                    },
                                });
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
                            // Field assignment on a struct: store value at the
                            // field's offset within the struct pointer.
                            let obj_op = lower_expr_to_operand(ctx, object);
                            let val_op = lower_expr_to_operand(ctx, value);
                            ctx.emit(Instruction::StoreField {
                                object: obj_op,
                                field: field.clone(),
                                value: val_op,
                            });
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
                        }
                    }
                }
            }
        }

        ast::Stmt::Return { value, .. } => {
            let operand = value.as_ref().map(|e| lower_expr_to_operand(ctx, e));
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
            lower_try_catch(ctx, try_block, catch_name, catch_block);
        }

        ast::Stmt::Throw { expr, .. } => {
            let val = lower_expr_to_operand(ctx, expr);
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
                let ch_op = lower_expr_to_operand(ctx, &branch.channel);
                let ch_local = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: ch_local,
                    value: RValue::Use(ch_op),
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
                lower_for_range(ctx, pattern, &args[0], &args[1], body);
                return;
            }
        }
    }

    // Check if the iterable is a range expression (start..end or start..=end).
    if let ast::Expr::RangeExpr { start, end, .. } = iterable {
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
        lower_for_range(ctx, pattern, s, e, body);
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
) {
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

    // Header: _idx < end
    let cond_temp = ctx.alloc_temp(MirType::Bool);
    ctx.emit(Instruction::Assign {
        dest: cond_temp,
        value: RValue::BinOp {
            op: MirBinOp::Lt,
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
    let spawn_name = format!("__spawn_{}", ctx.spawn_counter);
    ctx.spawn_counter += 1;

    // Find captured variables from enclosing scope.
    let captures = find_free_variables_block(ctx, stmts);

    // Build the capture operands before we save state (need access to current locals).
    let capture_ops: Vec<Operand> = captures
        .iter()
        .map(|name| {
            let local =
                find_local_by_name(ctx, name).expect("internal: spawn capture local not found");
            Operand::Local(local)
        })
        .collect();

    // Save current function state and lower the spawn body as a new function.
    let saved = ctx.save_function_state();

    // Build params from captures.
    let params: Vec<ast::Param> = captures
        .iter()
        .map(|name| ast::Param {
            name: name.clone(),
            ty: None,
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
) {
    let result_local = ctx.alloc_temp(MirType::Enum("Result".into()));

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
    ctx.emit(Instruction::Drop {
        local: result_local,
    });
    ctx.finish_block(Terminator::Goto(merge_bb), err_bb);

    // Err path: bind error value to catch_name, execute handler.
    let err_payload = ctx.alloc_local(Some(catch_name.to_string()), MirType::I64, false);
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
    ctx.emit(Instruction::Drop {
        local: result_local,
    });
    lower_block_stmts(ctx, &catch_block.stmts);
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

fn lower_match(ctx: &mut LoweringContext, subject: &ast::Expr, arms: &[ast::MatchArm]) -> Operand {
    let subj_op = lower_expr_to_operand(ctx, subject);
    // Infer the result type from the first arm's body expression.
    // For enum arms where the body is a simple identifier that will be bound
    // from a field extraction, look up the field type directly -- the local
    // does not exist yet when infer_expr_type runs, so it would fall back to
    // I64 even for f64 fields (e.g. `JsonValue::Number(n) => n`).
    let result_ty = arms
        .first()
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
                    if let Some(variants) = ctx.enum_defs.get(enum_name.as_str()) {
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
            infer_expr_type(ctx, &arm.body)
        })
        .unwrap_or(MirType::I64);
    let result_local = ctx.alloc_temp(result_ty);
    let merge_bb = ctx.alloc_block();

    // Detect enum match: either explicit Pattern::Enum, or bare ident patterns
    // where the subject type is an enum (e.g., `match c { Red => ... }`).
    let subj_ty = infer_expr_type(ctx, subject);
    let subj_enum_name = match &subj_ty {
        MirType::Enum(name) => Some(name.clone()),
        _ => None,
    };
    let is_enum_match = arms
        .iter()
        .any(|a| matches!(&a.pattern, ast::Pattern::Enum { .. }))
        || (subj_enum_name.is_some()
            && arms
                .iter()
                .any(|a| matches!(&a.pattern, ast::Pattern::Ident { .. })));

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
    let mut arm_blocks: Vec<(BlockId, &ast::Expr, Option<EnumBinding>)> = Vec::new();
    let mut default_arm: Option<(BlockId, &ast::Expr)> = None;

    for arm in arms {
        let arm_bb = ctx.alloc_block();
        match &arm.pattern {
            ast::Pattern::Enum {
                name,
                variant,
                fields,
                ..
            } => {
                if let Some(variants) = ctx.enum_defs.get(name.as_str()) {
                    if let Some(idx) = variants.iter().position(|v| v.name == *variant) {
                        targets.push((idx as i64, arm_bb));
                        arm_blocks.push((
                            arm_bb,
                            &arm.body,
                            Some(EnumBinding {
                                enum_name: name.clone(),
                                variant_idx: idx as u32,
                                field_patterns: fields.clone(),
                            }),
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
                    arm_blocks.push((arm_bb, &arm.body, None));
                } else if let ast::Expr::StringLiteral { value, .. } = expr.as_ref() {
                    string_targets.push((value.clone(), arm_bb));
                    arm_blocks.push((arm_bb, &arm.body, None));
                } else if let ast::Expr::BoolLiteral { value, .. } = expr.as_ref() {
                    // Bool patterns: compile as integer switch where true=1, false=0.
                    // The subject is already i8 (Cranelift's bool repr); the codegen's
                    // Switch terminator sizes the case constants to the subject's type.
                    targets.push((if *value { 1 } else { 0 }, arm_bb));
                    arm_blocks.push((arm_bb, &arm.body, None));
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
                            targets.push((idx as i64, arm_bb));
                            arm_blocks.push((arm_bb, &arm.body, None));
                            matched = true;
                        }
                    }
                }
                if !matched {
                    default_arm = Some((arm_bb, &arm.body));
                }
            }
            ast::Pattern::Wildcard { .. } => {
                default_arm = Some((arm_bb, &arm.body));
            }
            _ => {
                default_arm = Some((arm_bb, &arm.body));
            }
        }
    }

    let default_bb = default_arm.map(|(bb, _)| bb).unwrap_or(merge_bb);

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
                    .map(|(bb, _, _)| *bb)
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
    } else {
        ctx.finish_block(
            Terminator::Switch {
                value: switch_op,
                targets,
                default: default_bb,
            },
            if let Some((bb, _, _)) = arm_blocks.first() {
                *bb
            } else {
                default_bb
            },
        );
    }

    // Emit each arm block.
    for (i, (arm_bb, body, enum_binding)) in arm_blocks.iter().enumerate() {
        if i > 0 {
            ctx.current_block = *arm_bb;
        }

        // For enum arms, extract payload fields and bind to locals.
        if let Some(binding) = enum_binding {
            for (field_idx, pat) in binding.field_patterns.iter().enumerate() {
                if let ast::Pattern::Ident { name, .. } = pat {
                    // Look up the actual field type from enum_defs so
                    // the local has the correct type (e.g. f64 not i64).
                    let field_type = ctx
                        .enum_defs
                        .get(binding.enum_name.as_str())
                        .and_then(|variants| variants.get(binding.variant_idx as usize))
                        .and_then(|variant| variant.fields.get(field_idx))
                        .cloned()
                        .unwrap_or(MirType::I64);
                    let local = ctx.alloc_local(Some(name.clone()), field_type.clone(), false);
                    // Pre-mark non-copy payload bindings as consumed: they will be
                    // moved into the arm result, not dropped by scope cleanup.
                    if !is_copy_type(ctx, &field_type) {
                        ctx.dropped_locals.insert(local.0);
                    }
                    ctx.emit(Instruction::Assign {
                        dest: local,
                        value: RValue::EnumPayload {
                            operand: subj_op.clone(),
                            enum_name: binding.enum_name.clone(),
                            variant_idx: binding.variant_idx,
                            field_idx: field_idx as u32,
                        },
                    });
                }
                // Wildcard patterns — skip, no binding needed.
            }
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
fn infer_branch_value_type(ctx: &mut LoweringContext, block: &ast::Block) -> Option<MirType> {
    let last = block.stmts.last()?;
    match last {
        ast::Stmt::Expr { expr, .. } => Some(infer_expr_type(ctx, expr)),
        ast::Stmt::Return { value: Some(expr), .. } => Some(infer_expr_type(ctx, expr)),
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
                _ => MirType::I64,
            }
        }

        ast::Expr::FieldAccess { object, field, .. } => {
            // Resolve the object's type, then look up the field in struct_defs.
            let obj_ty = infer_expr_type(ctx, object);
            // Auto-deref: if the object is a reference to a struct, dereference first.
            let resolved_ty = match &obj_ty {
                MirType::Ref { inner, .. } => inner.as_ref().clone(),
                other => other.clone(),
            };
            if let MirType::Struct(name) = &resolved_ty {
                if let Some(fields) = ctx.struct_defs.get(name) {
                    if let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == field.as_str()) {
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
                _ => lty,
            }
        }

        ast::Expr::UnaryOp { operand, .. } => infer_expr_type(ctx, operand),

        ast::Expr::FnCall { callee, args, .. } => {
            // If the callee is a simple identifier, look up the return type.
            if let ast::Expr::Identifier { name, .. } = callee.as_ref() {
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
                    // declared TypeExpr against the inferred concrete argument
                    // type. Handles `[T]`, `(A, B)`, `fn(T) -> U`, `&T`, etc.
                    let mut type_map: HashMap<String, MirType> = HashMap::new();
                    for (i, param) in template_params.iter().enumerate() {
                        if let (Some(param_ty), Some(arg)) = (&param.ty, args.get(i)) {
                            let arg_ty = infer_expr_type(ctx, arg);
                            extract_type_bindings(
                                param_ty,
                                &arg_ty,
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
            MirType::Void
        }

        ast::Expr::MethodCall { object, method, .. } => {
            // Check if this is an enum variant constructor.
            if let ast::Expr::Identifier { name, .. } = object.as_ref() {
                if ctx.enum_defs.contains_key(name.as_str()) {
                    return MirType::Enum(name.clone());
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
            if let Some(type_name) = infer_type_name(ctx, object) {
                let mangled = format!("{type_name}__{method}");
                if let Some(ret_ty) = ctx.func_ret_types.get(&mangled) {
                    return ret_ty.clone();
                }
            }
            if let Some(ret_ty) = ctx.func_ret_types.get(method.as_str()) {
                return ret_ty.clone();
            }
            // Function-typed struct field: e.g. t.transform(5) where
            // `transform: fn(i64) -> i64`. Return the closure's declared ret ty.
            if let Some(type_name) = infer_type_name(ctx, object) {
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
            type_name, method, ..
        } => {
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

        ast::Expr::Lambda { ret_ty, .. } => {
            // A lambda expression's type is Function.
            MirType::Function {
                params: vec![MirType::I64], // simplified
                ret: Box::new(match ret_ty {
                    Some(ty) => ctx.resolve_type(ty),
                    None => MirType::I64,
                }),
            }
        }

        ast::Expr::PipeExpr { right, .. } => {
            // The pipe result type is the return type of the RHS function.
            infer_expr_type(ctx, right)
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

        ast::Expr::MatchExpr { arms, .. } => {
            // Infer the type from the first arm's body expression.
            arms.first()
                .map(|arm| infer_expr_type(ctx, &arm.body))
                .unwrap_or(MirType::I64)
        }

        ast::Expr::IfExpr { then_branch, .. } => {
            // Infer from the last expression of the then branch.
            then_branch
                .stmts
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

        _ => MirType::Void,
    }
}

// ---------------------------------------------------------------------------
// Expression lowering
// ---------------------------------------------------------------------------

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

            let lhs = lower_expr_to_operand(ctx, left);
            let rhs = lower_expr_to_operand(ctx, right);
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
                _ => "<closure>".to_string(),
            };

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
                    // Initialize each state field to its default value (0).
                    // Clone the field layout to avoid borrow conflict with ctx.
                    let fields = ctx
                        .actor_state_fields
                        .get(&func_name)
                        .cloned()
                        .unwrap_or_default();
                    for (_field_name, field_idx) in &fields {
                        ctx.emit(Instruction::ActorStateStore {
                            state_ptr,
                            field_offset: *field_idx,
                            value: Operand::Constant(Constant::Int(0)),
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
                return RValue::EnumVariant {
                    enum_name,
                    variant_idx,
                    fields: mir_args,
                };
            }

            // Check if this is a call to a generic function — monomorphize.
            if ctx.generic_templates.contains_key(&func_name) {
                let arg_types: Vec<MirType> =
                    args.iter().map(|a| infer_expr_type(ctx, a)).collect();
                let mangled = monomorphize(ctx, &func_name, &arg_types);
                let mir_args: Vec<Operand> =
                    args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
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
                    .map(|l| matches!(l.ty, MirType::Function { .. }))
                    .unwrap_or(false);
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
                        return RValue::EnumVariant {
                            enum_name: name.clone(),
                            variant_idx: idx as u32,
                            fields,
                        };
                    }
                }

                // Static method call via dot syntax: `TypeName.method(args)`.
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
                        let obj = lower_expr_to_operand(ctx, object);
                        let mut call_args: Vec<Operand> = Vec::new();
                        for a in args {
                            call_args.push(lower_expr_to_operand(ctx, a));
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

            let obj = lower_expr_to_operand(ctx, object);
            let mut mir_args: Vec<Operand> = vec![obj];
            for a in args {
                mir_args.push(lower_expr_to_operand(ctx, a));
            }

            // Resolve mangled method name: infer the object's type and look up
            // the impl method as TypeName__method.
            let func_name = if let Some(tn) = type_name {
                ctx.method_owners
                    .get(&(tn.clone(), method.clone()))
                    .cloned()
                    .unwrap_or_else(|| method.clone())
            } else {
                method.clone()
            };

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
            let mir_args: Vec<Operand> =
                args.iter().map(|a| lower_expr_to_operand(ctx, a)).collect();
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
            RValue::Call {
                func: func_name,
                args: mir_args,
            }
        }

        ast::Expr::ArrayLiteral { elements, .. } => {
            let ops: Vec<Operand> = elements
                .iter()
                .map(|e| lower_expr_to_operand(ctx, e))
                .collect();
            RValue::Array(ops)
        }

        ast::Expr::TupleLiteral { elements, .. } => {
            let ops: Vec<Operand> = elements
                .iter()
                .map(|e| lower_expr_to_operand(ctx, e))
                .collect();
            RValue::Tuple(ops)
        }

        ast::Expr::StructLiteral { name, fields, .. } => {
            let effective_name = resolve_struct_literal_name(ctx, name, fields);
            let mir_fields: Vec<(String, Operand)> = fields
                .iter()
                .map(|(n, e)| (n.clone(), lower_expr_to_operand(ctx, e)))
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
                        });
                    if let Some(ref aname) = actor_name {
                        if let Some(fields) = ctx.actor_state_fields.get(aname).cloned() {
                            if let Some((_fname, field_idx)) =
                                fields.iter().find(|(n, _)| n == field)
                            {
                                let dest = ctx.alloc_temp(MirType::I64);
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

            // Auto-deref: if the object is a reference, dereference it first.
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
            if matches!(obj_ty, MirType::Map { .. }) {
                let idx_ty = infer_expr_type(ctx, index);
                let get_fn = if idx_ty == MirType::Str {
                    "kryos_map_get_str"
                } else {
                    "kryos_map_get"
                };
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

        ast::Expr::Block { block, .. } => {
            // Lower all statements, return last expression.
            for (i, stmt) in block.stmts.iter().enumerate() {
                if i == block.stmts.len() - 1 {
                    if let ast::Stmt::Expr { expr, .. } = stmt {
                        return lower_expr_to_rvalue(ctx, expr);
                    }
                }
                lower_stmt(ctx, stmt);
            }
            RValue::ConstNone
        }

        ast::Expr::Lambda {
            params,
            ret_ty,
            body,
            ..
        } => {
            // Create an anonymous function name.
            let lambda_name = format!("__lambda_{}", ctx.lambda_counter);
            ctx.lambda_counter += 1;

            // Analyze free variables in the lambda body (captures from enclosing scope).
            let captures = find_free_variables(ctx, body, params);

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
            all_params.extend_from_slice(params);

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
                        let trailing_is_expr =
                            matches!(block.stmts.last(), Some(ast::Stmt::Expr { .. }));
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

            let mir_func =
                lower_function(ctx, &lambda_name, &all_params, effective_ret, &body_block);
            ctx.restore_function_state(saved);
            ctx.monomorphized_functions.push(mir_func);

            // Register the lambda's return type.
            let mir_ret = match ret_ty {
                Some(ty) => ctx.resolve_type(ty),
                None if body_is_void_call => MirType::Void,
                None => MirType::I64,
            };
            ctx.func_ret_types.insert(lambda_name.clone(), mir_ret);

            // Emit the closure RValue with captured variable operands.
            let capture_ops: Vec<Operand> = captures
                .iter()
                .map(|name| {
                    let local = find_local_by_name(ctx, name)
                        .expect("internal: lambda capture local not found");
                    Operand::Local(local)
                })
                .collect();

            RValue::Closure {
                func_name: lambda_name,
                captures: capture_ops,
            }
        }

        ast::Expr::PipeExpr { left, right, .. } => {
            // Desugar: `a |> f` → `f(a)`
            // Desugar: `a |> f(b, c)` → `f(a, b, c)`
            let lhs_op = lower_expr_to_operand(ctx, left);
            match right.as_ref() {
                ast::Expr::FnCall {
                    callee,
                    args,
                    span: _,
                } => {
                    // `a |> f(b, c)` → `f(a, b, c)`
                    let func_name = match callee.as_ref() {
                        ast::Expr::Identifier { name, .. } => name.clone(),
                        _ => "<pipe_target>".to_string(),
                    };
                    let mut all_args = vec![lhs_op];
                    for a in args {
                        all_args.push(lower_expr_to_operand(ctx, a));
                    }
                    RValue::Call {
                        func: func_name,
                        args: all_args,
                    }
                }
                ast::Expr::Identifier { name, .. } => {
                    // `a |> f` → `f(a)`
                    RValue::Call {
                        func: name.clone(),
                        args: vec![lhs_op],
                    }
                }
                _ => {
                    // Fallback: try to evaluate RHS as a function.
                    let rhs_op = lower_expr_to_operand(ctx, right);
                    RValue::Call {
                        func: "<pipe_target>".to_string(),
                        args: vec![lhs_op, rhs_op],
                    }
                }
            }
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
                    ast::StringPart::Expr(e) => lower_expr_to_operand(ctx, e),
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

        // Await — for MVP, lower as a direct call (no coroutine transform).
        ast::Expr::Await { value, .. } => lower_expr_to_rvalue(ctx, value),
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
            "f32" => MirType::F32,
            "f64" => MirType::F64,
            "bool" => MirType::Bool,
            "char" => MirType::Char,
            "str" | "string" | "String" => MirType::Str,
            "void" => MirType::Void,
            // The `!` (never) type denotes a function that never returns
            // normally (it diverges via exit/throw/loop). At the MIR level
            // we represent it as Void so the ABI matches `() -> ()`.
            "never" => MirType::Void,
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
            if name == "Map" && args.len() == 2 {
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
    match infer_expr_type(ctx, expr) {
        MirType::Struct(name) | MirType::Enum(name) => Some(name),
        _ => None,
    }
}

/// Check if `name` is an enum variant. Returns (enum_name, variant_index) if found.
fn find_enum_variant(ctx: &LoweringContext, name: &str) -> Option<(String, u32)> {
    for (enum_name, variants) in &ctx.enum_defs {
        for (idx, v) in variants.iter().enumerate() {
            if v.name == name {
                return Some((enum_name.clone(), idx as u32));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Lambda / Closure helpers
// ---------------------------------------------------------------------------

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
        ast::Expr::ComptimeBlock { body, .. } | ast::Expr::QuantumBlock { body, .. } => {
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

    // Pre-allocate block IDs.
    let bb_poll = alloc_block(); // bb0
    let bb_switch = alloc_block(); // bb1
    let bb_exit = alloc_block(); // bb2

    // Allocate handler blocks (one per handler).
    let handler_blocks: Vec<BlockId> = handlers.iter().map(|_| alloc_block()).collect();

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

    // Handler blocks: recv args, call handler, goto poll
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

        blocks.push(BasicBlock {
            id: handler_blocks[i],
            instructions,
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
    param_ty: &ast::TypeExpr,
    concrete: &MirType,
    generic_params: &[String],
    out: &mut HashMap<String, MirType>,
) {
    match (param_ty, concrete) {
        (ast::TypeExpr::Simple { name, .. }, c) => {
            if generic_params.iter().any(|gp| gp == name) {
                out.entry(name.clone()).or_insert_with(|| c.clone());
            }
        }
        (ast::TypeExpr::Array { element, .. }, MirType::Array(elem_ty, _)) => {
            extract_type_bindings(element, elem_ty, generic_params, out);
        }
        (ast::TypeExpr::Tuple { elements, .. }, MirType::Tuple(c_elems)) => {
            for (pe, ce) in elements.iter().zip(c_elems.iter()) {
                extract_type_bindings(pe, ce, generic_params, out);
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
                extract_type_bindings(pe, ce, generic_params, out);
            }
            extract_type_bindings(ret, c_ret, generic_params, out);
        }
        (ast::TypeExpr::Reference { inner, .. }, MirType::Ref { inner: c_inner, .. }) => {
            extract_type_bindings(inner, c_inner, generic_params, out);
        }
        (ast::TypeExpr::Pointer { inner, .. }, MirType::Ptr(c_inner)) => {
            extract_type_bindings(inner, c_inner, generic_params, out);
        }
        (ast::TypeExpr::Shared { inner, .. }, MirType::Shared(c_inner)) => {
            extract_type_bindings(inner, c_inner, generic_params, out);
        }
        // Optional<T> lowers to Struct("Option") -- we can't recover T from it,
        // so just skip. Generic structs / enums likewise can't be recovered
        // from a bare MirType::Struct(name) without auxiliary tracking.
        _ => {}
    }
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
            ctx.resolve_type(ty)
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
        _ => ctx.resolve_type(ty),
    }
}

/// Produce a mangled name for a monomorphized specialization.
/// e.g., `id` with `[I64]` → `id___i64`.
fn mono_mangled_name(base: &str, concrete_types: &[MirType]) -> String {
    let suffix: Vec<String> = concrete_types.iter().map(|t| format!("{t}")).collect();
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
    let type_args: Vec<MirType> = template
        .generic_params
        .iter()
        .map(|gp| {
            template
                .fields
                .iter()
                .find(|f| type_expr_mentions_param(&f.ty, gp))
                .and_then(|f| {
                    field_exprs
                        .iter()
                        .find(|(fn_, _)| fn_ == &f.name)
                        .map(|(_, fexpr)| infer_expr_type(ctx, fexpr))
                })
                .unwrap_or(MirType::I64)
        })
        .collect();
    monomorphize_struct(ctx, name, &type_args)
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

    // If already monomorphized, just return the name.
    if ctx.struct_defs.contains_key(&mangled) {
        return mangled;
    }

    // Substitute type parameters in the field types.
    let field_list: Vec<(String, MirType)> = fields
        .iter()
        .map(|f| {
            let substituted = substitute_type_expr(&f.ty, &type_map);
            (f.name.clone(), ctx.resolve_type(&substituted))
        })
        .collect();

    // Insert the monomorphized struct definition.
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

    // If already monomorphized, just return the name.
    if ctx.enum_defs.contains_key(&mangled) {
        return mangled;
    }

    // Substitute type parameters in the variant field types.
    let variant_defs: Vec<EnumVariantDef> = variants
        .iter()
        .map(|v| EnumVariantDef {
            name: v.name.clone(),
            fields: v
                .fields
                .iter()
                .map(|f| {
                    let substituted = substitute_type_expr(f, &type_map);
                    ctx.resolve_type(&substituted)
                })
                .collect(),
        })
        .collect();

    // Insert the monomorphized enum definition.
    ctx.enum_defs.insert(mangled.clone(), variant_defs);

    mangled
}

/// Monomorphize a generic function template with concrete argument types.
///
/// Infers type parameter bindings from argument types, substitutes them
/// in the parameter/return type annotations, lowers the specialized copy,
/// and returns the mangled name.
fn monomorphize(ctx: &mut LoweringContext, func_name: &str, arg_types: &[MirType]) -> String {
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
        if let (Some(param_ty), Some(concrete)) = (&param.ty, arg_types.get(i)) {
            extract_type_bindings(param_ty, concrete, &generic_params, &mut type_map);
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

    // Lower the specialized function.
    let mir_func = lower_function(
        ctx,
        &mangled,
        &specialized_params,
        &specialized_ret_ty,
        &template_body,
    );

    // Restore the caller's function state.
    ctx.restore_function_state(saved);

    // Store the monomorphized function for collection by lower_module.
    ctx.monomorphized_functions.push(mir_func);

    mangled
}

/// Substitute generic type parameters in a TypeExpr based on a type map.
fn substitute_type_expr(ty: &ast::TypeExpr, type_map: &HashMap<String, MirType>) -> ast::TypeExpr {
    match ty {
        ast::TypeExpr::Simple { name, span } => {
            if let Some(concrete) = type_map.get(name) {
                // Convert MirType back to a Simple TypeExpr name.
                let concrete_name = mir_type_to_name(concrete);
                ast::TypeExpr::Simple {
                    name: concrete_name,
                    span: *span,
                }
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
