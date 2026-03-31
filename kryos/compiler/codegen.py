"""
Kryos Language -- LLVM IR Code Generation

Generates LLVM IR text from the Kryos AST.  The output is plain text that
can be fed to ``llc`` and ``clang`` to produce a native binary.

No external bindings (llvmlite, etc.) are required.

Usage:
    from kryos.compiler.codegen import CodeGenerator, compile_and_run
    gen = CodeGenerator()
    ir = gen.generate(module)        # module: ast_nodes.Module
    ok, msg = compile_and_run(ir)
"""

from __future__ import annotations

import os
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from kryos.compiler.ast_nodes import (
    Module,
    # Declarations
    FnDecl, StructDecl, StructField, EnumDecl, TraitDecl, ImplBlock,
    # Statements
    Statement, BlockStmt, LetStmt, AssignStmt, ReturnStmt,
    IfStmt, ElifClause, ForStmt, WhileStmt, BreakStmt, ContinueStmt,
    ExprStmt,
    # Expressions
    Expression, IntLiteral, FloatLiteral, StringLiteral, CharLiteral,
    BoolLiteral, NoneLiteral, Identifier,
    BinaryOp, UnaryOp, FnCall, MethodCall, FieldAccess, IndexAccess,
    ArrayLiteral, StructLiteral, Lambda, Parameter,
    RangeExpr, IfExpr, PipeExpr,
    # Types
    TypeNode, SimpleType, GenericType, ArrayType, FnType,
    Attribute,
)


# ---------------------------------------------------------------------------
# Type mapping helpers
# ---------------------------------------------------------------------------

_KRYOS_TO_LLVM: dict[str, str] = {
    "i8":   "i8",
    "i16":  "i16",
    "i32":  "i32",
    "i64":  "i64",
    "i128": "i128",
    "u8":   "i8",
    "u16":  "i16",
    "u32":  "i32",
    "u64":  "i64",
    "u128": "i128",
    "f32":  "float",
    "f64":  "double",
    "bool": "i1",
    "str":  "i8*",
    "char": "i8",
    "void": "void",
}

_INT_TYPES = frozenset({"i8", "i16", "i32", "i64", "i128",
                         "u8", "u16", "u32", "u64", "u128"})
_FLOAT_TYPES = frozenset({"f32", "f64"})


def _llvm_type(ty: Optional[TypeNode]) -> str:
    """Convert a Kryos TypeNode to an LLVM IR type string."""
    if ty is None:
        return "void"
    if isinstance(ty, SimpleType):
        return _KRYOS_TO_LLVM.get(ty.name, "i32")
    if isinstance(ty, GenericType):
        # Fallback -- treat unknown generics as i8*
        return "i8*"
    if isinstance(ty, ArrayType):
        elem = _llvm_type(ty.element_type)
        return f"{elem}*"
    return "i32"  # conservative fallback


def _kryos_type_name(ty: Optional[TypeNode]) -> str:
    """Return the Kryos source-level type name (for dispatch decisions)."""
    if ty is None:
        return "void"
    if isinstance(ty, SimpleType):
        return ty.name
    if isinstance(ty, GenericType):
        return ty.name
    return "i32"


def _is_float_type(ty: Optional[TypeNode]) -> bool:
    return _kryos_type_name(ty) in _FLOAT_TYPES


def _is_int_type(ty: Optional[TypeNode]) -> bool:
    name = _kryos_type_name(ty)
    return name in _INT_TYPES or name == "bool"


# ---------------------------------------------------------------------------
# Code Generator
# ---------------------------------------------------------------------------

class CodeGenerator:
    """Generates LLVM IR text from a Kryos AST."""

    def __init__(self) -> None:
        self._reg_counter: int = 0
        self._label_counter: int = 0
        self._globals: list[str] = []
        self._string_counter: int = 0
        self._string_cache: dict[str, str] = {}
        self._env: dict[str, tuple[str, str]] = {}   # name -> (ptr_reg, llvm_type)
        self._functions: dict[str, FnDecl] = {}
        self._struct_types: dict[str, StructDecl] = {}
        self._declared_externs: set[str] = set()

    # -- helpers --

    def _next_reg(self) -> str:
        r = f"%{self._reg_counter}"
        self._reg_counter += 1
        return r

    def _next_label(self, hint: str = "L") -> str:
        self._label_counter += 1
        return f"{hint}{self._label_counter}"

    def _add_string_constant(self, value: str) -> str:
        """Register a string literal as a global constant and return the global name."""
        if value in self._string_cache:
            return self._string_cache[value]
        name = f"@.str.{self._string_counter}"
        self._string_counter += 1
        # Escape the string for LLVM IR
        escaped = value.replace("\\", "\\5C").replace('"', '\\22').replace("\n", "\\0A").replace("\t", "\\09")
        length = len(value) + 1  # +1 for null terminator
        self._globals.append(
            f'{name} = private unnamed_addr constant [{length} x i8] c"{escaped}\\00"'
        )
        self._string_cache[value] = name
        return name

    def _ensure_printf_decl(self) -> None:
        if "printf" not in self._declared_externs:
            self._globals.append("declare i32 @printf(i8*, ...)")
            self._declared_externs.add("printf")

    def _ensure_puts_decl(self) -> None:
        if "puts" not in self._declared_externs:
            self._globals.append("declare i32 @puts(i8*)")
            self._declared_externs.add("puts")

    # -----------------------------------------------------------------------
    # Public API
    # -----------------------------------------------------------------------

    def generate(self, module: Module) -> str:
        """Generate a complete LLVM IR module as a string."""
        self._reg_counter = 0
        self._label_counter = 0
        self._globals = []
        self._string_counter = 0
        self._string_cache = {}
        self._env = {}
        self._functions = {}
        self._struct_types = {}
        self._declared_externs = set()

        fn_irs: list[str] = []
        top_level_stmts: list[Statement] = []

        # First pass -- collect declarations
        for decl in module.declarations:
            if isinstance(decl, FnDecl):
                self._functions[decl.name] = decl
            elif isinstance(decl, StructDecl):
                self._struct_types[decl.name] = decl

        # Second pass -- generate code
        for decl in module.declarations:
            if isinstance(decl, FnDecl):
                fn_irs.append(self._gen_function(decl))
            elif isinstance(decl, StructDecl):
                self._globals.append(self._gen_struct_type(decl))
            elif isinstance(decl, (LetStmt, ExprStmt, AssignStmt, IfStmt,
                                   WhileStmt, ForStmt, ReturnStmt)):
                top_level_stmts.append(decl)

        # Wrap top-level statements in a main function
        main_ir = ""
        if top_level_stmts:
            main_ir = self._gen_main(top_level_stmts)

        # Assemble the module
        lines = [
            '; Kryos LLVM IR -- generated by the Kryos compiler',
            f'; Module: {module.name or "<main>"}',
            f'target triple = "x86_64-pc-linux-gnu"',
            '',
        ]
        lines.extend(self._globals)
        if self._globals:
            lines.append('')
        lines.extend(fn_irs)
        if main_ir:
            lines.append(main_ir)

        return "\n".join(lines) + "\n"

    # -----------------------------------------------------------------------
    # Struct types
    # -----------------------------------------------------------------------

    def _gen_struct_type(self, decl: StructDecl) -> str:
        field_types = [_llvm_type(f.type_annotation) for f in decl.fields]
        return f'%struct.{decl.name} = type {{ {", ".join(field_types)} }}'

    # -----------------------------------------------------------------------
    # Functions
    # -----------------------------------------------------------------------

    def _gen_function(self, fn: FnDecl) -> str:
        """Generate LLVM IR for a function declaration."""
        self._reg_counter = 0
        self._env = {}

        ret_type = _llvm_type(fn.return_type)
        params = []
        for p in fn.params:
            llty = _llvm_type(p.type_annotation)
            params.append(f"{llty} %{p.name}")

        param_str = ", ".join(params)
        lines = [f"define {ret_type} @{fn.name}({param_str}) {{"]
        lines.append("entry:")

        # Allocate and store parameters
        body_lines: list[str] = []
        for p in fn.params:
            llty = _llvm_type(p.type_annotation)
            ptr = self._next_reg()
            body_lines.append(f"  {ptr} = alloca {llty}")
            body_lines.append(f"  store {llty} %{p.name}, {llty}* {ptr}")
            self._env[p.name] = (ptr, llty)

        # Generate body
        if fn.body:
            for stmt in fn.body.statements:
                body_lines.extend(self._gen_stmt(stmt, fn.return_type))

        # Ensure a terminator
        if not body_lines or not self._is_terminator(body_lines[-1]):
            if ret_type == "void":
                body_lines.append("  ret void")
            elif ret_type in ("i1", "i8", "i16", "i32", "i64", "i128"):
                body_lines.append(f"  ret {ret_type} 0")
            elif ret_type in ("float", "double"):
                body_lines.append(f"  ret {ret_type} 0.0")
            else:
                body_lines.append(f"  ret {ret_type} zeroinitializer")

        lines.extend(body_lines)
        lines.append("}")
        lines.append("")
        return "\n".join(lines)

    def _gen_main(self, stmts: list[Statement]) -> str:
        """Wrap top-level statements in a main() function."""
        self._reg_counter = 0
        self._env = {}

        lines = ["define i32 @main() {"]
        lines.append("entry:")

        body: list[str] = []
        for stmt in stmts:
            body.extend(self._gen_stmt(stmt, SimpleType(name="i32")))

        if not body or not self._is_terminator(body[-1]):
            body.append("  ret i32 0")

        lines.extend(body)
        lines.append("}")
        lines.append("")
        return "\n".join(lines)

    @staticmethod
    def _is_terminator(line: str) -> bool:
        stripped = line.strip()
        return (stripped.startswith("ret ") or stripped.startswith("br ") or
                stripped == "unreachable")

    # -----------------------------------------------------------------------
    # Statements
    # -----------------------------------------------------------------------

    def _gen_stmt(self, stmt: Statement, fn_ret: Optional[TypeNode] = None) -> list[str]:
        """Generate IR lines for a statement."""
        if isinstance(stmt, LetStmt):
            return self._gen_let(stmt)
        elif isinstance(stmt, AssignStmt):
            return self._gen_assign(stmt)
        elif isinstance(stmt, ReturnStmt):
            return self._gen_return(stmt, fn_ret)
        elif isinstance(stmt, IfStmt):
            return self._gen_if(stmt, fn_ret)
        elif isinstance(stmt, WhileStmt):
            return self._gen_while(stmt, fn_ret)
        elif isinstance(stmt, ForStmt):
            return self._gen_for(stmt, fn_ret)
        elif isinstance(stmt, ExprStmt):
            if stmt.expression is not None:
                ir, _ = self._gen_expr(stmt.expression)
                return ir
            return []
        elif isinstance(stmt, BlockStmt):
            lines: list[str] = []
            for s in stmt.statements:
                lines.extend(self._gen_stmt(s, fn_ret))
            return lines
        elif isinstance(stmt, BreakStmt):
            # Break is handled by while loop codegen -- emit a branch placeholder
            return [f"  br label %while.end  ; break"]
        elif isinstance(stmt, ContinueStmt):
            return [f"  br label %while.cond  ; continue"]
        return [f"  ; TODO: unhandled statement {type(stmt).__name__}"]

    # -- let ----------------------------------------------------------------

    def _gen_let(self, stmt: LetStmt) -> list[str]:
        lines: list[str] = []

        # Determine type
        if stmt.type_annotation:
            llty = _llvm_type(stmt.type_annotation)
        elif stmt.value is not None:
            llty = self._infer_llvm_type(stmt.value)
        else:
            llty = "i32"

        ptr = self._next_reg()
        lines.append(f"  {ptr} = alloca {llty}")

        if stmt.value is not None:
            val_lines, val_reg = self._gen_expr(stmt.value)
            lines.extend(val_lines)
            lines.append(f"  store {llty} {val_reg}, {llty}* {ptr}")

        self._env[stmt.name] = (ptr, llty)
        return lines

    # -- assign -------------------------------------------------------------

    def _gen_assign(self, stmt: AssignStmt) -> list[str]:
        lines: list[str] = []
        if isinstance(stmt.target, Identifier):
            name = stmt.target.name
            if name not in self._env:
                return [f"  ; ERROR: undefined variable {name}"]
            ptr, llty = self._env[name]

            val_lines, val_reg = self._gen_expr(stmt.value)
            lines.extend(val_lines)

            if stmt.operator != "=":
                # Compound assignment: load current, apply op, store
                cur = self._next_reg()
                lines.append(f"  {cur} = load {llty}, {llty}* {ptr}")
                op_map = {"+=": "add", "-=": "sub", "*=": "mul", "/=": "sdiv"}
                if llty in ("float", "double"):
                    op_map = {"+=": "fadd", "-=": "fsub", "*=": "fmul", "/=": "fdiv"}
                op_inst = op_map.get(stmt.operator, "add")
                result = self._next_reg()
                lines.append(f"  {result} = {op_inst} {llty} {cur}, {val_reg}")
                val_reg = result

            lines.append(f"  store {llty} {val_reg}, {llty}* {ptr}")
        return lines

    # -- return -------------------------------------------------------------

    def _gen_return(self, stmt: ReturnStmt, fn_ret: Optional[TypeNode]) -> list[str]:
        lines: list[str] = []
        if stmt.value is None:
            lines.append("  ret void")
        else:
            val_lines, val_reg = self._gen_expr(stmt.value)
            lines.extend(val_lines)
            ret_llty = _llvm_type(fn_ret)
            lines.append(f"  ret {ret_llty} {val_reg}")
        return lines

    # -- if/else ------------------------------------------------------------

    def _gen_if(self, stmt: IfStmt, fn_ret: Optional[TypeNode]) -> list[str]:
        lines: list[str] = []
        label_then = self._next_label("if.then")
        label_else = self._next_label("if.else")
        label_end = self._next_label("if.end")

        # Condition
        cond_lines, cond_reg = self._gen_expr(stmt.condition)
        lines.extend(cond_lines)

        has_else = stmt.else_body is not None or stmt.elif_clauses
        target_else = label_else if has_else else label_end
        lines.append(f"  br i1 {cond_reg}, label %{label_then}, label %{target_else}")

        # Then block
        lines.append(f"{label_then}:")
        if stmt.body:
            for s in stmt.body.statements:
                lines.extend(self._gen_stmt(s, fn_ret))
        if not lines or not self._is_terminator(lines[-1]):
            lines.append(f"  br label %{label_end}")

        # Elif clauses
        if stmt.elif_clauses:
            for i, clause in enumerate(stmt.elif_clauses):
                elif_label = label_else
                label_else = self._next_label("elif.else") if i < len(stmt.elif_clauses) - 1 or stmt.else_body else label_end
                lines.append(f"{elif_label}:")
                ec_lines, ec_reg = self._gen_expr(clause.condition)
                lines.extend(ec_lines)
                elif_then = self._next_label("elif.then")
                lines.append(f"  br i1 {ec_reg}, label %{elif_then}, label %{label_else}")
                lines.append(f"{elif_then}:")
                if clause.body:
                    for s in clause.body.statements:
                        lines.extend(self._gen_stmt(s, fn_ret))
                if not self._is_terminator(lines[-1]):
                    lines.append(f"  br label %{label_end}")
            if stmt.else_body:
                lines.append(f"{label_else}:")
                for s in stmt.else_body.statements:
                    lines.extend(self._gen_stmt(s, fn_ret))
                if not self._is_terminator(lines[-1]):
                    lines.append(f"  br label %{label_end}")
        elif stmt.else_body:
            lines.append(f"{label_else}:")
            for s in stmt.else_body.statements:
                lines.extend(self._gen_stmt(s, fn_ret))
            if not self._is_terminator(lines[-1]):
                lines.append(f"  br label %{label_end}")

        lines.append(f"{label_end}:")
        return lines

    # -- while loop ---------------------------------------------------------

    def _gen_while(self, stmt: WhileStmt, fn_ret: Optional[TypeNode]) -> list[str]:
        lines: list[str] = []
        label_cond = self._next_label("while.cond")
        label_body = self._next_label("while.body")
        label_end = self._next_label("while.end")

        lines.append(f"  br label %{label_cond}")
        lines.append(f"{label_cond}:")

        cond_lines, cond_reg = self._gen_expr(stmt.condition)
        lines.extend(cond_lines)
        lines.append(f"  br i1 {cond_reg}, label %{label_body}, label %{label_end}")

        lines.append(f"{label_body}:")
        if stmt.body:
            for s in stmt.body.statements:
                lines.extend(self._gen_stmt(s, fn_ret))
        if not lines or not self._is_terminator(lines[-1]):
            lines.append(f"  br label %{label_cond}")

        lines.append(f"{label_end}:")
        return lines

    # -- for loop (basic range iteration) -----------------------------------

    def _gen_for(self, stmt: ForStmt, fn_ret: Optional[TypeNode]) -> list[str]:
        # For now, generate as a while loop with an index variable
        lines: list[str] = []
        lines.append(f"  ; for {stmt.variable} in ...")
        # Simplified: just generate body (full range iteration is complex)
        if stmt.body:
            for s in stmt.body.statements:
                lines.extend(self._gen_stmt(s, fn_ret))
        return lines

    # -----------------------------------------------------------------------
    # Expressions
    # -----------------------------------------------------------------------

    def _gen_expr(self, expr: Optional[Expression]) -> tuple[list[str], str]:
        """Generate IR for an expression. Returns (ir_lines, result_register)."""
        if expr is None:
            return [], "0"

        if isinstance(expr, IntLiteral):
            return [], str(expr.value)

        if isinstance(expr, FloatLiteral):
            # Format as LLVM double hex or decimal
            return [], f"{expr.value:#.6e}" if expr.value != 0 else "0.0"

        if isinstance(expr, BoolLiteral):
            return [], "1" if expr.value else "0"

        if isinstance(expr, StringLiteral):
            gname = self._add_string_constant(expr.value)
            reg = self._next_reg()
            length = len(expr.value) + 1
            lines = [f"  {reg} = getelementptr [{length} x i8], [{length} x i8]* {gname}, i32 0, i32 0"]
            return lines, reg

        if isinstance(expr, NoneLiteral):
            return [], "null"

        if isinstance(expr, CharLiteral):
            return [], str(ord(expr.value[0]) if expr.value else 0)

        if isinstance(expr, Identifier):
            return self._gen_identifier(expr)

        if isinstance(expr, BinaryOp):
            return self._gen_binary_op(expr)

        if isinstance(expr, UnaryOp):
            return self._gen_unary_op(expr)

        if isinstance(expr, FnCall):
            return self._gen_call(expr)

        if isinstance(expr, FieldAccess):
            return self._gen_field_access(expr)

        if isinstance(expr, IndexAccess):
            return self._gen_index_access(expr)

        if isinstance(expr, IfExpr):
            return self._gen_if_expr(expr)

        if isinstance(expr, ArrayLiteral):
            # Simplified: return first element or 0
            if expr.elements:
                return self._gen_expr(expr.elements[0])
            return [], "0"

        if isinstance(expr, StructLiteral):
            return self._gen_struct_literal(expr)

        # Fallback
        return [f"  ; TODO: unhandled expression {type(expr).__name__}"], "0"

    # -- identifier ---------------------------------------------------------

    def _gen_identifier(self, expr: Identifier) -> tuple[list[str], str]:
        name = expr.name
        if name in self._env:
            ptr, llty = self._env[name]
            reg = self._next_reg()
            return [f"  {reg} = load {llty}, {llty}* {ptr}"], reg
        # Could be a function reference or global
        return [], f"@{name}"

    # -- binary op ----------------------------------------------------------

    def _gen_binary_op(self, expr: BinaryOp) -> tuple[list[str], str]:
        lines: list[str] = []
        l_lines, l_reg = self._gen_expr(expr.left)
        r_lines, r_reg = self._gen_expr(expr.right)
        lines.extend(l_lines)
        lines.extend(r_lines)

        # Determine if float or int
        is_float = self._expr_is_float(expr.left) or self._expr_is_float(expr.right)
        result = self._next_reg()

        op = expr.operator

        if op in ("+", "-", "*", "/", "%"):
            if is_float:
                inst_map = {"+": "fadd", "-": "fsub", "*": "fmul", "/": "fdiv", "%": "frem"}
                ty = "double"
            else:
                inst_map = {"+": "add", "-": "sub", "*": "mul", "/": "sdiv", "%": "srem"}
                ty = "i32"
            lines.append(f"  {result} = {inst_map[op]} {ty} {l_reg}, {r_reg}")
            return lines, result

        if op in ("==", "!=", "<", ">", "<=", ">="):
            if is_float:
                cmp_map = {"==": "oeq", "!=": "one", "<": "olt",
                           ">": "ogt", "<=": "ole", ">=": "oge"}
                lines.append(f"  {result} = fcmp {cmp_map[op]} double {l_reg}, {r_reg}")
            else:
                cmp_map = {"==": "eq", "!=": "ne", "<": "slt",
                           ">": "sgt", "<=": "sle", ">=": "sge"}
                lines.append(f"  {result} = icmp {cmp_map[op]} i32 {l_reg}, {r_reg}")
            return lines, result

        if op == "and":
            lines.append(f"  {result} = and i1 {l_reg}, {r_reg}")
            return lines, result

        if op == "or":
            lines.append(f"  {result} = or i1 {l_reg}, {r_reg}")
            return lines, result

        # String concatenation via + is left as a TODO (needs runtime support)
        lines.append(f"  ; TODO: operator '{op}'")
        return lines, l_reg

    # -- unary op -----------------------------------------------------------

    def _gen_unary_op(self, expr: UnaryOp) -> tuple[list[str], str]:
        lines: list[str] = []
        op_lines, op_reg = self._gen_expr(expr.operand)
        lines.extend(op_lines)
        result = self._next_reg()

        if expr.operator == "-":
            if self._expr_is_float(expr.operand):
                lines.append(f"  {result} = fneg double {op_reg}")
            else:
                lines.append(f"  {result} = sub i32 0, {op_reg}")
            return lines, result

        if expr.operator in ("!", "not"):
            lines.append(f"  {result} = xor i1 {op_reg}, 1")
            return lines, result

        if expr.operator == "~":
            lines.append(f"  {result} = xor i32 {op_reg}, -1")
            return lines, result

        return lines, op_reg

    # -- function call ------------------------------------------------------

    def _gen_call(self, expr: FnCall) -> tuple[list[str], str]:
        lines: list[str] = []

        # Determine callee name
        callee_name = ""
        if isinstance(expr.callee, Identifier):
            callee_name = expr.callee.name
        elif isinstance(expr.callee, FieldAccess):
            callee_name = f"{expr.callee.field}"

        # Handle println specially
        if callee_name == "println":
            return self._gen_println(expr)

        if callee_name == "print":
            return self._gen_print(expr)

        # Generate argument values
        arg_regs: list[tuple[str, str]] = []  # (llvm_type, register)
        for arg in expr.args:
            arg_lines, arg_reg = self._gen_expr(arg)
            lines.extend(arg_lines)
            arg_ty = self._infer_llvm_type(arg)
            arg_regs.append((arg_ty, arg_reg))

        # Determine return type
        fn_decl = self._functions.get(callee_name)
        if fn_decl:
            ret_type = _llvm_type(fn_decl.return_type)
        else:
            ret_type = "i32"  # default

        args_str = ", ".join(f"{ty} {reg}" for ty, reg in arg_regs)

        if ret_type == "void":
            lines.append(f"  call void @{callee_name}({args_str})")
            return lines, "0"
        else:
            result = self._next_reg()
            lines.append(f"  {result} = call {ret_type} @{callee_name}({args_str})")
            return lines, result

    # -- println / print ----------------------------------------------------

    def _gen_println(self, expr: FnCall) -> tuple[list[str], str]:
        lines: list[str] = []
        self._ensure_puts_decl()

        if not expr.args:
            # Empty println -- print newline
            gname = self._add_string_constant("")
            reg = self._next_reg()
            lines.append(f"  {reg} = getelementptr [1 x i8], [1 x i8]* {gname}, i32 0, i32 0")
            result = self._next_reg()
            lines.append(f"  {result} = call i32 @puts(i8* {reg})")
            return lines, result

        # For single string argument, use puts
        arg = expr.args[0]
        arg_lines, arg_reg = self._gen_expr(arg)
        lines.extend(arg_lines)

        if isinstance(arg, StringLiteral):
            result = self._next_reg()
            lines.append(f"  {result} = call i32 @puts(i8* {arg_reg})")
            return lines, result

        # For integer arguments, use printf with %d\n
        self._ensure_printf_decl()
        if self._expr_is_float(arg):
            fmt_name = self._add_string_constant("%f\\0A")
            fmt_reg = self._next_reg()
            fmt_len = 4  # %f\n\0
            lines.append(f"  {fmt_reg} = getelementptr [{fmt_len} x i8], [{fmt_len} x i8]* {fmt_name}, i32 0, i32 0")
            result = self._next_reg()
            lines.append(f"  {result} = call i32 (i8*, ...) @printf(i8* {fmt_reg}, double {arg_reg})")
        else:
            fmt_name = self._add_string_constant("%d\\0A")
            fmt_reg = self._next_reg()
            fmt_len = 4  # %d\n\0
            lines.append(f"  {fmt_reg} = getelementptr [{fmt_len} x i8], [{fmt_len} x i8]* {fmt_name}, i32 0, i32 0")
            result = self._next_reg()
            lines.append(f"  {result} = call i32 (i8*, ...) @printf(i8* {fmt_reg}, i32 {arg_reg})")

        return lines, result

    def _gen_print(self, expr: FnCall) -> tuple[list[str], str]:
        self._ensure_printf_decl()
        lines: list[str] = []
        if not expr.args:
            return lines, "0"

        arg = expr.args[0]
        arg_lines, arg_reg = self._gen_expr(arg)
        lines.extend(arg_lines)

        if isinstance(arg, StringLiteral):
            fmt_name = self._add_string_constant("%s")
            fmt_reg = self._next_reg()
            lines.append(f"  {fmt_reg} = getelementptr [3 x i8], [3 x i8]* {fmt_name}, i32 0, i32 0")
            result = self._next_reg()
            lines.append(f"  {result} = call i32 (i8*, ...) @printf(i8* {fmt_reg}, i8* {arg_reg})")
            return lines, result

        return lines, "0"

    # -- field access -------------------------------------------------------

    def _gen_field_access(self, expr: FieldAccess) -> tuple[list[str], str]:
        lines: list[str] = []
        lines.append(f"  ; TODO: field access .{expr.field}")
        return lines, "0"

    # -- index access -------------------------------------------------------

    def _gen_index_access(self, expr: IndexAccess) -> tuple[list[str], str]:
        lines: list[str] = []
        lines.append(f"  ; TODO: index access")
        return lines, "0"

    # -- if expression ------------------------------------------------------

    def _gen_if_expr(self, expr: IfExpr) -> tuple[list[str], str]:
        lines: list[str] = []
        cond_lines, cond_reg = self._gen_expr(expr.condition)
        lines.extend(cond_lines)

        then_lines, then_reg = self._gen_expr(expr.then_expr)
        else_lines, else_reg = self._gen_expr(expr.else_expr)

        result = self._next_reg()
        # Use select for simple cases
        lines.extend(then_lines)
        lines.extend(else_lines)
        lines.append(f"  {result} = select i1 {cond_reg}, i32 {then_reg}, i32 {else_reg}")
        return lines, result

    # -- struct literal -----------------------------------------------------

    def _gen_struct_literal(self, expr: StructLiteral) -> tuple[list[str], str]:
        lines: list[str] = []
        sname = expr.type_name
        if sname in self._struct_types:
            ptr = self._next_reg()
            lines.append(f"  {ptr} = alloca %struct.{sname}")
            for i, (fname, fval) in enumerate(expr.field_values):
                flines, freg = self._gen_expr(fval)
                lines.extend(flines)
                fptr = self._next_reg()
                field_ty = "i32"  # simplified
                decl = self._struct_types[sname]
                if i < len(decl.fields):
                    field_ty = _llvm_type(decl.fields[i].type_annotation)
                lines.append(f"  {fptr} = getelementptr %struct.{sname}, %struct.{sname}* {ptr}, i32 0, i32 {i}")
                lines.append(f"  store {field_ty} {freg}, {field_ty}* {fptr}")
            return lines, ptr
        return [f"  ; TODO: struct literal {sname}"], "0"

    # -----------------------------------------------------------------------
    # Type inference helpers
    # -----------------------------------------------------------------------

    def _infer_llvm_type(self, expr: Optional[Expression]) -> str:
        """Infer the LLVM type of an expression (best effort)."""
        if expr is None:
            return "i32"
        if isinstance(expr, IntLiteral):
            return "i32"
        if isinstance(expr, FloatLiteral):
            return "double"
        if isinstance(expr, BoolLiteral):
            return "i1"
        if isinstance(expr, StringLiteral):
            return "i8*"
        if isinstance(expr, CharLiteral):
            return "i8"
        if isinstance(expr, NoneLiteral):
            return "i8*"
        if isinstance(expr, BinaryOp):
            if expr.operator in ("==", "!=", "<", ">", "<=", ">=", "and", "or"):
                return "i1"
            if self._expr_is_float(expr.left) or self._expr_is_float(expr.right):
                return "double"
            return "i32"
        if isinstance(expr, UnaryOp):
            if expr.operator in ("!", "not"):
                return "i1"
            return self._infer_llvm_type(expr.operand)
        if isinstance(expr, Identifier):
            if expr.name in self._env:
                return self._env[expr.name][1]
            return "i32"
        if isinstance(expr, FnCall):
            if isinstance(expr.callee, Identifier):
                fn = self._functions.get(expr.callee.name)
                if fn:
                    return _llvm_type(fn.return_type)
            return "i32"
        return "i32"

    def _expr_is_float(self, expr: Optional[Expression]) -> bool:
        """Check if an expression produces a floating point value."""
        if expr is None:
            return False
        if isinstance(expr, FloatLiteral):
            return True
        if isinstance(expr, BinaryOp):
            return self._expr_is_float(expr.left) or self._expr_is_float(expr.right)
        if isinstance(expr, UnaryOp):
            return self._expr_is_float(expr.operand)
        if isinstance(expr, Identifier):
            if expr.name in self._env:
                return self._env[expr.name][1] in ("float", "double")
        if isinstance(expr, FnCall):
            if isinstance(expr.callee, Identifier):
                fn = self._functions.get(expr.callee.name)
                if fn:
                    return _is_float_type(fn.return_type)
        return False


# ---------------------------------------------------------------------------
# Compile & Run helper
# ---------------------------------------------------------------------------

def compile_and_run(ir: str, output_path: str = "a.out") -> tuple[bool, str]:
    """
    Try to compile LLVM IR to a native binary using ``llc`` + ``clang``.

    Returns ``(success, message)``.  If the toolchain is not available,
    returns ``(False, ...)`` with a descriptive message.
    """
    with tempfile.TemporaryDirectory() as tmpdir:
        ll_path = os.path.join(tmpdir, "input.ll")
        obj_path = os.path.join(tmpdir, "input.o")

        # Write IR
        with open(ll_path, "w", encoding="utf-8") as f:
            f.write(ir)

        # Step 1: llc  (IR -> object file)
        try:
            result = subprocess.run(
                ["llc", "-filetype=obj", ll_path, "-o", obj_path],
                capture_output=True, text=True, timeout=60,
            )
            if result.returncode != 0:
                return False, f"llc failed:\n{result.stderr}"
        except FileNotFoundError:
            return False, "llc not found -- install LLVM to compile to native code"
        except subprocess.TimeoutExpired:
            return False, "llc timed out"

        # Step 2: clang  (object -> executable)
        try:
            result = subprocess.run(
                ["clang", obj_path, "-o", output_path],
                capture_output=True, text=True, timeout=60,
            )
            if result.returncode != 0:
                return False, f"clang failed:\n{result.stderr}"
        except FileNotFoundError:
            return False, "clang not found -- install LLVM/Clang to link the binary"
        except subprocess.TimeoutExpired:
            return False, "clang timed out"

    return True, f"Compiled successfully: {output_path}"
