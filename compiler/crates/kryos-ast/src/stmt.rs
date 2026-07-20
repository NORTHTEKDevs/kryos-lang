use crate::expr::{Expr, Pattern};
use crate::types::TypeExpr;
use kryos_errors::Span;

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

#[derive(Debug, Clone, PartialEq)]
pub struct SelectTimeout {
    pub duration_ms: Expr,
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
    ModAssign,
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
    Return {
        value: Option<Expr>,
        span: Span,
    },
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
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Expr {
        expr: Expr,
        span: Span,
    },
    Spawn {
        expr: Expr,
        span: Span,
    },
    Select {
        branches: Vec<SelectBranch>,
        timeout: Option<Box<SelectTimeout>>,
        span: Span,
    },
    TryCatch {
        try_block: Block,
        catch_name: String,
        catch_block: Block,
        span: Span,
    },
    Throw {
        expr: Expr,
        span: Span,
    },
    /// `deny!(net, ffi) { ... }` — lexically narrows the active capability set
    /// for the body, removing the denied capabilities. Compile-time only; at
    /// runtime the body executes exactly as if the wrapper were absent.
    DenyBlock {
        denied: Vec<String>,
        body: Block,
        span: Span,
    },
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Self::Let { span, .. }
            | Self::Assign { span, .. }
            | Self::Return { span, .. }
            | Self::If { span, .. }
            | Self::For { span, .. }
            | Self::While { span, .. }
            | Self::Break { span }
            | Self::Continue { span }
            | Self::Expr { span, .. }
            | Self::Spawn { span, .. }
            | Self::Select { span, .. }
            | Self::TryCatch { span, .. }
            | Self::Throw { span, .. }
            | Self::DenyBlock { span, .. } => *span,
        }
    }

    /// If this is a trailing `if ... else ...` in a value-position block,
    /// return the equivalent `IfExpr` so the block can use it as its tail
    /// VALUE. A trailing `if` parses as `Stmt::If`, and block-value logic
    /// only recognized `Stmt::Expr`, so `let x = { if c { a } else { b } }`
    /// (and a match arm `x => { if ... }`) mis-typed as `void`/printed
    /// empty. `elif` clauses fold into nested IfExprs. Returns `None` for an
    /// `if` without an `else` (a side-effecting statement whose value is
    /// `void`) or any non-`if` statement, preserving existing behavior.
    pub fn as_value_if_expr(&self) -> Option<Expr> {
        let Self::If {
            condition,
            then_block,
            elif_clauses,
            else_block: Some(final_else),
            span,
        } = self
        else {
            return None;
        };
        let mut else_branch: Option<Block> = Some(final_else.clone());
        for (econd, eblock) in elif_clauses.iter().rev() {
            let nested = Expr::IfExpr {
                condition: Box::new(econd.clone()),
                then_branch: eblock.clone(),
                else_branch: else_branch.take(),
                span: *span,
            };
            else_branch = Some(Block {
                stmts: vec![Stmt::Expr {
                    expr: nested,
                    span: *span,
                }],
                span: *span,
            });
        }
        Some(Expr::IfExpr {
            condition: Box::new(condition.clone()),
            then_branch: then_block.clone(),
            else_branch,
            span: *span,
        })
    }
}
