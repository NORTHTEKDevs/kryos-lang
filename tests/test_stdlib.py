"""Tests for Kryos standard library modules."""
import unittest
from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.interpreter import Interpreter


class TestStdlib(unittest.TestCase):
    def _run(self, source: str) -> str:
        tokens = tokenize(source)
        module = parse(tokens)
        import io
        import contextlib
        f = io.StringIO()
        with contextlib.redirect_stdout(f):
            interp = Interpreter()
            interp.run(module)
        return f.getvalue().strip()

    # ---- String utilities ----

    def test_string_split(self):
        result = self._run('println(to_string(len(split("a,b,c", ","))))')
        self.assertEqual(result, "3")

    def test_string_trim(self):
        result = self._run('println(trim("  hello  "))')
        self.assertEqual(result, "hello")

    def test_string_replace(self):
        result = self._run('println(replace("hello world", "world", "kryos"))')
        self.assertEqual(result, "hello kryos")

    def test_string_starts_with(self):
        result = self._run('println(to_string(starts_with("kryos", "kry")))')
        self.assertEqual(result, "true")

    def test_string_ends_with(self):
        result = self._run('println(to_string(ends_with("kryos", "os")))')
        self.assertEqual(result, "true")

    def test_string_to_upper(self):
        result = self._run('println(to_upper("hello"))')
        self.assertEqual(result, "HELLO")

    def test_string_to_lower(self):
        result = self._run('println(to_lower("HELLO"))')
        self.assertEqual(result, "hello")

    def test_string_contains(self):
        result = self._run('println(to_string(contains("hello world", "world")))')
        self.assertEqual(result, "true")

    def test_string_char_at(self):
        result = self._run('println(char_at("kryos", 0))')
        self.assertEqual(result, "k")

    def test_string_substr(self):
        result = self._run('println(substr("kryos", 1, 4))')
        self.assertEqual(result, "ryo")

    # ---- Math extensions ----

    def test_math_floor(self):
        result = self._run('println(to_string(floor(3.7)))')
        self.assertEqual(result, "3")

    def test_math_ceil(self):
        result = self._run('println(to_string(ceil(3.2)))')
        self.assertEqual(result, "4")

    def test_math_round(self):
        result = self._run('println(to_string(round(3.5)))')
        self.assertEqual(result, "4")

    def test_math_sin(self):
        result = self._run('println(to_string(sin(0.0)))')
        self.assertEqual(result, "0.0")

    def test_math_cos(self):
        result = self._run('println(to_string(cos(0.0)))')
        self.assertEqual(result, "1.0")

    def test_math_log10(self):
        result = self._run('println(to_string(log10(100.0)))')
        self.assertEqual(result, "2.0")

    def test_math_pow(self):
        result = self._run('println(to_string(pow(2.0, 10.0)))')
        self.assertEqual(result, "1024.0")

    def test_math_pi(self):
        result = self._run('println(to_string(pi()))')
        self.assertIn("3.14159", result)

    def test_math_e(self):
        result = self._run('println(to_string(e()))')
        self.assertIn("2.718", result)

    def test_math_random(self):
        result = self._run('let r = random()\nprintln(to_string(r >= 0.0))')
        self.assertEqual(result, "true")

    # ---- Collection utilities ----

    def test_sort(self):
        result = self._run('''
let arr = [3, 1, 4, 1, 5]
let sorted = sort(arr)
println(to_string(sorted[0]))
println(to_string(sorted[4]))
''')
        self.assertIn("1", result)
        self.assertIn("5", result)

    def test_reverse(self):
        result = self._run('''
let arr = [1, 2, 3]
let rev = reverse(arr)
println(to_string(rev[0]))
println(to_string(rev[2]))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "3")
        self.assertEqual(lines[1], "1")

    def test_filter(self):
        result = self._run('''
fn is_even(x: i32) -> bool { return x % 2 == 0 }
let nums = [1, 2, 3, 4, 5, 6]
let evens = filter(nums, is_even)
println(to_string(len(evens)))
''')
        self.assertEqual(result, "3")

    def test_reduce(self):
        result = self._run('''
fn add(a: i32, b: i32) -> i32 { return a + b }
let nums = [1, 2, 3, 4, 5]
println(to_string(reduce(nums, add, 0)))
''')
        self.assertEqual(result, "15")

    def test_map(self):
        result = self._run('''
fn double(x: i32) -> i32 { return x * 2 }
let nums = [1, 2, 3]
let doubled = map(nums, double)
println(to_string(doubled[0]))
println(to_string(doubled[1]))
println(to_string(doubled[2]))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "2")
        self.assertEqual(lines[1], "4")
        self.assertEqual(lines[2], "6")

    def test_zip(self):
        result = self._run('''
let a = [1, 2, 3]
let b = ["a", "b", "c"]
let zipped = zip(a, b)
println(to_string(len(zipped)))
''')
        self.assertEqual(result, "3")

    def test_enumerate(self):
        result = self._run('''
let arr = ["x", "y", "z"]
let pairs = enumerate(arr)
println(to_string(pairs[0][0]))
println(pairs[0][1])
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "0")
        self.assertEqual(lines[1], "x")

    def test_any(self):
        result = self._run('''
fn is_negative(x: i32) -> bool { return x < 0 }
let nums = [1, 2, -3, 4]
println(to_string(any(nums, is_negative)))
''')
        self.assertEqual(result, "true")

    def test_all(self):
        result = self._run('''
fn is_positive(x: i32) -> bool { return x > 0 }
let nums = [1, 2, 3, 4]
println(to_string(all(nums, is_positive)))
''')
        self.assertEqual(result, "true")

    def test_sum(self):
        result = self._run('''
let nums = [10, 20, 30]
println(to_string(sum(nums)))
''')
        self.assertEqual(result, "60")

    def test_count(self):
        result = self._run('''
let arr = [1, 2, 3, 4, 5]
println(to_string(count(arr)))
''')
        self.assertEqual(result, "5")

    def test_find(self):
        result = self._run('''
fn is_big(x: i32) -> bool { return x > 10 }
let nums = [1, 5, 15, 20]
println(to_string(find(nums, is_big)))
''')
        self.assertEqual(result, "15")

    def test_flat_map(self):
        result = self._run('''
fn duplicate(x: i32) -> [i32] { return [x, x] }
let nums = [1, 2, 3]
let result = flat_map(nums, duplicate)
println(to_string(len(result)))
''')
        self.assertEqual(result, "6")

    # ---- JSON module ----

    def test_json_parse(self):
        result = self._run('''
let obj = json_parse("\\{\\"name\\": \\"Kryos\\"\\}")
println(json_get(obj, "name"))
''')
        self.assertEqual(result, "Kryos")

    def test_json_stringify(self):
        result = self._run('''
let arr = [1, 2, 3]
println(json_stringify(arr))
''')
        self.assertEqual(result, "[1, 2, 3]")

    def test_json_has(self):
        result = self._run('''
let obj = json_parse("\\{\\"key\\": 42\\}")
println(to_string(json_has(obj, "key")))
println(to_string(json_has(obj, "nope")))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "true")
        self.assertEqual(lines[1], "false")

    def test_json_get_missing(self):
        result = self._run('''
let obj = json_parse("\\{\\"a\\": 1\\}")
let val = json_get(obj, "missing")
println(to_string(val))
''')
        self.assertEqual(result, "none")


if __name__ == "__main__":
    unittest.main()
