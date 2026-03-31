"""
Kryos Language -- Type System, Semantic Analysis & Capability Checking

Bootstrap implementation covering the common cases: type resolution,
inference for let-bindings, operator type checking, function call
validation, struct literal checking, and capability tracking.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Set, Tuple

from kryos.compiler.ast_nodes import (
    ASTNode,
    ASTVisitor,
    Span,
    # Type nodes
    TypeNode,
    SimpleType,
    GenericType,
    ArrayType as ASTArrayType,
    FnType as ASTFnType,
    ReferenceType,
    TupleType as ASTTupleType,
    OptionType as ASTOptionType,
    TensorShape,
    # Expressions
    Expression,
    IntLiteral,
    FloatLiteral,
    StringLiteral,
    CharLiteral,
    BoolLiteral,
    NoneLiteral,
    InterpolatedString,
    Identifier,
    FieldAccess,
    IndexAccess,
    BinaryOp,
    UnaryOp,
    FnCall,
    MethodCall,
    ArrayLiteral,
    TupleLiteral,
    TensorLiteral,
    StructLiteral,
    Lambda,
    Parameter,
    RangeExpr,
    IfExpr,
    MatchExpr,
    MatchArm,
    PipeExpr,
    QuantumBlock,
    ComptimeBlock,
    # Statements
    Statement,
    BlockStmt,
    LetStmt,
    AssignStmt,
    ReturnStmt,
    IfStmt,
    ElifClause,
    ForStmt,
    WhileStmt,
    BreakStmt,
    ContinueStmt,
    ExprStmt,
    SpawnStmt,
    ParallelForStmt,
    # Declarations
    Declaration,
    GenericParam,
    FnDecl,
    StructField,
    StructDecl,
    EnumVariant,
    EnumDecl,
    TraitDecl,
    ImplBlock,
    ActorDecl,
    TypeAlias,
    ImportDecl,
    ModuleDecl,
    Module,
    MessageHandler,
    Attribute,
)


# ====================================================================
#  TYPE DEFINITIONS
# ====================================================================

class KryosType:
    """Base class for all Kryos types."""

    def is_assignable_from(self, other: KryosType) -> bool:
        """Return True if a value of *other* can be assigned to this type."""
        if self == other:
            return True
        return False

    def __eq__(self, other: object) -> bool:
        return type(self) is type(other)

    def __hash__(self) -> int:
        return hash(type(self).__name__)

    def __repr__(self) -> str:
        return type(self).__name__


class IntType(KryosType):
    def __init__(self, bits: int = 32, signed: bool = True):
        self.bits = bits
        self.signed = signed

    def is_assignable_from(self, other: KryosType) -> bool:
        if self == other:
            return True
        # Allow promotion: smaller int -> larger int of same signedness
        if isinstance(other, IntType) and other.signed == self.signed and other.bits <= self.bits:
            return True
        # Allow unsigned -> signed if there is room
        if isinstance(other, IntType) and not other.signed and self.signed and other.bits < self.bits:
            return True
        return False

    def __eq__(self, other: object) -> bool:
        return isinstance(other, IntType) and self.bits == other.bits and self.signed == other.signed

    def __hash__(self) -> int:
        return hash(("IntType", self.bits, self.signed))

    def __repr__(self) -> str:
        prefix = "i" if self.signed else "u"
        return f"{prefix}{self.bits}"


class FloatType(KryosType):
    def __init__(self, bits: int = 64):
        self.bits = bits

    def is_assignable_from(self, other: KryosType) -> bool:
        if self == other:
            return True
        # f32 -> f64 promotion
        if isinstance(other, FloatType) and other.bits <= self.bits:
            return True
        # int -> float widening
        if isinstance(other, IntType):
            return True
        return False

    def __eq__(self, other: object) -> bool:
        return isinstance(other, FloatType) and self.bits == other.bits

    def __hash__(self) -> int:
        return hash(("FloatType", self.bits))

    def __repr__(self) -> str:
        return f"f{self.bits}"


class BoolType(KryosType):
    def __repr__(self) -> str:
        return "bool"


class StrType(KryosType):
    def __repr__(self) -> str:
        return "str"


class CharType(KryosType):
    def __repr__(self) -> str:
        return "char"


class VoidType(KryosType):
    def __repr__(self) -> str:
        return "void"


class NoneType(KryosType):
    """The type of the ``none`` literal."""

    def is_assignable_from(self, other: KryosType) -> bool:
        return isinstance(other, NoneType)

    def __repr__(self) -> str:
        return "none"


class TensorType(KryosType):
    def __init__(self, element_type: KryosType, shape: Optional[List[Optional[int]]] = None):
        self.element_type = element_type
        self.shape = shape  # None means fully dynamic; individual None dims are dynamic

    def is_assignable_from(self, other: KryosType) -> bool:
        if not isinstance(other, TensorType):
            return False
        if not self.element_type.is_assignable_from(other.element_type):
            return False
        if self.shape is None:
            return True  # dynamic shape accepts anything
        if other.shape is None:
            return True  # allow dynamic -> static at assignment (runtime check)
        if len(self.shape) != len(other.shape):
            return False
        for sd, od in zip(self.shape, other.shape):
            if sd is not None and od is not None and sd != od:
                return False
        return True

    def __eq__(self, other: object) -> bool:
        return (isinstance(other, TensorType)
                and self.element_type == other.element_type
                and self.shape == other.shape)

    def __hash__(self) -> int:
        shape_key = tuple(self.shape) if self.shape else None
        return hash(("TensorType", self.element_type, shape_key))

    def __repr__(self) -> str:
        shape_str = "?" if self.shape is None else repr(self.shape)
        return f"Tensor<{self.element_type}, {shape_str}>"


class VecType(KryosType):
    def __init__(self, element_type: KryosType):
        self.element_type = element_type

    def is_assignable_from(self, other: KryosType) -> bool:
        return isinstance(other, VecType) and self.element_type.is_assignable_from(other.element_type)

    def __eq__(self, other: object) -> bool:
        return isinstance(other, VecType) and self.element_type == other.element_type

    def __hash__(self) -> int:
        return hash(("VecType", self.element_type))

    def __repr__(self) -> str:
        return f"Vec<{self.element_type}>"


class MapType(KryosType):
    def __init__(self, key_type: KryosType, value_type: KryosType):
        self.key_type = key_type
        self.value_type = value_type

    def is_assignable_from(self, other: KryosType) -> bool:
        return (isinstance(other, MapType)
                and self.key_type.is_assignable_from(other.key_type)
                and self.value_type.is_assignable_from(other.value_type))

    def __eq__(self, other: object) -> bool:
        return (isinstance(other, MapType)
                and self.key_type == other.key_type
                and self.value_type == other.value_type)

    def __hash__(self) -> int:
        return hash(("MapType", self.key_type, self.value_type))

    def __repr__(self) -> str:
        return f"Map<{self.key_type}, {self.value_type}>"


class SetType(KryosType):
    def __init__(self, element_type: KryosType):
        self.element_type = element_type

    def is_assignable_from(self, other: KryosType) -> bool:
        return isinstance(other, SetType) and self.element_type.is_assignable_from(other.element_type)

    def __eq__(self, other: object) -> bool:
        return isinstance(other, SetType) and self.element_type == other.element_type

    def __hash__(self) -> int:
        return hash(("SetType", self.element_type))

    def __repr__(self) -> str:
        return f"Set<{self.element_type}>"


class OptionType(KryosType):
    def __init__(self, inner_type: KryosType):
        self.inner_type = inner_type

    def is_assignable_from(self, other: KryosType) -> bool:
        if isinstance(other, NoneType):
            return True
        if isinstance(other, OptionType):
            return self.inner_type.is_assignable_from(other.inner_type)
        # Allow assigning the inner type directly to an Option
        return self.inner_type.is_assignable_from(other)

    def __eq__(self, other: object) -> bool:
        return isinstance(other, OptionType) and self.inner_type == other.inner_type

    def __hash__(self) -> int:
        return hash(("OptionType", self.inner_type))

    def __repr__(self) -> str:
        return f"Option<{self.inner_type}>"


class ResultType(KryosType):
    def __init__(self, ok_type: KryosType, err_type: KryosType):
        self.ok_type = ok_type
        self.err_type = err_type

    def is_assignable_from(self, other: KryosType) -> bool:
        return (isinstance(other, ResultType)
                and self.ok_type.is_assignable_from(other.ok_type)
                and self.err_type.is_assignable_from(other.err_type))

    def __eq__(self, other: object) -> bool:
        return (isinstance(other, ResultType)
                and self.ok_type == other.ok_type
                and self.err_type == other.err_type)

    def __hash__(self) -> int:
        return hash(("ResultType", self.ok_type, self.err_type))

    def __repr__(self) -> str:
        return f"Result<{self.ok_type}, {self.err_type}>"


class SecretType(KryosType):
    """Timing-safe wrapper type."""
    def __init__(self, inner_type: KryosType):
        self.inner_type = inner_type

    def is_assignable_from(self, other: KryosType) -> bool:
        if isinstance(other, SecretType):
            return self.inner_type.is_assignable_from(other.inner_type)
        # Allow wrapping a plain value into a Secret
        return self.inner_type.is_assignable_from(other)

    def __eq__(self, other: object) -> bool:
        return isinstance(other, SecretType) and self.inner_type == other.inner_type

    def __hash__(self) -> int:
        return hash(("SecretType", self.inner_type))

    def __repr__(self) -> str:
        return f"Secret<{self.inner_type}>"


class QubitType(KryosType):
    def __repr__(self) -> str:
        return "Qubit"


class QuregType(KryosType):
    def __init__(self, size: Optional[int] = None):
        self.size = size

    def is_assignable_from(self, other: KryosType) -> bool:
        if not isinstance(other, QuregType):
            return False
        if self.size is None:
            return True
        return self.size == other.size

    def __eq__(self, other: object) -> bool:
        return isinstance(other, QuregType) and self.size == other.size

    def __hash__(self) -> int:
        return hash(("QuregType", self.size))

    def __repr__(self) -> str:
        if self.size is not None:
            return f"Qureg<{self.size}>"
        return "Qureg"


class FnType(KryosType):
    def __init__(self, param_types: List[KryosType], return_type: KryosType):
        self.param_types = param_types
        self.return_type = return_type

    def is_assignable_from(self, other: KryosType) -> bool:
        if not isinstance(other, FnType):
            return False
        if len(self.param_types) != len(other.param_types):
            return False
        # Contravariant params, covariant return
        for sp, op in zip(self.param_types, other.param_types):
            if not op.is_assignable_from(sp):
                return False
        return self.return_type.is_assignable_from(other.return_type)

    def __eq__(self, other: object) -> bool:
        return (isinstance(other, FnType)
                and self.param_types == other.param_types
                and self.return_type == other.return_type)

    def __hash__(self) -> int:
        return hash(("FnType", tuple(self.param_types), self.return_type))

    def __repr__(self) -> str:
        params = ", ".join(repr(p) for p in self.param_types)
        return f"fn({params}) -> {self.return_type}"


class TupleType(KryosType):
    def __init__(self, element_types: List[KryosType]):
        self.element_types = element_types

    def is_assignable_from(self, other: KryosType) -> bool:
        if not isinstance(other, TupleType):
            return False
        if len(self.element_types) != len(other.element_types):
            return False
        return all(s.is_assignable_from(o) for s, o in zip(self.element_types, other.element_types))

    def __eq__(self, other: object) -> bool:
        return isinstance(other, TupleType) and self.element_types == other.element_types

    def __hash__(self) -> int:
        return hash(("TupleType", tuple(self.element_types)))

    def __repr__(self) -> str:
        elems = ", ".join(repr(e) for e in self.element_types)
        return f"({elems})"


class RefType(KryosType):
    def __init__(self, inner_type: KryosType, mutable: bool = False):
        self.inner_type = inner_type
        self.mutable = mutable

    def is_assignable_from(self, other: KryosType) -> bool:
        if isinstance(other, RefType):
            if self.mutable and not other.mutable:
                return False  # can't assign &T to &mut T
            return self.inner_type.is_assignable_from(other.inner_type)
        # Allow assigning a value to a reference slot (auto-borrow)
        return self.inner_type.is_assignable_from(other)

    def __eq__(self, other: object) -> bool:
        return (isinstance(other, RefType)
                and self.inner_type == other.inner_type
                and self.mutable == other.mutable)

    def __hash__(self) -> int:
        return hash(("RefType", self.inner_type, self.mutable))

    def __repr__(self) -> str:
        mut = "mut " if self.mutable else ""
        return f"&{mut}{self.inner_type}"


class ArrayType(KryosType):
    def __init__(self, element_type: KryosType, size: Optional[int] = None):
        self.element_type = element_type
        self.size = size

    def is_assignable_from(self, other: KryosType) -> bool:
        if isinstance(other, ArrayType):
            if not self.element_type.is_assignable_from(other.element_type):
                return False
            if self.size is not None and other.size is not None and self.size != other.size:
                return False
            return True
        return False

    def __eq__(self, other: object) -> bool:
        return (isinstance(other, ArrayType)
                and self.element_type == other.element_type
                and self.size == other.size)

    def __hash__(self) -> int:
        return hash(("ArrayType", self.element_type, self.size))

    def __repr__(self) -> str:
        if self.size is not None:
            return f"[{self.element_type}; {self.size}]"
        return f"[{self.element_type}]"


class StructType(KryosType):
    def __init__(self, name: str, fields: Dict[str, KryosType],
                 generics: Optional[List[str]] = None):
        self.name = name
        self.fields = fields
        self.generics = generics or []

    def is_assignable_from(self, other: KryosType) -> bool:
        if isinstance(other, StructType):
            return self.name == other.name
        return False

    def __eq__(self, other: object) -> bool:
        return isinstance(other, StructType) and self.name == other.name

    def __hash__(self) -> int:
        return hash(("StructType", self.name))

    def __repr__(self) -> str:
        return self.name


class EnumType(KryosType):
    def __init__(self, name: str, variants: Dict[str, List[KryosType]]):
        self.name = name
        self.variants = variants  # variant_name -> list of field types

    def is_assignable_from(self, other: KryosType) -> bool:
        if isinstance(other, EnumType):
            return self.name == other.name
        return False

    def __eq__(self, other: object) -> bool:
        return isinstance(other, EnumType) and self.name == other.name

    def __hash__(self) -> int:
        return hash(("EnumType", self.name))

    def __repr__(self) -> str:
        return self.name


class TraitType(KryosType):
    def __init__(self, name: str, methods: Dict[str, FnType]):
        self.name = name
        self.methods = methods  # method_name -> FnType

    def is_assignable_from(self, other: KryosType) -> bool:
        # Structural subtyping: accept any type that has the trait implemented
        # (actual impl checking is done separately)
        return isinstance(other, TraitType) and self.name == other.name

    def __eq__(self, other: object) -> bool:
        return isinstance(other, TraitType) and self.name == other.name

    def __hash__(self) -> int:
        return hash(("TraitType", self.name))

    def __repr__(self) -> str:
        return f"trait {self.name}"


class TypeVar(KryosType):
    """Unresolved type variable used during inference and for generics."""
    _counter = 0

    def __init__(self, name: Optional[str] = None):
        if name is None:
            TypeVar._counter += 1
            name = f"_T{TypeVar._counter}"
        self.name = name

    def is_assignable_from(self, other: KryosType) -> bool:
        return True  # type vars accept anything during unification

    def __eq__(self, other: object) -> bool:
        return isinstance(other, TypeVar) and self.name == other.name

    def __hash__(self) -> int:
        return hash(("TypeVar", self.name))

    def __repr__(self) -> str:
        return self.name


# ====================================================================
#  BUILTIN TYPE REGISTRY
# ====================================================================

BUILTIN_TYPES: Dict[str, KryosType] = {
    "i8": IntType(8, True),
    "i16": IntType(16, True),
    "i32": IntType(32, True),
    "i64": IntType(64, True),
    "i128": IntType(128, True),
    "u8": IntType(8, False),
    "u16": IntType(16, False),
    "u32": IntType(32, False),
    "u64": IntType(64, False),
    "u128": IntType(128, False),
    "f32": FloatType(32),
    "f64": FloatType(64),
    "bool": BoolType(),
    "str": StrType(),
    "char": CharType(),
    "void": VoidType(),
    "none": NoneType(),
    "Qubit": QubitType(),
    "Qureg": QuregType(),
}


# ====================================================================
#  SYMBOL TABLE / ENVIRONMENT
# ====================================================================

@dataclass
class Symbol:
    """A named entity in the program (variable, function, type, ...)."""
    name: str
    type: KryosType
    mutable: bool = False
    capabilities: Set[str] = field(default_factory=set)


class Environment:
    """Scoped symbol table with parent chain for lexical scoping."""

    def __init__(self, parent: Optional[Environment] = None,
                 capabilities: Optional[Set[str]] = None):
        self.parent = parent
        self.symbols: Dict[str, Symbol] = {}
        self.types: Dict[str, KryosType] = {}
        # Capability set for this scope -- inherited from parent if not given
        if capabilities is not None:
            self.capabilities = set(capabilities)
        elif parent is not None:
            self.capabilities = set(parent.capabilities)
        else:
            self.capabilities = set()

    def define(self, name: str, symbol: Symbol) -> None:
        self.symbols[name] = symbol

    def define_type(self, name: str, ktype: KryosType) -> None:
        self.types[name] = ktype

    def lookup(self, name: str) -> Optional[Symbol]:
        if name in self.symbols:
            return self.symbols[name]
        if self.parent is not None:
            return self.parent.lookup(name)
        return None

    def lookup_type(self, name: str) -> Optional[KryosType]:
        if name in self.types:
            return self.types[name]
        if self.parent is not None:
            return self.parent.lookup_type(name)
        return None

    def enter_scope(self, capabilities: Optional[Set[str]] = None) -> Environment:
        return Environment(parent=self, capabilities=capabilities)

    def exit_scope(self) -> Environment:
        if self.parent is None:
            raise RuntimeError("Cannot exit the global scope")
        return self.parent


# ====================================================================
#  TYPE ERRORS
# ====================================================================

class TypeError:
    """A single type/semantic error."""

    def __init__(self, message: str, span: Span):
        self.message = message
        self.span = span

    def __repr__(self) -> str:
        line, col = self.span[0], self.span[1]
        return f"Type error at line {line}, col {col}: {self.message}"


# ====================================================================
#  NUMERIC HELPERS
# ====================================================================

def _is_numeric(t: KryosType) -> bool:
    return isinstance(t, (IntType, FloatType))


def _is_integer(t: KryosType) -> bool:
    return isinstance(t, IntType)


def _common_numeric_type(a: KryosType, b: KryosType) -> Optional[KryosType]:
    """Return the promoted common type for two numeric types, or None."""
    if not (_is_numeric(a) and _is_numeric(b)):
        return None
    # float wins over int
    if isinstance(a, FloatType) and isinstance(b, FloatType):
        return FloatType(max(a.bits, b.bits))
    if isinstance(a, FloatType):
        return a
    if isinstance(b, FloatType):
        return b
    # Both ints
    assert isinstance(a, IntType) and isinstance(b, IntType)
    if a.signed == b.signed:
        return IntType(max(a.bits, b.bits), a.signed)
    # Mixed sign: promote to signed with enough room
    return IntType(max(a.bits, b.bits, 32), True)


# ====================================================================
#  TYPE CHECKER / SEMANTIC ANALYZER
# ====================================================================

class TypeChecker(ASTVisitor):
    """
    Walk the AST, resolve types, check type compatibility, track capabilities.

    After visiting, ``self.errors`` contains all diagnosed problems and AST
    expression nodes have a ``resolved_type`` attribute attached.
    """

    def __init__(self) -> None:
        self.errors: List[TypeError] = []
        self.env = Environment()
        self._current_fn_return: Optional[KryosType] = None
        # Seed the environment with builtin types
        for name, ktype in BUILTIN_TYPES.items():
            self.env.define_type(name, ktype)

    # -----------------------------------------------------------------
    #  Error helpers
    # -----------------------------------------------------------------

    def _error(self, msg: str, span: Span) -> None:
        self.errors.append(TypeError(msg, span))

    def _span(self, node: ASTNode) -> Span:
        return node.span

    # -----------------------------------------------------------------
    #  Type resolution: AST TypeNode -> KryosType
    # -----------------------------------------------------------------

    def resolve_type(self, tnode: Optional[TypeNode]) -> KryosType:
        """Convert an AST TypeNode into a concrete KryosType."""
        if tnode is None:
            return TypeVar()  # needs inference

        if isinstance(tnode, SimpleType):
            return self._resolve_simple(tnode)

        if isinstance(tnode, GenericType):
            return self._resolve_generic(tnode)

        if isinstance(tnode, ASTArrayType):
            elem = self.resolve_type(tnode.element_type)
            return ArrayType(elem, tnode.size)

        if isinstance(tnode, ASTFnType):
            params = [self.resolve_type(p) for p in tnode.param_types]
            ret = self.resolve_type(tnode.return_type) if tnode.return_type else VoidType()
            return FnType(params, ret)

        if isinstance(tnode, ReferenceType):
            inner = self.resolve_type(tnode.inner)
            return RefType(inner, tnode.mutable)

        if isinstance(tnode, ASTTupleType):
            elems = [self.resolve_type(e) for e in tnode.element_types]
            return TupleType(elems)

        if isinstance(tnode, ASTOptionType):
            inner = self.resolve_type(tnode.inner)
            return OptionType(inner)

        if isinstance(tnode, TensorShape):
            # This shouldn't appear alone -- it's embedded in GenericType
            return TypeVar()

        self._error(f"Unknown type node: {type(tnode).__name__}", tnode.span)
        return TypeVar()

    def _resolve_simple(self, tnode: SimpleType) -> KryosType:
        name = tnode.name
        # Check builtins first
        builtin = BUILTIN_TYPES.get(name)
        if builtin is not None:
            return builtin
        # Check user-defined types in environment
        user_type = self.env.lookup_type(name)
        if user_type is not None:
            return user_type
        # Could be a type variable in a generic context
        sym = self.env.lookup(name)
        if sym is not None and isinstance(sym.type, TypeVar):
            return sym.type
        self._error(f"Unknown type '{name}'", tnode.span)
        return TypeVar(name)

    def _resolve_generic(self, tnode: GenericType) -> KryosType:
        name = tnode.name
        type_args = [self.resolve_type(p) for p in tnode.type_params
                     if not isinstance(p, TensorShape)]
        shape_args = [p for p in tnode.type_params if isinstance(p, TensorShape)]

        if name == "Tensor":
            elem = type_args[0] if type_args else FloatType(32)
            shape = shape_args[0].dimensions if shape_args else None
            return TensorType(elem, shape)

        if name == "Vec":
            elem = type_args[0] if type_args else TypeVar()
            return VecType(elem)

        if name == "Map":
            if len(type_args) >= 2:
                return MapType(type_args[0], type_args[1])
            self._error("Map requires two type parameters", tnode.span)
            return MapType(TypeVar(), TypeVar())

        if name == "Set":
            elem = type_args[0] if type_args else TypeVar()
            return SetType(elem)

        if name == "Option":
            inner = type_args[0] if type_args else TypeVar()
            return OptionType(inner)

        if name == "Result":
            if len(type_args) >= 2:
                return ResultType(type_args[0], type_args[1])
            self._error("Result requires two type parameters", tnode.span)
            return ResultType(TypeVar(), TypeVar())

        if name == "Secret":
            inner = type_args[0] if type_args else TypeVar()
            return SecretType(inner)

        if name == "Qureg":
            # Qureg might have a size parameter
            if type_args:
                return QuregType()
            return QuregType()

        # User-defined generic type
        user_type = self.env.lookup_type(name)
        if user_type is not None:
            return user_type

        self._error(f"Unknown generic type '{name}'", tnode.span)
        return TypeVar(name)

    # -----------------------------------------------------------------
    #  Annotation helper -- attach resolved_type to an AST node
    # -----------------------------------------------------------------

    def _annotate(self, node: ASTNode, ktype: KryosType) -> KryosType:
        node.resolved_type = ktype  # type: ignore[attr-defined]
        return ktype

    # -----------------------------------------------------------------
    #  Capability helpers
    # -----------------------------------------------------------------

    def _extract_capabilities(self, attrs: List[Attribute]) -> Set[str]:
        """Pull capability strings out of @capabilities(...) attributes."""
        caps: Set[str] = set()
        for attr in attrs:
            if attr.name == "capabilities":
                for arg in attr.args:
                    if isinstance(arg, StringLiteral):
                        caps.add(arg.value)
        return caps

    def _check_capability(self, required: str, span: Span) -> None:
        if self.env.capabilities and required not in self.env.capabilities:
            self._error(
                f"Capability '{required}' is required but not available in this scope",
                span,
            )

    # -----------------------------------------------------------------
    #  VISITORS -- Declarations
    # -----------------------------------------------------------------

    def visit_Module(self, node: Module) -> KryosType:
        for decl in node.declarations:
            self.visit(decl)
        return VoidType()

    def visit_FnDecl(self, node: FnDecl) -> KryosType:
        # Register generic params as TypeVars
        fn_scope = self.env.enter_scope()
        for gp in node.generics:
            tv = TypeVar(gp.name)
            fn_scope.define(gp.name, Symbol(gp.name, tv))
            fn_scope.define_type(gp.name, tv)

        # Resolve parameter types
        param_types: List[KryosType] = []
        for param in node.params:
            pt = self.resolve_type(param.type_annotation)
            param_types.append(pt)
            fn_scope.define(param.name, Symbol(param.name, pt, mutable=False))

        ret_type = self.resolve_type(node.return_type) if node.return_type else VoidType()

        fn_type = FnType(param_types, ret_type)
        # Define function in the outer scope
        self.env.define(node.name, Symbol(node.name, fn_type))

        # Capabilities for this function scope
        caps = self._extract_capabilities(node.attributes)
        if caps:
            fn_scope.capabilities = caps

        # Type-check body
        old_env = self.env
        old_ret = self._current_fn_return
        self.env = fn_scope
        self._current_fn_return = ret_type

        if node.body:
            self.visit(node.body)

        self.env = old_env
        self._current_fn_return = old_ret

        return self._annotate(node, fn_type)

    def visit_StructDecl(self, node: StructDecl) -> KryosType:
        fields: Dict[str, KryosType] = {}
        for sf in node.fields:
            fields[sf.name] = self.resolve_type(sf.type_annotation)
        generics = [gp.name for gp in node.generics]
        st = StructType(node.name, fields, generics)
        self.env.define_type(node.name, st)
        # Also define as a value (constructor-like)
        self.env.define(node.name, Symbol(node.name, st))
        return self._annotate(node, st)

    def visit_EnumDecl(self, node: EnumDecl) -> KryosType:
        variants: Dict[str, List[KryosType]] = {}
        for v in node.variants:
            variants[v.name] = [self.resolve_type(f) for f in v.fields]
        et = EnumType(node.name, variants)
        self.env.define_type(node.name, et)
        self.env.define(node.name, Symbol(node.name, et))
        return self._annotate(node, et)

    def visit_TraitDecl(self, node: TraitDecl) -> KryosType:
        methods: Dict[str, FnType] = {}
        for m in node.methods:
            param_types = [self.resolve_type(p.type_annotation) for p in m.params]
            ret = self.resolve_type(m.return_type) if m.return_type else VoidType()
            methods[m.name] = FnType(param_types, ret)
        tt = TraitType(node.name, methods)
        self.env.define_type(node.name, tt)
        return self._annotate(node, tt)

    def visit_ImplBlock(self, node: ImplBlock) -> KryosType:
        target = self.resolve_type(node.target_type)
        for m in node.methods:
            self.visit(m)
            # Register method on struct if applicable
            if isinstance(target, StructType):
                sym = self.env.lookup(m.name)
                if sym:
                    # Prefix with type name for method dispatch
                    self.env.define(f"{target.name}::{m.name}", sym)
        return VoidType()

    def visit_ActorDecl(self, node: ActorDecl) -> KryosType:
        fields: Dict[str, KryosType] = {}
        for sf in node.state_fields:
            fields[sf.name] = self.resolve_type(sf.type_annotation)
        st = StructType(node.name, fields)
        self.env.define_type(node.name, st)
        self.env.define(node.name, Symbol(node.name, st))

        # Check handlers
        actor_scope = self.env.enter_scope()
        # Define self fields
        for fname, ftype in fields.items():
            actor_scope.define(fname, Symbol(fname, ftype, mutable=True))

        old_env = self.env
        self.env = actor_scope
        for handler in node.handlers:
            self._visit_message_handler(handler)
        self.env = old_env

        return self._annotate(node, st)

    def _visit_message_handler(self, node: MessageHandler) -> None:
        scope = self.env.enter_scope()
        param_types = []
        for p in node.params:
            pt = self.resolve_type(p.type_annotation)
            param_types.append(pt)
            scope.define(p.name, Symbol(p.name, pt))

        ret_type = self.resolve_type(node.return_type) if node.return_type else VoidType()

        old_env = self.env
        old_ret = self._current_fn_return
        self.env = scope
        self._current_fn_return = ret_type

        if node.body:
            self.visit(node.body)

        self.env = old_env
        self._current_fn_return = old_ret

    def visit_TypeAlias(self, node: TypeAlias) -> KryosType:
        resolved = self.resolve_type(node.value)
        self.env.define_type(node.name, resolved)
        return self._annotate(node, resolved)

    def visit_ImportDecl(self, node: ImportDecl) -> KryosType:
        # Imports are resolved at a higher level; just acknowledge
        return VoidType()

    def visit_ModuleDecl(self, node: ModuleDecl) -> KryosType:
        mod_scope = self.env.enter_scope()
        old_env = self.env
        self.env = mod_scope
        for decl in node.body:
            self.visit(decl)
        self.env = old_env
        return VoidType()

    # -----------------------------------------------------------------
    #  VISITORS -- Statements
    # -----------------------------------------------------------------

    def visit_BlockStmt(self, node: BlockStmt) -> KryosType:
        scope = self.env.enter_scope()
        old_env = self.env
        self.env = scope
        result = VoidType()
        for stmt in node.statements:
            result = self.visit(stmt) or VoidType()
        self.env = old_env
        return result

    def visit_LetStmt(self, node: LetStmt) -> KryosType:
        declared_type = self.resolve_type(node.type_annotation)
        inferred_type: Optional[KryosType] = None

        if node.value is not None:
            inferred_type = self.visit(node.value)

        # Determine final type
        if isinstance(declared_type, TypeVar) and inferred_type is not None:
            # No annotation -- infer from value
            final_type = inferred_type
        elif not isinstance(declared_type, TypeVar) and inferred_type is not None:
            # Both annotation and value -- check compatibility
            if not declared_type.is_assignable_from(inferred_type):
                self._error(
                    f"Cannot assign '{inferred_type}' to variable '{node.name}' "
                    f"of type '{declared_type}'",
                    node.span,
                )
            final_type = declared_type
        else:
            final_type = declared_type

        self.env.define(node.name, Symbol(node.name, final_type, mutable=node.mutable))
        return self._annotate(node, final_type)

    def visit_AssignStmt(self, node: AssignStmt) -> KryosType:
        target_type = self.visit(node.target) if node.target else VoidType()
        value_type = self.visit(node.value) if node.value else VoidType()

        # Check mutability
        if isinstance(node.target, Identifier):
            sym = self.env.lookup(node.target.name)
            if sym is not None and not sym.mutable:
                self._error(
                    f"Cannot assign to immutable variable '{node.target.name}'",
                    node.span,
                )

        # For compound assignment, check operator validity
        if node.operator != "=":
            base_op = node.operator[:-1]  # strip '='
            self._check_binary_op(base_op, target_type, value_type, node.span)
        else:
            if not target_type.is_assignable_from(value_type):
                self._error(
                    f"Cannot assign '{value_type}' to '{target_type}'",
                    node.span,
                )

        return VoidType()

    def visit_ReturnStmt(self, node: ReturnStmt) -> KryosType:
        if node.value is not None:
            val_type = self.visit(node.value)
        else:
            val_type = VoidType()

        if self._current_fn_return is not None:
            if not self._current_fn_return.is_assignable_from(val_type):
                self._error(
                    f"Return type '{val_type}' does not match "
                    f"expected return type '{self._current_fn_return}'",
                    node.span,
                )
        return val_type

    def visit_IfStmt(self, node: IfStmt) -> KryosType:
        if node.condition:
            cond_type = self.visit(node.condition)
            if not isinstance(cond_type, (BoolType, TypeVar)):
                self._error(
                    f"If condition must be 'bool', got '{cond_type}'",
                    node.condition.span,
                )
        if node.body:
            self.visit(node.body)
        for elif_cl in node.elif_clauses:
            if elif_cl.condition:
                ct = self.visit(elif_cl.condition)
                if not isinstance(ct, (BoolType, TypeVar)):
                    self._error(
                        f"Elif condition must be 'bool', got '{ct}'",
                        elif_cl.condition.span,
                    )
            if elif_cl.body:
                self.visit(elif_cl.body)
        if node.else_body:
            self.visit(node.else_body)
        return VoidType()

    def visit_ForStmt(self, node: ForStmt) -> KryosType:
        iter_type = self.visit(node.iterable) if node.iterable else TypeVar()

        # Deduce element type from iterable
        elem_type: KryosType
        if isinstance(iter_type, VecType):
            elem_type = iter_type.element_type
        elif isinstance(iter_type, ArrayType):
            elem_type = iter_type.element_type
        elif isinstance(iter_type, SetType):
            elem_type = iter_type.element_type
        elif isinstance(iter_type, MapType):
            elem_type = TupleType([iter_type.key_type, iter_type.value_type])
        elif isinstance(iter_type, StrType):
            elem_type = CharType()
        elif isinstance(iter_type, TypeVar):
            elem_type = TypeVar()
        else:
            elem_type = TypeVar()

        scope = self.env.enter_scope()
        scope.define(node.variable, Symbol(node.variable, elem_type))
        old_env = self.env
        self.env = scope
        if node.body:
            self.visit(node.body)
        self.env = old_env
        return VoidType()

    def visit_WhileStmt(self, node: WhileStmt) -> KryosType:
        if node.condition:
            cond_type = self.visit(node.condition)
            if not isinstance(cond_type, (BoolType, TypeVar)):
                self._error(
                    f"While condition must be 'bool', got '{cond_type}'",
                    node.condition.span,
                )
        if node.body:
            self.visit(node.body)
        return VoidType()

    def visit_BreakStmt(self, node: BreakStmt) -> KryosType:
        return VoidType()

    def visit_ContinueStmt(self, node: ContinueStmt) -> KryosType:
        return VoidType()

    def visit_ExprStmt(self, node: ExprStmt) -> KryosType:
        if node.expression:
            return self.visit(node.expression)
        return VoidType()

    def visit_SpawnStmt(self, node: SpawnStmt) -> KryosType:
        self._check_capability("concurrency", node.span)
        if node.target:
            self.visit(node.target)
        if node.body:
            self.visit(node.body)
        return VoidType()

    def visit_ParallelForStmt(self, node: ParallelForStmt) -> KryosType:
        self._check_capability("concurrency", node.span)
        # Treat like ForStmt
        iter_type = self.visit(node.iterable) if node.iterable else TypeVar()
        elem_type: KryosType
        if isinstance(iter_type, (VecType, ArrayType, SetType)):
            elem_type = iter_type.element_type  # type: ignore[union-attr]
        else:
            elem_type = TypeVar()

        scope = self.env.enter_scope()
        scope.define(node.variable, Symbol(node.variable, elem_type))
        old_env = self.env
        self.env = scope
        if node.body:
            self.visit(node.body)
        self.env = old_env
        return VoidType()

    # -----------------------------------------------------------------
    #  VISITORS -- Expressions
    # -----------------------------------------------------------------

    def visit_IntLiteral(self, node: IntLiteral) -> KryosType:
        return self._annotate(node, IntType(32, True))

    def visit_FloatLiteral(self, node: FloatLiteral) -> KryosType:
        return self._annotate(node, FloatType(64))

    def visit_StringLiteral(self, node: StringLiteral) -> KryosType:
        return self._annotate(node, StrType())

    def visit_CharLiteral(self, node: CharLiteral) -> KryosType:
        return self._annotate(node, CharType())

    def visit_BoolLiteral(self, node: BoolLiteral) -> KryosType:
        return self._annotate(node, BoolType())

    def visit_NoneLiteral(self, node: NoneLiteral) -> KryosType:
        return self._annotate(node, NoneType())

    def visit_InterpolatedString(self, node: InterpolatedString) -> KryosType:
        for part in node.parts:
            self.visit(part)
        return self._annotate(node, StrType())

    def visit_Identifier(self, node: Identifier) -> KryosType:
        sym = self.env.lookup(node.name)
        if sym is None:
            self._error(f"Undefined variable '{node.name}'", node.span)
            return self._annotate(node, TypeVar(node.name))
        return self._annotate(node, sym.type)

    def visit_FieldAccess(self, node: FieldAccess) -> KryosType:
        obj_type = self.visit(node.object) if node.object else TypeVar()

        if isinstance(obj_type, StructType):
            if node.field in obj_type.fields:
                return self._annotate(node, obj_type.fields[node.field])
            self._error(
                f"Struct '{obj_type.name}' has no field '{node.field}'",
                node.span,
            )
            return self._annotate(node, TypeVar())

        if isinstance(obj_type, TupleType):
            # Tuple field access by numeric index: t.0, t.1, etc.
            try:
                idx = int(node.field)
                if 0 <= idx < len(obj_type.element_types):
                    return self._annotate(node, obj_type.element_types[idx])
                self._error(
                    f"Tuple index {idx} out of range (tuple has "
                    f"{len(obj_type.element_types)} elements)",
                    node.span,
                )
            except ValueError:
                self._error(
                    f"Cannot access field '{node.field}' on tuple type",
                    node.span,
                )
            return self._annotate(node, TypeVar())

        if isinstance(obj_type, TypeVar):
            return self._annotate(node, TypeVar())

        self._error(
            f"Cannot access field '{node.field}' on type '{obj_type}'",
            node.span,
        )
        return self._annotate(node, TypeVar())

    def visit_IndexAccess(self, node: IndexAccess) -> KryosType:
        obj_type = self.visit(node.object) if node.object else TypeVar()
        idx_type = self.visit(node.index) if node.index else TypeVar()

        if isinstance(obj_type, (VecType, ArrayType)):
            if not isinstance(idx_type, (IntType, TypeVar)):
                self._error(
                    f"Index must be an integer, got '{idx_type}'",
                    node.span,
                )
            return self._annotate(node, obj_type.element_type)

        if isinstance(obj_type, MapType):
            if not obj_type.key_type.is_assignable_from(idx_type):
                self._error(
                    f"Map key type is '{obj_type.key_type}', got '{idx_type}'",
                    node.span,
                )
            return self._annotate(node, obj_type.value_type)

        if isinstance(obj_type, TensorType):
            return self._annotate(node, obj_type.element_type)

        if isinstance(obj_type, StrType):
            return self._annotate(node, CharType())

        if isinstance(obj_type, TypeVar):
            return self._annotate(node, TypeVar())

        self._error(f"Type '{obj_type}' is not indexable", node.span)
        return self._annotate(node, TypeVar())

    def visit_BinaryOp(self, node: BinaryOp) -> KryosType:
        left_type = self.visit(node.left) if node.left else TypeVar()
        right_type = self.visit(node.right) if node.right else TypeVar()
        result = self._check_binary_op(node.operator, left_type, right_type, node.span)
        return self._annotate(node, result)

    def _check_binary_op(self, op: str, left: KryosType, right: KryosType,
                         span: Span) -> KryosType:
        """Validate a binary operation and return its result type."""

        # Bail out early if either side is unknown
        if isinstance(left, TypeVar) or isinstance(right, TypeVar):
            return TypeVar()

        # Arithmetic: + - * / % **
        if op in ("+", "-", "*", "/", "%", "**"):
            if _is_numeric(left) and _is_numeric(right):
                return _common_numeric_type(left, right) or left
            # String concatenation
            if op == "+" and isinstance(left, StrType) and isinstance(right, StrType):
                return StrType()
            # Vec concatenation
            if op == "+" and isinstance(left, VecType) and isinstance(right, VecType):
                if left == right:
                    return left
            self._error(
                f"Cannot apply '{op}' to '{left}' and '{right}'",
                span,
            )
            return TypeVar()

        # Matrix multiply: @
        if op == "@":
            if isinstance(left, TensorType) and isinstance(right, TensorType):
                self._check_capability("gpu", span)
                elem = _common_numeric_type(left.element_type, right.element_type)
                return TensorType(elem or left.element_type)
            self._error(
                f"Operator '@' requires tensor operands, got '{left}' and '{right}'",
                span,
            )
            return TypeVar()

        # Comparison: == != < > <= >=
        if op in ("==", "!=", "<", ">", "<=", ">="):
            if _is_numeric(left) and _is_numeric(right):
                return BoolType()
            if type(left) is type(right):
                return BoolType()
            self._error(
                f"Cannot compare '{left}' and '{right}' with '{op}'",
                span,
            )
            return BoolType()

        # Logical: and or
        if op in ("and", "or"):
            if isinstance(left, BoolType) and isinstance(right, BoolType):
                return BoolType()
            self._error(
                f"Logical '{op}' requires bool operands, got '{left}' and '{right}'",
                span,
            )
            return BoolType()

        # Bitwise: & | ^ << >>
        if op in ("&", "|", "^", "<<", ">>"):
            if _is_integer(left) and _is_integer(right):
                return _common_numeric_type(left, right) or left
            self._error(
                f"Bitwise '{op}' requires integer operands, got '{left}' and '{right}'",
                span,
            )
            return TypeVar()

        # Pipe: |> handled in visit_PipeExpr
        return TypeVar()

    def visit_UnaryOp(self, node: UnaryOp) -> KryosType:
        operand_type = self.visit(node.operand) if node.operand else TypeVar()

        if isinstance(operand_type, TypeVar):
            return self._annotate(node, TypeVar())

        if node.operator == "-":
            if _is_numeric(operand_type):
                return self._annotate(node, operand_type)
            self._error(
                f"Cannot negate type '{operand_type}'",
                node.span,
            )

        elif node.operator in ("!", "not"):
            if isinstance(operand_type, BoolType):
                return self._annotate(node, BoolType())
            self._error(
                f"Logical not requires 'bool', got '{operand_type}'",
                node.span,
            )

        elif node.operator == "~":
            if _is_integer(operand_type):
                return self._annotate(node, operand_type)
            self._error(
                f"Bitwise not requires integer type, got '{operand_type}'",
                node.span,
            )

        return self._annotate(node, TypeVar())

    def visit_FnCall(self, node: FnCall) -> KryosType:
        callee_type = self.visit(node.callee) if node.callee else TypeVar()
        arg_types = [self.visit(a) for a in node.args]

        if isinstance(callee_type, FnType):
            # Check argument count
            if len(arg_types) != len(callee_type.param_types):
                self._error(
                    f"Function expects {len(callee_type.param_types)} arguments, "
                    f"got {len(arg_types)}",
                    node.span,
                )
            else:
                for i, (expected, actual) in enumerate(
                    zip(callee_type.param_types, arg_types)
                ):
                    if not isinstance(expected, TypeVar) and not isinstance(actual, TypeVar):
                        if not expected.is_assignable_from(actual):
                            self._error(
                                f"Argument {i + 1}: expected '{expected}', got '{actual}'",
                                node.span,
                            )
            return self._annotate(node, callee_type.return_type)

        if isinstance(callee_type, StructType):
            # Struct constructor call
            return self._annotate(node, callee_type)

        if isinstance(callee_type, TypeVar):
            return self._annotate(node, TypeVar())

        self._error(f"Type '{callee_type}' is not callable", node.span)
        return self._annotate(node, TypeVar())

    def visit_MethodCall(self, node: MethodCall) -> KryosType:
        obj_type = self.visit(node.object) if node.object else TypeVar()
        arg_types = [self.visit(a) for a in node.args]

        # Look up method on the type
        if isinstance(obj_type, StructType):
            method_sym = self.env.lookup(f"{obj_type.name}::{node.method}")
            if method_sym and isinstance(method_sym.type, FnType):
                fn = method_sym.type
                # Skip 'self' parameter for arg count
                expected_count = len(fn.param_types)
                if expected_count > 0:
                    # First param may be self
                    expected_count -= 1
                if len(arg_types) != expected_count:
                    self._error(
                        f"Method '{node.method}' expects {expected_count} arguments, "
                        f"got {len(arg_types)}",
                        node.span,
                    )
                return self._annotate(node, fn.return_type)

        if isinstance(obj_type, TypeVar):
            return self._annotate(node, TypeVar())

        # Common built-in methods
        if isinstance(obj_type, VecType) and node.method in ("push", "pop", "len", "get"):
            if node.method == "len":
                return self._annotate(node, IntType(64, False))
            if node.method == "pop":
                return self._annotate(node, OptionType(obj_type.element_type))
            if node.method == "get":
                return self._annotate(node, OptionType(obj_type.element_type))
            if node.method == "push":
                if arg_types:
                    if not obj_type.element_type.is_assignable_from(arg_types[0]):
                        self._error(
                            f"Cannot push '{arg_types[0]}' into Vec<{obj_type.element_type}>",
                            node.span,
                        )
                return self._annotate(node, VoidType())

        if isinstance(obj_type, StrType) and node.method in ("len", "contains", "split"):
            if node.method == "len":
                return self._annotate(node, IntType(64, False))
            if node.method == "contains":
                return self._annotate(node, BoolType())
            if node.method == "split":
                return self._annotate(node, VecType(StrType()))

        if isinstance(obj_type, MapType) and node.method in ("get", "insert", "contains_key", "len"):
            if node.method == "get":
                return self._annotate(node, OptionType(obj_type.value_type))
            if node.method == "insert":
                return self._annotate(node, VoidType())
            if node.method == "contains_key":
                return self._annotate(node, BoolType())
            if node.method == "len":
                return self._annotate(node, IntType(64, False))

        if isinstance(obj_type, OptionType) and node.method in ("unwrap", "is_some", "is_none", "map"):
            if node.method == "unwrap":
                return self._annotate(node, obj_type.inner_type)
            if node.method in ("is_some", "is_none"):
                return self._annotate(node, BoolType())
            if node.method == "map":
                return self._annotate(node, TypeVar())

        if isinstance(obj_type, ResultType) and node.method in ("unwrap", "is_ok", "is_err"):
            if node.method == "unwrap":
                return self._annotate(node, obj_type.ok_type)
            if node.method in ("is_ok", "is_err"):
                return self._annotate(node, BoolType())

        if isinstance(obj_type, TensorType) and node.method in ("shape", "reshape", "transpose", "matmul"):
            if node.method == "shape":
                return self._annotate(node, VecType(IntType(64, False)))
            if node.method in ("reshape", "transpose", "matmul"):
                return self._annotate(node, obj_type)

        # Fallback: unknown method
        self._error(
            f"Unknown method '{node.method}' on type '{obj_type}'",
            node.span,
        )
        return self._annotate(node, TypeVar())

    def visit_ArrayLiteral(self, node: ArrayLiteral) -> KryosType:
        if not node.elements:
            return self._annotate(node, ArrayType(TypeVar(), 0))

        elem_types = [self.visit(e) for e in node.elements]
        # Use first non-TypeVar element as the canonical type
        canonical = TypeVar()
        for et in elem_types:
            if not isinstance(et, TypeVar):
                canonical = et
                break

        if not isinstance(canonical, TypeVar):
            for i, et in enumerate(elem_types):
                if not isinstance(et, TypeVar) and not canonical.is_assignable_from(et):
                    # Try numeric promotion
                    promoted = _common_numeric_type(canonical, et)
                    if promoted:
                        canonical = promoted
                    else:
                        self._error(
                            f"Array element {i} has type '{et}', expected '{canonical}'",
                            node.span,
                        )

        return self._annotate(node, ArrayType(canonical, len(node.elements)))

    def visit_TupleLiteral(self, node: TupleLiteral) -> KryosType:
        elem_types = [self.visit(e) for e in node.elements]
        return self._annotate(node, TupleType(elem_types))

    def visit_TensorLiteral(self, node: TensorLiteral) -> KryosType:
        self._check_capability("gpu", node.span)
        elem_type: KryosType = FloatType(32)
        if node.dtype:
            elem_type = self.resolve_type(node.dtype)
        shape: Optional[List[Optional[int]]] = None
        if node.shape:
            shape = [d for d in node.shape.dimensions]  # type: ignore[misc]
        return self._annotate(node, TensorType(elem_type, shape))

    def visit_StructLiteral(self, node: StructLiteral) -> KryosType:
        st = self.env.lookup_type(node.type_name)
        if st is None:
            self._error(f"Unknown struct type '{node.type_name}'", node.span)
            return self._annotate(node, TypeVar(node.type_name))

        if not isinstance(st, StructType):
            self._error(f"'{node.type_name}' is not a struct type", node.span)
            return self._annotate(node, TypeVar())

        provided: Set[str] = set()
        for field_name, value_expr in node.field_values:
            provided.add(field_name)
            val_type = self.visit(value_expr)
            if field_name not in st.fields:
                self._error(
                    f"Struct '{st.name}' has no field '{field_name}'",
                    node.span,
                )
            else:
                expected = st.fields[field_name]
                if not expected.is_assignable_from(val_type):
                    self._error(
                        f"Field '{field_name}': expected '{expected}', got '{val_type}'",
                        node.span,
                    )

        # Check for missing fields
        for fname in st.fields:
            if fname not in provided:
                self._error(
                    f"Missing field '{fname}' in struct literal '{st.name}'",
                    node.span,
                )

        return self._annotate(node, st)

    def visit_Lambda(self, node: Lambda) -> KryosType:
        scope = self.env.enter_scope()
        param_types: List[KryosType] = []
        for p in node.params:
            pt = self.resolve_type(p.type_annotation)
            param_types.append(pt)
            scope.define(p.name, Symbol(p.name, pt))

        ret_type_hint = self.resolve_type(node.return_type) if node.return_type else None

        old_env = self.env
        old_ret = self._current_fn_return
        self.env = scope
        self._current_fn_return = ret_type_hint

        body_type = VoidType()
        if node.body:
            body_type = self.visit(node.body) or VoidType()

        self.env = old_env
        self._current_fn_return = old_ret

        ret = ret_type_hint if ret_type_hint and not isinstance(ret_type_hint, TypeVar) else body_type
        fn_type = FnType(param_types, ret)
        return self._annotate(node, fn_type)

    def visit_RangeExpr(self, node: RangeExpr) -> KryosType:
        start_type = self.visit(node.start) if node.start else IntType(32, True)
        end_type = self.visit(node.end) if node.end else IntType(32, True)
        if not _is_integer(start_type) and not isinstance(start_type, TypeVar):
            self._error(f"Range start must be integer, got '{start_type}'", node.span)
        if not _is_integer(end_type) and not isinstance(end_type, TypeVar):
            self._error(f"Range end must be integer, got '{end_type}'", node.span)
        # Range is iterable over its element type
        common = _common_numeric_type(start_type, end_type) or IntType(32, True)
        return self._annotate(node, VecType(common))

    def visit_IfExpr(self, node: IfExpr) -> KryosType:
        if node.condition:
            cond_type = self.visit(node.condition)
            if not isinstance(cond_type, (BoolType, TypeVar)):
                self._error(
                    f"If expression condition must be 'bool', got '{cond_type}'",
                    node.span,
                )
        then_type = self.visit(node.then_expr) if node.then_expr else VoidType()
        else_type = self.visit(node.else_expr) if node.else_expr else VoidType()

        # Both branches should produce compatible types
        if not isinstance(then_type, TypeVar) and not isinstance(else_type, TypeVar):
            if then_type != else_type and not then_type.is_assignable_from(else_type):
                self._error(
                    f"If expression branches have incompatible types: "
                    f"'{then_type}' vs '{else_type}'",
                    node.span,
                )

        result = then_type if not isinstance(then_type, TypeVar) else else_type
        return self._annotate(node, result)

    def visit_MatchExpr(self, node: MatchExpr) -> KryosType:
        val_type = self.visit(node.value) if node.value else TypeVar()

        arm_types: List[KryosType] = []
        for arm in node.arms:
            if arm.pattern:
                self.visit(arm.pattern)
            if arm.guard:
                guard_type = self.visit(arm.guard)
                if not isinstance(guard_type, (BoolType, TypeVar)):
                    self._error("Match guard must be 'bool'", arm.span)
            if arm.body:
                arm_types.append(self.visit(arm.body))

        # All arms should have compatible result types
        if arm_types:
            result = arm_types[0]
            for at in arm_types[1:]:
                if not isinstance(result, TypeVar) and not isinstance(at, TypeVar):
                    if result != at:
                        # Try to find a common type
                        promoted = _common_numeric_type(result, at)
                        if promoted:
                            result = promoted
                        else:
                            self._error(
                                f"Match arms have incompatible types: '{result}' vs '{at}'",
                                node.span,
                            )
            return self._annotate(node, result)

        return self._annotate(node, VoidType())

    def visit_PipeExpr(self, node: PipeExpr) -> KryosType:
        left_type = self.visit(node.left) if node.left else TypeVar()
        right_type = self.visit(node.right) if node.right else TypeVar()

        # The right side should be callable, and left is its argument
        if isinstance(right_type, FnType):
            if len(right_type.param_types) >= 1:
                expected = right_type.param_types[0]
                if not isinstance(expected, TypeVar) and not isinstance(left_type, TypeVar):
                    if not expected.is_assignable_from(left_type):
                        self._error(
                            f"Pipe: function expects '{expected}', got '{left_type}'",
                            node.span,
                        )
            return self._annotate(node, right_type.return_type)

        if isinstance(right_type, TypeVar):
            return self._annotate(node, TypeVar())

        self._error(
            f"Right side of pipe must be callable, got '{right_type}'",
            node.span,
        )
        return self._annotate(node, TypeVar())

    def visit_QuantumBlock(self, node: QuantumBlock) -> KryosType:
        self._check_capability("quantum", node.span)
        scope = self.env.enter_scope()
        old_env = self.env
        self.env = scope
        for stmt in node.body:
            self.visit(stmt)
        self.env = old_env
        return self._annotate(node, VoidType())

    def visit_ComptimeBlock(self, node: ComptimeBlock) -> KryosType:
        scope = self.env.enter_scope()
        old_env = self.env
        self.env = scope
        result = VoidType()
        for stmt in node.body:
            result = self.visit(stmt) or VoidType()
        self.env = old_env
        return self._annotate(node, result)

    # -----------------------------------------------------------------
    #  VISITORS -- Passthrough nodes
    # -----------------------------------------------------------------

    def visit_MatchArm(self, node: MatchArm) -> KryosType:
        # Handled inline by visit_MatchExpr
        return VoidType()

    def visit_Attribute(self, node: Attribute) -> KryosType:
        return VoidType()

    def visit_Parameter(self, node: Parameter) -> KryosType:
        return self.resolve_type(node.type_annotation)

    def visit_GenericParam(self, node: GenericParam) -> KryosType:
        return TypeVar(node.name)

    # -----------------------------------------------------------------
    #  PUBLIC API
    # -----------------------------------------------------------------

    def check(self, module: Module) -> List[TypeError]:
        """
        Run the type checker on a parsed module.

        Returns a list of ``TypeError`` objects.  The AST is annotated
        in-place: expression / declaration nodes gain a ``resolved_type``
        attribute.
        """
        self.visit(module)
        return self.errors
