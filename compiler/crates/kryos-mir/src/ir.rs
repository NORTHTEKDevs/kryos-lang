//! MIR data structures — instructions, basic blocks, control flow graph.
//!
//! The MIR is a low-level representation that sits between the AST and codegen
//! backends (Cranelift / LLVM). It uses a control-flow graph of basic blocks
//! where each block contains a list of instructions and a single terminator.

use std::collections::{HashMap, HashSet};
use std::fmt;

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

/// Index into `MirFunction::locals`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalId(pub u32);

/// Index into `MirFunction::blocks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    Ref {
        inner: Box<MirType>,
        mutable: bool,
    },
    Shared(Box<MirType>),
    Array(Box<MirType>, Option<u64>),
    Tuple(Vec<MirType>),
    Struct(String),
    Enum(String),
    Function {
        params: Vec<MirType>,
        ret: Box<MirType>,
    },
    /// Dynamic trait object: fat pointer (data_ptr, vtable_ptr).
    DynTrait(String),
    /// Heap-allocated map: runtime handle stored as i64.
    Map {
        key: Box<MirType>,
        value: Box<MirType>,
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
            MirType::Ref {
                inner,
                mutable: true,
            } => write!(f, "&mut {inner}"),
            MirType::Ref {
                inner,
                mutable: false,
            } => write!(f, "&{inner}"),
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
            MirType::DynTrait(name) => write!(f, "dyn {name}"),
            MirType::Map { key, value } => write!(f, "Map<{key}, {value}>"),
        }
    }
}

// ---------------------------------------------------------------------------
// Module / Function / Local
// ---------------------------------------------------------------------------

/// Module-level metadata needed by codegen before per-function emission.
/// Split from `MirModule` to support incremental per-function codegen.
#[derive(Debug, Clone)]
pub struct MirModuleHeader {
    pub struct_defs: HashMap<String, Vec<(String, MirType)>>,
    pub enum_defs: HashMap<String, Vec<EnumVariantDef>>,
    pub trait_vtables: HashMap<(String, String), Vec<String>>,
    pub copy_structs: HashSet<String>,
}

/// Top-level MIR module — a collection of functions.
#[derive(Debug, Clone)]
pub struct MirModule {
    pub functions: Vec<MirFunction>,
    /// Struct definitions: struct name -> ordered list of (field_name, field_type).
    /// Used by codegen to compute memory layouts for struct allocation and field access.
    pub struct_defs: HashMap<String, Vec<(String, MirType)>>,
    /// Enum definitions: enum name -> ordered list of variants with their field types.
    pub enum_defs: HashMap<String, Vec<EnumVariantDef>>,
    /// Trait vtable map: (concrete_type, trait_name) -> ordered list of mangled method names.
    /// Used by codegen to build vtables for dynamic dispatch.
    pub trait_vtables: HashMap<(String, String), Vec<String>>,
    /// Structs annotated with `@copy` — assignment copies the value instead of
    /// moving the pointer.  Used by codegen to emit deep copies.
    pub copy_structs: HashSet<String>,
}

impl MirModule {
    /// Split the module into header metadata and the function list.
    /// Allows codegen to prescan all functions for cross-function metadata, then
    /// drain and emit functions one at a time to reduce peak memory.
    pub fn into_header_and_functions(self) -> (MirModuleHeader, Vec<MirFunction>) {
        let header = MirModuleHeader {
            struct_defs: self.struct_defs,
            enum_defs: self.enum_defs,
            trait_vtables: self.trait_vtables,
            copy_structs: self.copy_structs,
        };
        (header, self.functions)
    }
}

/// Metadata attributes preserved from source annotations.
#[derive(Debug, Clone, Default)]
pub struct MirAttributes {
    /// Function is marked `@inline` — hint for the inliner pass.
    pub inline: bool,
    /// Function is marked `@pure` — no side effects.
    pub pure_fn: bool,
    /// Function is marked `@test` — discoverable by the test runner.
    pub test: bool,
    /// Function is marked `@bench` — discoverable by the benchmark runner.
    pub bench: bool,
    /// Function is marked `@deprecated`.
    pub deprecated: bool,
    /// Function is declared `async fn` — eligible for state-machine
    /// lowering by `kryos_mir::async_lower`.
    pub is_async: bool,
    /// NO LONGER SET by lower.rs (superseded, LEDGER item 7 / CLAUDE.md
    /// gotcha #11): used to hold the 0-based capture index for a lambda that
    /// mutated EXACTLY ONE captured scalar whose body's return value was
    /// that same local, so the env-thunk could smuggle the new value back by
    /// writing the call's return value into the env slot. That mechanism
    /// silently lost persistence for every OTHER shape (two+ mutated
    /// scalars, a mutated scalar alongside a mutated struct, or a solitary
    /// mutated scalar whose tail expression wasn't literally that
    /// identifier). Replaced by boxing every mutated scalar capture behind
    /// an addressable heap cell and passing it BY POINTER, the same
    /// treatment `mutated_capture_ptr_slots` already gives struct captures
    /// (see the mutated-scalar-capture block in lower.rs's Lambda arm). Kept
    /// as a field (always `None` now) and the matching codegen plumbing kept
    /// as dead-but-harmless rather than ripped out, to keep this fix's blast
    /// radius small.
    pub mutated_capture_slot: Option<u32>,
    /// Set on a synthesized lambda function that mutates EXACTLY ONE of its
    /// captured (by-move) parameters AND that capture is a Struct/Enum
    /// (heap-aggregate) type. The value is the 0-based index of that
    /// capture among the lambda's captures/params (same indexing as
    /// `mutated_capture_slot`). Unlike a scalar mutated capture -- whose new
    /// value must be explicitly written BACK into the env slot after the
    /// call returns, because the env slot holds the value directly -- an
    /// aggregate capture's env slot already holds a POINTER to an
    /// independent (deep-copied at closure-construction time) heap block;
    /// mutating a field of it just needs that SAME pointer to reach the
    /// lambda body without an intervening copy. Both codegen backends use
    /// this to pass the capture to the underlying function BY POINTER
    /// (skipping the normal byval/copy-by-value convention every other
    /// aggregate parameter gets) so a field write inside the closure body
    /// lands in the persistent env block instead of a private copy that's
    /// discarded when the call returns. Holds the env-slot index of EVERY
    /// mutated Struct-typed capture (each is passed by pointer independently),
    /// so a closure mutating two or more struct captures persists all of them
    /// -- with a single slot, a second co-occurring mutated capture reverted
    /// every struct capture to the byval copy-on-entry path and silently
    /// dropped their per-call persistence on AOT. Empty for ordinary
    /// functions. Scalar mutated captures no longer use a MirAttributes slot
    /// at all -- they get the equivalent pointer-box treatment directly in
    /// lower.rs (a per-capture `MirType::Shared` param + prologue Deref +
    /// pre-Return `StoreDeref`), so this vec only ever holds Struct-typed
    /// capture indices.
    pub mutated_capture_ptr_slots: Vec<u32>,
    /// Set (in lower.rs's Lambda arm) whenever this lambda has ANY mutated
    /// capture (scalar or struct -- same condition that populates
    /// `mutating_closures`/`mutated_capture_ptr_slots`). LEDGER item 7b: a
    /// closure VALUE shared across `spawn`-ed threads is a single retained
    /// env allocation (`kryos_arc_retain`, not a snapshot -- see docs/09-
    /// concurrency.md), so two threads calling the SAME closure concurrently
    /// race on its persisted-capture load-mutate-store with no lock. Both
    /// codegen backends read this flag to (a) reserve one extra i64 "lock
    /// word" slot at the END of the closure's env allocation (offset
    /// `(1 + captures.len()) * 8`, seeded 0 -- same allocation, same ARC
    /// lifetime as the env itself, so this adds no new allocation and no new
    /// leak) and (b) wrap the ENTIRE underlying-function call inside the
    /// generated `{name}_env` thunk -- the ONE call path every invocation of
    /// a mutating closure value goes through, since `closure_locals`'s
    /// direct-call fast path is unconditionally disabled for exactly this
    /// closure set -- with `kryos_mutex_lock`/`kryos_mutex_unlock` on that
    /// word, serializing concurrent calls to the SAME closure value. A plain
    /// blocking lock (not a CAS retry) is deliberate: each caller executes
    /// the body exactly once, so no side effect in the closure body can be
    /// duplicated. False for ordinary (non-mutating) closures and all
    /// non-closure functions -- adds no overhead there.
    pub needs_capture_lock: bool,
    /// (ptr_local, value_local) pairs for mutated-SCALAR captures needing a
    /// `StoreDeref` write-back before EVERY exit from this function, not
    /// just the MIR `Terminator::Return` blocks lower.rs's epilogue loop
    /// inserts them into directly (LEDGER item 21). A `throw` mid-body never
    /// reaches one of those blocks -- Kryos exceptions are a thread-local
    /// flag, not native unwinding, so a throwing call is followed by an
    /// early-return path synthesized entirely in CODEGEN (both backends,
    /// right after the post-call `kryos_exception_check`), which the MIR
    /// lowering pass never sees and so cannot insert a `StoreDeref` into.
    /// Both codegen backends replay these exact pairs (re-emitting the same
    /// `StoreDeref` store the normal-return epilogue uses) at their own
    /// exception early-return synthesis site so a mutation made before the
    /// throw persists into the closure's box instead of silently reverting
    /// on the NEXT call. Empty for every function that isn't a closure with
    /// a mutated scalar capture.
    pub mutated_scalar_writeback_pairs: Vec<(u32, u32)>,
    /// Local ids this function's OWN lowering (`ctx.borrowed_locals` in
    /// lower.rs) knows are NOT independently owned -- e.g. a `let` bound
    /// directly from an array/map index read (`let f = forms[i]`), a
    /// match-destructure of a borrowed scrutinee, or a loop variable -- and
    /// so must never be freed by this function. `emit_named_scope_drops`/
    /// `drop_loop_exit_locals` already exclude these (alongside
    /// `param_locals`) from every MIR-emitted `Instruction::Drop`. This field
    /// exists so codegen's OWN exception-triggered early-return cleanup
    /// (`emit_exception_cleanup_drops` in kryos-codegen-cranelift, LEDGER
    /// item 44's exception-path class) -- synthesized entirely in codegen,
    /// same reason `mutated_scalar_writeback_pairs` above exists -- can apply
    /// the identical exclusion instead of blanket-dropping every named
    /// non-parameter heap local. Without it, a shared/borrowed local still
    /// live at a throw point was freed a second time when its REAL owner
    /// (further up the call stack) later dropped it normally, one
    /// "kryos_free: double free ... ignored" per unwound stack frame.
    pub non_owned_locals: Vec<u32>,
}

/// A single MIR function.
#[derive(Debug, Clone)]
pub struct MirFunction {
    pub name: String,
    pub params: Vec<MirParam>,
    pub ret_ty: MirType,
    pub blocks: Vec<BasicBlock>,
    pub locals: Vec<MirLocal>,
    /// Source-level attributes preserved for optimization and tooling passes.
    pub attributes: MirAttributes,
    /// Source file path (populated by the driver after lowering). Used for
    /// runtime trace frames and panic messages.
    pub source_file: Option<String>,
    /// 1-based line number where this function was declared (0 if unknown).
    pub source_line: u32,
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

    /// Store a value into a struct field.
    StoreField {
        object: Operand,
        field: String,
        value: Operand,
    },

    /// Store through a pointer/reference.
    StoreDeref { ptr: Operand, value: Operand },

    /// No-op placeholder.
    Nop,

    /// Source-line marker for the source-level debugger. Emitted only when
    /// debug-line instrumentation is active (the `kryos dap` path); normal
    /// builds never produce this. The Cranelift backend lowers it to a call to
    /// the `kryos_dbg_line(line)` runtime hook; all other consumers treat it as
    /// a no-op. `line` is a 1-based source line number.
    DebugLine(u32),

    /// Spawn a function on a new OS thread.
    ///
    /// `func` is the name of the function to call (may be a generated
    /// `__spawn_N` wrapper for block-style spawns).
    /// `args` are the operands to pass as i64 arguments.
    Spawn { func: String, args: Vec<Operand> },

    /// Send a value on a channel.
    Send { channel: LocalId, value: LocalId },

    /// Receive a value from a channel.
    Receive { dest: LocalId, channel: LocalId },

    /// Spawn an actor: dest = kryos_actor_spawn_i64(dispatch_fn_ptr, state).
    /// `dispatch_fn` is the name of the generated dispatch function.
    ActorSpawn {
        dest: LocalId,
        dispatch_fn: String,
        state: Operand,
    },

    /// Send a tagged message to an actor.
    /// Generates: lock(actor) → send(actor, tag) → send(actor, arg[0]) → ... → unlock(actor).
    ActorSend {
        actor: LocalId,
        handler_tag: u32,
        args: Vec<Operand>,
    },

    /// Load a field from actor state: dest = *(state_ptr + field_offset * 8).
    ActorStateLoad {
        dest: LocalId,
        state_ptr: LocalId,
        field_offset: u32,
    },

    /// Store a value to an actor state field: *(state_ptr + field_offset * 8) = value.
    ActorStateStore {
        state_ptr: LocalId,
        field_offset: u32,
        value: Operand,
    },
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

    /// Direct function call (callee known at compile time).
    Call {
        func: String,
        args: Vec<Operand>,
    },

    /// Indirect function call (callee is a runtime value — function pointer).
    CallIndirect {
        callee: Operand,
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
    ArcAlloc {
        inner: Operand,
    },

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
        enum_name: String,
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

    /// Take the address of a local (borrow: &x or &mut x).
    AddrOf {
        local: LocalId,
        mutable: bool,
    },

    /// Load from a reference/pointer (dereference: *x).
    Deref {
        operand: Operand,
    },

    /// Comptime-evaluated expression (result is constant after eval).
    Comptime(Box<RValue>),

    /// Create a trait object (fat pointer): packs data + vtable pointer.
    /// `concrete_type` is the struct name, `trait_name` identifies which vtable.
    MakeTraitObject {
        value: Operand,
        concrete_type: String,
        trait_name: String,
    },

    /// Call a method through a vtable (dynamic dispatch).
    /// `object` is the fat pointer (dyn Trait), `method_index` is the slot in the vtable.
    VtableCall {
        object: Operand,
        method_index: u32,
        args: Vec<Operand>,
        return_ty: MirType,
    },
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
