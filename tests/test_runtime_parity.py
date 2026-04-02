"""
Run all test programs on both runtimes and compare outputs.

Generates a test case per .kry program in tests/programs/.
Skips if kryos-runner binary is not available.
"""

import subprocess
import sys
import unittest
from pathlib import Path

PROGRAMS_DIR = Path(__file__).parent / "programs"


def _runner_available() -> bool:
    """Check if Rust VM runner is available."""
    try:
        result = subprocess.run(
            [sys.executable, "-m", "kryos", "run", "--runtime", "rust", "--help"],
            capture_output=True, text=True, timeout=10,
        )
        return result.returncode == 0
    except Exception:
        return False


class TestRuntimeParity(unittest.TestCase):
    """Dynamically generated parity tests between Python and Rust runtimes."""
    pass


def _make_parity_test(program_path: Path):
    """Create a test method that compares outputs between Python and Rust runtimes."""
    def test(self):
        # Run on Python interpreter
        py_result = subprocess.run(
            [sys.executable, "-m", "kryos", "run", "--runtime", "python", str(program_path)],
            capture_output=True, text=True, timeout=30,
        )

        # Run on Rust VM
        rust_result = subprocess.run(
            [sys.executable, "-m", "kryos", "run", "--runtime", "rust", str(program_path)],
            capture_output=True, text=True, timeout=30,
        )

        self.assertEqual(
            py_result.stdout, rust_result.stdout,
            f"Output mismatch for {program_path.name}:\n"
            f"Python: {py_result.stdout!r}\nRust: {rust_result.stdout!r}"
        )
    return test


# Generate a test for each .kry program in tests/programs/
if PROGRAMS_DIR.exists():
    for prog in sorted(PROGRAMS_DIR.glob("*.kry")):
        test_name = f"test_parity_{prog.stem}"
        setattr(TestRuntimeParity, test_name, _make_parity_test(prog))


if __name__ == "__main__":
    unittest.main()
