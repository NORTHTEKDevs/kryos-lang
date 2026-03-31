"""
Kryos Language CLI
Usage:
    kryos run <file.kry>         # interpret/run a Kryos file
    kryos build <file.kry>       # compile (stub)
    kryos check <file.kry>       # type check only
    kryos repl                   # interactive REPL
    kryos test <dir>             # run test files
    kryos version                # show version
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from kryos import __version__, __language__
from kryos.compiler import Lexer, tokenize, LexerError, parse, ParseError, Interpreter, KryosRuntimeError
from kryos.compiler.parser import ParseErrors


# ---------------------------------------------------------------------------
# ANSI Color helpers
# ---------------------------------------------------------------------------

_USE_COLOR = hasattr(sys.stdout, "isatty") and sys.stdout.isatty()


def _color(code: str, text: str) -> str:
    if not _USE_COLOR:
        return text
    return f"\033[{code}m{text}\033[0m"


def _red(text: str) -> str:
    return _color("31", text)


def _green(text: str) -> str:
    return _color("32", text)


def _yellow(text: str) -> str:
    return _color("33", text)


def _cyan(text: str) -> str:
    return _color("36", text)


def _bold(text: str) -> str:
    return _color("1", text)


def _dim(text: str) -> str:
    return _color("2", text)


# ---------------------------------------------------------------------------
# File reading
# ---------------------------------------------------------------------------

def _read_file(path: str) -> str:
    """Read a .kry source file."""
    p = Path(path)
    if not p.exists():
        print(_red(f"error: file not found: {path}"), file=sys.stderr)
        sys.exit(1)
    if not p.suffix == ".kry":
        print(_yellow(f"warning: file does not have .kry extension: {path}"), file=sys.stderr)
    return p.read_text(encoding="utf-8")


# ---------------------------------------------------------------------------
# Pipeline helpers
# ---------------------------------------------------------------------------

def _tokenize_source(source: str, filename: str = "<stdin>") -> list:
    """Tokenize source, printing errors on failure."""
    try:
        return tokenize(source, filename)
    except LexerError as e:
        print(_red(f"Lexer error: {e}"), file=sys.stderr)
        sys.exit(1)


def _parse_tokens(tokens: list):
    """Parse tokens into AST, printing errors on failure."""
    try:
        return parse(tokens)
    except (ParseError, ParseErrors) as e:
        print(_red(f"Parse error: {e}"), file=sys.stderr)
        sys.exit(1)


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

def cmd_run(args: argparse.Namespace) -> None:
    """Run a .kry file."""
    source = _read_file(args.file)
    tokens = _tokenize_source(source, args.file)
    module = _parse_tokens(tokens)

    no_heal = getattr(args, "no_heal", False)
    interp = Interpreter(self_healing=not no_heal)
    try:
        interp.run(module)
    except KryosRuntimeError as e:
        loc = f":{e.line}" if e.line else ""
        print(_red(f"Runtime error{loc}: {e}"), file=sys.stderr)
        # Show error explanation
        from kryos.compiler.ai_assist import ErrorExplainer
        explanation = ErrorExplainer().explain(str(e))
        print(_yellow(f"\n  Explanation: {explanation['explanation']}"), file=sys.stderr)
        print(_cyan(f"  Suggestion:  {explanation['suggestion']}"), file=sys.stderr)
        sys.exit(1)
    finally:
        if interp.heal_count > 0:
            print(_dim(f"\n[Kryos self-healed {interp.heal_count} issue(s) during execution]"), file=sys.stderr)


def cmd_build(args: argparse.Namespace) -> None:
    """Compile a .kry file (stub)."""
    source = _read_file(args.file)
    tokens = _tokenize_source(source, args.file)
    _parse_tokens(tokens)  # validate it parses
    print(_yellow("Compilation coming soon. Source parsed successfully."))


def cmd_check(args: argparse.Namespace) -> None:
    """Type-check and capability-audit a .kry file."""
    source = _read_file(args.file)
    tokens = _tokenize_source(source, args.file)
    module = _parse_tokens(tokens)

    # Run capability analysis
    from kryos.compiler.capabilities import CapabilityAnalyzer, CapabilityTier
    analyzer = CapabilityAnalyzer(tier=CapabilityTier.COMMUNITY)
    report = analyzer.analyze(module)

    if report["errors"]:
        print(_red(f"CAPABILITY VIOLATIONS in {args.file}:\n"))
        for err in report["errors"]:
            print(f"  {_red('ERROR')}: {err}")
        print()

    if report["warnings"]:
        for warn in report["warnings"]:
            print(f"  {_yellow('WARN')}: {warn}")

    # Print capability map
    if report["functions"]:
        print(_bold("Capability Map:"))
        for fn_name, info in report["functions"].items():
            caps = info["declared"]
            cap_str = ", ".join(caps) if caps else "compute (default)"
            print(f"  {_cyan(fn_name)}: [{cap_str}]")

    if not report["errors"]:
        print(_green(f"\nOK: {args.file} — all capability checks passed."))
    else:
        print(_red(f"\nFAILED: {len(report['errors'])} capability violation(s) found."))
        sys.exit(1)


def cmd_repl(args: argparse.Namespace) -> None:
    """Start an interactive REPL."""
    print(_bold(f"{__language__} v{__version__} REPL"))
    print(_dim("Type 'exit' or Ctrl+D to quit.\n"))

    interp = Interpreter()
    buffer = ""

    while True:
        try:
            prompt = _cyan("kryos> ") if not buffer else _dim("  ...> ")
            line = input(prompt)
        except (EOFError, KeyboardInterrupt):
            print("\nGoodbye.")
            break

        stripped = line.strip()
        if stripped == "exit" and not buffer:
            print("Goodbye.")
            break

        buffer += line + "\n"

        # Check for unclosed braces/parens
        if _has_unclosed(buffer):
            continue

        source = buffer.strip()
        buffer = ""

        if not source:
            continue

        try:
            tokens = tokenize(source, "<repl>")
            module = parse(tokens)
            interp.run(module)
        except LexerError as e:
            print(_red(f"Lexer error: {e}"))
        except (ParseError, ParseErrors) as e:
            print(_red(f"Parse error: {e}"))
        except KryosRuntimeError as e:
            print(_red(f"Runtime error: {e}"))


def _has_unclosed(source: str) -> bool:
    """Check if source has unclosed braces, brackets, or parens."""
    depth_brace = 0
    depth_paren = 0
    depth_bracket = 0
    in_string = False
    in_line_comment = False
    prev = ""

    for ch in source:
        if in_line_comment:
            if ch == "\n":
                in_line_comment = False
            prev = ch
            continue

        if not in_string and prev == "/" and ch == "/":
            in_line_comment = True
            prev = ch
            continue

        if ch == '"' and prev != "\\":
            in_string = not in_string

        if not in_string:
            if ch == "{":
                depth_brace += 1
            elif ch == "}":
                depth_brace -= 1
            elif ch == "(":
                depth_paren += 1
            elif ch == ")":
                depth_paren -= 1
            elif ch == "[":
                depth_bracket += 1
            elif ch == "]":
                depth_bracket -= 1

        prev = ch

    return depth_brace > 0 or depth_paren > 0 or depth_bracket > 0


def cmd_test(args: argparse.Namespace) -> None:
    """Run test files in a directory."""
    from kryos.tests.test_runner import run_tests
    test_dir = args.dir or "tests/programs"
    exit_code = run_tests(test_dir)
    sys.exit(exit_code)


def cmd_migrate(args: argparse.Namespace) -> None:
    """Migrate code from another language to Kryos."""
    from kryos.compiler.ai_assist import MigrationEngine

    source_path = Path(args.file)
    if not source_path.exists():
        print(_red(f"error: file not found: {args.file}"), file=sys.stderr)
        sys.exit(1)

    source = source_path.read_text(encoding="utf-8")
    engine = MigrationEngine()

    lang = args.lang or engine.detect_language(source)
    print(_dim(f"Detected language: {lang}"))

    result = engine.migrate(source, lang)

    print(_bold(f"\nMigration Result (confidence: {result.confidence:.0%})"))
    print("=" * 60)
    print(result.kryos_code)

    if result.notes:
        print(_yellow("\nNotes:"))
        for note in result.notes:
            print(f"  - {note}")

    if result.unmigrated:
        print(_red(f"\nUnmigrated ({len(result.unmigrated)} lines):"))
        for line in result.unmigrated[:10]:
            print(f"  - {line}")

    # Optionally write output
    if args.output:
        out_path = Path(args.output)
        out_path.write_text(result.kryos_code, encoding="utf-8")
        print(_green(f"\nWritten to {args.output}"))


def cmd_validate(args: argparse.Namespace) -> None:
    """Validate Kryos source code (AI-assist mode)."""
    from kryos.compiler.ai_assist import CodeValidator

    source = _read_file(args.file)
    validator = CodeValidator()
    result = validator.validate(source)

    if result.valid:
        print(_green(f"VALID: {args.file}"))
    else:
        print(_red(f"INVALID: {args.file}"))
        for err in result.errors:
            print(f"  {_red('ERROR')}: {err}")

    for warn in result.warnings:
        print(f"  {_yellow('WARN')}:  {warn}")

    for sug in result.suggestions:
        print(f"  {_cyan('HINT')}:  {sug}")

    if result.fixed_code:
        print(_green("\nAuto-fix available. Run with --fix to apply."))
        if getattr(args, "fix", False):
            Path(args.file).write_text(result.fixed_code, encoding="utf-8")
            print(_green(f"Fixed code written to {args.file}"))


def cmd_heal_report(args: argparse.Namespace) -> None:
    """Run a file and show the self-healing report."""
    source = _read_file(args.file)
    tokens = _tokenize_source(source, args.file)
    module = _parse_tokens(tokens)

    interp = Interpreter(self_healing=True)
    try:
        interp.run(module)
    except KryosRuntimeError as e:
        print(_red(f"Runtime error: {e}"), file=sys.stderr)

    print(_bold("\n" + interp.get_heal_report()))


def cmd_version(args: argparse.Namespace) -> None:
    """Show version information."""
    print(f"{__language__} v{__version__}")


# ---------------------------------------------------------------------------
# Main entry point
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(
        prog="kryos",
        description=f"{__language__} Programming Language v{__version__}",
    )
    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # run
    run_parser = subparsers.add_parser("run", help="Run a .kry file")
    run_parser.add_argument("file", help="Path to .kry source file")
    run_parser.add_argument("--no-heal", action="store_true", help="Disable self-healing")

    # build
    build_parser = subparsers.add_parser("build", help="Compile a .kry file")
    build_parser.add_argument("file", help="Path to .kry source file")

    # check
    check_parser = subparsers.add_parser("check", help="Type-check and audit a .kry file")
    check_parser.add_argument("file", help="Path to .kry source file")

    # repl
    subparsers.add_parser("repl", help="Start interactive REPL")

    # test
    test_parser = subparsers.add_parser("test", help="Run test files")
    test_parser.add_argument("dir", nargs="?", default=None, help="Test directory")

    # migrate
    migrate_parser = subparsers.add_parser("migrate", help="Convert code from another language to Kryos")
    migrate_parser.add_argument("file", help="Source file to migrate")
    migrate_parser.add_argument("--lang", help="Source language (auto-detected if omitted)")
    migrate_parser.add_argument("--output", "-o", help="Output .kry file")

    # validate
    validate_parser = subparsers.add_parser("validate", help="Validate Kryos code (AI-assist)")
    validate_parser.add_argument("file", help="Path to .kry source file")
    validate_parser.add_argument("--fix", action="store_true", help="Auto-fix issues")

    # heal-report
    heal_parser = subparsers.add_parser("heal-report", help="Run file and show self-healing report")
    heal_parser.add_argument("file", help="Path to .kry source file")

    # version
    subparsers.add_parser("version", help="Show version")

    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        sys.exit(0)

    commands = {
        "run": cmd_run,
        "build": cmd_build,
        "check": cmd_check,
        "repl": cmd_repl,
        "test": cmd_test,
        "migrate": cmd_migrate,
        "validate": cmd_validate,
        "heal-report": cmd_heal_report,
        "version": cmd_version,
    }

    cmd_fn = commands.get(args.command)
    if cmd_fn:
        cmd_fn(args)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
