"""Tests for Kryos standard library string extensions module."""
import contextlib
import io
import unittest

from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.interpreter import Interpreter


class TestStdlibStringExt(unittest.TestCase):
    def _run(self, source: str) -> str:
        tokens = tokenize(source)
        module = parse(tokens)
        f = io.StringIO()
        with contextlib.redirect_stdout(f):
            interp = Interpreter()
            interp.run(module)
        return f.getvalue().strip()

    # ---- string_repeat ----

    def test_string_repeat(self):
        result = self._run('println(string_repeat("ab", 3))')
        self.assertEqual(result, "ababab")

    # ---- string_pad_left ----

    def test_string_pad_left(self):
        result = self._run('println(to_string(len(string_pad_left("hi", 5))))')
        self.assertEqual(result, "5")

    def test_string_pad_left_custom_char(self):
        result = self._run('println(string_pad_left("hi", 5, "0"))')
        self.assertEqual(result, "000hi")

    # ---- string_pad_right ----

    def test_string_pad_right(self):
        result = self._run('println(to_string(len(string_pad_right("hi", 5))))')
        self.assertEqual(result, "5")

    def test_string_pad_right_custom_char(self):
        result = self._run('println(string_pad_right("hi", 5, "."))')
        self.assertEqual(result, "hi...")

    # ---- string_lines ----

    def test_string_lines(self):
        result = self._run(r'''
let lines = string_lines("a\nb\nc")
println(to_string(len(lines)))
println(lines[0])
println(lines[1])
println(lines[2])
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "3")
        self.assertEqual(lines[1], "a")
        self.assertEqual(lines[2], "b")
        self.assertEqual(lines[3], "c")

    # ---- to_int ----

    def test_to_int(self):
        result = self._run('println(to_string(to_int("42")))')
        self.assertEqual(result, "42")

    # ---- to_float ----

    def test_to_float(self):
        result = self._run('println(to_string(to_float("3.14")))')
        self.assertIn("3.14", result)

    # ---- string_index ----

    def test_string_index(self):
        result = self._run('println(to_string(string_index("hello", "ll")))')
        self.assertEqual(result, "2")

    def test_string_index_not_found(self):
        result = self._run('println(to_string(string_index("hello", "xyz")))')
        self.assertEqual(result, "-1")

    # ---- string_count ----

    def test_string_count(self):
        result = self._run('println(to_string(string_count("abcabc", "abc")))')
        self.assertEqual(result, "2")

    def test_string_count_no_match(self):
        result = self._run('println(to_string(string_count("hello", "xyz")))')
        self.assertEqual(result, "0")


if __name__ == "__main__":
    unittest.main()
