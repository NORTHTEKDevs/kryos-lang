//! AOT compilation: MIR -> Cranelift IR -> object file bytes.

use std::collections::HashMap;

use cranelift_codegen::ir::{
    types, AbiParam, Function, InstBuilder, Signature, Type, UserFuncName,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};

use kryos_mir::ir::{
    BasicBlock, Constant, Instruction, LocalId, MirBinOp, MirFunction, MirModule,
    MirType, MirUnOp, Operand, RValue, Terminator,
};

use crate::CodegenError;

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
        MirType::Str => Ok(Some(types::I64)),   // pointer to string data
        MirType::Void => Ok(None),
        MirType::Ptr(_) | MirType::Shared(_) => Ok(Some(types::I64)), // pointer
        MirType::Array(_, _) | MirType::Tuple(_) | MirType::Struct(_) => {
            Ok(Some(types::I64)) // pointer to heap/stack allocation
        }
        MirType::Function { .. } => Ok(Some(types::I64)), // function pointer
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
                mir_type_to_cl(&local.ty).ok().flatten().unwrap_or(types::I64)
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

// ---------------------------------------------------------------------------
// AOT entry point
// ---------------------------------------------------------------------------

/// Compile a MIR module into object file bytes (ELF / COFF / Mach-O).
pub fn compile_module(module: &MirModule) -> Result<Vec<u8>, CodegenError> {
    // Build ISA for the host.
    let mut flag_builder = settings::builder();
    flag_builder
        .set("opt_level", "speed")
        .map_err(|e| CodegenError::Target(e.to_string()))?;
    flag_builder
        .set("is_pic", "true")
        .map_err(|e| CodegenError::Target(e.to_string()))?;

    let isa_builder = cranelift_native::builder()
        .map_err(|e| CodegenError::Target(e.to_string()))?;
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
    let mut func_ids: HashMap<String, FuncId> = HashMap::new();
    for mir_func in &module.functions {
        let sig = build_signature(mir_func, object_module.isa().default_call_conv());
        let func_id = object_module.declare_function(
            &mir_func.name,
            Linkage::Export,
            &sig,
        )?;
        func_ids.insert(mir_func.name.clone(), func_id);
    }

    // Declare ARC runtime functions.
    let arc_retain_sig = {
        let mut sig = Signature::new(object_module.isa().default_call_conv());
        sig.params.push(AbiParam::new(types::I64));
        sig
    };
    let arc_release_sig = arc_retain_sig.clone();
    let arc_alloc_sig = {
        let mut sig = Signature::new(object_module.isa().default_call_conv());
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        sig
    };

    let arc_retain_id = object_module.declare_function(
        "kryos_arc_retain",
        Linkage::Import,
        &arc_retain_sig,
    )?;
    let arc_release_id = object_module.declare_function(
        "kryos_arc_release",
        Linkage::Import,
        &arc_release_sig,
    )?;
    let arc_alloc_id = object_module.declare_function(
        "kryos_arc_alloc",
        Linkage::Import,
        &arc_alloc_sig,
    )?;

    func_ids.insert("kryos_arc_retain".to_string(), arc_retain_id);
    func_ids.insert("kryos_arc_release".to_string(), arc_release_id);
    func_ids.insert("kryos_arc_alloc".to_string(), arc_alloc_id);

    // Second pass: translate each function body.
    for mir_func in &module.functions {
        let func_id = func_ids[&mir_func.name];
        let sig = build_signature(mir_func, object_module.isa().default_call_conv());

        let mut cl_func = Function::with_name_signature(
            UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        {
            let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
            translate_function(
                mir_func,
                &mut builder,
                &func_ids,
                &mut object_module,
            )?;
            builder.seal_all_blocks();
            builder.finalize();
        }

        let mut ctx = Context::for_function(cl_func);
        object_module
            .define_function(func_id, &mut ctx)
            .map_err(|e| CodegenError::Module(e))?;
    }

    let product = object_module.finish();
    let bytes = product.emit().map_err(|e| {
        CodegenError::Internal(format!("{e}"))
    })?;
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
}

/// Translate a MIR function body into Cranelift IR instructions.
pub fn translate_function<M: Module>(
    mir_func: &MirFunction,
    builder: &mut FunctionBuilder,
    func_ids: &HashMap<String, FuncId>,
    module: &mut M,
) -> Result<(), CodegenError> {
    let mut translator = FuncTranslator {
        mir_func,
        variables: HashMap::new(),
        blocks: HashMap::new(),
        func_refs: HashMap::new(),
        func_ids,
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

    // Translate the entry block's instructions (we already switched to it).
    translate_block_body(
        &mir_func.blocks[0],
        builder,
        &mut translator,
        module,
    )?;

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
    translate_terminator(&bb.terminator, builder, translator)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Instruction translation
// ---------------------------------------------------------------------------

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
                let var = translator
                    .variables
                    .get(&dest.0)
                    .copied()
                    .ok_or_else(|| {
                        CodegenError::Internal(format!("undefined local _{}", dest.0))
                    })?;
                builder.def_var(var, val);
            }
        }
        Instruction::ArcRetain { ptr } => {
            let func_ref = ensure_func_ref(
                "kryos_arc_retain",
                builder,
                translator,
                module,
            )?;
            let val = builder.use_var(
                translator.variables[&ptr.0],
            );
            builder.ins().call(func_ref, &[val]);
        }
        Instruction::ArcRelease { ptr } => {
            let func_ref = ensure_func_ref(
                "kryos_arc_release",
                builder,
                translator,
                module,
            )?;
            let val = builder.use_var(
                translator.variables[&ptr.0],
            );
            builder.ins().call(func_ref, &[val]);
        }
        Instruction::Drop { local: _ } => {
            // Drop is a no-op at the Cranelift level; actual cleanup is
            // handled by ARC retain/release pairs inserted during MIR lowering.
        }
        Instruction::Nop => {}
    }
    Ok(())
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
            let val = translate_operand(operand, builder, translator)?;
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

        RValue::ConstString(_s) => {
            // For now, string constants are represented as null pointers.
            // A full implementation would store them in a data section.
            let val = builder.ins().iconst(types::I64, 0);
            Ok(Some(val))
        }

        RValue::ConstNone => {
            let val = builder.ins().iconst(types::I64, 0);
            Ok(Some(val))
        }

        RValue::BinOp { op, left, right } => {
            let lhs = translate_operand(left, builder, translator)?;
            let rhs = translate_operand(right, builder, translator)?;
            let lhs_ty = type_of_operand_hint(left, &translator.mir_func.locals);
            let is_float = is_float_type(lhs_ty);

            let val = translate_binop(*op, lhs, rhs, is_float, builder)?;
            Ok(Some(val))
        }

        RValue::UnOp { op, operand } => {
            let val = translate_operand(operand, builder, translator)?;
            let val_ty = type_of_operand_hint(operand, &translator.mir_func.locals);
            let is_float = is_float_type(val_ty);

            let result = translate_unop(*op, val, is_float, val_ty, builder)?;
            Ok(Some(result))
        }

        RValue::Call { func, args } => {
            let func_ref = ensure_func_ref(func, builder, translator, module)?;
            let arg_vals: Vec<cranelift_codegen::ir::Value> = args
                .iter()
                .map(|a| translate_operand(a, builder, translator))
                .collect::<Result<_, _>>()?;

            let call_inst = builder.ins().call(func_ref, &arg_vals);
            let results = builder.inst_results(call_inst);
            if results.is_empty() {
                Ok(None)
            } else {
                Ok(Some(results[0]))
            }
        }

        RValue::Array(_elems) => {
            // Aggregate construction: return a placeholder pointer.
            let val = builder.ins().iconst(types::I64, 0);
            Ok(Some(val))
        }

        RValue::Tuple(_elems) => {
            let val = builder.ins().iconst(types::I64, 0);
            Ok(Some(val))
        }

        RValue::Struct { .. } => {
            let val = builder.ins().iconst(types::I64, 0);
            Ok(Some(val))
        }

        RValue::Field { .. } => {
            let val = builder.ins().iconst(types::I64, 0);
            Ok(Some(val))
        }

        RValue::Index { .. } => {
            let val = builder.ins().iconst(types::I64, 0);
            Ok(Some(val))
        }

        RValue::ArcAlloc { inner } => {
            let func_ref =
                ensure_func_ref("kryos_arc_alloc", builder, translator, module)?;
            let val = translate_operand(inner, builder, translator)?;
            let call_inst = builder.ins().call(func_ref, &[val]);
            let results = builder.inst_results(call_inst);
            Ok(Some(results[0]))
        }

        RValue::Cast { operand, ty } => {
            let val = translate_operand(operand, builder, translator)?;
            let src_ty = type_of_operand_hint(operand, &translator.mir_func.locals);
            let dest_ty = mir_type_to_cl(ty)?.unwrap_or(types::I64);
            let result = translate_cast(val, src_ty, dest_ty, builder)?;
            Ok(Some(result))
        }
    }
}

// ---------------------------------------------------------------------------
// Operand translation
// ---------------------------------------------------------------------------

fn translate_operand(
    operand: &Operand,
    builder: &mut FunctionBuilder,
    translator: &FuncTranslator,
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
            Constant::Str(_) => Ok(builder.ins().iconst(types::I64, 0)),
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
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    if is_float {
        translate_binop_float(op, lhs, rhs, builder)
    } else {
        translate_binop_int(op, lhs, rhs, builder)
    }
}

fn translate_binop_int(
    op: MirBinOp,
    lhs: cranelift_codegen::ir::Value,
    rhs: cranelift_codegen::ir::Value,
    builder: &mut FunctionBuilder,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    let val = match op {
        MirBinOp::Add => builder.ins().iadd(lhs, rhs),
        MirBinOp::Sub => builder.ins().isub(lhs, rhs),
        MirBinOp::Mul => builder.ins().imul(lhs, rhs),
        MirBinOp::Div => builder.ins().sdiv(lhs, rhs),
        MirBinOp::Mod => builder.ins().srem(lhs, rhs),
        MirBinOp::Pow => {
            // Cranelift has no integer power instruction. Emit a loop-based
            // exponentiation or call a runtime helper. For now, return lhs as
            // a placeholder — the runtime will provide `kryos_ipow`.
            return Err(CodegenError::UnsupportedOperation(
                "integer exponentiation (Pow) not yet supported in Cranelift backend".to_string(),
            ));
        }
        MirBinOp::Eq => {
            let cmp = builder.ins().icmp(IntCC::Equal, lhs, rhs);
            cmp
        }
        MirBinOp::Neq => {
            builder.ins().icmp(IntCC::NotEqual, lhs, rhs)
        }
        MirBinOp::Lt => {
            builder.ins().icmp(IntCC::SignedLessThan, lhs, rhs)
        }
        MirBinOp::Gt => {
            builder.ins().icmp(IntCC::SignedGreaterThan, lhs, rhs)
        }
        MirBinOp::LtEq => {
            builder
                .ins()
                .icmp(IntCC::SignedLessThanOrEqual, lhs, rhs)
        }
        MirBinOp::GtEq => {
            builder
                .ins()
                .icmp(IntCC::SignedGreaterThanOrEqual, lhs, rhs)
        }
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
        MirBinOp::Eq => {
            builder.ins().fcmp(FloatCC::Equal, lhs, rhs)
        }
        MirBinOp::Neq => {
            builder.ins().fcmp(FloatCC::NotEqual, lhs, rhs)
        }
        MirBinOp::Lt => {
            builder.ins().fcmp(FloatCC::LessThan, lhs, rhs)
        }
        MirBinOp::Gt => {
            builder.ins().fcmp(FloatCC::GreaterThan, lhs, rhs)
        }
        MirBinOp::LtEq => {
            builder.ins().fcmp(FloatCC::LessThanOrEqual, lhs, rhs)
        }
        MirBinOp::GtEq => {
            builder
                .ins()
                .fcmp(FloatCC::GreaterThanOrEqual, lhs, rhs)
        }
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

fn translate_terminator(
    term: &Terminator,
    builder: &mut FunctionBuilder,
    translator: &FuncTranslator,
) -> Result<(), CodegenError> {
    match term {
        Terminator::Return(None) => {
            builder.ins().return_(&[]);
        }
        Terminator::Return(Some(operand)) => {
            let val = translate_operand(operand, builder, translator)?;
            builder.ins().return_(&[val]);
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
            let cond_val = translate_operand(cond, builder, translator)?;
            let then_cl = translator.blocks[&then_block.0];
            let else_cl = translator.blocks[&else_block.0];
            builder.ins().brif(cond_val, then_cl, &[], else_cl, &[]);
        }
        Terminator::Switch {
            value,
            targets,
            default,
        } => {
            let val = translate_operand(value, builder, translator)?;
            let default_cl = translator.blocks[&default.0];

            // Emit a chain of brif instructions for each target.
            // For a small number of targets this is fine; a br_table would
            // be better for large switches but requires contiguous values.
            if targets.is_empty() {
                builder.ins().jump(default_cl, &[]);
            } else {
                for (i, (case_val, block_id)) in targets.iter().enumerate() {
                    let target_cl = translator.blocks[&block_id.0];
                    let case_const = builder.ins().iconst(types::I64, *case_val);
                    let cmp = builder.ins().icmp(IntCC::Equal, val, case_const);

                    if i + 1 == targets.len() {
                        // Last case: branch to target or default.
                        builder
                            .ins()
                            .brif(cmp, target_cl, &[], default_cl, &[]);
                    } else {
                        // More cases follow: branch to target or fall through
                        // to next comparison.
                        let next_block = builder.create_block();
                        builder
                            .ins()
                            .brif(cmp, target_cl, &[], next_block, &[]);
                        builder.seal_block(next_block);
                        builder.switch_to_block(next_block);
                    }
                }
            }
        }
        Terminator::Unreachable => {
            builder.ins().trap(cranelift_codegen::ir::TrapCode::user(0).unwrap());
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
    if let Some(func_ref) = translator.func_refs.get(name) {
        return Ok(*func_ref);
    }

    // Look up or declare the function in the module.
    let func_id = if let Some(id) = translator.func_ids.get(name) {
        *id
    } else {
        // Unknown function — declare it as an import with a generic signature.
        // In a real compiler, the MIR would carry type info for external calls.
        let mut sig = Signature::new(module.isa().default_call_conv());
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        module.declare_function(name, Linkage::Import, &sig)?
    };

    let func_ref = module.declare_func_in_func(func_id, builder.func);
    translator.func_refs.insert(name.to_string(), func_ref);
    Ok(func_ref)
}
