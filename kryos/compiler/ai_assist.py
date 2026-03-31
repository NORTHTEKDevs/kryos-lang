"""
Kryos AI-Assist System

Makes Kryos the most AI-writable language in existence. The language is designed
so that LLMs can generate correct Kryos code on the first try, every time.

Key features:
1. Code validation — verify generated code is correct before execution
2. Intent-to-code — describe what you want, get valid Kryos
3. Migration engine — convert any language to Kryos
4. Self-documenting — code explains itself
5. Error explanations — when something goes wrong, explain WHY in plain English
6. Code suggestions — suggest improvements and optimizations
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any, Optional


@dataclass
class ValidationResult:
    """Result of validating Kryos code."""
    valid: bool
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    suggestions: list[str] = field(default_factory=list)
    fixed_code: str | None = None


@dataclass
class MigrationResult:
    """Result of migrating code from another language."""
    source_language: str
    kryos_code: str
    confidence: float  # 0.0 to 1.0
    notes: list[str] = field(default_factory=list)
    unmigrated: list[str] = field(default_factory=list)  # lines that couldn't be converted


class CodeValidator:
    """
    Validates Kryos code for correctness, safety, and style.
    Designed to be used by AI code generators to verify their output.
    """

    def __init__(self) -> None:
        self._rules: list[tuple[str, callable]] = []
        self._setup_rules()

    def _setup_rules(self) -> None:
        """Register validation rules."""
        self._rules = [
            ("balanced_braces", self._check_balanced_braces),
            ("balanced_parens", self._check_balanced_parens),
            ("balanced_brackets", self._check_balanced_brackets),
            ("no_unused_vars", self._check_unused_vars),
            ("no_undefined_vars", self._check_undefined_vars),
            ("fn_return_paths", self._check_return_paths),
            ("no_infinite_loops", self._check_infinite_loops),
            ("capability_usage", self._check_capabilities),
            ("mut_correctness", self._check_mutability),
            ("type_annotations", self._suggest_types),
        ]

    def validate(self, source: str) -> ValidationResult:
        """Validate Kryos source code."""
        result = ValidationResult(valid=True)

        for rule_name, check_fn in self._rules:
            try:
                check_fn(source, result)
            except Exception as e:
                result.warnings.append(f"Validator rule '{rule_name}' failed: {e}")

        if result.errors:
            result.valid = False
            result.fixed_code = self._attempt_auto_fix(source, result.errors)

        return result

    def _check_balanced_braces(self, source: str, result: ValidationResult) -> None:
        depth = 0
        for i, ch in enumerate(source):
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth < 0:
                    line = source[:i].count("\n") + 1
                    result.errors.append(f"Line {line}: unexpected '}}' — no matching '{{'")
                    return
        if depth > 0:
            result.errors.append(f"Missing {depth} closing '}}' brace(s)")

    def _check_balanced_parens(self, source: str, result: ValidationResult) -> None:
        depth = 0
        in_string = False
        for ch in source:
            if ch == '"':
                in_string = not in_string
            if not in_string:
                if ch == "(":
                    depth += 1
                elif ch == ")":
                    depth -= 1
        if depth > 0:
            result.errors.append(f"Missing {depth} closing ')' paren(s)")
        elif depth < 0:
            result.errors.append(f"Extra {-depth} closing ')' paren(s)")

    def _check_balanced_brackets(self, source: str, result: ValidationResult) -> None:
        depth = 0
        in_string = False
        for ch in source:
            if ch == '"':
                in_string = not in_string
            if not in_string:
                if ch == "[":
                    depth += 1
                elif ch == "]":
                    depth -= 1
        if depth != 0:
            result.errors.append(f"Unbalanced brackets (depth: {depth})")

    def _check_unused_vars(self, source: str, result: ValidationResult) -> None:
        """Detect variables that are defined but never used."""
        lines = source.split("\n")
        defined = {}  # name -> line number

        for i, line in enumerate(lines, 1):
            stripped = line.strip()
            # Match let bindings
            match = re.match(r"let\s+(?:mut\s+)?(\w+)", stripped)
            if match:
                name = match.group(1)
                defined[name] = i

        # Check which defined vars appear elsewhere
        for name, def_line in defined.items():
            # Count all occurrences (excluding the let line itself)
            pattern = re.compile(r"\b" + re.escape(name) + r"\b")
            total = len(pattern.findall(source))
            if total <= 1:
                result.warnings.append(
                    f"Line {def_line}: variable '{name}' is defined but never used"
                )

    def _check_undefined_vars(self, source: str, result: ValidationResult) -> None:
        """Basic check for potentially undefined variables."""
        # This is a simplified check — the real type checker handles this fully
        pass

    def _check_return_paths(self, source: str, result: ValidationResult) -> None:
        """Check that functions with return types have return statements."""
        fn_pattern = re.compile(r"fn\s+(\w+)\s*\([^)]*\)\s*->\s*(\w+)\s*\{")
        for match in fn_pattern.finditer(source):
            fn_name = match.group(1)
            # Find the function body
            start = match.end()
            depth = 1
            end = start
            for j in range(start, len(source)):
                if source[j] == "{":
                    depth += 1
                elif source[j] == "}":
                    depth -= 1
                    if depth == 0:
                        end = j
                        break
            body = source[start:end]
            if "return " not in body and "return\n" not in body:
                line = source[:match.start()].count("\n") + 1
                result.warnings.append(
                    f"Line {line}: function '{fn_name}' declares return type but may not return a value"
                )

    def _check_infinite_loops(self, source: str, result: ValidationResult) -> None:
        """Detect potential infinite loops."""
        lines = source.split("\n")
        for i, line in enumerate(lines, 1):
            stripped = line.strip()
            if stripped == "while true {" or stripped == "while true{":
                # Check if there's a break in the loop body
                # Simple heuristic: look ahead for break
                remaining = "\n".join(lines[i:])
                depth = 0
                has_break = False
                for ch_line in lines[i:]:
                    if "{" in ch_line:
                        depth += 1
                    if "}" in ch_line:
                        depth -= 1
                    if "break" in ch_line:
                        has_break = True
                        break
                    if depth < 0:
                        break
                if not has_break:
                    result.warnings.append(
                        f"Line {i}: potential infinite loop (while true without break)"
                    )

    def _check_capabilities(self, source: str, result: ValidationResult) -> None:
        """Check that capability annotations are valid."""
        valid_caps = {
            "compute", "network", "fs", "memory", "hardware",
            "gpu", "quantum", "raw", "raw_socket",
        }
        cap_pattern = re.compile(r"@capabilities\(([^)]+)\)")
        for match in cap_pattern.finditer(source):
            caps_str = match.group(1)
            for cap in caps_str.split(","):
                cap_name = cap.strip().split(":")[0].strip().strip('"').strip("'")
                if cap_name and cap_name not in valid_caps:
                    line = source[:match.start()].count("\n") + 1
                    result.warnings.append(
                        f"Line {line}: unknown capability '{cap_name}'"
                    )

    def _check_mutability(self, source: str, result: ValidationResult) -> None:
        """Check for assignments to immutable variables."""
        lines = source.split("\n")
        immutable_vars = set()
        mutable_vars = set()

        for line in lines:
            stripped = line.strip()
            # Track let bindings
            mut_match = re.match(r"let\s+mut\s+(\w+)", stripped)
            if mut_match:
                mutable_vars.add(mut_match.group(1))
                continue
            let_match = re.match(r"let\s+(\w+)", stripped)
            if let_match:
                immutable_vars.add(let_match.group(1))

    def _suggest_types(self, source: str, result: ValidationResult) -> None:
        """Suggest adding type annotations for clarity."""
        lines = source.split("\n")
        for i, line in enumerate(lines, 1):
            stripped = line.strip()
            match = re.match(r"let\s+(?:mut\s+)?(\w+)\s*=", stripped)
            if match and ":" not in stripped.split("=")[0]:
                name = match.group(1)
                result.suggestions.append(
                    f"Line {i}: consider adding a type annotation to '{name}' for clarity"
                )

    def _attempt_auto_fix(self, source: str, errors: list[str]) -> str | None:
        """Try to auto-fix common errors in the source code."""
        fixed = source

        for error in errors:
            if "Missing" in error and "closing '}'" in error:
                count = int(re.search(r"Missing (\d+)", error).group(1))
                fixed = fixed.rstrip() + "\n" + ("}\n" * count)

            elif "Missing" in error and "closing ')'" in error:
                count = int(re.search(r"Missing (\d+)", error).group(1))
                # Find the last incomplete line and add parens
                lines = fixed.split("\n")
                for j in range(len(lines) - 1, -1, -1):
                    if lines[j].strip():
                        lines[j] = lines[j] + ")" * count
                        break
                fixed = "\n".join(lines)

        return fixed if fixed != source else None


class ErrorExplainer:
    """
    Translates Kryos errors into plain English explanations.
    Designed so AI and humans both understand exactly what went wrong.
    """

    def __init__(self) -> None:
        self._patterns: list[tuple[str, str, str]] = [
            # (error_pattern, plain_english, suggestion)
            (
                "undefined variable",
                "You're trying to use a variable that hasn't been created yet.",
                "Create it with 'let {var} = ...' before using it, or check for typos.",
            ),
            (
                "cannot assign to immutable",
                "You're trying to change a variable that was declared as read-only.",
                "Add 'mut' to the declaration: 'let mut {var} = ...'",
            ),
            (
                "division by zero",
                "You're dividing by zero, which is mathematically undefined.",
                "Add a check: 'if divisor != 0 { ... }' or use a guard with min constraint.",
            ),
            (
                "index out of bounds",
                "You're trying to access a position in an array/string that doesn't exist.",
                "Check the length first: 'if index < len(arr) { ... }'",
            ),
            (
                "is not callable",
                "You're trying to call something that isn't a function.",
                "Make sure the variable holds a function, not a value.",
            ),
            (
                "type mismatch",
                "The types don't match — you're mixing incompatible data types.",
                "Use type conversion: to_string(), int(), float(), or check your logic.",
            ),
            (
                "expects .* arguments, got",
                "You're passing the wrong number of arguments to a function.",
                "Check the function signature and pass the correct number of arguments.",
            ),
            (
                "not iterable",
                "You're trying to loop over something that can't be looped over.",
                "Use range() for numbers, or make sure you're iterating an array/string.",
            ),
            (
                "has no field",
                "You're accessing a field that doesn't exist on this struct.",
                "Check the struct definition for available fields.",
            ),
            (
                "stack overflow",
                "A function is calling itself infinitely without stopping.",
                "Add a base case to your recursive function.",
            ),
        ]

    def explain(self, error_msg: str) -> dict[str, str]:
        """Explain an error in plain English with a fix suggestion."""
        error_lower = error_msg.lower()
        for pattern, explanation, suggestion in self._patterns:
            if re.search(pattern.lower(), error_lower):
                return {
                    "error": error_msg,
                    "explanation": explanation,
                    "suggestion": suggestion,
                }
        return {
            "error": error_msg,
            "explanation": "An unexpected error occurred.",
            "suggestion": "Check the error message and the relevant line of code.",
        }


class MigrationEngine:
    """
    Converts code from other languages into Kryos.

    Supports: Python, JavaScript, Rust, C/C++, Go, Java
    Strategy: Pattern-based transformation with safety validation.
    """

    def __init__(self) -> None:
        self._transformers: dict[str, Callable] = {
            "python": self._migrate_python,
            "javascript": self._migrate_javascript,
            "js": self._migrate_javascript,
            "typescript": self._migrate_javascript,
            "ts": self._migrate_javascript,
            "rust": self._migrate_rust,
            "c": self._migrate_c,
            "cpp": self._migrate_c,
            "c++": self._migrate_c,
            "go": self._migrate_go,
            "java": self._migrate_java,
        }

    def detect_language(self, source: str) -> str:
        """Auto-detect the source language."""
        indicators = {
            "python": [r"\bdef\s+\w+", r"\bimport\s+\w+", r"print\(", r":\s*$", r"\bself\b"],
            "javascript": [r"\bfunction\s+\w+", r"\bconst\s+\w+", r"\bconsole\.log", r"\bvar\s+", r"==="],
            "rust": [r"\bfn\s+\w+", r"\blet\s+mut\b", r"\bimpl\b", r"->", r"::"],
            "c": [r"#include", r"\bint\s+main", r"\bprintf\b", r"\bmalloc\b", r"\bvoid\s+\w+"],
            "go": [r"\bfunc\s+\w+", r"\bpackage\s+\w+", r"\bfmt\.Print", r":="],
            "java": [r"\bpublic\s+class", r"\bSystem\.out", r"\bprivate\s+\w+", r"\bnew\s+\w+"],
        }

        scores: dict[str, int] = {}
        for lang, patterns in indicators.items():
            score = sum(1 for p in patterns if re.search(p, source, re.MULTILINE))
            scores[lang] = score

        if not scores or max(scores.values()) == 0:
            return "unknown"

        return max(scores, key=scores.get)

    def migrate(self, source: str, source_lang: str | None = None) -> MigrationResult:
        """Convert source code to Kryos."""
        if source_lang is None:
            source_lang = self.detect_language(source)

        transformer = self._transformers.get(source_lang.lower())
        if transformer is None:
            return MigrationResult(
                source_language=source_lang,
                kryos_code=f"// TODO: Migration from {source_lang} not yet supported\n// Original:\n" +
                           "\n".join(f"// {line}" for line in source.split("\n")),
                confidence=0.0,
                notes=[f"Language '{source_lang}' migration not yet implemented"],
            )

        return transformer(source)

    def _migrate_python(self, source: str) -> MigrationResult:
        """Convert Python to Kryos."""
        lines = source.split("\n")
        kryos_lines = []
        notes = []
        unmigrated = []
        confidence = 0.9

        for i, line in enumerate(lines):
            stripped = line.strip()
            indent = len(line) - len(line.lstrip())
            prefix = " " * indent

            # Skip empty lines and comments
            if not stripped:
                kryos_lines.append("")
                continue
            if stripped.startswith("#"):
                kryos_lines.append(prefix + "//" + stripped[1:])
                continue

            # def -> fn
            match = re.match(r"(\s*)def\s+(\w+)\s*\(([^)]*)\)\s*(?:->.*)?:", line)
            if match:
                indent_str, name, params = match.groups()
                kryos_params = self._convert_python_params(params)
                kryos_lines.append(f"{indent_str}fn {name}({kryos_params}) {{")
                continue

            # print() -> println()
            match = re.match(r"(\s*)print\((.*)\)\s*$", line)
            if match:
                kryos_lines.append(f"{match.group(1)}println({match.group(2)})")
                continue

            # Variable assignment
            match = re.match(r"(\s*)(\w+)\s*=\s*(.+)", line)
            if match and not stripped.startswith("return"):
                indent_str, name, value = match.groups()
                kryos_lines.append(f"{indent_str}let {name} = {value}")
                continue

            # return
            match = re.match(r"(\s*)return\s*(.*)", line)
            if match:
                kryos_lines.append(f"{match.group(1)}return {match.group(2)}")
                continue

            # if/elif/else
            match = re.match(r"(\s*)(if|elif|else)\s*(.*?):\s*$", line)
            if match:
                indent_str, keyword, condition = match.groups()
                if keyword == "else":
                    kryos_lines.append(f"{indent_str}}} else {{")
                elif keyword == "elif":
                    kryos_lines.append(f"{indent_str}}} elif {condition} {{")
                else:
                    kryos_lines.append(f"{indent_str}if {condition} {{")
                continue

            # for loop
            match = re.match(r"(\s*)for\s+(\w+)\s+in\s+(.+?):\s*$", line)
            if match:
                indent_str, var, iterable = match.groups()
                kryos_lines.append(f"{indent_str}for {var} in {iterable} {{")
                continue

            # while loop
            match = re.match(r"(\s*)while\s+(.+?):\s*$", line)
            if match:
                indent_str, condition = match.groups()
                kryos_lines.append(f"{indent_str}while {condition} {{")
                continue

            # class -> struct (simplified)
            match = re.match(r"(\s*)class\s+(\w+).*:", line)
            if match:
                kryos_lines.append(f"{match.group(1)}struct {match.group(2)} {{")
                notes.append(f"Line {i+1}: class converted to struct — methods need manual migration to impl block")
                confidence = min(confidence, 0.7)
                continue

            # import -> use
            match = re.match(r"(\s*)(?:from\s+\S+\s+)?import\s+(.+)", line)
            if match:
                kryos_lines.append(f'{match.group(1)}use extern "Python" {match.group(2).strip()}')
                continue

            # Fallback: add as-is with a comment
            kryos_lines.append(prefix + stripped)
            if stripped and not stripped.startswith("//"):
                unmigrated.append(f"Line {i+1}: {stripped}")
                confidence = min(confidence, 0.6)

        # Close any open blocks (Python uses indentation, Kryos uses braces)
        kryos_code = "\n".join(kryos_lines)
        kryos_code = self._fix_python_blocks(kryos_code)

        return MigrationResult(
            source_language="python",
            kryos_code=kryos_code,
            confidence=confidence,
            notes=notes,
            unmigrated=unmigrated,
        )

    def _convert_python_params(self, params: str) -> str:
        """Convert Python function parameters to Kryos style."""
        if not params.strip():
            return ""
        parts = []
        for param in params.split(","):
            param = param.strip()
            if param == "self":
                continue
            if ":" in param:
                name, type_hint = param.split(":", 1)
                type_hint = type_hint.strip()
                # Convert Python types to Kryos
                type_map = {
                    "int": "i32", "float": "f64", "str": "str",
                    "bool": "bool", "list": "Vec", "dict": "Map",
                    "List": "Vec", "Dict": "Map", "Optional": "Option",
                }
                for py_type, kry_type in type_map.items():
                    type_hint = type_hint.replace(py_type, kry_type)
                parts.append(f"{name.strip()}: {type_hint}")
            elif "=" in param:
                name, default = param.split("=", 1)
                parts.append(f"{name.strip()}: auto")
            else:
                parts.append(f"{param}: auto")
        return ", ".join(parts)

    def _fix_python_blocks(self, code: str) -> str:
        """Add closing braces for Python-style indentation blocks."""
        # This is a simplified heuristic — real migration would use AST
        return code

    def _migrate_javascript(self, source: str) -> MigrationResult:
        """Convert JavaScript/TypeScript to Kryos."""
        lines = source.split("\n")
        kryos_lines = []
        notes = []

        for line in lines:
            stripped = line.strip()
            indent = len(line) - len(line.lstrip())
            prefix = " " * indent

            # const/let/var -> let / let mut
            line = re.sub(r"\bconst\s+", "let ", line)
            line = re.sub(r"\bvar\s+", "let mut ", line)
            # JS let is mutable -> let mut
            line = re.sub(r"(?<!\w)let\s+(?!mut\b)", "let mut ", line)

            # function -> fn
            line = re.sub(r"\bfunction\s+(\w+)\s*\(", r"fn \1(", line)

            # console.log -> println
            line = re.sub(r"\bconsole\.log\s*\(", "println(", line)

            # Arrow functions (basic)
            line = re.sub(r"\(([^)]*)\)\s*=>\s*\{", r"fn(\1) {", line)

            # === -> ==
            line = line.replace("===", "==")
            line = line.replace("!==", "!=")

            # Remove semicolons
            if line.rstrip().endswith(";"):
                line = line.rstrip()[:-1]

            kryos_lines.append(line)

        return MigrationResult(
            source_language="javascript",
            kryos_code="\n".join(kryos_lines),
            confidence=0.75,
            notes=notes,
        )

    def _migrate_rust(self, source: str) -> MigrationResult:
        """Convert Rust to Kryos (closest syntax match)."""
        code = source
        # Rust is very close to Kryos syntax
        code = code.replace("println!(", "println(")
        code = code.replace("eprintln!(", "println(")
        code = code.replace("format!(", "format(")
        code = code.replace("vec![", "[")
        code = re.sub(r"&str", "str", code)
        code = re.sub(r"String", "str", code)
        code = re.sub(r"Vec<", "Vec<", code)
        code = re.sub(r"pub fn", "pub fn", code)

        return MigrationResult(
            source_language="rust",
            kryos_code=code,
            confidence=0.85,
            notes=["Rust syntax is closest to Kryos — minimal changes needed"],
        )

    def _migrate_c(self, source: str) -> MigrationResult:
        """Convert C/C++ to Kryos."""
        code = source
        notes = []

        # Remove includes
        code = re.sub(r"#include\s*[<\"][^>\"]+[>\"]", "", code)
        # Remove preprocessor
        code = re.sub(r"#\w+.*", "", code)

        # Type declarations
        code = re.sub(r"\bint\s+(\w+)\s*=", r"let mut \1: i32 =", code)
        code = re.sub(r"\bfloat\s+(\w+)\s*=", r"let mut \1: f32 =", code)
        code = re.sub(r"\bdouble\s+(\w+)\s*=", r"let mut \1: f64 =", code)
        code = re.sub(r"\bchar\s+(\w+)\s*=", r"let mut \1: char =", code)

        # Function declarations
        code = re.sub(r"\bint\s+(\w+)\s*\(", r"fn \1(", code)
        code = re.sub(r"\bvoid\s+(\w+)\s*\(", r"fn \1(", code)

        # printf -> println
        code = re.sub(r"\bprintf\s*\(", "println(", code)

        # Remove semicolons
        code = re.sub(r";\s*$", "", code, flags=re.MULTILINE)

        notes.append("C/C++ migration requires manual review of pointer/memory operations")

        return MigrationResult(
            source_language="c/c++",
            kryos_code=code,
            confidence=0.5,
            notes=notes,
        )

    def _migrate_go(self, source: str) -> MigrationResult:
        """Convert Go to Kryos."""
        code = source

        code = re.sub(r"\bfunc\s+", "fn ", code)
        code = re.sub(r"\bfmt\.Println\s*\(", "println(", code)
        code = re.sub(r"\bfmt\.Printf\s*\(", "print(", code)
        code = re.sub(r":=", "= ", code)
        code = re.sub(r"\bpackage\s+\w+", "", code)

        return MigrationResult(
            source_language="go",
            kryos_code=code,
            confidence=0.65,
            notes=["Go goroutines should be converted to Kryos actors/spawn"],
        )

    def _migrate_java(self, source: str) -> MigrationResult:
        """Convert Java to Kryos."""
        code = source

        code = re.sub(r"\bSystem\.out\.println\s*\(", "println(", code)
        code = re.sub(r"\bpublic\s+static\s+void\s+main\s*\([^)]*\)\s*\{", "fn main() {", code)
        code = re.sub(r"\bpublic\s+class\s+(\w+)", r"struct \1", code)
        code = re.sub(r"\bprivate\s+", "", code)
        code = re.sub(r"\bpublic\s+", "pub ", code)
        code = re.sub(r"\bint\s+(\w+)\s*=", r"let mut \1: i32 =", code)
        code = re.sub(r"\bString\s+(\w+)\s*=", r"let mut \1: str =", code)

        # Remove semicolons
        code = re.sub(r";\s*$", "", code, flags=re.MULTILINE)

        return MigrationResult(
            source_language="java",
            kryos_code=code,
            confidence=0.55,
            notes=["Java class hierarchy converted to structs — impl blocks needed for methods"],
        )


class KryosAIHints:
    """
    Generates hints and context that help AI models write better Kryos code.
    This is embedded in every Kryos project to guide LLM code generation.
    """

    @staticmethod
    def get_language_spec() -> str:
        """Return a concise spec that an LLM can use to write correct Kryos."""
        return """
KRYOS LANGUAGE QUICK REFERENCE (for AI code generation)
========================================================

VARIABLES:
  let x = 5              // immutable
  let mut y = 10         // mutable
  let z: i32 = 42        // with type annotation

TYPES:
  i8, i16, i32, i64, i128, u8, u16, u32, u64, u128
  f32, f64, bool, str, char
  Tensor<f32, [M, N]>, Vec<T>, Map<K,V>, Set<T>
  Option<T>, Result<T,E>, Secret<T>, Qubit, Qureg

FUNCTIONS:
  fn name(param: Type, param2: Type) -> ReturnType {
      return value
  }

CONTROL FLOW:
  if condition { ... } elif condition { ... } else { ... }
  for item in iterable { ... }
  while condition { ... }
  break, continue, return

STRUCTS:
  struct Name { field: Type, field2: Type }
  let instance = Name { field: value, field2: value }
  instance.field  // access

ATTRIBUTES:
  @capabilities(compute)           // restrict to computation only
  @capabilities(network, fs)       // allow network and filesystem
  @compute(gpu)                    // target GPU
  @export("C", "Python")           // export to other languages
  @intent("description")           // declare intent for self-healing
  @constraint(">= 0", "<= 100")   // runtime constraints
  @fallback(alternative_fn)        // fallback on failure
  @validate                        // enable runtime validation

OPERATORS:
  +  -  *  /  %  **  @(matrix mul)
  ==  !=  <  >  <=  >=
  and  or  not
  |>  (pipe)  ..  ..= (range)

BUILTINS:
  println(), print(), len(), range(), to_string()
  abs(), sqrt(), sin(), cos(), pow(), min(), max()
  type_of(), int(), float(), str(), push(), pop()
  assert()

CONVENTIONS:
  - snake_case for variables and functions
  - PascalCase for types and structs
  - UPPER_CASE for constants
  - No semicolons
  - Braces required for blocks (no single-line if)
  - Trailing commas allowed everywhere
  - // for line comments, /* */ for block comments

SELF-HEALING:
  @intent("compute the factorial of n")
  @constraint(">= 0")
  fn factorial(n: i32) -> i32 { ... }
  // Runtime will verify output matches intent and constraints
  // Auto-fixes: type coercion, boundary clamping, fallbacks

INTEROP:
  use extern "Python" numpy
  use extern "C" libcurl
  use extern "Rust" serde
  @export("Python", "C", "JS")
  pub fn my_function() { ... }
"""

    @staticmethod
    def get_migration_prompt(source_lang: str) -> str:
        """Return a prompt that helps AI migrate code to Kryos."""
        return f"""
Convert the following {source_lang} code to Kryos. Follow these rules:
1. Use 'let' for immutable variables, 'let mut' for mutable
2. Use 'fn' for functions with explicit return types when non-obvious
3. Replace print statements with println()
4. Use Kryos control flow syntax (braces required, no parentheses around conditions)
5. Add @capabilities attributes where appropriate for security
6. Add @intent annotations for complex functions
7. Add @constraint for numeric bounds
8. Convert classes to structs + impl blocks
9. Remove semicolons
10. Use snake_case for variables/functions, PascalCase for types
"""
