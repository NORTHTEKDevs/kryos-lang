"""
Kryos Language -- Abstract Syntax Tree Node Definitions

Every node carries a `span` tuple (start_line, start_col, end_line, end_col)
for source-location tracking and implements the visitor pattern via `accept()`.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any, List, Optional, Tuple, Union


# ---------------------------------------------------------------------------
# Span
# ---------------------------------------------------------------------------

#: (start_line, start_col, end_line, end_col)  -- 1-based lines, 0-based cols
Span = Tuple[int, int, int, int]

NO_SPAN: Span = (0, 0, 0, 0)


# ---------------------------------------------------------------------------
# Visitor
# ---------------------------------------------------------------------------

class ASTVisitor(ABC):
    """
    Base visitor.  Override ``visit_<ClassName>`` for every node type you
    care about; unhandled nodes fall through to ``generic_visit``.
    """

    def visit(self, node: "ASTNode") -> Any:
        method = getattr(self, f"visit_{type(node).__name__}", self.generic_visit)
        return method(node)

    def generic_visit(self, node: "ASTNode") -> Any:  # noqa: ARG002
        """Default -- do nothing."""
        return None


# ---------------------------------------------------------------------------
# Base class
# ---------------------------------------------------------------------------

@dataclass
class ASTNode(ABC):
    """Root of the AST hierarchy.  Every concrete node must be a dataclass."""
    span: Span = NO_SPAN

    def accept(self, visitor: ASTVisitor) -> Any:
        return visitor.visit(self)


# ===================================================================
#  TYPE NODES
# ===================================================================

@dataclass
class TypeNode(ASTNode):
    """Abstract base for all type representations."""


@dataclass
class SimpleType(TypeNode):
    """
    A plain named type -- ``i32``, ``bool``, ``str``, ``MyStruct``, ...
    """
    name: str = ""


@dataclass
class GenericType(TypeNode):
    """
    A parameterised type -- ``Vec<i32>``, ``Tensor<f32, [3, 3]>``, ...
    """
    name: str = ""
    type_params: List[TypeNode] = field(default_factory=list)


@dataclass
class ArrayType(TypeNode):
    """``[T]`` (dynamic) or ``[T; N]`` (fixed-size)."""
    element_type: Optional[TypeNode] = None
    size: Optional[int] = None


@dataclass
class FnType(TypeNode):
    """``fn(A, B) -> C``"""
    param_types: List[TypeNode] = field(default_factory=list)
    return_type: Optional[TypeNode] = None


@dataclass
class ReferenceType(TypeNode):
    """``&T`` or ``&mut T``."""
    inner: Optional[TypeNode] = None
    mutable: bool = False


@dataclass
class TupleType(TypeNode):
    """``(A, B, C)``"""
    element_types: List[TypeNode] = field(default_factory=list)


@dataclass
class OptionType(TypeNode):
    """Sugar for ``Option<T>`` -- written ``T?`` in Kryos."""
    inner: Optional[TypeNode] = None


@dataclass
class TensorShape(TypeNode):
    """Dimension descriptor for tensor types, e.g. ``[128, 64]``."""
    dimensions: List[int] = field(default_factory=list)


# ===================================================================
#  ATTRIBUTES
# ===================================================================

@dataclass
class Attribute(ASTNode):
    """
    Decorator-style annotation::

        @capabilities("gpu", "network")
        @compute(device="cuda")
        @export
    """
    name: str = ""
    args: List["Expression"] = field(default_factory=list)


# ===================================================================
#  EXPRESSIONS
# ===================================================================

@dataclass
class Expression(ASTNode):
    """Abstract base for all expression nodes."""


# -- Literals ---------------------------------------------------------------

@dataclass
class IntLiteral(Expression):
    """Integer literal: ``42``, ``0xFF``."""
    value: int = 0


@dataclass
class FloatLiteral(Expression):
    """Floating-point literal: ``3.14``, ``1e-8``."""
    value: float = 0.0


@dataclass
class StringLiteral(Expression):
    """Plain string literal: ``"hello"``."""
    value: str = ""


@dataclass
class CharLiteral(Expression):
    """Character literal: ``'a'``."""
    value: str = ""


@dataclass
class BoolLiteral(Expression):
    """Boolean literal: ``true`` / ``false``."""
    value: bool = False


@dataclass
class NoneLiteral(Expression):
    """The ``none`` literal."""


@dataclass
class InterpolatedString(Expression):
    """
    ``f"hello {name}, you are {age} years old"``

    *parts* alternates between raw ``StringLiteral`` segments and
    arbitrary embedded expressions.
    """
    parts: List[Expression] = field(default_factory=list)


# -- Names & access ---------------------------------------------------------

@dataclass
class Identifier(Expression):
    """A bare name reference: ``x``, ``std``."""
    name: str = ""


@dataclass
class FieldAccess(Expression):
    """``object.field``"""
    object: Optional[Expression] = None
    field: str = ""


@dataclass
class IndexAccess(Expression):
    """``object[index]``"""
    object: Optional[Expression] = None
    index: Optional[Expression] = None


# -- Operators ---------------------------------------------------------------

@dataclass
class BinaryOp(Expression):
    """
    Binary operator expression.

    Operators include arithmetic (``+ - * / % **``), matrix (``@``),
    comparison (``== != < > <= >=``), logical (``and or``),
    and bitwise (``& | ^ << >>``).
    """
    left: Optional[Expression] = None
    operator: str = ""
    right: Optional[Expression] = None


@dataclass
class UnaryOp(Expression):
    """Unary operator: ``-x``, ``!flag``, ``~bits``, ``not cond``."""
    operator: str = ""
    operand: Optional[Expression] = None


# -- Calls -------------------------------------------------------------------

@dataclass
class FnCall(Expression):
    """
    ``callee(args)`` or ``callee::<TypeArgs>(args)``
    """
    callee: Optional[Expression] = None
    args: List[Expression] = field(default_factory=list)
    type_args: List[TypeNode] = field(default_factory=list)


@dataclass
class MethodCall(Expression):
    """``object.method(args)`` or ``object.method::<T>(args)``"""
    object: Optional[Expression] = None
    method: str = ""
    args: List[Expression] = field(default_factory=list)
    type_args: List[TypeNode] = field(default_factory=list)


# -- Collection literals -----------------------------------------------------

@dataclass
class ArrayLiteral(Expression):
    """``[1, 2, 3]``"""
    elements: List[Expression] = field(default_factory=list)


@dataclass
class TupleLiteral(Expression):
    """``(1, "two", 3.0)``"""
    elements: List[Expression] = field(default_factory=list)


@dataclass
class TensorLiteral(Expression):
    """
    Tensor literal -- nested numeric arrays with optional dtype / shape
    metadata resolved during type checking.
    """
    elements: List[Expression] = field(default_factory=list)
    dtype: Optional[TypeNode] = None
    shape: Optional[TensorShape] = None


@dataclass
class MapLiteral(Expression):
    """Map/dictionary literal: ``{"key": value, "key2": value2}``"""
    pairs: List[Tuple[Expression, Expression]] = field(default_factory=list)


@dataclass
class StructLiteral(Expression):
    """``Point { x: 1, y: 2 }``"""
    type_name: str = ""
    field_values: List[Tuple[str, Expression]] = field(default_factory=list)


# -- Parameters & Lambda -----------------------------------------------------

@dataclass
class Parameter(ASTNode):
    """A single function or lambda parameter."""
    name: str = ""
    type_annotation: Optional[TypeNode] = None
    default_value: Optional[Expression] = None


@dataclass
class Lambda(Expression):
    """
    ``(x, y) => x + y``  or  ``(x: i32) -> i32 => { ... }``
    """
    params: List[Parameter] = field(default_factory=list)
    return_type: Optional[TypeNode] = None
    body: Optional[Union[Expression, "BlockStmt"]] = None


# -- Compound expressions ----------------------------------------------------

@dataclass
class RangeExpr(Expression):
    """``start..end`` or ``start..=end``"""
    start: Optional[Expression] = None
    end: Optional[Expression] = None
    inclusive: bool = False


@dataclass
class IfExpr(Expression):
    """Ternary / inline-if: ``if cond then a else b``"""
    condition: Optional[Expression] = None
    then_expr: Optional[Expression] = None
    else_expr: Optional[Expression] = None


@dataclass
class MatchArm(ASTNode):
    """A single arm inside a ``match`` expression."""
    pattern: Optional[Expression] = None
    guard: Optional[Expression] = None
    body: Optional[Expression] = None


@dataclass
class MatchExpr(Expression):
    """``match value { pattern => expr, ... }``"""
    value: Optional[Expression] = None
    arms: List[MatchArm] = field(default_factory=list)


@dataclass
class PipeExpr(Expression):
    """Pipe operator: ``left |> right``"""
    left: Optional[Expression] = None
    right: Optional[Expression] = None


@dataclass
class QuantumBlock(Expression):
    """``quantum { ... }`` -- quantum computing block."""
    body: List["Statement"] = field(default_factory=list)


@dataclass
class ComptimeBlock(Expression):
    """``comptime { ... }`` -- compile-time evaluation block."""
    body: List["Statement"] = field(default_factory=list)


# ===================================================================
#  STATEMENTS
# ===================================================================

@dataclass
class Statement(ASTNode):
    """Abstract base for all statement nodes."""


@dataclass
class BlockStmt(Statement):
    """A braced block of statements: ``{ ... }``"""
    statements: List[Statement] = field(default_factory=list)


@dataclass
class LetStmt(Statement):
    """
    ``let x: i32 = 42``  or  ``let mut y = "hello"``
    """
    name: str = ""
    type_annotation: Optional[TypeNode] = None
    value: Optional[Expression] = None
    mutable: bool = False


@dataclass
class AssignStmt(Statement):
    """
    ``target = value``  or  ``target += value``  etc.

    *operator* is one of ``=  +=  -=  *=  /=  %=  **=  @=  &=  |=  ^=  <<=  >>=``
    """
    target: Optional[Expression] = None
    operator: str = "="
    value: Optional[Expression] = None


@dataclass
class ReturnStmt(Statement):
    """``return value`` or bare ``return``."""
    value: Optional[Expression] = None


@dataclass
class ElifClause(ASTNode):
    """A single ``elif`` branch."""
    condition: Optional[Expression] = None
    body: Optional[BlockStmt] = None


@dataclass
class IfStmt(Statement):
    """
    ``if cond { ... } elif cond { ... } else { ... }``
    """
    condition: Optional[Expression] = None
    body: Optional[BlockStmt] = None
    elif_clauses: List[ElifClause] = field(default_factory=list)
    else_body: Optional[BlockStmt] = None


@dataclass
class ForStmt(Statement):
    """``for x in iterable { ... }``"""
    variable: str = ""
    iterable: Optional[Expression] = None
    body: Optional[BlockStmt] = None


@dataclass
class WhileStmt(Statement):
    """``while cond { ... }``"""
    condition: Optional[Expression] = None
    body: Optional[BlockStmt] = None


@dataclass
class BreakStmt(Statement):
    """``break``"""


@dataclass
class ContinueStmt(Statement):
    """``continue``"""


@dataclass
class ExprStmt(Statement):
    """An expression used as a statement (e.g. a bare function call)."""
    expression: Optional[Expression] = None


@dataclass
class SpawnStmt(Statement):
    """
    ``spawn actor_expr``  or  ``spawn { parallel_work }``
    """
    target: Optional[Expression] = None
    body: Optional[BlockStmt] = None


@dataclass
class ParallelForStmt(Statement):
    """``parallel for x in iterable { ... }``"""
    variable: str = ""
    iterable: Optional[Expression] = None
    body: Optional[BlockStmt] = None


@dataclass
class TryCatchStmt(Statement):
    """``try { body } catch (name) { handler }``"""
    try_body: Optional[BlockStmt] = None
    catch_name: str = "error"
    catch_body: Optional[BlockStmt] = None


@dataclass
class ThrowStmt(Statement):
    """``throw expression``"""
    value: Optional[Expression] = None


# ===================================================================
#  DECLARATIONS
# ===================================================================

@dataclass
class Declaration(ASTNode):
    """Abstract base for top-level declarations."""


@dataclass
class GenericParam(ASTNode):
    """
    A single generic type parameter, optionally with trait bounds::

        T
        T: Numeric
        T: Numeric + Display
    """
    name: str = ""
    bounds: List[TypeNode] = field(default_factory=list)


@dataclass
class FnDecl(Declaration):
    """
    Function declaration::

        @capabilities("gpu")
        @compute(device="cuda")
        fn matmul<T: Numeric>(a: Tensor<T>, b: Tensor<T>) -> Tensor<T> { ... }
    """
    name: str = ""
    params: List[Parameter] = field(default_factory=list)
    return_type: Optional[TypeNode] = None
    body: Optional[BlockStmt] = None
    attributes: List[Attribute] = field(default_factory=list)
    generics: List[GenericParam] = field(default_factory=list)


@dataclass
class StructField(ASTNode):
    """A single field inside a ``struct`` declaration."""
    name: str = ""
    type_annotation: Optional[TypeNode] = None
    default_value: Optional[Expression] = None
    attributes: List[Attribute] = field(default_factory=list)


@dataclass
class StructDecl(Declaration):
    """
    ::

        struct Point<T> {
            x: T,
            y: T,
        }
    """
    name: str = ""
    fields: List[StructField] = field(default_factory=list)
    generics: List[GenericParam] = field(default_factory=list)
    attributes: List[Attribute] = field(default_factory=list)


@dataclass
class EnumVariant(ASTNode):
    """A single variant of an enum -- may carry associated data."""
    name: str = ""
    fields: List[TypeNode] = field(default_factory=list)
    attributes: List[Attribute] = field(default_factory=list)


@dataclass
class EnumDecl(Declaration):
    """
    ::

        enum Shape {
            Circle(f64),
            Rect(f64, f64),
            None,
        }
    """
    name: str = ""
    variants: List[EnumVariant] = field(default_factory=list)
    generics: List[GenericParam] = field(default_factory=list)
    attributes: List[Attribute] = field(default_factory=list)


@dataclass
class TraitDecl(Declaration):
    """
    ::

        trait Drawable {
            fn draw(&self) -> void;
        }
    """
    name: str = ""
    methods: List[FnDecl] = field(default_factory=list)
    generics: List[GenericParam] = field(default_factory=list)
    attributes: List[Attribute] = field(default_factory=list)


@dataclass
class ImplBlock(Declaration):
    """
    ::

        impl Drawable for Circle { ... }
        impl Circle { ... }
    """
    target_type: Optional[TypeNode] = None
    trait_type: Optional[TypeNode] = None
    methods: List[FnDecl] = field(default_factory=list)
    attributes: List[Attribute] = field(default_factory=list)


@dataclass
class MessageHandler(ASTNode):
    """A single message handler inside an ``actor`` declaration."""
    name: str = ""
    params: List[Parameter] = field(default_factory=list)
    return_type: Optional[TypeNode] = None
    body: Optional[BlockStmt] = None
    attributes: List[Attribute] = field(default_factory=list)


@dataclass
class ActorDecl(Declaration):
    """
    ::

        actor Counter {
            state count: i32 = 0;
            on increment(amount: i32) { ... }
            on get_count() -> i32  { ... }
        }
    """
    name: str = ""
    state_fields: List[StructField] = field(default_factory=list)
    handlers: List[MessageHandler] = field(default_factory=list)
    attributes: List[Attribute] = field(default_factory=list)


@dataclass
class TypeAlias(Declaration):
    """``type Vec3 = Tensor<f32, [3]>;``"""
    name: str = ""
    generics: List[GenericParam] = field(default_factory=list)
    value: Optional[TypeNode] = None
    attributes: List[Attribute] = field(default_factory=list)


# ===================================================================
#  PROGRAM STRUCTURE
# ===================================================================

@dataclass
class ImportPath(ASTNode):
    """
    The path in a ``use`` / ``import`` statement::

        std::collections::HashMap
        std::io::{Read, Write}
        std::collections::*
    """
    segments: List[str] = field(default_factory=list)
    alias: Optional[str] = None
    is_glob: bool = False


@dataclass
class ImportDecl(Declaration):
    """
    ::

        use std::collections::HashMap;
        use std::io::{Read, Write};
        extern use "libcuda.so" as cuda;
    """
    path: Optional[ImportPath] = None
    items: List[ImportPath] = field(default_factory=list)
    is_extern: bool = False
    extern_source: Optional[str] = None


@dataclass
class ModuleDecl(Declaration):
    """
    ::

        mod math { ... }
    """
    name: str = ""
    body: List[Declaration] = field(default_factory=list)
    attributes: List[Attribute] = field(default_factory=list)


@dataclass
class Module(ASTNode):
    """Top-level compilation unit -- the entire source file."""
    name: str = ""
    declarations: List[Declaration] = field(default_factory=list)
