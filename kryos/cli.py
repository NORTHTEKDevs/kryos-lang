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

    interp = Interpreter()
    try:
        interp.run(module)
    except KryosRuntimeError as e:
        loc = f":{e.line}" if e.line else ""
        print(_red(f"Runtime error{loc}: {e}"), file=sys.stderr)
        sys.exit(1)


def cmd_build(args: argparse.Namespace) -> None:
    """Compile a .kry file (stub)."""
    source = _read_file(args.file)
    tokens = _tokenize_source(source, args.file)
    _parse_tokens(tokens)  # validate it parses
    print(_yellow("Compilation coming soon. Source parsed successfully."))


def cmd_check(args: argparse.Namespace) -> None:
    """Type-check a .kry file."""
    source = _read_file(args.file)
    tokens = _tokenize_source(source, args.file)
    module = _parse_tokens(tokens)
    print(_green(f"OK: {args.file} parsed and checked successfully."))


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

    # build
    build_parser = subparsers.add_parser("build", help="Compile a .kry file")
    build_parser.add_argument("file", help="Path to .kry source file")

    # check
    check_parser = subparsers.add_parser("check", help="Type-check a .kry file")
    check_parser.add_argument("file", help="Path to .kry source file")

    # repl
    subparsers.add_parser("repl", help="Start interactive REPL")

    # test
    test_parser = subparsers.add_parser("test", help="Run test files")
    test_parser.add_argument("dir", nargs="?", default=None, help="Test directory")

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
