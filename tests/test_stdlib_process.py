"""Tests for Kryos standard library process module."""
import contextlib
import io
import unittest

from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.interpreter import Interpreter


class TestStdlibProcess(unittest.TestCase):
    def _run(self, source: str) -> str:
        tokens = tokenize(source)
        module = parse(tokens)
        f = io.StringIO()
        with contextlib.redirect_stdout(f):
            interp = Interpreter()
            interp.run(module)
        return f.getvalue().strip()

    # ---- exec ----

    def test_exec_echo(self):
        result = self._run('''
let r = exec("echo hello")
println(trim(map_get(r, "stdout")))
println(to_string(map_get(r, "exit_code")))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "hello")
        self.assertEqual(lines[1], "0")

    def test_exec_bad_command(self):
        result = self._run('''
let r = exec("exit 1")
println(to_string(map_get(r, "exit_code") != 0))
''')
        self.assertEqual(result, "true")

    # ---- exec_capture ----

    def test_exec_capture(self):
        result = self._run('''
let out = exec_capture("echo hello")
println(trim(out))
''')
        self.assertEqual(result, "hello")

    # ---- sleep ----

    def test_sleep_zero(self):
        result = self._run('''
sleep(0)
println("ok")
''')
        self.assertEqual(result, "ok")


if __name__ == "__main__":
    unittest.main()
