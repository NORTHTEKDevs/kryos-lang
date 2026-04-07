use kryos_errors::Span;
use crate::types::TypeExpr;
use crate::expr::{Expr, Pattern};

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectBranch {
    pub pattern: String,
    pub channel: Expr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        mutable: bool,
        ty: Option<TypeExpr>,
        value: Option<Expr>,
        pattern: Option<Pattern>,
        span: Span,
    },
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
        span: Span,
    },
    Return { value: Option<Expr>, span: Span },
    If {
        condition: Expr,
        then_block: Block,
        elif_clauses: Vec<(Expr, Block)>,
        else_block: Option<Block>,
        span: Span,
    },
    For {
        parallel: bool,
        pattern: Pattern,
        iterable: Expr,
        body: Block,
        span: Span,
    },
    While {
        condition: Expr,
        body: Block,
        span: Span,
    },
    Break { span: Span },
    Continue { span: Span },
    Expr { expr: Expr, span: Span },
    Spawn { expr: Expr, span: Span },
    Select { branches: Vec<SelectBranch>, span: Span },
    TryCatch {
        try_block: Block,
        catch_name: String,
        catch_block: Block,
        span: Span,
    },
    Throw { expr: Expr, span: Span },
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Self::Let { span, .. } | Self::Assign { span, .. } |
            Self::Return { span, .. } | Self::If { span, .. } |
            Self::For { span, .. } | Self::While { span, .. } |
            Self::Break { span } | Self::Continue { span } |
            Self::Expr { span, .. } | Self::Spawn { span, .. } |
            Self::Select { span, .. } | Self::TryCatch { span, .. } |
            Self::Throw { span, .. } => *span,
        }
    }
}
