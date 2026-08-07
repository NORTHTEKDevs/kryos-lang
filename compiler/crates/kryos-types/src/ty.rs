//! Internal type representation for the Kryos type system.
//!
//! These types are distinct from AST `TypeExpr` nodes — they represent
//! resolved, canonical types used during type checking and inference.

use std::fmt;

/// A capability bitset: one bit per `kryos_capabilities::model::Capability`
/// variant (15 total, see `model.rs:13-45`). `Type` derives `Eq, Hash` (this
/// file), and `kryos_capabilities::model::CapabilitySet` derives only `Eq`
/// (its `HashSet<Capability>` inner field cannot soundly derive `Hash`) —
/// so the concrete part of a function's inferred capability requirement is
/// this `Copy + Eq + Hash + Ord` bitset instead of a reused `CapabilitySet`.
/// See `docs/capability-effects-spec.md` §1.3 for the full representation
/// rationale. Kept manually in sync with `Capability`'s variant list via
/// `CapBits::from_capability` (this crate now depends on
/// `kryos-capabilities` for that one bridge — a deliberate, small deviation
/// from the spec's "no new Cargo.toml edge" note, made to avoid duplicating
/// the ~80-line, actively-maintained builtin-name -> capability table
/// verbatim in a second crate, which is a worse drift risk than one
/// dependency edge; see LEDGER for the full justification).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct CapBits(u32);

impl CapBits {
    pub const NET: CapBits = CapBits(1 << 0);
    pub const NET_HTTP: CapBits = CapBits(1 << 1);
    pub const NET_TCP: CapBits = CapBits(1 << 2);
    pub const IO: CapBits = CapBits(1 << 3);
    pub const FS_READ: CapBits = CapBits(1 << 4);
    pub const FS_WRITE: CapBits = CapBits(1 << 5);
    pub const FFI: CapBits = CapBits(1 << 6);
    pub const COMPUTE: CapBits = CapBits(1 << 7);
    pub const CRYPTO: CapBits = CapBits(1 << 8);
    pub const PROCESS: CapBits = CapBits(1 << 9);
    pub const ENV: CapBits = CapBits(1 << 10);
    pub const TERM: CapBits = CapBits(1 << 11);
    pub const DB: CapBits = CapBits(1 << 12);
    pub const TIME: CapBits = CapBits(1 << 13);
    pub const ALL: CapBits = CapBits(1 << 14);
    pub const EMPTY: CapBits = CapBits(0);

    pub fn union(self, other: CapBits) -> CapBits {
        CapBits(self.0 | other.0)
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True iff every bit set in `required` is also set in `self`. NOTE:
    /// this is raw bitwise containment, NOT the coarse/sub-capability
    /// lattice (`net` ⊇ `net:http`, `io` ⊇ `fs:read`/`fs:write`, `all` ⊇
    /// everything) that `Capability::satisfies` implements. Stage 1 has no
    /// enforcement consumer of this method; a future enforcement stage MUST
    /// route any accept/reject decision through a lattice-aware comparison
    /// (e.g. by converting both sides through `Capability`/`CapabilitySet`
    /// and calling `satisfies_required`/`is_subset_of` there) rather than
    /// this raw bit test, or a declared coarse `net` would wrongly fail to
    /// cover an inferred `net:http` bit.
    pub fn contains_bits(self, required: CapBits) -> bool {
        self.0 & required.0 == required.0
    }

    /// Human-readable names of the set bits, for the `KRYOS_DUMP_FN_EFFECTS`
    /// debug dump and diagnostics. Order is fixed (declaration order above).
    pub fn names(self) -> Vec<&'static str> {
        const TABLE: &[(CapBits, &str)] = &[
            (CapBits::NET, "net"),
            (CapBits::NET_HTTP, "net:http"),
            (CapBits::NET_TCP, "net:tcp"),
            (CapBits::IO, "io"),
            (CapBits::FS_READ, "fs:read"),
            (CapBits::FS_WRITE, "fs:write"),
            (CapBits::FFI, "ffi"),
            (CapBits::COMPUTE, "compute"),
            (CapBits::CRYPTO, "crypto"),
            (CapBits::PROCESS, "process"),
            (CapBits::ENV, "env"),
            (CapBits::TERM, "term"),
            (CapBits::DB, "db"),
            (CapBits::TIME, "time"),
            (CapBits::ALL, "all"),
        ];
        TABLE
            .iter()
            .filter(|(b, _)| self.contains_bits(*b))
            .map(|(_, n)| *n)
            .collect()
    }

    /// Bridge from the enforcement-layer `Capability` enum (kryos-capabilities).
    pub fn from_capability(cap: kryos_capabilities::model::Capability) -> CapBits {
        use kryos_capabilities::model::Capability as C;
        match cap {
            C::Net => CapBits::NET,
            C::NetHttp => CapBits::NET_HTTP,
            C::NetTcp => CapBits::NET_TCP,
            C::Io => CapBits::IO,
            C::FsRead => CapBits::FS_READ,
            C::FsWrite => CapBits::FS_WRITE,
            C::Ffi => CapBits::FFI,
            C::Compute => CapBits::COMPUTE,
            C::Crypto => CapBits::CRYPTO,
            C::Process => CapBits::PROCESS,
            C::Env => CapBits::ENV,
            C::Term => CapBits::TERM,
            C::Db => CapBits::DB,
            C::Time => CapBits::TIME,
            C::All => CapBits::ALL,
        }
    }
}

/// Capability-row variable id — same numbering scheme as ordinary type
/// variable ids (`Type::Var(u32)`), but tracked in a PARALLEL substitution
/// map (`InferenceEngine::cap_substitutions`) since a capability row is not
/// an ordinary `Type`. See `docs/capability-effects-spec.md` §1.3/§2.3.
pub type CapVarId = u32;

/// A capability row.
///
/// **Structural fail-closed guarantee (row-propagation stage,
/// `docs/capability-effects-spec.md`, gap 2 in the stage-1 verifier's
/// close-out list):** this is an ENUM, not a struct with a `Default` impl,
/// specifically so "provenance is unknown" and "provenance is the empty
/// set" are two constructions that cannot be confused or silently
/// substituted for one another. There is deliberately no
/// `impl Default for CapRow` — every site that needs a row MUST pick one of
/// `closed`/`empty`/`var`/`unknown` explicitly; a hypothetical
/// `CapRow::default()` that quietly meant "empty" is exactly the shape of
/// bug that caused every prior "Unknown must mean all, never nothing"
/// violation in this codebase's capability-checker history (see
/// `docs/capability-soundness.md` invariant 22), just relocated into the
/// new type-directed inference instead of the old syntax-directed checker.
///
/// - `Resolved { concrete, vars }` — a concrete lower bound plus zero or
///   more still-open row VARIABLES (ordinary Hindley-Milner-style
///   metavariables that WILL be bound by unification once enough of the
///   program has been walked — see `InferenceEngine::cap_substitutions`).
///   `vars` is kept SORTED + DEDUPED as a struct invariant so
///   `#[derive(Eq, Hash)]` gives correct structural equality for free.
/// - `Unknown` — provenance is not merely "not yet bound", it is
///   PERMANENTLY UNRESOLVABLE by this design: the value's real behavior
///   depends on a concrete implementation the checker cannot and never
///   will identify statically (a `dyn Trait` dispatch or a generic
///   trait-bound method call, where more than one — or zero yet-unwritten
///   — `impl` could be the one that actually runs). Every operation below
///   is defined so `Unknown` POISONS through it (`union`, `with_vars_
///   replaced`) and so any site that needs to materialize a row into
///   concrete bits (`concrete_bits`) gets `CapBits::ALL`, never
///   `CapBits::EMPTY`, for `Unknown` — the fail-closed default is therefore
///   the type's own structure, not a check a future call site has to
///   remember to perform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CapRow {
    Resolved { concrete: CapBits, vars: Vec<CapVarId> },
    Unknown,
}

impl CapRow {
    pub fn closed(bits: CapBits) -> Self {
        Self::Resolved {
            concrete: bits,
            vars: vec![],
        }
    }

    pub fn empty() -> Self {
        Self::closed(CapBits::EMPTY)
    }

    pub fn var(v: CapVarId) -> Self {
        Self::Resolved {
            concrete: CapBits::EMPTY,
            vars: vec![v],
        }
    }

    /// Provenance is genuinely, permanently unresolvable by this design —
    /// see the type-level doc comment above. Used by `dyn Trait` method
    /// dispatch and generic trait-bound method dispatch (row-propagation
    /// stage), and available to any future call site that discovers the
    /// same shape of gap.
    pub fn unknown() -> Self {
        Self::Unknown
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// A row is "closed" only when it is `Resolved` with no open vars.
    /// `Unknown` is deliberately NOT closed — treating it as closed would
    /// let `is_subset_of` read its `concrete_bits()` (`ALL`) as if that
    /// were a proven fact rather than a fail-closed placeholder, which
    /// would happen to be sound for `is_subset_of` today (`ALL` fails
    /// almost every subset check) but is exactly the kind of coincidental
    /// soundness this stage is required not to rely on.
    pub fn is_closed(&self) -> bool {
        match self {
            Self::Resolved { vars, .. } => vars.is_empty(),
            Self::Unknown => false,
        }
    }

    /// Materialize this row into a concrete bitset. `Unknown` MUST erase to
    /// `CapBits::ALL` here — this is the one erasure point every other
    /// erasure/display helper in this file is built from, so the
    /// fail-closed choice is made exactly once, structurally, instead of
    /// being a convention every future call site has to remember. NOTE:
    /// even for `Resolved`, this drops any still-open `vars` silently —
    /// callers that care whether a row is FULLY resolved must check
    /// `is_closed()` first (unchanged from before this stage).
    pub fn concrete_bits(&self) -> CapBits {
        match self {
            Self::Resolved { concrete, .. } => *concrete,
            Self::Unknown => CapBits::ALL,
        }
    }

    pub fn var_ids(&self) -> &[CapVarId] {
        match self {
            Self::Resolved { vars, .. } => vars,
            Self::Unknown => &[],
        }
    }

    /// Union of two rows: bits OR'd, var lists merged (sorted + deduped).
    /// `Unknown` POISONS the union — once any contributor to a row is
    /// unresolvable, the whole row is, by construction, never silently
    /// downgraded back to a merely-open-var or closed row by unioning in
    /// more (concrete) information.
    pub fn union(&self, other: &CapRow) -> CapRow {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (
                Self::Resolved { concrete: c1, vars: v1 },
                Self::Resolved { concrete: c2, vars: v2 },
            ) => {
                let mut vars = v1.clone();
                for v in v2 {
                    if !vars.contains(v) {
                        vars.push(*v);
                    }
                }
                vars.sort_unstable();
                Self::Resolved {
                    concrete: c1.union(*c2),
                    vars,
                }
            }
        }
    }

    pub fn union_bits(&self, bits: CapBits) -> CapRow {
        match self {
            Self::Unknown => Self::Unknown,
            Self::Resolved { concrete, vars } => Self::Resolved {
                concrete: concrete.union(bits),
                vars: vars.clone(),
            },
        }
    }

    /// Substitute every var in `vars` using `resolve`, unioning in whatever
    /// row each resolves to; drops vars that don't resolve (still open).
    /// Used by `InferenceEngine::resolve_cap_row`. `Unknown` has no vars to
    /// substitute and stays `Unknown` unconditionally — it is already a
    /// terminal answer, never a placeholder waiting on `resolve`.
    pub fn with_vars_replaced(&self, mut resolve: impl FnMut(CapVarId) -> Option<CapRow>) -> CapRow {
        let (concrete, vars) = match self {
            Self::Unknown => return Self::Unknown,
            Self::Resolved { concrete, vars } => (*concrete, vars),
        };
        let mut out = CapRow::closed(concrete);
        for &v in vars {
            match resolve(v) {
                Some(row) => out = out.union(&row),
                None => out = out.union(&CapRow::var(v)),
            }
        }
        out
    }

    /// Only decidable precisely when both sides are closed; see
    /// `docs/capability-effects-spec.md` §3 for the open-row comparison
    /// rule (not implemented here — Stage 1 carries no enforcement
    /// consumer). NOTE (see `CapBits::contains_bits`): this is raw bitwise
    /// containment, not the coarse/sub-capability lattice — not
    /// enforcement-ready. `Unknown.is_closed()` is `false`, so `Unknown`
    /// always reports "not a subset" here, never a false pass.
    pub fn is_subset_of(&self, other: &CapRow) -> bool {
        self.is_closed() && other.concrete_bits().contains_bits(self.concrete_bits())
    }

    /// Render as `{fs:read, net:http}` / `{}` / `{fs:read, ?C3}` (open var)
    /// / `{unknown->all}` (permanently unresolvable provenance — spelled
    /// out distinctly, never rendered as `{}`), for the
    /// `KRYOS_DUMP_FN_EFFECTS` debug dump.
    pub fn display(&self) -> String {
        match self {
            Self::Unknown => "{unknown->all}".to_string(),
            Self::Resolved { concrete, vars } => {
                let mut parts: Vec<String> =
                    concrete.names().into_iter().map(|s| s.to_string()).collect();
                for v in vars {
                    parts.push(format!("?C{v}"));
                }
                format!("{{{}}}", parts.join(", "))
            }
        }
    }
}

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
        /// The capability set this function value carries — inferred, never
        /// user-annotated yet (Stage 1: representation + inference only, no
        /// enforcement). See `docs/capability-effects-spec.md`.
        caps: CapRow,
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
            Type::Function { params, ret, .. } => {
                params.iter().any(|p| p.has_vars()) || ret.has_vars()
            }
            Type::Reference { inner, .. }
            | Type::Shared { inner }
            | Type::Weak { inner }
            | Type::Pointer { inner, .. } => inner.has_vars(),
            _ => false,
        }
    }

    /// Recursively rewrite every capability row reachable inside this type
    /// to `CapRow::Unknown` — used at any dispatch site where the checker
    /// cannot statically name the concrete implementation that will
    /// actually run (`dyn Trait` method dispatch, generic trait-bound
    /// method dispatch; see `docs/capability-effects-spec.md`, gaps 1/3 of
    /// the row-propagation stage). An EXHAUSTIVE match over every `Type`
    /// variant, with NO wildcard arm, deliberately — mirrors
    /// `docs/capability-soundness.md` §7's argument for
    /// `kryos-capabilities/src/checker.rs`'s exhaustive `Expr` matches: if
    /// a future `Type` variant is added that can carry a `Type::Function`
    /// value (a new container/wrapper kind), this fails to compile until a
    /// human decides whether it needs to recurse here, instead of silently
    /// passing an un-stamped — and therefore wrongly still-Resolved,
    /// possibly wrongly still-EMPTY — row through an unrecognized shape.
    pub fn with_caps_erased_to_unknown(&self) -> Type {
        match self {
            // Primitives and Error/Never/DynTrait/Var carry no capability
            // row anywhere inside them — nothing to stamp.
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
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Char
            | Type::Str
            | Type::USize
            | Type::ISize
            | Type::Void
            | Type::Never
            | Type::DynTrait { .. }
            | Type::Var(_)
            | Type::Error => self.clone(),
            Type::Array { element, size } => Type::Array {
                element: Box::new(element.with_caps_erased_to_unknown()),
                size: *size,
            },
            Type::Tuple { elements } => Type::Tuple {
                elements: elements.iter().map(|e| e.with_caps_erased_to_unknown()).collect(),
            },
            Type::Map { key, value } => Type::Map {
                key: Box::new(key.with_caps_erased_to_unknown()),
                value: Box::new(value.with_caps_erased_to_unknown()),
            },
            Type::Set { element } => Type::Set {
                element: Box::new(element.with_caps_erased_to_unknown()),
            },
            Type::Option { inner } => Type::Option {
                inner: Box::new(inner.with_caps_erased_to_unknown()),
            },
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(ok.with_caps_erased_to_unknown()),
                err: Box::new(err.with_caps_erased_to_unknown()),
            },
            Type::Struct { name, generics } => Type::Struct {
                name: name.clone(),
                generics: generics.iter().map(|g| g.with_caps_erased_to_unknown()).collect(),
            },
            Type::Enum { name, generics } => Type::Enum {
                name: name.clone(),
                generics: generics.iter().map(|g| g.with_caps_erased_to_unknown()).collect(),
            },
            Type::Function { params, ret, caps: _ } => Type::Function {
                params: params.iter().map(|p| p.with_caps_erased_to_unknown()).collect(),
                ret: Box::new(ret.with_caps_erased_to_unknown()),
                // The function VALUE itself is the thing dynamic dispatch
                // cannot pin to one concrete implementation — its own row
                // is the primary target this stamp exists to fix.
                caps: CapRow::unknown(),
            },
            Type::Reference { inner, mutable } => Type::Reference {
                inner: Box::new(inner.with_caps_erased_to_unknown()),
                mutable: *mutable,
            },
            Type::Shared { inner } => Type::Shared {
                inner: Box::new(inner.with_caps_erased_to_unknown()),
            },
            Type::Weak { inner } => Type::Weak {
                inner: Box::new(inner.with_caps_erased_to_unknown()),
            },
            Type::Pointer { inner, mutable } => Type::Pointer {
                inner: Box::new(inner.with_caps_erased_to_unknown()),
                mutable: *mutable,
            },
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
            // NOTE: `caps` is deliberately NOT rendered here — Display output
            // feeds diagnostic messages, and Stage 1 must not change any
            // existing error/warning text (behaviour-preserving requirement).
            Type::Function { params, ret, .. } => {
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
