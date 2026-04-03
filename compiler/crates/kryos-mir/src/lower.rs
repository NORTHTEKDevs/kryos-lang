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
    /// Struct definitions: struct_name -> (field_name -> MirType).
    struct_defs: HashMap<String, HashMap<String, MirType>>,
    /// Function return types: func_name -> MirType.
    func_ret_types: HashMap<String, MirType>,
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
            func_ret_types: HashMap::new(),
        }
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
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Lower an entire AST module to MIR.
pub fn lower_module(module: &ast::Module) -> MirModule {
    let mut ctx = LoweringContext::new();

    // Pre-pass: collect struct definitions and function return types so the
    // lowerer can infer correct types for field accesses and call results.
    for decl in &module.declarations {
        match decl {
            ast::Decl::Struct { name, fields, .. } => {
                let mut field_map = HashMap::new();
                for f in fields {
                    field_map.insert(f.name.clone(), lower_type_expr(&f.ty));
                }
                ctx.struct_defs.insert(name.clone(), field_map);
            }
            ast::Decl::Function { name, ret_ty, .. } => {
                let mir_ret = match ret_ty {
                    Some(ty) => lower_type_expr(ty),
                    None => MirType::Void,
                };
                ctx.func_ret_types.insert(name.clone(), mir_ret);
            }
            _ => {}
        }
    }

    let mut functions = Vec::new();

    for decl in &module.declarations {
        if let ast::Decl::Function {
            name,
            params,
            ret_ty,
            body: Some(body),
            ..
        } = decl
        {
            functions.push(lower_function(&mut ctx, name, params, ret_ty, body));
        }
    }

    MirModule { functions }
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

        // Spawn, Select, TryCatch, Throw — emit Nop placeholders for now.
        _ => {
            ctx.emit(Instruction::Nop);
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
    // Desugar `for x in iterable { body }` to:
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

// ---------------------------------------------------------------------------
// Match lowering
// ---------------------------------------------------------------------------

fn lower_match(ctx: &mut LoweringContext, subject: &ast::Expr, arms: &[ast::MatchArm]) -> Operand {
    let subj_op = lower_expr_to_operand(ctx, subject);
    let result_local = ctx.alloc_temp(MirType::I64);
    let merge_bb = ctx.alloc_block();

    // Collect integer-literal arms for Switch, fallback to default.
    let mut targets: Vec<(i64, BlockId)> = Vec::new();
    let mut arm_blocks: Vec<(BlockId, &ast::Expr)> = Vec::new();
    let mut default_arm: Option<(BlockId, &ast::Expr)> = None;

    for arm in arms {
        let arm_bb = ctx.alloc_block();
        match &arm.pattern {
            ast::Pattern::Literal { expr, .. } => {
                if let ast::Expr::IntLiteral { value, .. } = expr.as_ref() {
                    targets.push((*value, arm_bb));
                    arm_blocks.push((arm_bb, &arm.body));
                } else {
                    // Non-integer literal -> treat as default.
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
    let first_arm_bb = arm_blocks.first().map(|(bb, _)| *bb).unwrap_or(default_bb);
    let _ = first_arm_bb;
    ctx.finish_block(
        Terminator::Switch {
            value: subj_op,
            targets,
            default: default_bb,
        },
        if let Some((bb, _)) = arm_blocks.first() {
            *bb
        } else {
            default_bb
        },
    );

    // Emit each arm block.
    for (i, (arm_bb, body)) in arm_blocks.iter().enumerate() {
        // The current_block should already be `arm_bb` for the first one (set
        // by finish_block above). For subsequent ones, we need to set it.
        if i > 0 {
            ctx.current_block = *arm_bb;
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
                    if let Some(field_ty) = fields.get(field.as_str()) {
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

        ast::Expr::FnCall { callee, .. } => {
            // If the callee is a simple identifier, look up the return type.
            if let ast::Expr::Identifier { name, .. } = callee.as_ref() {
                if let Some(ret_ty) = ctx.func_ret_types.get(name.as_str()) {
                    return ret_ty.clone();
                }
            }
            MirType::I64
        }

        ast::Expr::MethodCall { method, .. } => {
            if let Some(ret_ty) = ctx.func_ret_types.get(method.as_str()) {
                return ret_ty.clone();
            }
            MirType::I64
        }

        ast::Expr::StructLiteral { name, .. } => MirType::Struct(name.clone()),
        ast::Expr::ArrayLiteral { .. } => MirType::I64,
        ast::Expr::TupleLiteral { .. } => MirType::I64,

        ast::Expr::Cast { ty, .. } => lower_type_expr(ty),

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
            RValue::Call {
                func: method.clone(),
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
        Type::Struct { name, .. } | Type::Enum { name, .. } => MirType::Struct(name.clone()),
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
