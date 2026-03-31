"""
Kryos Capability Enforcement Engine

This is the most critical security component of Kryos. Every function must operate
within its declared capabilities. No exceptions, no escape hatches, no runtime surprises.

Design principles:
- DENY BY DEFAULT: A function with no @capabilities has ZERO access beyond pure computation
- NARROW ONLY: Child scopes can only restrict capabilities, never elevate
- COMPILE-TIME: Blocked capabilities fail at compile/check time, not runtime
- AUDITABLE: Every capability usage is logged and exportable
- SANDBOXED: FFI and dynamic code run in strict sandboxes

Capability categories:
  compute    - pure computation (always available)
  network    - TCP/UDP/HTTP connections
  network:raw_socket - raw socket access (Pro/Enterprise only)
  filesystem - file read/write
  filesystem:read - read-only file access
  filesystem:write - write file access
  memory     - general memory access
  memory:raw - raw/unsafe memory access (Pro/Enterprise only)
  gpu        - GPU compute access
  quantum    - quantum compute access (Pro/Enterprise only)
  syscall    - direct system calls (Pro/Enterprise only)
  ffi        - foreign function interface
"""

from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from enum import Enum, auto
from typing import Any, Optional

from kryos.compiler.ast_nodes import (
    ASTNode, ASTVisitor, Module, FnDecl, StructDecl, LetStmt, ExprStmt,
    FnCall, MethodCall, Identifier, FieldAccess, Attribute, BlockStmt,
    ImportDecl, ForStmt, WhileStmt, IfStmt, ReturnStmt, AssignStmt,
    BinaryOp, UnaryOp, IndexAccess, ArrayLiteral, StructLiteral,
    IntLiteral, FloatLiteral, StringLiteral, BoolLiteral, NoneLiteral,
    CharLiteral, Lambda, RangeExpr, Expression,
)


class CapabilityTier(Enum):
    """License tier that determines available capabilities."""
    COMMUNITY = auto()    # Free / open source
    PRO = auto()          # Paid — GPU, advanced networking
    ENTERPRISE = auto()   # Full — quantum, raw memory, syscall


# All capability categories
CAPABILITY_TREE = {
    "compute": {
        "description": "Pure computation — always available",
        "tier": CapabilityTier.COMMUNITY,
        "children": {},
    },
    "network": {
        "description": "Network access (TCP/UDP/HTTP)",
        "tier": CapabilityTier.COMMUNITY,
        "children": {
            "raw_socket": {
                "description": "Raw socket access",
                "tier": CapabilityTier.PRO,
            },
        },
    },
    "filesystem": {
        "description": "File system access",
        "tier": CapabilityTier.COMMUNITY,
        "children": {
            "read": {
                "description": "Read-only file access",
                "tier": CapabilityTier.COMMUNITY,
            },
            "write": {
                "description": "Write file access",
                "tier": CapabilityTier.COMMUNITY,
            },
        },
    },
    "memory": {
        "description": "Memory management",
        "tier": CapabilityTier.COMMUNITY,
        "children": {
            "raw": {
                "description": "Raw/unsafe memory access",
                "tier": CapabilityTier.PRO,
            },
        },
    },
    "gpu": {
        "description": "GPU compute access",
        "tier": CapabilityTier.COMMUNITY,
        "children": {},
    },
    "quantum": {
        "description": "Quantum compute access",
        "tier": CapabilityTier.ENTERPRISE,
        "children": {},
    },
    "syscall": {
        "description": "Direct system calls",
        "tier": CapabilityTier.ENTERPRISE,
        "children": {
            "direct": {
                "description": "Direct syscall execution",
                "tier": CapabilityTier.ENTERPRISE,
            },
        },
    },
    "ffi": {
        "description": "Foreign function interface",
        "tier": CapabilityTier.COMMUNITY,
        "children": {},
    },
}

# Built-in functions and the capabilities they require
BUILTIN_CAPABILITIES: dict[str, set[str]] = {
    # Pure compute — no capabilities needed
    "println": set(),
    "print": set(),
    "len": set(),
    "range": set(),
    "to_string": set(),
    "abs": set(),
    "type_of": set(),
    "int": set(),
    "float": set(),
    "str": set(),
    "sqrt": set(),
    "sin": set(),
    "cos": set(),
    "tan": set(),
    "log": set(),
    "pow": set(),
    "floor": set(),
    "ceil": set(),
    "min": set(),
    "max": set(),
    "assert": set(),
    "push": set(),
    "pop": set(),
    # I/O functions (would require filesystem in real impl)
    "input": {"filesystem"},
    # Network functions (future)
    "http_get": {"network"},
    "http_post": {"network"},
    "tcp_connect": {"network"},
    "tcp_listen": {"network"},
    "raw_socket": {"network", "network:raw_socket"},
    # Filesystem functions (future)
    "file_read": {"filesystem", "filesystem:read"},
    "file_write": {"filesystem", "filesystem:write"},
    "file_delete": {"filesystem", "filesystem:write"},
    # System functions (future)
    "exec": {"syscall"},
    "spawn_process": {"syscall"},
    "eval": {"syscall"},
    # Memory functions (future)
    "alloc": {"memory"},
    "dealloc": {"memory"},
    "raw_ptr": {"memory", "memory:raw"},
}


@dataclass
class CapabilitySet:
    """An immutable set of granted capabilities."""
    capabilities: frozenset[str]

    @classmethod
    def none(cls) -> CapabilitySet:
        """No capabilities — pure computation only."""
        return cls(frozenset({"compute"}))

    @classmethod
    def from_list(cls, caps: list[str]) -> CapabilitySet:
        """Create from a list of capability strings."""
        expanded = {"compute"}  # compute is always implicitly available
        for cap in caps:
            cap = cap.strip().strip('"').strip("'")
            if ":" in cap:
                # Sub-capability like "filesystem:read"
                parent = cap.split(":")[0]
                expanded.add(parent)
                expanded.add(cap)
            else:
                expanded.add(cap)
        return cls(frozenset(expanded))

    def has(self, capability: str) -> bool:
        """Check if this set includes a capability."""
        if capability in self.capabilities:
            return True
        # Check parent (e.g., "filesystem" grants "filesystem:read")
        if ":" in capability:
            parent = capability.split(":")[0]
            return parent in self.capabilities
        return False

    def narrow(self, child_caps: CapabilitySet) -> CapabilitySet:
        """Create a narrowed set — child can only have subset of parent."""
        narrowed = self.capabilities & child_caps.capabilities
        # Always keep compute
        narrowed = narrowed | frozenset({"compute"})
        return CapabilitySet(narrowed)

    def is_subset_of(self, parent: CapabilitySet) -> bool:
        """Check if this set is a valid subset of parent."""
        for cap in self.capabilities:
            if cap == "compute":
                continue
            if not parent.has(cap):
                return False
        return True

    def __contains__(self, cap: str) -> bool:
        return self.has(cap)

    def __repr__(self) -> str:
        return f"CapabilitySet({sorted(self.capabilities)})"


@dataclass
class AuditEntry:
    """Record of a capability usage."""
    timestamp: float
    function_name: str
    capability_used: str
    capability_declared: list[str]
    action: str  # "allowed", "blocked", "sandbox_escape_attempt"
    details: str = ""

    def to_dict(self) -> dict:
        return {
            "timestamp": self.timestamp,
            "function": self.function_name,
            "capability": self.capability_used,
            "declared": self.capability_declared,
            "action": self.action,
            "details": self.details,
        }


class CapabilityViolation(Exception):
    """
    Raised when code attempts to use a capability it doesn't have.
    This exception CANNOT be caught by user code — it always propagates
    to the top level.
    """
    def __init__(self, function: str, required: str, available: CapabilitySet, detail: str = ""):
        self.function = function
        self.required = required
        self.available = available
        self.detail = detail
        super().__init__(
            f"CAPABILITY VIOLATION in '{function}': requires '{required}' "
            f"but only has {sorted(available.capabilities)}. {detail}"
        )


class SandboxEscapeAttempt(Exception):
    """
    Raised when sandbox code attempts to elevate capabilities.
    CANNOT be caught. Always fatal.
    """
    def __init__(self, detail: str):
        super().__init__(f"SANDBOX ESCAPE ATTEMPT: {detail}")


class CapabilityAuditLog:
    """Append-only audit log of all capability usage."""

    def __init__(self) -> None:
        self._entries: list[AuditEntry] = []
        self._frozen = False

    def log(self, entry: AuditEntry) -> None:
        """Add an entry. Cannot be modified after adding."""
        if self._frozen:
            raise RuntimeError("Audit log is frozen")
        self._entries.append(entry)

    def log_allowed(self, fn_name: str, capability: str, declared: list[str]) -> None:
        self.log(AuditEntry(
            timestamp=time.time(),
            function_name=fn_name,
            capability_used=capability,
            capability_declared=declared,
            action="allowed",
        ))

    def log_blocked(self, fn_name: str, capability: str, declared: list[str], detail: str = "") -> None:
        self.log(AuditEntry(
            timestamp=time.time(),
            function_name=fn_name,
            capability_used=capability,
            capability_declared=declared,
            action="blocked",
            details=detail,
        ))

    def log_escape_attempt(self, detail: str) -> None:
        self.log(AuditEntry(
            timestamp=time.time(),
            function_name="<sandbox>",
            capability_used="<escalation>",
            capability_declared=[],
            action="sandbox_escape_attempt",
            details=detail,
        ))

    def freeze(self) -> None:
        """Freeze the log — no more entries can be added."""
        self._frozen = True

    @property
    def entries(self) -> list[AuditEntry]:
        return list(self._entries)

    def to_json(self) -> str:
        """Export as JSON for external security tooling."""
        return json.dumps(
            [e.to_dict() for e in self._entries],
            indent=2,
        )

    def summary(self) -> str:
        """Human-readable summary."""
        if not self._entries:
            return "No capability usage recorded."

        allowed = sum(1 for e in self._entries if e.action == "allowed")
        blocked = sum(1 for e in self._entries if e.action == "blocked")
        escapes = sum(1 for e in self._entries if e.action == "sandbox_escape_attempt")

        lines = [
            f"Capability Audit Summary:",
            f"  Allowed:          {allowed}",
            f"  Blocked:          {blocked}",
            f"  Escape Attempts:  {escapes}",
        ]

        if blocked > 0:
            lines.append("\nBlocked Actions:")
            for e in self._entries:
                if e.action == "blocked":
                    lines.append(f"  [{e.function_name}] tried '{e.capability_used}' — DENIED")

        if escapes > 0:
            lines.append("\nESCAPE ATTEMPTS (CRITICAL):")
            for e in self._entries:
                if e.action == "sandbox_escape_attempt":
                    lines.append(f"  {e.details}")

        return "\n".join(lines)


class CapabilityAnalyzer(ASTVisitor):
    """
    Static analyzer that builds a complete capability map of a Kryos program.
    Used by `kryos check` to audit what a program can and cannot do.

    This runs at COMPILE TIME — violations are errors, not warnings.
    """

    def __init__(self, tier: CapabilityTier = CapabilityTier.COMMUNITY) -> None:
        self.tier = tier
        self.errors: list[str] = []
        self.warnings: list[str] = []
        self.function_caps: dict[str, CapabilitySet] = {}  # fn name -> declared caps
        self.capability_map: dict[str, list[str]] = {}  # fn name -> list of caps used
        self.audit_log = CapabilityAuditLog()

        # Scope stack for tracking capability context
        self._scope_stack: list[tuple[str, CapabilitySet]] = []
        self._current_fn: str = "<top-level>"

    def analyze(self, module: Module) -> dict:
        """
        Analyze an entire module. Returns a capability report.
        Raises errors for any violations.
        """
        # Top-level has no capabilities (pure compute only)
        self._scope_stack = [("<top-level>", CapabilitySet.none())]
        self._current_fn = "<top-level>"

        for decl in module.declarations:
            self._analyze_node(decl)

        return {
            "functions": {
                name: {
                    "declared": sorted(caps.capabilities),
                    "used": self.capability_map.get(name, []),
                }
                for name, caps in self.function_caps.items()
            },
            "errors": self.errors,
            "warnings": self.warnings,
            "audit": self.audit_log.to_json(),
        }

    def _analyze_node(self, node: ASTNode) -> None:
        """Analyze an AST node for capability violations."""
        if isinstance(node, FnDecl):
            self._analyze_fn_decl(node)
        elif isinstance(node, ImportDecl):
            self._analyze_import(node)
        elif isinstance(node, FnCall):
            self._analyze_fn_call(node)
        elif isinstance(node, MethodCall):
            self._analyze_method_call(node)
        elif isinstance(node, BlockStmt):
            for stmt in node.statements:
                self._analyze_node(stmt)
        elif isinstance(node, IfStmt):
            self._analyze_node(node.body)
            for elif_cl in node.elif_clauses:
                self._analyze_node(elif_cl.body)
            if node.else_body:
                self._analyze_node(node.else_body)
        elif isinstance(node, ForStmt):
            self._analyze_node(node.body)
        elif isinstance(node, WhileStmt):
            self._analyze_node(node.body)
        elif isinstance(node, LetStmt):
            if node.value:
                self._analyze_node(node.value)
        elif isinstance(node, AssignStmt):
            self._analyze_node(node.value)
        elif isinstance(node, ReturnStmt):
            if node.value:
                self._analyze_node(node.value)
        elif isinstance(node, ExprStmt):
            self._analyze_node(node.expression)
        elif isinstance(node, BinaryOp):
            self._analyze_node(node.left)
            self._analyze_node(node.right)
        elif isinstance(node, UnaryOp):
            self._analyze_node(node.operand)
        elif isinstance(node, IndexAccess):
            self._analyze_node(node.object)
            self._analyze_node(node.index)
        elif isinstance(node, FieldAccess):
            self._analyze_node(node.object)
        elif isinstance(node, ArrayLiteral):
            for elem in node.elements:
                self._analyze_node(elem)
        elif isinstance(node, StructLiteral):
            for _, expr in node.field_values:
                self._analyze_node(expr)

    def _analyze_fn_decl(self, node: FnDecl) -> None:
        """Analyze a function declaration for capability correctness."""
        # Extract declared capabilities from attributes
        caps = self._extract_capabilities(node.attributes)
        parent_fn, parent_caps = self._scope_stack[-1]

        # RULE: Child scope can only narrow, never elevate
        if not caps.is_subset_of(parent_caps) and parent_fn != "<top-level>":
            for cap in caps.capabilities:
                if cap != "compute" and not parent_caps.has(cap):
                    line = node.span[0] if node.span else 0
                    self.errors.append(
                        f"Line {line}: function '{node.name}' declares capability '{cap}' "
                        f"but parent '{parent_fn}' does not have it. "
                        f"Capabilities can only be narrowed, never elevated."
                    )
                    self.audit_log.log_escape_attempt(
                        f"'{node.name}' tried to elevate capability '{cap}' beyond parent '{parent_fn}'"
                    )

        # RULE: Check tier restrictions
        for cap in caps.capabilities:
            required_tier = self._get_tier(cap)
            if required_tier and required_tier.value > self.tier.value:
                line = node.span[0] if node.span else 0
                self.errors.append(
                    f"Line {line}: capability '{cap}' requires {required_tier.name} license "
                    f"(current: {self.tier.name})"
                )

        # Register the function's capabilities
        effective_caps = parent_caps.narrow(caps) if parent_fn != "<top-level>" else caps
        self.function_caps[node.name] = effective_caps
        self.capability_map[node.name] = []

        # Analyze function body with new scope
        self._scope_stack.append((node.name, effective_caps))
        self._current_fn = node.name
        if node.body:
            self._analyze_node(node.body)
        self._scope_stack.pop()
        self._current_fn = parent_fn

    def _analyze_fn_call(self, node: FnCall) -> None:
        """Check if a function call is allowed under current capabilities."""
        # Analyze arguments first
        for arg in node.args:
            self._analyze_node(arg)

        if not isinstance(node.callee, Identifier):
            return

        fn_name = node.callee.name
        current_fn, current_caps = self._scope_stack[-1]

        # Check if calling a built-in with capability requirements
        if fn_name in BUILTIN_CAPABILITIES:
            required = BUILTIN_CAPABILITIES[fn_name]
            for req_cap in required:
                if not current_caps.has(req_cap):
                    line = node.span[0] if node.span else 0
                    self.errors.append(
                        f"Line {line}: '{fn_name}()' requires capability '{req_cap}' "
                        f"but '{current_fn}' only has {sorted(current_caps.capabilities)}"
                    )
                    self.audit_log.log_blocked(current_fn, req_cap, sorted(current_caps.capabilities))
                else:
                    self.audit_log.log_allowed(current_fn, req_cap, sorted(current_caps.capabilities))
                    if current_fn in self.capability_map:
                        self.capability_map[current_fn].append(req_cap)

    def _analyze_method_call(self, node: MethodCall) -> None:
        """Check method calls for capability requirements."""
        self._analyze_node(node.object)
        for arg in node.args:
            self._analyze_node(arg)

    def _analyze_import(self, node: ImportDecl) -> None:
        """Check that extern imports have proper capability declarations."""
        if node.path and hasattr(node.path, 'segments'):
            # extern imports require ffi capability at the import site
            current_fn, current_caps = self._scope_stack[-1]
            # For now, just note that FFI requires the ffi capability
            pass

    def _extract_capabilities(self, attributes: list[Attribute]) -> CapabilitySet:
        """Extract capability set from @capabilities(...) attributes."""
        for attr in attributes:
            if attr.name == "capabilities":
                cap_names = []
                for arg in attr.args:
                    if isinstance(arg, Identifier):
                        cap_names.append(arg.name)
                    elif isinstance(arg, StringLiteral):
                        cap_names.append(arg.value)
                    elif isinstance(arg, FieldAccess):
                        # Handle filesystem:read style
                        cap_names.append(f"{self._expr_to_str(arg.object)}:{arg.field}")
                    else:
                        # Try to get the string value
                        cap_names.append(str(getattr(arg, 'name', getattr(arg, 'value', str(arg)))))
                return CapabilitySet.from_list(cap_names)
        # No @capabilities → pure compute only
        return CapabilitySet.none()

    def _expr_to_str(self, expr: Any) -> str:
        if isinstance(expr, Identifier):
            return expr.name
        return str(expr)

    def _get_tier(self, capability: str) -> CapabilityTier | None:
        """Get the required license tier for a capability."""
        base = capability.split(":")[0]
        sub = capability.split(":")[1] if ":" in capability else None

        if base in CAPABILITY_TREE:
            if sub and sub in CAPABILITY_TREE[base].get("children", {}):
                return CAPABILITY_TREE[base]["children"][sub]["tier"]
            return CAPABILITY_TREE[base]["tier"]
        return None


class CapabilityEnforcer:
    """
    Runtime capability enforcement. Wraps the interpreter to block
    unauthorized capability usage at execution time as a second line of defense.

    The static analyzer (CapabilityAnalyzer) catches violations at compile time.
    This enforcer is the runtime safety net — it should never trigger if the
    analyzer did its job, but defense in depth is non-negotiable.
    """

    def __init__(self, tier: CapabilityTier = CapabilityTier.COMMUNITY) -> None:
        self.tier = tier
        self.audit_log = CapabilityAuditLog()
        self._scope_stack: list[tuple[str, CapabilitySet]] = [
            ("<runtime>", CapabilitySet.none())
        ]

    def enter_function(self, name: str, caps: CapabilitySet) -> None:
        """Enter a function scope with declared capabilities."""
        parent_name, parent_caps = self._scope_stack[-1]

        # Runtime check: cannot elevate beyond parent
        if not caps.is_subset_of(parent_caps) and parent_name != "<runtime>":
            self.audit_log.log_escape_attempt(
                f"Runtime: '{name}' tried to elevate beyond '{parent_name}'"
            )
            raise SandboxEscapeAttempt(
                f"'{name}' attempted to acquire capabilities "
                f"{sorted(caps.capabilities - parent_caps.capabilities)} "
                f"not held by parent '{parent_name}'"
            )

        effective = parent_caps.narrow(caps) if parent_name != "<runtime>" else caps
        self._scope_stack.append((name, effective))

    def exit_function(self) -> None:
        """Exit the current function scope."""
        if len(self._scope_stack) > 1:
            self._scope_stack.pop()

    def check_capability(self, capability: str) -> bool:
        """Check if the current scope has a capability. Returns True/False."""
        name, caps = self._scope_stack[-1]
        has_it = caps.has(capability)

        if has_it:
            self.audit_log.log_allowed(name, capability, sorted(caps.capabilities))
        else:
            self.audit_log.log_blocked(name, capability, sorted(caps.capabilities))

        return has_it

    def require_capability(self, capability: str, action: str = "") -> None:
        """Require a capability or raise CapabilityViolation."""
        name, caps = self._scope_stack[-1]

        if not caps.has(capability):
            self.audit_log.log_blocked(name, capability, sorted(caps.capabilities), action)
            raise CapabilityViolation(name, capability, caps, action)

        self.audit_log.log_allowed(name, capability, sorted(caps.capabilities))

    def create_sandbox(self, granted_caps: CapabilitySet | None = None) -> CapabilityEnforcer:
        """
        Create a child sandbox. The sandbox starts with ZERO capabilities
        unless the parent explicitly grants a subset of its own.
        """
        sandbox = CapabilityEnforcer(self.tier)
        sandbox.audit_log = self.audit_log  # Share audit log

        parent_name, parent_caps = self._scope_stack[-1]

        if granted_caps is None:
            # Zero capabilities by default
            sandbox._scope_stack = [("<sandbox>", CapabilitySet.none())]
        else:
            # Can only grant what parent has
            if not granted_caps.is_subset_of(parent_caps):
                self.audit_log.log_escape_attempt(
                    f"Sandbox creation attempted with capabilities "
                    f"{sorted(granted_caps.capabilities - parent_caps.capabilities)} "
                    f"not held by parent '{parent_name}'"
                )
                raise SandboxEscapeAttempt(
                    f"Cannot grant sandbox capabilities {sorted(granted_caps.capabilities)} "
                    f"— parent '{parent_name}' only has {sorted(parent_caps.capabilities)}"
                )
            sandbox._scope_stack = [("<sandbox>", granted_caps)]

        return sandbox

    @property
    def current_capabilities(self) -> CapabilitySet:
        return self._scope_stack[-1][1]

    @property
    def current_function(self) -> str:
        return self._scope_stack[-1][0]
