//! Internal type representation for the Kryos type system.
//!
//! These types are distinct from AST `TypeExpr` nodes — they represent
//! resolved, canonical types used during type checking and inference.

use std::fmt;

/// Concrete types used during type checking (distinct from AST TypeExpr).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    // Primitives
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
    USize,
    ISize,
    Void,
    Never, // ! type (diverging)

    // Compound
    Array {
        element: Box<Type>,
        size: Option<u64>,
    },
    Tuple {
        elements: Vec<Type>,
    },
    Map {
        key: Box<Type>,
        value: Box<Type>,
    },
    Set {
        element: Box<Type>,
    },
    Option {
        inner: Box<Type>,
    },
    Result {
        ok: Box<Type>,
        err: Box<Type>,
    },

    // User-defined
    Struct {
        name: String,
        generics: Vec<Type>,
    },
    Enum {
        name: String,
        generics: Vec<Type>,
    },

    // Function
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
    },

    // References and ownership
    Reference {
        inner: Box<Type>,
        mutable: bool,
    },
    Shared {
        inner: Box<Type>,
    }, // ARC-wrapped
    Weak {
        inner: Box<Type>,
    }, // Weak reference
    Pointer {
        inner: Box<Type>,
        mutable: bool,
    }, // Raw pointer (FFI)

    /// Dynamic trait object: `dyn TraitName`
    DynTrait {
        trait_name: String,
    },

    // Type variables (for inference)
    Var(u32), // Unresolved type variable

    // Error type (for error recovery — never propagates mismatches)
    Error,
}

impl Type {
    /// Returns true if this type is a numeric integer type.
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::I128
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
                | Type::U128
                | Type::USize
                | Type::ISize
        )
    }

    /// Returns true if this type is a floating-point type.
    pub fn is_float(&self) -> bool {
        matches!(self, Type::F32 | Type::F64)
    }

    /// Returns true if this type is any numeric type.
    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    /// Returns true if this type is a signed integer type.
    pub fn is_signed_integer(&self) -> bool {
        matches!(
            self,
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 | Type::ISize
        )
    }

    /// Returns true if this is the error recovery type.
    pub fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    /// Returns true if this type contains unresolved type variables.
    pub fn has_vars(&self) -> bool {
        match self {
            Type::Var(_) => true,
            Type::Array { element, .. } => element.has_vars(),
            Type::Tuple { elements } => elements.iter().any(|e| e.has_vars()),
            Type::Map { key, value } => key.has_vars() || value.has_vars(),
            Type::Set { element } => element.has_vars(),
            Type::Option { inner } => inner.has_vars(),
            Type::Result { ok, err } => ok.has_vars() || err.has_vars(),
            Type::Struct { generics, .. } | Type::Enum { generics, .. } => {
                generics.iter().any(|g| g.has_vars())
            }
            Type::Function { params, ret } => params.iter().any(|p| p.has_vars()) || ret.has_vars(),
            Type::Reference { inner, .. }
            | Type::Shared { inner }
            | Type::Weak { inner }
            | Type::Pointer { inner, .. } => inner.has_vars(),
            _ => false,
        }
    }

    /// Resolve a simple type name string to a concrete Type.
    pub fn from_name(name: &str) -> Option<Type> {
        match name {
            "i8" => Some(Type::I8),
            "i16" => Some(Type::I16),
            "i32" => Some(Type::I32),
            "i64" => Some(Type::I64),
            "i128" => Some(Type::I128),
            "u8" => Some(Type::U8),
            "u16" => Some(Type::U16),
            "u32" => Some(Type::U32),
            "u64" => Some(Type::U64),
            "u128" => Some(Type::U128),
            "f32" => Some(Type::F32),
            "f64" => Some(Type::F64),
            "bool" => Some(Type::Bool),
            "char" => Some(Type::Char),
            "str" | "string" | "String" => Some(Type::Str),
            "usize" => Some(Type::USize),
            "isize" => Some(Type::ISize),
            "void" => Some(Type::Void),
            "never" | "Never" => Some(Type::Never),
            // `ptr` — opaque mutable raw pointer used by stdlib FFI declarations.
            // Resolves to a raw pointer with void element type.
            "ptr" => Some(Type::Pointer {
                inner: Box::new(Type::Void),
                mutable: true,
            }),
            // `any` — dynamic / unchecked type used in stdlib signatures that
            // need to accept arbitrary values (e.g. Option's wrapped value,
            // generic format args). Resolves to `Type::Error` which the type
            // checker treats as an error-recovery sentinel that unifies with
            // anything without emitting a mismatch diagnostic.
            "any" | "Any" => Some(Type::Error),
            _ => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::I8 => write!(f, "i8"),
            Type::I16 => write!(f, "i16"),
            Type::I32 => write!(f, "i32"),
            Type::I64 => write!(f, "i64"),
            Type::I128 => write!(f, "i128"),
            Type::U8 => write!(f, "u8"),
            Type::U16 => write!(f, "u16"),
            Type::U32 => write!(f, "u32"),
            Type::U64 => write!(f, "u64"),
            Type::U128 => write!(f, "u128"),
            Type::F32 => write!(f, "f32"),
            Type::F64 => write!(f, "f64"),
            Type::Bool => write!(f, "bool"),
            Type::Char => write!(f, "char"),
            Type::Str => write!(f, "str"),
            Type::USize => write!(f, "usize"),
            Type::ISize => write!(f, "isize"),
            Type::Void => write!(f, "void"),
            Type::Never => write!(f, "!"),
            Type::Array {
                element,
                size: Some(n),
            } => write!(f, "[{element}; {n}]"),
            Type::Array {
                element,
                size: None,
            } => write!(f, "[{element}]"),
            Type::Tuple { elements } => {
                write!(f, "(")?;
                for (i, e) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{e}")?;
                }
                write!(f, ")")
            }
            Type::Map { key, value } => write!(f, "map<{key}, {value}>"),
            Type::Set { element } => write!(f, "set<{element}>"),
            Type::Option { inner } => write!(f, "Option<{inner}>"),
            Type::Result { ok, err } => write!(f, "Result<{ok}, {err}>"),
            Type::Struct { name, generics } | Type::Enum { name, generics } => {
                write!(f, "{name}")?;
                if !generics.is_empty() {
                    write!(f, "<")?;
                    for (i, g) in generics.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{g}")?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            Type::Function { params, ret } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
            Type::Reference {
                inner,
                mutable: true,
            } => write!(f, "&mut {inner}"),
            Type::Reference {
                inner,
                mutable: false,
            } => write!(f, "&{inner}"),
            Type::Shared { inner } => write!(f, "shared {inner}"),
            Type::Weak { inner } => write!(f, "weak {inner}"),
            Type::Pointer {
                inner,
                mutable: true,
            } => write!(f, "*mut {inner}"),
            Type::Pointer {
                inner,
                mutable: false,
            } => write!(f, "*const {inner}"),
            Type::DynTrait { trait_name } => write!(f, "dyn {trait_name}"),
            Type::Var(id) => write!(f, "?T{id}"),
            Type::Error => write!(f, "<error>"),
        }
    }
}
