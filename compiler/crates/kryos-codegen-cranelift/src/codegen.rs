//! AOT compilation: MIR -> Cranelift IR -> object file bytes.

use std::collections::{HashMap, HashSet};

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    types, AbiParam, Function, InstBuilder, MemFlags, Signature, StackSlotData, StackSlotKind,
    Type, UserFuncName,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use kryos_mir::ir::{
    BasicBlock, Constant, EnumVariantDef, Instruction, LocalId, MirBinOp, MirFunction, MirModule,
    MirType, MirUnOp, Operand, RValue, Terminator,
};

use crate::CodegenError;

// ---------------------------------------------------------------------------
// Codegen options
// ---------------------------------------------------------------------------

/// Options controlling code generation behavior.
#[derive(Debug, Clone, Default)]
pub struct CodegenOptions {
    /// Emit overflow checks for integer add/sub/mul.
    pub checked_arithmetic: bool,
}

// ---------------------------------------------------------------------------
// Type mapping
// ---------------------------------------------------------------------------

/// Convert a MIR type to a Cranelift IR type.
///
/// Compound types (arrays, tuples, structs) and strings are lowered to
/// pointer-sized integers (addresses) because Cranelift is a scalar
/// register-oriented IR — aggregate layout lives in the runtime / linker.
pub fn mir_type_to_cl(ty: &MirType) -> Result<Option<Type>, CodegenError> {
    match ty {
        MirType::I8 | MirType::U8 => Ok(Some(types::I8)),
        MirType::I16 | MirType::U16 => Ok(Some(types::I16)),
        MirType::I32 | MirType::U32 => Ok(Some(types::I32)),
        MirType::I64 | MirType::U64 => Ok(Some(types::I64)),
        MirType::I128 | MirType::U128 => Ok(Some(types::I128)),
        MirType::F32 => Ok(Some(types::F32)),
        MirType::F64 => Ok(Some(types::F64)),
        MirType::Bool => Ok(Some(types::I8)),
        MirType::Char => Ok(Some(types::I32)), // Unicode scalar value
        MirType::Str => Ok(Some(types::I64)),  // pointer to string data
        MirType::Void => Ok(None),
        MirType::Ptr(_) | MirType::Ref { .. } | MirType::Shared(_) => Ok(Some(types::I64)), // pointer
        MirType::Array(_, _)
        | MirType::Tuple(_)
        | MirType::Struct(_)
        | MirType::Enum(_)
        | MirType::Map { .. } => {
            Ok(Some(types::I64)) // pointer to heap/stack allocation
        }
        MirType::Function { .. } => Ok(Some(types::I64)), // function pointer
        MirType::DynTrait(_) => Ok(Some(types::I64)), // fat pointer (packed as i64 pair, or data ptr)
    }
}

/// Get the Cranelift type for a MirType, returning I64 for Void (used in
/// contexts where we must have a concrete type, like local variables that
/// hold call results before we know they're void).
fn mir_type_to_cl_or_i64(ty: &MirType) -> Result<Type, CodegenError> {
    Ok(mir_type_to_cl(ty)?.unwrap_or(types::I64))
}

/// Infer a Cranelift type from a MIR operand when we don't have explicit type info.
fn type_of_operand_hint(operand: &Operand, locals: &[kryos_mir::ir::MirLocal]) -> Type {
    match operand {
        Operand::Local(id) => {
            if let Some(local) = locals.iter().find(|l| l.id == *id) {
                mir_type_to_cl(&local.ty)
                    .ok()
                    .flatten()
                    .unwrap_or(types::I64)
            } else {
                types::I64
            }
        }
        Operand::Constant(c) => match c {
            Constant::Int(_) => types::I64,
            Constant::Float(_) => types::F64,
            Constant::Bool(_) => types::I8,
            Constant::Str(_) => types::I64,
            Constant::None => types::I64,
        },
    }
}

/// Returns true if a type is a floating-point type.
fn is_float_type(ty: Type) -> bool {
    ty == types::F32 || ty == types::F64
}

/// Returns true if the MIR operand has string type.
fn is_string_operand(operand: &Operand, locals: &[kryos_mir::ir::MirLocal]) -> bool {
    match operand {
        Operand::Local(id) => locals
            .iter()
            .find(|l| l.id == *id)
            .is_some_and(|l| l.ty == kryos_mir::ir::MirType::Str),
        Operand::Constant(Constant::Str(_)) => true,
        _ => false,
    }
}

/// Returns true if the MIR operand has bool type.
fn is_bool_operand(operand: &Operand, locals: &[kryos_mir::ir::MirLocal]) -> bool {
    match operand {
        Operand::Local(id) => locals
            .iter()
            .find(|l| l.id == *id)
            .is_some_and(|l| l.ty == kryos_mir::ir::MirType::Bool),
        Operand::Constant(Constant::Bool(_)) => true,
        _ => false,
    }
}

/// Returns true if the MIR operand has a float type.
fn is_float_operand(operand: &Operand, locals: &[kryos_mir::ir::MirLocal]) -> bool {
    match operand {
        Operand::Local(id) => locals.iter().find(|l| l.id == *id).is_some_and(|l| {
            matches!(
                l.ty,
                kryos_mir::ir::MirType::F32 | kryos_mir::ir::MirType::F64
            )
        }),
        Operand::Constant(Constant::Float(_)) => true,
        _ => false,
    }
}

/// Returns the type name string for a MIR operand, used by `type_of()`.
/// Covers all MIR types so that `type_of` can be fully resolved at compile time.
fn mir_type_name_of_operand(operand: &Operand, locals: &[kryos_mir::ir::MirLocal]) -> &'static str {
    match operand {
        Operand::Local(id) => {
            if let Some(local) = locals.iter().find(|l| l.id == *id) {
                mir_type_name(&local.ty)
            } else {
                "i64"
            }
        }
        Operand::Constant(c) => match c {
            Constant::Int(_) => "i64",
            Constant::Float(_) => "f64",
            Constant::Bool(_) => "bool",
            Constant::Str(_) => "str",
            Constant::None => "void",
        },
    }
}

/// Maps a MIR type to its Kryos type name string.
fn mir_type_name(ty: &kryos_mir::ir::MirType) -> &'static str {
    use kryos_mir::ir::MirType;
    match ty {
        MirType::I8 => "i8",
        MirType::I16 => "i16",
        MirType::I32 => "i32",
        MirType::I64 => "i64",
        MirType::I128 => "i128",
        MirType::U8 => "u8",
        MirType::U16 => "u16",
        MirType::U32 => "u32",
        MirType::U64 => "u64",
        MirType::U128 => "u128",
        MirType::F32 => "f32",
        MirType::F64 => "f64",
        MirType::Bool => "bool",
        MirType::Char => "char",
        MirType::Str => "str",
        MirType::Void => "void",
        MirType::Ptr(_) => "ptr",
        MirType::Ref { .. } => "ref",
        MirType::Shared(_) => "shared",
        MirType::Array(_, _) => "array",
        MirType::Tuple(_) => "tuple",
        MirType::Struct(_) => "struct",
        MirType::Enum(_) => "enum",
        MirType::Function { .. } => "fn",
        MirType::DynTrait(_) => "dyn",
        MirType::Map { .. } => "map",
    }
}

// ---------------------------------------------------------------------------
// Struct layout computation
// ---------------------------------------------------------------------------

/// Computed memory layout for a struct type.
struct StructLayout {
    /// Total size of the struct in bytes.
    total_size: u32,
    /// Per-field: (field_name, byte_offset, cranelift_type).
    field_offsets: Vec<(String, u32, Type)>,
}

/// Compute the memory layout for a struct given its ordered field definitions.
/// Fields are naturally aligned (aligned to their own size).
fn compute_struct_layout(fields: &[(String, MirType)]) -> Result<StructLayout, CodegenError> {
    let mut offset = 0u32;
    let mut field_offsets = Vec::new();
    for (name, ty) in fields {
        let cl_ty = mir_type_to_cl(ty)?.unwrap_or(types::I64);
        let size = cl_ty.bytes();
        // Natural alignment: align to the field's own size.
        let align = size;
        if align > 0 {
            offset = (offset + align - 1) & !(align - 1);
        }
        field_offsets.push((name.clone(), offset, cl_ty));
        offset += size;
    }
    Ok(StructLayout {
        total_size: offset,
        field_offsets,
    })
}

// ---------------------------------------------------------------------------
// AOT entry point
// ---------------------------------------------------------------------------

/// Compile a MIR module into object file bytes (ELF / COFF / Mach-O).
pub fn compile_module(module: &MirModule) -> Result<Vec<u8>, CodegenError> {
    compile_module_with_options(module, &CodegenOptions::default())
}

/// Compile a MIR module with explicit codegen options.
pub fn compile_module_with_options(
    module: &MirModule,
    options: &CodegenOptions,
) -> Result<Vec<u8>, CodegenError> {
    // Build ISA for the host.
    let mut flag_builder = settings::builder();
    flag_builder
        .set("opt_level", "speed")
        .map_err(|e| CodegenError::Target(e.to_string()))?;
    flag_builder
        .set("is_pic", "true")
        .map_err(|e| CodegenError::Target(e.to_string()))?;
    // Enable Cranelift's IR verifier so any malformed function emitted by
    // the MIR-to-CL translation is caught at compile time rather than
    // producing silently-wrong machine code. Cheap relative to AOT compile
    // time and catches the class of bugs that produce nondeterministic
    // stage-1 segfaults.
    let _ = flag_builder.set("enable_verifier", "true");

    let isa_builder =
        cranelift_native::builder().map_err(|e| CodegenError::Target(e.to_string()))?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError::Target(e.to_string()))?;

    let mut object_module = ObjectModule::new(
        ObjectBuilder::new(
            isa,
            "kryos_module",
            cranelift_module::default_libcall_names(),
        )
        .map_err(|e: cranelift_module::ModuleError| CodegenError::Module(e))?,
    );

    let mut fb_ctx = FunctionBuilderContext::new();

    // First pass: declare all functions so we can reference them.
    //
    // Special handling for `main`: if the user's main returns void, we rename
    // it to `_kryos_main` and later emit a C-compatible `main` wrapper that
    // calls it and returns 0.  This satisfies the linker expectation that
    // `main` has signature `() -> i32`.
    let mut func_ids: HashMap<String, FuncId> = HashMap::new();
    let mut needs_main_wrapper = false;

    // Names of user-defined Kryos functions (including those merged in from
    // imported modules). When a user-defined function shadows a builtin name
    // (e.g. std::io defines `fn print(msg: str) -> i64`), we skip declaring
    // the C-level builtin import for that name so the user fn wins.
    let user_func_names: HashSet<String> = module
        .functions
        .iter()
        .map(|f| f.name.clone())
        .collect();

    for mir_func in &module.functions {
        let sig = build_signature(mir_func, object_module.isa().default_call_conv());

        if mir_func.name == "main" {
            // Declare the user's main under an internal name. The exported
            // `int main()` C-runtime entry point is the wrapper synthesised
            // below — it calls `_kryos_main`, ignores any user return value,
            // runs `kryos_spawn_wait_all`, and returns `0i32`.
            //
            // Both `fn main()` (void) and `fn main() -> i64` route through
            // the same wrapper. Previously only the void form did, and a
            // user `fn main() -> i64` ended up declared as `Linkage::Local`
            // with the bare name `main`, which collided with the C
            // runtime's expected entry-point symbol at link time:
            //   error LNK2019: unresolved external symbol main
            // (the test build_cache_roundtrip_with_cli in v2.8.0 used
            //  `fn main() -> i64 { return 7 }` and exercised this path).
            let func_id = object_module.declare_function("_kryos_main", Linkage::Local, &sig)?;
            func_ids.insert(mir_func.name.clone(), func_id);
            needs_main_wrapper = true;
        } else {
            // User-defined functions are declared as Local (not Export) so they
            // do NOT collide with libc/POSIX symbols of the same name (e.g. a
            // user-level `bind`, `read`, `write`, `open`, `close` would
            // otherwise be resolved via dlsym to libc's version inside the JIT,
            // which causes silent stack overflows or segfaults).
            let func_id = object_module.declare_function(&mir_func.name, Linkage::Local, &sig)?;
            func_ids.insert(mir_func.name.clone(), func_id);
        }
    }

    // ---------------------------------------------------------------------
    // Async poll-wrapper declarations.
    //
    // For every `async fn F`, declare an exported `__kryos_poll_F` with the
    // Kryos runtime poll ABI:
    //   extern "C" fn(*mut u8) -> i32   ; KRYOS_PENDING=0 / KRYOS_READY=1
    //
    // The body (synthesised below) reads params back out of the state
    // struct, calls the original fn synchronously, stores the result into
    // the state struct's `result` field, marks state = -1 (done), and
    // returns KRYOS_READY. This makes async fns runnable end-to-end via
    // kryos_async_block_on while preserving today's semantics (await is
    // still a direct call; the wrapper is just glue to the executor).
    //
    // Names match kryos_mir::async_lower::poll_fn_name_for so the rest of
    // the toolchain stays in lock-step.
    // ---------------------------------------------------------------------
    let mut async_poll_ids: HashMap<String, FuncId> = HashMap::new();
    {
        let call_conv = object_module.isa().default_call_conv();
        for mir_func in &module.functions {
            if !mir_func.attributes.is_async {
                continue;
            }
            let poll_name = kryos_mir::async_lower::poll_fn_name_for(&mir_func.name);
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // state pointer (as i64)
            sig.returns.push(AbiParam::new(types::I32)); // KryosPoll discriminant
            let id = object_module.declare_function(&poll_name, Linkage::Export, &sig)?;
            async_poll_ids.insert(mir_func.name.clone(), id);
            func_ids.insert(poll_name, id);
        }
    }

    // -----------------------------------------------------------------------
    // Closure env-wrapper (thunk) generation
    // -----------------------------------------------------------------------
    // Scan all MIR functions for RValue::Closure to collect info about which
    // functions are used as closure values and how many captures they have.
    // For each, we generate a thunk `{name}_env(env, user_args...) -> i64`
    // that unpacks captures from the env pointer and calls the original.
    // This gives ALL function values a uniform env-based calling convention:
    //   env layout: [thunk_fn_ptr, cap0, cap1, ...]
    //   CallIndirect: load fn from env[0], call fn(env, user_args...)
    let mir_func_map: HashMap<&str, &MirFunction> = module
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f))
        .collect();

    // Maps func_name -> (num_captures, user_param_count, capture_types).
    // capture_types tracks the MIR type of each capture so we can generate
    // a dropper function that frees heap-allocated captures when the closure
    // env's ARC ref count reaches zero.
    let mut closure_info: HashMap<String, (usize, usize, Vec<Option<MirType>>)> = HashMap::new();
    for mir_func in &module.functions {
        for bb in &mir_func.blocks {
            for inst in &bb.instructions {
                if let Instruction::Assign {
                    value:
                        RValue::Closure {
                            func_name,
                            captures,
                        },
                    ..
                } = inst
                {
                    if !closure_info.contains_key(func_name.as_str()) {
                        let user_params = if let Some(f) = mir_func_map.get(func_name.as_str()) {
                            f.params.len().saturating_sub(captures.len())
                        } else {
                            0
                        };
                        let cap_types: Vec<Option<MirType>> = captures
                            .iter()
                            .map(|cap| match cap {
                                Operand::Local(id) => mir_func
                                    .locals
                                    .iter()
                                    .find(|l| l.id == *id)
                                    .map(|l| l.ty.clone()),
                                _ => None,
                            })
                            .collect();
                        closure_info
                            .insert(func_name.clone(), (captures.len(), user_params, cap_types));
                    }
                }
            }
        }
    }

    // Declare thunk functions in the module.
    let mut thunk_ids: HashMap<String, FuncId> = HashMap::new();
    {
        let call_conv = object_module.isa().default_call_conv();
        for (func_name, (_, user_param_count, _)) in &closure_info {
            let env_thunk_name = format!("{func_name}_env");
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // env pointer
            for _ in 0..*user_param_count {
                sig.params.push(AbiParam::new(types::I64));
            }
            sig.returns.push(AbiParam::new(types::I64));
            let id = object_module.declare_function(&env_thunk_name, Linkage::Export, &sig)?;
            thunk_ids.insert(func_name.clone(), id);
            func_ids.insert(env_thunk_name, id);
        }
    }

    // Declare dropper functions for closures with heap-typed captures.
    // When ARC ref count reaches 0, the dropper frees each captured value.
    let mut dropper_ids: HashMap<String, FuncId> = HashMap::new();
    {
        let call_conv = object_module.isa().default_call_conv();
        for (func_name, (_, _, cap_types)) in &closure_info {
            let has_heap_caps = cap_types.iter().any(|ct| {
                matches!(
                    ct,
                    Some(MirType::Str)
                        | Some(MirType::Array(_, _))
                        | Some(MirType::Function { .. })
                        | Some(MirType::Shared(_))
                        | Some(MirType::Struct(_))
                        | Some(MirType::Enum(_))
                )
            });
            if has_heap_caps {
                let dropper_name = format!("{func_name}_drop");
                let mut sig = Signature::new(call_conv);
                sig.params.push(AbiParam::new(types::I64)); // env ptr
                let id = object_module.declare_function(&dropper_name, Linkage::Local, &sig)?;
                dropper_ids.insert(func_name.clone(), id);
                func_ids.insert(dropper_name, id);
            }
        }
    }

    // Declare named drop helpers for struct/enum types with heap-owning fields.
    // These break compile-time recursion when array elements are structs/enums
    // that themselves contain heap fields (e.g. strings inside structs inside arrays).
    let mut type_drop_ids: HashMap<String, FuncId> = HashMap::new();
    {
        let call_conv = object_module.isa().default_call_conv();
        let has_heap_fields = |fields: &[(String, MirType)]| -> bool {
            fields.iter().any(|(_, ty)| {
                matches!(
                    ty,
                    MirType::Str
                        | MirType::Array(_, _)
                        | MirType::Struct(_)
                        | MirType::Function { .. }
                        | MirType::Enum(_)
                        | MirType::Shared(_)
                )
            })
        };
        for (name, fields) in &module.struct_defs {
            if name != "Map" && has_heap_fields(fields) {
                let drop_name = format!("__kryos_drop_{name}");
                let mut sig = Signature::new(call_conv);
                sig.params.push(AbiParam::new(types::I64)); // struct ptr
                let id = object_module.declare_function(&drop_name, Linkage::Local, &sig)?;
                type_drop_ids.insert(name.clone(), id);
                func_ids.insert(drop_name, id);
            }
        }
        for (name, variants) in &module.enum_defs {
            let has_droppable = variants.iter().any(|v| {
                v.fields.iter().any(|f| {
                    matches!(
                        f,
                        MirType::Str
                            | MirType::Array(_, _)
                            | MirType::Struct(_)
                            | MirType::Function { .. }
                            | MirType::Enum(_)
                            | MirType::Shared(_)
                    )
                })
            });
            if has_droppable {
                let drop_name = format!("__kryos_drop_{name}");
                let mut sig = Signature::new(call_conv);
                sig.params.push(AbiParam::new(types::I64)); // enum ptr
                let id = object_module.declare_function(&drop_name, Linkage::Local, &sig)?;
                type_drop_ids.insert(name.clone(), id);
                func_ids.insert(drop_name, id);
            }
        }
    }

    // Pre-declare runtime functions needed by clone helper bodies.
    {
        let call_conv = object_module.isa().default_call_conv();
        let calloc_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            sig
        };
        let one_in_one_out = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            sig
        };
        if !func_ids.contains_key("calloc") {
            let id = object_module.declare_function("calloc", Linkage::Import, &calloc_sig)?;
            func_ids.insert("calloc".to_string(), id);
        }
        for name in ["kryos_string_clone", "kryos_array_clone", "kryos_map_clone"] {
            if !func_ids.contains_key(name) {
                let id = object_module.declare_function(name, Linkage::Import, &one_in_one_out)?;
                func_ids.insert(name.to_string(), id);
            }
        }
        // kryos_array_clone_deep(arr, elem_clone_fn) -> arr
        // Per-element deep clone moved into the runtime: takes the array
        // and a (i64) -> i64 clone function pointer (same shape as
        // kryos_string_clone / __kryos_clone_<N>). Replaces the codegen-
        // emitted loops in emit_array_str_deep_clone / emit_array_struct_deep_clone
        // — one call instead of an inline loop, reduces heap pressure.
        let two_in_one_out = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            sig
        };
        if !func_ids.contains_key("kryos_array_clone_deep") {
            let id = object_module.declare_function(
                "kryos_array_clone_deep",
                Linkage::Import,
                &two_in_one_out,
            )?;
            func_ids.insert("kryos_array_clone_deep".to_string(), id);
        }
    }

    // ISOLATION-TEST: declare __kryos_clone_<Name> for @copy structs with heap
    // fields. Body emission below.
    let mut type_clone_ids: HashMap<String, FuncId> = HashMap::new();
    {
        let call_conv = object_module.isa().default_call_conv();
        let has_heap_fields = |fields: &[(String, MirType)]| -> bool {
            fields.iter().any(|(_, ty)| {
                matches!(
                    ty,
                    MirType::Str
                        | MirType::Array(_, _)
                        | MirType::Struct(_)
                        | MirType::Function { .. }
                        | MirType::Enum(_)
                        | MirType::Shared(_)
                        | MirType::Map { .. }
                )
            })
        };
        for (name, fields) in &module.struct_defs {
            if name != "Map"
                && module.copy_structs.contains(name)
                && has_heap_fields(fields)
            {
                let clone_name = format!("__kryos_clone_{name}");
                let mut sig = Signature::new(call_conv);
                sig.params.push(AbiParam::new(types::I64));
                sig.returns.push(AbiParam::new(types::I64));
                // Linkage::Export, not Local. Linkage::Local on a function whose
                // return value is immediately stored across the call boundary
                // triggers a Cranelift IR materialization bug that segfaults
                // stage-1 at runtime. Diagnosed in shift 2: an identical pattern
                // calling Linkage::Import kryos_string_clone works fine; only the
                // Local variant crashes. Export sidesteps the issue.
                let id = object_module.declare_function(&clone_name, Linkage::Export, &sig)?;
                type_clone_ids.insert(name.clone(), id);
                func_ids.insert(clone_name, id);
            }
        }
    }

    // Declare and define ARC runtime stub functions.
    // These are no-op stubs until a proper runtime library is linked.
    // We define them locally so the linker doesn't require an external runtime.
    let call_conv = object_module.isa().default_call_conv();
    let arc_retain_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64));
        sig
    };
    let arc_release_sig = arc_retain_sig.clone();
    let arc_set_drop_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // ptr
        sig.params.push(AbiParam::new(types::I64)); // drop_fn (fn ptr as i64)
        sig
    };
    let arc_alloc_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        sig
    };

    // Import ARC runtime functions from libkryos_rt (i64 wrappers).
    let arc_retain_id =
        object_module.declare_function("kryos_arc_retain_i64", Linkage::Import, &arc_retain_sig)?;
    let arc_release_id = object_module.declare_function(
        "kryos_arc_release_i64",
        Linkage::Import,
        &arc_release_sig,
    )?;
    let arc_alloc_id =
        object_module.declare_function("kryos_arc_alloc_i64", Linkage::Import, &arc_alloc_sig)?;
    let arc_set_drop_id = object_module.declare_function(
        "kryos_arc_set_drop_i64",
        Linkage::Import,
        &arc_set_drop_sig,
    )?;

    func_ids.insert("kryos_arc_retain".to_string(), arc_retain_id);
    func_ids.insert("kryos_arc_release".to_string(), arc_release_id);
    func_ids.insert("kryos_arc_set_drop".to_string(), arc_set_drop_id);
    func_ids.insert("kryos_arc_alloc".to_string(), arc_alloc_id);
    func_ids.insert("kryos_arc_alloc_i64".to_string(), arc_alloc_id);

    // Declare C `puts` for println support.
    // println("...") in Kryos MIR maps to a call to C puts(const char*).
    let puts_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // const char* string pointer
        sig.returns.push(AbiParam::new(types::I32)); // int return
        sig
    };
    let puts_id = object_module.declare_function("puts", Linkage::Import, &puts_sig)?;
    if !user_func_names.contains("println") {
        func_ids.insert("println".to_string(), puts_id);
    }

    // Declare C `printf` for print support (no newline).
    // print("...") in Kryos MIR maps to a call to C printf(const char*).
    let printf_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // const char* format/string pointer
        sig.returns.push(AbiParam::new(types::I32)); // int return
        sig
    };
    let printf_id = object_module.declare_function("printf", Linkage::Import, &printf_sig)?;
    if !user_func_names.contains("print") {
        func_ids.insert("print".to_string(), printf_id);
    }
    // Suppress unused-warning when user redefines print.
    let _ = printf_id;

    // Declare fputs and fputc for stderr output (used by eprintln).
    let fputs_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // const char*
        sig.params.push(AbiParam::new(types::I64)); // FILE*
        sig.returns.push(AbiParam::new(types::I32));
        sig
    };
    let fputs_id = object_module.declare_function("fputs", Linkage::Import, &fputs_sig)?;

    let fputc_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I32)); // int char
        sig.params.push(AbiParam::new(types::I64)); // FILE*
        sig.returns.push(AbiParam::new(types::I32));
        sig
    };
    let fputc_id = object_module.declare_function("fputc", Linkage::Import, &fputc_sig)?;

    // Define kryos_eprintln: writes message to stderr with trailing newline.
    let eprintln_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // const char* message
        sig
    };
    let eprintln_id =
        object_module.declare_function("kryos_eprintln", Linkage::Local, &eprintln_sig)?;
    if !user_func_names.contains("eprintln") {
        func_ids.insert("eprintln".to_string(), eprintln_id);
    }
    {
        let mut ep_fn = Function::with_name_signature(
            UserFuncName::user(0, eprintln_id.as_u32()),
            eprintln_sig.clone(),
        );
        {
            let mut builder = FunctionBuilder::new(&mut ep_fn, &mut fb_ctx);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);

            let msg = builder.block_params(block)[0];

            // Get stderr FILE* — platform-specific.
            let stderr_ptr = if cfg!(target_os = "windows") {
                // Windows UCRT: __acrt_iob_func(2) returns FILE* for stderr.
                let iob_sig = {
                    let mut sig = Signature::new(call_conv);
                    sig.params.push(AbiParam::new(types::I32));
                    sig.returns.push(AbiParam::new(types::I64));
                    sig
                };
                let iob_id =
                    object_module.declare_function("__acrt_iob_func", Linkage::Import, &iob_sig)?;
                let iob_ref = object_module.declare_func_in_func(iob_id, builder.func);
                let two = builder.ins().iconst(types::I32, 2);
                let call = builder.ins().call(iob_ref, &[two]);
                builder.inst_results(call)[0]
            } else {
                // Unix: load from extern FILE *stderr global.
                let stderr_data_id =
                    object_module.declare_data("stderr", Linkage::Import, false, false)?;
                let stderr_gv = object_module.declare_data_in_func(stderr_data_id, builder.func);
                let addr = builder.ins().global_value(types::I64, stderr_gv);
                builder.ins().load(types::I64, MemFlags::trusted(), addr, 0)
            };

            // fputs(msg, stderr)
            let fputs_ref = object_module.declare_func_in_func(fputs_id, builder.func);
            builder.ins().call(fputs_ref, &[msg, stderr_ptr]);

            // fputc('\n', stderr)
            let fputc_ref = object_module.declare_func_in_func(fputc_id, builder.func);
            let newline = builder.ins().iconst(types::I32, 10);
            builder.ins().call(fputc_ref, &[newline, stderr_ptr]);

            builder.ins().return_(&[]);
            builder.finalize();
        }
        let mut ctx = Context::for_function(ep_fn);
        object_module.define_function(eprintln_id, &mut ctx)?;
        ctx.clear();
    }

    // Declare C `exit` for exit support.
    // exit(code) in Kryos MIR maps to a call to C exit(int).
    let exit_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I32)); // int status
        sig
    };
    let exit_id = object_module.declare_function("exit", Linkage::Import, &exit_sig)?;
    if !user_func_names.contains("exit") {
        func_ids.insert("exit".to_string(), exit_id);
    }
    let _ = exit_id;

    // Import len() builtin — reads len field from any Kryos collection.
    let len_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // collection handle
        sig.returns.push(AbiParam::new(types::I64)); // length
        sig
    };
    let len_id = object_module.declare_function("kryos_builtin_len", Linkage::Import, &len_sig)?;
    func_ids.insert("len".to_string(), len_id);

    // Import to_string() builtin — converts i64 to KryosString.
    let to_string_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // value
        sig.returns.push(AbiParam::new(types::I64)); // string handle
        sig
    };
    let to_string_id = object_module.declare_function(
        "kryos_builtin_to_string",
        Linkage::Import,
        &to_string_sig,
    )?;
    func_ids.insert("to_string".to_string(), to_string_id);

    // Import f64_to_string — converts f64 to KryosString.
    let f64_to_string_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::F64)); // f64 value
        sig.returns.push(AbiParam::new(types::I64)); // string handle
        sig
    };
    let f64_to_string_id = object_module.declare_function(
        "kryos_f64_to_string",
        Linkage::Import,
        &f64_to_string_sig,
    )?;
    func_ids.insert("kryos_f64_to_string".to_string(), f64_to_string_id);

    // Import bool_to_string — converts i64 (0/nonzero) to "true"/"false" KryosString.
    let bool_to_string_id = object_module.declare_function(
        "kryos_bool_to_string",
        Linkage::Import,
        &to_string_sig, // same signature as builtin_to_string: (i64) -> i64
    )?;
    func_ids.insert("kryos_bool_to_string".to_string(), bool_to_string_id);

    // Import ipow() builtin — integer exponentiation.
    let ipow_sig = {
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // base
        sig.params.push(AbiParam::new(types::I64)); // exp
        sig.returns.push(AbiParam::new(types::I64)); // result
        sig
    };
    let ipow_id = object_module.declare_function("kryos_ipow", Linkage::Import, &ipow_sig)?;
    func_ids.insert("kryos_ipow".to_string(), ipow_id);

    // -------------------------------------------------------------------
    // Low-level FFI helpers (i64 ABI). These let Kryos code cross the
    // FFI boundary without exposing raw `*mut u8` pointers. They map to
    // `kryos_*` symbols in libkryos_rt (see kryos-rt::builtins).
    // -------------------------------------------------------------------
    {
        // str_to_ptr(s: str) -> i64
        let sig_i64_i64 = {
            let mut s = Signature::new(call_conv);
            s.params.push(AbiParam::new(types::I64));
            s.returns.push(AbiParam::new(types::I64));
            s
        };
        // alloc(n: i64) -> i64  (same shape)
        // handle_to_str(h: i64) -> i64
        let str_to_ptr_id = object_module.declare_function(
            "kryos_str_to_ptr",
            Linkage::Import,
            &sig_i64_i64,
        )?;
        func_ids.insert("str_to_ptr".to_string(), str_to_ptr_id);

        let alloc_id = object_module.declare_function(
            "kryos_alloc_bytes",
            Linkage::Import,
            &sig_i64_i64,
        )?;
        func_ids.insert("alloc".to_string(), alloc_id);

        let handle_to_str_id = object_module.declare_function(
            "kryos_handle_to_str",
            Linkage::Import,
            &sig_i64_i64,
        )?;
        func_ids.insert("handle_to_str".to_string(), handle_to_str_id);

        // (i64, i64) -> i64
        let sig_i64_i64_to_i64 = {
            let mut s = Signature::new(call_conv);
            s.params.push(AbiParam::new(types::I64));
            s.params.push(AbiParam::new(types::I64));
            s.returns.push(AbiParam::new(types::I64));
            s
        };
        let buf_to_str_id = object_module.declare_function(
            "kryos_buf_to_str",
            Linkage::Import,
            &sig_i64_i64_to_i64,
        )?;
        func_ids.insert("buf_to_str".to_string(), buf_to_str_id);

        let ptr_byte_at_id = object_module.declare_function(
            "kryos_ptr_byte_at",
            Linkage::Import,
            &sig_i64_i64_to_i64,
        )?;
        func_ids.insert("ptr_byte_at".to_string(), ptr_byte_at_id);

        let ptr_read_i64_id = object_module.declare_function(
            "kryos_ptr_read_i64",
            Linkage::Import,
            &sig_i64_i64_to_i64,
        )?;
        func_ids.insert("ptr_read_i64".to_string(), ptr_read_i64_id);

        // (i64, i64) -> void   (free_bytes)
        let sig_i64_i64_void = {
            let mut s = Signature::new(call_conv);
            s.params.push(AbiParam::new(types::I64));
            s.params.push(AbiParam::new(types::I64));
            s
        };
        let free_bytes_id = object_module.declare_function(
            "kryos_free_bytes",
            Linkage::Import,
            &sig_i64_i64_void,
        )?;
        func_ids.insert("free_bytes".to_string(), free_bytes_id);

        // (i64, i64, i64) -> void   (ptr_set_byte, ptr_write_i64)
        let sig_i64_i64_i64_void = {
            let mut s = Signature::new(call_conv);
            s.params.push(AbiParam::new(types::I64));
            s.params.push(AbiParam::new(types::I64));
            s.params.push(AbiParam::new(types::I64));
            s
        };
        let ptr_set_byte_id = object_module.declare_function(
            "kryos_ptr_set_byte",
            Linkage::Import,
            &sig_i64_i64_i64_void,
        )?;
        func_ids.insert("ptr_set_byte".to_string(), ptr_set_byte_id);

        let ptr_write_i64_id = object_module.declare_function(
            "kryos_ptr_write_i64",
            Linkage::Import,
            &sig_i64_i64_i64_void,
        )?;
        func_ids.insert("ptr_write_i64".to_string(), ptr_write_i64_id);
    }

    // NOTE: ARC runtime functions (kryos_arc_alloc, kryos_arc_retain,
    // kryos_arc_release) are declared above via kryos_arc_*_i64 wrappers.
    // Do NOT re-declare them here — the i64 wrappers use the correct
    // 1-param signatures matching the codegen calling convention.

    // Declare C heap functions as imports.
    {
        let malloc_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            sig
        };
        let free_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64));
            sig
        };
        let realloc_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            sig
        };
        let malloc_id = object_module.declare_function("malloc", Linkage::Import, &malloc_sig)?;
        let free_id = object_module.declare_function("free", Linkage::Import, &free_sig)?;
        let realloc_id =
            object_module.declare_function("realloc", Linkage::Import, &realloc_sig)?;
        func_ids.insert("malloc".to_string(), malloc_id);
        func_ids.insert("free".to_string(), free_id);
        func_ids.insert("realloc".to_string(), realloc_id);
    }

    // Declare Kryos runtime string/array functions as imports.
    {
        // String: (ptr, i64) -> ptr
        let string_new_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // ptr
            sig.params.push(AbiParam::new(types::I64)); // len
            sig.returns.push(AbiParam::new(types::I64)); // ptr
            sig
        };
        let string_concat_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // a
            sig.params.push(AbiParam::new(types::I64)); // b
            sig.returns.push(AbiParam::new(types::I64)); // ptr
            sig
        };
        let string_len_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // s
            sig.returns.push(AbiParam::new(types::I64)); // len
            sig
        };
        let string_eq_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // a
            sig.params.push(AbiParam::new(types::I64)); // b
            sig.returns.push(AbiParam::new(types::I8)); // bool
            sig
        };
        let string_slice_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // s
            sig.params.push(AbiParam::new(types::I64)); // start
            sig.params.push(AbiParam::new(types::I64)); // end
            sig.returns.push(AbiParam::new(types::I64)); // ptr
            sig
        };
        let string_find_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // s
            sig.params.push(AbiParam::new(types::I64)); // needle
            sig.returns.push(AbiParam::new(types::I64)); // offset
            sig
        };
        let string_free_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // s
            sig
        };

        let sn_id =
            object_module.declare_function("kryos_string_new", Linkage::Import, &string_new_sig)?;
        let sc_id = object_module.declare_function(
            "kryos_string_concat",
            Linkage::Import,
            &string_concat_sig,
        )?;
        let sl_id =
            object_module.declare_function("kryos_string_len", Linkage::Import, &string_len_sig)?;
        let se_id =
            object_module.declare_function("kryos_string_eq", Linkage::Import, &string_eq_sig)?;
        // kryos_string_compare(a, b) -> i64  (-1/0/+1). Reuse the (i64,i64)->i64 shape of string_concat_sig.
        let scmp_id = object_module.declare_function(
            "kryos_string_compare",
            Linkage::Import,
            &string_concat_sig,
        )?;
        let ss_id = object_module.declare_function(
            "kryos_string_slice",
            Linkage::Import,
            &string_slice_sig,
        )?;
        let sf_id = object_module.declare_function(
            "kryos_string_find",
            Linkage::Import,
            &string_find_sig,
        )?;
        let sfr_id = object_module.declare_function(
            "kryos_string_free",
            Linkage::Import,
            &string_free_sig,
        )?;

        func_ids.insert("kryos_string_new".to_string(), sn_id);
        func_ids.insert("kryos_string_concat".to_string(), sc_id);
        func_ids.insert("kryos_string_len".to_string(), sl_id);
        func_ids.insert("kryos_string_eq".to_string(), se_id);
        func_ids.insert("kryos_string_compare".to_string(), scmp_id);
        func_ids.insert("kryos_string_slice".to_string(), ss_id);
        func_ids.insert("kryos_string_find".to_string(), sf_id);
        func_ids.insert("kryos_string_free".to_string(), sfr_id);

        // kryos_string_char_at(s_handle, idx) -> i64  (same sig as array_get)
        let string_char_at_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // s_handle
            sig.params.push(AbiParam::new(types::I64)); // idx
            sig.returns.push(AbiParam::new(types::I64)); // char handle
            sig
        };
        let sca_id = object_module.declare_function(
            "kryos_string_char_at",
            Linkage::Import,
            &string_char_at_sig,
        )?;
        func_ids.insert("kryos_string_char_at".to_string(), sca_id);

        // Array functions.
        let array_new_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // elem_size
            sig.params.push(AbiParam::new(types::I64)); // cap
            sig.returns.push(AbiParam::new(types::I64)); // ptr
            sig
        };
        let array_push_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // arr
            sig.params.push(AbiParam::new(types::I64)); // val
            sig
        };
        let array_get_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // arr
            sig.params.push(AbiParam::new(types::I64)); // idx
            sig.returns.push(AbiParam::new(types::I64)); // val
            sig
        };
        let array_set_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // arr
            sig.params.push(AbiParam::new(types::I64)); // idx
            sig.params.push(AbiParam::new(types::I64)); // val
            sig
        };
        let array_len_sig = string_len_sig.clone();
        let array_free_sig = string_free_sig.clone();

        // kryos_array_concat(a: i64, b: i64) -> i64
        let array_concat_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // a
            sig.params.push(AbiParam::new(types::I64)); // b
            sig.returns.push(AbiParam::new(types::I64)); // new arr
            sig
        };

        let an_id =
            object_module.declare_function("kryos_array_new", Linkage::Import, &array_new_sig)?;
        let ap_id =
            object_module.declare_function("kryos_array_push", Linkage::Import, &array_push_sig)?;
        let ag_id =
            object_module.declare_function("kryos_array_get", Linkage::Import, &array_get_sig)?;
        let as_id =
            object_module.declare_function("kryos_array_set", Linkage::Import, &array_set_sig)?;
        let al_id =
            object_module.declare_function("kryos_array_len", Linkage::Import, &array_len_sig)?;
        let af_id =
            object_module.declare_function("kryos_array_free", Linkage::Import, &array_free_sig)?;
        let ac_id = object_module.declare_function(
            "kryos_array_concat",
            Linkage::Import,
            &array_concat_sig,
        )?;

        func_ids.insert("kryos_array_new".to_string(), an_id);
        func_ids.insert("kryos_array_push".to_string(), ap_id);
        func_ids.insert("kryos_array_get".to_string(), ag_id);
        func_ids.insert("kryos_array_set".to_string(), as_id);
        func_ids.insert("kryos_array_len".to_string(), al_id);
        func_ids.insert("kryos_array_free".to_string(), af_id);
        func_ids.insert("kryos_array_concat".to_string(), ac_id);

        // Map functions.
        let map_new_sig = {
            let mut sig = Signature::new(call_conv);
            sig.returns.push(AbiParam::new(types::I64)); // opaque handle
            sig
        };
        let map_insert_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // map
            sig.params.push(AbiParam::new(types::I64)); // key
            sig.params.push(AbiParam::new(types::I64)); // value
            sig
        };
        let map_get_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // map
            sig.params.push(AbiParam::new(types::I64)); // key
            sig.returns.push(AbiParam::new(types::I64)); // value
            sig
        };
        let map_len_sig = string_len_sig.clone();
        let map_free_sig = string_free_sig.clone();

        let mn_id =
            object_module.declare_function("kryos_map_new", Linkage::Import, &map_new_sig)?;
        let mi_id =
            object_module.declare_function("kryos_map_insert", Linkage::Import, &map_insert_sig)?;
        let mis_id = object_module.declare_function(
            "kryos_map_insert_str",
            Linkage::Import,
            &map_insert_sig,
        )?;
        let mg_id =
            object_module.declare_function("kryos_map_get", Linkage::Import, &map_get_sig)?;
        let mgs_id =
            object_module.declare_function("kryos_map_get_str", Linkage::Import, &map_get_sig)?;
        let ml_id =
            object_module.declare_function("kryos_map_len", Linkage::Import, &map_len_sig)?;
        let mf_id =
            object_module.declare_function("kryos_map_free", Linkage::Import, &map_free_sig)?;

        func_ids.insert("kryos_map_new".to_string(), mn_id);
        func_ids.insert("kryos_map_insert".to_string(), mi_id);
        func_ids.insert("kryos_map_insert_str".to_string(), mis_id);
        func_ids.insert("kryos_map_get".to_string(), mg_id);
        func_ids.insert("kryos_map_get_str".to_string(), mgs_id);
        func_ids.insert("kryos_map_len".to_string(), ml_id);
        func_ids.insert("kryos_map_free".to_string(), mf_id);
    }

    // Declare trace runtime functions for stack trace support.
    {
        let trace_enter_sig = {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // name_ptr
            sig.params.push(AbiParam::new(types::I64)); // name_len
            sig.params.push(AbiParam::new(types::I64)); // file_ptr
            sig.params.push(AbiParam::new(types::I64)); // file_len
            sig.params.push(AbiParam::new(types::I64)); // line
            sig
        };
        let trace_exit_sig = Signature::new(call_conv);

        let te_id = object_module.declare_function(
            "kryos_trace_enter",
            Linkage::Import,
            &trace_enter_sig,
        )?;
        let tx_id =
            object_module.declare_function("kryos_trace_exit", Linkage::Import, &trace_exit_sig)?;
        func_ids.insert("kryos_trace_enter".to_string(), te_id);
        func_ids.insert("kryos_trace_exit".to_string(), tx_id);
    }

    // Module-level string counter to avoid duplicate data section names.
    let mut global_str_counter: u32 = 0;

    // Second pass: translate each function body.
    for mir_func in &module.functions {
        let func_id = func_ids[&mir_func.name];
        let sig = build_signature(mir_func, object_module.isa().default_call_conv());

        let mut cl_func =
            Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);

        {
            let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
            translate_function(
                mir_func,
                &mut builder,
                &func_ids,
                &mut object_module,
                &module.struct_defs,
                &module.enum_defs,
                &mut global_str_counter,
                &module.trait_vtables,
                options.checked_arithmetic,
                &module.copy_structs,
                &user_func_names,
            )?;
            builder.seal_all_blocks();
            builder.finalize();
        }

        let mut ctx = Context::for_function(cl_func);
        if std::env::var("KRYOS_DUMP_IR").is_ok() {
            eprintln!("[kryos-aot] IR for '{}':\n{}", mir_func.name, ctx.func.display());
        }
        object_module
            .define_function(func_id, &mut ctx)
            .map_err(|e| {
                eprintln!("[kryos] codegen error in function '{}': {e}", mir_func.name);
                eprintln!("[kryos] full error details: {e:#?}");
                CodegenError::Module(e)
            })?;
    }

    // -----------------------------------------------------------------------
    // Async poll-wrapper bodies.
    //
    // For each async fn F, emit __kryos_poll_F(state: *mut u8) -> i32:
    //   1. Read parameter values from the state struct fields (those marked
    //      is_param in the AsyncPlan have known offsets via the struct layout).
    //   2. Call F(params..., state_ptr) — when split-at-await has rewritten F,
    //      F itself is the dispatcher and returns KRYOS_PENDING (0) or
    //      KRYOS_READY (1). When split-at-await did not run (no awaits, or
    //      `KRYOS_DISABLE_AWAIT_SPLIT=1`), F's natural return value is the
    //      computed result and we store it into the state's `result` field.
    //   3. Store the call's return into `result` (only meaningful when the
    //      callee runs to completion in this poll).
    //   4. Mark state = -1 (done) only when the callee reported READY. When
    //      the callee reported PENDING, leave the state discriminant so the
    //      next poll resumes at the correct split.
    //   5. Return the propagated status.
    //
    // If any prerequisite is missing (no state struct, fields don't match,
    // etc.), we emit a stub that just returns KRYOS_READY = 1 so the binary
    // still links cleanly. The driver-side validation in async_lower::run
    // catches the cases that matter.
    // -----------------------------------------------------------------------
    {
        let call_conv = object_module.isa().default_call_conv();
        for mir_func in &module.functions {
            if !mir_func.attributes.is_async {
                continue;
            }
            let Some(&poll_id) = async_poll_ids.get(&mir_func.name) else {
                continue;
            };
            let state_name = kryos_mir::async_lower::state_struct_name_for(&mir_func.name);
            let state_fields = module.struct_defs.get(&state_name);

            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));

            let mut cl_func =
                Function::with_name_signature(UserFuncName::user(0, poll_id.as_u32()), sig);
            {
                let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                let state_ptr = builder.block_params(entry)[0];

                // Best-effort: only synthesise a real body when we have a
                // matching state-struct layout. Otherwise fall through to
                // the always-ready stub at the bottom.
                let mut emitted_real_body = false;
                if let Some(fields) = state_fields {
                    if let Ok(layout) = compute_struct_layout(fields) {
                        // Build offset map by field name.
                        let mut off: HashMap<&str, (u32, Type)> = HashMap::new();
                        for (n, o, t) in &layout.field_offsets {
                            off.insert(n.as_str(), (*o, *t));
                        }

                        // Look up each param's slot in the state struct.
                        // Param names come from MirParam.name; field names
                        // were inserted by async_lower::apply_state_structs
                        // and match the local's name.
                        let mut call_args: Vec<cranelift_codegen::ir::Value> = Vec::new();
                        let mut all_params_found = true;
                        for param in &mir_func.params {
                            // Look up the param's name via the matching MirLocal.
                            let pname = mir_func
                                .locals
                                .iter()
                                .find(|l| l.id == param.local)
                                .and_then(|l| l.name.as_deref());
                            let Some(pname) = pname else {
                                all_params_found = false;
                                break;
                            };
                            let Some(&(offset, ty)) = off.get(pname) else {
                                all_params_found = false;
                                break;
                            };
                            let v = builder.ins().load(
                                ty,
                                MemFlags::new(),
                                state_ptr,
                                offset as i32,
                            );
                            call_args.push(v);
                        }

                        if all_params_found {
                            // Coerce to the callee's expected signature
                            // (build_signature is the canonical source).
                            let callee_sig =
                                build_signature(mir_func, object_module.isa().default_call_conv());
                            for (i, arg) in call_args.iter_mut().enumerate() {
                                if i >= callee_sig.params.len() {
                                    break;
                                }
                                let expected = callee_sig.params[i].value_type;
                                let actual = builder.func.dfg.value_type(*arg);
                                if expected != actual {
                                    if expected.is_int() && actual.is_int() {
                                        if expected.bits() < actual.bits() {
                                            *arg = builder.ins().ireduce(expected, *arg);
                                        } else {
                                            *arg = builder.ins().sextend(expected, *arg);
                                        }
                                    } else if expected.is_float() && actual.is_int() {
                                        *arg = builder
                                            .ins()
                                            .bitcast(expected, MemFlags::new(), *arg);
                                    } else if expected.is_int() && actual.is_float() {
                                        *arg = builder
                                            .ins()
                                            .bitcast(expected, MemFlags::new(), *arg);
                                    }
                                }
                            }

                            // Capture poll-status awareness: if the function
                            // was split, its signature's first non-param slot
                            // is the *status* (READY/PENDING). We detect this
                            // by checking whether the function was rewritten
                            // (heuristic: it has more than one block and the
                            // entry block ends in a Switch on `state`). For
                            // simplicity and safety we always propagate the
                            // returned i32/i64 as the status when the callee's
                            // single return is an integer type.
                            let callee_id = func_ids[&mir_func.name];
                            let callee_ref =
                                object_module.declare_func_in_func(callee_id, builder.func);
                            let call_inst = builder.ins().call(callee_ref, &call_args);
                            let results = builder.inst_results(call_inst).to_vec();

                            // Store result into state.result (if the slot exists).
                            if let Some(&(res_off, res_ty)) = off.get("result") {
                                let result_val = if results.is_empty() {
                                    builder.ins().iconst(res_ty, 0)
                                } else {
                                    let r = results[0];
                                    let rt = builder.func.dfg.value_type(r);
                                    if rt == res_ty {
                                        r
                                    } else if rt.is_int() && res_ty.is_int() {
                                        if res_ty.bits() < rt.bits() {
                                            builder.ins().ireduce(res_ty, r)
                                        } else if res_ty.bits() > rt.bits() {
                                            builder.ins().sextend(res_ty, r)
                                        } else {
                                            r
                                        }
                                    } else if is_float_type(rt) && res_ty.is_int() {
                                        builder.ins().bitcast(res_ty, MemFlags::new(), r)
                                    } else if rt.is_int() && is_float_type(res_ty) {
                                        builder.ins().bitcast(res_ty, MemFlags::new(), r)
                                    } else {
                                        r
                                    }
                                };
                                builder.ins().store(
                                    MemFlags::new(),
                                    result_val,
                                    state_ptr,
                                    res_off as i32,
                                );
                            }

                            // Mark state = -1 (done). After split-at-await,
                            // the rewritten dispatcher itself updates the
                            // state discriminant on each suspend; if it
                            // returned READY (1) the function is fully
                            // complete and we can stamp DONE. If the callee
                            // returns PENDING (0), the state field already
                            // holds the resume index — do NOT overwrite it.
                            //
                            // We resolve this conservatively by inspecting
                            // the call result: if results[0] is non-zero,
                            // stamp done. For the non-split (legacy) path,
                            // results[0] is the user return value, which we
                            // can't safely interpret as a status — so we keep
                            // the old eager DONE behaviour when there is no
                            // detectable split.
                            let function_was_split =
                                mir_func.blocks.len() > 1
                                && matches!(
                                    mir_func.blocks.first().map(|b| &b.terminator),
                                    Some(kryos_mir::ir::Terminator::Switch { .. })
                                );
                            if let Some(&(s_off, s_ty)) = off.get("state") {
                                if function_was_split && !results.is_empty() {
                                    // Only stamp DONE if the dispatcher
                                    // returned READY (non-zero).
                                    let status = results[0];
                                    let status_ty = builder.func.dfg.value_type(status);
                                    let zero = builder.ins().iconst(status_ty, 0);
                                    let is_done = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                        status,
                                        zero,
                                    );
                                    let done_const = builder.ins().iconst(s_ty, -1);
                                    // Read current state for the false arm.
                                    let cur_state =
                                        builder.ins().load(
                                            s_ty,
                                            MemFlags::new(),
                                            state_ptr,
                                            s_off as i32,
                                        );
                                    let new_state = builder.ins().select(
                                        is_done,
                                        done_const,
                                        cur_state,
                                    );
                                    builder.ins().store(
                                        MemFlags::new(),
                                        new_state,
                                        state_ptr,
                                        s_off as i32,
                                    );
                                } else {
                                    let done = builder.ins().iconst(s_ty, -1);
                                    builder.ins().store(
                                        MemFlags::new(),
                                        done,
                                        state_ptr,
                                        s_off as i32,
                                    );
                                }
                            }

                            // Propagate the dispatcher's return status when
                            // the function was split; otherwise the legacy
                            // always-READY return below still applies.
                            if function_was_split && !results.is_empty() {
                                let r = results[0];
                                let rt = builder.func.dfg.value_type(r);
                                let status = if rt == types::I32 {
                                    r
                                } else if rt.is_int() {
                                    if rt.bits() > 32 {
                                        builder.ins().ireduce(types::I32, r)
                                    } else {
                                        builder.ins().sextend(types::I32, r)
                                    }
                                } else {
                                    builder.ins().iconst(types::I32, 1)
                                };
                                builder.ins().return_(&[status]);
                                builder.seal_all_blocks();
                                builder.finalize();

                                let mut ctx = Context::for_function(cl_func);
                                object_module
                                    .define_function(poll_id, &mut ctx)
                                    .map_err(|e| {
                                        eprintln!(
                                            "[kryos] codegen error in async poll wrapper for '{}': {e}",
                                            mir_func.name
                                        );
                                        CodegenError::Module(e)
                                    })?;
                                continue;
                            }

                            emitted_real_body = true;
                        }
                    }
                }

                // Final return: KRYOS_READY = 1.
                let _ = emitted_real_body; // (kept for readability / future use)
                let ready = builder.ins().iconst(types::I32, 1);
                builder.ins().return_(&[ready]);
                builder.seal_all_blocks();
                builder.finalize();
            }

            let mut ctx = Context::for_function(cl_func);
            object_module
                .define_function(poll_id, &mut ctx)
                .map_err(|e| {
                    eprintln!(
                        "[kryos] codegen error in async poll wrapper for '{}': {e}",
                        mir_func.name
                    );
                    CodegenError::Module(e)
                })?;
        }
    }

    // Generate env-wrapper (thunk) function bodies for closures.
    for (func_name, (num_captures, user_param_count, _)) in &closure_info {
        let env_thunk_id = thunk_ids[func_name.as_str()];
        let call_conv = object_module.isa().default_call_conv();

        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // env
        for _ in 0..*user_param_count {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));

        let mut cl_func =
            Function::with_name_signature(UserFuncName::user(0, env_thunk_id.as_u32()), sig);

        {
            let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);

            let block_params: Vec<_> = builder.block_params(entry).to_vec();
            let env_val = block_params[0];

            // Load captures from env at offsets 8, 16, ...
            let mut call_args: Vec<cranelift_codegen::ir::Value> = Vec::new();
            for i in 0..*num_captures {
                let offset = ((i + 1) * 8) as i32;
                let cap = builder
                    .ins()
                    .load(types::I64, MemFlags::new(), env_val, offset);
                call_args.push(cap);
            }

            // Append user args from thunk parameters (indices 1..)
            for i in 0..*user_param_count {
                call_args.push(block_params[1 + i]);
            }

            // Get reference to the original function.
            let orig_id = func_ids[func_name.as_str()];

            // Coerce args to match original function's signature types.
            let orig_sig = if let Some(f) = mir_func_map.get(func_name.as_str()) {
                build_signature(f, call_conv)
            } else {
                let mut s = Signature::new(call_conv);
                for _ in 0..call_args.len() {
                    s.params.push(AbiParam::new(types::I64));
                }
                s.returns.push(AbiParam::new(types::I64));
                s
            };

            for (i, arg) in call_args.iter_mut().enumerate() {
                if i < orig_sig.params.len() {
                    let expected = orig_sig.params[i].value_type;
                    let actual = builder.func.dfg.value_type(*arg);
                    if expected != actual {
                        if expected.is_int() && actual.is_int() {
                            if expected.bits() < actual.bits() {
                                *arg = builder.ins().ireduce(expected, *arg);
                            } else {
                                *arg = builder.ins().sextend(expected, *arg);
                            }
                        } else if expected.is_float() && actual.is_int() {
                            *arg = builder.ins().bitcast(expected, MemFlags::new(), *arg);
                        } else if expected.is_int() && actual.is_float() {
                            *arg = builder.ins().bitcast(expected, MemFlags::new(), *arg);
                        }
                    }
                }
            }

            // Call original function.
            let orig_ref = object_module.declare_func_in_func(orig_id, builder.func);
            let call_inst = builder.ins().call(orig_ref, &call_args);

            // Widen return value to i64.
            let results = builder.inst_results(call_inst).to_vec();
            let ret_val = if results.is_empty() {
                builder.ins().iconst(types::I64, 0)
            } else {
                let result = results[0];
                let result_ty = builder.func.dfg.value_type(result);
                if result_ty == types::I64 {
                    result
                } else if result_ty.is_int() && result_ty.bits() < 64 {
                    builder.ins().sextend(types::I64, result)
                } else if is_float_type(result_ty) {
                    builder.ins().bitcast(types::I64, MemFlags::new(), result)
                } else {
                    result
                }
            };

            builder.ins().return_(&[ret_val]);
            builder.seal_all_blocks();
            builder.finalize();
        }

        let mut ctx = Context::for_function(cl_func);
        object_module
            .define_function(env_thunk_id, &mut ctx)
            .map_err(CodegenError::Module)?;
    }

    // Generate dropper function bodies for closures with heap captures.
    // Each dropper has signature `fn(env_ptr: i64)` and frees captured heap
    // values at their known offsets before the ARC system frees the env buffer.
    //
    // Pre-declare runtime functions needed by droppers (idempotent if already declared).
    {
        let call_conv = object_module.isa().default_call_conv();
        let one_arg_sig = |cc| {
            let mut sig = Signature::new(cc);
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            sig
        };
        let one_arg_void_sig = |cc| {
            let mut sig = Signature::new(cc);
            sig.params.push(AbiParam::new(types::I64));
            sig
        };
        for (rt_name, is_void) in [
            ("kryos_string_free", true),
            ("kryos_array_free", true),
            ("kryos_map_free", true),
            ("free", true),
        ] {
            if !func_ids.contains_key(rt_name) {
                let sig = if is_void {
                    one_arg_void_sig(call_conv)
                } else {
                    one_arg_sig(call_conv)
                };
                let id = object_module.declare_function(rt_name, Linkage::Import, &sig)?;
                func_ids.insert(rt_name.to_string(), id);
            }
        }
    }
    for (func_name, (_, _, cap_types)) in &closure_info {
        if let Some(&dropper_id) = dropper_ids.get(func_name.as_str()) {
            let call_conv = object_module.isa().default_call_conv();
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // env ptr

            let mut cl_func =
                Function::with_name_signature(UserFuncName::user(0, dropper_id.as_u32()), sig);

            {
                let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);

                let env_ptr = builder.block_params(entry)[0];

                // Free each heap-typed capture at offset (i+1)*8.
                for (i, cap_ty) in cap_types.iter().enumerate() {
                    let offset = ((i + 1) * 8) as i32;
                    let rt_fn_name = match cap_ty {
                        Some(MirType::Str) => Some("kryos_string_free"),
                        Some(MirType::Array(_, _)) => Some("kryos_array_free"),
                        Some(MirType::Map { .. }) => Some("kryos_map_free"),
                        Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                            Some("kryos_arc_release")
                        }
                        Some(MirType::Struct(_)) | Some(MirType::Enum(_)) => Some("free"),
                        _ => None,
                    };
                    if let Some(fn_name) = rt_fn_name {
                        let val = builder
                            .ins()
                            .load(types::I64, MemFlags::new(), env_ptr, offset);
                        let func_id = func_ids[fn_name];
                        let func_ref = object_module.declare_func_in_func(func_id, builder.func);
                        builder.ins().call(func_ref, &[val]);
                    }
                }

                builder.ins().return_(&[]);
                builder.seal_all_blocks();
                builder.finalize();
            }

            let mut ctx = Context::for_function(cl_func);
            object_module
                .define_function(dropper_id, &mut ctx)
                .map_err(CodegenError::Module)?;
        }
    }

    // Generate type drop helper bodies for struct/enum types with heap fields.
    // These enable array element drop to properly clean up nested struct/enum fields.
    for (type_name, &drop_id) in &type_drop_ids {
        let call_conv = object_module.isa().default_call_conv();
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // type ptr

        let mut cl_func =
            Function::with_name_signature(UserFuncName::user(0, drop_id.as_u32()), sig);

        {
            let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let ptr = builder.block_params(entry)[0];

            if let Some(struct_def) = module.struct_defs.get(type_name) {
                // Struct drop: free each heap-owning field, then free the struct.
                if let Ok(layout) = compute_struct_layout(struct_def) {
                    for (field_name, field_ty) in struct_def.iter() {
                        let field_offset = layout
                            .field_offsets
                            .iter()
                            .find(|(n, _, _)| n == field_name)
                            .map(|(_, off, _)| *off as i32);
                        if let Some(offset) = field_offset {
                            let free_fn = match field_ty {
                                MirType::Str => Some("kryos_string_free"),
                                MirType::Array(_, _) => Some("kryos_array_free"),
                                MirType::Map { .. } => Some("kryos_map_free"),
                                MirType::Function { .. } | MirType::Shared(_) => {
                                    Some("kryos_arc_release")
                                }
                                MirType::Struct(n) => {
                                    let dn = format!("__kryos_drop_{n}");
                                    if func_ids.contains_key(&dn) {
                                        // Will resolve below
                                        None
                                    } else {
                                        Some("free")
                                    }
                                }
                                MirType::Enum(n) => {
                                    let dn = format!("__kryos_drop_{n}");
                                    if func_ids.contains_key(&dn) {
                                        None
                                    } else {
                                        Some("free")
                                    }
                                }
                                _ => continue,
                            };
                            let field_val =
                                builder.ins().load(types::I64, MemFlags::new(), ptr, offset);
                            if let Some(fn_name) = free_fn {
                                let func_id = func_ids[fn_name];
                                let func_ref =
                                    object_module.declare_func_in_func(func_id, builder.func);
                                builder.ins().call(func_ref, &[field_val]);
                            } else {
                                // Named type drop helper for nested struct/enum.
                                let nested_name = match field_ty {
                                    MirType::Struct(n) | MirType::Enum(n) => {
                                        format!("__kryos_drop_{n}")
                                    }
                                    _ => unreachable!(),
                                };
                                let nested_id = func_ids[&nested_name];
                                let nested_ref =
                                    object_module.declare_func_in_func(nested_id, builder.func);
                                builder.ins().call(nested_ref, &[field_val]);
                            }
                        }
                    }
                }
            } else if let Some(variants) = module.enum_defs.get(type_name) {
                // Enum drop: load tag, dispatch per-variant, free fields.
                let tag = builder.ins().load(types::I64, MemFlags::new(), ptr, 0);
                let merge_block = builder.create_block();

                for (idx, variant) in variants.iter().enumerate() {
                    let droppable_fields: Vec<(usize, &MirType)> = variant
                        .fields
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| {
                            matches!(
                                f,
                                MirType::Str
                                    | MirType::Array(_, _)
                                    | MirType::Struct(_)
                                    | MirType::Function { .. }
                                    | MirType::Enum(_)
                                    | MirType::Shared(_)
                            )
                        })
                        .collect();
                    if droppable_fields.is_empty() {
                        continue;
                    }

                    let variant_block = builder.create_block();
                    let skip_block = builder.create_block();

                    let tag_val = builder.ins().iconst(types::I64, idx as i64);
                    let cmp = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        tag,
                        tag_val,
                    );
                    builder.ins().brif(cmp, variant_block, &[], skip_block, &[]);
                    builder.seal_block(variant_block);

                    builder.switch_to_block(variant_block);
                    for (field_idx, field_ty) in &droppable_fields {
                        let offset = ((*field_idx + 1) * 8) as i32;
                        let field_val =
                            builder.ins().load(types::I64, MemFlags::new(), ptr, offset);
                        let fn_name = match *field_ty {
                            MirType::Str => "kryos_string_free",
                            MirType::Array(_, _) => "kryos_array_free",
                            MirType::Map { .. } => "kryos_map_free",
                            MirType::Function { .. } | MirType::Shared(_) => "kryos_arc_release",
                            MirType::Struct(ref n) | MirType::Enum(ref n) => {
                                let dn = format!("__kryos_drop_{n}");
                                if func_ids.contains_key(&dn) {
                                    let fid = func_ids[&dn];
                                    let fref =
                                        object_module.declare_func_in_func(fid, builder.func);
                                    builder.ins().call(fref, &[field_val]);
                                    continue;
                                }
                                "free"
                            }
                            _ => continue,
                        };
                        let fid = func_ids[fn_name];
                        let fref = object_module.declare_func_in_func(fid, builder.func);
                        builder.ins().call(fref, &[field_val]);
                    }
                    builder.ins().jump(merge_block, &[]);

                    builder.switch_to_block(skip_block);
                    builder.seal_block(skip_block);
                }
                builder.ins().jump(merge_block, &[]);
                builder.seal_block(merge_block);
                builder.switch_to_block(merge_block);
            }

            // Free the struct/enum allocation itself.
            let free_id = func_ids["free"];
            let free_ref = object_module.declare_func_in_func(free_id, builder.func);
            builder.ins().call(free_ref, &[ptr]);

            builder.ins().return_(&[]);
            builder.seal_all_blocks();
            builder.finalize();
        }

        let mut ctx = Context::for_function(cl_func);
        object_module
            .define_function(drop_id, &mut ctx)
            .map_err(CodegenError::Module)?;
    }

    // ISOLATION-TEST 2: per-field loop body (calloc + load/store each field,
    // clone Str fields, leave others as raw copy). No Array branch, no
    // nested Struct clone call. Tests if per-field iteration is correct.
    for (type_name, &clone_id) in &type_clone_ids {
        let call_conv = object_module.isa().default_call_conv();
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));

        let mut cl_func =
            Function::with_name_signature(UserFuncName::user(0, clone_id.as_u32()), sig);

        {
            let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let src = builder.block_params(entry)[0];

            // Null guard.
            let zero = builder.ins().iconst(types::I64, 0);
            let nonnull = builder.ins().icmp(
                cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                src,
                zero,
            );
            let body_block = builder.create_block();
            let null_block = builder.create_block();
            builder
                .ins()
                .brif(nonnull, body_block, &[], null_block, &[]);
            builder.seal_block(null_block);
            builder.switch_to_block(null_block);
            builder.ins().return_(&[zero]);
            builder.seal_block(body_block);
            builder.switch_to_block(body_block);

            let struct_def = module
                .struct_defs
                .get(type_name)
                .expect("type_clone_ids only contains names from struct_defs");
            let layout = compute_struct_layout(struct_def)?;

            let one_v = builder.ins().iconst(types::I64, 1);
            let size_v = builder.ins().iconst(types::I64, layout.total_size as i64);
            let calloc_id = func_ids["calloc"];
            let calloc_ref = object_module.declare_func_in_func(calloc_id, builder.func);
            let alloc_call = builder.ins().call(calloc_ref, &[one_v, size_v]);
            let dst = builder.inst_results(alloc_call)[0];

            for (field_name, offset, cl_ty) in &layout.field_offsets {
                let field_val = builder
                    .ins()
                    .load(*cl_ty, MemFlags::new(), src, *offset as i32);
                let field_mir_ty = struct_def
                    .iter()
                    .find(|(n, _)| n == field_name)
                    .map(|(_, t)| t);

                let stored_val = match field_mir_ty {
                    Some(MirType::Str) => {
                        let f = func_ids["kryos_string_clone"];
                        let fr = object_module.declare_func_in_func(f, builder.func);
                        let c = builder.ins().call(fr, &[field_val]);
                        builder.inst_results(c)[0]
                    }
                    Some(MirType::Array(_, _)) => {
                        let f = func_ids["kryos_array_clone"];
                        let fr = object_module.declare_func_in_func(f, builder.func);
                        let c = builder.ins().call(fr, &[field_val]);
                        builder.inst_results(c)[0]
                    }
                    Some(MirType::Map { .. }) => {
                        let f = func_ids["kryos_map_clone"];
                        let fr = object_module.declare_func_in_func(f, builder.func);
                        let c = builder.ins().call(fr, &[field_val]);
                        builder.inst_results(c)[0]
                    }
                    Some(MirType::Struct(n)) => {
                        if let Some(&inner_clone_id) = type_clone_ids.get(n) {
                            let fr = object_module
                                .declare_func_in_func(inner_clone_id, builder.func);
                            let c = builder.ins().call(fr, &[field_val]);
                            builder.inst_results(c)[0]
                        } else {
                            field_val
                        }
                    }
                    Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                        let f = func_ids["kryos_arc_retain"];
                        let fr = object_module.declare_func_in_func(f, builder.func);
                        builder.ins().call(fr, &[field_val]);
                        field_val
                    }
                    _ => field_val,
                };

                builder
                    .ins()
                    .store(MemFlags::new(), stored_val, dst, *offset as i32);
            }

            builder.ins().return_(&[dst]);
            builder.seal_all_blocks();
            builder.finalize();
        }

        let mut ctx = Context::for_function(cl_func);
        object_module
            .define_function(clone_id, &mut ctx)
            .map_err(CodegenError::Module)?;
    }

    // If the user's main returns void, emit a C-compatible `main` wrapper:
    //   i32 main() { _kryos_main(); return 0; }
    if needs_main_wrapper {
        let call_conv = object_module.isa().default_call_conv();

        // Declare the exported `main` symbol with C signature: () -> i32.
        let mut main_sig = Signature::new(call_conv);
        main_sig.returns.push(AbiParam::new(types::I32));
        let main_id = object_module.declare_function("main", Linkage::Export, &main_sig)?;

        // Build the wrapper function body.
        let kryos_main_id = func_ids["main"]; // points to _kryos_main
        let mut cl_func = Function::with_name_signature(
            UserFuncName::user(0, main_id.as_u32()),
            main_sig.clone(),
        );

        {
            let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            builder.append_block_params_for_function_params(entry);

            // Initialize runtime (stack guard, panic hook, etc.).
            let init_sig = Signature::new(call_conv);
            let init_id =
                object_module.declare_function("kryos_rt_init", Linkage::Import, &init_sig)?;
            let init_ref = object_module.declare_func_in_func(init_id, builder.func);
            builder.ins().call(init_ref, &[]);

            // Call _kryos_main().
            let main_func = module
                .functions
                .iter()
                .find(|f| f.name == "main")
                .ok_or_else(|| {
                    CodegenError::Internal(
                        "no `fn main()` found — every Kryos program must define a main function"
                            .to_string(),
                    )
                })?;
            let callee_sig = build_signature(main_func, call_conv);
            let callee_sig_ref = builder.import_signature(callee_sig);
            let callee_ref = object_module.declare_func_in_func(kryos_main_id, builder.func);
            let user_call = builder.ins().call(callee_ref, &[]);

            // Preserve the user's main return value (truncated to i32) so a
            // shell-script `$?` reflects what the program intended. Void
            // user-mains fall through to the 0i32 default.
            let user_ret = if !matches!(main_func.ret_ty, MirType::Void) {
                let rets = builder.inst_results(user_call);
                rets.first().copied()
            } else {
                None
            };

            // Wait for all spawned threads before exiting.
            let wait_sig = {
                let mut s = Signature::new(call_conv);
                s.returns.push(AbiParam::new(types::I64));
                s
            };
            let wait_id = object_module.declare_function(
                "kryos_spawn_wait_all",
                Linkage::Import,
                &wait_sig,
            )?;
            let wait_ref = object_module.declare_func_in_func(wait_id, builder.func);
            builder.ins().call(wait_ref, &[]);

            // Return the user's i64 truncated to i32, or 0 if void.
            let exit_code = match user_ret {
                Some(v) => {
                    let actual_ty = builder.func.dfg.value_type(v);
                    if actual_ty == types::I32 {
                        v
                    } else if actual_ty.is_int() && actual_ty.bits() > 32 {
                        builder.ins().ireduce(types::I32, v)
                    } else if actual_ty.is_int() && actual_ty.bits() < 32 {
                        builder.ins().sextend(types::I32, v)
                    } else {
                        builder.ins().iconst(types::I32, 0)
                    }
                }
                None => builder.ins().iconst(types::I32, 0),
            };
            builder.ins().return_(&[exit_code]);

            builder.seal_all_blocks();
            builder.finalize();

            // Suppress unused-variable warning — sig_ref is consumed by the
            // import but not explicitly referenced after; the builder owns it.
            let _ = callee_sig_ref;
        }

        let mut ctx = Context::for_function(cl_func);
        object_module
            .define_function(main_id, &mut ctx)
            .map_err(CodegenError::Module)?;
    }

    let product = object_module.finish();
    let bytes = product
        .emit()
        .map_err(|e| CodegenError::Internal(format!("{e}")))?;
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Signature builder
// ---------------------------------------------------------------------------

/// Build a Cranelift `Signature` from a MIR function.
pub fn build_signature(func: &MirFunction, call_conv: CallConv) -> Signature {
    let mut sig = Signature::new(call_conv);
    for param in &func.params {
        if let Ok(Some(cl_ty)) = mir_type_to_cl(&param.ty) {
            sig.params.push(AbiParam::new(cl_ty));
        }
    }
    if let Ok(Some(ret_ty)) = mir_type_to_cl(&func.ret_ty) {
        sig.returns.push(AbiParam::new(ret_ty));
    }
    sig
}

// ---------------------------------------------------------------------------
// Function translator
// ---------------------------------------------------------------------------

/// State for translating a single MIR function.
struct FuncTranslator<'a> {
    mir_func: &'a MirFunction,
    /// Maps LocalId -> Cranelift Variable.
    variables: HashMap<u32, Variable>,
    /// Maps BlockId -> Cranelift Block.
    blocks: HashMap<u32, cranelift_codegen::ir::Block>,
    /// Declared function refs in the current Cranelift function.
    func_refs: HashMap<String, cranelift_codegen::ir::FuncRef>,
    /// Access to function ID table.
    func_ids: &'a HashMap<String, FuncId>,
    /// Module-level counter for unique string data section names.
    string_counter: &'a mut u32,
    /// Struct definitions for layout computation.
    struct_defs: &'a HashMap<String, Vec<(String, MirType)>>,
    /// Enum definitions for tag/payload codegen.
    enum_defs: &'a HashMap<String, Vec<EnumVariantDef>>,
    /// Persistent stack slots for address-taken locals (used by &/&mut).
    /// Maps LocalId -> StackSlot. The variable and slot are kept in sync:
    /// - Before AddrOf: variable value is stored into the slot
    /// - After calls taking a &mut: slot value is loaded back into variable
    borrow_slots: HashMap<u32, cranelift_codegen::ir::StackSlot>,
    /// Trait method vtable map: (concrete_type, trait_name) -> [method_name, ...].
    mir_module_trait_methods: &'a HashMap<(String, String), Vec<String>>,
    /// Whether this function has MIR-level exception checks (try/catch).
    /// When true, the codegen does NOT emit its own post-call exception
    /// checks because the MIR checks handle routing to catch blocks.
    has_mir_exception_checks: bool,
    /// Emit overflow checks for integer arithmetic.
    checked_arithmetic: bool,
    /// Structs annotated with `@copy` — assignment deep-copies the struct.
    copy_structs: &'a HashSet<String>,
    /// Names of functions defined by the user (or imported user modules).
    /// Used to suppress builtin-name rewriting when a user fn shadows a
    /// builtin of the same name (e.g. user-defined `index_of(arr, target)`
    /// must not be routed to `kryos_builtin_index_of`).
    user_func_names: &'a HashSet<String>,
}

/// Translate a MIR function body into Cranelift IR instructions.
pub fn translate_function<M: Module>(
    mir_func: &MirFunction,
    builder: &mut FunctionBuilder,
    func_ids: &HashMap<String, FuncId>,
    module: &mut M,
    struct_defs: &HashMap<String, Vec<(String, MirType)>>,
    enum_defs: &HashMap<String, Vec<EnumVariantDef>>,
    string_counter: &mut u32,
    trait_vtables: &HashMap<(String, String), Vec<String>>,
    checked_arithmetic: bool,
    copy_structs: &HashSet<String>,
    user_func_names: &HashSet<String>,
) -> Result<(), CodegenError> {
    // Check if this function already contains MIR-level exception checks
    // (from try/catch lowering).  If so, the codegen must NOT add its own
    // post-call return-on-exception guards because they would bypass the
    // catch handler.
    let has_mir_exception_checks = mir_func.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Assign { value: RValue::Call { func, .. }, .. }
                if func == "kryos_exception_check")
        })
    });

    let mut translator = FuncTranslator {
        mir_func,
        variables: HashMap::new(),
        blocks: HashMap::new(),
        func_refs: HashMap::new(),
        func_ids,
        string_counter,
        struct_defs,
        enum_defs,
        borrow_slots: HashMap::new(),
        mir_module_trait_methods: trait_vtables,
        has_mir_exception_checks,
        checked_arithmetic,
        copy_structs,
        user_func_names,
    };

    // Create Cranelift blocks for each MIR basic block.
    for bb in &mir_func.blocks {
        let cl_block = builder.create_block();
        translator.blocks.insert(bb.id.0, cl_block);
    }

    // Declare variables (locals).
    for (idx, local) in mir_func.locals.iter().enumerate() {
        let var = Variable::from_u32(idx as u32);
        let cl_ty = mir_type_to_cl_or_i64(&local.ty)?;
        builder.declare_var(var, cl_ty);
        translator.variables.insert(local.id.0, var);
    }

    // Append block params for the entry block and initialize param locals.
    let entry_block = translator.blocks[&0];
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);

    // Bind function parameters to their local variables.
    for (i, param) in mir_func.params.iter().enumerate() {
        let val = builder.block_params(entry_block)[i];
        let var = translator.variables[&param.local.0];
        builder.def_var(var, val);
    }

    // Initialize non-parameter locals to zero.
    let param_ids: std::collections::HashSet<u32> =
        mir_func.params.iter().map(|p| p.local.0).collect();
    for local in &mir_func.locals {
        if !param_ids.contains(&local.id.0) {
            let cl_ty = mir_type_to_cl_or_i64(&local.ty)?;
            let zero = if is_float_type(cl_ty) {
                if cl_ty == types::F32 {
                    builder.ins().f32const(0.0)
                } else {
                    builder.ins().f64const(0.0)
                }
            } else {
                builder.ins().iconst(cl_ty, 0)
            };
            let var = translator.variables[&local.id.0];
            builder.def_var(var, zero);
        }
    }

    // Emit trace_enter at the start of the function for stack trace support.
    emit_trace_enter(mir_func, builder, &mut translator, module)?;

    // Translate the entry block's instructions (we already switched to it).
    translate_block_body(&mir_func.blocks[0], builder, &mut translator, module)?;

    // Translate remaining blocks.
    for bb in mir_func.blocks.iter().skip(1) {
        let cl_block = translator.blocks[&bb.id.0];
        builder.switch_to_block(cl_block);
        translate_block_body(bb, builder, &mut translator, module)?;
    }

    Ok(())
}

/// Translate a single basic block's instructions and terminator.
fn translate_block_body<M: Module>(
    bb: &BasicBlock,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<(), CodegenError> {
    for instr in &bb.instructions {
        translate_instruction(instr, builder, translator, module)?;
    }
    translate_terminator(&bb.terminator, builder, translator, module)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Instruction translation
// ---------------------------------------------------------------------------

#[allow(clippy::collapsible_match)]
fn translate_instruction<M: Module>(
    instr: &Instruction,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<(), CodegenError> {
    match instr {
        Instruction::Assign { dest, value } => {
            let val = translate_rvalue(value, builder, translator, module, Some(*dest))?;
            if let Some(val) = val {
                let var = translator.variables.get(&dest.0).copied().ok_or_else(|| {
                    CodegenError::Internal(format!("undefined local _{}", dest.0))
                })?;

                // Coerce the value to the declared variable type if they differ
                // (e.g. a call returning I32 assigned to a Void/I64 temp).
                let dest_ty = mir_type_to_cl_or_i64(
                    &translator
                        .mir_func
                        .locals
                        .iter()
                        .find(|l| l.id == *dest)
                        .map(|l| l.ty.clone())
                        .unwrap_or(MirType::I64),
                )?;
                let val_ty = builder.func.dfg.value_type(val);
                let coerced = if val_ty != dest_ty {
                    if is_float_type(dest_ty) && !is_float_type(val_ty) {
                        // Int -> float: bitcast to reinterpret bits as float.
                        // Runtime builtins like kryos_builtin_float return f64
                        // bits packed into an i64 at the C ABI level.
                        builder.ins().bitcast(dest_ty, MemFlags::new(), val)
                    } else if !is_float_type(dest_ty) && is_float_type(val_ty) {
                        // Float -> int: bitcast to pack float bits into an int.
                        builder.ins().bitcast(dest_ty, MemFlags::new(), val)
                    } else if val_ty.bits() < dest_ty.bits() {
                        builder.ins().sextend(dest_ty, val)
                    } else if val_ty.bits() > dest_ty.bits() {
                        builder.ins().ireduce(dest_ty, val)
                    } else {
                        val
                    }
                } else {
                    val
                };
                builder.def_var(var, coerced);
            }

            // After a function call, reload all borrowed locals from their
            // persistent stack slots, since the callee may have mutated them
            // through a &mut reference.
            if matches!(value, RValue::Call { .. } | RValue::CallIndirect { .. }) {
                for (&local_id, &slot) in &translator.borrow_slots {
                    if let Some(&var) = translator.variables.get(&local_id) {
                        let cl_ty = translator
                            .mir_func
                            .locals
                            .iter()
                            .find(|l| l.id.0 == local_id)
                            .and_then(|l| mir_type_to_cl(&l.ty).ok().flatten())
                            .unwrap_or(types::I64);
                        let reloaded = builder.ins().stack_load(cl_ty, slot, 0);
                        builder.def_var(var, reloaded);
                    }
                }
            }

            // For functions WITHOUT MIR-level exception handling (i.e., no
            // try/catch), check the thread-local exception state after every
            // user function call.  If an exception is pending, return
            // immediately to propagate the unwind toward the nearest try/catch
            // up the call stack.
            if !translator.has_mir_exception_checks {
                let should_check = match value {
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
                            )
                    }
                    RValue::CallIndirect { .. } | RValue::VtableCall { .. } => true,
                    _ => false,
                };
                if should_check {
                    let check_ref = ensure_func_ref_with_args(
                        "kryos_exception_check",
                        builder,
                        translator,
                        module,
                        0,
                    )?;
                    let check_call = builder.ins().call(check_ref, &[]);
                    let has_exc = builder.inst_results(check_call)[0];

                    let zero = builder.ins().iconst(types::I64, 0);
                    let is_pending = builder.ins().icmp(IntCC::NotEqual, has_exc, zero);

                    let exc_return_block = builder.create_block();
                    let continue_block = builder.create_block();

                    builder
                        .ins()
                        .brif(is_pending, exc_return_block, &[], continue_block, &[]);

                    // Early-return block: drop live locals, emit trace_exit,
                    // and return a default value to propagate the exception
                    // up the call stack.
                    builder.switch_to_block(exc_return_block);
                    builder.seal_block(exc_return_block);
                    emit_exception_cleanup_drops(builder, translator, module)?;
                    emit_trace_exit(builder, translator, module)?;
                    if builder.func.signature.returns.is_empty() {
                        builder.ins().return_(&[]);
                    } else {
                        let ret_ty = builder.func.signature.returns[0].value_type;
                        let default_ret = if is_float_type(ret_ty) {
                            builder.ins().f64const(0.0)
                        } else {
                            builder.ins().iconst(ret_ty, 0)
                        };
                        builder.ins().return_(&[default_ret]);
                    }

                    // Continue normal execution.
                    builder.switch_to_block(continue_block);
                    builder.seal_block(continue_block);
                }
            }
        }
        Instruction::ArcRetain { ptr } => {
            let func_ref = ensure_func_ref("kryos_arc_retain", builder, translator, module)?;
            let val = builder.use_var(translator.variables[&ptr.0]);
            builder.ins().call(func_ref, &[val]);
        }
        Instruction::ArcRelease { ptr } => {
            let func_ref = ensure_func_ref("kryos_arc_release", builder, translator, module)?;
            let val = builder.use_var(translator.variables[&ptr.0]);
            builder.ins().call(func_ref, &[val]);
        }
        Instruction::Drop { local } => {
            let local_ty = translator
                .mir_func
                .locals
                .iter()
                .find(|l| l.id == *local)
                .map(|l| l.ty.clone());

            if let Some(ref ty) = local_ty {
                if let Some(&var) = translator.variables.get(&local.0) {
                    let val = builder.use_var(var);
                    match ty {
                        kryos_mir::ir::MirType::Str => {
                            let free_ref = ensure_func_ref_with_args(
                                "kryos_string_free",
                                builder,
                                translator,
                                module,
                                1,
                            )?;
                            builder.ins().call(free_ref, &[val]);
                        }
                        kryos_mir::ir::MirType::Function { .. }
                        | kryos_mir::ir::MirType::Shared(_) => {
                            // ARC-managed; release our reference.
                            let release_ref = ensure_func_ref_with_args(
                                "kryos_arc_release",
                                builder,
                                translator,
                                module,
                                1,
                            )?;
                            builder.ins().call(release_ref, &[val]);
                        }
                        kryos_mir::ir::MirType::Struct(ref _sname) => {
                            // Drop the struct via emit_drop_for_value. For @copy
                            // structs the field arrays/strings are retained (ref-
                            // counted), so multi-owner drops correctly decrement
                            // ref_count and free at zero. Previously this was a
                            // no-op for @copy structs ("original owner will
                            // free") — but that produced a leak because no
                            // owner's drop ran, so ref_count never decremented.
                            emit_drop_for_value(val, ty, builder, translator, module)?;
                        }
                        kryos_mir::ir::MirType::Enum(_) => {
                            // Runtime variant-aware Drop: dispatch on tag to
                            // free heap-owning payload fields.
                            emit_drop_for_value(val, ty, builder, translator, module)?;
                        }
                        kryos_mir::ir::MirType::Array(_, _) => {
                            // Drop array elements, then free the array.
                            emit_drop_for_value(val, ty, builder, translator, module)?;
                        }
                        _ => {}
                    }
                }
            }
        }
        Instruction::StoreField {
            object,
            field,
            value,
        } => {
            // Store a value into a struct field at its computed offset.
            let ptr = translate_operand(object, builder, translator, module)?;
            let val = translate_operand(value, builder, translator, module)?;

            // Determine the struct type from the object operand.
            let struct_name = match object {
                Operand::Local(id) => translator
                    .mir_func
                    .locals
                    .iter()
                    .find(|l| l.id == *id)
                    .and_then(|l| match &l.ty {
                        MirType::Struct(name) => Some(name.clone()),
                        _ => None,
                    }),
                _ => None,
            };

            if let Some(name) = struct_name {
                if let Some(struct_def) = translator.struct_defs.get(&name) {
                    let layout = compute_struct_layout(struct_def)?;
                    if let Some((_, offset, cl_ty)) =
                        layout.field_offsets.iter().find(|(n, _, _)| n == field)
                    {
                        // Coerce value to the field's Cranelift type if needed.
                        let val_ty = builder.func.dfg.value_type(val);
                        let coerced = if val_ty != *cl_ty {
                            if is_float_type(val_ty) && !is_float_type(*cl_ty) {
                                builder.ins().bitcast(*cl_ty, MemFlags::new(), val)
                            } else if !is_float_type(val_ty) && is_float_type(*cl_ty) {
                                builder.ins().bitcast(*cl_ty, MemFlags::new(), val)
                            } else if val_ty.bits() < cl_ty.bits() {
                                builder.ins().sextend(*cl_ty, val)
                            } else if val_ty.bits() > cl_ty.bits() {
                                builder.ins().ireduce(*cl_ty, val)
                            } else {
                                val
                            }
                        } else {
                            val
                        };
                        builder
                            .ins()
                            .store(MemFlags::new(), coerced, ptr, *offset as i32);
                    } else {
                        eprintln!(
                            "warning: StoreField '{}' not found in struct '{}'",
                            field, name
                        );
                    }
                } else {
                    eprintln!("warning: StoreField struct '{}' not in struct_defs", name);
                }
            } else {
                // Fallback: store at offset 0 (useful for dynamic/untyped stores).
                eprintln!(
                    "warning: StoreField '{}' on unknown struct type — storing at offset 0",
                    field
                );
                builder.ins().store(MemFlags::new(), val, ptr, 0);
            }
        }
        Instruction::StoreDeref { ptr, value } => {
            // Store a value through a reference/pointer.
            let ptr_val = translate_operand(ptr, builder, translator, module)?;
            let val = translate_operand(value, builder, translator, module)?;
            builder.ins().store(MemFlags::new(), val, ptr_val, 0);
        }
        Instruction::Nop => {}
        Instruction::Spawn { func, args } => {
            // Get the function reference and its address as i64.
            let func_ref =
                ensure_func_ref_with_args(func, builder, translator, module, args.len())?;
            let fn_ptr = builder.ins().func_addr(types::I64, func_ref);

            if args.is_empty() {
                // Zero args: kryos_spawn(fn_ptr, null, 0)
                let null_ptr = builder.ins().iconst(types::I64, 0);
                let zero = builder.ins().iconst(types::I64, 0);
                let spawn_ref =
                    ensure_func_ref_with_args("kryos_spawn", builder, translator, module, 3)?;
                builder.ins().call(spawn_ref, &[fn_ptr, null_ptr, zero]);
            } else {
                // Pack args into a stack slot: [arg0, arg1, ...]
                let slot_size = (args.len() * 8) as u32;
                let slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    slot_size,
                    0,
                ));
                let args_ptr = builder.ins().stack_addr(types::I64, slot, 0);

                for (i, arg_op) in args.iter().enumerate() {
                    let val = translate_operand(arg_op, builder, translator, module)?;
                    // Clone heap-typed args so the spawned thread owns its copies.
                    let arg_ty = match arg_op {
                        Operand::Local(id) => translator
                            .mir_func
                            .locals
                            .iter()
                            .find(|l| l.id == *id)
                            .map(|l| l.ty.clone()),
                        _ => None,
                    };
                    let store_val = match arg_ty.as_ref() {
                        Some(MirType::Str) => {
                            let f = ensure_func_ref_with_args(
                                "kryos_string_clone",
                                builder,
                                translator,
                                module,
                                1,
                            )?;
                            let c = builder.ins().call(f, &[val]);
                            builder.inst_results(c)[0]
                        }
                        Some(MirType::Array(_, _)) => {
                            let f = ensure_func_ref_with_args(
                                "kryos_array_clone",
                                builder,
                                translator,
                                module,
                                1,
                            )?;
                            let c = builder.ins().call(f, &[val]);
                            builder.inst_results(c)[0]
                        }
                        Some(MirType::Map { .. }) => {
                            let f = ensure_func_ref_with_args(
                                "kryos_map_clone",
                                builder,
                                translator,
                                module,
                                1,
                            )?;
                            let c = builder.ins().call(f, &[val]);
                            builder.inst_results(c)[0]
                        }
                        Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                            let f = ensure_func_ref_with_args(
                                "kryos_arc_retain",
                                builder,
                                translator,
                                module,
                                1,
                            )?;
                            builder.ins().call(f, &[val]);
                            val
                        }
                        _ => val,
                    };
                    builder
                        .ins()
                        .store(MemFlags::trusted(), store_val, args_ptr, (i * 8) as i32);
                }

                let count = builder.ins().iconst(types::I64, args.len() as i64);
                let spawn_ref =
                    ensure_func_ref_with_args("kryos_spawn", builder, translator, module, 3)?;
                builder.ins().call(spawn_ref, &[fn_ptr, args_ptr, count]);
            }
        }
        Instruction::Send { channel, value } => {
            let ch_val = builder.use_var(translator.variables[&channel.0]);
            let v_val = builder.use_var(translator.variables[&value.0]);

            // Clone heap-typed values before sending to prevent double-free.
            // The runtime copies the raw i64; without cloning, both sender
            // and receiver hold the same pointer and both will free it.
            let val_ty = translator
                .mir_func
                .locals
                .iter()
                .find(|l| l.id == *value)
                .map(|l| l.ty.clone());

            let send_val = match val_ty.as_ref() {
                Some(MirType::Str) => {
                    let f = ensure_func_ref_with_args(
                        "kryos_string_clone",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    let c = builder.ins().call(f, &[v_val]);
                    builder.inst_results(c)[0]
                }
                Some(MirType::Array(_, _)) => {
                    let f = ensure_func_ref_with_args(
                        "kryos_array_clone",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    let c = builder.ins().call(f, &[v_val]);
                    builder.inst_results(c)[0]
                }
                Some(MirType::Map { .. }) => {
                    let f = ensure_func_ref_with_args(
                        "kryos_map_clone",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    let c = builder.ins().call(f, &[v_val]);
                    builder.inst_results(c)[0]
                }
                Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                    let f = ensure_func_ref_with_args(
                        "kryos_arc_retain",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    builder.ins().call(f, &[v_val]);
                    v_val
                }
                _ => v_val,
            };

            let send_ref =
                ensure_func_ref_with_args("kryos_chan_send_i64", builder, translator, module, 2)?;
            builder.ins().call(send_ref, &[ch_val, send_val]);
        }
        Instruction::Receive { dest, channel } => {
            let ch_val = builder.use_var(translator.variables[&channel.0]);
            let recv_ref =
                ensure_func_ref_with_args("kryos_chan_recv_i64", builder, translator, module, 1)?;
            let call = builder.ins().call(recv_ref, &[ch_val]);
            let result = builder.inst_results(call)[0];
            let var = translator.variables[&dest.0];
            builder.def_var(var, result);
        }
        Instruction::ActorSpawn {
            dest,
            dispatch_fn,
            state,
        } => {
            // Get dispatch function pointer as i64.
            let func_ref = ensure_func_ref_with_args(dispatch_fn, builder, translator, module, 1)?;
            let fn_ptr = builder.ins().func_addr(types::I64, func_ref);
            let state_val = translate_operand(state, builder, translator, module)?;
            // Call kryos_actor_spawn_i64(fn_ptr, state).
            let spawn_ref =
                ensure_func_ref_with_args("kryos_actor_spawn_i64", builder, translator, module, 2)?;
            let call = builder.ins().call(spawn_ref, &[fn_ptr, state_val]);
            let result = builder.inst_results(call)[0];
            let var = translator.variables[&dest.0];
            builder.def_var(var, result);
        }
        Instruction::ActorSend {
            actor,
            handler_tag,
            args,
        } => {
            let actor_val = builder.use_var(translator.variables[&actor.0]);
            // Lock to prevent message interleaving.
            let lock_ref =
                ensure_func_ref_with_args("kryos_actor_lock_i64", builder, translator, module, 1)?;
            builder.ins().call(lock_ref, &[actor_val]);
            // Send handler tag.
            let tag_val = builder.ins().iconst(types::I64, *handler_tag as i64);
            let send_ref =
                ensure_func_ref_with_args("kryos_actor_send_i64", builder, translator, module, 2)?;
            builder.ins().call(send_ref, &[actor_val, tag_val]);
            // Send each argument (clone heap-typed values to prevent double-free).
            for arg in args {
                let val = translate_operand(arg, builder, translator, module)?;
                let arg_ty = match arg {
                    Operand::Local(id) => translator
                        .mir_func
                        .locals
                        .iter()
                        .find(|l| l.id == *id)
                        .map(|l| l.ty.clone()),
                    _ => None,
                };
                let send_val = match arg_ty.as_ref() {
                    Some(MirType::Str) => {
                        let f = ensure_func_ref_with_args(
                            "kryos_string_clone",
                            builder,
                            translator,
                            module,
                            1,
                        )?;
                        let c = builder.ins().call(f, &[val]);
                        builder.inst_results(c)[0]
                    }
                    Some(MirType::Array(_, _)) => {
                        let f = ensure_func_ref_with_args(
                            "kryos_array_clone",
                            builder,
                            translator,
                            module,
                            1,
                        )?;
                        let c = builder.ins().call(f, &[val]);
                        builder.inst_results(c)[0]
                    }
                    Some(MirType::Map { .. }) => {
                        let f = ensure_func_ref_with_args(
                            "kryos_map_clone",
                            builder,
                            translator,
                            module,
                            1,
                        )?;
                        let c = builder.ins().call(f, &[val]);
                        builder.inst_results(c)[0]
                    }
                    Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                        let f = ensure_func_ref_with_args(
                            "kryos_arc_retain",
                            builder,
                            translator,
                            module,
                            1,
                        )?;
                        builder.ins().call(f, &[val]);
                        val
                    }
                    _ => val,
                };
                builder.ins().call(send_ref, &[actor_val, send_val]);
            }
            // Unlock.
            let unlock_ref = ensure_func_ref_with_args(
                "kryos_actor_unlock_i64",
                builder,
                translator,
                module,
                1,
            )?;
            builder.ins().call(unlock_ref, &[actor_val]);
        }
        Instruction::ActorStateLoad {
            dest,
            state_ptr,
            field_offset,
        } => {
            // Load from state_ptr + field_offset * 8.
            let ptr_val = builder.use_var(translator.variables[&state_ptr.0]);
            let offset_bytes = (*field_offset as i32) * 8;
            let loaded = builder
                .ins()
                .load(types::I64, MemFlags::new(), ptr_val, offset_bytes);
            let var = translator.variables[&dest.0];
            builder.def_var(var, loaded);
        }
        Instruction::ActorStateStore {
            state_ptr,
            field_offset,
            value,
        } => {
            // Store value to state_ptr + field_offset * 8.
            let ptr_val = builder.use_var(translator.variables[&state_ptr.0]);
            let val = translate_operand(value, builder, translator, module)?;
            let offset_bytes = (*field_offset as i32) * 8;
            builder
                .ins()
                .store(MemFlags::new(), val, ptr_val, offset_bytes);
        }
    }
    Ok(())
}

/// Coerce an operand to a KryosString handle. If the operand is already a
/// string, return it directly. Otherwise, convert via the appropriate runtime
/// function (int -> kryos_builtin_to_string, float -> kryos_f64_to_string,
/// bool -> kryos_bool_to_string).
fn coerce_to_string<M: Module>(
    operand: &Operand,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    if is_string_operand(operand, &translator.mir_func.locals) {
        return translate_operand(operand, builder, translator, module);
    }
    let mut val = translate_operand(operand, builder, translator, module)?;
    let to_str_fn = if is_bool_operand(operand, &translator.mir_func.locals) {
        let val_ty = builder.func.dfg.value_type(val);
        if val_ty.is_int() && val_ty.bits() < 64 {
            val = builder.ins().sextend(types::I64, val);
        }
        "kryos_bool_to_string"
    } else if is_float_operand(operand, &translator.mir_func.locals) {
        "kryos_f64_to_string"
    } else {
        let val_ty = builder.func.dfg.value_type(val);
        if val_ty.is_int() && val_ty.bits() < 64 {
            val = builder.ins().sextend(types::I64, val);
        }
        "kryos_builtin_to_string"
    };
    let to_str_ref = ensure_func_ref_with_args(to_str_fn, builder, translator, module, 1)?;
    let call = builder.ins().call(to_str_ref, &[val]);
    Ok(builder.inst_results(call)[0])
}

// ---------------------------------------------------------------------------
// RValue translation
// ---------------------------------------------------------------------------

fn translate_rvalue<M: Module>(
    rvalue: &RValue,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
    dest: Option<LocalId>,
) -> Result<Option<cranelift_codegen::ir::Value>, CodegenError> {
    match rvalue {
        RValue::Use(operand) => {
            let val = translate_operand(operand, builder, translator, module)?;

            // If the source is a @copy struct, deep-copy: malloc + field copy.
            if let Operand::Local(id) = operand {
                let struct_name = translator
                    .mir_func
                    .locals
                    .iter()
                    .find(|l| l.id == *id)
                    .and_then(|l| match &l.ty {
                        MirType::Struct(name) => Some(name.clone()),
                        _ => None,
                    });
                if let Some(ref sname) = struct_name {
                    if translator.copy_structs.contains(sname) {
                        if let Some(struct_def) = translator.struct_defs.get(sname).cloned() {
                            let layout = compute_struct_layout(&struct_def)?;
                            let one_val = builder.ins().iconst(types::I64, 1);
                            let size_val =
                                builder.ins().iconst(types::I64, layout.total_size as i64);
                            let calloc_ref = ensure_func_ref_with_args(
                                "calloc", builder, translator, module, 2,
                            )?;
                            let alloc_call = builder.ins().call(calloc_ref, &[one_val, size_val]);
                            let new_ptr = builder.inst_results(alloc_call)[0];

                            // Deep-copy each field: clone heap-allocated fields
                            // (arrays, strings) so each copy owns its own data.
                            for (field_name, offset, cl_ty) in &layout.field_offsets {
                                let field_val = builder.ins().load(
                                    *cl_ty,
                                    MemFlags::new(),
                                    val,
                                    *offset as i32,
                                );
                                // Look up the MIR type for this field.
                                let field_mir_ty = struct_def
                                    .iter()
                                    .find(|(n, _)| n == field_name)
                                    .map(|(_, t)| t);
                                let stored_val = match field_mir_ty {
                                    Some(MirType::Array(_, _)) => {
                                        let clone_ref = ensure_func_ref_with_args(
                                            "kryos_array_clone",
                                            builder,
                                            translator,
                                            module,
                                            1,
                                        )?;
                                        let call = builder.ins().call(clone_ref, &[field_val]);
                                        builder.inst_results(call)[0]
                                    }
                                    Some(MirType::Str) => {
                                        let clone_ref = ensure_func_ref_with_args(
                                            "kryos_string_clone",
                                            builder,
                                            translator,
                                            module,
                                            1,
                                        )?;
                                        let call = builder.ins().call(clone_ref, &[field_val]);
                                        builder.inst_results(call)[0]
                                    }
                                    Some(MirType::Map { .. }) => {
                                        // Deep-clone maps via kryos_map_clone.
                                        let clone_ref = ensure_func_ref_with_args(
                                            "kryos_map_clone",
                                            builder,
                                            translator,
                                            module,
                                            1,
                                        )?;
                                        let call = builder.ins().call(clone_ref, &[field_val]);
                                        builder.inst_results(call)[0]
                                    }
                                    Some(MirType::Struct(inner_name)) => {
                                        if translator.copy_structs.contains(inner_name) {
                                            // Recursively deep-copy nested @copy structs.
                                            if let Some(inner_def) =
                                                translator.struct_defs.get(inner_name).cloned()
                                            {
                                                emit_deep_copy_struct(
                                                    field_val, &inner_def, builder, translator,
                                                    module,
                                                )?
                                            } else {
                                                field_val
                                            }
                                        } else {
                                            field_val
                                        }
                                    }
                                    Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                                        let retain_ref = ensure_func_ref_with_args(
                                            "kryos_arc_retain",
                                            builder,
                                            translator,
                                            module,
                                            1,
                                        )?;
                                        builder.ins().call(retain_ref, &[field_val]);
                                        field_val
                                    }
                                    _ => field_val,
                                };
                                builder.ins().store(
                                    MemFlags::new(),
                                    stored_val,
                                    new_ptr,
                                    *offset as i32,
                                );
                            }

                            return Ok(Some(new_ptr));
                        }
                    }
                }
            }

            Ok(Some(val))
        }

        RValue::ConstInt(n) => {
            // Determine target type from dest local, default to I64.
            let cl_ty = dest
                .and_then(|d| {
                    translator
                        .mir_func
                        .locals
                        .iter()
                        .find(|l| l.id == d)
                        .and_then(|l| mir_type_to_cl(&l.ty).ok().flatten())
                })
                .unwrap_or(types::I64);
            let val = builder.ins().iconst(cl_ty, *n);
            Ok(Some(val))
        }

        RValue::ConstFloat(n) => {
            let cl_ty = dest
                .and_then(|d| {
                    translator
                        .mir_func
                        .locals
                        .iter()
                        .find(|l| l.id == d)
                        .and_then(|l| mir_type_to_cl(&l.ty).ok().flatten())
                })
                .unwrap_or(types::F64);
            let val = if cl_ty == types::F32 {
                builder.ins().f32const(*n as f32)
            } else {
                builder.ins().f64const(*n)
            };
            Ok(Some(val))
        }

        RValue::ConstBool(b) => {
            let val = builder.ins().iconst(types::I8, *b as i64);
            Ok(Some(val))
        }

        RValue::ConstString(s) => {
            // Store the string bytes in the object file's data section with a
            // null terminator, then call kryos_string_new to create a proper
            // KryosString handle. This ensures all string values are uniform
            // KryosString pointers, making concat/len/print work consistently.
            let data_name = format!(".str.{}", translator.string_counter);
            *translator.string_counter += 1;

            let data_id = module
                .declare_data(&data_name, Linkage::Local, false, false)
                .map_err(CodegenError::Module)?;

            let mut data_desc = DataDescription::new();
            let mut bytes = s.as_bytes().to_vec();
            let str_len = bytes.len();
            bytes.push(0); // null terminator
            data_desc.define(bytes.into_boxed_slice());
            module
                .define_data(data_id, &data_desc)
                .map_err(CodegenError::Module)?;

            let gv = module.declare_data_in_func(data_id, builder.func);
            let data_ptr = builder.ins().global_value(types::I64, gv);
            let len_val = builder.ins().iconst(types::I64, str_len as i64);

            // Call kryos_string_new(data_ptr, len) to create a KryosString handle.
            let string_new_ref =
                ensure_func_ref_with_args("kryos_string_new", builder, translator, module, 2)?;
            let call = builder.ins().call(string_new_ref, &[data_ptr, len_val]);
            let handle = builder.inst_results(call)[0];
            Ok(Some(handle))
        }

        RValue::ConstNone => {
            let val = builder.ins().iconst(types::I64, 0);
            Ok(Some(val))
        }

        RValue::BinOp { op, left, right } => {
            let mut lhs = translate_operand(left, builder, translator, module)?;
            let mut rhs = translate_operand(right, builder, translator, module)?;
            let lhs_ty = type_of_operand_hint(left, &translator.mir_func.locals);
            let rhs_ty = type_of_operand_hint(right, &translator.mir_func.locals);
            let is_float = is_float_type(lhs_ty);

            // Coerce integer operands to the same width before the operation.
            if !is_float && !is_float_type(rhs_ty) {
                let lhs_actual = builder.func.dfg.value_type(lhs);
                let rhs_actual = builder.func.dfg.value_type(rhs);
                if lhs_actual != rhs_actual {
                    if lhs_actual.bits() < rhs_actual.bits() {
                        lhs = builder.ins().sextend(rhs_actual, lhs);
                    } else {
                        rhs = builder.ins().sextend(lhs_actual, rhs);
                    }
                }
            }

            // String operations: dispatch to runtime instead of integer ops.
            let is_string = is_string_operand(left, &translator.mir_func.locals)
                || is_string_operand(right, &translator.mir_func.locals);

            if is_string && *op == MirBinOp::Add {
                let concat_ref = ensure_func_ref_with_args(
                    "kryos_string_concat",
                    builder,
                    translator,
                    module,
                    2,
                )?;
                let call = builder.ins().call(concat_ref, &[lhs, rhs]);
                return Ok(Some(builder.inst_results(call)[0]));
            }

            if is_string && (*op == MirBinOp::Eq || *op == MirBinOp::Neq) {
                let eq_ref =
                    ensure_func_ref_with_args("kryos_string_eq", builder, translator, module, 2)?;
                let call = builder.ins().call(eq_ref, &[lhs, rhs]);
                let raw = builder.inst_results(call)[0];
                // kryos_string_eq returns Rust `bool` (1 byte). The declared
                // ABI return type depends on the pipeline: AOT declares i8,
                // JIT declares i64. Normalize to a single-byte boolean so
                // subsequent bitwise ops don't mix widths.
                let raw_ty = builder.func.dfg.value_type(raw);
                let eq_val = if raw_ty == types::I8 {
                    raw
                } else {
                    let masked = builder.ins().band_imm(raw, 1);
                    builder.ins().ireduce(types::I8, masked)
                };
                if *op == MirBinOp::Neq {
                    // Invert the boolean: xor with 1 (both i8).
                    let one = builder.ins().iconst(types::I8, 1);
                    let neq = builder.ins().bxor(eq_val, one);
                    return Ok(Some(neq));
                }
                return Ok(Some(eq_val));
            }

            if is_string
                && matches!(
                    *op,
                    MirBinOp::Lt | MirBinOp::Gt | MirBinOp::LtEq | MirBinOp::GtEq
                )
            {
                let cmp_ref = ensure_func_ref_with_args(
                    "kryos_string_compare",
                    builder,
                    translator,
                    module,
                    2,
                )?;
                let call = builder.ins().call(cmp_ref, &[lhs, rhs]);
                let cmp_val = builder.inst_results(call)[0];
                let zero = builder.ins().iconst(types::I64, 0);
                let icc = match *op {
                    MirBinOp::Lt => IntCC::SignedLessThan,
                    MirBinOp::Gt => IntCC::SignedGreaterThan,
                    MirBinOp::LtEq => IntCC::SignedLessThanOrEqual,
                    MirBinOp::GtEq => IntCC::SignedGreaterThanOrEqual,
                    _ => unreachable!(),
                };
                return Ok(Some(builder.ins().icmp(icc, cmp_val, zero)));
            }

            // Power: dispatch to runtime (kryos_ipow for int, kryos_fpow for float).
            if *op == MirBinOp::Pow {
                let (fn_name, needs_f64_sig) = if is_float {
                    ("kryos_fpow", true)
                } else {
                    ("kryos_ipow", false)
                };
                if needs_f64_sig {
                    let func_ref = ensure_func_ref_f64(fn_name, builder, translator, module, 2)?;
                    let call = builder.ins().call(func_ref, &[lhs, rhs]);
                    return Ok(Some(builder.inst_results(call)[0]));
                } else {
                    let func_ref =
                        ensure_func_ref_with_args(fn_name, builder, translator, module, 2)?;
                    let call = builder.ins().call(func_ref, &[lhs, rhs]);
                    return Ok(Some(builder.inst_results(call)[0]));
                }
            }

            // Float modulo: call runtime kryos_fmod.
            if *op == MirBinOp::Mod && is_float {
                let func_ref = ensure_func_ref_f64("kryos_fmod", builder, translator, module, 2)?;
                let call = builder.ins().call(func_ref, &[lhs, rhs]);
                return Ok(Some(builder.inst_results(call)[0]));
            }

            // Integer division/modulo: emit a runtime div-by-zero check before
            // the actual sdiv/srem instruction. Without this, a zero divisor
            // causes a hardware exception instead of a friendly error message.
            if !is_float && (*op == MirBinOp::Div || *op == MirBinOp::Mod) {
                let check_ref = ensure_func_ref_with_args(
                    "kryos_check_div_zero_i64",
                    builder,
                    translator,
                    module,
                    1,
                )?;
                // Widen rhs to i64 for the runtime check (which uses all-i64 ABI).
                let rhs_ty = builder.func.dfg.value_type(rhs);
                let rhs_wide = if rhs_ty.is_int() && rhs_ty.bits() < 64 {
                    builder.ins().sextend(types::I64, rhs)
                } else {
                    rhs
                };
                builder.ins().call(check_ref, &[rhs_wide]);
            }

            let val = translate_binop(
                *op,
                lhs,
                rhs,
                is_float,
                builder,
                translator.checked_arithmetic,
            )?;
            Ok(Some(val))
        }

        RValue::UnOp { op, operand } => {
            let val = translate_operand(operand, builder, translator, module)?;
            let val_ty = type_of_operand_hint(operand, &translator.mir_func.locals);
            let is_float = is_float_type(val_ty);

            let result = translate_unop(*op, val, is_float, val_ty, builder)?;
            Ok(Some(result))
        }

        RValue::Call { func, args } => {
            // Handle print/println/eprintln specially: all string values are
            // KryosString handles (even constants), and non-string values must
            // be converted to KryosString before printing.
            if matches!(func.as_str(), "println" | "print" | "eprintln") {
                let print_fn = match func.as_str() {
                    "println" => "kryos_println_str",
                    "print" => "kryos_print_str",
                    _ => "kryos_eprintln_str",
                };
                let print_ref =
                    ensure_func_ref_with_args(print_fn, builder, translator, module, 1)?;

                if args.is_empty() {
                    // println() with no args: pass null handle → prints newline.
                    let zero = builder.ins().iconst(types::I64, 0);
                    builder.ins().call(print_ref, &[zero]);
                } else if is_string_operand(&args[0], &translator.mir_func.locals) {
                    // String arg: already a KryosString handle.
                    let val = translate_operand(&args[0], builder, translator, module)?;
                    builder.ins().call(print_ref, &[val]);
                } else {
                    // Non-string arg: convert to string using type-specific runtime.
                    let mut val = translate_operand(&args[0], builder, translator, module)?;
                    let val_ty = builder.func.dfg.value_type(val);

                    let to_str_fn = if is_bool_operand(&args[0], &translator.mir_func.locals) {
                        // Widen i8 bool to i64 for kryos_bool_to_string.
                        if val_ty.is_int() && val_ty.bits() < 64 {
                            val = builder.ins().sextend(types::I64, val);
                        }
                        "kryos_bool_to_string"
                    } else if is_float_operand(&args[0], &translator.mir_func.locals) {
                        "kryos_f64_to_string"
                    } else {
                        // Integer: widen to i64 for kryos_builtin_to_string.
                        if val_ty.is_int() && val_ty.bits() < 64 {
                            val = builder.ins().sextend(types::I64, val);
                        }
                        "kryos_builtin_to_string"
                    };

                    let to_str_ref =
                        ensure_func_ref_with_args(to_str_fn, builder, translator, module, 1)?;
                    let call = builder.ins().call(to_str_ref, &[val]);
                    let handle = builder.inst_results(call)[0];
                    builder.ins().call(print_ref, &[handle]);
                }
                return Ok(None);
            }

            // Handle sleep() specially: convert f64 arg to bits (i64).
            if func == "sleep" && args.len() == 1 {
                let mut val = translate_operand(&args[0], builder, translator, module)?;
                let val_ty = builder.func.dfg.value_type(val);
                // If the operand is a float, bitcast to i64.
                if is_float_type(val_ty) {
                    val = builder.ins().bitcast(types::I64, MemFlags::new(), val);
                }
                // If it's already an integer (e.g. from a variable), assume it holds f64 bits.
                let sleep_ref =
                    ensure_func_ref_with_args("kryos_sleep", builder, translator, module, 1)?;
                builder.ins().call(sleep_ref, &[val]);
                return Ok(None);
            }

            // sleep_ms(millis: i64) — pass i64 directly.
            if func == "sleep_ms" && args.len() == 1 {
                let val = translate_operand(&args[0], builder, translator, module)?;
                let sleep_ref =
                    ensure_func_ref_with_args("kryos_sleep_ms", builder, translator, module, 1)?;
                builder.ins().call(sleep_ref, &[val]);
                return Ok(None);
            }

            // Handle math builtins that map to native Cranelift f64 instructions.
            // sqrt, floor, ceil use Cranelift's native instructions directly;
            // abs dispatches to fabs for floats or integer negation for ints.
            if matches!(func.as_str(), "sqrt" | "floor" | "ceil" | "round" | "abs")
                && args.len() == 1
            {
                let val = translate_operand(&args[0], builder, translator, module)?;
                let val_ty = builder.func.dfg.value_type(val);

                if is_float_type(val_ty) {
                    let result = match func.as_str() {
                        "sqrt" => builder.ins().sqrt(val),
                        "floor" => builder.ins().floor(val),
                        "ceil" => builder.ins().ceil(val),
                        "round" => builder.ins().nearest(val),
                        "abs" => builder.ins().fabs(val),
                        _ => unreachable!(),
                    };
                    return Ok(Some(result));
                } else if func == "abs" {
                    // Integer abs: if val < 0 then -val else val.
                    let zero = builder.ins().iconst(val_ty, 0);
                    let neg = builder.ins().ineg(val);
                    let is_neg = builder.ins().icmp(IntCC::SignedLessThan, val, zero);
                    let result = builder.ins().select(is_neg, neg, val);
                    return Ok(Some(result));
                }
                // For non-float sqrt/floor/ceil, fall through (should be
                // a type error, but let the generic path handle it).
            }

            // Handle min/max builtins using Cranelift comparison + select.
            if matches!(func.as_str(), "min" | "max") && args.len() == 2 {
                let a = translate_operand(&args[0], builder, translator, module)?;
                let b = translate_operand(&args[1], builder, translator, module)?;
                let a_ty = builder.func.dfg.value_type(a);
                if is_float_type(a_ty) {
                    // Float min/max using fcmp + select.
                    let cmp = if func == "min" {
                        FloatCC::LessThan
                    } else {
                        FloatCC::GreaterThan
                    };
                    let cond = builder.ins().fcmp(cmp, a, b);
                    let result = builder.ins().select(cond, a, b);
                    return Ok(Some(result));
                } else {
                    let cmp = if func == "min" {
                        IntCC::SignedLessThan
                    } else {
                        IntCC::SignedGreaterThan
                    };
                    let cond = builder.ins().icmp(cmp, a, b);
                    let result = builder.ins().select(cond, a, b);
                    return Ok(Some(result));
                }
            }

            // Handle f64→f64 math builtins (sin, cos, tan, log, log2, log10)
            // via runtime calls with proper f64 signatures.
            if matches!(
                func.as_str(),
                "sin" | "cos" | "tan" | "log" | "log2" | "log10"
            ) && args.len() == 1
            {
                let val = translate_operand(&args[0], builder, translator, module)?;
                let rt_name = format!("kryos_builtin_{func}");
                let func_ref = ensure_func_ref_f64_f64(&rt_name, builder, translator, module)?;
                let call_inst = builder.ins().call(func_ref, &[val]);
                return Ok(Some(builder.inst_results(call_inst)[0]));
            }

            // Handle int() and float() conversions using Cranelift native instructions.
            if func == "int" && args.len() == 1 {
                let val = translate_operand(&args[0], builder, translator, module)?;
                let val_ty = builder.func.dfg.value_type(val);
                if is_float_type(val_ty) {
                    // f64 → i64: truncate
                    let result = builder.ins().fcvt_to_sint(types::I64, val);
                    return Ok(Some(result));
                }
                // Already an integer — identity.
                return Ok(Some(val));
            }
            if func == "float" && args.len() == 1 {
                if is_string_operand(&args[0], &translator.mir_func.locals) {
                    // float(str) — parse string as f64 via kryos_builtin_parse_float.
                    // That function returns the f64 bits packed as i64; the assignment
                    // site will bitcast to F64.
                    let val = translate_operand(&args[0], builder, translator, module)?;
                    let parse_float_ref = ensure_func_ref_with_args(
                        "kryos_builtin_parse_float",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    let call = builder.ins().call(parse_float_ref, &[val]);
                    return Ok(Some(builder.inst_results(call)[0]));
                }
                let val = translate_operand(&args[0], builder, translator, module)?;
                let val_ty = builder.func.dfg.value_type(val);
                if is_float_type(val_ty) {
                    // Already float — identity.
                    return Ok(Some(val));
                }
                // i64 → f64: convert
                let result = builder.ins().fcvt_from_sint(types::F64, val);
                return Ok(Some(result));
            }

            // Handle to_string() with type dispatch: str → pass-through,
            // float → kryos_f64_to_string, bool → kryos_bool_to_string,
            // int → kryos_builtin_to_string.
            if func == "to_string" && args.len() == 1 {
                // If the argument is already a string, just return it as-is.
                // Without this check, string pointers fall through to
                // kryos_builtin_to_string which formats the pointer address
                // as an integer (e.g. "2353427280336" instead of "hello").
                if is_string_operand(&args[0], &translator.mir_func.locals) {
                    let val = translate_operand(&args[0], builder, translator, module)?;
                    return Ok(Some(val));
                }
                let val = translate_operand(&args[0], builder, translator, module)?;
                if is_float_operand(&args[0], &translator.mir_func.locals) {
                    let f64_ref = ensure_func_ref_with_args(
                        "kryos_f64_to_string",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    let call = builder.ins().call(f64_ref, &[val]);
                    return Ok(Some(builder.inst_results(call)[0]));
                } else if is_bool_operand(&args[0], &translator.mir_func.locals) {
                    let mut v = val;
                    let val_ty = builder.func.dfg.value_type(v);
                    if val_ty.is_int() && val_ty.bits() < 64 {
                        v = builder.ins().sextend(types::I64, v);
                    }
                    let bool_ref = ensure_func_ref_with_args(
                        "kryos_bool_to_string",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    let call = builder.ins().call(bool_ref, &[v]);
                    return Ok(Some(builder.inst_results(call)[0]));
                }
                // Fall through to generic int to_string below.
            }

            // Handle type_of() with compile-time type dispatch.
            // Resolve the type name entirely at compile time for ALL MIR types
            // to avoid Cranelift verifier errors from type mismatches (e.g. passing
            // f64 or i8 to a runtime function expecting i64).
            if func == "type_of" && args.len() == 1 {
                let s = mir_type_name_of_operand(&args[0], &translator.mir_func.locals);

                // Emit the argument for side effects (unused), then return a
                // string constant with the type name.
                let _ = translate_operand(&args[0], builder, translator, module)?;

                let data_name = format!(".str.{}", translator.string_counter);
                *translator.string_counter += 1;

                let data_id = module
                    .declare_data(&data_name, Linkage::Local, false, false)
                    .map_err(CodegenError::Module)?;

                let mut data_desc = DataDescription::new();
                let mut bytes = s.as_bytes().to_vec();
                let str_len = bytes.len();
                bytes.push(0); // null terminator
                data_desc.define(bytes.into_boxed_slice());
                module
                    .define_data(data_id, &data_desc)
                    .map_err(CodegenError::Module)?;

                let gv = module.declare_data_in_func(data_id, builder.func);
                let data_ptr = builder.ins().global_value(types::I64, gv);
                let len_val = builder.ins().iconst(types::I64, str_len as i64);

                let string_new_ref =
                    ensure_func_ref_with_args("kryos_string_new", builder, translator, module, 2)?;
                let call = builder.ins().call(string_new_ref, &[data_ptr, len_val]);
                return Ok(Some(builder.inst_results(call)[0]));
            }

            // Handle assert() with optional message and bool condition support.
            // The runtime function expects (i64, i64) where the second arg is a
            // string message. Support single-arg calls with a default message,
            // and extend bool (i8) conditions to i64.
            if func == "assert" && !args.is_empty() {
                let mut condition = translate_operand(&args[0], builder, translator, module)?;

                // Coerce the condition to i64 for the runtime assert function.
                let cond_ty = builder.func.dfg.value_type(condition);
                if is_float_type(cond_ty) {
                    // Float condition: treat non-zero as truthy (fcmp ne 0.0).
                    let zero = builder.ins().f64const(0.0);
                    let cmp = builder.ins().fcmp(FloatCC::NotEqual, condition, zero);
                    // bint is i8 (0 or 1); extend to i64.
                    condition = builder.ins().uextend(types::I64, cmp);
                } else if cond_ty.is_int() && cond_ty.bits() < 64 {
                    // Bool (i8) or other small int: sign-extend to i64.
                    condition = builder.ins().sextend(types::I64, condition);
                }

                // Get or create the message argument.
                let message = if args.len() >= 2 {
                    translate_operand(&args[1], builder, translator, module)?
                } else {
                    // Create a default "assertion failed" message.
                    let default_msg = "assertion failed";
                    let data_name = format!(".str.{}", translator.string_counter);
                    *translator.string_counter += 1;

                    let data_id = module
                        .declare_data(&data_name, Linkage::Local, false, false)
                        .map_err(CodegenError::Module)?;

                    let mut data_desc = DataDescription::new();
                    let mut bytes = default_msg.as_bytes().to_vec();
                    let str_len = bytes.len();
                    bytes.push(0);
                    data_desc.define(bytes.into_boxed_slice());
                    module
                        .define_data(data_id, &data_desc)
                        .map_err(CodegenError::Module)?;

                    let gv = module.declare_data_in_func(data_id, builder.func);
                    let data_ptr = builder.ins().global_value(types::I64, gv);
                    let len_val = builder.ins().iconst(types::I64, str_len as i64);

                    let string_new_ref = ensure_func_ref_with_args(
                        "kryos_string_new",
                        builder,
                        translator,
                        module,
                        2,
                    )?;
                    let call = builder.ins().call(string_new_ref, &[data_ptr, len_val]);
                    builder.inst_results(call)[0]
                };

                let assert_ref = ensure_func_ref_with_args(
                    "kryos_builtin_assert",
                    builder,
                    translator,
                    module,
                    2,
                )?;
                builder.ins().call(assert_ref, &[condition, message]);
                return Ok(None);
            }

            // assert_eq(left, right) — stringify both args using the same
            // type-aware to_string lowering used for `{x}` interpolation,
            // then forward the two KryosString handles to the runtime.
            // The runtime compares the strings and prints a diff on failure.
            if func == "assert_eq" && args.len() == 2 {
                let mut handles: Vec<cranelift_codegen::ir::Value> = Vec::with_capacity(2);
                for arg in args.iter() {
                    if is_string_operand(arg, &translator.mir_func.locals) {
                        let val = translate_operand(arg, builder, translator, module)?;
                        handles.push(val);
                        continue;
                    }
                    let val = translate_operand(arg, builder, translator, module)?;
                    if is_float_operand(arg, &translator.mir_func.locals) {
                        let f64_ref = ensure_func_ref_with_args(
                            "kryos_f64_to_string",
                            builder,
                            translator,
                            module,
                            1,
                        )?;
                        let call = builder.ins().call(f64_ref, &[val]);
                        handles.push(builder.inst_results(call)[0]);
                    } else if is_bool_operand(arg, &translator.mir_func.locals) {
                        let mut v = val;
                        let val_ty = builder.func.dfg.value_type(v);
                        if val_ty.is_int() && val_ty.bits() < 64 {
                            v = builder.ins().sextend(types::I64, v);
                        }
                        let bool_ref = ensure_func_ref_with_args(
                            "kryos_bool_to_string",
                            builder,
                            translator,
                            module,
                            1,
                        )?;
                        let call = builder.ins().call(bool_ref, &[v]);
                        handles.push(builder.inst_results(call)[0]);
                    } else {
                        // Default: stringify as i64.
                        let mut v = val;
                        let val_ty = builder.func.dfg.value_type(v);
                        if val_ty.is_int() && val_ty.bits() < 64 {
                            v = builder.ins().sextend(types::I64, v);
                        }
                        let i_ref = ensure_func_ref_with_args(
                            "kryos_i64_to_string",
                            builder,
                            translator,
                            module,
                            1,
                        )?;
                        let call = builder.ins().call(i_ref, &[v]);
                        handles.push(builder.inst_results(call)[0]);
                    }
                }
                let assert_eq_ref = ensure_func_ref_with_args(
                    "kryos_builtin_assert_eq",
                    builder,
                    translator,
                    module,
                    2,
                )?;
                builder.ins().call(assert_eq_ref, &handles);
                return Ok(None);
            }

            // panic(msg: str) — abort the process with a user message.
            // Lowering: forward the single string argument to
            // kryos_builtin_panic, which never returns at runtime.
            if func == "panic" && !args.is_empty() {
                let message = translate_operand(&args[0], builder, translator, module)?;
                let panic_ref = ensure_func_ref_with_args(
                    "kryos_builtin_panic",
                    builder,
                    translator,
                    module,
                    1,
                )?;
                builder.ins().call(panic_ref, &[message]);
                return Ok(None);
            }

            // If the user has defined a function with this exact name, the
            // user definition shadows any builtin of the same name. Skip the
            // builtin map entirely and dispatch directly to the user function
            // (which is already registered in func_ids under its plain name).
            //
            // Without this guard, e.g. `fn index_of(arr: [str], target: str)`
            // would silently be routed to `kryos_builtin_index_of(s, sub)`
            // because the builtin map below unconditionally rewrites the
            // function name.
            let user_shadows_builtin = translator.user_func_names.contains(func.as_str());

            // Map Kryos builtin names to their runtime function names.
            let (runtime_name, runtime_arg_count) = if user_shadows_builtin {
                (func.as_str(), args.len())
            } else { match func.as_str() {
                "chan" => ("kryos_chan_new_i64", 0usize),
                "send" => ("kryos_chan_send_i64", 2),
                "recv" => ("kryos_chan_recv_i64", 1),
                "file_read" => ("kryos_builtin_file_read", 1),
                "file_write" => ("kryos_builtin_file_write", 2),
                // Legacy aliases (LLVM codegen uses these too)
                "read_file" => ("kryos_builtin_file_read", 1),
                "write_file" => ("kryos_builtin_file_write", 2),
                "env_get" => ("kryos_builtin_env_get", 1),
                "time_now" => ("kryos_builtin_time_now", 0),
                "parse_int" => ("kryos_builtin_parse_int", 1),
                "parse_float" => ("kryos_builtin_parse_float", 1),
                "type_of" => ("kryos_builtin_type_of", 1),
                "char_code" => ("kryos_builtin_char_code", 1),
                "char_from" => ("kryos_builtin_char_from", 1),
                "substr" => ("kryos_builtin_substr", 3),
                "contains" => ("kryos_builtin_contains", 2),
                "starts_with" => ("kryos_builtin_starts_with", 2),
                "ends_with" => ("kryos_builtin_ends_with", 2),
                "trim" => ("kryos_builtin_trim", 1),
                "to_upper" => ("kryos_builtin_to_upper", 1),
                "to_lower" => ("kryos_builtin_to_lower", 1),
                "replace" => ("kryos_builtin_replace", 3),
                "split" => ("kryos_builtin_split", 2),
                "join" => ("kryos_builtin_join", 2),
                "push" => ("kryos_builtin_push", 2),
                "pop" => ("kryos_builtin_pop", 1),
                "int" => ("kryos_builtin_int", 1),
                "float" => ("kryos_builtin_float", 1),
                "buf_new" => ("kryos_buf_new", 1),
                "buf_write_byte" => ("kryos_buf_write_byte", 2),
                "buf_write_i16_le" => ("kryos_buf_write_i16_le", 2),
                "buf_write_i32_le" => ("kryos_buf_write_i32_le", 2),
                "buf_write_i64_le" => ("kryos_buf_write_i64_le", 2),
                "buf_write_bytes" => ("kryos_buf_write_bytes", 3),
                "buf_write_str" => ("kryos_buf_write_str", 2),
                "buf_write_zeros" => ("kryos_buf_write_zeros", 2),
                "buf_len" => ("kryos_buf_len", 1),
                "buf_get_byte" => ("kryos_buf_get_byte", 2),
                "buf_set_byte" => ("kryos_buf_set_byte", 3),
                "buf_patch_i32_le" => ("kryos_buf_patch_i32_le", 3),
                "buf_patch_i64_le" => ("kryos_buf_patch_i64_le", 3),
                "buf_write_to_file" => ("kryos_buf_write_to_file", 2),
                "buf_free" => ("kryos_buf_free", 1),
                "exit" => ("kryos_builtin_exit", 1),
                "args" => ("kryos_builtin_args", 0),
                "map_has" => ("kryos_map_has", 2),
                "map_has_str" => ("kryos_map_has_str", 2),
                "map_delete" => ("kryos_map_delete", 2),
                "map_delete_str" => ("kryos_map_delete_str", 2),
                "map_keys" => ("kryos_map_keys", 1),
                "map_keys_str" => ("kryos_map_keys_str", 1),
                "read_line" => ("kryos_builtin_read_line", 0),
                "file_exists" => ("kryos_builtin_file_exists", 1),
                "file_size" => ("kryos_builtin_file_size", 1),
                "create_dir" => ("kryos_builtin_create_dir", 1),
                "trim_start" => ("kryos_builtin_trim_start", 1),
                "trim_end" => ("kryos_builtin_trim_end", 1),
                "index_of" => ("kryos_builtin_index_of", 2),
                "sort" => ("kryos_builtin_sort", 1),
                "reverse" => ("kryos_builtin_reverse", 1),
                "append_file" => ("kryos_builtin_file_append", 2),
                "abs" => ("kryos_builtin_abs", 1),
                "abs_f" => ("kryos_builtin_abs_f", 1),
                "sqrt" => ("kryos_builtin_sqrt", 1),
                "floor" => ("kryos_builtin_floor", 1),
                "ceil" => ("kryos_builtin_ceil", 1),
                "pow" => ("kryos_builtin_pow", 2),
                "min" => ("kryos_builtin_min", 2),
                "max" => ("kryos_builtin_max", 2),
                "min_f" => ("kryos_builtin_min_f", 2),
                "max_f" => ("kryos_builtin_max_f", 2),
                "http_get" => ("kryos_builtin_http_get", 1),
                "tcp_connect" => ("kryos_tcp_connect_ks", 2),
                "tcp_listen" => ("kryos_tcp_bind_ks", 2),
                "tcp_accept" => ("kryos_tcp_accept", 1),
                "tcp_send" => ("kryos_tcp_send_ks", 2),
                "tcp_recv" => ("kryos_tcp_recv_ks", 2),
                "tcp_close" => ("kryos_socket_close_ks", 1),
                "tcp_set_nonblocking" => ("kryos_tcp_set_nonblocking", 2),
                "tcp_try_accept" => ("kryos_tcp_try_accept", 1),
                "tcp_try_recv" => ("kryos_tcp_try_recv_ks", 2),
                // TLS server (Gap A)
                "tls_server_config" => ("kryos_tls_server_config_ks", 2),
                "tls_accept" => ("kryos_tls_accept", 2),
                "tls_send" => ("kryos_tls_send_ks", 2),
                "tls_recv" => ("kryos_tls_recv_ks", 2),
                "tls_close" => ("kryos_tls_close_ks", 1),
                // Unix domain sockets (v2.0)
                "uds_connect" => ("kryos_uds_connect_ks", 1),
                "uds_bind" => ("kryos_uds_bind_ks", 1),
                "uds_accept" => ("kryos_uds_accept", 1),
                "uds_send" => ("kryos_uds_send_ks", 2),
                "uds_recv" => ("kryos_uds_recv_ks", 2),
                "uds_close" => ("kryos_uds_close", 1),
                // WebSocket helpers (RFC 6455) (v2.0)
                "ws_accept_key" => ("kryos_ws_accept_key_ks", 1),
                "ws_encode_text" => ("kryos_ws_encode_text_ks", 1),
                "ws_encode_binary" => ("kryos_ws_encode_binary_ks", 1),
                "ws_encode_close" => ("kryos_ws_encode_close", 1),
                "ws_encode_ping" => ("kryos_ws_encode_ping_ks", 1),
                "ws_encode_pong" => ("kryos_ws_encode_pong_ks", 1),
                "ws_unmask" => ("kryos_ws_unmask_ks", 4),
                "ws_read_frame" => ("kryos_ws_read_frame_ks", 1),
                // PostgreSQL (Gap B)
                "pg_connect" => ("kryos_pg_connect_ks", 1),
                "pg_exec" => ("kryos_pg_exec_ks", 2),
                "pg_query" => ("kryos_pg_query_ks", 2),
                "pg_close" => ("kryos_pg_close_ks", 1),
                "sleep_ms" => ("kryos_sleep_ms", 1),
                // JSON (handles are i64; all string args are i64 KryosString handles).
                "json_parse" => ("kryos_json_parse", 1),
                "json_stringify" => ("kryos_json_stringify", 1),
                "json_object" => ("kryos_json_object", 2),
                "json_array" => ("kryos_json_array", 1),
                "json_string" => ("kryos_json_string", 1),
                "json_number" => ("kryos_json_number", 1),
                "json_bool" => ("kryos_json_bool", 1),
                "json_null" => ("kryos_json_null", 0),
                "json_get" => ("kryos_json_get", 2),
                "json_get_index" => ("kryos_json_get_index", 2),
                "json_to_str" => ("kryos_json_to_str", 1),
                "json_to_int" => ("kryos_json_to_int", 1),
                "json_to_float" => ("kryos_json_to_float", 1),
                "json_is_null" => ("kryos_json_is_null", 1),
                "json_length" => ("kryos_json_length", 1),
                "json_type" => ("kryos_json_type", 1),
                // Crypto / hashing
                "sha256" => ("kryos_sha256_ks", 1),
                "sha512" => ("kryos_sha512_ks", 1),
                "random_bytes" => ("kryos_random_bytes_ks", 1),
                "sha1_hex" => ("kryos_sha1_hex_ks", 1),
                "sha1_base64" => ("kryos_sha1_base64_ks", 1),
                "base64_encode" => ("kryos_base64_encode_ks", 1),
                "base64_decode" => ("kryos_base64_decode_ks", 1),
                "chr" => ("kryos_chr_ks", 1),
                "byte_at" => ("kryos_byte_at_ks", 2),
                // Time
                "time_now_secs" => ("kryos_time_now_secs", 0),
                "time_now_millis" => ("kryos_time_now_millis", 0),
                // Mutex
                "mutex_new" => ("kryos_mutex_new", 0),
                "mutex_lock" => ("kryos_mutex_lock", 1),
                "mutex_unlock" => ("kryos_mutex_unlock", 1),
                "mutex_drop" => ("kryos_mutex_drop", 1),
                // Regex
                "regex_new" => ("kryos_regex_new_ks", 1),
                "regex_match" => ("kryos_regex_is_match_ks", 2),
                "regex_find" => ("kryos_regex_find_ks", 2),
                "regex_find_pos" => ("kryos_regex_find_pos_ks", 3),
                "regex_find_end" => ("kryos_regex_find_end_ks", 3),
                "regex_replace_all" => ("kryos_regex_replace_all_ks", 3),
                "regex_drop" => ("kryos_regex_drop_ks", 1),
                // HTTP/HTTPS request
                "http_request" => ("kryos_http_request_ks", 5),
                "https_get" => ("kryos_https_get_ks", 1),
                // HTTP/2 client (Gap C)
                "http2_get" => ("kryos_http2_get_ks", 1),
                "http2_post" => ("kryos_http2_post_ks", 2),
                "http2_request" => ("kryos_http2_request_ks", 4),
                // WASM v0.4 web builtins (native fallbacks).
                "dom_set_text" => ("kryos_dom_set_text_ks", 2),
                "dom_get_value" => ("kryos_dom_get_value_ks", 1),
                "alert" => ("kryos_alert_ks", 1),
                "canvas_fill_rect" => ("kryos_canvas_fill_rect_ks", 6),
                "canvas_clear" => ("kryos_canvas_clear_ks", 1),
                "fetch_text" => ("kryos_fetch_text_ks", 1),
                // Overflow-aware integer arithmetic.
                "wrapping_add" => ("kryos_wrapping_add_i64", 2),
                "wrapping_sub" => ("kryos_wrapping_sub_i64", 2),
                "wrapping_mul" => ("kryos_wrapping_mul_i64", 2),
                "checked_add" => ("kryos_checked_add_i64", 2),
                "checked_sub" => ("kryos_checked_sub_i64", 2),
                "checked_mul" => ("kryos_checked_mul_i64", 2),
                "saturating_add" => ("kryos_saturating_add_i64", 2),
                "saturating_sub" => ("kryos_saturating_sub_i64", 2),
                "saturating_mul" => ("kryos_saturating_mul_i64", 2),
                _ => (func.as_str(), args.len()),
            } };

            // Translate argument operands first
            let mut arg_vals: Vec<cranelift_codegen::ir::Value> = args
                .iter()
                .map(|a| translate_operand(a, builder, translator, module))
                .collect::<Result<_, _>>()?;

            // Use appropriate function reference based on the builtin type
            let func_ref = match func.as_str() {
                // f64 → f64 single-arg functions
                "abs_f" | "sqrt" | "floor" | "ceil" => {
                    ensure_func_ref_f64_f64(runtime_name, builder, translator, module)?
                }
                // f64, f64 → f64 two-arg functions
                "pow" | "min_f" | "max_f" => {
                    ensure_func_ref_f64_f64_f64(runtime_name, builder, translator, module)?
                }
                // f64 → i64 single-arg functions (JSON node constructor for
                // numbers). The f64 arg must be passed in the float register
                // class, so we declare a real (F64) -> I64 signature instead
                // of the default all-i64 one.
                "json_number" => {
                    ensure_func_ref_f64_i64(runtime_name, builder, translator, module)?
                }
                // i64 → f64 single-arg functions (JSON node accessor that
                // returns an actual float value).
                "json_to_float" => {
                    ensure_func_ref_i64_f64(runtime_name, builder, translator, module)?
                }
                // All other functions: standard i64-based
                _ => ensure_func_ref_with_args(
                    runtime_name,
                    builder,
                    translator,
                    module,
                    runtime_arg_count,
                )?,
            };

            // Widen arguments to match the callee's expected parameter types.
            // This handles cases like passing an i32 to a function expecting i64.
            let sig = builder.func.dfg.ext_funcs[func_ref].signature;
            let param_types: Vec<Type> = builder.func.dfg.signatures[sig]
                .params
                .iter()
                .map(|p| p.value_type)
                .collect();
            for (i, arg) in arg_vals.iter_mut().enumerate() {
                if let Some(&expected_ty) = param_types.get(i) {
                    let actual_ty = builder.func.dfg.value_type(*arg);
                    if actual_ty != expected_ty {
                        if is_float_type(actual_ty) && !is_float_type(expected_ty) {
                            // Float → int: bitcast to preserve bits (e.g. f64 stored as i64).
                            *arg = builder.ins().bitcast(expected_ty, MemFlags::new(), *arg);
                        } else if !is_float_type(actual_ty) && is_float_type(expected_ty) {
                            // Int → float: bitcast to interpret bits as float.
                            *arg = builder.ins().bitcast(expected_ty, MemFlags::new(), *arg);
                        } else if !is_float_type(actual_ty) && !is_float_type(expected_ty) {
                            if actual_ty.bits() < expected_ty.bits() {
                                *arg = builder.ins().sextend(expected_ty, *arg);
                            } else if actual_ty.bits() > expected_ty.bits() {
                                *arg = builder.ins().ireduce(expected_ty, *arg);
                            }
                        }
                    }
                }
            }

            let call_inst = builder.ins().call(func_ref, &arg_vals);
            let results = builder.inst_results(call_inst);
            if results.is_empty() {
                Ok(None)
            } else {
                Ok(Some(results[0]))
            }
        }

        RValue::CallIndirect { callee, args } => {
            // Indirect call via env-based calling convention.
            // The callee is an env pointer: [thunk_fn_ptr, cap0, cap1, ...]
            // Load the thunk function pointer from env[0], then call
            // thunk(env, user_arg0, user_arg1, ...).
            let env_ptr = translate_operand(callee, builder, translator, module)?;
            let fn_ptr = builder.ins().load(types::I64, MemFlags::new(), env_ptr, 0);

            // Build argument values: [env_ptr, user_args...]
            let mut arg_vals: Vec<cranelift_codegen::ir::Value> = vec![env_ptr];
            for arg in args.iter() {
                let val = translate_operand(arg, builder, translator, module)?;
                arg_vals.push(val);
            }

            // Widen all arguments to i64.
            for arg in arg_vals.iter_mut() {
                let ty = builder.func.dfg.value_type(*arg);
                if ty.is_int() && ty.bits() < 64 {
                    *arg = builder.ins().sextend(types::I64, *arg);
                }
            }

            // Build an all-i64 signature: (env, user_args...) -> i64.
            let call_conv = module.isa().default_call_conv();
            let mut sig = Signature::new(call_conv);
            for _ in 0..arg_vals.len() {
                sig.params.push(AbiParam::new(types::I64));
            }
            sig.returns.push(AbiParam::new(types::I64));
            let sig_ref = builder.import_signature(sig);

            let call_inst = builder.ins().call_indirect(sig_ref, fn_ptr, &arg_vals);
            let results = builder.inst_results(call_inst);
            if results.is_empty() {
                Ok(None)
            } else {
                Ok(Some(results[0]))
            }
        }

        RValue::Array(elems) => {
            // Create a KryosArray via runtime API for consistency with
            // runtime functions (args, push, len, kryos_array_get).
            let elem_size_val = builder.ins().iconst(types::I64, 8);
            let cap_val = builder.ins().iconst(types::I64, elems.len().max(1) as i64);
            let new_ref =
                ensure_func_ref_with_args("kryos_array_new", builder, translator, module, 2)?;
            let call = builder.ins().call(new_ref, &[elem_size_val, cap_val]);
            let arr_ptr = builder.inst_results(call)[0];

            // Push each element.
            let push_ref =
                ensure_func_ref_with_args("kryos_array_push", builder, translator, module, 2)?;
            for elem in elems.iter() {
                let val = translate_operand(elem, builder, translator, module)?;
                // Widen or bitcast value to i64 for the push function.
                let val_ty = builder.func.dfg.value_type(val);
                let val_i64 = if is_float_type(val_ty) {
                    builder.ins().bitcast(types::I64, MemFlags::new(), val)
                } else if val_ty.is_int() && val_ty.bits() < 64 {
                    builder.ins().sextend(types::I64, val)
                } else {
                    val
                };
                builder.ins().call(push_ref, &[arr_ptr, val_i64]);
            }

            Ok(Some(arr_ptr))
        }

        RValue::Tuple(elems) => {
            // Use KryosArray for tuples (same runtime representation as arrays).
            let elem_size_val = builder.ins().iconst(types::I64, 8);
            let cap_val = builder.ins().iconst(types::I64, elems.len().max(1) as i64);
            let new_ref =
                ensure_func_ref_with_args("kryos_array_new", builder, translator, module, 2)?;
            let call = builder.ins().call(new_ref, &[elem_size_val, cap_val]);
            let arr_ptr = builder.inst_results(call)[0];

            let push_ref =
                ensure_func_ref_with_args("kryos_array_push", builder, translator, module, 2)?;
            for elem in elems.iter() {
                let val = translate_operand(elem, builder, translator, module)?;
                let val_ty = builder.func.dfg.value_type(val);
                let val_i64 = if is_float_type(val_ty) {
                    builder.ins().bitcast(types::I64, MemFlags::new(), val)
                } else if val_ty.is_int() && val_ty.bits() < 64 {
                    builder.ins().sextend(types::I64, val)
                } else {
                    val
                };
                builder.ins().call(push_ref, &[arr_ptr, val_i64]);
            }

            Ok(Some(arr_ptr))
        }

        RValue::Struct { name, fields } => {
            // Look up the struct definition to compute its memory layout.
            if let Some(struct_def) = translator.struct_defs.get(name).cloned() {
                let layout = compute_struct_layout(&struct_def)?;
                let is_copy = translator.copy_structs.contains(name);

                // Heap-allocate the struct via calloc (zero-initialized) so
                // fields not explicitly set in the struct literal are null.
                // This is critical for @copy structs: emit_deep_copy_struct
                // calls kryos_array_clone/kryos_string_clone on every field,
                // and both functions require either a valid pointer or null (0).
                // malloc leaves unset fields as garbage → heap corruption.
                let one_val = builder.ins().iconst(types::I64, 1);
                let size_val = builder.ins().iconst(types::I64, layout.total_size as i64);
                let calloc_ref =
                    ensure_func_ref_with_args("calloc", builder, translator, module, 2)?;
                let call = builder.ins().call(calloc_ref, &[one_val, size_val]);
                let ptr = builder.inst_results(call)[0];

                // Store each field value at its computed offset.
                // For @copy structs, clone heap-allocated fields (arrays,
                // strings) so the new struct owns its own copies and is
                // safe to drop independently.
                for (field_name, operand) in fields {
                    if let Some((_, offset, cl_ty)) = layout
                        .field_offsets
                        .iter()
                        .find(|(n, _, _)| n == field_name)
                    {
                        let val = translate_operand(operand, builder, translator, module)?;
                        let stored_val = if is_copy {
                            let field_mir_ty = struct_def
                                .iter()
                                .find(|(n, _)| n == field_name)
                                .map(|(_, t)| t);
                            match field_mir_ty {
                                Some(MirType::Array(inner_ty, _)) => {
                                    // Per-element deep clone whitelist (shift 18 H1 finding).
                                    // Token is the only struct empirically validated to be
                                    // safe for deep clone via the runtime helper. Expanded
                                    // whitelists (16 candidates) regressed bootstrap mean
                                    // 12.6 -> 11.8/16 — picked up some struct that does
                                    // depend on identity-shared semantics. Token alone is
                                    // the safest minimum.
                                    let safe_for_deep_clone = |n: &str| {
                                        matches!(n, "Token")
                                    };
                                    match &**inner_ty {
                                        MirType::Str => emit_array_str_deep_clone(
                                            val, builder, translator, module,
                                        )?,
                                        MirType::Struct(inner_name)
                                            if safe_for_deep_clone(inner_name)
                                                && translator.func_ids.contains_key(
                                                    &format!("__kryos_clone_{inner_name}"),
                                                ) =>
                                        {
                                            let elem_fn_name =
                                                format!("__kryos_clone_{inner_name}");
                                            emit_array_clone_deep_call(
                                                val,
                                                &elem_fn_name,
                                                builder,
                                                translator,
                                                module,
                                            )?
                                        }
                                        _ => {
                                            // H8 (shift step 24): retain semantics for non-deep-clone
                                            // Array fallback. Eliminates element-pointer double-free by
                                            // sharing the underlying array via ref counting. Trade-off:
                                            // @copy struct field mutation post-clone affects both sides.
                                            // Stage-1 mostly produces new arrays rather than mutating
                                            // existing ones, so this should be safe in practice.
                                            // H4 sentinel detection remains as a tripwire.
                                            let retain_ref = ensure_func_ref_with_args(
                                                "kryos_array_retain",
                                                builder,
                                                translator,
                                                module,
                                                1,
                                            )?;
                                            let c = builder.ins().call(retain_ref, &[val]);
                                            builder.inst_results(c)[0]
                                        }
                                    }
                                }
                                Some(MirType::Str) => {
                                    let clone_ref = ensure_func_ref_with_args(
                                        "kryos_string_clone",
                                        builder,
                                        translator,
                                        module,
                                        1,
                                    )?;
                                    let c = builder.ins().call(clone_ref, &[val]);
                                    builder.inst_results(c)[0]
                                }
                                Some(MirType::Struct(inner_name)) => {
                                    if translator.copy_structs.contains(inner_name) {
                                        if let Some(inner_def) =
                                            translator.struct_defs.get(inner_name).cloned()
                                        {
                                            emit_deep_copy_struct(
                                                val, &inner_def, builder, translator, module,
                                            )?
                                        } else {
                                            val
                                        }
                                    } else {
                                        val
                                    }
                                }
                                Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                                    let retain_ref = ensure_func_ref_with_args(
                                        "kryos_arc_retain",
                                        builder,
                                        translator,
                                        module,
                                        1,
                                    )?;
                                    builder.ins().call(retain_ref, &[val]);
                                    val
                                }
                                _ => val,
                            }
                        } else {
                            // Non-copy struct: retain reference-counted fields so
                            // both the source and destination own the allocation
                            // and `kryos_array_free` only frees when the last
                            // owner drops it.
                            //
                            // For strings (which are not ref-counted), clone so each
                            // struct field owns an independent allocation. Otherwise
                            // aliased sources (e.g. `let o = ident(p); Box{a: p, b: o}`
                            // where `ident` returns its argument) put the same pointer
                            // in two fields and double-free on drop.
                            let field_mir_ty = struct_def
                                .iter()
                                .find(|(n, _)| n == field_name)
                                .map(|(_, t)| t);
                            match field_mir_ty {
                                Some(MirType::Array(_, _)) => {
                                    let retain_ref = ensure_func_ref_with_args(
                                        "kryos_array_retain",
                                        builder,
                                        translator,
                                        module,
                                        1,
                                    )?;
                                    let c = builder.ins().call(retain_ref, &[val]);
                                    builder.inst_results(c)[0]
                                }
                                Some(MirType::Str) => {
                                    let clone_ref = ensure_func_ref_with_args(
                                        "kryos_string_clone",
                                        builder,
                                        translator,
                                        module,
                                        1,
                                    )?;
                                    let c = builder.ins().call(clone_ref, &[val]);
                                    builder.inst_results(c)[0]
                                }
                                _ => val,
                            }
                        };
                        // Coerce stored_val to the field's Cranelift type before
                        // storing. translate_operand emits Constant::Int as I64
                        // regardless of dest field width, so a narrow field (i32,
                        // i16, i8, bool) would receive an 8-byte store into a
                        // 4/2/1-byte slot — overrunning past the calloc'd struct
                        // and corrupting adjacent heap. This mirrors the coercion
                        // already done in Instruction::StoreField and Instruction::Assign.
                        let val_ty = builder.func.dfg.value_type(stored_val);
                        let coerced = if val_ty != *cl_ty {
                            if is_float_type(val_ty) && !is_float_type(*cl_ty) {
                                builder.ins().bitcast(*cl_ty, MemFlags::new(), stored_val)
                            } else if !is_float_type(val_ty) && is_float_type(*cl_ty) {
                                builder.ins().bitcast(*cl_ty, MemFlags::new(), stored_val)
                            } else if val_ty.bits() < cl_ty.bits() {
                                builder.ins().sextend(*cl_ty, stored_val)
                            } else if val_ty.bits() > cl_ty.bits() {
                                builder.ins().ireduce(*cl_ty, stored_val)
                            } else {
                                stored_val
                            }
                        } else {
                            stored_val
                        };
                        builder
                            .ins()
                            .store(MemFlags::new(), coerced, ptr, *offset as i32);
                    }
                }

                Ok(Some(ptr))
            } else {
                // Unknown struct — fall back to a zero pointer.
                let val = builder.ins().iconst(types::I64, 0);
                Ok(Some(val))
            }
        }

        RValue::Field { object, field } => {
            let ptr = translate_operand(object, builder, translator, module)?;

            // Determine the struct type name from the object operand's local type.
            let struct_name = match object {
                Operand::Local(id) => translator
                    .mir_func
                    .locals
                    .iter()
                    .find(|l| l.id == *id)
                    .and_then(|l| match &l.ty {
                        MirType::Struct(name) => Some(name.clone()),
                        _ => None,
                    }),
                _ => None,
            };

            if let Some(name) = struct_name {
                if let Some(struct_def) = translator.struct_defs.get(&name).cloned() {
                    let layout = compute_struct_layout(&struct_def)?;
                    if let Some((_, offset, cl_ty)) =
                        layout.field_offsets.iter().find(|(n, _, _)| n == field)
                    {
                        let val = builder
                            .ins()
                            .load(*cl_ty, MemFlags::new(), ptr, *offset as i32);
                        // If the field is a @copy struct, deep-copy it so
                        // the extracted value is independent from the parent.
                        // Without this, dropping the parent would free the
                        // shared heap data (arrays, strings) still aliased
                        // by the extracted field value.
                        let field_mir_ty = struct_def
                            .iter()
                            .find(|(n, _)| n == field)
                            .map(|(_, t)| t.clone());
                        if let Some(MirType::Struct(ref inner_name)) = field_mir_ty {
                            if translator.copy_structs.contains(inner_name) {
                                if let Some(inner_def) =
                                    translator.struct_defs.get(inner_name).cloned()
                                {
                                    let copied = emit_deep_copy_struct(
                                        val, &inner_def, builder, translator, module,
                                    )?;
                                    return Ok(Some(copied));
                                }
                            }
                        }
                        return Ok(Some(val));
                    }
                }
            }

            // Tuple field access: tuples are stored as KryosArray at runtime.
            // Field names are "0", "1", "2", ... — parse as integer index.
            let tuple_idx = match object {
                Operand::Local(id) => translator
                    .mir_func
                    .locals
                    .iter()
                    .find(|l| l.id == *id)
                    .and_then(|l| match &l.ty {
                        MirType::Tuple(_) => field.parse::<i64>().ok(),
                        _ => None,
                    }),
                _ => None,
            };
            if let Some(idx) = tuple_idx {
                let idx_val = builder.ins().iconst(types::I64, idx);
                let get_ref =
                    ensure_func_ref_with_args("kryos_array_get", builder, translator, module, 2)?;
                let call = builder.ins().call(get_ref, &[ptr, idx_val]);
                return Ok(Some(builder.inst_results(call)[0]));
            }

            // Heuristic: when the object's type is opaque (e.g. MirType::I64 from
            // a map index whose element type wasn't propagated), scan all known
            // struct definitions for one that contains the requested field.
            // For ambiguous cases (field name shared across multiple structs),
            // scan the entire function body for ALL field accesses on the same
            // local and use the combined set to uniquely identify the struct.
            if let Operand::Local(obj_id) = object {
                let obj_id = *obj_id;

                // Collect all field names accessed on this local across the function.
                let mut accessed_fields: Vec<String> = vec![field.clone()];
                for bb in &translator.mir_func.blocks {
                    for instr in &bb.instructions {
                        if let Instruction::Assign {
                            value:
                                RValue::Field {
                                    object: Operand::Local(id),
                                    field: f,
                                },
                            ..
                        } = instr
                        {
                            if *id == obj_id && f != field {
                                accessed_fields.push(f.clone());
                            }
                        }
                    }
                }
                accessed_fields.sort();
                accessed_fields.dedup();

                // Find structs that have ALL the accessed field names.
                let mut candidates: Vec<(String, Vec<(String, MirType)>)> = translator
                    .struct_defs
                    .iter()
                    .filter(|(_, fields)| {
                        accessed_fields
                            .iter()
                            .all(|f| fields.iter().any(|(n, _)| n == f))
                    })
                    .map(|(name, fields)| (name.clone(), fields.clone()))
                    .collect();

                if candidates.len() == 1 {
                    let (_, struct_def) = candidates.remove(0);
                    let layout = compute_struct_layout(&struct_def)?;
                    if let Some((_, offset, cl_ty)) =
                        layout.field_offsets.iter().find(|(n, _, _)| n == field)
                    {
                        let val = builder
                            .ins()
                            .load(*cl_ty, MemFlags::new(), ptr, *offset as i32);
                        return Ok(Some(val));
                    }
                }
            }

            // Fallback for unknown structs or fields: emit a warning and
            // return typed zero. This should not happen in well-typed programs;
            // if it does, the type checker or struct propagation has a gap.
            eprintln!(
                "warning: codegen fallback for field access '{}' on unknown struct — returning zero",
                field
            );
            let cl_ty = dest
                .and_then(|d| {
                    translator
                        .mir_func
                        .locals
                        .iter()
                        .find(|l| l.id == d)
                        .and_then(|l| mir_type_to_cl(&l.ty).ok().flatten())
                })
                .unwrap_or(types::I64);
            let val = if is_float_type(cl_ty) {
                if cl_ty == types::F32 {
                    builder.ins().f32const(0.0)
                } else {
                    builder.ins().f64const(0.0)
                }
            } else {
                builder.ins().iconst(cl_ty, 0)
            };
            Ok(Some(val))
        }

        RValue::Index { object, index } => {
            // Use the runtime kryos_array_get(arr, idx) function which
            // handles both KryosArray heap objects and bounds checking.
            let ptr = translate_operand(object, builder, translator, module)?;
            let idx_raw = translate_operand(index, builder, translator, module)?;
            // Widen index to i64 if it's i32 (e.g. struct field used as index).
            let idx_ty = builder.func.dfg.value_type(idx_raw);
            let idx = if idx_ty.is_int() && idx_ty.bits() < 64 {
                builder.ins().sextend(types::I64, idx_raw)
            } else {
                idx_raw
            };
            let get_ref =
                ensure_func_ref_with_args("kryos_array_get", builder, translator, module, 2)?;
            let call = builder.ins().call(get_ref, &[ptr, idx]);
            let raw_result = builder.inst_results(call)[0];

            // If the array element type is heap-owning, retain (or clone for
            // strings) so the reader becomes an additional owner. Without this,
            // reading a struct/enum/nested-array out of an array would share a
            // raw pointer with the array; when either side drops, the other
            // sees freed memory.
            let result = if let Operand::Local(id) = object {
                if let Some(local) = translator.mir_func.locals.iter().find(|l| l.id == *id) {
                    if let MirType::Array(elem_ty, _) = &local.ty {
                        match elem_ty.as_ref() {
                            MirType::Str => {
                                let clone_ref = ensure_func_ref_with_args(
                                    "kryos_string_clone",
                                    builder,
                                    translator,
                                    module,
                                    1,
                                )?;
                                let clone_call = builder.ins().call(clone_ref, &[raw_result]);
                                builder.inst_results(clone_call)[0]
                            }
                            MirType::Array(_, _) => {
                                let retain_ref = ensure_func_ref_with_args(
                                    "kryos_array_retain",
                                    builder,
                                    translator,
                                    module,
                                    1,
                                )?;
                                builder.ins().call(retain_ref, &[raw_result]);
                                raw_result
                            }
                            MirType::Function { .. } | MirType::Shared(_) => {
                                let retain_ref = ensure_func_ref_with_args(
                                    "kryos_arc_retain",
                                    builder,
                                    translator,
                                    module,
                                    1,
                                )?;
                                builder.ins().call(retain_ref, &[raw_result]);
                                raw_result
                            }
                            // Struct/Enum elements are malloc'd and freed via
                            // free(), not ARC. Reading a pointer out of the
                            // array shares it with the array; the array's own
                            // drop only frees its elements when refcount==1,
                            // so views remain valid while the array lives.
                            // For independent copies, mark the struct @copy.
                            _ => raw_result,
                        }
                    } else {
                        raw_result
                    }
                } else {
                    raw_result
                }
            } else {
                raw_result
            };

            Ok(Some(result))
        }

        RValue::ArcAlloc { inner } => {
            // shared <expr>: allocate ARC storage, store the value, return ptr.
            // 1. Evaluate the inner expression.
            let val = translate_operand(inner, builder, translator, module)?;
            // 2. Allocate 8 bytes (one i64 slot) via kryos_arc_alloc_i64(8).
            let func_ref = ensure_func_ref("kryos_arc_alloc", builder, translator, module)?;
            let size = builder.ins().iconst(types::I64, 8);
            let call_inst = builder.ins().call(func_ref, &[size]);
            let ptr = builder.inst_results(call_inst)[0];
            // 3. Store the inner value at the allocated pointer.
            builder.ins().store(MemFlags::new(), val, ptr, 0);
            Ok(Some(ptr))
        }

        RValue::EnumVariant {
            enum_name,
            variant_idx,
            fields,
        } => {
            // Enum layout: [tag: i64, field0: i64, field1: i64, ...]
            // All fields are stored as i64 (8 bytes each) for uniform layout.
            //
            // Heap-allocate via malloc so the pointer survives across function
            // returns (stack slots are invalidated when the frame is popped,
            // causing dangling-pointer crashes for returned enums).
            let max_fields = translator
                .enum_defs
                .get(enum_name.as_str())
                .map(|vs| vs.iter().map(|v| v.fields.len()).max().unwrap_or(0))
                .unwrap_or(0);
            let total_size = (1 + max_fields) as u32 * 8;

            let size_val = builder.ins().iconst(types::I64, total_size as i64);
            let malloc_ref = ensure_func_ref_with_args("malloc", builder, translator, module, 1)?;
            let call = builder.ins().call(malloc_ref, &[size_val]);
            let ptr = builder.inst_results(call)[0];

            // Store tag at offset 0.
            let tag = builder.ins().iconst(types::I64, *variant_idx as i64);
            builder.ins().store(MemFlags::trusted(), tag, ptr, 0);

            // Store payload fields at offsets 8, 16, 24, ...
            for (i, field_op) in fields.iter().enumerate() {
                let val = translate_operand(field_op, builder, translator, module)?;
                let offset = ((i + 1) * 8) as i32;
                builder.ins().store(MemFlags::trusted(), val, ptr, offset);
            }

            Ok(Some(ptr))
        }

        RValue::EnumTag { operand } => {
            // Load the tag (i64) from offset 0 of the enum value pointer.
            let ptr = translate_operand(operand, builder, translator, module)?;
            let tag = builder.ins().load(types::I64, MemFlags::new(), ptr, 0);
            Ok(Some(tag))
        }

        RValue::EnumPayload {
            operand,
            enum_name,
            variant_idx,
            field_idx,
        } => {
            // Load the field value from offset (1 + field_idx) * 8.
            let ptr = translate_operand(operand, builder, translator, module)?;
            let offset = ((field_idx + 1) * 8) as i32;

            // Look up the actual field type from enum_defs so we load with
            // the correct Cranelift type (e.g. f64 instead of i64).
            let cl_ty = translator
                .enum_defs
                .get(enum_name.as_str())
                .and_then(|variants| variants.get(*variant_idx as usize))
                .and_then(|variant| variant.fields.get(*field_idx as usize))
                .map(|mir_ty| match mir_ty {
                    MirType::F64 => types::F64,
                    MirType::F32 => types::F32,
                    MirType::Bool => types::I8,
                    _ => types::I64,
                })
                .unwrap_or(types::I64);
            let val = builder.ins().load(cl_ty, MemFlags::new(), ptr, offset);
            // For heap-owning payload types, retain so the match binding becomes
            // an additional owner alongside the enum. Previously we zeroed the
            // slot (pseudo-move), which broke `match p { ... } ; match p { ... }`
            // patterns where the enum is matched more than once. Retain-on-read
            // mirrors how struct field access works: the enum keeps its own
            // reference to the payload, the binding gets its own, and both are
            // released independently when their respective owners drop.
            let payload_mir_ty = translator
                .enum_defs
                .get(enum_name.as_str())
                .and_then(|variants| variants.get(*variant_idx as usize))
                .and_then(|variant| variant.fields.get(*field_idx as usize))
                .cloned();
            match payload_mir_ty {
                Some(MirType::Array(_, _)) => {
                    let retain_ref = ensure_func_ref_with_args(
                        "kryos_array_retain",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    builder.ins().call(retain_ref, &[val]);
                }
                Some(MirType::Str) => {
                    let clone_ref = ensure_func_ref_with_args(
                        "kryos_string_clone",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    let c = builder.ins().call(clone_ref, &[val]);
                    let cloned = builder.inst_results(c)[0];
                    // String clone returns a fresh handle; use it in place of val.
                    let _ = cloned; // falls through -- handle via override below
                                    // Overwrite val with cloned handle for downstream use.
                                    // (Cranelift SSA: cannot reassign val, so restructure.)
                    let widened = if cl_ty == types::I8 {
                        builder.ins().uextend(types::I64, cloned)
                    } else {
                        cloned
                    };
                    return Ok(Some(widened));
                }
                Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                    let retain_ref = ensure_func_ref_with_args(
                        "kryos_arc_retain",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    builder.ins().call(retain_ref, &[val]);
                }
                // Struct/Enum payloads are malloc'd and freed with free(), not
                // ARC. We leave the pointer shared and rely on the destructure
                // not explicitly dropping -- the enum's own Drop will free.
                // Reading the same enum twice from the same variant works since
                // we no longer zero the slot; nested mutation through both
                // aliases would clash, but Kryos doesn't expose that.
                _ => {}
            }
            // Widen bools back to i64 for consistency with the rest of codegen.
            let val = if cl_ty == types::I8 {
                builder.ins().uextend(types::I64, val)
            } else {
                val
            };
            Ok(Some(val))
        }

        RValue::Cast { operand, ty } => {
            let val = translate_operand(operand, builder, translator, module)?;
            let src_ty = type_of_operand_hint(operand, &translator.mir_func.locals);
            let dest_ty = mir_type_to_cl(ty)?.unwrap_or(types::I64);
            let result = translate_cast(val, src_ty, dest_ty, builder)?;
            Ok(Some(result))
        }

        RValue::Closure {
            func_name,
            captures,
        } => {
            // Uniform env-based calling convention for ALL function values.
            // Layout: [thunk_fn_ptr, cap0, cap1, ...]
            // CallIndirect loads fn from env[0] and calls thunk(env, user_args...).
            // The thunk unpacks captures and calls the original function.

            let env_thunk_name = format!("{func_name}_env");
            let has_thunk = translator.func_ids.contains_key(&env_thunk_name);

            if has_thunk {
                // Allocate env via ARC: [thunk_ptr, cap0, cap1, ...]
                let env_slots = 1 + captures.len();
                let env_size = (env_slots * 8) as i64;
                let size_val = builder.ins().iconst(types::I64, env_size);
                let arc_alloc_ref =
                    ensure_func_ref("kryos_arc_alloc", builder, translator, module)?;
                let call = builder.ins().call(arc_alloc_ref, &[size_val]);
                let ptr = builder.inst_results(call)[0];

                // Store thunk function pointer at offset 0.
                let thunk_ref = ensure_func_ref_with_args(
                    &env_thunk_name,
                    builder,
                    translator,
                    module,
                    1 + captures.len(),
                )?;
                let thunk_addr = builder.ins().func_addr(types::I64, thunk_ref);
                builder.ins().store(MemFlags::new(), thunk_addr, ptr, 0);

                // Store captures at offsets 8, 16, ...
                // Clone/retain heap-typed captures so the closure owns them
                // independently of the original local's lifetime.
                for (i, cap) in captures.iter().enumerate() {
                    let val = translate_operand(cap, builder, translator, module)?;
                    let offset = ((i + 1) * 8) as i32;

                    let cap_ty = match cap {
                        Operand::Local(id) => translator
                            .mir_func
                            .locals
                            .iter()
                            .find(|l| l.id == *id)
                            .map(|l| l.ty.clone()),
                        _ => None,
                    };

                    let store_val = match cap_ty.as_ref() {
                        Some(MirType::Str) => {
                            let f = ensure_func_ref_with_args(
                                "kryos_string_clone",
                                builder,
                                translator,
                                module,
                                1,
                            )?;
                            let c = builder.ins().call(f, &[val]);
                            builder.inst_results(c)[0]
                        }
                        Some(MirType::Array(_, _)) => {
                            let f = ensure_func_ref_with_args(
                                "kryos_array_clone",
                                builder,
                                translator,
                                module,
                                1,
                            )?;
                            let c = builder.ins().call(f, &[val]);
                            builder.inst_results(c)[0]
                        }
                        Some(MirType::Map { .. }) => {
                            let f = ensure_func_ref_with_args(
                                "kryos_map_clone",
                                builder,
                                translator,
                                module,
                                1,
                            )?;
                            let c = builder.ins().call(f, &[val]);
                            builder.inst_results(c)[0]
                        }
                        Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                            let f = ensure_func_ref_with_args(
                                "kryos_arc_retain",
                                builder,
                                translator,
                                module,
                                1,
                            )?;
                            builder.ins().call(f, &[val]);
                            val
                        }
                        _ => val,
                    };

                    builder.ins().store(MemFlags::new(), store_val, ptr, offset);
                }

                // Register a dropper function so captured heap values are freed
                // when the closure's ARC ref count reaches zero.
                let dropper_name = format!("{func_name}_drop");
                if translator.func_ids.contains_key(&dropper_name) {
                    let set_drop_ref = ensure_func_ref_with_args(
                        "kryos_arc_set_drop",
                        builder,
                        translator,
                        module,
                        2,
                    )?;
                    let dropper_ref =
                        ensure_func_ref_with_args(&dropper_name, builder, translator, module, 1)?;
                    let dropper_addr = builder.ins().func_addr(types::I64, dropper_ref);
                    builder.ins().call(set_drop_ref, &[ptr, dropper_addr]);
                }

                Ok(Some(ptr))
            } else {
                // No thunk generated — raw function pointer (built-in).
                let func_ref = ensure_func_ref_with_args(
                    func_name,
                    builder,
                    translator,
                    module,
                    captures.len(),
                )?;
                Ok(Some(builder.ins().func_addr(types::I64, func_ref)))
            }
        }

        RValue::Map(entries) => {
            // Create map via kryos_map_new, then insert each entry.
            let new_ref =
                ensure_func_ref_with_args("kryos_map_new", builder, translator, module, 0)?;
            let call = builder.ins().call(new_ref, &[]);
            let map_handle = builder.inst_results(call)[0];

            if !entries.is_empty() {
                // Use string-aware insert if keys are strings.
                let has_str_keys = entries
                    .first()
                    .map(|(k, _)| is_string_operand(k, &translator.mir_func.locals))
                    .unwrap_or(false);
                let insert_fn = if has_str_keys {
                    "kryos_map_insert_str"
                } else {
                    "kryos_map_insert"
                };
                let insert_ref =
                    ensure_func_ref_with_args(insert_fn, builder, translator, module, 3)?;
                for (k, v) in entries {
                    let key_val = translate_operand(k, builder, translator, module)?;
                    let val_val = translate_operand(v, builder, translator, module)?;
                    builder
                        .ins()
                        .call(insert_ref, &[map_handle, key_val, val_val]);
                }
            }

            Ok(Some(map_handle))
        }

        RValue::StringConcat(parts) => {
            if parts.is_empty() {
                let val = builder.ins().iconst(types::I64, 0);
                Ok(Some(val))
            } else if parts.len() == 1 {
                let val = coerce_to_string(&parts[0], builder, translator, module)?;
                Ok(Some(val))
            } else {
                // Fold: acc = concat(parts[0], parts[1]), then concat(acc, parts[2]), ...
                let func_ref = ensure_func_ref("kryos_string_concat", builder, translator, module)?;
                let first = coerce_to_string(&parts[0], builder, translator, module)?;
                let second = coerce_to_string(&parts[1], builder, translator, module)?;
                let call = builder.ins().call(func_ref, &[first, second]);
                let mut acc = builder.inst_results(call)[0];
                let free_ref =
                    ensure_func_ref_with_args("kryos_string_free", builder, translator, module, 1)?;
                for part in &parts[2..] {
                    let next_val = coerce_to_string(part, builder, translator, module)?;
                    let old_acc = acc;
                    let call = builder.ins().call(func_ref, &[acc, next_val]);
                    acc = builder.inst_results(call)[0];
                    // Free the intermediate concat result that was just replaced.
                    builder.ins().call(free_ref, &[old_acc]);
                }
                Ok(Some(acc))
            }
        }

        RValue::Range {
            start,
            end,
            inclusive,
        } => {
            // Range layout: [start: i64, end: i64, inclusive: i64] — 24 bytes.
            // Heap-allocate so the pointer survives across function returns.
            let size_val = builder.ins().iconst(types::I64, 24);
            let malloc_ref = ensure_func_ref_with_args("malloc", builder, translator, module, 1)?;
            let call = builder.ins().call(malloc_ref, &[size_val]);
            let ptr = builder.inst_results(call)[0];

            let start_val = match start {
                Some(op) => translate_operand(op, builder, translator, module)?,
                None => builder.ins().iconst(types::I64, 0),
            };
            builder.ins().store(MemFlags::trusted(), start_val, ptr, 0);

            let end_val = match end {
                Some(op) => translate_operand(op, builder, translator, module)?,
                None => builder.ins().iconst(types::I64, i64::MAX),
            };
            builder.ins().store(MemFlags::trusted(), end_val, ptr, 8);

            let incl_val = builder.ins().iconst(types::I64, *inclusive as i64);
            builder.ins().store(MemFlags::trusted(), incl_val, ptr, 16);

            Ok(Some(ptr))
        }

        RValue::AddrOf { local, mutable: _ } => {
            // Take the address of a local variable.
            // Use a persistent borrow slot so mutations through the pointer
            // are visible when we reload the variable.
            let cl_ty = translator
                .mir_func
                .locals
                .iter()
                .find(|l| l.id == *local)
                .and_then(|l| mir_type_to_cl(&l.ty).ok().flatten())
                .unwrap_or(types::I64);
            let slot = if let Some(existing) = translator.borrow_slots.get(&local.0) {
                *existing
            } else {
                let slot_size = if cl_ty == types::I128 { 16 } else { 8 };
                let new_slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    slot_size,
                    0,
                ));
                translator.borrow_slots.insert(local.0, new_slot);
                new_slot
            };
            // Store the current value of the local into the slot.
            let val = builder.use_var(translator.variables[&local.0]);
            builder.ins().stack_store(val, slot, 0);
            // Return the address of the slot.
            let addr = builder.ins().stack_addr(types::I64, slot, 0);
            Ok(Some(addr))
        }

        RValue::Deref { operand } => {
            // Load from a reference/pointer.
            let ptr = translate_operand(operand, builder, translator, module)?;
            // Determine the target type from the dest local.
            let cl_ty = dest
                .and_then(|d| {
                    translator
                        .mir_func
                        .locals
                        .iter()
                        .find(|l| l.id == d)
                        .and_then(|l| mir_type_to_cl(&l.ty).ok().flatten())
                })
                .unwrap_or(types::I64);
            let val = builder.ins().load(cl_ty, MemFlags::new(), ptr, 0);
            Ok(Some(val))
        }

        RValue::Comptime(inner) => {
            // Comptime: lower inner RValue directly.
            translate_rvalue(inner, builder, translator, module, dest)
        }

        RValue::MakeTraitObject {
            value,
            concrete_type,
            trait_name,
        } => {
            // Dynamic dispatch: heap-allocate a fat pointer [data, vtable_entry...].
            // Heap allocation ensures the fat pointer survives the current scope.
            let data_val = translate_operand(value, builder, translator, module)?;

            // Look up the trait method(s) for this concrete type.
            let vtable_key = (concrete_type.clone(), trait_name.clone());
            let method_names = translator
                .mir_module_trait_methods
                .get(&vtable_key)
                .cloned()
                .unwrap_or_default();

            // Heap-allocate fat pointer: [data (i64), fn_ptr_0, fn_ptr_1, ...]
            let num_methods = method_names.len().max(1) as u32;
            let alloc_size = (8 + 8 * num_methods) as i64;

            let malloc_ref = ensure_func_ref_with_args("malloc", builder, translator, module, 1)?;
            let size_val = builder.ins().iconst(types::I64, alloc_size);
            let call_inst = builder.ins().call(malloc_ref, &[size_val]);
            let fat_ptr = builder.inst_results(call_inst)[0];

            // Store the data value.
            builder.ins().store(MemFlags::new(), data_val, fat_ptr, 0);

            // Store each method function pointer.
            for (i, method_name) in method_names.iter().enumerate() {
                let func_ref =
                    ensure_func_ref_with_args(method_name, builder, translator, module, 1)?;
                let fn_addr = builder.ins().func_addr(types::I64, func_ref);
                builder
                    .ins()
                    .store(MemFlags::new(), fn_addr, fat_ptr, 8 + 8 * i as i32);
            }

            Ok(Some(fat_ptr))
        }

        RValue::VtableCall {
            object,
            method_index,
            args,
            return_ty,
        } => {
            // Dynamic dispatch: load data and method fn_ptr from the fat pointer.
            let fat_ptr = translate_operand(object, builder, translator, module)?;

            // Load the data pointer (offset 0).
            let data_ptr = builder.ins().load(types::I64, MemFlags::new(), fat_ptr, 0);
            // Load the method fn_ptr (offset 8 + 8 * method_index).
            let fn_offset = 8 + 8 * (*method_index as i32);
            let fn_ptr = builder
                .ins()
                .load(types::I64, MemFlags::new(), fat_ptr, fn_offset);

            // Build argument list: data_ptr (self) + any extra args.
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // self/data
            for _ in args {
                sig.params.push(AbiParam::new(types::I64));
            }

            // Use the actual return type from the trait method signature.
            let ret_cranelift_ty = match return_ty {
                kryos_mir::ir::MirType::F64 => types::F64,
                kryos_mir::ir::MirType::Bool => types::I64, // bools are i64 at ABI level
                kryos_mir::ir::MirType::Void => types::I64, // void methods still return 0
                _ => types::I64, // i64, str (pointer), struct (pointer), etc.
            };
            let is_void = matches!(return_ty, kryos_mir::ir::MirType::Void);
            if !is_void {
                sig.returns.push(AbiParam::new(ret_cranelift_ty));
            }

            let sig_ref = builder.import_signature(sig);
            let mut call_args = vec![data_ptr];
            for arg in args {
                let a = translate_operand(arg, builder, translator, module)?;
                call_args.push(a);
            }

            let call = builder.ins().call_indirect(sig_ref, fn_ptr, &call_args);
            if is_void {
                Ok(Some(builder.ins().iconst(types::I64, 0)))
            } else {
                let result = builder.inst_results(call)[0];
                Ok(Some(result))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Operand translation
// ---------------------------------------------------------------------------

fn translate_operand<M: Module>(
    operand: &Operand,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    match operand {
        Operand::Local(id) => {
            let var = translator
                .variables
                .get(&id.0)
                .copied()
                .ok_or_else(|| CodegenError::Internal(format!("undefined local _{}", id.0)))?;
            Ok(builder.use_var(var))
        }
        Operand::Constant(c) => match c {
            Constant::Int(n) => Ok(builder.ins().iconst(types::I64, *n)),
            Constant::Float(n) => Ok(builder.ins().f64const(*n)),
            Constant::Bool(b) => Ok(builder.ins().iconst(types::I8, *b as i64)),
            Constant::Str(s) => {
                // Store the string bytes in the data section, then call
                // kryos_string_new to create a proper KryosString handle.
                let data_name = format!(".str.{}", translator.string_counter);
                *translator.string_counter += 1;

                let data_id = module
                    .declare_data(&data_name, Linkage::Local, false, false)
                    .map_err(CodegenError::Module)?;

                let mut data_desc = DataDescription::new();
                let mut bytes = s.as_bytes().to_vec();
                let str_len = bytes.len();
                bytes.push(0); // null terminator
                data_desc.define(bytes.into_boxed_slice());
                module
                    .define_data(data_id, &data_desc)
                    .map_err(CodegenError::Module)?;

                let gv = module.declare_data_in_func(data_id, builder.func);
                let data_ptr = builder.ins().global_value(types::I64, gv);
                let len_val = builder.ins().iconst(types::I64, str_len as i64);

                let string_new_ref =
                    ensure_func_ref_with_args("kryos_string_new", builder, translator, module, 2)?;
                let call = builder.ins().call(string_new_ref, &[data_ptr, len_val]);
                Ok(builder.inst_results(call)[0])
            }
            Constant::None => Ok(builder.ins().iconst(types::I64, 0)),
        },
    }
}

// ---------------------------------------------------------------------------
// Binary operations
// ---------------------------------------------------------------------------

fn translate_binop(
    op: MirBinOp,
    lhs: cranelift_codegen::ir::Value,
    rhs: cranelift_codegen::ir::Value,
    is_float: bool,
    builder: &mut FunctionBuilder,
    checked: bool,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    if is_float {
        translate_binop_float(op, lhs, rhs, builder)
    } else {
        translate_binop_int(op, lhs, rhs, builder, checked)
    }
}

fn translate_binop_int(
    op: MirBinOp,
    lhs: cranelift_codegen::ir::Value,
    rhs: cranelift_codegen::ir::Value,
    builder: &mut FunctionBuilder,
    checked: bool,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    let val = match op {
        MirBinOp::Add if checked => {
            let (result, overflow) = builder.ins().sadd_overflow(lhs, rhs);
            let trap_block = builder.create_block();
            let ok_block = builder.create_block();
            builder.ins().brif(overflow, trap_block, &[], ok_block, &[]);
            builder.switch_to_block(trap_block);
            builder.seal_block(trap_block);
            builder
                .ins()
                .trap(cranelift_codegen::ir::TrapCode::unwrap_user(2));
            builder.switch_to_block(ok_block);
            builder.seal_block(ok_block);
            result
        }
        MirBinOp::Sub if checked => {
            let (result, overflow) = builder.ins().ssub_overflow(lhs, rhs);
            let trap_block = builder.create_block();
            let ok_block = builder.create_block();
            builder.ins().brif(overflow, trap_block, &[], ok_block, &[]);
            builder.switch_to_block(trap_block);
            builder.seal_block(trap_block);
            builder
                .ins()
                .trap(cranelift_codegen::ir::TrapCode::unwrap_user(2));
            builder.switch_to_block(ok_block);
            builder.seal_block(ok_block);
            result
        }
        MirBinOp::Mul if checked => {
            let (result, overflow) = builder.ins().smul_overflow(lhs, rhs);
            let trap_block = builder.create_block();
            let ok_block = builder.create_block();
            builder.ins().brif(overflow, trap_block, &[], ok_block, &[]);
            builder.switch_to_block(trap_block);
            builder.seal_block(trap_block);
            builder
                .ins()
                .trap(cranelift_codegen::ir::TrapCode::unwrap_user(2));
            builder.switch_to_block(ok_block);
            builder.seal_block(ok_block);
            result
        }
        MirBinOp::Add => builder.ins().iadd(lhs, rhs),
        MirBinOp::Sub => builder.ins().isub(lhs, rhs),
        MirBinOp::Mul => builder.ins().imul(lhs, rhs),
        MirBinOp::Div => builder.ins().sdiv(lhs, rhs),
        MirBinOp::Mod => builder.ins().srem(lhs, rhs),
        MirBinOp::Pow => {
            // Handled at call site via kryos_ipow runtime call.
            // Should never reach here — Pow is intercepted before dispatch.
            return Err(CodegenError::UnsupportedOperation(
                "Pow should be handled at call site".to_string(),
            ));
        }
        MirBinOp::Eq => {
            let cmp = builder.ins().icmp(IntCC::Equal, lhs, rhs);
            cmp
        }
        MirBinOp::Neq => builder.ins().icmp(IntCC::NotEqual, lhs, rhs),
        MirBinOp::Lt => builder.ins().icmp(IntCC::SignedLessThan, lhs, rhs),
        MirBinOp::Gt => builder.ins().icmp(IntCC::SignedGreaterThan, lhs, rhs),
        MirBinOp::LtEq => builder.ins().icmp(IntCC::SignedLessThanOrEqual, lhs, rhs),
        MirBinOp::GtEq => builder
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, lhs, rhs),
        MirBinOp::And => builder.ins().band(lhs, rhs),
        MirBinOp::Or => builder.ins().bor(lhs, rhs),
        MirBinOp::BitAnd => builder.ins().band(lhs, rhs),
        MirBinOp::BitOr => builder.ins().bor(lhs, rhs),
        MirBinOp::BitXor => builder.ins().bxor(lhs, rhs),
        MirBinOp::Shl => builder.ins().ishl(lhs, rhs),
        MirBinOp::Shr => builder.ins().sshr(lhs, rhs),
    };
    Ok(val)
}

fn translate_binop_float(
    op: MirBinOp,
    lhs: cranelift_codegen::ir::Value,
    rhs: cranelift_codegen::ir::Value,
    builder: &mut FunctionBuilder,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    let val = match op {
        MirBinOp::Add => builder.ins().fadd(lhs, rhs),
        MirBinOp::Sub => builder.ins().fsub(lhs, rhs),
        MirBinOp::Mul => builder.ins().fmul(lhs, rhs),
        MirBinOp::Div => builder.ins().fdiv(lhs, rhs),
        MirBinOp::Eq => builder.ins().fcmp(FloatCC::Equal, lhs, rhs),
        MirBinOp::Neq => builder.ins().fcmp(FloatCC::NotEqual, lhs, rhs),
        MirBinOp::Lt => builder.ins().fcmp(FloatCC::LessThan, lhs, rhs),
        MirBinOp::Gt => builder.ins().fcmp(FloatCC::GreaterThan, lhs, rhs),
        MirBinOp::LtEq => builder.ins().fcmp(FloatCC::LessThanOrEqual, lhs, rhs),
        MirBinOp::GtEq => builder.ins().fcmp(FloatCC::GreaterThanOrEqual, lhs, rhs),
        _ => {
            return Err(CodegenError::UnsupportedOperation(format!(
                "float binary op {:?}",
                op
            )));
        }
    };
    Ok(val)
}

// ---------------------------------------------------------------------------
// Unary operations
// ---------------------------------------------------------------------------

fn translate_unop(
    op: MirUnOp,
    val: cranelift_codegen::ir::Value,
    is_float: bool,
    val_ty: Type,
    builder: &mut FunctionBuilder,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    match op {
        MirUnOp::Neg => {
            if is_float {
                Ok(builder.ins().fneg(val))
            } else {
                Ok(builder.ins().ineg(val))
            }
        }
        MirUnOp::Not => {
            // Logical not: compare == 0.
            let zero = builder.ins().iconst(val_ty, 0);
            Ok(builder.ins().icmp(IntCC::Equal, val, zero))
        }
        MirUnOp::BitNot => Ok(builder.ins().bnot(val)),
    }
}

// ---------------------------------------------------------------------------
// Cast translation
// ---------------------------------------------------------------------------

fn translate_cast(
    val: cranelift_codegen::ir::Value,
    src_ty: Type,
    dest_ty: Type,
    builder: &mut FunctionBuilder,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    if src_ty == dest_ty {
        return Ok(val);
    }

    // Float -> Float
    if is_float_type(src_ty) && is_float_type(dest_ty) {
        if src_ty == types::F32 && dest_ty == types::F64 {
            return Ok(builder.ins().fpromote(types::F64, val));
        } else if src_ty == types::F64 && dest_ty == types::F32 {
            return Ok(builder.ins().fdemote(types::F32, val));
        }
    }

    // Int -> Float
    if !is_float_type(src_ty) && is_float_type(dest_ty) {
        return Ok(builder.ins().fcvt_from_sint(dest_ty, val));
    }

    // Float -> Int
    if is_float_type(src_ty) && !is_float_type(dest_ty) {
        return Ok(builder.ins().fcvt_to_sint_sat(dest_ty, val));
    }

    // Int -> Int (widening / narrowing)
    let src_bits = src_ty.bits();
    let dest_bits = dest_ty.bits();
    if src_bits < dest_bits {
        Ok(builder.ins().sextend(dest_ty, val))
    } else if src_bits > dest_bits {
        Ok(builder.ins().ireduce(dest_ty, val))
    } else {
        Ok(val)
    }
}

// ---------------------------------------------------------------------------
// Terminator translation
// ---------------------------------------------------------------------------

fn translate_terminator<M: Module>(
    term: &Terminator,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<(), CodegenError> {
    match term {
        Terminator::Return(None) => {
            if builder.func.signature.returns.is_empty() {
                emit_trace_exit(builder, translator, module)?;
                builder.ins().return_(&[]);
            } else {
                // Unreachable code path in a non-void function (e.g., dead
                // block after an explicit return).  Emit a trap instead of
                // a bare return so the verifier doesn't reject the
                // signature mismatch.
                builder
                    .ins()
                    .trap(cranelift_codegen::ir::TrapCode::user(1).unwrap());
            }
        }
        Terminator::Return(Some(operand)) => {
            let val = translate_operand(operand, builder, translator, module)?;
            // Coerce the value to match the function's declared return type.
            // The codegen uses i64 for most values, but the signature may
            // declare a narrower type (i32, i16, i8) or a wider one.
            let ret_val = if let Some(ret_abi) = builder.func.signature.returns.first() {
                let ret_ty = ret_abi.value_type;
                let val_ty = builder.func.dfg.value_type(val);
                if val_ty == ret_ty {
                    val
                } else if val_ty.bits() > ret_ty.bits() && ret_ty.is_int() {
                    builder.ins().ireduce(ret_ty, val)
                } else if val_ty.bits() < ret_ty.bits() && val_ty.is_int() && ret_ty.is_int() {
                    builder.ins().sextend(ret_ty, val)
                } else if is_float_type(ret_ty) && !is_float_type(val_ty) {
                    // Int -> float: reinterpret bits (float stored as i64 at C ABI level).
                    builder.ins().bitcast(ret_ty, MemFlags::new(), val)
                } else if !is_float_type(ret_ty) && is_float_type(val_ty) {
                    // Float -> int: pack float bits into int.
                    builder.ins().bitcast(ret_ty, MemFlags::new(), val)
                } else {
                    val
                }
            } else {
                val
            };
            emit_trace_exit(builder, translator, module)?;
            builder.ins().return_(&[ret_val]);
        }
        Terminator::Goto(target) => {
            let cl_block = translator.blocks[&target.0];
            builder.ins().jump(cl_block, &[]);
        }
        Terminator::Branch {
            cond,
            then_block,
            else_block,
        } => {
            let cond_val = translate_operand(cond, builder, translator, module)?;
            let then_cl = translator.blocks[&then_block.0];
            let else_cl = translator.blocks[&else_block.0];
            builder.ins().brif(cond_val, then_cl, &[], else_cl, &[]);
        }
        Terminator::Switch {
            value,
            targets,
            default,
        } => {
            let val = translate_operand(value, builder, translator, module)?;
            let default_cl = translator.blocks[&default.0];
            // Size the case constants to match the subject value's type, otherwise
            // we get verifier errors like `arg 1 has type i64, expected i32` when
            // matching on i32/i16/i8/bool subjects.
            let case_ty = builder.func.dfg.value_type(val);

            // Emit a chain of brif instructions for each target.
            // For a small number of targets this is fine; a br_table would
            // be better for large switches but requires contiguous values.
            if targets.is_empty() {
                builder.ins().jump(default_cl, &[]);
            } else {
                for (i, (case_val, block_id)) in targets.iter().enumerate() {
                    let target_cl = translator.blocks[&block_id.0];
                    let case_const = if case_ty.is_int() {
                        builder.ins().iconst(case_ty, *case_val)
                    } else {
                        // Fallback: non-integer subject -- preserve old behavior to
                        // surface a clear verifier error rather than panic here.
                        builder.ins().iconst(types::I64, *case_val)
                    };
                    let cmp = builder.ins().icmp(IntCC::Equal, val, case_const);

                    if i + 1 == targets.len() {
                        // Last case: branch to target or default.
                        builder.ins().brif(cmp, target_cl, &[], default_cl, &[]);
                    } else {
                        // More cases follow: branch to target or fall through
                        // to next comparison.
                        let next_block = builder.create_block();
                        builder.ins().brif(cmp, target_cl, &[], next_block, &[]);
                        builder.seal_block(next_block);
                        builder.switch_to_block(next_block);
                    }
                }
            }
        }
        Terminator::Unreachable => {
            builder
                .ins()
                .trap(cranelift_codegen::ir::TrapCode::user(1).unwrap());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Function reference helpers
// ---------------------------------------------------------------------------

/// Ensure a Cranelift FuncRef exists for a named function, declaring it if
/// necessary (for external calls not in the original module).
fn ensure_func_ref<M: Module>(
    name: &str,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::FuncRef, CodegenError> {
    ensure_func_ref_with_args(name, builder, translator, module, 1)
}

/// Like `ensure_func_ref`, but accepts the expected number of arguments so
/// that unknown (external) functions get a signature with the right arity.
fn ensure_func_ref_with_args<M: Module>(
    name: &str,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
    arg_count: usize,
) -> Result<cranelift_codegen::ir::FuncRef, CodegenError> {
    if let Some(func_ref) = translator.func_refs.get(name) {
        return Ok(*func_ref);
    }

    // Look up or declare the function in the module.
    let func_id = if let Some(id) = translator.func_ids.get(name) {
        *id
    } else {
        // Unknown function — declare it as an import with a generic signature.
        // We use I64 for all parameters and a single I64 return, which works
        // for runtime builtins like `range`, `len`, `print`, etc.
        let mut sig = Signature::new(module.isa().default_call_conv());
        for _ in 0..arg_count {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));
        module.declare_function(name, Linkage::Import, &sig)?
    };

    let func_ref = module.declare_func_in_func(func_id, builder.func);
    translator.func_refs.insert(name.to_string(), func_ref);
    Ok(func_ref)
}

/// Ensure a FuncRef for an f64→f64 function (math builtins like sin, cos, log).
fn ensure_func_ref_f64_f64<M: Module>(
    name: &str,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::FuncRef, CodegenError> {
    if let Some(func_ref) = translator.func_refs.get(name) {
        return Ok(*func_ref);
    }

    let func_id = if let Some(id) = translator.func_ids.get(name) {
        *id
    } else {
        let mut sig = Signature::new(module.isa().default_call_conv());
        sig.params.push(AbiParam::new(types::F64));
        sig.returns.push(AbiParam::new(types::F64));
        module.declare_function(name, Linkage::Import, &sig)?
    };

    let func_ref = module.declare_func_in_func(func_id, builder.func);
    translator.func_refs.insert(name.to_string(), func_ref);
    Ok(func_ref)
}

/// Ensure a FuncRef for an f64→i64 function (e.g. kryos_json_number).
///
/// The default `ensure_func_ref_with_args` declares all-i64 signatures, which
/// would cause Cranelift to pass arguments in integer registers / on the
/// integer side of the calling convention. For C FFI functions whose first
/// argument is `f64` on the Rust side (and which would be passed in xmm0 on
/// SysV), we MUST declare the signature accurately so the Kryos value gets
/// bitcast into the right register class. Without this, an f64 operand ends
/// up in rdi instead of xmm0 and the callee reads garbage.
fn ensure_func_ref_f64_i64<M: Module>(
    name: &str,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::FuncRef, CodegenError> {
    if let Some(func_ref) = translator.func_refs.get(name) {
        return Ok(*func_ref);
    }
    let func_id = if let Some(id) = translator.func_ids.get(name) {
        *id
    } else {
        let mut sig = Signature::new(module.isa().default_call_conv());
        sig.params.push(AbiParam::new(types::F64));
        sig.returns.push(AbiParam::new(types::I64));
        module.declare_function(name, Linkage::Import, &sig)?
    };
    let func_ref = module.declare_func_in_func(func_id, builder.func);
    translator.func_refs.insert(name.to_string(), func_ref);
    Ok(func_ref)
}

/// Ensure a FuncRef for an i64→f64 function (e.g. kryos_json_to_float).
fn ensure_func_ref_i64_f64<M: Module>(
    name: &str,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::FuncRef, CodegenError> {
    if let Some(func_ref) = translator.func_refs.get(name) {
        return Ok(*func_ref);
    }
    let func_id = if let Some(id) = translator.func_ids.get(name) {
        *id
    } else {
        let mut sig = Signature::new(module.isa().default_call_conv());
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::F64));
        module.declare_function(name, Linkage::Import, &sig)?
    };
    let func_ref = module.declare_func_in_func(func_id, builder.func);
    translator.func_refs.insert(name.to_string(), func_ref);
    Ok(func_ref)
}

/// Ensure a FuncRef for an f64,f64→f64 function (pow, min_f, max_f).
fn ensure_func_ref_f64_f64_f64<M: Module>(
    name: &str,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::FuncRef, CodegenError> {
    if let Some(func_ref) = translator.func_refs.get(name) {
        return Ok(*func_ref);
    }

    let func_id = if let Some(id) = translator.func_ids.get(name) {
        *id
    } else {
        let mut sig = Signature::new(module.isa().default_call_conv());
        sig.params.push(AbiParam::new(types::F64));
        sig.params.push(AbiParam::new(types::F64));
        sig.returns.push(AbiParam::new(types::F64));
        module.declare_function(name, Linkage::Import, &sig)?
    };

    let func_ref = module.declare_func_in_func(func_id, builder.func);
    translator.func_refs.insert(name.to_string(), func_ref);
    Ok(func_ref)
}

/// Ensure a FuncRef for a void-return function with the given number of I64 params.
/// Used for trace runtime functions that return nothing.
fn ensure_func_ref_void<M: Module>(
    name: &str,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
    arg_count: usize,
) -> Result<cranelift_codegen::ir::FuncRef, CodegenError> {
    if let Some(func_ref) = translator.func_refs.get(name) {
        return Ok(*func_ref);
    }

    let func_id = if let Some(id) = translator.func_ids.get(name) {
        *id
    } else {
        let mut sig = Signature::new(module.isa().default_call_conv());
        for _ in 0..arg_count {
            sig.params.push(AbiParam::new(types::I64));
        }
        // No return value.
        module.declare_function(name, Linkage::Import, &sig)?
    };

    let func_ref = module.declare_func_in_func(func_id, builder.func);
    translator.func_refs.insert(name.to_string(), func_ref);
    Ok(func_ref)
}

/// Emit a `kryos_trace_enter(name_ptr, name_len, file_ptr, file_len, line)` call.
///
/// Creates data sections for the function name and file name strings, then
/// emits the call instruction. The `string_counter` is used to generate
/// unique data section names.
fn emit_trace_enter<M: Module>(
    mir_func: &MirFunction,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<(), CodegenError> {
    let func_name = &mir_func.name;
    // Create a data section for the function name.
    let name_data_name = format!(".trace_name.{}", translator.string_counter);
    *translator.string_counter += 1;

    let name_bytes = func_name.as_bytes();
    let name_len = name_bytes.len();

    let name_data_id = module
        .declare_data(&name_data_name, Linkage::Local, false, false)
        .map_err(CodegenError::Module)?;
    let mut name_desc = DataDescription::new();
    let mut name_buf = name_bytes.to_vec();
    name_buf.push(0); // null terminator for safety
    name_desc.define(name_buf.into_boxed_slice());
    module
        .define_data(name_data_id, &name_desc)
        .map_err(CodegenError::Module)?;

    let name_gv = module.declare_data_in_func(name_data_id, builder.func);
    let name_ptr = builder.ins().global_value(types::I64, name_gv);
    let name_len_val = builder.ins().iconst(types::I64, name_len as i64);

    // Use source_file from MIR if present — the driver populates this from
    // the AST span before codegen runs.
    let file_str: &str = mir_func.source_file.as_deref().unwrap_or("<unknown>");
    let file_data_name = format!(".trace_file.{}", translator.string_counter);
    *translator.string_counter += 1;

    let file_bytes = file_str.as_bytes();
    let file_len = file_bytes.len();

    let file_data_id = module
        .declare_data(&file_data_name, Linkage::Local, false, false)
        .map_err(CodegenError::Module)?;
    let mut file_desc = DataDescription::new();
    let mut file_buf = file_bytes.to_vec();
    file_buf.push(0);
    file_desc.define(file_buf.into_boxed_slice());
    module
        .define_data(file_data_id, &file_desc)
        .map_err(CodegenError::Module)?;

    let file_gv = module.declare_data_in_func(file_data_id, builder.func);
    let file_ptr = builder.ins().global_value(types::I64, file_gv);
    let file_len_val = builder.ins().iconst(types::I64, file_len as i64);

    // Line number comes from the MIR function (driver populates from AST span).
    let line_val = builder
        .ins()
        .iconst(types::I64, mir_func.source_line as i64);

    // Call kryos_trace_enter(name_ptr, name_len, file_ptr, file_len, line).
    let trace_enter_ref =
        ensure_func_ref_void("kryos_trace_enter", builder, translator, module, 5)?;
    builder.ins().call(
        trace_enter_ref,
        &[name_ptr, name_len_val, file_ptr, file_len_val, line_val],
    );

    Ok(())
}

/// Emit a `kryos_trace_exit()` call before a return instruction.
fn emit_trace_exit<M: Module>(
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<(), CodegenError> {
    let trace_exit_ref = ensure_func_ref_void("kryos_trace_exit", builder, translator, module, 0)?;
    builder.ins().call(trace_exit_ref, &[]);
    Ok(())
}

/// Like `ensure_func_ref_with_args`, but uses F64 for all params and returns.
/// Used for float math runtime functions (kryos_fpow, kryos_fmod).
fn ensure_func_ref_f64<M: Module>(
    name: &str,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
    arg_count: usize,
) -> Result<cranelift_codegen::ir::FuncRef, CodegenError> {
    if let Some(func_ref) = translator.func_refs.get(name) {
        return Ok(*func_ref);
    }

    let func_id = if let Some(id) = translator.func_ids.get(name) {
        *id
    } else {
        let mut sig = Signature::new(module.isa().default_call_conv());
        for _ in 0..arg_count {
            sig.params.push(AbiParam::new(types::F64));
        }
        sig.returns.push(AbiParam::new(types::F64));
        module.declare_function(name, Linkage::Import, &sig)?
    };

    let func_ref = module.declare_func_in_func(func_id, builder.func);
    translator.func_refs.insert(name.to_string(), func_ref);
    Ok(func_ref)
}

// ---------------------------------------------------------------------------
// Deep copy for @copy structs
// ---------------------------------------------------------------------------

/// Emit a deep clone of an Array<Str> value: shallow-clone the header + data
/// buffer via kryos_array_clone, then iterate the cloned buffer and replace
/// each element pointer with an independent kryos_string_clone of itself.
///
/// Why only Array<Str> and not the general Array<T> case: this helper is
/// non-recursive by design. The general case (Array<@copy Struct>) would
/// need to call emit_deep_copy_struct on each element, which recurses
/// through self-referential types like MirType{element_type:[MirType]}
/// and overflows stage-0's stack at compile time. Array<Str> is bounded
/// (kryos_string_clone is a runtime function, no compile-time recursion)
/// so it's safe.
///
/// Use this where the prior code called kryos_array_clone directly on a
/// @copy struct field of type [Str], to avoid the double-free that
/// happens when both the source and the clone end up sole-owners of
/// the same string pointers and both iterate-and-free on drop.
///
/// Emit a call to kryos_array_clone_deep(src, elem_clone_fn). The runtime
/// function does the shallow array clone + per-element deep clone loop
/// inside itself. This emits ONE call instead of a multi-block inline
/// loop, dramatically reducing the IR/asm size at every dispatch site
/// (relevant because stage-1 has hundreds of @copy struct construction
/// sites that fire these helpers).
///
/// `elem_clone_fn_name` is the name of the per-element clone function
/// (must already be in func_ids): "kryos_string_clone" for Array<Str>,
/// "__kryos_clone_<N>" for Array<Struct(N)>, etc.
fn emit_array_clone_deep_call<M: Module>(
    src_arr: cranelift_codegen::ir::Value,
    elem_clone_fn_name: &str,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    // Materialize the address of the elem clone function as an i64.
    let elem_fn_id = *translator
        .func_ids
        .get(elem_clone_fn_name)
        .ok_or_else(|| CodegenError::UnsupportedOperation(format!(
            "emit_array_clone_deep_call: {} not in func_ids",
            elem_clone_fn_name
        )))?;
    let elem_fn_ref = module.declare_func_in_func(elem_fn_id, builder.func);
    let elem_fn_addr = builder.ins().func_addr(types::I64, elem_fn_ref);

    // Call kryos_array_clone_deep(src_arr, elem_fn_addr).
    let clone_deep_id = *translator
        .func_ids
        .get("kryos_array_clone_deep")
        .ok_or_else(|| CodegenError::UnsupportedOperation(
            "kryos_array_clone_deep not pre-declared".to_string()
        ))?;
    let clone_deep_ref = module.declare_func_in_func(clone_deep_id, builder.func);
    let call = builder.ins().call(clone_deep_ref, &[src_arr, elem_fn_addr]);
    Ok(builder.inst_results(call)[0])
}

fn emit_array_str_deep_clone<M: Module>(
    src_arr: cranelift_codegen::ir::Value,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    let clone_ref =
        ensure_func_ref_with_args("kryos_array_clone", builder, translator, module, 1)?;
    let c = builder.ins().call(clone_ref, &[src_arr]);
    let cloned = builder.inst_results(c)[0];

    // Guard: skip the loop when cloned is null.
    let zero_ptr = builder.ins().iconst(types::I64, 0);
    let is_nonnull = builder.ins().icmp(
        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
        cloned,
        zero_ptr,
    );
    let body_block = builder.create_block();
    let after_block = builder.create_block();
    builder
        .ins()
        .brif(is_nonnull, body_block, &[], after_block, &[]);
    builder.seal_block(body_block);
    builder.switch_to_block(body_block);

    // KryosArray { len:i64@0, cap:i64@8, elem_size:i64@16, ref_count:i64@24, data:ptr@32 }
    let len = builder.ins().load(types::I64, MemFlags::new(), cloned, 0);
    let data = builder.ins().load(types::I64, MemFlags::new(), cloned, 32);

    let loop_header = builder.create_block();
    builder.append_block_param(loop_header, types::I64);
    let loop_body = builder.create_block();
    let exit_block = builder.create_block();

    let zero = builder.ins().iconst(types::I64, 0);
    builder.ins().jump(loop_header, &[zero]);

    builder.switch_to_block(loop_header);
    let i = builder.block_params(loop_header)[0];
    let done = builder.ins().icmp(
        cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThanOrEqual,
        i,
        len,
    );
    builder.ins().brif(done, exit_block, &[], loop_body, &[]);
    builder.seal_block(loop_body);

    builder.switch_to_block(loop_body);
    let byte_off = builder.ins().imul_imm(i, 8);
    let elem_addr = builder.ins().iadd(data, byte_off);
    let elem = builder
        .ins()
        .load(types::I64, MemFlags::new(), elem_addr, 0);

    // Each element is a *KryosString. Clone it.
    let str_clone_ref =
        ensure_func_ref_with_args("kryos_string_clone", builder, translator, module, 1)?;
    let cc = builder.ins().call(str_clone_ref, &[elem]);
    let cloned_str = builder.inst_results(cc)[0];
    builder
        .ins()
        .store(MemFlags::new(), cloned_str, elem_addr, 0);

    let i_next = builder.ins().iadd_imm(i, 1);
    builder.ins().jump(loop_header, &[i_next]);

    builder.seal_block(loop_header);
    builder.seal_block(exit_block);
    builder.switch_to_block(exit_block);
    builder.ins().jump(after_block, &[]);

    builder.seal_block(after_block);
    builder.switch_to_block(after_block);

    Ok(cloned)
}

/// Emit a deep clone of an Array<Struct(N)> value where N is a @copy struct
/// with heap fields: shallow-clone the header + data via kryos_array_clone,
/// then iterate the cloned buffer and replace each element pointer with an
/// independent __kryos_clone_<N>(elem) result. Recursion is at the call
/// boundary at runtime, not at compile time — avoids the b428325 cycle.
///
/// Currently superseded by emit_array_clone_deep_call (runtime ABI variant)
/// for active dispatch paths. Kept as historical reference for the codegen-
/// emitted-loop variant; may be revived if a non-runtime-ABI deep clone is
/// needed for a backend without kryos_array_clone_deep.
#[allow(dead_code)]
fn emit_array_struct_deep_clone<M: Module>(
    src_arr: cranelift_codegen::ir::Value,
    struct_name: &str,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    let clone_ref =
        ensure_func_ref_with_args("kryos_array_clone", builder, translator, module, 1)?;
    let c = builder.ins().call(clone_ref, &[src_arr]);
    let cloned = builder.inst_results(c)[0];

    let clone_name = format!("__kryos_clone_{struct_name}");
    let _ = &clone_name;

    let zero_ptr = builder.ins().iconst(types::I64, 0);
    let is_nonnull = builder.ins().icmp(
        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
        cloned,
        zero_ptr,
    );
    let body_block = builder.create_block();
    let after_block = builder.create_block();
    builder
        .ins()
        .brif(is_nonnull, body_block, &[], after_block, &[]);
    builder.seal_block(body_block);
    builder.switch_to_block(body_block);

    let len = builder.ins().load(types::I64, MemFlags::new(), cloned, 0);
    let data = builder.ins().load(types::I64, MemFlags::new(), cloned, 32);

    let loop_header = builder.create_block();
    builder.append_block_param(loop_header, types::I64);
    let loop_body = builder.create_block();
    let exit_block = builder.create_block();

    let zero = builder.ins().iconst(types::I64, 0);
    builder.ins().jump(loop_header, &[zero]);

    builder.switch_to_block(loop_header);
    let i = builder.block_params(loop_header)[0];
    let done = builder.ins().icmp(
        cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThanOrEqual,
        i,
        len,
    );
    builder.ins().brif(done, exit_block, &[], loop_body, &[]);
    builder.seal_block(loop_body);

    builder.switch_to_block(loop_body);
    let byte_off = builder.ins().imul_imm(i, 8);
    let elem_addr = builder.ins().iadd(data, byte_off);
    let elem = builder
        .ins()
        .load(types::I64, MemFlags::new(), elem_addr, 0);

    let elem_clone_ref =
        ensure_func_ref_with_args(&clone_name, builder, translator, module, 1)?;
    let cc = builder.ins().call(elem_clone_ref, &[elem]);
    let cloned_elem = builder.inst_results(cc)[0];
    builder
        .ins()
        .store(MemFlags::new(), cloned_elem, elem_addr, 0);

    let i_next = builder.ins().iadd_imm(i, 1);
    builder.ins().jump(loop_header, &[i_next]);

    builder.seal_block(loop_header);
    builder.seal_block(exit_block);
    builder.switch_to_block(exit_block);
    builder.ins().jump(after_block, &[]);

    builder.seal_block(after_block);
    builder.switch_to_block(after_block);

    Ok(cloned)
}

/// Emit a deep copy of a @copy struct: malloc a new struct, clone all
/// heap-allocated fields (arrays, strings, nested @copy structs).
fn emit_deep_copy_struct<M: Module>(
    src_ptr: cranelift_codegen::ir::Value,
    struct_def: &[(String, MirType)],
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    let layout = compute_struct_layout(struct_def)?;
    // Use calloc so unset/uninitialized fields are null, not garbage.
    // kryos_array_clone(null) and kryos_string_clone(null) are both safe;
    // garbage pointers passed to them cause STATUS_HEAP_CORRUPTION.
    let one_val = builder.ins().iconst(types::I64, 1);
    let size_val = builder.ins().iconst(types::I64, layout.total_size as i64);
    let calloc_ref = ensure_func_ref_with_args("calloc", builder, translator, module, 2)?;
    let alloc_call = builder.ins().call(calloc_ref, &[one_val, size_val]);
    let new_ptr = builder.inst_results(alloc_call)[0];

    for (field_name, offset, cl_ty) in &layout.field_offsets {
        let field_val = builder
            .ins()
            .load(*cl_ty, MemFlags::new(), src_ptr, *offset as i32);
        let field_mir_ty = struct_def
            .iter()
            .find(|(n, _)| n == field_name)
            .map(|(_, t)| t);
        let stored_val = match field_mir_ty {
            Some(MirType::Array(inner_ty, _)) => {
                // Array<Str>: deep-clone. Else: retain. Array<Struct> dispatch
                // disabled pending isolation of emit_array_struct_deep_clone bug.
                if matches!(**inner_ty, MirType::Str) {
                    emit_array_str_deep_clone(field_val, builder, translator, module)?
                } else {
                    let retain_ref = ensure_func_ref_with_args(
                        "kryos_array_retain",
                        builder,
                        translator,
                        module,
                        1,
                    )?;
                    let call = builder.ins().call(retain_ref, &[field_val]);
                    builder.inst_results(call)[0]
                }
            }
            Some(MirType::Str) => {
                let clone_ref = ensure_func_ref_with_args(
                    "kryos_string_clone",
                    builder,
                    translator,
                    module,
                    1,
                )?;
                let call = builder.ins().call(clone_ref, &[field_val]);
                builder.inst_results(call)[0]
            }
            Some(MirType::Map { .. }) => {
                // Deep-clone maps via kryos_map_clone.
                let clone_ref =
                    ensure_func_ref_with_args("kryos_map_clone", builder, translator, module, 1)?;
                let call = builder.ins().call(clone_ref, &[field_val]);
                builder.inst_results(call)[0]
            }
            Some(MirType::Struct(inner_name)) => {
                if translator.copy_structs.contains(inner_name) {
                    if let Some(inner_def) = translator.struct_defs.get(inner_name).cloned() {
                        emit_deep_copy_struct(field_val, &inner_def, builder, translator, module)?
                    } else {
                        field_val
                    }
                } else {
                    field_val
                }
            }
            Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                // ARC-managed: retain to share ownership with the copy.
                let retain_ref =
                    ensure_func_ref_with_args("kryos_arc_retain", builder, translator, module, 1)?;
                builder.ins().call(retain_ref, &[field_val]);
                field_val
            }
            _ => field_val,
        };
        builder
            .ins()
            .store(MemFlags::new(), stored_val, new_ptr, *offset as i32);
    }

    Ok(new_ptr)
}

// ---------------------------------------------------------------------------
// Exception cleanup: drop live locals on early-return
// ---------------------------------------------------------------------------

/// Emit a drop (free) for a single Cranelift value of the given MIR type.
/// The caller is responsible for null-checking the value before calling this.
#[allow(clippy::collapsible_match)]
fn emit_drop_for_value<M: Module>(
    val: cranelift_codegen::ir::Value,
    ty: &MirType,
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<(), CodegenError> {
    match ty {
        MirType::Str => {
            let free_ref =
                ensure_func_ref_with_args("kryos_string_free", builder, translator, module, 1)?;
            builder.ins().call(free_ref, &[val]);
        }
        MirType::Function { .. } | MirType::Shared(_) => {
            let release_ref =
                ensure_func_ref_with_args("kryos_arc_release", builder, translator, module, 1)?;
            builder.ins().call(release_ref, &[val]);
        }
        MirType::Map { .. } => {
            // Maps use their own allocator — call kryos_map_free directly.
            let free_ref =
                ensure_func_ref_with_args("kryos_map_free", builder, translator, module, 1)?;
            builder.ins().call(free_ref, &[val]);
        }
        MirType::Struct(ref name) => {
            // Recursively free heap-allocated fields, then free the struct.
            if let Some(struct_def) = translator.struct_defs.get(name).cloned() {
                if let Ok(layout) = compute_struct_layout(&struct_def) {
                    for (field_name, field_ty) in struct_def.iter() {
                        let field_offset = layout
                            .field_offsets
                            .iter()
                            .find(|(n, _, _)| n == field_name)
                            .map(|(_, off, _)| *off as i32);
                        if let Some(offset) = field_offset {
                            match field_ty {
                                MirType::Str
                                | MirType::Array(_, _)
                                | MirType::Function { .. }
                                | MirType::Enum(_)
                                | MirType::Shared(_) => {
                                    let field_val = builder.ins().load(
                                        types::I64,
                                        MemFlags::new(),
                                        val,
                                        offset,
                                    );
                                    emit_drop_for_value(
                                        field_val, field_ty, builder, translator, module,
                                    )?;
                                }
                                MirType::Struct(ref inner_name) => {
                                    // @copy structs embedded in a containing struct share
                                    // field pointers with their original source; skip
                                    // recursive drop to avoid double-free.
                                    if !translator.copy_structs.contains(inner_name) {
                                        let field_val = builder.ins().load(
                                            types::I64,
                                            MemFlags::new(),
                                            val,
                                            offset,
                                        );
                                        emit_drop_for_value(
                                            field_val, field_ty, builder, translator, module,
                                        )?;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            let free_ref = ensure_func_ref_with_args("free", builder, translator, module, 1)?;
            builder.ins().call(free_ref, &[val]);
        }
        MirType::Array(ref elem_ty, _) => {
            // Determine the per-element drop function for heap element types.
            // For struct/enum elements, use a named drop helper (__kryos_drop_X)
            // that recursively frees nested heap fields. This avoids compile-time
            // infinite recursion while still cleaning up deeply nested structures.
            let elem_free_fn: Option<String> = match elem_ty.as_ref() {
                MirType::Str => Some("kryos_string_free".to_string()),
                MirType::Array(_, _) => Some("kryos_array_free".to_string()),
                MirType::Function { .. } | MirType::Shared(_) => {
                    Some("kryos_arc_release".to_string())
                }
                MirType::Map { .. } => Some("kryos_map_free".to_string()),
                MirType::Struct(n) => {
                    let drop_name = format!("__kryos_drop_{n}");
                    if translator.func_ids.contains_key(&drop_name) {
                        Some(drop_name)
                    } else {
                        Some("free".to_string())
                    }
                }
                MirType::Enum(n) => {
                    let drop_name = format!("__kryos_drop_{n}");
                    if translator.func_ids.contains_key(&drop_name) {
                        Some(drop_name)
                    } else {
                        Some("free".to_string())
                    }
                }
                _ => None,
            };

            if let Some(ref free_fn) = elem_free_fn {
                // Guard: skip element cleanup if array pointer is null.
                let zero_ptr = builder.ins().iconst(types::I64, 0);
                let is_nonnull = builder.ins().icmp(
                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                    val,
                    zero_ptr,
                );
                let elem_drop_block = builder.create_block();
                let after_elem_block = builder.create_block();
                builder
                    .ins()
                    .brif(is_nonnull, elem_drop_block, &[], after_elem_block, &[]);
                builder.seal_block(elem_drop_block);
                builder.switch_to_block(elem_drop_block);

                // KryosArray layout: { len: i64 @0, cap: i64 @8,
                //                      elem_size: i64 @16, ref_count: i64 @24, data: ptr @32 }
                //
                // Guard: only drop elements when this is the sole owner (ref_count == 1).
                // When ref_count > 1, another owner still holds the array; dropping elements
                // here would corrupt the shared buffer. kryos_array_free handles the decrement.
                let ref_count = builder.ins().load(types::I64, MemFlags::new(), val, 24);
                let one = builder.ins().iconst(types::I64, 1);
                let is_sole_owner = builder.ins().icmp(
                    cranelift_codegen::ir::condcodes::IntCC::Equal,
                    ref_count,
                    one,
                );
                let sole_owner_block = builder.create_block();
                builder
                    .ins()
                    .brif(is_sole_owner, sole_owner_block, &[], after_elem_block, &[]);
                builder.seal_block(sole_owner_block);
                builder.switch_to_block(sole_owner_block);

                let len = builder.ins().load(types::I64, MemFlags::new(), val, 0);
                let data = builder.ins().load(types::I64, MemFlags::new(), val, 32);

                let loop_header = builder.create_block();
                builder.append_block_param(loop_header, types::I64);
                let loop_body = builder.create_block();
                let exit_block = builder.create_block();

                let zero = builder.ins().iconst(types::I64, 0);
                builder.ins().jump(loop_header, &[zero]);

                builder.switch_to_block(loop_header);
                let i = builder.block_params(loop_header)[0];
                let done = builder.ins().icmp(
                    cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThanOrEqual,
                    i,
                    len,
                );
                builder.ins().brif(done, exit_block, &[], loop_body, &[]);
                builder.seal_block(loop_body);

                builder.switch_to_block(loop_body);
                let byte_off = builder.ins().imul_imm(i, 8);
                let elem_addr = builder.ins().iadd(data, byte_off);
                let elem = builder
                    .ins()
                    .load(types::I64, MemFlags::new(), elem_addr, 0);
                let elem_free_ref =
                    ensure_func_ref_with_args(free_fn, builder, translator, module, 1)?;
                builder.ins().call(elem_free_ref, &[elem]);
                let i_next = builder.ins().iadd_imm(i, 1);
                builder.ins().jump(loop_header, &[i_next]);

                builder.seal_block(loop_header);
                builder.seal_block(exit_block);
                builder.switch_to_block(exit_block);
                builder.ins().jump(after_elem_block, &[]);

                builder.seal_block(after_elem_block);
                builder.switch_to_block(after_elem_block);
            }

            let free_ref =
                ensure_func_ref_with_args("kryos_array_free", builder, translator, module, 1)?;
            builder.ins().call(free_ref, &[val]);
        }
        MirType::Enum(ref enum_name) => {
            // Runtime variant-aware Drop: load the tag, dispatch on it,
            // and free heap-owning payload fields for the active variant.
            if let Some(variants) = translator.enum_defs.get(enum_name).cloned() {
                // Check if any variant holds droppable fields.
                let has_droppable = variants.iter().any(|v| {
                    v.fields.iter().any(|f| {
                        matches!(
                            f,
                            MirType::Str
                                | MirType::Array(_, _)
                                | MirType::Struct(_)
                                | MirType::Function { .. }
                                | MirType::Enum(_)
                                | MirType::Shared(_)
                        )
                    })
                });

                if has_droppable {
                    // Load the tag from offset 0.
                    let tag = builder.ins().load(types::I64, MemFlags::new(), val, 0);

                    // Create a merge block where all variant cleanup paths converge.
                    let merge_block = builder.create_block();

                    for (idx, variant) in variants.iter().enumerate() {
                        let droppable_fields: Vec<(usize, &MirType)> = variant
                            .fields
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| {
                                matches!(
                                    f,
                                    MirType::Str
                                        | MirType::Array(_, _)
                                        | MirType::Struct(_)
                                        | MirType::Function { .. }
                                        | MirType::Enum(_)
                                        | MirType::Shared(_)
                                )
                            })
                            .collect();

                        if droppable_fields.is_empty() {
                            continue;
                        }

                        let variant_block = builder.create_block();
                        let skip_block = builder.create_block();

                        let tag_const = builder.ins().iconst(types::I64, idx as i64);
                        let is_match = builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::Equal,
                            tag,
                            tag_const,
                        );
                        builder
                            .ins()
                            .brif(is_match, variant_block, &[], skip_block, &[]);

                        builder.seal_block(variant_block);
                        builder.switch_to_block(variant_block);

                        // Drop each heap-owning field in this variant.
                        for (field_idx, field_ty) in &droppable_fields {
                            let offset = ((*field_idx + 1) * 8) as i32;
                            let field_val =
                                builder.ins().load(types::I64, MemFlags::new(), val, offset);
                            emit_drop_for_value(field_val, field_ty, builder, translator, module)?;
                        }
                        builder.ins().jump(merge_block, &[]);

                        builder.seal_block(skip_block);
                        builder.switch_to_block(skip_block);
                    }

                    // Final skip block falls through to merge.
                    builder.ins().jump(merge_block, &[]);
                    builder.seal_block(merge_block);
                    builder.switch_to_block(merge_block);
                }
            }
            // Enum is heap-allocated via malloc — free the enum pointer.
            let free_ref = ensure_func_ref_with_args("free", builder, translator, module, 1)?;
            builder.ins().call(free_ref, &[val]);
        }
        _ => {}
    }
    Ok(())
}

/// Emit cleanup drops for all live locals in the exception early-return path.
///
/// Iterates locals in reverse order, skipping function parameters.  For each
/// local of a droppable type (Str, Function, Struct, Array), loads the
/// variable, null-checks it, and conditionally calls the appropriate free
/// function.  This is safe because all non-parameter locals are
/// zero-initialized, so an uninitialized local will have value 0 and the
/// null-check will skip the free.
fn emit_exception_cleanup_drops<M: Module>(
    builder: &mut FunctionBuilder,
    translator: &mut FuncTranslator,
    module: &mut M,
) -> Result<(), CodegenError> {
    let param_ids: std::collections::HashSet<u32> = translator
        .mir_func
        .params
        .iter()
        .map(|p| p.local.0)
        .collect();

    // Collect the locals we need to drop: non-parameter, droppable types.
    // @copy structs are excluded: they share field pointers with their source
    // and must not free those fields -- the original owner handles cleanup.
    let locals_to_drop: Vec<(u32, MirType)> = translator
        .mir_func
        .locals
        .iter()
        .filter(|l| !param_ids.contains(&l.id.0))
        .filter(|l| {
            matches!(
                l.ty,
                MirType::Str
                    | MirType::Function { .. }
                    | MirType::Struct(_)
                    | MirType::Array(_, _)
                    | MirType::Enum(_)
                    | MirType::Shared(_)
            )
        })
        .filter(|l| match &l.ty {
            MirType::Struct(name) => !translator.copy_structs.contains(name),
            _ => true,
        })
        .map(|l| (l.id.0, l.ty.clone()))
        .collect();

    // Emit drops in reverse order (last-declared first).
    for (local_id, ty) in locals_to_drop.iter().rev() {
        if let Some(&var) = translator.variables.get(local_id) {
            let val = builder.use_var(var);
            let zero = builder.ins().iconst(types::I64, 0);
            let is_nonnull = builder.ins().icmp(IntCC::NotEqual, val, zero);

            let do_drop_block = builder.create_block();
            let after_drop_block = builder.create_block();

            builder
                .ins()
                .brif(is_nonnull, do_drop_block, &[], after_drop_block, &[]);

            builder.switch_to_block(do_drop_block);
            builder.seal_block(do_drop_block);
            emit_drop_for_value(val, ty, builder, translator, module)?;
            builder.ins().jump(after_drop_block, &[]);

            builder.switch_to_block(after_drop_block);
            builder.seal_block(after_drop_block);
        }
    }

    Ok(())
}
