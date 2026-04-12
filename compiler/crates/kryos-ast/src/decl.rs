use kryos_errors::Span;
use crate::types::TypeExpr;
use crate::expr::Param;
use crate::stmt::Block;

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub name: String,
    pub args: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericParam {
    pub name: String,
    pub bounds: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: String,
    pub ty: TypeExpr,
    pub public: bool,
    pub default: Option<crate::expr::Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageHandler {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_ty: Option<TypeExpr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportPath {
    pub segments: Vec<String>,
    pub alias: Option<String>,
    pub items: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    Function {
        name: String,
        generics: Vec<GenericParam>,
        params: Vec<Param>,
        ret_ty: Option<TypeExpr>,
        body: Option<Block>,
        public: bool,
        is_async: bool,
        annotations: Vec<Annotation>,
        doc_comments: Vec<String>,
        span: Span,
    },
    Struct {
        name: String,
        generics: Vec<GenericParam>,
        fields: Vec<StructField>,
        public: bool,
        annotations: Vec<Annotation>,
        doc_comments: Vec<String>,
        span: Span,
    },
    Enum {
        name: String,
        generics: Vec<GenericParam>,
        variants: Vec<EnumVariant>,
        public: bool,
        annotations: Vec<Annotation>,
        doc_comments: Vec<String>,
        span: Span,
    },
    Trait {
        name: String,
        generics: Vec<GenericParam>,
        methods: Vec<Decl>,
        public: bool,
        doc_comments: Vec<String>,
        span: Span,
    },
    Impl {
        target: String,
        trait_name: Option<String>,
        generics: Vec<GenericParam>,
        methods: Vec<Decl>,
        doc_comments: Vec<String>,
        span: Span,
    },
    Actor {
        name: String,
        state_fields: Vec<StructField>,
        handlers: Vec<MessageHandler>,
        annotations: Vec<Annotation>,
        doc_comments: Vec<String>,
        span: Span,
    },
    TypeAlias {
        name: String,
        generics: Vec<GenericParam>,
        ty: TypeExpr,
        public: bool,
        doc_comments: Vec<String>,
        span: Span,
    },
    Import { path: ImportPath, doc_comments: Vec<String>, span: Span },
    Extern {
        abi: String,
        items: Vec<Decl>,
        doc_comments: Vec<String>,
        span: Span,
    },
    /// Top-level constant: `let NAME = expr`
    Const {
        name: String,
        ty: Option<TypeExpr>,
        value: Box<crate::Expr>,
        public: bool,
        doc_comments: Vec<String>,
        span: Span,
    },
}

impl Decl {
    pub fn span(&self) -> Span {
        match self {
            Self::Function { span, .. } | Self::Struct { span, .. } |
            Self::Enum { span, .. } | Self::Trait { span, .. } |
            Self::Impl { span, .. } | Self::Actor { span, .. } |
            Self::TypeAlias { span, .. } | Self::Import { span, .. } |
            Self::Extern { span, .. } | Self::Const { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub name: String,
    pub declarations: Vec<Decl>,
    pub span: Span,
}
