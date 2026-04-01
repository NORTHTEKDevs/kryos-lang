"""
Kryos Language -- Ownership Analyzer (Borrow Checker)

Compile-time pass that enforces memory safety rules with zero runtime cost.
Runs between parsing and codegen, analyzing the AST to catch:
  - Use-after-move
  - Double mutable borrow
  - Simultaneous mutable + immutable borrow
  - Mutation while immutably borrowed

Core design principle: CONSERVATIVE -- better to reject valid code than
accept invalid code.  False positives can be refined later.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Dict, List, Optional, Set

from kryos.compiler.ast_nodes import (
    # Base types
    ASTNode,
    Span,
    NO_SPAN,
    # Type nodes
    TypeNode,
    SimpleType,
    ReferenceType,
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
    FnDecl,
    StructDecl,
    EnumDecl,
    TraitDecl,
    ImplBlock,
    ActorDecl,
    TypeAlias,
    ImportDecl,
    ModuleDecl,
    Module,
)


# ====================================================================
#  OWNERSHIP STATE
# ====================================================================

class OwnershipState(Enum):
    """Tracks the ownership state of a variable."""
    OWNED = "owned"                          # Variable owns its value
    MOVED = "moved"                          # Value has been moved to another variable
    BORROWED_IMMUTABLE = "borrowed_immutable"  # &T borrow active
    BORROWED_MUTABLE = "borrowed_mutable"      # &mut T borrow active
    DROPPED = "dropped"                      # Out of scope


# ====================================================================
#  VARIABLE INFO
# ====================================================================

@dataclass
class VarInfo:
    """Tracks ownership metadata for a single variable."""
    name: str
    state: OwnershipState
    mutable: bool
    is_copy: bool = False
    immutable_borrow_count: int = 0
    mutable_borrow_count: int = 0
    defined_at: Span = NO_SPAN
    moved_at: Optional[Span] = None
    # Track which variables are borrowing from this one
    borrowed_by: List[str] = field(default_factory=list)


# ====================================================================
#  OWNERSHIP ERROR
# ====================================================================

@dataclass
class OwnershipError:
    """A single ownership violation with location and diagnostic info."""
    message: str
    span: Span = NO_SPAN
    note: str = ""

    def __str__(self) -> str:
        line = self.span[0] if self.span != NO_SPAN else "?"
        col = self.span[1] if self.span != NO_SPAN else "?"
        result = f"ownership error at line {line}, col {col}: {self.message}"
        if self.note:
            result += f"\n  note: {self.note}"
        return result


# ====================================================================
#  COPY-TYPE DETECTION
# ====================================================================

# Types that are cheap to copy (no heap allocation)
_COPY_TYPE_NAMES: Set[str] = {
    "i8", "i16", "i32", "i64", "i128",
    "u8", "u16", "u32", "u64", "u128",
    "f32", "f64",
    "bool", "char",
    "int", "float",
}


def _is_copy_type_name(name: str) -> bool:
    """Return True if a type name denotes a Copy type."""
    return name.lower() in _COPY_TYPE_NAMES


# ====================================================================
#  OWNERSHIP ANALYZER
# ====================================================================

class OwnershipAnalyzer:
    """
    Static ownership analyzer for Kryos AST.

    Walks the AST and tracks ownership state per variable, enforcing:
      1. No use-after-move
      2. Move semantics for non-Copy types
      3. At most one &mut borrow at a time
      4. No simultaneous &T and &mut T borrows
      5. No mutation while immutably borrowed
      6. Borrows released when borrowing variable goes out of scope
    """

    def __init__(self) -> None:
        # Stack of scopes; each scope maps variable name -> VarInfo
        self.scopes: List[Dict[str, VarInfo]] = [{}]
        self.errors: List[OwnershipError] = []
        self.warnings: List[str] = []

    # ----------------------------------------------------------------
    #  Public API
    # ----------------------------------------------------------------

    def analyze(self, module: Module) -> List[OwnershipError]:
        """Analyze a module and return ownership errors."""
        for decl in module.declarations:
            self._analyze_node(decl)
        return self.errors

    # ----------------------------------------------------------------
    #  Top-level dispatch
    # ----------------------------------------------------------------

    def _analyze_node(self, node: ASTNode) -> None:
        """Dispatch to the appropriate handler based on node type."""
        if node is None:
            return

        # Declarations
        if isinstance(node, FnDecl):
            self._analyze_fn_decl(node)
        elif isinstance(node, StructDecl):
            pass  # Struct declarations don't need ownership analysis
        elif isinstance(node, EnumDecl):
            pass  # Enum declarations don't need ownership analysis
        elif isinstance(node, TraitDecl):
            pass  # Trait declarations don't need ownership analysis
        elif isinstance(node, ImplBlock):
            self._analyze_impl_block(node)
        elif isinstance(node, ImportDecl):
            pass
        elif isinstance(node, TypeAlias):
            pass

        # Statements
        elif isinstance(node, LetStmt):
            self._analyze_let(node)
        elif isinstance(node, AssignStmt):
            self._analyze_assign(node)
        elif isinstance(node, ReturnStmt):
            self._analyze_return(node)
        elif isinstance(node, IfStmt):
            self._analyze_if(node)
        elif isinstance(node, ForStmt):
            self._analyze_for(node)
        elif isinstance(node, WhileStmt):
            self._analyze_while(node)
        elif isinstance(node, BlockStmt):
            self._analyze_block(node)
        elif isinstance(node, ExprStmt):
            self._analyze_expr(node.expression)
        elif isinstance(node, BreakStmt):
            pass
        elif isinstance(node, ContinueStmt):
            pass
        elif isinstance(node, SpawnStmt):
            if node.target:
                self._analyze_expr(node.target)
            if node.body:
                self._analyze_block(node.body)
        elif isinstance(node, ParallelForStmt):
            self._analyze_parallel_for(node)

        # Expressions (top-level expression statements come through here)
        elif isinstance(node, Expression):
            self._analyze_expr(node)

    # ----------------------------------------------------------------
    #  Scope management
    # ----------------------------------------------------------------

    def _enter_scope(self) -> None:
        """Push a new scope onto the scope stack."""
        self.scopes.append({})

    def _exit_scope(self) -> None:
        """Pop the top scope and release any borrows held by its variables."""
        if not self.scopes:
            return
        scope = self.scopes.pop()
        # Release borrows: if a variable in this scope was borrowing from
        # a variable in an outer scope, decrement that outer variable's
        # borrow count.
        for var_name, var_info in scope.items():
            self._release_borrows_for(var_name)

    def _release_borrows_for(self, borrower_name: str) -> None:
        """Release any borrows that `borrower_name` holds on other variables."""
        # Walk all scopes to find variables borrowed by this one
        for scope in self.scopes:
            for var_name, var_info in scope.items():
                if borrower_name in var_info.borrowed_by:
                    var_info.borrowed_by.remove(borrower_name)
                    if var_info.immutable_borrow_count > 0:
                        var_info.immutable_borrow_count -= 1
                    if var_info.mutable_borrow_count > 0:
                        var_info.mutable_borrow_count -= 1
                    # If no more borrows, restore to OWNED state
                    if (var_info.immutable_borrow_count == 0
                            and var_info.mutable_borrow_count == 0
                            and var_info.state in (
                                OwnershipState.BORROWED_IMMUTABLE,
                                OwnershipState.BORROWED_MUTABLE)):
                        var_info.state = OwnershipState.OWNED

    # ----------------------------------------------------------------
    #  Variable tracking
    # ----------------------------------------------------------------

    def _define_var(self, name: str, mutable: bool, span: Span,
                    is_copy: bool = False) -> None:
        """Register a new variable in the current scope."""
        self.scopes[-1][name] = VarInfo(
            name=name,
            state=OwnershipState.OWNED,
            mutable=mutable,
            is_copy=is_copy,
            defined_at=span,
        )

    def _lookup_var(self, name: str) -> Optional[VarInfo]:
        """Look up a variable by walking scopes from innermost to outermost."""
        for scope in reversed(self.scopes):
            if name in scope:
                return scope[name]
        return None

    def _mark_moved(self, name: str, span: Span) -> None:
        """Mark a variable as moved at the given source location."""
        var = self._lookup_var(name)
        if var and not var.is_copy:
            var.state = OwnershipState.MOVED
            var.moved_at = span

    def _check_usable(self, name: str, span: Span) -> None:
        """Check that a variable is in a usable state; emit error if moved."""
        var = self._lookup_var(name)
        if var is None:
            return  # Unknown variable -- not our concern (let type checker handle)
        if var.state == OwnershipState.MOVED:
            moved_line = var.moved_at[0] if var.moved_at else "?"
            self.errors.append(OwnershipError(
                message=f"use of moved value '{name}'",
                span=span,
                note=f"value moved at line {moved_line}",
            ))
        elif var.state == OwnershipState.DROPPED:
            self.errors.append(OwnershipError(
                message=f"use of dropped value '{name}'",
                span=span,
                note="variable is no longer in scope",
            ))

    # ----------------------------------------------------------------
    #  Copy-type heuristics
    # ----------------------------------------------------------------

    def _expr_is_copy(self, expr: Optional[Expression]) -> bool:
        """
        Determine if an expression produces a Copy-type value.

        For the MVP, this uses heuristics:
        - IntLiteral, FloatLiteral, BoolLiteral, CharLiteral -> Copy
        - StringLiteral -> non-Copy (heap-allocated)
        - ArrayLiteral, StructLiteral, TensorLiteral -> non-Copy
        - Identifier -> check if the referenced variable is Copy
        - BinaryOp where both sides are Copy -> Copy (arithmetic result)
        - FnCall -> conservatively non-Copy
        """
        if expr is None:
            return True  # No value = no ownership concern

        if isinstance(expr, (IntLiteral, FloatLiteral, BoolLiteral, CharLiteral)):
            return True

        if isinstance(expr, NoneLiteral):
            return True

        if isinstance(expr, (StringLiteral, InterpolatedString)):
            return False  # Strings are heap-allocated

        if isinstance(expr, (ArrayLiteral, TupleLiteral, TensorLiteral,
                             StructLiteral)):
            return False  # Collection types are non-Copy

        if isinstance(expr, Identifier):
            var = self._lookup_var(expr.name)
            if var is not None:
                return var.is_copy
            return False  # Unknown variable -- conservative

        if isinstance(expr, BinaryOp):
            # Arithmetic on copy types produces copy types
            if self._expr_is_copy(expr.left) and self._expr_is_copy(expr.right):
                return True
            return False

        if isinstance(expr, UnaryOp):
            return self._expr_is_copy(expr.operand)

        if isinstance(expr, RangeExpr):
            return False  # Ranges produce iterators (non-Copy)

        if isinstance(expr, IfExpr):
            # Conservative: Copy only if both branches are Copy
            if (self._expr_is_copy(expr.then_expr)
                    and self._expr_is_copy(expr.else_expr)):
                return True
            return False

        # FnCall, MethodCall, Lambda, etc. -> conservatively non-Copy
        return False

    def _type_annotation_is_copy(self, type_ann: Optional[TypeNode]) -> bool:
        """Check if a type annotation refers to a Copy type."""
        if type_ann is None:
            return False  # No annotation -- can't tell, be conservative
        if isinstance(type_ann, SimpleType):
            return _is_copy_type_name(type_ann.name)
        if isinstance(type_ann, ReferenceType):
            return True  # References themselves are Copy (they're pointers)
        return False

    # ----------------------------------------------------------------
    #  Borrow management
    # ----------------------------------------------------------------

    def _add_immutable_borrow(self, target_name: str, borrower_name: str,
                              span: Span) -> None:
        """Record an immutable borrow of `target_name` by `borrower_name`."""
        var = self._lookup_var(target_name)
        if var is None:
            return

        # Check: can't immutably borrow if already mutably borrowed
        if var.mutable_borrow_count > 0:
            self.errors.append(OwnershipError(
                message=(f"cannot borrow '{target_name}' as immutable "
                         f"because it is already borrowed as mutable"),
                span=span,
                note=f"mutable borrow of '{target_name}' is still active",
            ))
            return

        var.immutable_borrow_count += 1
        var.borrowed_by.append(borrower_name)
        if var.state == OwnershipState.OWNED:
            var.state = OwnershipState.BORROWED_IMMUTABLE

    def _add_mutable_borrow(self, target_name: str, borrower_name: str,
                            span: Span) -> None:
        """Record a mutable borrow of `target_name` by `borrower_name`."""
        var = self._lookup_var(target_name)
        if var is None:
            return

        # Check: can't mutably borrow if already immutably borrowed
        if var.immutable_borrow_count > 0:
            self.errors.append(OwnershipError(
                message=(f"cannot borrow '{target_name}' as mutable "
                         f"because it is already borrowed as immutable"),
                span=span,
                note=(f"immutable borrow of '{target_name}' is still active "
                      f"({var.immutable_borrow_count} active borrow(s))"),
            ))
            return

        # Check: can't have more than one mutable borrow
        if var.mutable_borrow_count > 0:
            self.errors.append(OwnershipError(
                message=(f"cannot borrow '{target_name}' as mutable "
                         f"more than once at a time"),
                span=span,
                note=f"first mutable borrow of '{target_name}' is still active",
            ))
            return

        var.mutable_borrow_count += 1
        var.borrowed_by.append(borrower_name)
        var.state = OwnershipState.BORROWED_MUTABLE

    # ----------------------------------------------------------------
    #  Statement analyzers
    # ----------------------------------------------------------------

    def _analyze_let(self, stmt: LetStmt) -> None:
        """
        Analyze a let statement:
        - Track the new variable
        - Analyze the initializer expression
        - If initializer is an Identifier (move of another variable),
          mark the source as moved (unless Copy type)
        """
        # First, analyze the initializer expression for any ownership violations
        if stmt.value is not None:
            self._analyze_expr(stmt.value)

        # Determine if the new variable holds a Copy type
        is_copy = self._expr_is_copy(stmt.value)
        if stmt.type_annotation is not None:
            is_copy = self._type_annotation_is_copy(stmt.type_annotation) or is_copy

        # Register the variable
        self._define_var(
            name=stmt.name,
            mutable=stmt.mutable,
            span=stmt.span,
            is_copy=is_copy,
        )

        # Handle move semantics: if RHS is an Identifier and non-Copy, move it
        if stmt.value is not None and isinstance(stmt.value, Identifier):
            source_var = self._lookup_var(stmt.value.name)
            if source_var is not None and not source_var.is_copy:
                self._mark_moved(stmt.value.name, stmt.span)

    def _analyze_assign(self, stmt: AssignStmt) -> None:
        """
        Analyze an assignment:
        - Check target is mutable
        - If target is borrowed, error
        - If rhs is a move, mark rhs as moved
        - If target was previously moved, re-assign restores it
        """
        # Analyze the RHS first
        if stmt.value is not None:
            self._analyze_expr(stmt.value)

        # Check target
        if isinstance(stmt.target, Identifier):
            target_var = self._lookup_var(stmt.target.name)
            if target_var is not None:
                # Check mutability
                if not target_var.mutable:
                    self.errors.append(OwnershipError(
                        message=(f"cannot assign to immutable variable "
                                 f"'{stmt.target.name}'"),
                        span=stmt.span,
                        note="consider declaring with 'let mut'",
                    ))

                # Check if target is borrowed
                if target_var.immutable_borrow_count > 0:
                    self.errors.append(OwnershipError(
                        message=(f"cannot assign to '{stmt.target.name}' "
                                 f"because it is borrowed"),
                        span=stmt.span,
                        note=(f"{target_var.immutable_borrow_count} immutable "
                              f"borrow(s) still active"),
                    ))
                if target_var.mutable_borrow_count > 0:
                    self.errors.append(OwnershipError(
                        message=(f"cannot assign to '{stmt.target.name}' "
                                 f"because it is mutably borrowed"),
                        span=stmt.span,
                        note="mutable borrow is still active",
                    ))

                # Re-assignment to a moved variable restores it
                if target_var.state == OwnershipState.MOVED:
                    target_var.state = OwnershipState.OWNED
                    target_var.moved_at = None
            else:
                # Target variable not found; for now, let type checker handle
                pass

            # Handle move of RHS
            if stmt.value is not None and isinstance(stmt.value, Identifier):
                source_var = self._lookup_var(stmt.value.name)
                if source_var is not None and not source_var.is_copy:
                    self._mark_moved(stmt.value.name, stmt.span)

        elif isinstance(stmt.target, IndexAccess):
            # e.g., data[0] = 42
            # Check if the base object is borrowed immutably
            if isinstance(stmt.target.object, Identifier):
                base_var = self._lookup_var(stmt.target.object.name)
                if base_var is not None:
                    if base_var.immutable_borrow_count > 0:
                        self.errors.append(OwnershipError(
                            message=(f"cannot mutate '{stmt.target.object.name}' "
                                     f"because it is borrowed as immutable"),
                            span=stmt.span,
                            note=(f"{base_var.immutable_borrow_count} immutable "
                                  f"borrow(s) still active"),
                        ))
                    self._check_usable(stmt.target.object.name, stmt.span)
            # Analyze index expression
            if stmt.target.index is not None:
                self._analyze_expr(stmt.target.index)

        elif isinstance(stmt.target, FieldAccess):
            # e.g., point.x = 10
            if isinstance(stmt.target.object, Identifier):
                self._check_usable(stmt.target.object.name, stmt.span)

        # Analyze the target expression itself for sub-expressions
        if isinstance(stmt.target, (FieldAccess, IndexAccess, MethodCall)):
            self._analyze_expr(stmt.target)

    def _analyze_return(self, stmt: ReturnStmt) -> None:
        """Analyze a return statement. Returning moves the value out."""
        if stmt.value is not None:
            self._analyze_expr(stmt.value)
            # Returning an identifier moves it
            if isinstance(stmt.value, Identifier):
                source_var = self._lookup_var(stmt.value.name)
                if source_var is not None and not source_var.is_copy:
                    self._mark_moved(stmt.value.name, stmt.span)

    def _analyze_block(self, block: BlockStmt) -> None:
        """Analyze a block, entering and exiting a scope."""
        self._enter_scope()
        for stmt in block.statements:
            self._analyze_node(stmt)
        self._exit_scope()

    def _analyze_if(self, stmt: IfStmt) -> None:
        """
        Analyze an if/elif/else statement.
        Each branch gets its own scope.
        """
        # Analyze condition in current scope
        if stmt.condition is not None:
            self._analyze_expr(stmt.condition)

        # Analyze the if body in its own scope
        if stmt.body is not None:
            self._analyze_block(stmt.body)

        # Analyze elif clauses
        for elif_clause in stmt.elif_clauses:
            if elif_clause.condition is not None:
                self._analyze_expr(elif_clause.condition)
            if elif_clause.body is not None:
                self._analyze_block(elif_clause.body)

        # Analyze else body
        if stmt.else_body is not None:
            self._analyze_block(stmt.else_body)

    def _analyze_for(self, stmt: ForStmt) -> None:
        """
        Analyze a for loop.
        The iterable is borrowed for the duration of the loop.
        The loop variable is defined in the loop's scope.

        Key insight: the loop body can execute multiple times, so any
        move of an outer-scope variable inside the body is illegal --
        the second iteration would use a moved value.  We detect this
        by snapshotting outer scope states before analysis and checking
        which variables were moved by the body.
        """
        # Analyze the iterable expression
        if stmt.iterable is not None:
            self._analyze_expr(stmt.iterable)

        # Snapshot states of all outer-scope variables before the body
        outer_states: dict[str, OwnershipState] = {}
        for scope in self.scopes:
            for var_name, var_info in scope.items():
                outer_states[var_name] = var_info.state

        # Enter loop scope
        self._enter_scope()

        # Define the loop variable as Copy by default (iteration yields values)
        self._define_var(
            name=stmt.variable,
            mutable=False,
            span=stmt.span,
            is_copy=True,  # Loop variables are typically Copy (int indices, etc.)
        )

        # Analyze the body
        if stmt.body is not None:
            for s in stmt.body.statements:
                self._analyze_node(s)

        self._exit_scope()

        # After the body: check if any outer-scope variable was moved
        # inside the loop body.  If so, on the second iteration it would
        # be used-after-move, so emit an error now.
        for scope in self.scopes:
            for var_name, var_info in scope.items():
                if (var_name in outer_states
                        and outer_states[var_name] == OwnershipState.OWNED
                        and var_info.state == OwnershipState.MOVED
                        and not var_info.is_copy):
                    self.errors.append(OwnershipError(
                        message=(f"value '{var_name}' moved inside loop body -- "
                                 f"would be used-after-move on next iteration"),
                        span=var_info.moved_at or stmt.span,
                        note=(f"'{var_name}' is moved on each iteration; "
                              f"consider cloning or borrowing"),
                    ))

    def _analyze_while(self, stmt: WhileStmt) -> None:
        """Analyze a while loop.  Same loop-move detection as for-loops."""
        # Analyze condition
        if stmt.condition is not None:
            self._analyze_expr(stmt.condition)

        # Snapshot outer scope states
        outer_states: dict[str, OwnershipState] = {}
        for scope in self.scopes:
            for var_name, var_info in scope.items():
                outer_states[var_name] = var_info.state

        # Analyze body in its own scope
        if stmt.body is not None:
            self._analyze_block(stmt.body)

        # Check for moves of outer-scope variables inside loop body
        for scope in self.scopes:
            for var_name, var_info in scope.items():
                if (var_name in outer_states
                        and outer_states[var_name] == OwnershipState.OWNED
                        and var_info.state == OwnershipState.MOVED
                        and not var_info.is_copy):
                    self.errors.append(OwnershipError(
                        message=(f"value '{var_name}' moved inside loop body -- "
                                 f"would be used-after-move on next iteration"),
                        span=var_info.moved_at or stmt.span,
                        note=(f"'{var_name}' is moved on each iteration; "
                              f"consider cloning or borrowing"),
                    ))

    def _analyze_parallel_for(self, stmt: ParallelForStmt) -> None:
        """Analyze a parallel-for loop (similar to for)."""
        if stmt.iterable is not None:
            self._analyze_expr(stmt.iterable)

        self._enter_scope()
        self._define_var(
            name=stmt.variable,
            mutable=False,
            span=stmt.span,
            is_copy=True,
        )
        if stmt.body is not None:
            for s in stmt.body.statements:
                self._analyze_node(s)
        self._exit_scope()

    # ----------------------------------------------------------------
    #  Declaration analyzers
    # ----------------------------------------------------------------

    def _analyze_fn_decl(self, decl: FnDecl) -> None:
        """
        Analyze a function declaration:
        - Enter a new scope
        - Register parameters as owned variables
        - Analyze the body
        - Exit scope (parameters dropped)
        """
        # Register the function name in the current scope so it can be called
        self._define_var(
            name=decl.name,
            mutable=False,
            span=decl.span,
            is_copy=True,  # Function references are Copy
        )

        # Enter function scope
        self._enter_scope()

        # Register parameters
        for param in decl.params:
            is_copy = self._type_annotation_is_copy(param.type_annotation)
            # Parameters without type annotations are conservatively non-Copy
            self._define_var(
                name=param.name,
                mutable=False,
                span=param.span,
                is_copy=is_copy,
            )

        # Analyze body
        if decl.body is not None:
            for stmt in decl.body.statements:
                self._analyze_node(stmt)

        # Exit function scope
        self._exit_scope()

    def _analyze_impl_block(self, impl: ImplBlock) -> None:
        """Analyze methods in an impl block."""
        for method in impl.methods:
            self._analyze_fn_decl(method)

    # ----------------------------------------------------------------
    #  Expression analyzers
    # ----------------------------------------------------------------

    def _analyze_expr(self, expr: Optional[Expression]) -> None:
        """
        Recursively analyze an expression for ownership violations.
        This is where use-after-move is primarily caught.
        """
        if expr is None:
            return

        if isinstance(expr, Identifier):
            self._analyze_identifier(expr)

        elif isinstance(expr, (IntLiteral, FloatLiteral, StringLiteral,
                               CharLiteral, BoolLiteral, NoneLiteral)):
            pass  # Literals are always fresh values

        elif isinstance(expr, InterpolatedString):
            for part in expr.parts:
                self._analyze_expr(part)

        elif isinstance(expr, BinaryOp):
            self._analyze_expr(expr.left)
            self._analyze_expr(expr.right)

        elif isinstance(expr, UnaryOp):
            self._analyze_expr(expr.operand)

        elif isinstance(expr, FnCall):
            self._analyze_fn_call(expr)

        elif isinstance(expr, MethodCall):
            self._analyze_method_call(expr)

        elif isinstance(expr, FieldAccess):
            self._analyze_expr(expr.object)

        elif isinstance(expr, IndexAccess):
            self._analyze_expr(expr.object)
            self._analyze_expr(expr.index)

        elif isinstance(expr, ArrayLiteral):
            for elem in expr.elements:
                self._analyze_expr(elem)

        elif isinstance(expr, TupleLiteral):
            for elem in expr.elements:
                self._analyze_expr(elem)

        elif isinstance(expr, TensorLiteral):
            for elem in expr.elements:
                self._analyze_expr(elem)

        elif isinstance(expr, StructLiteral):
            for _name, value in expr.field_values:
                self._analyze_expr(value)

        elif isinstance(expr, Lambda):
            # Lambdas capture variables -- for MVP, just analyze the body
            self._enter_scope()
            for param in expr.params:
                self._define_var(
                    name=param.name,
                    mutable=False,
                    span=param.span,
                    is_copy=self._type_annotation_is_copy(param.type_annotation),
                )
            if isinstance(expr.body, BlockStmt):
                for stmt in expr.body.statements:
                    self._analyze_node(stmt)
            elif isinstance(expr.body, Expression):
                self._analyze_expr(expr.body)
            self._exit_scope()

        elif isinstance(expr, RangeExpr):
            self._analyze_expr(expr.start)
            self._analyze_expr(expr.end)

        elif isinstance(expr, IfExpr):
            self._analyze_expr(expr.condition)
            self._analyze_expr(expr.then_expr)
            self._analyze_expr(expr.else_expr)

        elif isinstance(expr, MatchExpr):
            self._analyze_expr(expr.value)
            for arm in expr.arms:
                self._analyze_expr(arm.pattern)
                self._analyze_expr(arm.guard)
                self._analyze_expr(arm.body)

        elif isinstance(expr, PipeExpr):
            self._analyze_expr(expr.left)
            self._analyze_expr(expr.right)

        elif isinstance(expr, QuantumBlock):
            for stmt in expr.body:
                self._analyze_node(stmt)

        elif isinstance(expr, ComptimeBlock):
            for stmt in expr.body:
                self._analyze_node(stmt)

    def _analyze_identifier(self, ident: Identifier) -> None:
        """
        Check that a variable reference is in a usable state.
        This is the primary detection point for use-after-move.
        """
        self._check_usable(ident.name, ident.span)

    def _analyze_fn_call(self, call: FnCall) -> None:
        """
        Analyze a function call:
        - Analyze the callee
        - For each argument:
          - If it's an Identifier and non-Copy: mark as moved (pass-by-value)
          - Otherwise, just analyze the expression
        """
        # Don't move the callee when it's a plain identifier (function name)
        if isinstance(call.callee, Identifier):
            # Just check it's usable, don't move it
            self._check_usable(call.callee.name, call.callee.span)
        else:
            self._analyze_expr(call.callee)

        # Analyze arguments
        for arg in call.args:
            self._analyze_expr(arg)
            # If argument is an Identifier and non-Copy, it's moved into the call
            if isinstance(arg, Identifier):
                var = self._lookup_var(arg.name)
                if var is not None and not var.is_copy:
                    self._mark_moved(arg.name, call.span)

    def _analyze_method_call(self, call: MethodCall) -> None:
        """Analyze a method call on an object."""
        # Analyze the receiver object
        self._analyze_expr(call.object)

        # Analyze arguments (similar to FnCall)
        for arg in call.args:
            self._analyze_expr(arg)
            if isinstance(arg, Identifier):
                var = self._lookup_var(arg.name)
                if var is not None and not var.is_copy:
                    self._mark_moved(arg.name, call.span)
