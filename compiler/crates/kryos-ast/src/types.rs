use kryos_errors::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Simple {
        name: String,
        span: Span,
    },
    Generic {
        name: String,
        args: Vec<TypeExpr>,
        span: Span,
    },
    Array {
        element: Box<TypeExpr>,
        size: Option<u64>,
        span: Span,
    },
    Tuple {
        elements: Vec<TypeExpr>,
        span: Span,
    },
    Function {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
        span: Span,
    },
    Optional {
        inner: Box<TypeExpr>,
        span: Span,
    },
    Reference {
        inner: Box<TypeExpr>,
        mutable: bool,
        span: Span,
    },
    Shared {
        inner: Box<TypeExpr>,
        span: Span,
    },
    Weak {
        inner: Box<TypeExpr>,
        span: Span,
    },
    Pointer {
        inner: Box<TypeExpr>,
        mutable: bool,
        span: Span,
    },
    /// `dyn TraitName` — dynamic dispatch trait object.
    DynTrait {
        trait_name: String,
        span: Span,
    },
    Inferred {
        span: Span,
    },
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            Self::Simple { span, .. }
            | Self::Generic { span, .. }
            | Self::Array { span, .. }
            | Self::Tuple { span, .. }
            | Self::Function { span, .. }
            | Self::Optional { span, .. }
            | Self::Reference { span, .. }
            | Self::Shared { span, .. }
            | Self::Weak { span, .. }
            | Self::Pointer { span, .. }
            | Self::DynTrait { span, .. }
            | Self::Inferred { span } => *span,
        }
    }
}
