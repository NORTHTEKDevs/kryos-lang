//! MIR data structures — instructions, basic blocks, control flow graph.
//!
//! The MIR is a low-level representation that sits between the AST and codegen
//! backends (Cranelift / LLVM). It uses a control-flow graph of basic blocks
//! where each block contains a list of instructions and a single terminator.

use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

/// Index into `MirFunction::locals`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

/// Index into `MirFunction::blocks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

impl fmt::Display for LocalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "_{}", self.0)
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Enum definitions
// ---------------------------------------------------------------------------

/// A single variant in an enum definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariantDef {
    pub name: String,
    pub fields: Vec<MirType>,
}

// ---------------------------------------------------------------------------
// Trait definitions (for MIR-level trait tracking)
// ---------------------------------------------------------------------------

/// A single method signature within a trait definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitMethodSig {
    pub name: String,
    pub param_types: Vec<MirType>,
    pub ret_ty: MirType,
}

// ---------------------------------------------------------------------------
// Types (MIR-level, decoupled from AST TypeExpr)
// ---------------------------------------------------------------------------

/// MIR-level type representation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MirType {
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    F32,
    F64,
    Bool,
    Char,
    Str,
    Void,
    Ptr(Box<MirType>),
    Shared(Box<MirType>),
    Array(Box<MirType>, Option<u64>),
    Tuple(Vec<MirType>),
    Struct(String),
    Enum(String),
    Function {
        params: Vec<MirType>,
        ret: Box<MirType>,
    },
}

impl fmt::Display for MirType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MirType::I8 => write!(f, "i8"),
            MirType::I16 => write!(f, "i16"),
            MirType::I32 => write!(f, "i32"),
            MirType::I64 => write!(f, "i64"),
            MirType::I128 => write!(f, "i128"),
            MirType::U8 => write!(f, "u8"),
            MirType::U16 => write!(f, "u16"),
            MirType::U32 => write!(f, "u32"),
            MirType::U64 => write!(f, "u64"),
            MirType::U128 => write!(f, "u128"),
            MirType::F32 => write!(f, "f32"),
            MirType::F64 => write!(f, "f64"),
            MirType::Bool => write!(f, "bool"),
            MirType::Char => write!(f, "char"),
            MirType::Str => write!(f, "str"),
            MirType::Void => write!(f, "void"),
            MirType::Ptr(inner) => write!(f, "*{inner}"),
            MirType::Shared(inner) => write!(f, "shared {inner}"),
            MirType::Array(elem, Some(n)) => write!(f, "[{elem}; {n}]"),
            MirType::Array(elem, None) => write!(f, "[{elem}]"),
            MirType::Tuple(elems) => {
                write!(f, "(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{e}")?;
                }
                write!(f, ")")
            }
            MirType::Struct(name) => write!(f, "{name}"),
            MirType::Enum(name) => write!(f, "{name}"),
            MirType::Function { params, ret } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Module / Function / Local
// ---------------------------------------------------------------------------

/// Top-level MIR module — a collection of functions.
#[derive(Debug, Clone)]
pub struct MirModule {
    pub functions: Vec<MirFunction>,
    /// Struct definitions: struct name -> ordered list of (field_name, field_type).
    /// Used by codegen to compute memory layouts for struct allocation and field access.
    pub struct_defs: HashMap<String, Vec<(String, MirType)>>,
    /// Enum definitions: enum name -> ordered list of variants with their field types.
    pub enum_defs: HashMap<String, Vec<EnumVariantDef>>,
}

/// A single MIR function.
#[derive(Debug, Clone)]
pub struct MirFunction {
    pub name: String,
    pub params: Vec<MirParam>,
    pub ret_ty: MirType,
    pub blocks: Vec<BasicBlock>,
    pub locals: Vec<MirLocal>,
}

/// A formal parameter — refers to a local slot.
#[derive(Debug, Clone)]
pub struct MirParam {
    pub local: LocalId,
    pub ty: MirType,
}

/// A local variable (includes parameters, temporaries, user bindings).
#[derive(Debug, Clone)]
pub struct MirLocal {
    pub id: LocalId,
    pub name: Option<String>,
    pub ty: MirType,
    pub mutable: bool,
}

// ---------------------------------------------------------------------------
// Basic Block
// ---------------------------------------------------------------------------

/// A basic block: straight-line instructions followed by exactly one terminator.
#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

// ---------------------------------------------------------------------------
// Instructions
// ---------------------------------------------------------------------------

/// A single MIR instruction (non-branching).
#[derive(Debug, Clone)]
pub enum Instruction {
    /// Assign an r-value to a local.
    Assign { dest: LocalId, value: RValue },

    /// Increment ARC reference count.
    ArcRetain { ptr: LocalId },

    /// Decrement ARC reference count (may deallocate).
    ArcRelease { ptr: LocalId },

    /// Drop a local (scope-exit cleanup).
    Drop { local: LocalId },

    /// Spawn a concurrent task — evaluates expr and runs it concurrently.
    Spawn { task: LocalId },

    /// Send a value on a channel.
    Send { channel: LocalId, value: LocalId },

    /// Receive a value from a channel.
    Receive { dest: LocalId, channel: LocalId },

    /// No-op placeholder.
    Nop,
}

/// Right-hand side of an assignment.
#[derive(Debug, Clone)]
pub enum RValue {
    /// Copy/move a single operand.
    Use(Operand),

    /// Binary arithmetic / comparison / logical.
    BinOp {
        op: MirBinOp,
        left: Operand,
        right: Operand,
    },

    /// Unary operation.
    UnOp {
        op: MirUnOp,
        operand: Operand,
    },

    /// Function call.
    Call {
        func: String,
        args: Vec<Operand>,
    },

    // ---- Constants ----
    ConstInt(i64),
    ConstFloat(f64),
    ConstBool(bool),
    ConstString(String),
    ConstNone,

    // ---- Aggregates ----
    Array(Vec<Operand>),
    Tuple(Vec<Operand>),
    Struct {
        name: String,
        fields: Vec<(String, Operand)>,
    },

    // ---- Access ----
    Field {
        object: Operand,
        field: String,
    },
    Index {
        object: Operand,
        index: Operand,
    },

    /// Allocate an ARC-wrapped value.
    ArcAlloc { inner: Operand },

    /// Type cast.
    Cast {
        operand: Operand,
        ty: MirType,
    },

    // ---- Enums ----

    /// Construct an enum value with a tag and payload fields.
    EnumVariant {
        enum_name: String,
        variant_idx: u32,
        fields: Vec<Operand>,
    },

    /// Extract the tag (variant index) from an enum value.
    EnumTag {
        operand: Operand,
    },

    /// Extract a payload field from an enum value.
    EnumPayload {
        operand: Operand,
        variant_idx: u32,
        field_idx: u32,
    },

    // ---- Closures ----

    /// A closure: function pointer + captured environment variables.
    Closure {
        func_name: String,
        captures: Vec<Operand>,
    },

    /// Map literal: ordered list of key-value pairs.
    Map(Vec<(Operand, Operand)>),

    /// String concatenation (from interpolated strings).
    StringConcat(Vec<Operand>),

    /// Range expression: start..end or start..=end.
    Range {
        start: Option<Operand>,
        end: Option<Operand>,
        inclusive: bool,
    },

    /// Comptime-evaluated expression (result is constant after eval).
    Comptime(Box<RValue>),
}

/// An operand — either a local reference or an inline constant.
#[derive(Debug, Clone)]
pub enum Operand {
    Local(LocalId),
    Constant(Constant),
}

/// Inline constant values.
#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    None,
}

// ---------------------------------------------------------------------------
// Binary / Unary operators
// ---------------------------------------------------------------------------

/// MIR binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Neq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

/// MIR unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirUnOp {
    Neg,
    Not,
    BitNot,
}

// ---------------------------------------------------------------------------
// Terminators
// ---------------------------------------------------------------------------

/// Block terminator — always the last "instruction" of a basic block.
#[derive(Debug, Clone)]
pub enum Terminator {
    /// Return from the function.
    Return(Option<Operand>),

    /// Unconditional jump.
    Goto(BlockId),

    /// Conditional branch (if/else).
    Branch {
        cond: Operand,
        then_block: BlockId,
        else_block: BlockId,
    },

    /// Multi-way branch (match/switch).
    Switch {
        value: Operand,
        targets: Vec<(i64, BlockId)>,
        default: BlockId,
    },

    /// This block should never be reached.
    Unreachable,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

impl MirFunction {
    /// Returns the entry block (always block 0).
    pub fn entry_block(&self) -> &BasicBlock {
        &self.blocks[0]
    }

    /// Returns the number of basic blocks.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Finds a block by its `BlockId`.
    pub fn block(&self, id: BlockId) -> &BasicBlock {
        &self.blocks[id.0 as usize]
    }
}

impl BasicBlock {
    /// Returns true if this block's terminator is `Unreachable`.
    pub fn is_unreachable(&self) -> bool {
        matches!(self.terminator, Terminator::Unreachable)
    }

    /// Returns the successor block IDs for this block.
    pub fn successors(&self) -> Vec<BlockId> {
        match &self.terminator {
            Terminator::Return(_) | Terminator::Unreachable => vec![],
            Terminator::Goto(target) => vec![*target],
            Terminator::Branch {
                then_block,
                else_block,
                ..
            } => vec![*then_block, *else_block],
            Terminator::Switch {
                targets, default, ..
            } => {
                let mut succs: Vec<BlockId> = targets.iter().map(|(_, b)| *b).collect();
                succs.push(*default);
                succs
            }
        }
    }
}
