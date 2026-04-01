"""Tests for Kryos standard library map/dict module."""
import contextlib
import io
import unittest

from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.interpreter import Interpreter


class TestStdlibMap(unittest.TestCase):
    def _run(self, source: str) -> str:
        tokens = tokenize(source)
        module = parse(tokens)
        f = io.StringIO()
        with contextlib.redirect_stdout(f):
            interp = Interpreter()
            interp.run(module)
        return f.getvalue().strip()

    # ---- map_new ----

    def test_map_new_empty(self):
        result = self._run('''
let m = map_new()
println(to_string(len(map_keys(m))))
''')
        self.assertEqual(result, "0")

    # ---- map_set + map_get ----

    def test_map_set_and_get(self):
        result = self._run('''
let m = map_new()
let m2 = map_set(m, "name", "kryos")
println(map_get(m2, "name"))
''')
        self.assertEqual(result, "kryos")

    def test_map_set_immutable(self):
        """map_set should not mutate the original map."""
        result = self._run('''
let m = map_new()
let m2 = map_set(m, "key", "val")
println(to_string(len(map_keys(m))))
println(to_string(len(map_keys(m2))))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "0")
        self.assertEqual(lines[1], "1")

    def test_map_get_default(self):
        result = self._run('''
let m = map_new()
println(to_string(map_get(m, "missing", 42)))
''')
        self.assertEqual(result, "42")

    def test_map_get_missing_none(self):
        result = self._run('''
let m = map_new()
println(to_string(map_get(m, "missing")))
''')
        self.assertEqual(result, "none")

    # ---- map_from ----

    def test_map_from(self):
        result = self._run('''
let m = map_from("a", 1, "b", 2)
println(to_string(map_get(m, "a")))
println(to_string(map_get(m, "b")))
println(to_string(len(map_keys(m))))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "1")
        self.assertEqual(lines[1], "2")
        self.assertEqual(lines[2], "2")

    # ---- map_has ----

    def test_map_has(self):
        result = self._run('''
let m = map_from("x", 10)
println(to_string(map_has(m, "x")))
println(to_string(map_has(m, "y")))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "true")
        self.assertEqual(lines[1], "false")

    # ---- map_keys ----

    def test_map_keys(self):
        result = self._run('''
let m = map_from("alpha", 1, "beta", 2)
let keys = sort(map_keys(m))
println(keys[0])
println(keys[1])
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "alpha")
        self.assertEqual(lines[1], "beta")

    # ---- map_values ----

    def test_map_values(self):
        result = self._run('''
let m = map_from("a", 10, "b", 20)
let vals = sort(map_values(m))
println(to_string(vals[0]))
println(to_string(vals[1]))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "10")
        self.assertEqual(lines[1], "20")

    # ---- map_remove ----

    def test_map_remove(self):
        result = self._run('''
let m = map_from("a", 1, "b", 2)
let m2 = map_remove(m, "a")
println(to_string(map_has(m2, "a")))
println(to_string(map_has(m2, "b")))
println(to_string(len(map_keys(m2))))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "false")
        self.assertEqual(lines[1], "true")
        self.assertEqual(lines[2], "1")

    # ---- map_merge ----

    def test_map_merge(self):
        result = self._run('''
let a = map_from("x", 1, "y", 2)
let b = map_from("y", 99, "z", 3)
let merged = map_merge(a, b)
println(to_string(map_get(merged, "x")))
println(to_string(map_get(merged, "y")))
println(to_string(map_get(merged, "z")))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "1")
        self.assertEqual(lines[1], "99")
        self.assertEqual(lines[2], "3")


if __name__ == "__main__":
    unittest.main()
