"""
Kryos Language - Tree-Walking Interpreter
Evaluates the AST produced by the parser.
"""

from __future__ import annotations

import sys
from typing import Any, Callable, Optional

from kryos.compiler.ast_nodes import (
    ASTNode, Module,
    # Declarations
    FnDecl, StructDecl, EnumDecl, TraitDecl, ImplBlock,
    LetStmt, AssignStmt, ReturnStmt,
    IfStmt, ElifClause, ForStmt, WhileStmt, BreakStmt, ContinueStmt,
    ExprStmt, BlockStmt,
    # Expressions
    Expression, IntLiteral, FloatLiteral, StringLiteral, CharLiteral,
    BoolLiteral, NoneLiteral, Identifier,
    BinaryOp, UnaryOp, FnCall, MethodCall, FieldAccess, IndexAccess,
    ArrayLiteral, StructLiteral, Lambda, Parameter,
    RangeExpr, Attribute,
)


class KryosRuntimeError(Exception):
    """Raised when a runtime error occurs during interpretation."""

    def __init__(self, message: str, line: int = 0, column: int = 0) -> None:
        self.line = line
        self.column = column
        super().__init__(message)


class ReturnSignal(Exception):
    """Internal signal for return statements."""

    def __init__(self, value: Any) -> None:
        self.value = value


class BreakSignal(Exception):
    """Internal signal for break statements."""


class ContinueSignal(Exception):
    """Internal signal for continue statements."""


class Environment:
    """A scope containing variable bindings."""

    def __init__(self, parent: Optional[Environment] = None) -> None:
        self._vars: dict[str, Any] = {}
        self._mutables: set[str] = set()
        self.parent = parent

    def define(self, name: str, value: Any, mutable: bool = False) -> None:
        self._vars[name] = value
        if mutable:
            self._mutables.add(name)

    def get(self, name: str) -> Any:
        if name in self._vars:
            return self._vars[name]
        if self.parent is not None:
            return self.parent.get(name)
        raise KryosRuntimeError(f"undefined variable '{name}'")

    def set(self, name: str, value: Any) -> None:
        if name in self._vars:
            if name not in self._mutables:
                raise KryosRuntimeError(
                    f"cannot assign to immutable variable '{name}'"
                )
            self._vars[name] = value
            return
        if self.parent is not None:
            self.parent.set(name, value)
            return
        raise KryosRuntimeError(f"undefined variable '{name}'")

    def has(self, name: str) -> bool:
        if name in self._vars:
            return True
        if self.parent is not None:
            return self.parent.has(name)
        return False


class KryosFunction:
    """A user-defined Kryos function."""

    def __init__(
        self, name: str, params: list[Parameter], body: BlockStmt,
        closure: Environment, attributes: list[Attribute] | None = None,
    ) -> None:
        self.name = name
        self.params = params
        self.body = body
        self.closure = closure
        self.attributes = attributes or []

    def __repr__(self) -> str:
        return f"<fn {self.name}>"


class KryosStruct:
    """A Kryos struct type."""

    def __init__(self, name: str, field_names: list[str]) -> None:
        self.name = name
        self.field_names = field_names

    def __repr__(self) -> str:
        return f"<struct {self.name}>"


class KryosInstance:
    """An instance of a Kryos struct."""

    def __init__(self, struct: KryosStruct, fields: dict[str, Any]) -> None:
        self.struct = struct
        self.fields = fields

    def __repr__(self) -> str:
        pairs = ", ".join(f"{k}: {_format_value(v)}" for k, v in self.fields.items())
        return f"{self.struct.name} {{ {pairs} }}"


class KryosEnum:
    """A Kryos enum type."""

    def __init__(self, name: str, variants: dict[str, int]) -> None:
        self.name = name
        # variant_name -> field_count
        self.variants = variants

    def __repr__(self) -> str:
        return f"<enum {self.name}>"


class KryosEnumValue:
    """An instance of a Kryos enum variant."""

    def __init__(self, enum: KryosEnum, variant_name: str, fields: list[Any] | None = None) -> None:
        self.enum = enum
        self.variant_name = variant_name
        self.fields = fields or []

    def __repr__(self) -> str:
        if self.fields:
            inner = ", ".join(_format_value(f) for f in self.fields)
            return f"{self.enum.name}::{self.variant_name}({inner})"
        return f"{self.enum.name}::{self.variant_name}"

    def __eq__(self, other: object) -> bool:
        if isinstance(other, KryosEnumValue):
            return (self.enum.name == other.enum.name
                    and self.variant_name == other.variant_name
                    and self.fields == other.fields)
        return NotImplemented


class KryosTrait:
    """A Kryos trait definition."""

    def __init__(self, name: str, method_names: list[str]) -> None:
        self.name = name
        self.method_names = method_names

    def __repr__(self) -> str:
        return f"<trait {self.name}>"


def _format_value(value: Any) -> str:
    """Format a Kryos value for printing."""
    if value is None:
        return "none"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        # Print floats without trailing zeros when they're integers, but keep decimal
        if value == int(value) and not ("e" in str(value) or "E" in str(value)):
            formatted = f"{value:.2f}"
            # Strip trailing zeros but keep at least one decimal
            if "." in formatted:
                formatted = formatted.rstrip("0")
                if formatted.endswith("."):
                    formatted += "0"
            return formatted
        return str(value)
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        inner = ", ".join(_format_value(v) for v in value)
        return f"[{inner}]"
    if isinstance(value, KryosInstance):
        return repr(value)
    if isinstance(value, KryosEnumValue):
        return repr(value)
    if isinstance(value, KryosFunction):
        return repr(value)
    return str(value)


class Interpreter:
    """
    Tree-walking interpreter for the Kryos AST.

    Usage::

        interp = Interpreter()
        interp.run(module)        # module: ast_nodes.Module
        output = interp.output    # list of printed strings
    """

    def __init__(self, output_fn: Callable[[str], None] | None = None, self_healing: bool = True) -> None:
        self.globals = Environment()
        self.output: list[str] = []
        self._output_fn = output_fn or (lambda s: print(s))
        self.self_healing = self_healing
        self._heal_engine = None
        if self_healing:
            from kryos.compiler.self_heal import SelfHealingEngine
            self._heal_engine = SelfHealingEngine()
        self._fn_intents: dict[str, Any] = {}  # function name -> Intent
        # type_name -> {method_name: KryosFunction}
        self._impl_methods: dict[str, dict[str, KryosFunction]] = {}
        # type_name -> [trait_name, ...]
        self._impl_traits: dict[str, list[str]] = {}
        self._setup_builtins()

    def _setup_builtins(self) -> None:
        """Register built-in functions."""

        def _println(*args: Any) -> None:
            text = " ".join(_format_value(a) for a in args)
            self.output.append(text)
            self._output_fn(text)

        def _print(*args: Any) -> None:
            text = " ".join(_format_value(a) for a in args)
            self.output.append(text)
            self._output_fn(text)

        def _len(val: Any) -> int:
            if isinstance(val, (str, list)):
                return len(val)
            raise KryosRuntimeError(f"len() not supported for {type(val).__name__}")

        def _range(*args: Any) -> list[int]:
            if len(args) == 1:
                return list(range(int(args[0])))
            elif len(args) == 2:
                return list(range(int(args[0]), int(args[1])))
            elif len(args) == 3:
                return list(range(int(args[0]), int(args[1]), int(args[2])))
            raise KryosRuntimeError("range() takes 1 to 3 arguments")

        def _to_string(val: Any) -> str:
            return _format_value(val)

        def _abs(val: Any) -> Any:
            if isinstance(val, (int, float)):
                return abs(val)
            raise KryosRuntimeError(f"abs() not supported for {type(val).__name__}")

        def _type_of(val: Any) -> str:
            if isinstance(val, bool):
                return "bool"
            if isinstance(val, int):
                return "i32"
            if isinstance(val, float):
                return "f64"
            if isinstance(val, str):
                return "str"
            if isinstance(val, list):
                return "array"
            if isinstance(val, KryosInstance):
                return val.struct.name
            if isinstance(val, KryosEnumValue):
                return val.enum.name
            if val is None:
                return "none"
            return "unknown"

        def _int(val: Any) -> int:
            return int(val)

        def _float(val: Any) -> float:
            return float(val)

        def _str(val: Any) -> str:
            return _format_value(val)

        def _push(arr: Any, val: Any) -> None:
            if not isinstance(arr, list):
                raise KryosRuntimeError("push() requires an array")
            arr.append(val)

        def _pop(arr: Any) -> Any:
            if not isinstance(arr, list):
                raise KryosRuntimeError("pop() requires an array")
            if len(arr) == 0:
                raise KryosRuntimeError("pop() on empty array")
            return arr.pop()

        def _char_code(ch: Any) -> int:
            if isinstance(ch, str) and len(ch) == 1:
                return ord(ch)
            if isinstance(ch, str) and len(ch) > 1:
                return ord(ch[0])
            return int(ch)

        def _char_from(code: Any) -> str:
            return chr(int(code))

        def _char_at(s: Any, idx: Any) -> str:
            return str(s)[int(idx)]

        def _substr(s: Any, start: Any, end: Any = None) -> str:
            s = str(s)
            if end is None:
                return s[int(start):]
            return s[int(start):int(end)]

        def _contains(s: Any, sub: Any) -> bool:
            return str(sub) in str(s)

        def _starts_with(s: Any, prefix: Any) -> bool:
            return str(s).startswith(str(prefix))

        def _ends_with(s: Any, suffix: Any) -> bool:
            return str(s).endswith(str(suffix))

        def _upper(s: Any) -> str:
            return str(s).upper()

        def _lower(s: Any) -> str:
            return str(s).lower()

        def _trim(s: Any) -> str:
            return str(s).strip()

        def _split(s: Any, sep: Any = " ") -> list:
            return str(s).split(str(sep))

        def _join(sep: Any, arr: Any) -> str:
            return str(sep).join(str(x) for x in arr)

        def _replace(s: Any, old: Any, new: Any) -> str:
            return str(s).replace(str(old), str(new))

        self.globals.define("char_code", _char_code)
        self.globals.define("char_from", _char_from)
        self.globals.define("char_at", _char_at)
        self.globals.define("substr", _substr)
        self.globals.define("contains", _contains)
        self.globals.define("starts_with", _starts_with)
        self.globals.define("ends_with", _ends_with)
        self.globals.define("upper", _upper)
        self.globals.define("lower", _lower)
        self.globals.define("trim", _trim)
        self.globals.define("split", _split)
        self.globals.define("join", _join)
        self.globals.define("replace", _replace)

        import math as _math

        def _sqrt(x: Any) -> float:
            return _math.sqrt(float(x))

        def _sin(x: Any) -> float:
            return _math.sin(float(x))

        def _cos(x: Any) -> float:
            return _math.cos(float(x))

        def _tan(x: Any) -> float:
            return _math.tan(float(x))

        def _log(x: Any) -> float:
            return _math.log(float(x))

        def _pow(x: Any, y: Any) -> float:
            return _math.pow(float(x), float(y))

        def _floor(x: Any) -> int:
            return int(_math.floor(float(x)))

        def _ceil(x: Any) -> int:
            return int(_math.ceil(float(x)))

        def _min(*args: Any) -> Any:
            if len(args) == 1 and isinstance(args[0], list):
                return min(args[0])
            return min(args)

        def _max(*args: Any) -> Any:
            if len(args) == 1 and isinstance(args[0], list):
                return max(args[0])
            return max(args)

        def _assert(condition: Any, msg: str = "assertion failed") -> None:
            if not condition:
                raise KryosRuntimeError(f"Assertion failed: {msg}")

        self.globals.define("println", _println)
        self.globals.define("print", _print)
        self.globals.define("len", _len)
        self.globals.define("range", _range)
        self.globals.define("to_string", _to_string)
        self.globals.define("abs", _abs)
        self.globals.define("type_of", _type_of)
        self.globals.define("int", _int)
        self.globals.define("float", _float)
        self.globals.define("str", _str)
        self.globals.define("push", _push)
        self.globals.define("pop", _pop)
        self.globals.define("sqrt", _sqrt)
        self.globals.define("sin", _sin)
        self.globals.define("cos", _cos)
        self.globals.define("tan", _tan)
        self.globals.define("log", _log)
        self.globals.define("pow", _pow)
        self.globals.define("floor", _floor)
        self.globals.define("ceil", _ceil)
        self.globals.define("min", _min)
        self.globals.define("max", _max)
        self.globals.define("assert", _assert)

        # ---- AI-native runtime ----
        self._setup_ai_runtime()

    def _setup_ai_runtime(self) -> None:
        """Register AI-native runtime modules as builtins."""
        from kryos.runtime.probable import Probable, Ensemble
        from kryos.runtime.agents import Agent, AgentSwarm, AlignmentMode
        from kryos.runtime.streams import Stream
        from kryos.runtime.lineage import Tracked
        from kryos.runtime.cost import CostTracker, Budget, ComputeCost

        # Probable<T> — probability type
        def _probable(value: Any, confidence: float = 1.0) -> Probable:
            return Probable(value=value, confidence=confidence)

        def _probable_from(options: Any) -> Probable:
            if isinstance(options, list):
                return Probable.uncertain([(v, p) for v, p in options])
            return Probable.certain(options)

        def _ensemble_vote(predictions: Any) -> Probable:
            return Ensemble.majority_vote(predictions)

        def _ensemble_weighted(predictions: Any) -> Probable:
            return Ensemble.weighted_average(predictions)

        # Agent — autonomous agent
        def _agent(name: str, goal: str = "", alignment: str = "unrestricted") -> Agent:
            mode_map = {
                "unrestricted": AlignmentMode.UNRESTRICTED,
                "minimal": AlignmentMode.MINIMAL,
                "standard": AlignmentMode.STANDARD,
                "strict": AlignmentMode.STRICT,
            }
            mode = mode_map.get(alignment.lower(), AlignmentMode.UNRESTRICTED)
            return Agent(name=name, goal=goal, alignment=mode)

        def _agent_swarm(name: str = "swarm") -> AgentSwarm:
            return AgentSwarm(name=name)

        # Stream
        def _stream(data: Any) -> Stream:
            if isinstance(data, list):
                return Stream.from_list(data)
            return Stream(data)

        def _stream_range(start: int, end: int) -> Stream:
            return Stream.from_range(start, end)

        # Lineage tracking
        def _tracked(value: Any, source: str = "user") -> Tracked:
            return Tracked.source(value, source)

        # Cost tracking
        def _budget(max_usd: float = 100.0, max_tokens: int = 1000000) -> Budget:
            return Budget(max_usd=max_usd, max_tokens=max_tokens)

        def _cost_tracker(budget: Any = None) -> CostTracker:
            return CostTracker(budget=budget)

        self.globals.define("Probable", _probable)
        self.globals.define("probable_from", _probable_from)
        self.globals.define("ensemble_vote", _ensemble_vote)
        self.globals.define("ensemble_weighted", _ensemble_weighted)
        self.globals.define("Agent", _agent)
        self.globals.define("AgentSwarm", _agent_swarm)
        self.globals.define("Stream", _stream)
        self.globals.define("stream_range", _stream_range)
        self.globals.define("Tracked", _tracked)
        self.globals.define("Budget", _budget)
        self.globals.define("CostTracker", _cost_tracker)

        # Tensor primitives — foundation for LLM ops
        from kryos.runtime.tensor import (
            KryosTensor, GradTensor,
            tensor_from_list, tensor_zeros, tensor_ones, tensor_rand,
            tensor_randn, tensor_eye, tensor_arange, tensor_matmul,
            tensor_softmax, tensor_relu, tensor_reshape, tensor_cat,
            tensor_sum, tensor_mean,
        )

        self.globals.define("Tensor", tensor_from_list)
        self.globals.define("tensor_zeros", tensor_zeros)
        self.globals.define("tensor_ones", tensor_ones)
        self.globals.define("tensor_rand", tensor_rand)
        self.globals.define("tensor_randn", tensor_randn)
        self.globals.define("tensor_eye", tensor_eye)
        self.globals.define("tensor_arange", tensor_arange)
        self.globals.define("tensor_matmul", tensor_matmul)
        self.globals.define("tensor_softmax", tensor_softmax)
        self.globals.define("tensor_relu", tensor_relu)
        self.globals.define("tensor_reshape", tensor_reshape)
        self.globals.define("tensor_cat", tensor_cat)
        self.globals.define("tensor_sum", tensor_sum)
        self.globals.define("tensor_mean", tensor_mean)
        self.globals.define("GradTensor", GradTensor.wrap)

        # ---- FFI — Foreign Function Interface ----
        self._setup_ffi_runtime()

    def _setup_ffi_runtime(self) -> None:
        """Register FFI builtins (Python & C interop)."""
        from kryos.runtime.ffi import get_registry

        registry = get_registry()

        # Python FFI (Community tier — ffi:python)
        def _py_import(module_name: str) -> Any:
            return registry.py_import(module_name)

        def _py_call(module: Any, fn_name: str, *args: Any) -> Any:
            return registry.py_call(module, fn_name, *args)

        def _py_attr(module: Any, attr_name: str) -> Any:
            return registry.py_attr(module, attr_name)

        # Native FFI (Pro tier — ffi:native)
        def _c_load(path: str) -> Any:
            return registry.c_load(path)

        def _c_call(lib: Any, fn_name: str, arg_types: Any, return_type: str, *args: Any) -> Any:
            if not isinstance(arg_types, list):
                arg_types = list(arg_types)
            return registry.c_call(lib, fn_name, arg_types, return_type, *args)

        self.globals.define("py_import", _py_import)
        self.globals.define("py_call", _py_call)
        self.globals.define("py_attr", _py_attr)
        self.globals.define("c_load", _c_load)
        self.globals.define("c_call", _c_call)

    def run(self, module: Module) -> None:
        """Execute a parsed module."""
        for decl in module.declarations:
            self._exec(decl, self.globals)

    def interpret(self, module: Module) -> None:
        """Alias for run()."""
        return self.run(module)

    def _exec(self, node: ASTNode, env: Environment) -> Any:
        """Execute a statement or declaration."""
        method_name = f"_exec_{type(node).__name__}"
        method = getattr(self, method_name, None)
        if method is None:
            raise KryosRuntimeError(
                f"unsupported AST node: {type(node).__name__}",
                getattr(node, "span", (0, 0, 0, 0))[0],
            )
        return method(node, env)

    def _eval(self, node: ASTNode, env: Environment) -> Any:
        """Evaluate an expression."""
        method_name = f"_eval_{type(node).__name__}"
        method = getattr(self, method_name, None)
        if method is None:
            raise KryosRuntimeError(
                f"unsupported expression: {type(node).__name__}",
                getattr(node, "span", (0, 0, 0, 0))[0],
            )
        return method(node, env)

    # ------------------------------------------------------------------
    # Declarations
    # ------------------------------------------------------------------

    def _exec_FnDecl(self, node: FnDecl, env: Environment) -> None:
        fn = KryosFunction(node.name, node.params, node.body, env, node.attributes)
        env.define(node.name, fn)

        # Extract self-healing metadata from attributes
        if self.self_healing and node.attributes:
            self._register_fn_intent(node.name, node.attributes, env)

    def _exec_StructDecl(self, node: StructDecl, env: Environment) -> None:
        field_names = [f.name for f in node.fields]
        struct = KryosStruct(node.name, field_names)
        env.define(node.name, struct)

    def _exec_EnumDecl(self, node: EnumDecl, env: Environment) -> None:
        variants: dict[str, int] = {}
        for v in node.variants:
            variants[v.name] = len(v.fields)

        enum = KryosEnum(node.name, variants)
        env.define(node.name, enum)

        # Create variant constructors / values in the environment
        for v_name, field_count in variants.items():
            if field_count == 0:
                # Unit variant -- store directly as a KryosEnumValue
                env.define(f"{node.name}::{v_name}", KryosEnumValue(enum, v_name))
            else:
                # Variant with fields -- store a callable constructor
                def _make_constructor(_enum: KryosEnum, _vname: str, _fc: int) -> Callable:
                    def _constructor(*args: Any) -> KryosEnumValue:
                        if len(args) != _fc:
                            raise KryosRuntimeError(
                                f"enum variant '{_enum.name}::{_vname}' expects "
                                f"{_fc} fields, got {len(args)}"
                            )
                        return KryosEnumValue(_enum, _vname, list(args))
                    return _constructor
                env.define(
                    f"{node.name}::{v_name}",
                    _make_constructor(enum, v_name, field_count),
                )

    def _exec_TraitDecl(self, node: TraitDecl, env: Environment) -> None:
        method_names = [m.name for m in node.methods]
        trait = KryosTrait(node.name, method_names)
        env.define(node.name, trait)

    def _exec_ImplBlock(self, node: ImplBlock, env: Environment) -> None:
        # Resolve target type name from the TypeNode
        target_name = getattr(node.target_type, "name", None)
        if target_name is None:
            raise KryosRuntimeError("impl block has no valid target type")

        if target_name not in self._impl_methods:
            self._impl_methods[target_name] = {}

        # Register each method as a KryosFunction
        for method_decl in node.methods:
            fn = KryosFunction(
                method_decl.name, method_decl.params, method_decl.body,
                env, method_decl.attributes,
            )
            self._impl_methods[target_name][method_decl.name] = fn

        # Track trait implementations
        if node.trait_type is not None:
            trait_name = getattr(node.trait_type, "name", None)
            if trait_name:
                if target_name not in self._impl_traits:
                    self._impl_traits[target_name] = []
                self._impl_traits[target_name].append(trait_name)

    # ------------------------------------------------------------------
    # Statements
    # ------------------------------------------------------------------

    def _exec_ExprStmt(self, node: ExprStmt, env: Environment) -> Any:
        return self._eval(node.expression, env)

    def _exec_BlockStmt(self, node: BlockStmt, env: Environment) -> Any:
        block_env = Environment(parent=env)
        result = None
        for stmt in node.statements:
            result = self._exec(stmt, block_env)
        return result

    def _exec_LetStmt(self, node: LetStmt, env: Environment) -> None:
        value = self._eval(node.value, env) if node.value else None
        env.define(node.name, value, mutable=node.mutable)

    def _exec_AssignStmt(self, node: AssignStmt, env: Environment) -> None:
        value = self._eval(node.value, env)

        if isinstance(node.target, Identifier):
            if node.operator == "=":
                env.set(node.target.name, value)
            else:
                current = env.get(node.target.name)
                value = self._apply_compound_op(node.operator, current, value)
                env.set(node.target.name, value)
        elif isinstance(node.target, FieldAccess):
            obj = self._eval(node.target.object, env)
            if isinstance(obj, KryosInstance):
                if node.operator == "=":
                    obj.fields[node.target.field] = value
                else:
                    current = obj.fields[node.target.field]
                    obj.fields[node.target.field] = self._apply_compound_op(
                        node.operator, current, value
                    )
            else:
                raise KryosRuntimeError("cannot assign to field of non-struct")
        elif isinstance(node.target, IndexAccess):
            obj = self._eval(node.target.object, env)
            idx = self._eval(node.target.index, env)
            if isinstance(obj, list):
                if node.operator == "=":
                    obj[int(idx)] = value
                else:
                    current = obj[int(idx)]
                    obj[int(idx)] = self._apply_compound_op(
                        node.operator, current, value
                    )
            else:
                raise KryosRuntimeError("cannot index into non-array")
        else:
            raise KryosRuntimeError("invalid assignment target")

    def _apply_compound_op(self, op: str, left: Any, right: Any) -> Any:
        match op:
            case "+=":
                return left + right
            case "-=":
                return left - right
            case "*=":
                return left * right
            case "/=":
                return left / right
            case "%=":
                return left % right
            case _:
                raise KryosRuntimeError(f"unsupported compound operator: {op}")

    def _exec_ReturnStmt(self, node: ReturnStmt, env: Environment) -> None:
        value = self._eval(node.value, env) if node.value else None
        raise ReturnSignal(value)

    def _exec_IfStmt(self, node: IfStmt, env: Environment) -> None:
        condition = self._eval(node.condition, env)
        if self._is_truthy(condition):
            self._exec(node.body, env)
            return

        for elif_clause in node.elif_clauses:
            condition = self._eval(elif_clause.condition, env)
            if self._is_truthy(condition):
                self._exec(elif_clause.body, env)
                return

        if node.else_body:
            self._exec(node.else_body, env)

    def _exec_WhileStmt(self, node: WhileStmt, env: Environment) -> None:
        while self._is_truthy(self._eval(node.condition, env)):
            try:
                self._exec(node.body, env)
            except BreakSignal:
                break
            except ContinueSignal:
                continue

    def _exec_ForStmt(self, node: ForStmt, env: Environment) -> None:
        iterable = self._eval(node.iterable, env)
        if not hasattr(iterable, "__iter__"):
            raise KryosRuntimeError("for loop target is not iterable")

        for item in iterable:
            loop_env = Environment(parent=env)
            loop_env.define(node.variable, item, mutable=True)
            try:
                self._exec_block_in_env(node.body, loop_env)
            except BreakSignal:
                break
            except ContinueSignal:
                continue

    def _exec_block_in_env(self, block: BlockStmt, env: Environment) -> Any:
        """Execute block statements in the given env (no new scope)."""
        result = None
        for stmt in block.statements:
            result = self._exec(stmt, env)
        return result

    def _exec_BreakStmt(self, node: BreakStmt, env: Environment) -> None:
        raise BreakSignal()

    def _exec_ContinueStmt(self, node: ContinueStmt, env: Environment) -> None:
        raise ContinueSignal()

    # ------------------------------------------------------------------
    # Expressions
    # ------------------------------------------------------------------

    def _eval_IntLiteral(self, node: IntLiteral, env: Environment) -> int:
        return node.value

    def _eval_FloatLiteral(self, node: FloatLiteral, env: Environment) -> float:
        return node.value

    def _eval_StringLiteral(self, node: StringLiteral, env: Environment) -> str:
        return node.value

    def _eval_CharLiteral(self, node: CharLiteral, env: Environment) -> str:
        return node.value

    def _eval_BoolLiteral(self, node: BoolLiteral, env: Environment) -> bool:
        return node.value

    def _eval_NoneLiteral(self, node: NoneLiteral, env: Environment) -> None:
        return None

    def _eval_Identifier(self, node: Identifier, env: Environment) -> Any:
        return env.get(node.name)

    def _eval_BinaryOp(self, node: BinaryOp, env: Environment) -> Any:
        # Short-circuit for logical operators
        if node.operator == "and":
            left = self._eval(node.left, env)
            if not self._is_truthy(left):
                return left
            return self._eval(node.right, env)
        if node.operator == "or":
            left = self._eval(node.left, env)
            if self._is_truthy(left):
                return left
            return self._eval(node.right, env)

        left = self._eval(node.left, env)
        right = self._eval(node.right, env)

        try:
            return self._binary_op(node.operator, left, right)
        except (TypeError, KryosRuntimeError) as e:
            if self.self_healing and self._heal_engine:
                return self._heal_binary_op(node.operator, left, right, e)
            raise

    def _binary_op(self, op: str, left: Any, right: Any) -> Any:
        """Core binary operation logic."""
        match op:
            case "+":
                return left + right
            case "-":
                return left - right
            case "*":
                return left * right
            case "/":
                if right == 0:
                    raise KryosRuntimeError("division by zero")
                if isinstance(left, int) and isinstance(right, int):
                    return left // right
                return left / right
            case "%":
                if right == 0:
                    raise KryosRuntimeError("modulo by zero")
                return left % right
            case "**":
                return left ** right
            case "==":
                return left == right
            case "!=":
                return left != right
            case "<":
                return left < right
            case ">":
                return left > right
            case "<=":
                return left <= right
            case ">=":
                return left >= right
            case "&":
                return left & right
            case "|":
                return left | right
            case "^":
                return left ^ right
            case "<<":
                return left << right
            case ">>":
                return left >> right
            case _:
                raise KryosRuntimeError(f"unknown operator: {op}")

    def _heal_binary_op(self, op: str, left: Any, right: Any, error: Exception) -> Any:
        """Self-heal a failed binary operation."""
        from kryos.compiler.self_heal import HealAction

        # Division/modulo by zero → return 0
        if op in ("/", "%") and right == 0:
            self._heal_engine._log_heal(
                HealAction.SUBSTITUTE, f"binary '{op}'",
                str(error), "substituted 0 for division by zero", 0,
            )
            return 0

        # Type mismatch → try coercion
        if isinstance(left, str) and isinstance(right, (int, float)):
            try:
                result = self._binary_op(op, left, str(right))
                self._heal_engine._log_heal(
                    HealAction.COERCE, f"binary '{op}'",
                    str(error), f"coerced right operand to str", result,
                )
                return result
            except Exception:
                pass

        if isinstance(right, str) and isinstance(left, (int, float)):
            try:
                result = self._binary_op(op, str(left), right)
                self._heal_engine._log_heal(
                    HealAction.COERCE, f"binary '{op}'",
                    str(error), f"coerced left operand to str", result,
                )
                return result
            except Exception:
                pass

        # Number string + number → numeric operation
        if isinstance(left, str) and op in ("+", "-", "*", "/"):
            try:
                left_num = float(left) if "." in left else int(left)
                result = self._binary_op(op, left_num, right)
                self._heal_engine._log_heal(
                    HealAction.COERCE, f"binary '{op}'",
                    str(error), f"coerced string '{left}' to number", result,
                )
                return result
            except (ValueError, KryosRuntimeError):
                pass

        raise error

    def _eval_UnaryOp(self, node: UnaryOp, env: Environment) -> Any:
        operand = self._eval(node.operand, env)
        match node.operator:
            case "-":
                return -operand
            case "not" | "!":
                return not self._is_truthy(operand)
            case "~":
                return ~operand
            case _:
                raise KryosRuntimeError(f"unknown unary operator: {node.operator}")

    def _eval_FnCall(self, node: FnCall, env: Environment) -> Any:
        callee = self._eval(node.callee, env)
        args = [self._eval(a, env) for a in node.args]

        if callable(callee) and not isinstance(callee, (KryosFunction, KryosStruct, KryosEnum)):
            # Built-in Python function (includes enum variant constructors)
            return callee(*args)

        if isinstance(callee, KryosStruct):
            # Struct constructor call -- shouldn't normally happen via FnCall
            # but handle it gracefully
            fields = {}
            for i, name in enumerate(callee.field_names):
                if i < len(args):
                    fields[name] = args[i]
            return KryosInstance(callee, fields)

        if isinstance(callee, KryosFunction):
            if self.self_healing and callee.name in self._fn_intents:
                return self._call_with_healing(callee, args)
            return self._call_function(callee, args)

        raise KryosRuntimeError(
            f"'{_format_value(callee)}' is not callable"
        )

    def _call_function(self, fn: KryosFunction, args: list[Any]) -> Any:
        if len(args) != len(fn.params):
            raise KryosRuntimeError(
                f"function '{fn.name}' expects {len(fn.params)} arguments, "
                f"got {len(args)}"
            )

        call_env = Environment(parent=fn.closure)
        for param, arg in zip(fn.params, args):
            call_env.define(param.name, arg, mutable=True)

        try:
            self._exec_block_in_env(fn.body, call_env)
        except ReturnSignal as ret:
            return ret.value

        return None

    def _eval_MethodCall(self, node: MethodCall, env: Environment) -> Any:
        obj = self._eval(node.object, env)
        args = [self._eval(a, env) for a in node.args]

        # String methods
        if isinstance(obj, str):
            match node.method:
                case "len":
                    return len(obj)
                case "upper":
                    return obj.upper()
                case "lower":
                    return obj.lower()
                case "trim":
                    return obj.strip()
                case "contains":
                    return args[0] in obj if args else False
                case "split":
                    return obj.split(args[0]) if args else obj.split()
                case _:
                    raise KryosRuntimeError(
                        f"unknown string method '{node.method}'"
                    )

        # Array methods
        if isinstance(obj, list):
            match node.method:
                case "len":
                    return len(obj)
                case "push":
                    obj.append(args[0])
                    return None
                case "pop":
                    return obj.pop()
                case "contains":
                    return args[0] in obj if args else False
                case _:
                    raise KryosRuntimeError(
                        f"unknown array method '{node.method}'"
                    )

        # Impl'd methods on struct/enum instances
        if isinstance(obj, KryosInstance):
            type_name = obj.struct.name
            methods = self._impl_methods.get(type_name, {})
            if node.method in methods:
                fn = methods[node.method]
                return self._call_function(fn, [obj] + args)

        if isinstance(obj, KryosEnumValue):
            type_name = obj.enum.name
            methods = self._impl_methods.get(type_name, {})
            if node.method in methods:
                fn = methods[node.method]
                return self._call_function(fn, [obj] + args)

        raise KryosRuntimeError(
            f"cannot call method '{node.method}' on {type(obj).__name__}"
        )

    def _eval_FieldAccess(self, node: FieldAccess, env: Environment) -> Any:
        obj = self._eval(node.object, env)
        if isinstance(obj, KryosInstance):
            if node.field in obj.fields:
                return obj.fields[node.field]
            raise KryosRuntimeError(
                f"struct '{obj.struct.name}' has no field '{node.field}'"
            )
        # Enum namespace access: Color.Red -> lookup Color::Red
        if isinstance(obj, KryosEnum):
            if node.field in obj.variants:
                return env.get(f"{obj.name}::{node.field}")
            raise KryosRuntimeError(
                f"enum '{obj.name}' has no variant '{node.field}'"
            )
        raise KryosRuntimeError("field access on non-struct value")

    def _eval_IndexAccess(self, node: IndexAccess, env: Environment) -> Any:
        obj = self._eval(node.object, env)
        idx = self._eval(node.index, env)
        if isinstance(obj, (list, str)):
            try:
                return obj[int(idx)]
            except IndexError:
                if self.self_healing and self._heal_engine:
                    from kryos.compiler.self_heal import HealAction
                    # Clamp index to valid range
                    clamped = max(0, min(int(idx), len(obj) - 1))
                    self._heal_engine._log_heal(
                        HealAction.CLAMP, "index_access",
                        f"index {idx} out of bounds (len={len(obj)})",
                        f"clamped to {clamped}",
                        obj[clamped],
                    )
                    return obj[clamped]
                raise KryosRuntimeError("index out of bounds")
        raise KryosRuntimeError("index access on non-indexable value")

    def _eval_ArrayLiteral(self, node: ArrayLiteral, env: Environment) -> list:
        return [self._eval(e, env) for e in node.elements]

    def _eval_StructLiteral(self, node: StructLiteral, env: Environment) -> KryosInstance:
        struct = env.get(node.type_name)
        if not isinstance(struct, KryosStruct):
            raise KryosRuntimeError(f"'{node.type_name}' is not a struct")
        fields = {}
        for name, expr in node.field_values:
            fields[name] = self._eval(expr, env)
        return KryosInstance(struct, fields)

    def _eval_Lambda(self, node: Lambda, env: Environment) -> KryosFunction:
        body = node.body
        if not isinstance(body, BlockStmt):
            # Wrap expression body in a return + block
            from kryos.compiler.ast_nodes import ReturnStmt as RetNode, BlockStmt as BStmt
            ret = RetNode(value=body, span=node.span)
            body = BStmt(statements=[ret], span=node.span)
        return KryosFunction("<lambda>", node.params, body, env)

    def _eval_RangeExpr(self, node: RangeExpr, env: Environment) -> list[int]:
        start = self._eval(node.start, env) if node.start else 0
        end = self._eval(node.end, env)
        if node.inclusive:
            return list(range(int(start), int(end) + 1))
        return list(range(int(start), int(end)))

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _is_truthy(value: Any) -> bool:
        if value is None:
            return False
        if isinstance(value, bool):
            return value
        if isinstance(value, int):
            return value != 0
        if isinstance(value, float):
            return value != 0.0
        if isinstance(value, str):
            return len(value) > 0
        if isinstance(value, list):
            return len(value) > 0
        return True

    # ------------------------------------------------------------------
    # Self-Healing Integration
    # ------------------------------------------------------------------

    def _register_fn_intent(self, name: str, attributes: list[Attribute], env: Environment) -> None:
        """Extract @intent, @constraint, @fallback from function attributes."""
        from kryos.compiler.self_heal import Intent
        intent_desc = ""
        constraints = []
        fallback_names = []

        for attr in attributes:
            if attr.name == "intent" and attr.args:
                # @intent("description")
                intent_desc = self._eval(attr.args[0], env) if attr.args else ""
            elif attr.name == "constraint" and attr.args:
                # @constraint(">= 0", "<= 100")
                for arg in attr.args:
                    constraints.append(self._eval(arg, env))
            elif attr.name == "fallback" and attr.args:
                # @fallback(other_fn)
                for arg in attr.args:
                    fallback_names.append(self._eval(arg, env))

        if intent_desc or constraints:
            intent = Intent(
                description=intent_desc,
                constraints=constraints,
            )
            self._fn_intents[name] = intent
            self._heal_engine.register_intent(name, intent)

    def _call_with_healing(self, fn: KryosFunction, args: list[Any]) -> Any:
        """Call a function with self-healing active."""
        intent = self._fn_intents.get(fn.name)

        if not intent:
            return self._call_function(fn, args)

        # Save pre-call state
        self._heal_engine.snapshot(fn.name, args)

        try:
            result = self._call_function(fn, args)
        except Exception as e:
            # Try self-healing
            fixed = self._heal_engine._try_auto_fix(e, lambda *a: self._call_function(fn, list(a)), args, fn.name)
            if fixed is not None:
                result = fixed
            else:
                raise

        # Enforce constraints on result
        if intent.constraints:
            for constraint in intent.constraints:
                result = self._heal_engine._enforce_constraint(result, constraint, fn.name)

        return result

    def get_heal_report(self) -> str:
        """Get a report of all self-healing actions taken during execution."""
        if self._heal_engine:
            return self._heal_engine.get_heal_report()
        return "Self-healing disabled."

    @property
    def heal_count(self) -> int:
        """Number of self-healing actions taken."""
        if self._heal_engine:
            return len(self._heal_engine.heal_log)
        return 0
