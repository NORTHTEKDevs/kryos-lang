//! Constant folding pass.
//!
//! Walks all functions, blocks, and instructions. For each
//! `Assign { dest, value: BinOp/UnOp }` where all operands are constants,
//! computes the result at compile time and replaces the instruction with a
//! constant assignment.
//!
//! This delegates to the existing evaluator in `consteval` to avoid
//! duplicating the folding logic.

use crate::consteval;
use crate::ir::MirModule;

/// Fold all constant binary/unary operations across the entire module.
///
/// This is a thin wrapper around `consteval::run_constant_fold_pass` which
/// already implements the correct folding logic for all operator / constant
/// type combinations.
pub fn fold_constants(module: &mut MirModule) {
    consteval::run_constant_fold_pass(module);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    /// Helper: build a minimal MirModule with a single function containing
    /// the given instructions in one basic block.
    fn module_with_instructions(instructions: Vec<Instruction>) -> MirModule {
        MirModule {
            functions: vec![MirFunction {
                name: "test".into(),
                params: vec![],
                ret_ty: MirType::I64,
                blocks: vec![BasicBlock {
                    id: BlockId(0),
                    instructions,
                    terminator: Terminator::Return(None),
                }],
                locals: vec![MirLocal {
                    id: LocalId(0),
                    name: Some("x".into()),
                    ty: MirType::I64,
                    mutable: false,
                }],
                attributes: MirAttributes::default(),
                source_file: None,
                source_line: 0,
            }],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        }
    }

    #[test]
    fn fold_int_addition() {
        let mut module = module_with_instructions(vec![Instruction::Assign {
            dest: LocalId(0),
            value: RValue::BinOp {
                op: MirBinOp::Add,
                left: Operand::Constant(Constant::Int(3)),
                right: Operand::Constant(Constant::Int(4)),
            },
        }]);

        fold_constants(&mut module);

        match &module.functions[0].blocks[0].instructions[0] {
            Instruction::Assign { value, .. } => match value {
                RValue::ConstInt(7) => {}
                other => panic!("expected ConstInt(7), got {other:?}"),
            },
            other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn fold_float_multiplication() {
        let mut module = module_with_instructions(vec![Instruction::Assign {
            dest: LocalId(0),
            value: RValue::BinOp {
                op: MirBinOp::Mul,
                left: Operand::Constant(Constant::Float(2.5)),
                right: Operand::Constant(Constant::Float(4.0)),
            },
        }]);

        fold_constants(&mut module);

        match &module.functions[0].blocks[0].instructions[0] {
            Instruction::Assign {
                value: RValue::ConstFloat(v),
                ..
            } if (*v - 10.0).abs() < f64::EPSILON => {}
            other => panic!("expected ConstFloat(10.0), got {other:?}"),
        }
    }

    #[test]
    fn no_fold_when_operand_is_local() {
        let mut module = module_with_instructions(vec![Instruction::Assign {
            dest: LocalId(0),
            value: RValue::BinOp {
                op: MirBinOp::Add,
                left: Operand::Local(LocalId(0)),
                right: Operand::Constant(Constant::Int(1)),
            },
        }]);

        fold_constants(&mut module);

        // Should remain a BinOp — can't fold.
        match &module.functions[0].blocks[0].instructions[0] {
            Instruction::Assign {
                value: RValue::BinOp { .. },
                ..
            } => {}
            other => panic!("expected BinOp (unfoldable), got {other:?}"),
        }
    }

    #[test]
    fn fold_unary_negation() {
        let mut module = module_with_instructions(vec![Instruction::Assign {
            dest: LocalId(0),
            value: RValue::UnOp {
                op: MirUnOp::Neg,
                operand: Operand::Constant(Constant::Int(42)),
            },
        }]);

        fold_constants(&mut module);

        match &module.functions[0].blocks[0].instructions[0] {
            Instruction::Assign {
                value: RValue::ConstInt(-42),
                ..
            } => {}
            other => panic!("expected ConstInt(-42), got {other:?}"),
        }
    }

    #[test]
    fn fold_empty_function_no_panic() {
        let mut module = MirModule {
            functions: vec![MirFunction {
                name: "empty".into(),
                params: vec![],
                ret_ty: MirType::Void,
                blocks: vec![BasicBlock {
                    id: BlockId(0),
                    instructions: vec![],
                    terminator: Terminator::Return(None),
                }],
                locals: vec![],
                attributes: MirAttributes::default(),
                source_file: None,
                source_line: 0,
            }],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        };

        // Should not panic on empty instructions.
        fold_constants(&mut module);
    }
}
