"""
Kryos Self-Healing Runtime Engine

The core principle: Kryos code declares INTENT alongside IMPLEMENTATION.
The runtime continuously verifies that actual behavior matches declared intent.
When they diverge, the self-healing engine activates and corrects automatically.

Self-healing strategies:
1. @intent declarations — what the code SHOULD produce
2. @constraint annotations — invariants that must always hold
3. @fallback chains — ordered recovery strategies
4. @validate blocks — runtime checks with auto-correction
5. Automatic type coercion and boundary clamping
6. Hot-patching of detected issues without restart
"""

from __future__ import annotations

import copy
import traceback
from dataclasses import dataclass, field
from typing import Any, Callable, Optional
from enum import Enum, auto


class HealAction(Enum):
    """Types of self-healing actions the engine can take."""
    RETRY = auto()           # Retry the operation
    COERCE = auto()          # Coerce types to match expectations
    CLAMP = auto()           # Clamp values to valid range
    FALLBACK = auto()        # Use fallback value/function
    RECONSTRUCT = auto()     # Reconstruct from last known good state
    SKIP = auto()            # Skip the failing operation safely
    SUBSTITUTE = auto()      # Substitute with equivalent operation


@dataclass
class HealEvent:
    """Record of a self-healing action taken."""
    action: HealAction
    location: str
    original_error: str
    fix_applied: str
    result: Any = None
    timestamp: float = 0.0


@dataclass
class Intent:
    """Declares what a piece of code intends to accomplish."""
    description: str
    input_types: dict[str, str] = field(default_factory=dict)
    output_type: str = ""
    constraints: list[str] = field(default_factory=list)
    fallbacks: list[Callable] = field(default_factory=list)
    examples: list[tuple[Any, Any]] = field(default_factory=list)  # (input, expected_output)


@dataclass
class Constraint:
    """A runtime invariant that must always hold."""
    name: str
    check: Callable[..., bool]
    fix: Callable[..., Any] | None = None
    message: str = ""


class SelfHealingEngine:
    """
    The brain of Kryos self-healing. Monitors execution, detects anomalies,
    and applies fixes automatically without breaking the system.

    Design principles:
    - Fixes must be SAFE: never make things worse
    - Fixes must be MINIMAL: change as little as possible
    - Fixes must be LOGGED: every auto-fix is recorded for audit
    - Fixes must be DETERMINISTIC: same problem -> same fix every time
    """

    def __init__(self) -> None:
        self.heal_log: list[HealEvent] = []
        self.constraints: dict[str, Constraint] = {}
        self.intents: dict[str, Intent] = {}
        self.snapshots: dict[str, Any] = {}  # last known good states
        self.max_retries: int = 3
        self._fix_registry: dict[str, list[Callable]] = {}

    def register_intent(self, name: str, intent: Intent) -> None:
        """Register an intent declaration for a function or block."""
        self.intents[name] = intent

    def register_constraint(self, constraint: Constraint) -> None:
        """Register a global constraint/invariant."""
        self.constraints[constraint.name] = constraint

    def snapshot(self, name: str, state: Any) -> None:
        """Save a known-good state for potential rollback."""
        self.snapshots[name] = copy.deepcopy(state)

    def get_snapshot(self, name: str) -> Any | None:
        """Retrieve last known good state."""
        return self.snapshots.get(name)

    def register_fix(self, error_pattern: str, fix_fn: Callable) -> None:
        """Register a fix strategy for a specific error pattern."""
        if error_pattern not in self._fix_registry:
            self._fix_registry[error_pattern] = []
        self._fix_registry[error_pattern].append(fix_fn)

    # ------------------------------------------------------------------
    # Core self-healing execution
    # ------------------------------------------------------------------

    def execute_with_healing(
        self,
        fn: Callable,
        args: list[Any],
        intent: Intent | None = None,
        location: str = "<unknown>",
    ) -> Any:
        """
        Execute a function with full self-healing protection.

        1. Validate inputs against intent
        2. Try execution
        3. If it fails, apply fix strategies
        4. Validate output against intent
        5. If output is wrong, attempt correction
        """
        # Step 1: Auto-fix inputs if they don't match intent
        if intent:
            args = self._heal_inputs(args, intent, location)

        # Step 2: Try execution with retry and fallback
        result = None
        last_error = None

        for attempt in range(self.max_retries + 1):
            try:
                result = fn(*args)
                last_error = None
                break
            except Exception as e:
                last_error = e
                fix_applied = self._try_auto_fix(e, fn, args, location)
                if fix_applied is not None:
                    result = fix_applied
                    last_error = None
                    break
                # If we have a fixable input issue, try coercing
                args = self._coerce_args_for_retry(args, e, attempt, location)

        # Step 3: If all retries failed, try fallbacks
        if last_error is not None and intent and intent.fallbacks:
            for fallback_fn in intent.fallbacks:
                try:
                    result = fallback_fn(*args)
                    self._log_heal(
                        HealAction.FALLBACK, location,
                        str(last_error), f"used fallback: {fallback_fn.__name__}",
                        result,
                    )
                    last_error = None
                    break
                except Exception:
                    continue

        # Step 4: If still failing, try reconstruction from snapshot
        if last_error is not None:
            snapshot = self.get_snapshot(location)
            if snapshot is not None:
                self._log_heal(
                    HealAction.RECONSTRUCT, location,
                    str(last_error), "restored from last known good state",
                    snapshot,
                )
                return snapshot
            # Nothing worked — raise the original error
            raise last_error

        # Step 5: Validate output and auto-correct if needed
        if intent:
            result = self._heal_output(result, intent, location)

        return result

    # ------------------------------------------------------------------
    # Input healing
    # ------------------------------------------------------------------

    def _heal_inputs(self, args: list[Any], intent: Intent, location: str) -> list[Any]:
        """Auto-fix inputs to match expected types."""
        healed = list(args)
        type_names = list(intent.input_types.values())

        for i, (arg, expected) in enumerate(zip(healed, type_names)):
            fixed = self._coerce_value(arg, expected)
            if fixed is not arg:
                self._log_heal(
                    HealAction.COERCE, location,
                    f"arg {i} is {type(arg).__name__}, expected {expected}",
                    f"coerced to {expected}",
                    fixed,
                )
                healed[i] = fixed

        return healed

    def _coerce_value(self, value: Any, target_type: str) -> Any:
        """Safely coerce a value to a target type."""
        try:
            match target_type:
                case "i32" | "i64" | "int":
                    if isinstance(value, (int, float, str, bool)):
                        return int(value)
                case "f32" | "f64" | "float":
                    if isinstance(value, (int, float, str, bool)):
                        return float(value)
                case "str" | "string":
                    return str(value)
                case "bool":
                    return bool(value)
                case "array" | "list":
                    if not isinstance(value, list):
                        return [value]
        except (ValueError, TypeError):
            pass
        return value

    # ------------------------------------------------------------------
    # Output healing
    # ------------------------------------------------------------------

    def _heal_output(self, result: Any, intent: Intent, location: str) -> Any:
        """Validate and auto-correct output against intent."""
        # Check output type
        if intent.output_type:
            result = self._coerce_value(result, intent.output_type)

        # Check constraints
        for constraint_str in intent.constraints:
            result = self._enforce_constraint(result, constraint_str, location)

        # Verify against examples if available
        # (This is used during development/testing to catch regressions)

        return result

    def _enforce_constraint(self, value: Any, constraint: str, location: str) -> Any:
        """Enforce a constraint on a value, auto-fixing if possible."""
        # Parse simple constraints
        if constraint.startswith(">="):
            limit = _parse_number(constraint[2:].strip())
            if isinstance(value, (int, float)) and value < limit:
                self._log_heal(
                    HealAction.CLAMP, location,
                    f"value {value} violates constraint {constraint}",
                    f"clamped to {limit}",
                    limit,
                )
                return limit

        elif constraint.startswith("<="):
            limit = _parse_number(constraint[2:].strip())
            if isinstance(value, (int, float)) and value > limit:
                self._log_heal(
                    HealAction.CLAMP, location,
                    f"value {value} violates constraint {constraint}",
                    f"clamped to {limit}",
                    limit,
                )
                return limit

        elif constraint.startswith(">"):
            limit = _parse_number(constraint[1:].strip())
            if isinstance(value, (int, float)) and value <= limit:
                fixed = limit + (1 if isinstance(value, int) else 0.001)
                self._log_heal(
                    HealAction.CLAMP, location,
                    f"value {value} violates constraint {constraint}",
                    f"clamped to {fixed}",
                    fixed,
                )
                return fixed

        elif constraint == "not_empty":
            if isinstance(value, (str, list)) and len(value) == 0:
                default = " " if isinstance(value, str) else [None]
                self._log_heal(
                    HealAction.SUBSTITUTE, location,
                    "value is empty, constraint requires not_empty",
                    f"substituted default",
                    default,
                )
                return default

        elif constraint == "not_none" or constraint == "not_null":
            if value is None:
                self._log_heal(
                    HealAction.SUBSTITUTE, location,
                    "value is None, constraint requires not_none",
                    "substituted 0",
                    0,
                )
                return 0

        return value

    # ------------------------------------------------------------------
    # Auto-fix engine
    # ------------------------------------------------------------------

    def _try_auto_fix(
        self, error: Exception, fn: Callable, args: list[Any], location: str,
    ) -> Any | None:
        """Try to automatically fix a runtime error."""
        error_str = str(error).lower()
        error_type = type(error).__name__

        # Division by zero → return 0 or infinity based on context
        if "division by zero" in error_str or "modulo by zero" in error_str:
            self._log_heal(
                HealAction.SUBSTITUTE, location,
                str(error), "substituted 0 for division by zero",
                0,
            )
            return 0

        # Index out of bounds → clamp to valid range
        if "index out of" in error_str or "list index" in error_str:
            for arg in args:
                if isinstance(arg, (list, str)) and len(arg) > 0:
                    self._log_heal(
                        HealAction.CLAMP, location,
                        str(error), "clamped index to last valid element",
                        arg[-1],
                    )
                    return arg[-1]

        # Type mismatch → try coercion
        if "unsupported operand" in error_str or "cannot" in error_str:
            coerced_args = []
            for arg in args:
                if isinstance(arg, str):
                    try:
                        coerced_args.append(float(arg) if "." in arg else int(arg))
                        continue
                    except ValueError:
                        pass
                coerced_args.append(arg)

            if coerced_args != args:
                try:
                    result = fn(*coerced_args)
                    self._log_heal(
                        HealAction.COERCE, location,
                        str(error), "coerced string args to numeric",
                        result,
                    )
                    return result
                except Exception:
                    pass

        # Key/attribute not found → return None
        if "key" in error_str and "not found" in error_str:
            self._log_heal(
                HealAction.SUBSTITUTE, location,
                str(error), "substituted none for missing key",
                None,
            )
            return None

        # Check registered fixes
        for pattern, fixes in self._fix_registry.items():
            if pattern.lower() in error_str:
                for fix_fn in fixes:
                    try:
                        result = fix_fn(error, fn, args)
                        self._log_heal(
                            HealAction.SUBSTITUTE, location,
                            str(error), f"applied registered fix: {fix_fn.__name__}",
                            result,
                        )
                        return result
                    except Exception:
                        continue

        return None

    def _coerce_args_for_retry(
        self, args: list[Any], error: Exception, attempt: int, location: str,
    ) -> list[Any]:
        """Try to fix args for the next retry attempt."""
        error_str = str(error).lower()

        if attempt == 0 and "type" in error_str:
            # First retry: try converting strings to numbers
            return [
                (int(a) if isinstance(a, str) and a.isdigit() else a) for a in args
            ]
        if attempt == 1:
            # Second retry: try converting everything to strings
            return [str(a) for a in args]

        return args

    # ------------------------------------------------------------------
    # Logging
    # ------------------------------------------------------------------

    def _log_heal(
        self, action: HealAction, location: str,
        original_error: str, fix_applied: str, result: Any = None,
    ) -> None:
        """Log a self-healing action."""
        import time
        event = HealEvent(
            action=action,
            location=location,
            original_error=original_error,
            fix_applied=fix_applied,
            result=result,
            timestamp=time.time(),
        )
        self.heal_log.append(event)

    def get_heal_report(self) -> str:
        """Generate a human-readable report of all healing actions."""
        if not self.heal_log:
            return "No self-healing actions taken."

        lines = [f"Self-Healing Report ({len(self.heal_log)} actions):"]
        lines.append("=" * 60)
        for i, event in enumerate(self.heal_log, 1):
            lines.append(f"\n[{i}] {event.action.name} at {event.location}")
            lines.append(f"    Error:  {event.original_error}")
            lines.append(f"    Fix:    {event.fix_applied}")
            if event.result is not None:
                lines.append(f"    Result: {event.result}")
        return "\n".join(lines)

    def clear_log(self) -> None:
        self.heal_log.clear()


# ------------------------------------------------------------------
# Guard System — wraps values with runtime constraints
# ------------------------------------------------------------------

class Guard:
    """
    A guarded value that maintains invariants automatically.

    Usage in Kryos:
        let health = Guard(100, min=0, max=100)
        health.set(health.get() - 150)  // auto-clamped to 0
        health.set(health.get() + 999)  // auto-clamped to 100
    """

    def __init__(
        self,
        value: Any,
        min_val: Any = None,
        max_val: Any = None,
        type_constraint: type | None = None,
        not_none: bool = False,
        not_empty: bool = False,
        custom_check: Callable[[Any], bool] | None = None,
        on_heal: Callable[[str], None] | None = None,
    ) -> None:
        self._value = value
        self._min = min_val
        self._max = max_val
        self._type = type_constraint
        self._not_none = not_none
        self._not_empty = not_empty
        self._custom = custom_check
        self._on_heal = on_heal
        self._heal_count = 0
        # Apply constraints to initial value
        self._value = self._enforce(value)

    def get(self) -> Any:
        return self._value

    def set(self, new_value: Any) -> None:
        self._value = self._enforce(new_value)

    def _enforce(self, value: Any) -> Any:
        """Enforce all constraints, auto-fixing violations."""
        original = value

        # Type constraint
        if self._type is not None and not isinstance(value, self._type):
            try:
                value = self._type(value)
                self._healed(f"coerced {type(original).__name__} to {self._type.__name__}")
            except (ValueError, TypeError):
                pass

        # Not none
        if self._not_none and value is None:
            value = self._type() if self._type else 0
            self._healed("replaced None with default")

        # Not empty
        if self._not_empty and isinstance(value, (str, list)) and len(value) == 0:
            if isinstance(value, str):
                value = " "
            else:
                value = [None]
            self._healed("replaced empty with default")

        # Min/max clamping
        if self._min is not None and value < self._min:
            value = self._min
            self._healed(f"clamped below minimum {self._min}")

        if self._max is not None and value > self._max:
            value = self._max
            self._healed(f"clamped above maximum {self._max}")

        # Custom check
        if self._custom and not self._custom(value):
            self._healed(f"custom constraint violated, keeping previous value")
            value = self._value if hasattr(self, '_value') else value

        return value

    def _healed(self, msg: str) -> None:
        self._heal_count += 1
        if self._on_heal:
            self._on_heal(msg)

    @property
    def heal_count(self) -> int:
        return self._heal_count


# ------------------------------------------------------------------
# Watchdog — monitors running state and auto-recovers
# ------------------------------------------------------------------

class Watchdog:
    """
    Monitors a set of named values/functions and ensures they stay healthy.

    Usage:
        watchdog = Watchdog()
        watchdog.watch("server_connections", lambda: len(connections), max=1000)
        watchdog.watch("memory_mb", get_memory, max=512)
        watchdog.check_all()  # auto-heals any violations
    """

    def __init__(self, heal_engine: SelfHealingEngine | None = None) -> None:
        self._watches: dict[str, dict] = {}
        self.engine = heal_engine or SelfHealingEngine()

    def watch(
        self,
        name: str,
        getter: Callable[[], Any],
        min_val: Any = None,
        max_val: Any = None,
        fix: Callable[[], None] | None = None,
    ) -> None:
        self._watches[name] = {
            "getter": getter,
            "min": min_val,
            "max": max_val,
            "fix": fix,
        }

    def check_all(self) -> list[str]:
        """Check all watched values. Returns list of issues found and fixed."""
        issues = []
        for name, watch in self._watches.items():
            try:
                value = watch["getter"]()
            except Exception as e:
                issues.append(f"{name}: getter failed ({e})")
                continue

            if watch["min"] is not None and value < watch["min"]:
                issues.append(f"{name}: {value} below min {watch['min']}")
                if watch["fix"]:
                    watch["fix"]()

            if watch["max"] is not None and value > watch["max"]:
                issues.append(f"{name}: {value} above max {watch['max']}")
                if watch["fix"]:
                    watch["fix"]()

        return issues


# ------------------------------------------------------------------
# Resilient Pipeline — chain of operations that self-heals
# ------------------------------------------------------------------

class Pipeline:
    """
    A chain of operations where each step can self-heal.
    If a step fails, the pipeline tries fixes before moving to the next step.

    Usage:
        result = (Pipeline(heal_engine)
            .step("parse", parse_fn)
            .step("validate", validate_fn, fallback=default_validate)
            .step("transform", transform_fn)
            .step("output", output_fn)
            .run(input_data))
    """

    def __init__(self, engine: SelfHealingEngine | None = None) -> None:
        self.engine = engine or SelfHealingEngine()
        self._steps: list[tuple[str, Callable, Intent | None]] = []

    def step(
        self,
        name: str,
        fn: Callable,
        fallback: Callable | None = None,
        constraints: list[str] | None = None,
    ) -> Pipeline:
        intent = None
        if fallback or constraints:
            intent = Intent(
                description=name,
                constraints=constraints or [],
                fallbacks=[fallback] if fallback else [],
            )
        self._steps.append((name, fn, intent))
        return self

    def run(self, input_data: Any) -> Any:
        """Run the pipeline with self-healing on every step."""
        current = input_data
        for name, fn, intent in self._steps:
            self.engine.snapshot(name, current)
            current = self.engine.execute_with_healing(
                fn, [current], intent=intent, location=f"pipeline:{name}",
            )
        return current


# ------------------------------------------------------------------
# Helpers
# ------------------------------------------------------------------

def _parse_number(s: str) -> int | float:
    """Parse a number from a constraint string."""
    s = s.strip()
    try:
        return int(s)
    except ValueError:
        return float(s)
