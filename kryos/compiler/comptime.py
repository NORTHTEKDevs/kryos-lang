"""
Kryos Language -- Compile-Time Evaluator

Evaluates ``comptime { ... }`` blocks at compile time using the interpreter.
Returns the resulting value as a constant that can be embedded in LLVM IR
or used directly by the interpreter.
"""

from __future__ import annotations

import struct
from typing import Any, List

from kryos.compiler.ast_nodes import (
    ASTNode,
    ComptimeBlock, Module, FnDecl, Expression,
    IntLiteral, FloatLiteral, StringLiteral, BoolLiteral, NoneLiteral,
    ArrayLiteral, Statement, ExprStmt, LetStmt, ReturnStmt,
    BlockStmt, AssignStmt, IfStmt, WhileStmt, ForStmt,
)
from kryos.compiler.interpreter import Interpreter, ReturnSignal


class ComptimeEvaluator:
    """Evaluates comptime blocks using the Kryos interpreter."""

    def __init__(self) -> None:
        self.cache: dict[int, Any] = {}  # cache by AST node id

    def evaluate(self, block: ComptimeBlock) -> Any:
        """
        Evaluate a comptime block and return the result.

        The block's body is a list of statements.  The result is:
        - The value of the last expression statement, OR
        - The return value if a ReturnStmt is used
        """
        node_id = id(block)
        if node_id in self.cache:
            return self.cache[node_id]

        # Create a fresh interpreter for isolation
        interp = Interpreter(self_healing=False)

        # Execute each statement, capture the last expression's value
        result = None
        try:
            for stmt in block.body:
                result = interp._exec(stmt, interp.globals)
        except ReturnSignal as ret:
            result = ret.value

        self.cache[node_id] = result
        return result

    def value_to_ast(self, value: Any) -> Expression:
        """
        Convert a Python runtime value back to an AST literal node.
        This is used to replace comptime blocks with their evaluated result.
        """
        if isinstance(value, bool):
            # bool check must come before int (bool is a subclass of int)
            return BoolLiteral(value=value)
        elif isinstance(value, int):
            return IntLiteral(value=value)
        elif isinstance(value, float):
            return FloatLiteral(value=value)
        elif isinstance(value, str):
            return StringLiteral(value=value)
        elif value is None:
            return NoneLiteral()
        elif isinstance(value, list):
            elements = [self.value_to_ast(item) for item in value]
            return ArrayLiteral(elements=elements)
        else:
            # For complex types, convert to string representation
            return StringLiteral(value=str(value))

    def value_to_llvm_constant(self, value: Any) -> str:
        """
        Convert a Python runtime value to an LLVM IR constant string.
        Used by the codegen to embed comptime results.
        """
        if isinstance(value, bool):
            return f"i1 {1 if value else 0}"
        elif isinstance(value, int):
            return f"i64 {value}"
        elif isinstance(value, float):
            # LLVM requires exact hex representation for doubles
            hex_val = format(
                struct.unpack('<Q', struct.pack('<d', value))[0], '016X'
            )
            return f"double 0x{hex_val}"
        elif isinstance(value, str):
            escaped = (
                value
                .replace('\\', '\\5C')
                .replace('"', '\\22')
                .replace('\n', '\\0A')
                .replace('\r', '\\0D')
                .replace('\t', '\\09')
            )
            return f'[{len(value) + 1} x i8] c"{escaped}\\00"'
        elif value is None:
            return "i64 0"
        elif isinstance(value, list):
            if not value:
                return "i64 0"
            # Placeholder -- full array constant generation is non-trivial
            return f"; comptime array of {len(value)} elements"
        else:
            return f"i64 0  ; comptime: unsupported type {type(value).__name__}"


# -----------------------------------------------------------------------
# AST Transformer -- replaces ComptimeBlock nodes with literal values
# -----------------------------------------------------------------------

class ComptimeTransformer:
    """
    Walks the AST and replaces ComptimeBlock nodes with literal values.
    This runs *before* codegen or interpretation, so downstream passes
    never see comptime blocks.
    """

    def __init__(self) -> None:
        self.evaluator = ComptimeEvaluator()
        self.errors: list[str] = []

    def transform(self, module: Module) -> Module:
        """Transform all comptime blocks in the module."""
        new_decls: list[ASTNode] = []
        for decl in module.declarations:
            new_decls.append(self._transform_node(decl))
        module.declarations = new_decls  # type: ignore[assignment]
        return module

    # -- Recursive node transformer ------------------------------------------

    def _transform_node(self, node: ASTNode) -> ASTNode:
        """Recursively transform a node, replacing comptime blocks."""
        if isinstance(node, LetStmt):
            if isinstance(node.value, ComptimeBlock):
                node.value = self._eval_comptime(node.value)
            elif node.value is not None:
                node.value = self._transform_expr(node.value)

        elif isinstance(node, FnDecl):
            if node.body is not None:
                node.body.statements = [
                    self._transform_node(s) for s in node.body.statements  # type: ignore[misc]
                ]

        elif isinstance(node, ExprStmt):
            if node.expression is not None:
                node.expression = self._transform_expr(node.expression)

        elif isinstance(node, ReturnStmt):
            if node.value is not None:
                node.value = self._transform_expr(node.value)

        elif isinstance(node, BlockStmt):
            node.statements = [
                self._transform_node(s) for s in node.statements  # type: ignore[misc]
            ]

        elif isinstance(node, AssignStmt):
            if node.value is not None:
                node.value = self._transform_expr(node.value)

        elif isinstance(node, IfStmt):
            if node.condition is not None:
                node.condition = self._transform_expr(node.condition)
            if node.body is not None:
                self._transform_node(node.body)
            for elif_clause in node.elif_clauses:
                if elif_clause.condition is not None:
                    elif_clause.condition = self._transform_expr(elif_clause.condition)
                if elif_clause.body is not None:
                    self._transform_node(elif_clause.body)
            if node.else_body is not None:
                self._transform_node(node.else_body)

        elif isinstance(node, WhileStmt):
            if node.condition is not None:
                node.condition = self._transform_expr(node.condition)
            if node.body is not None:
                self._transform_node(node.body)

        elif isinstance(node, ForStmt):
            if node.iterable is not None:
                node.iterable = self._transform_expr(node.iterable)
            if node.body is not None:
                self._transform_node(node.body)

        return node

    def _transform_expr(self, expr: Expression) -> Expression:
        """Transform an expression, evaluating comptime blocks inline."""
        if isinstance(expr, ComptimeBlock):
            return self._eval_comptime(expr)
        # For other expression types, we could recurse into sub-expressions
        # (e.g. BinaryOp operands), but comptime blocks normally appear as
        # standalone expression values, not nested inside operators.
        return expr

    def _eval_comptime(self, block: ComptimeBlock) -> Expression:
        """Evaluate a comptime block and return the AST literal."""
        try:
            result = self.evaluator.evaluate(block)
            return self.evaluator.value_to_ast(result)
        except Exception as e:
            self.errors.append(f"comptime evaluation failed: {e}")
            # Return a safe fallback -- NoneLiteral
            return NoneLiteral()
