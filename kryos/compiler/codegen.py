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
import platform
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from kryos.compiler.ast_nodes import (
    Module,
    # Declarations
    FnDecl, StructDecl, StructField, EnumDecl, EnumVariant, TraitDecl, ImplBlock,
    GenericParam,
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
    MatchExpr, MatchArm,
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
        safe = elem.replace("*", "ptr").replace(" ", "_")
        return f"%Array_{safe}*"
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
        # Extended codegen state
        self._lambda_counter: int = 0
        self._enum_types: dict[str, EnumDecl] = {}
        self._enum_variant_tags: dict[str, int] = {}  # "EnumName::Variant" -> tag
        self._impl_methods: dict[str, FnDecl] = {}    # "Type_method" -> FnDecl
        self._trait_decls: dict[str, TraitDecl] = {}
        self._generated_functions: set[str] = set()
        self._emitted_array_types: set[str] = set()  # e.g. {"i32", "double"}

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

    def _ensure_malloc_decl(self) -> None:
        if "malloc" not in self._declared_externs:
            self._globals.append("declare i8* @malloc(i64)")
            self._declared_externs.add("malloc")

    def _ensure_free_decl(self) -> None:
        if "free" not in self._declared_externs:
            self._globals.append("declare void @free(i8*)")
            self._declared_externs.add("free")

    def _ensure_strlen_decl(self) -> None:
        if "strlen" not in self._declared_externs:
            self._globals.append("declare i64 @strlen(i8*)")
            self._declared_externs.add("strlen")

    def _ensure_strcpy_decl(self) -> None:
        if "strcpy" not in self._declared_externs:
            self._globals.append("declare i8* @strcpy(i8*, i8*)")
            self._declared_externs.add("strcpy")

    def _ensure_strcat_decl(self) -> None:
        if "strcat" not in self._declared_externs:
            self._globals.append("declare i8* @strcat(i8*, i8*)")
            self._declared_externs.add("strcat")

    def _ensure_sqrt_decl(self) -> None:
        if "llvm.sqrt.f64" not in self._declared_externs:
            self._globals.append("declare double @llvm.sqrt.f64(double)")
            self._declared_externs.add("llvm.sqrt.f64")

    def _ensure_fabs_decl(self) -> None:
        if "llvm.fabs.f64" not in self._declared_externs:
            self._globals.append("declare double @llvm.fabs.f64(double)")
            self._declared_externs.add("llvm.fabs.f64")

    def _ensure_pow_decl(self) -> None:
        if "llvm.pow.f64" not in self._declared_externs:
            self._globals.append("declare double @llvm.pow.f64(double, double)")
            self._declared_externs.add("llvm.pow.f64")

    def _ensure_realloc_decl(self) -> None:
        if "realloc" not in self._declared_externs:
            self._globals.append("declare i8* @realloc(i8*, i64)")
            self._declared_externs.add("realloc")

    def _ensure_array_type_decl(self, elem_llty: str) -> str:
        """Emit ``%Array_<elem> = type { i32, i32, <elem>* }`` once per element type.

        Returns the struct type name (e.g. ``%Array_i32``).
        """
        # Normalise element type to a safe name fragment
        safe = elem_llty.replace("*", "ptr").replace(" ", "_")
        struct_name = f"%Array_{safe}"
        if elem_llty not in self._emitted_array_types:
            self._globals.append(
                f"{struct_name} = type {{ i32, i32, {elem_llty}* }}"
            )
            self._emitted_array_types.add(elem_llty)
        return struct_name

    def _array_struct_name_for_expr(self, expr: Expression) -> str:
        """Return the %Array_<elem> struct name for an array expression."""
        # Look up the variable's type in _env
        if isinstance(expr, Identifier) and expr.name in self._env:
            _, llty = self._env[expr.name]
            # llty should be "%Array_i32*" → extract "%Array_i32"
            if llty.startswith("%Array_") and llty.endswith("*"):
                return llty[:-1]
        # Fallback: try to infer from the expression
        if isinstance(expr, ArrayLiteral):
            if expr.elements:
                elem_llty = self._infer_llvm_type(expr.elements[0])
            else:
                elem_llty = "i32"
            safe = elem_llty.replace("*", "ptr").replace(" ", "_")
            return f"%Array_{safe}"
        return "%Array_i32"  # conservative fallback

    def _array_elem_type_for_expr(self, expr: Expression) -> str:
        """Return the element LLVM type for an array expression."""
        if isinstance(expr, Identifier) and expr.name in self._env:
            _, llty = self._env[expr.name]
            # llty is e.g. "%Array_i32*" → extract element type "i32"
            if llty.startswith("%Array_") and llty.endswith("*"):
                inner = llty[len("%Array_"):-1]  # e.g. "i32"
                return inner.replace("ptr", "*").replace("_", " ")
        if isinstance(expr, ArrayLiteral) and expr.elements:
            return self._infer_llvm_type(expr.elements[0])
        return "i32"

    def _ensure_sprintf_decl(self) -> None:
        if "sprintf" not in self._declared_externs:
            self._globals.append("declare i32 @sprintf(i8*, i8*, ...)")
            self._declared_externs.add("sprintf")

    def _target_triple(self) -> str:
        """Return the LLVM target triple for the current platform."""
        system = platform.system()
        if system == "Windows":
            return "x86_64-pc-windows-msvc"
        elif system == "Darwin":
            return "x86_64-apple-macosx10.15.0"
        return "x86_64-pc-linux-gnu"

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
        self._lambda_counter = 0
        self._enum_types = {}
        self._enum_variant_tags = {}
        self._impl_methods = {}
        self._trait_decls = {}
        self._generated_functions = set()
        self._emitted_array_types = set()

        fn_irs: list[str] = []
        top_level_stmts: list[Statement] = []

        # First pass -- collect declarations
        for decl in module.declarations:
            if isinstance(decl, FnDecl):
                self._functions[decl.name] = decl
            elif isinstance(decl, StructDecl):
                self._struct_types[decl.name] = decl
            elif isinstance(decl, EnumDecl):
                self._enum_types[decl.name] = decl
                for i, variant in enumerate(decl.variants):
                    self._enum_variant_tags[f"{decl.name}::{variant.name}"] = i
            elif isinstance(decl, TraitDecl):
                self._trait_decls[decl.name] = decl

        # Second pass -- generate code
        for decl in module.declarations:
            if isinstance(decl, FnDecl):
                fn_irs.append(self._gen_function(decl))
            elif isinstance(decl, StructDecl):
                self._globals.append(self._gen_struct_type(decl))
            elif isinstance(decl, EnumDecl):
                self._globals.append(self._gen_enum_type(decl))
            elif isinstance(decl, TraitDecl):
                self._globals.append(self._gen_trait_vtable_type(decl))
            elif isinstance(decl, ImplBlock):
                for ir in self._gen_impl_block(decl):
                    fn_irs.append(ir)
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
            f'target triple = "{self._target_triple()}"',
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

        # For struct literals: _gen_struct_literal already does alloca and returns
        # a pointer -- just alias that pointer in _env.
        if isinstance(stmt.value, StructLiteral):
            val_lines, val_reg = self._gen_expr(stmt.value)
            lines.extend(val_lines)
            self._env[stmt.name] = (val_reg, llty)
            return lines

        # For array literals: _gen_array_literal returns a %Array_<elem>* pointer.
        # Store it in a local alloca so it can be loaded later.
        if isinstance(stmt.value, ArrayLiteral):
            val_lines, val_reg = self._gen_expr(stmt.value)
            lines.extend(val_lines)
            ptr = self._next_reg()
            lines.append(f"  {ptr} = alloca {llty}")
            lines.append(f"  store {llty} {val_reg}, {llty}* {ptr}")
            self._env[stmt.name] = (ptr, llty)
            return lines

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
        """Generate IR for ``for x in range(start, end) { ... }``."""
        lines: list[str] = []

        # Detect range(start, end) pattern: iterable is a FnCall with callee "range"
        start_reg = "0"
        end_reg = "0"
        start_lines: list[str] = []
        end_lines: list[str] = []

        if (isinstance(stmt.iterable, FnCall)
                and isinstance(stmt.iterable.callee, Identifier)
                and stmt.iterable.callee.name == "range"
                and len(stmt.iterable.args) >= 2):
            start_lines, start_reg = self._gen_expr(stmt.iterable.args[0])
            end_lines, end_reg = self._gen_expr(stmt.iterable.args[1])
        elif (isinstance(stmt.iterable, FnCall)
              and isinstance(stmt.iterable.callee, Identifier)
              and stmt.iterable.callee.name == "range"
              and len(stmt.iterable.args) == 1):
            # range(n) -- start at 0
            end_lines, end_reg = self._gen_expr(stmt.iterable.args[0])
        else:
            # Fallback: cannot generate loop for unknown iterable
            lines.append(f"  ; TODO: for-loop over non-range iterable")
            if stmt.body:
                for s in stmt.body.statements:
                    lines.extend(self._gen_stmt(s, fn_ret))
            return lines

        label_cond = self._next_label("for.cond")
        label_body = self._next_label("for.body")
        label_inc = self._next_label("for.inc")
        label_end = self._next_label("for.end")

        # Allocate loop variable
        counter_ptr = self._next_reg()
        lines.append(f"  {counter_ptr} = alloca i32")
        lines.extend(start_lines)
        lines.append(f"  store i32 {start_reg}, i32* {counter_ptr}")
        self._env[stmt.variable] = (counter_ptr, "i32")

        # Generate end value (may depend on a variable, so emit outside the loop)
        lines.extend(end_lines)

        # Condition block
        lines.append(f"  br label %{label_cond}")
        lines.append(f"{label_cond}:")
        cur_val = self._next_reg()
        lines.append(f"  {cur_val} = load i32, i32* {counter_ptr}")
        cmp = self._next_reg()
        lines.append(f"  {cmp} = icmp slt i32 {cur_val}, {end_reg}")
        lines.append(f"  br i1 {cmp}, label %{label_body}, label %{label_end}")

        # Body block
        lines.append(f"{label_body}:")
        if stmt.body:
            for s in stmt.body.statements:
                lines.extend(self._gen_stmt(s, fn_ret))
        if not lines or not self._is_terminator(lines[-1]):
            lines.append(f"  br label %{label_inc}")

        # Increment block
        lines.append(f"{label_inc}:")
        inc_val = self._next_reg()
        lines.append(f"  {inc_val} = load i32, i32* {counter_ptr}")
        inc_result = self._next_reg()
        lines.append(f"  {inc_result} = add i32 {inc_val}, 1")
        lines.append(f"  store i32 {inc_result}, i32* {counter_ptr}")
        lines.append(f"  br label %{label_cond}")

        # End block
        lines.append(f"{label_end}:")
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
            return self._gen_array_literal(expr)

        if isinstance(expr, StructLiteral):
            return self._gen_struct_literal(expr)

        if isinstance(expr, Lambda):
            return self._gen_lambda_expr(expr)

        if isinstance(expr, MatchExpr):
            return self._gen_match_expr(expr)

        if isinstance(expr, MethodCall):
            return self._gen_method_call(expr)

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

        # String concatenation via +
        if op == "+" and self._expr_is_string(expr.left):
            return self._gen_string_concat(expr.left, expr.right)

        # Power operator (**)
        if op == "**":
            if is_float:
                self._ensure_pow_decl()
                lines.append(f"  {result} = call double @llvm.pow.f64(double {l_reg}, double {r_reg})")
                return lines, result
            else:
                # For integers: cast to double, call pow, cast back to i32
                self._ensure_pow_decl()
                l_f = self._next_reg()
                lines.append(f"  {l_f} = sitofp i32 {l_reg} to double")
                r_f = self._next_reg()
                lines.append(f"  {r_f} = sitofp i32 {r_reg} to double")
                pow_result = self._next_reg()
                lines.append(f"  {pow_result} = call double @llvm.pow.f64(double {l_f}, double {r_f})")
                lines.append(f"  {result} = fptosi double {pow_result} to i32")
                return lines, result

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

    # -- string concat ------------------------------------------------------

    def _gen_string_concat(self, left: Expression, right: Expression) -> tuple[list[str], str]:
        """Generate IR for string concatenation: strlen both, malloc, strcpy, strcat."""
        lines: list[str] = []
        self._ensure_strlen_decl()
        self._ensure_malloc_decl()
        self._ensure_strcpy_decl()
        self._ensure_strcat_decl()

        l_lines, l_reg = self._gen_expr(left)
        r_lines, r_reg = self._gen_expr(right)
        lines.extend(l_lines)
        lines.extend(r_lines)

        # strlen(left)
        len_l = self._next_reg()
        lines.append(f"  {len_l} = call i64 @strlen(i8* {l_reg})")
        # strlen(right)
        len_r = self._next_reg()
        lines.append(f"  {len_r} = call i64 @strlen(i8* {r_reg})")
        # total = len_l + len_r + 1 (null terminator)
        total = self._next_reg()
        lines.append(f"  {total} = add i64 {len_l}, {len_r}")
        total_plus1 = self._next_reg()
        lines.append(f"  {total_plus1} = add i64 {total}, 1")
        # malloc
        raw_ptr = self._next_reg()
        lines.append(f"  {raw_ptr} = call i8* @malloc(i64 {total_plus1})")
        # strcpy(buf, left)
        self._next_reg()  # discard return
        lines.append(f"  call i8* @strcpy(i8* {raw_ptr}, i8* {l_reg})")
        # strcat(buf, right)
        self._next_reg()  # discard return
        lines.append(f"  call i8* @strcat(i8* {raw_ptr}, i8* {r_reg})")

        return lines, raw_ptr

    def _expr_is_string(self, expr: Optional[Expression]) -> bool:
        """Check if an expression produces a string (i8*) value."""
        if expr is None:
            return False
        if isinstance(expr, StringLiteral):
            return True
        if isinstance(expr, Identifier):
            if expr.name in self._env:
                return self._env[expr.name][1] == "i8*"
        if isinstance(expr, FnCall):
            if isinstance(expr.callee, Identifier):
                if expr.callee.name in ("to_string",):
                    return True
                fn = self._functions.get(expr.callee.name)
                if fn:
                    return _llvm_type(fn.return_type) == "i8*"
        return False

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

        # Built-in: len(x)
        if callee_name == "len" and len(expr.args) == 1:
            return self._gen_builtin_len(expr.args[0])

        # Built-in: to_string(x)
        if callee_name == "to_string" and len(expr.args) == 1:
            return self._gen_builtin_to_string(expr.args[0])

        # Built-in: sqrt(x)
        if callee_name == "sqrt" and len(expr.args) == 1:
            return self._gen_builtin_sqrt(expr.args[0])

        # Built-in: abs(x)
        if callee_name == "abs" and len(expr.args) == 1:
            return self._gen_builtin_abs(expr.args[0])

        # Built-in: range(start, end) -- only meaningful inside for-loops,
        # handled by _gen_for. If called standalone, return 0 with a comment.
        if callee_name == "range":
            return [f"  ; range() is handled by for-loop codegen, not as a runtime call"], "0"

        # Built-in: push(arr, val)
        if callee_name == "push" and len(expr.args) == 2:
            return self._gen_builtin_push(expr.args[0], expr.args[1])

        # Built-in: pop(arr)
        if callee_name == "pop" and len(expr.args) == 1:
            return self._gen_builtin_pop(expr.args[0])

        # Generic function instantiation -- if the function is generic and
        # type arguments are provided, generate a specialized version.
        fn_decl = self._functions.get(callee_name)
        if fn_decl and fn_decl.generics and expr.type_args:
            type_arg_names = [_kryos_type_name(ta) for ta in expr.type_args]
            callee_name = self._gen_generic_instantiation(fn_decl, type_arg_names)

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

        if isinstance(arg, StringLiteral) or self._expr_is_string(arg):
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

    # -- built-in functions -------------------------------------------------

    def _gen_builtin_len(self, arg: Expression) -> tuple[list[str], str]:
        """Generate IR for len(x): strlen for strings, struct field 0 for arrays."""
        lines: list[str] = []
        arg_lines, arg_reg = self._gen_expr(arg)
        lines.extend(arg_lines)

        if self._expr_is_string(arg):
            self._ensure_strlen_decl()
            len_reg = self._next_reg()
            lines.append(f"  {len_reg} = call i64 @strlen(i8* {arg_reg})")
            # Truncate i64 to i32 for consistency
            result = self._next_reg()
            lines.append(f"  {result} = trunc i64 {len_reg} to i32")
            return lines, result

        # For arrays: GEP into struct field 0 (length) and load it.
        # The arg_reg is a %Array_<elem>* pointer.
        struct_name = self._array_struct_name_for_expr(arg)
        len_ptr = self._next_reg()
        lines.append(f"  {len_ptr} = getelementptr inbounds {struct_name}, {struct_name}* {arg_reg}, i32 0, i32 0")
        result = self._next_reg()
        lines.append(f"  {result} = load i32, i32* {len_ptr}")
        return lines, result

    def _gen_builtin_push(self, arr_expr: Expression, val_expr: Expression) -> tuple[list[str], str]:
        """Generate IR for push(arr, val): append value, resize if needed."""
        lines: list[str] = []
        self._ensure_realloc_decl()
        self._ensure_malloc_decl()

        # Generate the array struct pointer
        arr_lines, arr_reg = self._gen_expr(arr_expr)
        lines.extend(arr_lines)

        struct_name = self._array_struct_name_for_expr(arr_expr)
        elem_ty = self._array_elem_type_for_expr(arr_expr)
        elem_size = self._sizeof_llvm_type(elem_ty)

        # Load current length
        len_ptr = self._next_reg()
        lines.append(f"  {len_ptr} = getelementptr inbounds {struct_name}, {struct_name}* {arr_reg}, i32 0, i32 0")
        cur_len = self._next_reg()
        lines.append(f"  {cur_len} = load i32, i32* {len_ptr}")

        # Load current capacity
        cap_ptr = self._next_reg()
        lines.append(f"  {cap_ptr} = getelementptr inbounds {struct_name}, {struct_name}* {arr_reg}, i32 0, i32 1")
        cur_cap = self._next_reg()
        lines.append(f"  {cur_cap} = load i32, i32* {cap_ptr}")

        # Check if length == capacity (need to grow)
        need_grow = self._next_reg()
        lines.append(f"  {need_grow} = icmp eq i32 {cur_len}, {cur_cap}")

        lbl_grow = self._next_label("push_grow")
        lbl_no_grow = self._next_label("push_no_grow")
        lbl_continue = self._next_label("push_cont")
        lines.append(f"  br i1 {need_grow}, label %{lbl_grow}, label %{lbl_no_grow}")

        # --- grow branch: double capacity and realloc ---
        lines.append(f"{lbl_grow}:")
        # New capacity = max(cur_cap * 2, 8)
        doubled = self._next_reg()
        lines.append(f"  {doubled} = mul i32 {cur_cap}, 2")
        cmp_min = self._next_reg()
        lines.append(f"  {cmp_min} = icmp slt i32 {doubled}, 8")
        new_cap_grow = self._next_reg()
        lines.append(f"  {new_cap_grow} = select i1 {cmp_min}, i32 8, i32 {doubled}")

        # Load current data pointer
        data_field_grow = self._next_reg()
        lines.append(f"  {data_field_grow} = getelementptr inbounds {struct_name}, {struct_name}* {arr_reg}, i32 0, i32 2")
        old_data_grow = self._next_reg()
        lines.append(f"  {old_data_grow} = load {elem_ty}*, {elem_ty}** {data_field_grow}")

        # Realloc
        old_raw_grow = self._next_reg()
        lines.append(f"  {old_raw_grow} = bitcast {elem_ty}* {old_data_grow} to i8*")
        new_bytes = self._next_reg()
        lines.append(f"  {new_bytes} = sext i32 {new_cap_grow} to i64")
        new_total = self._next_reg()
        lines.append(f"  {new_total} = mul i64 {new_bytes}, {elem_size}")
        new_raw = self._next_reg()
        lines.append(f"  {new_raw} = call i8* @realloc(i8* {old_raw_grow}, i64 {new_total})")
        new_data_grow = self._next_reg()
        lines.append(f"  {new_data_grow} = bitcast i8* {new_raw} to {elem_ty}*")

        # Update data pointer and capacity in struct
        lines.append(f"  store {elem_ty}* {new_data_grow}, {elem_ty}** {data_field_grow}")
        cap_ptr_grow = self._next_reg()
        lines.append(f"  {cap_ptr_grow} = getelementptr inbounds {struct_name}, {struct_name}* {arr_reg}, i32 0, i32 1")
        lines.append(f"  store i32 {new_cap_grow}, i32* {cap_ptr_grow}")
        lines.append(f"  br label %{lbl_continue}")

        # --- no-grow branch ---
        lines.append(f"{lbl_no_grow}:")
        data_field_no = self._next_reg()
        lines.append(f"  {data_field_no} = getelementptr inbounds {struct_name}, {struct_name}* {arr_reg}, i32 0, i32 2")
        old_data_no = self._next_reg()
        lines.append(f"  {old_data_no} = load {elem_ty}*, {elem_ty}** {data_field_no}")
        lines.append(f"  br label %{lbl_continue}")

        # --- continue: phi the data pointer ---
        lines.append(f"{lbl_continue}:")
        data_phi = self._next_reg()
        lines.append(f"  {data_phi} = phi {elem_ty}* [{new_data_grow}, %{lbl_grow}], [{old_data_no}, %{lbl_no_grow}]")

        # Generate the value to push
        val_lines, val_reg = self._gen_expr(val_expr)
        lines.extend(val_lines)

        # Store value at data[length]
        elem_ptr = self._next_reg()
        lines.append(f"  {elem_ptr} = getelementptr {elem_ty}, {elem_ty}* {data_phi}, i32 {cur_len}")
        lines.append(f"  store {elem_ty} {val_reg}, {elem_ty}* {elem_ptr}")

        # Increment length
        new_len = self._next_reg()
        lines.append(f"  {new_len} = add i32 {cur_len}, 1")
        len_ptr2 = self._next_reg()
        lines.append(f"  {len_ptr2} = getelementptr inbounds {struct_name}, {struct_name}* {arr_reg}, i32 0, i32 0")
        lines.append(f"  store i32 {new_len}, i32* {len_ptr2}")

        return lines, "0"

    def _gen_builtin_pop(self, arr_expr: Expression) -> tuple[list[str], str]:
        """Generate IR for pop(arr): decrement length and return the last element."""
        lines: list[str] = []

        # Generate the array struct pointer
        arr_lines, arr_reg = self._gen_expr(arr_expr)
        lines.extend(arr_lines)

        struct_name = self._array_struct_name_for_expr(arr_expr)
        elem_ty = self._array_elem_type_for_expr(arr_expr)

        # Load current length
        len_ptr = self._next_reg()
        lines.append(f"  {len_ptr} = getelementptr inbounds {struct_name}, {struct_name}* {arr_reg}, i32 0, i32 0")
        cur_len = self._next_reg()
        lines.append(f"  {cur_len} = load i32, i32* {len_ptr}")

        # Decrement length
        new_len = self._next_reg()
        lines.append(f"  {new_len} = sub i32 {cur_len}, 1")

        # Store new length
        lines.append(f"  store i32 {new_len}, i32* {len_ptr}")

        # Load data pointer
        data_field = self._next_reg()
        lines.append(f"  {data_field} = getelementptr inbounds {struct_name}, {struct_name}* {arr_reg}, i32 0, i32 2")
        data_ptr = self._next_reg()
        lines.append(f"  {data_ptr} = load {elem_ty}*, {elem_ty}** {data_field}")

        # Load value at data[new_length] (the old last element)
        elem_ptr = self._next_reg()
        lines.append(f"  {elem_ptr} = getelementptr {elem_ty}, {elem_ty}* {data_ptr}, i32 {new_len}")
        result = self._next_reg()
        lines.append(f"  {result} = load {elem_ty}, {elem_ty}* {elem_ptr}")

        return lines, result

    def _gen_builtin_to_string(self, arg: Expression) -> tuple[list[str], str]:
        """Generate IR for to_string(x): use sprintf to convert int/float to string."""
        lines: list[str] = []
        self._ensure_sprintf_decl()
        self._ensure_malloc_decl()

        arg_lines, arg_reg = self._gen_expr(arg)
        lines.extend(arg_lines)

        # Allocate a buffer (32 bytes is plenty for numeric strings)
        buf = self._next_reg()
        lines.append(f"  {buf} = call i8* @malloc(i64 32)")

        if self._expr_is_float(arg):
            fmt_name = self._add_string_constant("%f")
            fmt_reg = self._next_reg()
            lines.append(f"  {fmt_reg} = getelementptr [3 x i8], [3 x i8]* {fmt_name}, i32 0, i32 0")
            self._next_reg()  # discard sprintf return
            lines.append(f"  call i32 (i8*, i8*, ...) @sprintf(i8* {buf}, i8* {fmt_reg}, double {arg_reg})")
        else:
            fmt_name = self._add_string_constant("%d")
            fmt_reg = self._next_reg()
            lines.append(f"  {fmt_reg} = getelementptr [3 x i8], [3 x i8]* {fmt_name}, i32 0, i32 0")
            self._next_reg()  # discard sprintf return
            lines.append(f"  call i32 (i8*, i8*, ...) @sprintf(i8* {buf}, i8* {fmt_reg}, i32 {arg_reg})")

        return lines, buf

    def _gen_builtin_sqrt(self, arg: Expression) -> tuple[list[str], str]:
        """Generate IR for sqrt(x): call llvm.sqrt.f64."""
        lines: list[str] = []
        self._ensure_sqrt_decl()

        arg_lines, arg_reg = self._gen_expr(arg)
        lines.extend(arg_lines)

        # If the argument is an integer, convert to double first
        if not self._expr_is_float(arg):
            conv = self._next_reg()
            lines.append(f"  {conv} = sitofp i32 {arg_reg} to double")
            arg_reg = conv

        result = self._next_reg()
        lines.append(f"  {result} = call double @llvm.sqrt.f64(double {arg_reg})")
        return lines, result

    def _gen_builtin_abs(self, arg: Expression) -> tuple[list[str], str]:
        """Generate IR for abs(x): fabs for floats, select for ints."""
        lines: list[str] = []
        arg_lines, arg_reg = self._gen_expr(arg)
        lines.extend(arg_lines)

        if self._expr_is_float(arg):
            self._ensure_fabs_decl()
            result = self._next_reg()
            lines.append(f"  {result} = call double @llvm.fabs.f64(double {arg_reg})")
            return lines, result
        else:
            # For ints: neg = 0 - x; cmp = x < 0; result = select(cmp, neg, x)
            neg = self._next_reg()
            lines.append(f"  {neg} = sub i32 0, {arg_reg}")
            cmp = self._next_reg()
            lines.append(f"  {cmp} = icmp slt i32 {arg_reg}, 0")
            result = self._next_reg()
            lines.append(f"  {result} = select i1 {cmp}, i32 {neg}, i32 {arg_reg}")
            return lines, result

    # -- struct name inference -----------------------------------------------

    def _infer_struct_name(self, expr: Optional[Expression]) -> Optional[str]:
        """Infer the struct type name from an expression (for field access)."""
        if expr is None:
            return None
        if isinstance(expr, Identifier):
            name = expr.name
            if name in self._env:
                _, llty = self._env[name]
                # Check if llty matches a known struct type (e.g. %struct.Point*)
                for sname in self._struct_types:
                    if llty == f"%struct.{sname}" or llty == f"%struct.{sname}*":
                        return sname
        if isinstance(expr, StructLiteral):
            return expr.type_name
        return None

    # -- field access -------------------------------------------------------

    def _gen_field_access(self, expr: FieldAccess) -> tuple[list[str], str]:
        """Generate GEP + load for struct field access (e.g. ``p.x``)."""
        lines: list[str] = []

        # For identifier objects, get the pointer directly from _env
        # (don't load the struct value -- we need the pointer for GEP)
        struct_name = self._infer_struct_name(expr.object)
        obj_reg = "0"

        if isinstance(expr.object, Identifier) and expr.object.name in self._env:
            obj_reg = self._env[expr.object.name][0]
        else:
            obj_lines, obj_reg = self._gen_expr(expr.object)
            lines.extend(obj_lines)

        if struct_name and struct_name in self._struct_types:
            decl = self._struct_types[struct_name]
            # Find the field index
            field_idx = -1
            field_ty = "i32"
            for i, f in enumerate(decl.fields):
                if f.name == expr.field:
                    field_idx = i
                    field_ty = _llvm_type(f.type_annotation)
                    break

            if field_idx >= 0:
                fptr = self._next_reg()
                lines.append(
                    f"  {fptr} = getelementptr %struct.{struct_name}, "
                    f"%struct.{struct_name}* {obj_reg}, i32 0, i32 {field_idx}"
                )
                result = self._next_reg()
                lines.append(f"  {result} = load {field_ty}, {field_ty}* {fptr}")
                return lines, result

        # Fallback if struct info is unavailable
        lines.append(f"  ; WARNING: could not resolve field access .{expr.field}")
        return lines, "0"

    # -- index access -------------------------------------------------------

    def _gen_index_access(self, expr: IndexAccess) -> tuple[list[str], str]:
        """Generate index access through array metadata struct.

        Loads the data pointer from struct field 2, then GEPs into
        ``data[index]`` and loads the value.
        """
        lines: list[str] = []

        # Generate the array struct pointer expression
        arr_lines, arr_reg = self._gen_expr(expr.object)
        lines.extend(arr_lines)

        # Generate the index expression
        idx_lines, idx_reg = self._gen_expr(expr.index)
        lines.extend(idx_lines)

        # Determine struct name and element type
        struct_name = self._array_struct_name_for_expr(expr.object)
        elem_ty = self._array_elem_type_for_expr(expr.object)

        # GEP to field 2 (data pointer) and load it
        data_field_ptr = self._next_reg()
        lines.append(f"  {data_field_ptr} = getelementptr inbounds {struct_name}, {struct_name}* {arr_reg}, i32 0, i32 2")
        data_ptr = self._next_reg()
        lines.append(f"  {data_ptr} = load {elem_ty}*, {elem_ty}** {data_field_ptr}")

        # GEP into data[index]
        elem_ptr = self._next_reg()
        lines.append(f"  {elem_ptr} = getelementptr {elem_ty}, {elem_ty}* {data_ptr}, i32 {idx_reg}")

        # Load the value
        result = self._next_reg()
        lines.append(f"  {result} = load {elem_ty}, {elem_ty}* {elem_ptr}")
        return lines, result

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

    # -- array literal ------------------------------------------------------

    def _gen_array_literal(self, expr: ArrayLiteral) -> tuple[list[str], str]:
        """Allocate an array as a metadata struct on the heap.

        The struct layout is ``{ i32 length, i32 capacity, elem* data }``.
        Returns a pointer to the struct (``%Array_<elem>*``).
        """
        lines: list[str] = []
        self._ensure_malloc_decl()

        # Determine element type
        if expr.elements:
            elem_llty = self._infer_llvm_type(expr.elements[0])
        else:
            elem_llty = "i32"  # default for empty arrays

        struct_name = self._ensure_array_type_decl(elem_llty)
        count = len(expr.elements)
        capacity = max(count, 8) if count > 0 else 0
        elem_size = self._sizeof_llvm_type(elem_llty)

        # --- allocate the struct (3 fields: i32, i32, ptr = 4+4+8 = 16 bytes) ---
        struct_raw = self._next_reg()
        lines.append(f"  {struct_raw} = call i8* @malloc(i64 16)")
        struct_ptr = self._next_reg()
        lines.append(f"  {struct_ptr} = bitcast i8* {struct_raw} to {struct_name}*")

        if count == 0:
            # Empty array: length=0, capacity=0, data=null
            len_ptr = self._next_reg()
            lines.append(f"  {len_ptr} = getelementptr inbounds {struct_name}, {struct_name}* {struct_ptr}, i32 0, i32 0")
            lines.append(f"  store i32 0, i32* {len_ptr}")
            cap_ptr = self._next_reg()
            lines.append(f"  {cap_ptr} = getelementptr inbounds {struct_name}, {struct_name}* {struct_ptr}, i32 0, i32 1")
            lines.append(f"  store i32 0, i32* {cap_ptr}")
            data_ptr_field = self._next_reg()
            lines.append(f"  {data_ptr_field} = getelementptr inbounds {struct_name}, {struct_name}* {struct_ptr}, i32 0, i32 2")
            lines.append(f"  store {elem_llty}* null, {elem_llty}** {data_ptr_field}")
            return lines, struct_ptr

        # --- allocate data buffer (capacity * sizeof(elem)) ---
        data_raw = self._next_reg()
        data_bytes = capacity * elem_size
        lines.append(f"  {data_raw} = call i8* @malloc(i64 {data_bytes})")
        data_ptr = self._next_reg()
        lines.append(f"  {data_ptr} = bitcast i8* {data_raw} to {elem_llty}*")

        # Store each element into the data buffer
        for i, elem in enumerate(expr.elements):
            elem_lines, elem_reg = self._gen_expr(elem)
            lines.extend(elem_lines)
            gep = self._next_reg()
            lines.append(f"  {gep} = getelementptr {elem_llty}, {elem_llty}* {data_ptr}, i32 {i}")
            lines.append(f"  store {elem_llty} {elem_reg}, {elem_llty}* {gep}")

        # Store length into struct field 0
        len_ptr = self._next_reg()
        lines.append(f"  {len_ptr} = getelementptr inbounds {struct_name}, {struct_name}* {struct_ptr}, i32 0, i32 0")
        lines.append(f"  store i32 {count}, i32* {len_ptr}")

        # Store capacity into struct field 1
        cap_ptr = self._next_reg()
        lines.append(f"  {cap_ptr} = getelementptr inbounds {struct_name}, {struct_name}* {struct_ptr}, i32 0, i32 1")
        lines.append(f"  store i32 {capacity}, i32* {cap_ptr}")

        # Store data pointer into struct field 2
        data_ptr_field = self._next_reg()
        lines.append(f"  {data_ptr_field} = getelementptr inbounds {struct_name}, {struct_name}* {struct_ptr}, i32 0, i32 2")
        lines.append(f"  store {elem_llty}* {data_ptr}, {elem_llty}** {data_ptr_field}")

        return lines, struct_ptr

    @staticmethod
    def _sizeof_llvm_type(llty: str) -> int:
        """Return the size in bytes for a given LLVM type (approximate, for malloc)."""
        sizes = {
            "i1": 1, "i8": 1, "i16": 2, "i32": 4, "i64": 8, "i128": 16,
            "float": 4, "double": 8, "i8*": 8,
        }
        if llty in sizes:
            return sizes[llty]
        if llty.endswith("*"):
            return 8  # pointer size on 64-bit
        return 8  # conservative default

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
    # Enum types
    # -----------------------------------------------------------------------

    def _gen_enum_type(self, decl: EnumDecl) -> str:
        """Generate LLVM IR type for an enum (tagged union).

        Layout: { i8 tag, [max_payload x i8] payload }
        Each variant gets a sequential tag number starting at 0.
        """
        max_payload = 0
        for variant in decl.variants:
            if variant.fields:
                size = sum(self._sizeof_type(f) for f in variant.fields)
                max_payload = max(max_payload, size)

        type_name = f"%enum.{decl.name}"
        if max_payload > 0:
            return f"{type_name} = type {{ i8, [{max_payload} x i8] }}"
        else:
            return f"{type_name} = type {{ i8 }}"

    def _sizeof_type(self, ty: TypeNode) -> int:
        """Return the size in bytes for a Kryos type node (for enum payload sizing)."""
        llty = _llvm_type(ty)
        return self._sizeof_llvm_type(llty)

    # -----------------------------------------------------------------------
    # Trait vtable types
    # -----------------------------------------------------------------------

    def _gen_trait_vtable_type(self, decl: TraitDecl) -> str:
        """Generate LLVM IR type for a trait vtable (struct of function pointers).

        Each trait method becomes a function pointer in the vtable struct.
        For MVP, this establishes the vtable layout without dynamic dispatch.
        """
        fn_ptr_types: list[str] = []
        for method in decl.methods:
            ret_ty = _llvm_type(method.return_type)
            # self is passed as i8* (opaque pointer)
            param_types = ["i8*"]
            for p in method.params:
                if p.name != "self":
                    param_types.append(_llvm_type(p.type_annotation))
            params_str = ", ".join(param_types)
            fn_ptr_types.append(f"{ret_ty} ({params_str})*")

        if fn_ptr_types:
            fields = ", ".join(fn_ptr_types)
        else:
            fields = "i8"  # empty trait placeholder
        return f"%vtable.{decl.name} = type {{ {fields} }}"

    # -----------------------------------------------------------------------
    # Impl blocks
    # -----------------------------------------------------------------------

    def _gen_impl_block(self, node: ImplBlock) -> list[str]:
        """Generate methods from an impl block as mangled-name functions.

        Methods are emitted as top-level functions with the naming convention
        ``TypeName_methodName``.  The ``self`` parameter is compiled as a
        pointer to the target struct type.
        """
        results: list[str] = []
        target_name = _kryos_type_name(node.target_type)
        for method in node.methods:
            # Mangle the name: StructName_method_name
            mangled = f"{target_name}_{method.name}"
            original_name = method.name
            method.name = mangled

            # If first param is "self", adjust its type to be a pointer to the struct
            original_params = list(method.params)
            if method.params and method.params[0].name == "self":
                self_param = method.params[0]
                if self_param.type_annotation is None:
                    self_param.type_annotation = SimpleType(name=f"%struct.{target_name}*")

            ir = self._gen_function(method)
            results.append(ir)

            # Register the method so it can be found later
            self._functions[mangled] = method
            self._impl_methods[mangled] = method

            # Restore original state
            method.name = original_name
            method.params = original_params

        return results

    # -----------------------------------------------------------------------
    # Lambda expressions
    # -----------------------------------------------------------------------

    def _gen_lambda_expr(self, node: Lambda) -> tuple[list[str], str]:
        """Generate a lambda as a top-level function and return its function pointer.

        For MVP, lambdas are compiled as regular top-level functions with
        mangled names.  No environment capture is performed yet.

        TODO: Implement closure environment capture -- allocate an env struct
        on the heap containing captured free variables, and pass it as an
        extra parameter to the generated function.
        """
        name = f"__lambda_{self._lambda_counter}"
        self._lambda_counter += 1

        # Convert the lambda body to a BlockStmt if it's a bare expression
        if isinstance(node.body, BlockStmt):
            body = node.body
        else:
            # Wrap a bare expression in a return statement inside a block
            from kryos.compiler.ast_nodes import ReturnStmt as RetStmt
            ret = RetStmt(value=node.body, span=node.span)
            body = BlockStmt(statements=[ret], span=node.span)

        fn_decl = FnDecl(
            name=name,
            params=node.params,
            return_type=node.return_type,
            body=body,
            span=node.span,
        )
        self._functions[name] = fn_decl
        ir = self._gen_function(fn_decl)
        self._globals.append(ir)

        # Return the function pointer as the expression value
        return [], f"@{name}"

    # -----------------------------------------------------------------------
    # Match expressions
    # -----------------------------------------------------------------------

    def _gen_match_expr(self, node: MatchExpr) -> tuple[list[str], str]:
        """Generate LLVM IR for a match expression.

        For integer/bool subjects, generates a chain of compare-and-branch.
        For enum subjects, extracts the tag and uses an LLVM switch instruction.
        Each arm's body is generated as a separate basic block, and the
        match result is collected via a phi node at the end.
        """
        lines: list[str] = []

        # Evaluate the subject expression
        subj_lines, subj_reg = self._gen_expr(node.value)
        lines.extend(subj_lines)

        subj_type = self._infer_llvm_type(node.value)

        # Determine if this is an enum match
        is_enum_match = subj_type.startswith("%enum.")

        end_label = self._next_label("match.end")

        if is_enum_match:
            return self._gen_enum_match(node, subj_reg, subj_type, end_label, lines)
        else:
            return self._gen_value_match(node, subj_reg, subj_type, end_label, lines)

    def _gen_value_match(
        self,
        node: MatchExpr,
        subj_reg: str,
        subj_type: str,
        end_label: str,
        lines: list[str],
    ) -> tuple[list[str], str]:
        """Generate a match over integer/bool values using compare-and-branch."""
        arm_labels: list[str] = []
        arm_results: list[tuple[str, str]] = []  # (result_reg, label)

        for i, arm in enumerate(node.arms):
            arm_labels.append(self._next_label(f"match.arm.{i}"))

        default_label = self._next_label("match.default")

        # Generate condition checks and branches
        for i, arm in enumerate(node.arms):
            if arm.pattern is not None:
                # Check if this is a wildcard/default arm (identifier "_")
                if isinstance(arm.pattern, Identifier) and arm.pattern.name == "_":
                    lines.append(f"  br label %{arm_labels[i]}")
                else:
                    pat_lines, pat_reg = self._gen_expr(arm.pattern)
                    lines.extend(pat_lines)
                    cmp_reg = self._next_reg()
                    if subj_type in ("float", "double"):
                        lines.append(f"  {cmp_reg} = fcmp oeq {subj_type} {subj_reg}, {pat_reg}")
                    else:
                        lines.append(f"  {cmp_reg} = icmp eq {subj_type} {subj_reg}, {pat_reg}")
                    next_check = arm_labels[i + 1] if i + 1 < len(arm_labels) else default_label
                    lines.append(f"  br i1 {cmp_reg}, label %{arm_labels[i]}, label %{next_check}")
            else:
                lines.append(f"  br label %{arm_labels[i]}")

        # Generate arm bodies
        for i, arm in enumerate(node.arms):
            lines.append(f"{arm_labels[i]}:")
            if arm.body is not None:
                body_lines, body_reg = self._gen_expr(arm.body)
                lines.extend(body_lines)
                arm_results.append((body_reg, arm_labels[i]))
            else:
                arm_results.append(("0", arm_labels[i]))
            lines.append(f"  br label %{end_label}")

        # Default arm (unreachable)
        lines.append(f"{default_label}:")
        arm_results.append(("0", default_label))
        lines.append(f"  br label %{end_label}")

        # End block -- merge results with phi
        lines.append(f"{end_label}:")
        if arm_results:
            result_type = self._infer_llvm_type(node.arms[0].body) if node.arms and node.arms[0].body else "i32"
            result = self._next_reg()
            phi_entries = ", ".join(f"[{reg}, %{label}]" for reg, label in arm_results)
            lines.append(f"  {result} = phi {result_type} {phi_entries}")
            return lines, result

        return lines, "0"

    def _gen_enum_match(
        self,
        node: MatchExpr,
        subj_reg: str,
        subj_type: str,
        end_label: str,
        lines: list[str],
    ) -> tuple[list[str], str]:
        """Generate a match over enum tags using LLVM switch."""
        # Extract the tag (field 0 of the enum struct)
        tag_reg = self._next_reg()
        lines.append(f"  {tag_reg} = extractvalue {subj_type} {subj_reg}, 0")

        arm_labels: list[str] = []
        arm_results: list[tuple[str, str]] = []

        for i in range(len(node.arms)):
            arm_labels.append(self._next_label(f"match.arm.{i}"))

        default_label = self._next_label("match.default")

        # Build switch instruction
        cases: list[str] = []
        for i, arm in enumerate(node.arms):
            if isinstance(arm.pattern, Identifier) and arm.pattern.name == "_":
                # Wildcard -- this becomes the default
                default_label = arm_labels[i]
            else:
                cases.append(f"i8 {i}, label %{arm_labels[i]}")

        cases_str = " ".join(cases)
        lines.append(f"  switch i8 {tag_reg}, label %{default_label} [{cases_str}]")

        # Generate arm bodies
        for i, arm in enumerate(node.arms):
            lines.append(f"{arm_labels[i]}:")
            if arm.body is not None:
                body_lines, body_reg = self._gen_expr(arm.body)
                lines.extend(body_lines)
                arm_results.append((body_reg, arm_labels[i]))
            else:
                arm_results.append(("0", arm_labels[i]))
            lines.append(f"  br label %{end_label}")

        # If default wasn't claimed by a wildcard arm, emit it
        if not any(isinstance(a.pattern, Identifier) and a.pattern.name == "_" for a in node.arms):
            lines.append(f"{default_label}:")
            arm_results.append(("0", default_label))
            lines.append(f"  br label %{end_label}")

        lines.append(f"{end_label}:")
        if arm_results:
            result = self._next_reg()
            phi_entries = ", ".join(f"[{reg}, %{label}]" for reg, label in arm_results)
            lines.append(f"  {result} = phi i32 {phi_entries}")
            return lines, result

        return lines, "0"

    # -----------------------------------------------------------------------
    # Method calls (dispatching to impl block methods)
    # -----------------------------------------------------------------------

    def _gen_method_call(self, expr: MethodCall) -> tuple[list[str], str]:
        """Generate IR for a method call by dispatching to the mangled impl method.

        ``obj.method(args)`` is compiled as ``Type_method(obj_ptr, args)``.
        """
        lines: list[str] = []

        # Infer the type of the object to find the mangled method name
        struct_name = self._infer_struct_name(expr.object)
        if struct_name:
            mangled = f"{struct_name}_{expr.method}"
        else:
            mangled = expr.method

        # Get the object pointer (not loaded -- we need the address for self)
        if isinstance(expr.object, Identifier) and expr.object.name in self._env:
            obj_ptr = self._env[expr.object.name][0]
            obj_llty = self._env[expr.object.name][1]
        else:
            obj_lines, obj_ptr = self._gen_expr(expr.object)
            lines.extend(obj_lines)
            obj_llty = self._infer_llvm_type(expr.object)

        # Build argument list: self pointer + remaining args
        arg_regs: list[tuple[str, str]] = []
        if struct_name and struct_name in self._struct_types:
            arg_regs.append((f"%struct.{struct_name}*", obj_ptr))
        else:
            arg_regs.append((f"{obj_llty}*" if not obj_llty.endswith("*") else obj_llty, obj_ptr))

        for arg in expr.args:
            arg_lines, arg_reg = self._gen_expr(arg)
            lines.extend(arg_lines)
            arg_ty = self._infer_llvm_type(arg)
            arg_regs.append((arg_ty, arg_reg))

        # Determine return type
        fn_decl = self._functions.get(mangled)
        if fn_decl:
            ret_type = _llvm_type(fn_decl.return_type)
        else:
            ret_type = "i32"

        args_str = ", ".join(f"{ty} {reg}" for ty, reg in arg_regs)

        if ret_type == "void":
            lines.append(f"  call void @{mangled}({args_str})")
            return lines, "0"
        else:
            result = self._next_reg()
            lines.append(f"  {result} = call {ret_type} @{mangled}({args_str})")
            return lines, result

    # -----------------------------------------------------------------------
    # Generic instantiation (monomorphization)
    # -----------------------------------------------------------------------

    def _gen_generic_instantiation(self, fn_decl: FnDecl, type_args: list[str]) -> str:
        """Generate a specialized version of a generic function.

        Mangles the function name with the concrete type arguments and
        generates a new version of the function with all generic type
        parameters substituted.  Caches the result so each specialization
        is emitted only once.

        Returns the mangled function name for the specialized version.
        """
        mangled = fn_decl.name + "_" + "_".join(type_args)
        if mangled in self._generated_functions:
            return mangled

        # Build type parameter mapping: generic_name -> concrete_type
        type_map: dict[str, str] = {}
        for i, gp in enumerate(fn_decl.generics):
            if i < len(type_args):
                type_map[gp.name] = type_args[i]

        # Clone the function and substitute types
        specialized = self._substitute_types(fn_decl, type_map)
        specialized.name = mangled
        self._functions[mangled] = specialized
        self._generated_functions.add(mangled)
        ir = self._gen_function(specialized)
        self._globals.append(ir)
        return mangled

    def _substitute_types(self, fn_decl: FnDecl, type_map: dict[str, str]) -> FnDecl:
        """Create a copy of a FnDecl with generic type parameters substituted.

        Returns a new FnDecl with all SimpleType references that match
        generic parameter names replaced with the concrete types.
        """
        import copy
        fn = copy.deepcopy(fn_decl)

        def subst(ty: Optional[TypeNode]) -> Optional[TypeNode]:
            if ty is None:
                return None
            if isinstance(ty, SimpleType) and ty.name in type_map:
                return SimpleType(name=type_map[ty.name], span=ty.span)
            if isinstance(ty, ArrayType):
                ty.element_type = subst(ty.element_type)
            if isinstance(ty, GenericType):
                ty.type_params = [subst(p) for p in ty.type_params]  # type: ignore[misc]
            return ty

        fn.return_type = subst(fn.return_type)
        for p in fn.params:
            p.type_annotation = subst(p.type_annotation)
        fn.generics = []  # no longer generic
        return fn

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
                cname = expr.callee.name
                # Built-in return types
                if cname == "to_string":
                    return "i8*"
                if cname == "len":
                    return "i32"
                if cname == "sqrt":
                    return "double"
                if cname == "abs":
                    if expr.args and self._expr_is_float(expr.args[0]):
                        return "double"
                    return "i32"
                if cname == "push":
                    return "i32"
                if cname == "pop":
                    if expr.args:
                        return self._array_elem_type_for_expr(expr.args[0])
                    return "i32"
                fn = self._functions.get(cname)
                if fn:
                    return _llvm_type(fn.return_type)
            return "i32"
        if isinstance(expr, ArrayLiteral):
            if expr.elements:
                elem_ty = self._infer_llvm_type(expr.elements[0])
            else:
                elem_ty = "i32"
            safe = elem_ty.replace("*", "ptr").replace(" ", "_")
            return f"%Array_{safe}*"
        if isinstance(expr, StructLiteral):
            sname = expr.type_name
            if sname in self._struct_types:
                return f"%struct.{sname}"
            return "i32"
        if isinstance(expr, Lambda):
            # A lambda expression evaluates to a function pointer
            ret = _llvm_type(expr.return_type)
            ptypes = ", ".join(_llvm_type(p.type_annotation) for p in expr.params)
            return f"{ret} ({ptypes})*"
        if isinstance(expr, MatchExpr):
            # Infer type from the first arm's body
            if expr.arms and expr.arms[0].body:
                return self._infer_llvm_type(expr.arms[0].body)
            return "i32"
        if isinstance(expr, MethodCall):
            struct_name = self._infer_struct_name(expr.object)
            if struct_name:
                mangled = f"{struct_name}_{expr.method}"
                fn = self._functions.get(mangled)
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
