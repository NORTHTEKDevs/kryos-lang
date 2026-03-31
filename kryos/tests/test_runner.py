"""
Kryos Test Runner
Finds .kry test files, runs them, compares output to expected values in comments.

Test file format:
    // expect: Hello World
    // expect: 42
    // expect_error: undefined variable

    println("Hello World")
    println(42)
"""

from __future__ import annotations

import os
import sys
import time
from pathlib import Path

from kryos.compiler.lexer import tokenize, LexerError
from kryos.compiler.parser import parse, ParseError, ParseErrors
from kryos.compiler.interpreter import Interpreter, KryosRuntimeError


def _extract_expectations(source: str) -> tuple[list[str], list[str]]:
    """Extract expected output and expected errors from source comments."""
    expected_output = []
    expected_errors = []
    for line in source.splitlines():
        stripped = line.strip()
        if stripped.startswith("// expect:"):
            expected_output.append(stripped[len("// expect:"):].strip())
        elif stripped.startswith("// expect_error:"):
            expected_errors.append(stripped[len("// expect_error:"):].strip())
    return expected_output, expected_errors


def run_single_test(filepath: Path) -> tuple[bool, str]:
    """Run a single .kry test file. Returns (passed, message)."""
    source = filepath.read_text(encoding="utf-8")
    expected_output, expected_errors = _extract_expectations(source)

    if not expected_output and not expected_errors:
        return True, "SKIP (no expectations)"

    try:
        tokens = tokenize(source, str(filepath))
        module = parse(tokens)
        interp = Interpreter(output_fn=lambda s: None)  # suppress stdout
        interp.run(module)
        actual_output = interp.output
    except (LexerError, ParseError, ParseErrors, KryosRuntimeError) as e:
        error_msg = str(e)
        if expected_errors:
            for exp_err in expected_errors:
                if exp_err.lower() in error_msg.lower():
                    return True, "PASS (expected error matched)"
            return False, f"FAIL: expected error containing '{expected_errors[0]}', got: {error_msg}"
        return False, f"FAIL: unexpected error: {error_msg}"

    if expected_errors:
        return False, f"FAIL: expected error but program ran successfully"

    if len(actual_output) != len(expected_output):
        return False, (
            f"FAIL: expected {len(expected_output)} output lines, got {len(actual_output)}\n"
            f"  Expected: {expected_output}\n"
            f"  Actual:   {actual_output}"
        )

    for i, (expected, actual) in enumerate(zip(expected_output, actual_output)):
        if expected != actual:
            return False, (
                f"FAIL: line {i + 1} mismatch\n"
                f"  Expected: {expected!r}\n"
                f"  Actual:   {actual!r}"
            )

    return True, "PASS"


def run_tests(test_dir: str) -> int:
    """Run all .kry test files in a directory. Returns exit code."""
    test_path = Path(test_dir)
    if not test_path.exists():
        print(f"Error: test directory not found: {test_dir}", file=sys.stderr)
        return 1

    test_files = sorted(test_path.glob("**/*.kry"))
    if not test_files:
        print(f"No .kry test files found in {test_dir}")
        return 0

    passed = 0
    failed = 0
    skipped = 0
    start_time = time.time()

    print(f"\nRunning {len(test_files)} test(s) from {test_dir}\n")
    print("-" * 60)

    for filepath in test_files:
        rel_path = filepath.relative_to(test_path)
        success, message = run_single_test(filepath)

        if "SKIP" in message:
            skipped += 1
            status = "\033[33mSKIP\033[0m"
        elif success:
            passed += 1
            status = "\033[32mPASS\033[0m"
        else:
            failed += 1
            status = "\033[31mFAIL\033[0m"

        print(f"  {status}  {rel_path}")
        if not success and "SKIP" not in message:
            for line in message.split("\n"):
                if line.startswith("FAIL:"):
                    line = line[5:].strip()
                print(f"         {line}")

    elapsed = time.time() - start_time
    print("-" * 60)
    print(f"\nResults: {passed} passed, {failed} failed, {skipped} skipped ({elapsed:.2f}s)")

    return 0 if failed == 0 else 1
