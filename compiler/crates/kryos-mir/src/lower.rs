//! AST -> MIR lowering pass.
//!
//! Converts a typed Kryos AST (`kryos_ast::Module`) into the MIR control-flow
//! graph representation (`MirModule`).  The lowerer walks each function body,
//! creating basic blocks, instructions, and terminators.

use std::collections::HashMap;

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
    /// Impl-for-trait map: (type_name, trait_name) -> list of mangled method names.
    impl_map: HashMap<(String, String), Vec<String>>,
    /// Generic function templates: func_name -> (generic_params, AST function decl).
    /// These are not lowered immediately; they are instantiated on demand at call sites.
    generic_templates: HashMap<String, GenericTemplate>,
    /// Already-monomorphized specializations, to avoid duplicate lowering.
    monomorphized: HashMap<String, bool>,
    /// Functions produced by monomorphization (collected after lowering).
    monomorphized_functions: Vec<MirFunction>,
    /// Counter for anonymous lambda function names.
    lambda_counter: u32,
    /// Type alias map: alias_name -> resolved MirType.
    type_aliases: HashMap<String, MirType>,
}

/// Stores a generic function's AST for deferred monomorphization.
struct GenericTemplate {
    generic_params: Vec<String>,
    params: Vec<ast::Param>,
    ret_ty: Option<ast::TypeExpr>,
    body: ast::Block,
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
            impl_map: HashMap::new(),
            generic_templates: HashMap::new(),
            monomorphized: HashMap::new(),
            monomorphized_functions: Vec::new(),
            lambda_counter: 0,
            type_aliases: HashMap::new(),
        }
    }

    // ----- type resolution -----

    /// Resolve a type, checking type aliases first.
    #[allow(dead_code)]
    fn resolve_type(&self, ty: &ast::TypeExpr) -> MirType {
        let mir_ty = lower_type_expr(ty);
        // If the result is a Struct with a name that matches a type alias, resolve it.
        if let MirType::Struct(ref name) = mir_ty {
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
    }

    /// Save the per-function state so we can restore it after monomorphization.
    fn save_function_state(&self) -> FunctionState {
        FunctionState {
            locals: self.locals.clone(),
            blocks: self.blocks.clone(),
            current_instructions: self.current_instructions.clone(),
            current_block: self.current_block,
            next_local: self.next_local,
            next_block: self.next_block,
            loop_headers: self.loop_headers.clone(),
            loop_exits: self.loop_exits.clone(),
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
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Lower an entire AST module to MIR.
pub fn lower_module(module: &ast::Module) -> MirModule {
    let mut ctx = LoweringContext::new();

    // Register built-in prelude enums (Option, Result) so they're available
    // to all programs without explicit import.
    ctx.enum_defs.insert("Option".to_string(), vec![
        EnumVariantDef { name: "Some".to_string(), fields: vec![MirType::I64] },
        EnumVariantDef { name: "None".to_string(), fields: vec![] },
    ]);
    ctx.enum_defs.insert("Result".to_string(), vec![
        EnumVariantDef { name: "Ok".to_string(), fields: vec![MirType::I64] },
        EnumVariantDef { name: "Err".to_string(), fields: vec![MirType::I64] },
    ]);

    // Pre-pass: collect struct definitions and function return types so the
    // lowerer can infer correct types for field accesses and call results.
    for decl in &module.declarations {
        match decl {
            ast::Decl::Struct { name, fields, .. } => {
                let field_list: Vec<(String, MirType)> = fields
                    .iter()
                    .map(|f| (f.name.clone(), lower_type_expr(&f.ty)))
                    .collect();
                ctx.struct_defs.insert(name.clone(), field_list);
            }
            ast::Decl::Enum { name, variants, .. } => {
                let variant_defs: Vec<EnumVariantDef> = variants
                    .iter()
                    .map(|v| EnumVariantDef {
                        name: v.name.clone(),
                        fields: v.fields.iter().map(|t| lower_type_expr(t)).collect(),
                    })
                    .collect();
                ctx.enum_defs.insert(name.clone(), variant_defs);
            }
            ast::Decl::Function { name, generics, params, ret_ty, body, .. } => {
                let mir_ret = match ret_ty {
                    Some(ty) => lower_type_expr(ty),
                    None => MirType::Void,
                };
                ctx.func_ret_types.insert(name.clone(), mir_ret);

                // If this function has generic params, store it as a template
                // for monomorphization instead of lowering it immediately.
                if !generics.is_empty() {
                    if let Some(body) = body {
                        ctx.generic_templates.insert(name.clone(), GenericTemplate {
                            generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                            params: params.clone(),
                            ret_ty: ret_ty.clone(),
                            body: body.clone(),
                        });
                    }
                }
            }
            ast::Decl::Trait { name, methods, .. } => {
                let method_sigs: Vec<TraitMethodSig> = methods
                    .iter()
                    .filter_map(|m| {
                        if let ast::Decl::Function { name, params, ret_ty, .. } = m {
                            let param_types: Vec<MirType> = params
                                .iter()
                                .filter(|p| p.name != "self")
                                .map(|p| {
                                    p.ty.as_ref()
                                        .map(|t| lower_type_expr(t))
                                        .unwrap_or(MirType::I64)
                                })
                                .collect();
                            let ret = match ret_ty {
                                Some(ty) => lower_type_expr(ty),
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
            }
            ast::Decl::Impl { target, trait_name, methods, .. } => {
                // Register mangled method names in func_ret_types.
                for method in methods {
                    if let ast::Decl::Function { name, ret_ty, .. } = method {
                        let mangled = format!("{target}__{name}");
                        let mir_ret = match ret_ty {
                            Some(ty) => lower_type_expr(ty),
                            None => MirType::Void,
                        };
                        ctx.func_ret_types.insert(mangled, mir_ret);
                    }
                }
                // Track which methods belong to which type for method call resolution.
                for method in methods {
                    if let ast::Decl::Function { name, .. } = method {
                        ctx.method_owners.insert(
                            (target.clone(), name.clone()),
                            format!("{target}__{name}"),
                        );
                    }
                }
                // If implementing a trait, record in impl_map.
                if let Some(trait_name) = trait_name {
                    let mangled_names: Vec<String> = methods
                        .iter()
                        .filter_map(|m| {
                            if let ast::Decl::Function { name, .. } = m {
                                Some(format!("{target}__{name}"))
                            } else {
                                None
                            }
                        })
                        .collect();
                    ctx.impl_map.insert(
                        (target.clone(), trait_name.clone()),
                        mangled_names,
                    );
                }
            }
            ast::Decl::TypeAlias { name, ty, .. } => {
                let mir_ty = lower_type_expr(ty);
                ctx.type_aliases.insert(name.clone(), mir_ty);
            }
            ast::Decl::Extern { items, .. } => {
                // Register extern function signatures so they can be called.
                for item in items {
                    if let ast::Decl::Function { name, ret_ty, .. } = item {
                        let mir_ret = match ret_ty {
                            Some(ty) => lower_type_expr(ty),
                            None => MirType::Void,
                        };
                        ctx.func_ret_types.insert(name.clone(), mir_ret);
                    }
                }
            }
            ast::Decl::Actor { name, state_fields, handlers, .. } => {
                // Register actor state as a struct def.
                let fields: Vec<(String, MirType)> = state_fields
                    .iter()
                    .map(|f| (f.name.clone(), lower_type_expr(&f.ty)))
                    .collect();
                ctx.struct_defs.insert(name.clone(), fields);
                // Register handler signatures.
                for handler in handlers {
                    let mangled = format!("{name}__{}", handler.name);
                    let mir_ret = match &handler.ret_ty {
                        Some(ty) => lower_type_expr(ty),
                        None => MirType::Void,
                    };
                    ctx.func_ret_types.insert(mangled.clone(), mir_ret.clone());
                    ctx.method_owners.insert(
                        (name.clone(), handler.name.clone()),
                        mangled,
                    );
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
                ..
            } => {
                // Skip generic functions — they are lowered on demand via monomorphization.
                if !generics.is_empty() {
                    continue;
                }
                functions.push(lower_function(&mut ctx, name, params, ret_ty, body));
            }
            ast::Decl::Impl { target, methods, .. } => {
                // Lower each method as a free function with mangled name.
                for method in methods {
                    if let ast::Decl::Function {
                        name,
                        params,
                        ret_ty,
                        body: Some(body),
                        ..
                    } = method
                    {
                        let mangled = format!("{target}__{name}");
                        let mut all_params = Vec::new();
                        let has_self = params.iter().any(|p| p.name == "self");
                        if has_self {
                            all_params.extend_from_slice(params);
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
                        functions.push(lower_function(&mut ctx, &mangled, &all_params, ret_ty, body));
                    }
                }
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

    // Collect monomorphized specializations generated during lowering.
    functions.extend(ctx.monomorphized_functions.drain(..));

    MirModule {
        functions,
        struct_defs: ctx.struct_defs.clone(),
        enum_defs: ctx.enum_defs.clone(),
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

    // Lower return type.
    let mir_ret_ty = match ret_ty {
        Some(ty) => lower_type_expr(ty),
        None => MirType::Void,
    };

    // Lower parameters -> locals.
    let mir_params: Vec<MirParam> = params
        .iter()
        .map(|p| {
            let ty = p
                .ty
                .as_ref()
                .map(|t| lower_type_expr(t))
                .unwrap_or(MirType::I64);
            let local = ctx.alloc_local(Some(p.name.clone()), ty.clone(), false);
            MirParam { local, ty }
        })
        .collect();

    // Lower the body statements.
    lower_block_stmts(ctx, &body.stmts);

    // If the current block hasn't been sealed yet, add an implicit return.
    if ctx.blocks.len() < ctx.next_block as usize {
        if mir_ret_ty == MirType::Void {
            ctx.seal_block(Terminator::Return(None));
        } else {
            // Implicit return of last expression if present is handled by
            // `lower_block_stmts`; if we reach here we still need a return.
            ctx.seal_block(Terminator::Return(None));
        }
    }

    MirFunction {
        name: name.to_string(),
        params: mir_params,
        ret_ty: mir_ret_ty,
        blocks: ctx.blocks.clone(),
        locals: ctx.locals.clone(),
    }
}

// ---------------------------------------------------------------------------
// Statement lowering
// ---------------------------------------------------------------------------

fn lower_block_stmts(ctx: &mut LoweringContext, stmts: &[ast::Stmt]) {
    // Collect locals declared in this scope so we can emit Drops.
    let scope_locals: Vec<LocalId> = Vec::new();
    let scope_start = ctx.locals.len();

    for stmt in stmts {
        lower_stmt(ctx, stmt);
    }

    // Emit drops for locals declared in this scope (reverse order).
    let scope_end = ctx.locals.len();
    for i in (scope_start..scope_end).rev() {
        let local_id = ctx.locals[i].id;
        // Don't drop scope_locals that are also parameters (they are
        // owned by the caller).
        if !scope_locals.contains(&local_id) {
            ctx.emit(Instruction::Drop { local: local_id });
        }
    }
}

fn lower_stmt(ctx: &mut LoweringContext, stmt: &ast::Stmt) {
    match stmt {
        ast::Stmt::Let {
            name,
            mutable,
            ty,
            value,
            ..
        } => {
            let mir_ty = if let Some(t) = ty {
                lower_type_expr(t)
            } else if let Some(expr) = value {
                // No explicit type annotation — infer from the initializer.
                infer_expr_type(ctx, expr)
            } else {
                MirType::I64
            };
            let local = ctx.alloc_local(Some(name.clone()), mir_ty, *mutable);

            if let Some(expr) = value {
                let rvalue = lower_expr_to_rvalue(ctx, expr);

                // If the value is Shared, emit ArcRetain.
                if matches!(expr, ast::Expr::SharedExpr { .. }) {
                    ctx.emit(Instruction::Assign {
                        dest: local,
                        value: rvalue,
                    });
                    ctx.emit(Instruction::ArcRetain { ptr: local });
                } else {
                    ctx.emit(Instruction::Assign {
                        dest: local,
                        value: rvalue,
                    });
                }
            }
        }

        ast::Stmt::Assign {
            target,
            op,
            value,
            ..
        } => {
            match op {
                ast::AssignOp::Assign => {
                    // For simple assignment to an identifier, find the local.
                    if let ast::Expr::Identifier { name, .. } = target {
                        let dest = find_local_by_name(ctx, name);
                        let rvalue = lower_expr_to_rvalue(ctx, value);
                        ctx.emit(Instruction::Assign {
                            dest,
                            value: rvalue,
                        });
                    } else {
                        // Complex assignment target — use a temp.
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
                        let dest = find_local_by_name(ctx, name);
                        let mir_op = match op {
                            ast::AssignOp::AddAssign => MirBinOp::Add,
                            ast::AssignOp::SubAssign => MirBinOp::Sub,
                            ast::AssignOp::MulAssign => MirBinOp::Mul,
                            ast::AssignOp::DivAssign => MirBinOp::Div,
                            ast::AssignOp::Assign => unreachable!(),
                        };
                        let rhs = lower_expr_to_operand(ctx, value);
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
            pattern,
            iterable,
            body,
            ..
        } => {
            lower_for(ctx, pattern, iterable, body);
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
            if matches!(&rvalue, RValue::Call { .. }) {
                let temp = ctx.alloc_temp(MirType::Void);
                ctx.emit(Instruction::Assign {
                    dest: temp,
                    value: rvalue,
                });
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
            // Lower `throw expr` as constructing Result::Err(expr).
            let val = lower_expr_to_operand(ctx, expr);
            let err_local = ctx.alloc_temp(MirType::Enum("Result".into()));
            ctx.emit(Instruction::Assign {
                dest: err_local,
                value: RValue::EnumVariant {
                    enum_name: "Result".into(),
                    variant_idx: 1, // Err
                    fields: vec![val],
                },
            });
        }

        ast::Stmt::Spawn { expr, .. } => {
            // Lower spawn: evaluate the expression into a temp, emit Spawn instruction.
            let rvalue = lower_expr_to_rvalue(ctx, expr);
            let task_local = ctx.alloc_temp(MirType::I64);
            ctx.emit(Instruction::Assign {
                dest: task_local,
                value: rvalue,
            });
            ctx.emit(Instruction::Spawn { task: task_local });
        }

        ast::Stmt::Select { branches, .. } => {
            // Lower select: evaluate each channel, emit a Switch on readiness.
            // Each branch becomes: receive from channel → run body.
            let merge_bb = ctx.alloc_block();

            if branches.is_empty() {
                ctx.emit(Instruction::Nop);
                return;
            }

            // Build switch targets: tag i → branch_bb_i.
            let mut targets = Vec::new();
            let mut branch_bbs = Vec::new();
            for (i, branch) in branches.iter().enumerate() {
                let bb = ctx.alloc_block();
                targets.push((i as i64, bb));
                branch_bbs.push((bb, branch));
            }

            // Emit a switch on a readiness index (simplified: sequential check).
            let select_idx = ctx.alloc_temp(MirType::I64);
            ctx.emit(Instruction::Assign {
                dest: select_idx,
                value: RValue::ConstInt(0), // runtime would populate this
            });
            let default_bb = merge_bb;
            ctx.finish_block(
                Terminator::Switch {
                    value: Operand::Local(select_idx),
                    targets: targets.clone(),
                    default: default_bb,
                },
                branch_bbs[0].0,
            );

            // Lower each branch body.
            for (i, (_bb, branch)) in branch_bbs.into_iter().enumerate() {
                // At the start of each branch, receive from the channel.
                let ch_op = lower_expr_to_operand(ctx, &branch.channel);
                let ch_local = ctx.alloc_temp(MirType::I64);
                ctx.emit(Instruction::Assign {
                    dest: ch_local,
                    value: RValue::Use(ch_op),
                });
                let recv_local = ctx.alloc_local(
                    Some(branch.pattern.clone()),
                    MirType::I64,
                    false,
                );
                ctx.emit(Instruction::Receive {
                    dest: recv_local,
                    channel: ch_local,
                });

                lower_block_stmts(ctx, &branch.body.stmts);

                let next_bb = if i + 1 < targets.len() {
                    targets[i + 1].1
                } else {
                    merge_bb
                };
                ctx.finish_block(Terminator::Goto(merge_bb), next_bb);
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
        }
        ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
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

    // General case: desugar `for x in iterable { body }` to:
    //   let _iter = iterable;
    //   let _idx  = 0;
    //   while _idx < len(_iter) {
    //       let x = _iter[_idx];
    //       body;
    //       _idx += 1;
    //   }

    let iter_local = ctx.alloc_temp(MirType::I64);
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
    let loop_var = ctx.alloc_local(Some(loop_var_name), MirType::I64, false);
    ctx.emit(Instruction::Assign {
        dest: loop_var,
        value: RValue::Index {
            object: Operand::Local(iter_local),
            index: Operand::Local(idx_local),
        },
    });

    ctx.loop_headers.push(header_bb);
    ctx.loop_exits.push(exit_bb);
    lower_block_stmts(ctx, &body.stmts);
    ctx.loop_headers.pop();
    ctx.loop_exits.pop();

    // Increment idx.
    ctx.emit(Instruction::Assign {
        dest: idx_local,
        value: RValue::BinOp {
            op: MirBinOp::Add,
            left: Operand::Local(idx_local),
            right: Operand::Constant(Constant::Int(1)),
        },
    });

    // Back-edge.
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

    ctx.loop_headers.push(header_bb);
    ctx.loop_exits.push(exit_bb);
    lower_block_stmts(ctx, &body.stmts);
    ctx.loop_headers.pop();
    ctx.loop_exits.pop();

    // Increment: _idx += 1
    ctx.emit(Instruction::Assign {
        dest: idx_local,
        value: RValue::BinOp {
            op: MirBinOp::Add,
            left: Operand::Local(idx_local),
            right: Operand::Constant(Constant::Int(1)),
        },
    });

    // Back-edge.
    ctx.finish_block(Terminator::Goto(header_bb), exit_bb);
}

// ---------------------------------------------------------------------------
// Try/Catch lowering
// ---------------------------------------------------------------------------

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

    // Lower the try block body. The last expression is wrapped in Result::Ok.
    for (i, stmt) in try_block.stmts.iter().enumerate() {
        if i == try_block.stmts.len() - 1 {
            // Wrap last expression in Result::Ok.
            if let ast::Stmt::Expr { expr, .. } = stmt {
                let val = lower_expr_to_operand(ctx, expr);
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

    // Extract tag and branch.
    let tag_local = ctx.alloc_temp(MirType::I64);
    ctx.emit(Instruction::Assign {
        dest: tag_local,
        value: RValue::EnumTag { operand: Operand::Local(result_local) },
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
            variant_idx: 0,
            field_idx: 0,
        },
    });
    ctx.finish_block(Terminator::Goto(merge_bb), err_bb);

    // Err path: bind error value to catch_name, execute handler.
    let err_payload = ctx.alloc_local(Some(catch_name.to_string()), MirType::I64, false);
    ctx.emit(Instruction::Assign {
        dest: err_payload,
        value: RValue::EnumPayload {
            operand: Operand::Local(result_local),
            variant_idx: 1,
            field_idx: 0,
        },
    });
    lower_block_stmts(ctx, &catch_block.stmts);
    ctx.finish_block(Terminator::Goto(merge_bb), merge_bb);
}

// ---------------------------------------------------------------------------
// Match lowering
// ---------------------------------------------------------------------------

/// Per-arm enum binding: (variant_idx, field_patterns).
struct EnumBinding {
    variant_idx: u32,
    field_patterns: Vec<ast::Pattern>,
}

fn lower_match(ctx: &mut LoweringContext, subject: &ast::Expr, arms: &[ast::MatchArm]) -> Operand {
    let subj_op = lower_expr_to_operand(ctx, subject);
    let result_local = ctx.alloc_temp(MirType::I64);
    let merge_bb = ctx.alloc_block();

    // Detect enum match: any arm uses Pattern::Enum.
    let is_enum_match = arms.iter().any(|a| matches!(&a.pattern, ast::Pattern::Enum { .. }));

    // For enum matches, extract the tag first and switch on that.
    let switch_op = if is_enum_match {
        let tag_local = ctx.alloc_temp(MirType::I64);
        ctx.emit(Instruction::Assign {
            dest: tag_local,
            value: RValue::EnumTag { operand: subj_op.clone() },
        });
        Operand::Local(tag_local)
    } else {
        subj_op.clone()
    };

    // Collect arms into switch targets.
    let mut targets: Vec<(i64, BlockId)> = Vec::new();
    let mut arm_blocks: Vec<(BlockId, &ast::Expr, Option<EnumBinding>)> = Vec::new();
    let mut default_arm: Option<(BlockId, &ast::Expr)> = None;

    for arm in arms {
        let arm_bb = ctx.alloc_block();
        match &arm.pattern {
            ast::Pattern::Enum { name, variant, fields, .. } => {
                if let Some(variants) = ctx.enum_defs.get(name.as_str()) {
                    if let Some(idx) = variants.iter().position(|v| v.name == *variant) {
                        targets.push((idx as i64, arm_bb));
                        arm_blocks.push((arm_bb, &arm.body, Some(EnumBinding {
                            variant_idx: idx as u32,
                            field_patterns: fields.clone(),
                        })));
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
                } else {
                    default_arm = Some((arm_bb, &arm.body));
                }
            }
            ast::Pattern::Wildcard { .. } | ast::Pattern::Ident { .. } => {
                default_arm = Some((arm_bb, &arm.body));
            }
            _ => {
                default_arm = Some((arm_bb, &arm.body));
            }
        }
    }

    let default_bb = default_arm
        .map(|(bb, _)| bb)
        .unwrap_or(merge_bb);

    // Emit switch terminator.
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

    // Emit each arm block.
    for (i, (arm_bb, body, enum_binding)) in arm_blocks.iter().enumerate() {
        if i > 0 {
            ctx.current_block = *arm_bb;
        }

        // For enum arms, extract payload fields and bind to locals.
        if let Some(binding) = enum_binding {
            for (field_idx, pat) in binding.field_patterns.iter().enumerate() {
                if let ast::Pattern::Ident { name, .. } = pat {
                    let local = ctx.alloc_local(Some(name.clone()), MirType::I64, false);
                    ctx.emit(Instruction::Assign {
                        dest: local,
                        value: RValue::EnumPayload {
                            operand: subj_op.clone(),
                            variant_idx: binding.variant_idx,
                            field_idx: field_idx as u32,
                        },
                    });
                }
                // Wildcard patterns — skip, no binding needed.
            }
        }

        let arm_rvalue = lower_expr_to_rvalue(ctx, body);
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

/// Best-effort inference of a MIR type for an AST expression.
///
/// Uses struct definitions and function return types collected during the
/// pre-pass to resolve field accesses and call results.  Falls back to I64
/// for anything it can't resolve.
fn infer_expr_type(ctx: &LoweringContext, expr: &ast::Expr) -> MirType {
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
            // Look up the local's MIR type.
            ctx.locals
                .iter()
                .rev()
                .find(|l| l.name.as_deref() == Some(name))
                .map(|l| l.ty.clone())
                .unwrap_or(MirType::I64)
        }

        ast::Expr::FieldAccess { object, field, .. } => {
            // Resolve the object's type, then look up the field in struct_defs.
            let obj_ty = infer_expr_type(ctx, object);
            if let MirType::Struct(name) = &obj_ty {
                if let Some(fields) = ctx.struct_defs.get(name) {
                    if let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == field.as_str()) {
                        return field_ty.clone();
                    }
                }
            }
            MirType::I64
        }

        ast::Expr::BinaryOp { left, right, op, .. } => {
            // Comparison operators always produce bool.
            match op {
                ast::BinOp::Eq | ast::BinOp::Neq | ast::BinOp::Lt
                | ast::BinOp::Gt | ast::BinOp::LtEq | ast::BinOp::GtEq
                | ast::BinOp::And | ast::BinOp::Or => return MirType::Bool,
                _ => {}
            }
            // For arithmetic, propagate the type of the left operand; if
            // either side is float, the result is float.
            let lty = infer_expr_type(ctx, left);
            let rty = infer_expr_type(ctx, right);
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
                // For generic functions, the return type depends on argument types.
                if let Some(template) = ctx.generic_templates.get(name.as_str()) {
                    let generic_params = template.generic_params.clone();
                    let template_params = template.params.clone();
                    let template_ret_ty = template.ret_ty.clone();
                    // Infer type map from arguments.
                    let mut type_map: HashMap<String, MirType> = HashMap::new();
                    for (i, param) in template_params.iter().enumerate() {
                        if let Some(ty_expr) = &param.ty {
                            if let ast::TypeExpr::Simple { name: tn, .. } = ty_expr {
                                if generic_params.contains(tn) {
                                    if let Some(arg) = args.get(i) {
                                        type_map.insert(tn.clone(), infer_expr_type(ctx, arg));
                                    }
                                }
                            }
                        }
                    }
                    if let Some(ret_ty) = &template_ret_ty {
                        if let ast::TypeExpr::Simple { name: rn, .. } = ret_ty {
                            if let Some(concrete) = type_map.get(rn) {
                                return concrete.clone();
                            }
                        }
                        return lower_type_expr(ret_ty);
                    }
                    return MirType::Void;
                }
                if let Some(ret_ty) = ctx.func_ret_types.get(name.as_str()) {
                    return ret_ty.clone();
                }
            }
            MirType::I64
        }

        ast::Expr::MethodCall { object, method, .. } => {
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
            MirType::I64
        }

        ast::Expr::StructLiteral { name, .. } => MirType::Struct(name.clone()),
        ast::Expr::ArrayLiteral { .. } => MirType::I64,
        ast::Expr::TupleLiteral { .. } => MirType::I64,

        ast::Expr::Cast { ty, .. } => lower_type_expr(ty),

        ast::Expr::Lambda { ret_ty, .. } => {
            // A lambda expression's type is Function.
            MirType::Function {
                params: vec![MirType::I64], // simplified
                ret: Box::new(match ret_ty {
                    Some(ty) => lower_type_expr(ty),
                    None => MirType::I64,
                }),
            }
        }

        ast::Expr::PipeExpr { right, .. } => {
            // The pipe result type is the return type of the RHS function.
            infer_expr_type(ctx, right)
        }

        ast::Expr::MapLiteral { .. } => MirType::I64, // opaque map handle

        _ => MirType::I64,
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
        ast::Expr::StringLiteral { value, .. } => {
            Operand::Constant(Constant::Str(value.clone()))
        }
        ast::Expr::NoneLiteral { .. } => Operand::Constant(Constant::None),
        ast::Expr::Identifier { name, .. } => {
            let local = find_local_by_name(ctx, name);
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
            // Check if this is a unit enum variant (e.g., `None`, `Red`).
            if let Some((enum_name, variant_idx)) = find_enum_variant(ctx, name) {
                return RValue::EnumVariant {
                    enum_name,
                    variant_idx,
                    fields: vec![],
                };
            }
            let local = find_local_by_name(ctx, name);
            RValue::Use(Operand::Local(local))
        }

        ast::Expr::BinaryOp {
            op, left, right, ..
        } => {
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

            // Check if this is an enum variant constructor (e.g., `Some(42)`).
            if let Some((enum_name, variant_idx)) = find_enum_variant(ctx, &func_name) {
                let mir_args: Vec<Operand> = args
                    .iter()
                    .map(|a| lower_expr_to_operand(ctx, a))
                    .collect();
                return RValue::EnumVariant {
                    enum_name,
                    variant_idx,
                    fields: mir_args,
                };
            }

            // Check if this is a call to a generic function — monomorphize.
            if ctx.generic_templates.contains_key(&func_name) {
                let arg_types: Vec<MirType> = args
                    .iter()
                    .map(|a| infer_expr_type(ctx, a))
                    .collect();
                let mangled = monomorphize(ctx, &func_name, &arg_types);
                let mir_args: Vec<Operand> = args
                    .iter()
                    .map(|a| lower_expr_to_operand(ctx, a))
                    .collect();
                return RValue::Call {
                    func: mangled,
                    args: mir_args,
                };
            }

            let mir_args: Vec<Operand> = args
                .iter()
                .map(|a| lower_expr_to_operand(ctx, a))
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
            let obj = lower_expr_to_operand(ctx, object);
            let mut mir_args: Vec<Operand> = vec![obj];
            for a in args {
                mir_args.push(lower_expr_to_operand(ctx, a));
            }

            // Resolve mangled method name: infer the object's type and look up
            // the impl method as TypeName__method.
            let type_name = infer_type_name(ctx, object);
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
            let mir_fields: Vec<(String, Operand)> = fields
                .iter()
                .map(|(n, e)| (n.clone(), lower_expr_to_operand(ctx, e)))
                .collect();
            RValue::Struct {
                name: name.clone(),
                fields: mir_fields,
            }
        }

        ast::Expr::FieldAccess { object, field, .. } => {
            let obj = lower_expr_to_operand(ctx, object);
            RValue::Field {
                object: obj,
                field: field.clone(),
            }
        }

        ast::Expr::IndexAccess { object, index, .. } => {
            let obj = lower_expr_to_operand(ctx, object);
            let idx = lower_expr_to_operand(ctx, index);
            RValue::Index {
                object: obj,
                index: idx,
            }
        }

        ast::Expr::SharedExpr { inner, .. } => {
            let inner_op = lower_expr_to_operand(ctx, inner);
            RValue::ArcAlloc { inner: inner_op }
        }

        ast::Expr::Cast { expr, ty, .. } => {
            let inner = lower_expr_to_operand(ctx, expr);
            let mir_ty = lower_type_expr(ty);
            RValue::Cast {
                operand: inner,
                ty: mir_ty,
            }
        }

        ast::Expr::MatchExpr {
            subject, arms, ..
        } => {
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
            let result_local = ctx.alloc_temp(MirType::I64);
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
            if let Some(last) = then_branch.stmts.last() {
                if let ast::Stmt::Expr { expr, .. } = last {
                    let rv = lower_expr_to_rvalue(ctx, expr);
                    ctx.emit(Instruction::Assign {
                        dest: result_local,
                        value: rv,
                    });
                }
            }
            ctx.finish_block(Terminator::Goto(merge_bb), else_bb);

            // Else.
            if let Some(else_blk) = else_branch {
                if let Some(last) = else_blk.stmts.last() {
                    if let ast::Stmt::Expr { expr, .. } = last {
                        let rv = lower_expr_to_rvalue(ctx, expr);
                        ctx.emit(Instruction::Assign {
                            dest: result_local,
                            value: rv,
                        });
                    }
                }
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

            // Save state, lower the lambda as a standalone function.
            let saved = ctx.save_function_state();

            // Build params: captures first (as extra params), then declared params.
            let mut all_params: Vec<ast::Param> = captures
                .iter()
                .map(|name| ast::Param {
                    name: name.clone(),
                    ty: None,
                    default: None,
                    span: kryos_errors::Span::DUMMY,
                })
                .collect();
            all_params.extend_from_slice(params);

            // Create a block from the body expression.
            let body_block = ast::Block {
                stmts: vec![ast::Stmt::Return {
                    value: Some(body.as_ref().clone()),
                    span: kryos_errors::Span::DUMMY,
                }],
                span: kryos_errors::Span::DUMMY,
            };

            let mir_func = lower_function(
                ctx,
                &lambda_name,
                &all_params,
                ret_ty,
                &body_block,
            );
            ctx.restore_function_state(saved);
            ctx.monomorphized_functions.push(mir_func);

            // Register the lambda's return type.
            let mir_ret = match ret_ty {
                Some(ty) => lower_type_expr(ty),
                None => MirType::I64,
            };
            ctx.func_ret_types.insert(lambda_name.clone(), mir_ret);

            // Emit the closure RValue with captured variable operands.
            let capture_ops: Vec<Operand> = captures
                .iter()
                .map(|name| {
                    let local = find_local_by_name(ctx, name);
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
                ast::Expr::FnCall { callee, args, span: _ } => {
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
                .map(|(k, v)| {
                    (lower_expr_to_operand(ctx, k), lower_expr_to_operand(ctx, v))
                })
                .collect();
            RValue::Map(mir_entries)
        }

        ast::Expr::CharLiteral { value, .. } => RValue::ConstInt(*value as i64),

        ast::Expr::RangeExpr { start, end, inclusive, .. } => {
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

        // Fallback for unsupported expressions.
        _ => RValue::ConstNone,
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
        // Pipe and MatMul don't have direct MIR equivalents — lower as Add placeholders.
        ast::BinOp::Pipe => MirBinOp::Add,
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
            other => MirType::Struct(other.to_string()),
        },
        ast::TypeExpr::Array { element, size, .. } => {
            MirType::Array(Box::new(lower_type_expr(element)), *size)
        }
        ast::TypeExpr::Tuple { elements, .. } => {
            MirType::Tuple(elements.iter().map(|e| lower_type_expr(e)).collect())
        }
        ast::TypeExpr::Function { params, ret, .. } => MirType::Function {
            params: params.iter().map(|p| lower_type_expr(p)).collect(),
            ret: Box::new(lower_type_expr(ret)),
        },
        ast::TypeExpr::Shared { inner, .. } => {
            MirType::Shared(Box::new(lower_type_expr(inner)))
        }
        ast::TypeExpr::Pointer { inner, .. } => {
            MirType::Ptr(Box::new(lower_type_expr(inner)))
        }
        ast::TypeExpr::Generic { name, .. } => MirType::Struct(name.clone()),
        ast::TypeExpr::Optional { inner, .. } => {
            // Lower Optional<T> as Struct("Option") — codegen decides representation.
            let _ = lower_type_expr(inner);
            MirType::Struct("Option".to_string())
        }
        ast::TypeExpr::Reference { inner, .. } => {
            MirType::Ptr(Box::new(lower_type_expr(inner)))
        }
        ast::TypeExpr::Weak { inner, .. } => {
            // Lower Weak as Ptr — codegen adds weak-ref bookkeeping.
            MirType::Ptr(Box::new(lower_type_expr(inner)))
        }
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
            MirType::Tuple(elements.iter().map(|e| lower_resolved_type(e)).collect())
        }
        Type::Struct { name, .. } => MirType::Struct(name.clone()),
        Type::Enum { name, .. } => MirType::Enum(name.clone()),
        Type::Function { params, ret } => MirType::Function {
            params: params.iter().map(|p| lower_resolved_type(p)).collect(),
            ret: Box::new(lower_resolved_type(ret)),
        },
        Type::Shared { inner } => MirType::Shared(Box::new(lower_resolved_type(inner))),
        Type::Reference { inner, .. } | Type::Pointer { inner, .. } | Type::Weak { inner } => {
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
        Type::Map { .. } => MirType::Struct("Map".to_string()),
        Type::Set { .. } => MirType::Struct("Set".to_string()),
        Type::Var(_) | Type::Error => MirType::I64, // fallback
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Look up a local by name. Returns the local id if found, otherwise allocates
/// a new temporary (graceful degradation for unresolved names).
fn find_local_by_name(ctx: &mut LoweringContext, name: &str) -> LocalId {
    for local in ctx.locals.iter().rev() {
        if local.name.as_deref() == Some(name) {
            return local.id;
        }
    }
    // Not found — allocate a placeholder local.
    ctx.alloc_local(Some(name.to_string()), MirType::I64, false)
}

/// Infer the type name of an expression (for method call resolution).
/// Returns the struct/enum name if resolvable, None otherwise.
fn infer_type_name(ctx: &LoweringContext, expr: &ast::Expr) -> Option<String> {
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
                && ctx.locals.iter().any(|l| l.name.as_deref() == Some(name.as_str()))
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
        ast::Expr::FieldAccess { object, .. } => {
            collect_identifiers(object, bound, free_vars, seen, ctx);
        }
        ast::Expr::IndexAccess { object, index, .. } => {
            collect_identifiers(object, bound, free_vars, seen, ctx);
            collect_identifiers(index, bound, free_vars, seen, ctx);
        }
        ast::Expr::IfExpr { condition, then_branch, else_branch, .. } => {
            collect_identifiers(condition, bound, free_vars, seen, ctx);
            for s in &then_branch.stmts {
                if let ast::Stmt::Expr { expr, .. } = s {
                    collect_identifiers(expr, bound, free_vars, seen, ctx);
                }
            }
            if let Some(eb) = else_branch {
                for s in &eb.stmts {
                    if let ast::Stmt::Expr { expr, .. } = s {
                        collect_identifiers(expr, bound, free_vars, seen, ctx);
                    }
                }
            }
        }
        ast::Expr::Cast { expr, .. } => {
            collect_identifiers(expr, bound, free_vars, seen, ctx);
        }
        // For other expression types, we don't recurse deeper.
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Monomorphization
// ---------------------------------------------------------------------------

/// Produce a mangled name for a monomorphized specialization.
/// e.g., `id` with `[I64]` → `id___i64`.
fn mono_mangled_name(base: &str, concrete_types: &[MirType]) -> String {
    let suffix: Vec<String> = concrete_types.iter().map(|t| format!("{t}")).collect();
    format!("{base}___{}", suffix.join("_"))
}

/// Monomorphize a generic function template with concrete argument types.
///
/// Infers type parameter bindings from argument types, substitutes them
/// in the parameter/return type annotations, lowers the specialized copy,
/// and returns the mangled name.
fn monomorphize(
    ctx: &mut LoweringContext,
    func_name: &str,
    arg_types: &[MirType],
) -> String {
    // Build type param → concrete type mapping by matching args to params.
    let template = ctx.generic_templates.get(func_name).expect("template exists");
    let generic_params = template.generic_params.clone();
    let template_params = template.params.clone();
    let template_ret_ty = template.ret_ty.clone();
    let template_body = template.body.clone();

    let mut type_map: HashMap<String, MirType> = HashMap::new();
    for (i, param) in template_params.iter().enumerate() {
        if let Some(ty_expr) = &param.ty {
            if let ast::TypeExpr::Simple { name, .. } = ty_expr {
                if generic_params.contains(name) {
                    if let Some(concrete) = arg_types.get(i) {
                        type_map.insert(name.clone(), concrete.clone());
                    }
                }
            }
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

    // Register the return type for the specialized function.
    let specialized_ret = if let Some(ret_ty) = &template_ret_ty {
        if let ast::TypeExpr::Simple { name, .. } = ret_ty {
            if let Some(concrete) = type_map.get(name) {
                concrete.clone()
            } else {
                lower_type_expr(ret_ty)
            }
        } else {
            lower_type_expr(ret_ty)
        }
    } else {
        MirType::Void
    };
    ctx.func_ret_types.insert(mangled.clone(), specialized_ret);

    // Substitute type params in the parameter list.
    let specialized_params: Vec<ast::Param> = template_params
        .iter()
        .map(|p| {
            let new_ty = p.ty.as_ref().map(|ty_expr| substitute_type_expr(ty_expr, &type_map));
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
    let mir_func = lower_function(ctx, &mangled, &specialized_params, &specialized_ret_ty, &template_body);

    // Restore the caller's function state.
    ctx.restore_function_state(saved);

    // Store the monomorphized function for collection by lower_module.
    ctx.monomorphized_functions.push(mir_func);

    mangled
}

/// Substitute generic type parameters in a TypeExpr based on a type map.
fn substitute_type_expr(
    ty: &ast::TypeExpr,
    type_map: &HashMap<String, MirType>,
) -> ast::TypeExpr {
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
        ast::TypeExpr::Array { element, size, span } => ast::TypeExpr::Array {
            element: Box::new(substitute_type_expr(element, type_map)),
            size: *size,
            span: *span,
        },
        ast::TypeExpr::Tuple { elements, span } => ast::TypeExpr::Tuple {
            elements: elements.iter().map(|e| substitute_type_expr(e, type_map)).collect(),
            span: *span,
        },
        ast::TypeExpr::Function { params, ret, span } => ast::TypeExpr::Function {
            params: params.iter().map(|p| substitute_type_expr(p, type_map)).collect(),
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
                    args: args.iter().map(|a| substitute_type_expr(a, type_map)).collect(),
                    span: *span,
                }
            }
        }
        ast::TypeExpr::Shared { inner, span } => ast::TypeExpr::Shared {
            inner: Box::new(substitute_type_expr(inner, type_map)),
            span: *span,
        },
        ast::TypeExpr::Pointer { inner, mutable, span } => ast::TypeExpr::Pointer {
            inner: Box::new(substitute_type_expr(inner, type_map)),
            mutable: *mutable,
            span: *span,
        },
        ast::TypeExpr::Optional { inner, span } => ast::TypeExpr::Optional {
            inner: Box::new(substitute_type_expr(inner, type_map)),
            span: *span,
        },
        ast::TypeExpr::Reference { inner, mutable, span } => ast::TypeExpr::Reference {
            inner: Box::new(substitute_type_expr(inner, type_map)),
            mutable: *mutable,
            span: *span,
        },
        ast::TypeExpr::Weak { inner, span } => ast::TypeExpr::Weak {
            inner: Box::new(substitute_type_expr(inner, type_map)),
            span: *span,
        },
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
