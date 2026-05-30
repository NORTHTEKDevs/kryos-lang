//! Strength reduction pass.
//!
//! Replaces expensive arithmetic operations with cheaper equivalents:
//!
//! - `x * 2^n`  ->  `x << n`
//! - `x * 0`    ->  `0`
//! - `x * 1`    ->  `x`
//! - `x + 0`    ->  `x`
//! - `x - 0`    ->  `x`
//! - `0 + x`    ->  `x`
//! - `0 * x`    ->  `0`
//! - `1 * x`    ->  `x`

use crate::ir::{Constant, Instruction, MirBinOp, MirModule, Operand, RValue};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run strength reduction on every function in the module.
pub fn reduce_strength(module: &mut MirModule) {
    for func in &mut module.functions {
        for block in &mut func.blocks {
            for inst in &mut block.instructions {
                if let Instruction::Assign { value, .. } = inst {
                    if let Some(reduced) = try_reduce(value) {
                        *value = reduced;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reduction logic
// ---------------------------------------------------------------------------

/// Attempt to reduce an RValue to a simpler equivalent.
/// Returns `None` if no reduction applies.
fn try_reduce(rv: &RValue) -> Option<RValue> {
    match rv {
        RValue::BinOp { op, left, right } => try_reduce_binop(*op, left, right),
        _ => None,
    }
}

fn try_reduce_binop(op: MirBinOp, left: &Operand, right: &Operand) -> Option<RValue> {
    match op {
        // ----- Multiplication -----
        MirBinOp::Mul => {
            // x * 0 -> 0
            if is_int_const(right, 0) || is_int_const(left, 0) {
                return Some(RValue::ConstInt(0));
            }
            // x * 1 -> x
            if is_int_const(right, 1) {
                return Some(RValue::Use(left.clone()));
            }
            // 1 * x -> x
            if is_int_const(left, 1) {
                return Some(RValue::Use(right.clone()));
            }
            // x * 2^n -> x << n
            if let Some(n) = power_of_two_const(right) {
                return Some(RValue::BinOp {
                    op: MirBinOp::Shl,
                    left: left.clone(),
                    right: Operand::Constant(Constant::Int(n as i64)),
                });
            }
            // 2^n * x -> x << n
            if let Some(n) = power_of_two_const(left) {
                return Some(RValue::BinOp {
                    op: MirBinOp::Shl,
                    left: right.clone(),
                    right: Operand::Constant(Constant::Int(n as i64)),
                });
            }
            None
        }

        // ----- Division -----
        MirBinOp::Div => {
            // x / 1 -> x
            if is_int_const(right, 1) {
                return Some(RValue::Use(left.clone()));
            }
            // NOTE: `x / 2^n -> x >> n` is intentionally NOT done here. Kryos
            // integers are signed and division truncates toward zero, but an
            // arithmetic shift floors toward negative infinity, so the two
            // disagree for negative dividends (e.g. -7 / 2 = -3 but -7 >> 1 =
            // -4). The LLVM/Cranelift backends emit sdiv and strength-reduce it
            // correctly (with the sign bias) themselves.
            None
        }

        // ----- Modulo -----
        MirBinOp::Mod => {
            // x % 1 -> 0
            if is_int_const(right, 1) {
                return Some(RValue::ConstInt(0));
            }
            // NOTE: `x % 2^n -> x & (2^n - 1)` is intentionally NOT done here.
            // Signed remainder takes the sign of the dividend, but a bitmask is
            // always non-negative (e.g. -7 % 4 = -3 but -7 & 3 = 1). Leave it
            // as srem for the backend to handle correctly.
            None
        }

        // ----- Addition -----
        MirBinOp::Add => {
            // x + 0 -> x
            if is_int_const(right, 0) {
                return Some(RValue::Use(left.clone()));
            }
            // 0 + x -> x
            if is_int_const(left, 0) {
                return Some(RValue::Use(right.clone()));
            }
            None
        }

        // ----- Subtraction -----
        MirBinOp::Sub => {
            // x - 0 -> x
            if is_int_const(right, 0) {
                return Some(RValue::Use(left.clone()));
            }
            None
        }

        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` if the operand is `Constant::Int(expected)`.
fn is_int_const(op: &Operand, expected: i64) -> bool {
    matches!(op, Operand::Constant(Constant::Int(v)) if *v == expected)
}

/// If the operand is a `Constant::Int` that is a positive power of two (> 1),
/// returns the exponent.  E.g. 8 -> Some(3).
fn power_of_two_const(op: &Operand) -> Option<u32> {
    match op {
        Operand::Constant(Constant::Int(v)) if *v > 1 => {
            let u = *v as u64;
            if u.is_power_of_two() {
                Some(u.trailing_zeros())
            } else {
                None
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn module_with_assign(value: RValue) -> MirModule {
        MirModule {
            functions: vec![MirFunction {
                name: "test".into(),
                params: vec![],
                ret_ty: MirType::I64,
                blocks: vec![BasicBlock {
                    id: BlockId(0),
                    instructions: vec![Instruction::Assign {
                        dest: LocalId(0),
                        value,
                    }],
                    terminator: Terminator::Return(None),
                }],
                locals: vec![MirLocal {
                    id: LocalId(0),
                    name: None,
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

    fn get_value(module: &MirModule) -> &RValue {
        match &module.functions[0].blocks[0].instructions[0] {
            Instruction::Assign { value, .. } => value,
            other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn mul_by_zero_right() {
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Mul,
            left: Operand::Local(LocalId(0)),
            right: Operand::Constant(Constant::Int(0)),
        });
        reduce_strength(&mut m);
        assert!(matches!(get_value(&m), RValue::ConstInt(0)));
    }

    #[test]
    fn mul_by_zero_left() {
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Mul,
            left: Operand::Constant(Constant::Int(0)),
            right: Operand::Local(LocalId(0)),
        });
        reduce_strength(&mut m);
        assert!(matches!(get_value(&m), RValue::ConstInt(0)));
    }

    #[test]
    fn mul_by_one() {
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Mul,
            left: Operand::Local(LocalId(0)),
            right: Operand::Constant(Constant::Int(1)),
        });
        reduce_strength(&mut m);
        assert!(matches!(
            get_value(&m),
            RValue::Use(Operand::Local(LocalId(0)))
        ));
    }

    #[test]
    fn mul_by_power_of_two() {
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Mul,
            left: Operand::Local(LocalId(0)),
            right: Operand::Constant(Constant::Int(8)),
        });
        reduce_strength(&mut m);
        match get_value(&m) {
            RValue::BinOp {
                op: MirBinOp::Shl,
                right: Operand::Constant(Constant::Int(3)),
                ..
            } => {}
            other => panic!("expected x << 3, got {other:?}"),
        }
    }

    #[test]
    fn div_by_power_of_two_is_not_reduced() {
        // Signed division truncates toward zero; an arithmetic shift floors, so
        // `x / 2^n` must NOT be reduced to `x >> n` (wrong for negative x).
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Div,
            left: Operand::Local(LocalId(0)),
            right: Operand::Constant(Constant::Int(4)),
        });
        reduce_strength(&mut m);
        assert!(matches!(
            get_value(&m),
            RValue::BinOp {
                op: MirBinOp::Div,
                ..
            }
        ));
    }

    #[test]
    fn mod_by_power_of_two_is_not_reduced() {
        // Signed remainder takes the dividend's sign; a bitmask is always
        // non-negative, so `x % 2^n` must NOT be reduced to `x & (2^n - 1)`.
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Mod,
            left: Operand::Local(LocalId(0)),
            right: Operand::Constant(Constant::Int(16)),
        });
        reduce_strength(&mut m);
        assert!(matches!(
            get_value(&m),
            RValue::BinOp {
                op: MirBinOp::Mod,
                ..
            }
        ));
    }

    #[test]
    fn add_zero() {
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Add,
            left: Operand::Local(LocalId(0)),
            right: Operand::Constant(Constant::Int(0)),
        });
        reduce_strength(&mut m);
        assert!(matches!(
            get_value(&m),
            RValue::Use(Operand::Local(LocalId(0)))
        ));
    }

    #[test]
    fn sub_zero() {
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Sub,
            left: Operand::Local(LocalId(0)),
            right: Operand::Constant(Constant::Int(0)),
        });
        reduce_strength(&mut m);
        assert!(matches!(
            get_value(&m),
            RValue::Use(Operand::Local(LocalId(0)))
        ));
    }

    #[test]
    fn no_reduction_for_non_power_of_two() {
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Mul,
            left: Operand::Local(LocalId(0)),
            right: Operand::Constant(Constant::Int(7)),
        });
        reduce_strength(&mut m);
        // 7 is not a power of two — should remain a Mul.
        assert!(matches!(
            get_value(&m),
            RValue::BinOp {
                op: MirBinOp::Mul,
                ..
            }
        ));
    }

    #[test]
    fn div_by_one() {
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Div,
            left: Operand::Local(LocalId(0)),
            right: Operand::Constant(Constant::Int(1)),
        });
        reduce_strength(&mut m);
        assert!(matches!(
            get_value(&m),
            RValue::Use(Operand::Local(LocalId(0)))
        ));
    }

    #[test]
    fn mod_by_one() {
        let mut m = module_with_assign(RValue::BinOp {
            op: MirBinOp::Mod,
            left: Operand::Local(LocalId(0)),
            right: Operand::Constant(Constant::Int(1)),
        });
        reduce_strength(&mut m);
        assert!(matches!(get_value(&m), RValue::ConstInt(0)));
    }

    #[test]
    fn empty_module_no_panic() {
        let mut m = MirModule {
            functions: vec![],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        };
        reduce_strength(&mut m);
    }

    #[test]
    fn non_binop_untouched() {
        let mut m = module_with_assign(RValue::ConstInt(42));
        reduce_strength(&mut m);
        assert!(matches!(get_value(&m), RValue::ConstInt(42)));
    }
}
