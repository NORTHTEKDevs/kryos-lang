use kryos_errors::Span;
use crate::types::TypeExpr;
use crate::stmt::Block;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod, Pow,
    Eq, Neq, Lt, Gt, LtEq, GtEq,
    And, Or,
    BitAnd, BitOr, BitXor, Shl, Shr,
    Pipe, MatMul,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg, Not, BitNot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Option<TypeExpr>,
    pub default: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Box<Expr>>,
    pub body: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard { span: Span },
    Ident { name: String, mutable: bool, span: Span },
    Literal { expr: Box<Expr>, span: Span },
    Tuple { elements: Vec<Pattern>, span: Span },
    Struct { name: String, fields: Vec<(String, Pattern)>, span: Span },
    Enum { name: String, variant: String, fields: Vec<Pattern>, span: Span },
    Or { patterns: Vec<Pattern>, span: Span },
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Literal(String),
    Expr(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    IntLiteral { value: i64, span: Span },
    FloatLiteral { value: f64, span: Span },
    StringLiteral { value: String, span: Span },
    InterpolatedString { parts: Vec<StringPart>, span: Span },
    CharLiteral { value: char, span: Span },
    BoolLiteral { value: bool, span: Span },
    NoneLiteral { span: Span },

    Identifier { name: String, span: Span },
    FieldAccess { object: Box<Expr>, field: String, span: Span },
    IndexAccess { object: Box<Expr>, index: Box<Expr>, span: Span },

    BinaryOp { op: BinOp, left: Box<Expr>, right: Box<Expr>, span: Span },
    UnaryOp { op: UnOp, operand: Box<Expr>, span: Span },

    FnCall { callee: Box<Expr>, args: Vec<Expr>, span: Span },
    MethodCall { object: Box<Expr>, method: String, args: Vec<Expr>, span: Span },
    StaticMethodCall { type_name: String, method: String, args: Vec<Expr>, span: Span },

    ArrayLiteral { elements: Vec<Expr>, span: Span },
    TupleLiteral { elements: Vec<Expr>, span: Span },
    MapLiteral { entries: Vec<(Expr, Expr)>, span: Span },
    StructLiteral { name: String, fields: Vec<(String, Expr)>, span: Span },

    Lambda { params: Vec<Param>, ret_ty: Option<TypeExpr>, body: Box<Expr>, span: Span },

    IfExpr { condition: Box<Expr>, then_branch: Block, else_branch: Option<Block>, span: Span },
    MatchExpr { subject: Box<Expr>, arms: Vec<MatchArm>, span: Span },
    RangeExpr { start: Option<Box<Expr>>, end: Option<Box<Expr>>, inclusive: bool, span: Span },

    PipeExpr { left: Box<Expr>, right: Box<Expr>, span: Span },

    /// Borrow expression: `&x` (immutable) or `&mut x` (mutable).
    Borrow { inner: Box<Expr>, mutable: bool, span: Span },
    /// Dereference expression: `*x`.
    Deref { inner: Box<Expr>, span: Span },

    SharedExpr { inner: Box<Expr>, span: Span },
    MoveExpr { inner: Box<Expr>, span: Span },
    WeakExpr { inner: Box<Expr>, span: Span },

    ComptimeBlock { body: Block, span: Span },
    QuantumBlock { body: Block, span: Span },

    Cast { expr: Box<Expr>, ty: TypeExpr, span: Span },
    Block { block: Block, span: Span },

    /// Await expression: `await expr`
    Await { value: Box<Expr>, span: Span },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Self::IntLiteral { span, .. } | Self::FloatLiteral { span, .. } |
            Self::StringLiteral { span, .. } | Self::InterpolatedString { span, .. } |
            Self::CharLiteral { span, .. } | Self::BoolLiteral { span, .. } |
            Self::NoneLiteral { span } | Self::Identifier { span, .. } |
            Self::FieldAccess { span, .. } | Self::IndexAccess { span, .. } |
            Self::BinaryOp { span, .. } | Self::UnaryOp { span, .. } |
            Self::FnCall { span, .. } | Self::MethodCall { span, .. } |
            Self::StaticMethodCall { span, .. } |
            Self::ArrayLiteral { span, .. } | Self::TupleLiteral { span, .. } |
            Self::MapLiteral { span, .. } | Self::StructLiteral { span, .. } |
            Self::Lambda { span, .. } | Self::IfExpr { span, .. } |
            Self::MatchExpr { span, .. } | Self::RangeExpr { span, .. } |
            Self::PipeExpr { span, .. } |
            Self::Borrow { span, .. } | Self::Deref { span, .. } |
            Self::SharedExpr { span, .. } |
            Self::MoveExpr { span, .. } | Self::WeakExpr { span, .. } |
            Self::ComptimeBlock { span, .. } | Self::QuantumBlock { span, .. } |
            Self::Cast { span, .. } | Self::Block { span, .. } |
            Self::Await { span, .. } => *span,
        }
    }
}
