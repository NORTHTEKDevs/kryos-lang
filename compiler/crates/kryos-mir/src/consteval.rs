//! Compile-time constant evaluator.
//!
//! Walks MIR and evaluates `RValue::Comptime` nodes whose operands are
//! all constants, replacing them with literal constant values.  When an
//! inner expression cannot be fully evaluated (e.g. it references a
//! local variable), the `Comptime` wrapper is stripped and the inner
//! expression is kept as-is.

use crate::ir::{
    BasicBlock, Constant, Instruction, MirBinOp, MirFunction, MirModule,
    MirUnOp, Operand, RValue,
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the comptime evaluation pass over an entire MIR module.
///
/// For every `Assign` instruction whose value is `RValue::Comptime(inner)`,
/// the pass attempts to fold `inner` into a constant.  If folding succeeds
/// the instruction's value becomes a plain `ConstInt` / `ConstFloat` / etc.
/// If the inner expression is not fully constant, the `Comptime` wrapper is
/// removed and the inner expression is kept verbatim.
pub fn run_comptime_pass(module: &mut MirModule) {
    for func in &mut module.functions {
        run_comptime_function(func);
    }
}

// ---------------------------------------------------------------------------
// Per-function / per-block traversal
// ---------------------------------------------------------------------------

fn run_comptime_function(func: &mut MirFunction) {
    for block in &mut func.blocks {
        run_comptime_block(block);
    }
}

fn run_comptime_block(block: &mut BasicBlock) {
    for instr in &mut block.instructions {
        if let Instruction::Assign { value, .. } = instr {
            if let RValue::Comptime(inner) = value {
                *value = eval_comptime(inner);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluator
// ---------------------------------------------------------------------------

/// Evaluate a comptime `RValue`.  If all operands are constants, returns a
/// constant `RValue`.  Otherwise returns the inner expression unwrapped
/// (i.e. the `Comptime` wrapper is stripped).
fn eval_comptime(rvalue: &RValue) -> RValue {
    match rvalue {
        // Already-constant leaves — return as-is.
        RValue::ConstInt(v) => RValue::ConstInt(*v),
        RValue::ConstFloat(v) => RValue::ConstFloat(*v),
        RValue::ConstBool(v) => RValue::ConstBool(*v),
        RValue::ConstString(s) => RValue::ConstString(s.clone()),
        RValue::ConstNone => RValue::ConstNone,

        // Nested comptime — recurse.
        RValue::Comptime(inner) => eval_comptime(inner),

        // Binary operation — try to fold if both operands are constant.
        RValue::BinOp { op, left, right } => {
            match (operand_to_const(left), operand_to_const(right)) {
                (Some(l), Some(r)) => eval_binop(*op, &l, &r),
                _ => rvalue.clone(), // can't fold — keep as-is
            }
        }

        // Unary operation — try to fold if operand is constant.
        RValue::UnOp { op, operand } => match operand_to_const(operand) {
            Some(c) => eval_unop(*op, &c),
            None => rvalue.clone(),
        },

        // Anything else (Call, Use(Local), aggregates, etc.) — can't evaluate
        // at compile time.  Return the inner expression unwrapped.
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a `Constant` from an `Operand`, if it is one.
fn operand_to_const(op: &Operand) -> Option<Constant> {
    match op {
        Operand::Constant(c) => Some(c.clone()),
        Operand::Local(_) => None,
    }
}

/// Convert a `Constant` back into an `Operand` (for unfoldable fallbacks).
fn const_to_operand(c: &Constant) -> Operand {
    Operand::Constant(c.clone())
}

// ---------------------------------------------------------------------------
// Binary operator evaluation
// ---------------------------------------------------------------------------

fn eval_binop(op: MirBinOp, left: &Constant, right: &Constant) -> RValue {
    match (op, left, right) {
        // ----- Integer arithmetic -----
        (MirBinOp::Add, Constant::Int(a), Constant::Int(b)) => {
            RValue::ConstInt(a.wrapping_add(*b))
        }
        (MirBinOp::Sub, Constant::Int(a), Constant::Int(b)) => {
            RValue::ConstInt(a.wrapping_sub(*b))
        }
        (MirBinOp::Mul, Constant::Int(a), Constant::Int(b)) => {
            RValue::ConstInt(a.wrapping_mul(*b))
        }
        (MirBinOp::Div, Constant::Int(a), Constant::Int(b)) if *b != 0 => {
            RValue::ConstInt(a / b)
        }
        (MirBinOp::Mod, Constant::Int(a), Constant::Int(b)) if *b != 0 => {
            RValue::ConstInt(a % b)
        }
        (MirBinOp::Pow, Constant::Int(a), Constant::Int(b)) if *b >= 0 => {
            RValue::ConstInt(a.wrapping_pow(*b as u32))
        }

        // ----- Float arithmetic -----
        (MirBinOp::Add, Constant::Float(a), Constant::Float(b)) => RValue::ConstFloat(a + b),
        (MirBinOp::Sub, Constant::Float(a), Constant::Float(b)) => RValue::ConstFloat(a - b),
        (MirBinOp::Mul, Constant::Float(a), Constant::Float(b)) => RValue::ConstFloat(a * b),
        (MirBinOp::Div, Constant::Float(a), Constant::Float(b)) if *b != 0.0 => {
            RValue::ConstFloat(a / b)
        }

        // ----- Integer comparisons -----
        (MirBinOp::Eq, Constant::Int(a), Constant::Int(b)) => RValue::ConstBool(a == b),
        (MirBinOp::Neq, Constant::Int(a), Constant::Int(b)) => RValue::ConstBool(a != b),
        (MirBinOp::Lt, Constant::Int(a), Constant::Int(b)) => RValue::ConstBool(a < b),
        (MirBinOp::LtEq, Constant::Int(a), Constant::Int(b)) => RValue::ConstBool(a <= b),
        (MirBinOp::Gt, Constant::Int(a), Constant::Int(b)) => RValue::ConstBool(a > b),
        (MirBinOp::GtEq, Constant::Int(a), Constant::Int(b)) => RValue::ConstBool(a >= b),

        // ----- Float comparisons -----
        (MirBinOp::Eq, Constant::Float(a), Constant::Float(b)) => RValue::ConstBool(a == b),
        (MirBinOp::Neq, Constant::Float(a), Constant::Float(b)) => RValue::ConstBool(a != b),
        (MirBinOp::Lt, Constant::Float(a), Constant::Float(b)) => RValue::ConstBool(a < b),
        (MirBinOp::LtEq, Constant::Float(a), Constant::Float(b)) => RValue::ConstBool(a <= b),
        (MirBinOp::Gt, Constant::Float(a), Constant::Float(b)) => RValue::ConstBool(a > b),
        (MirBinOp::GtEq, Constant::Float(a), Constant::Float(b)) => RValue::ConstBool(a >= b),

        // ----- Boolean logic -----
        (MirBinOp::And, Constant::Bool(a), Constant::Bool(b)) => RValue::ConstBool(*a && *b),
        (MirBinOp::Or, Constant::Bool(a), Constant::Bool(b)) => RValue::ConstBool(*a || *b),

        // ----- Bitwise integer ops -----
        (MirBinOp::BitAnd, Constant::Int(a), Constant::Int(b)) => RValue::ConstInt(a & b),
        (MirBinOp::BitOr, Constant::Int(a), Constant::Int(b)) => RValue::ConstInt(a | b),
        (MirBinOp::BitXor, Constant::Int(a), Constant::Int(b)) => RValue::ConstInt(a ^ b),
        (MirBinOp::Shl, Constant::Int(a), Constant::Int(b)) => {
            RValue::ConstInt(a.wrapping_shl(*b as u32))
        }
        (MirBinOp::Shr, Constant::Int(a), Constant::Int(b)) => {
            RValue::ConstInt(a.wrapping_shr(*b as u32))
        }

        // ----- String concatenation -----
        (MirBinOp::Add, Constant::Str(a), Constant::Str(b)) => {
            RValue::ConstString(format!("{a}{b}"))
        }

        // ----- Can't fold (division by zero, mixed types, etc.) -----
        _ => RValue::BinOp {
            op,
            left: const_to_operand(left),
            right: const_to_operand(right),
        },
    }
}

// ---------------------------------------------------------------------------
// Unary operator evaluation
// ---------------------------------------------------------------------------

fn eval_unop(op: MirUnOp, operand: &Constant) -> RValue {
    match (op, operand) {
        (MirUnOp::Neg, Constant::Int(v)) => RValue::ConstInt(v.wrapping_neg()),
        (MirUnOp::Neg, Constant::Float(v)) => RValue::ConstFloat(-v),
        (MirUnOp::Not, Constant::Bool(v)) => RValue::ConstBool(!v),
        (MirUnOp::BitNot, Constant::Int(v)) => RValue::ConstInt(!v),
        _ => RValue::UnOp {
            op,
            operand: const_to_operand(operand),
        },
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: wraps an rvalue in Comptime and evaluates it.
    fn fold(rv: RValue) -> RValue {
        eval_comptime(&rv)
    }

    #[test]
    fn fold_const_int_passthrough() {
        match fold(RValue::ConstInt(42)) {
            RValue::ConstInt(42) => {}
            other => panic!("expected ConstInt(42), got {other:?}"),
        }
    }

    #[test]
    fn fold_int_add() {
        let rv = RValue::BinOp {
            op: MirBinOp::Add,
            left: Operand::Constant(Constant::Int(2)),
            right: Operand::Constant(Constant::Int(3)),
        };
        match fold(rv) {
            RValue::ConstInt(5) => {}
            other => panic!("expected ConstInt(5), got {other:?}"),
        }
    }

    #[test]
    fn fold_int_sub() {
        let rv = RValue::BinOp {
            op: MirBinOp::Sub,
            left: Operand::Constant(Constant::Int(10)),
            right: Operand::Constant(Constant::Int(4)),
        };
        match fold(rv) {
            RValue::ConstInt(6) => {}
            other => panic!("expected ConstInt(6), got {other:?}"),
        }
    }

    #[test]
    fn fold_int_mul() {
        let rv = RValue::BinOp {
            op: MirBinOp::Mul,
            left: Operand::Constant(Constant::Int(7)),
            right: Operand::Constant(Constant::Int(6)),
        };
        match fold(rv) {
            RValue::ConstInt(42) => {}
            other => panic!("expected ConstInt(42), got {other:?}"),
        }
    }

    #[test]
    fn fold_int_div() {
        let rv = RValue::BinOp {
            op: MirBinOp::Div,
            left: Operand::Constant(Constant::Int(15)),
            right: Operand::Constant(Constant::Int(3)),
        };
        match fold(rv) {
            RValue::ConstInt(5) => {}
            other => panic!("expected ConstInt(5), got {other:?}"),
        }
    }

    #[test]
    fn fold_int_div_by_zero_is_unfoldable() {
        let rv = RValue::BinOp {
            op: MirBinOp::Div,
            left: Operand::Constant(Constant::Int(10)),
            right: Operand::Constant(Constant::Int(0)),
        };
        // Division by zero can't be folded — should return a BinOp.
        match fold(rv) {
            RValue::BinOp { .. } => {}
            other => panic!("expected BinOp (unfoldable), got {other:?}"),
        }
    }

    #[test]
    fn fold_int_pow() {
        let rv = RValue::BinOp {
            op: MirBinOp::Pow,
            left: Operand::Constant(Constant::Int(2)),
            right: Operand::Constant(Constant::Int(10)),
        };
        match fold(rv) {
            RValue::ConstInt(1024) => {}
            other => panic!("expected ConstInt(1024), got {other:?}"),
        }
    }

    #[test]
    fn fold_float_add() {
        let rv = RValue::BinOp {
            op: MirBinOp::Add,
            left: Operand::Constant(Constant::Float(1.5)),
            right: Operand::Constant(Constant::Float(2.5)),
        };
        match fold(rv) {
            RValue::ConstFloat(v) if (v - 4.0).abs() < f64::EPSILON => {}
            other => panic!("expected ConstFloat(4.0), got {other:?}"),
        }
    }

    #[test]
    fn fold_int_comparison_lt() {
        let rv = RValue::BinOp {
            op: MirBinOp::Lt,
            left: Operand::Constant(Constant::Int(3)),
            right: Operand::Constant(Constant::Int(5)),
        };
        match fold(rv) {
            RValue::ConstBool(true) => {}
            other => panic!("expected ConstBool(true), got {other:?}"),
        }
    }

    #[test]
    fn fold_bool_and() {
        let rv = RValue::BinOp {
            op: MirBinOp::And,
            left: Operand::Constant(Constant::Bool(true)),
            right: Operand::Constant(Constant::Bool(false)),
        };
        match fold(rv) {
            RValue::ConstBool(false) => {}
            other => panic!("expected ConstBool(false), got {other:?}"),
        }
    }

    #[test]
    fn fold_bitwise_xor() {
        let rv = RValue::BinOp {
            op: MirBinOp::BitXor,
            left: Operand::Constant(Constant::Int(0xFF)),
            right: Operand::Constant(Constant::Int(0x0F)),
        };
        match fold(rv) {
            RValue::ConstInt(0xF0) => {}
            other => panic!("expected ConstInt(0xF0), got {other:?}"),
        }
    }

    #[test]
    fn fold_unop_neg_int() {
        let rv = RValue::UnOp {
            op: MirUnOp::Neg,
            operand: Operand::Constant(Constant::Int(42)),
        };
        match fold(rv) {
            RValue::ConstInt(-42) => {}
            other => panic!("expected ConstInt(-42), got {other:?}"),
        }
    }

    #[test]
    fn fold_unop_not_bool() {
        let rv = RValue::UnOp {
            op: MirUnOp::Not,
            operand: Operand::Constant(Constant::Bool(true)),
        };
        match fold(rv) {
            RValue::ConstBool(false) => {}
            other => panic!("expected ConstBool(false), got {other:?}"),
        }
    }

    #[test]
    fn fold_unop_bitnot() {
        let rv = RValue::UnOp {
            op: MirUnOp::BitNot,
            operand: Operand::Constant(Constant::Int(0)),
        };
        match fold(rv) {
            RValue::ConstInt(-1) => {}
            other => panic!("expected ConstInt(-1), got {other:?}"),
        }
    }

    #[test]
    fn fold_non_const_operand_returns_as_is() {
        let rv = RValue::BinOp {
            op: MirBinOp::Add,
            left: Operand::Local(crate::ir::LocalId(0)),
            right: Operand::Constant(Constant::Int(1)),
        };
        // Can't fold because left is a local.
        match fold(rv) {
            RValue::BinOp { .. } => {}
            other => panic!("expected BinOp (unfoldable), got {other:?}"),
        }
    }

    #[test]
    fn fold_nested_comptime() {
        let inner = RValue::Comptime(Box::new(RValue::ConstInt(99)));
        match fold(inner) {
            RValue::ConstInt(99) => {}
            other => panic!("expected ConstInt(99), got {other:?}"),
        }
    }

    #[test]
    fn fold_string_concat() {
        let rv = RValue::BinOp {
            op: MirBinOp::Add,
            left: Operand::Constant(Constant::Str("hello ".into())),
            right: Operand::Constant(Constant::Str("world".into())),
        };
        match fold(rv) {
            RValue::ConstString(s) if s == "hello world" => {}
            other => panic!("expected ConstString(\"hello world\"), got {other:?}"),
        }
    }
}
