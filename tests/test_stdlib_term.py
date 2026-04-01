"""Tests for Kryos standard library - Terminal module."""
import unittest
from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.interpreter import Interpreter


class TestStdlibTerm(unittest.TestCase):
    def _run(self, source: str) -> str:
        tokens = tokenize(source)
        module = parse(tokens)
        import io
        import contextlib
        f = io.StringIO()
        with contextlib.redirect_stdout(f):
            interp = Interpreter()
            interp.run(module)
        return f.getvalue()

    # ---- Styling (return string) ----

    def test_term_bold(self):
        result = self._run('println(term_bold("hello"))')
        self.assertIn("\033[1m", result)
        self.assertIn("hello", result)
        self.assertIn("\033[0m", result)

    def test_term_dim(self):
        result = self._run('println(term_dim("dim"))')
        self.assertIn("\033[2m", result)
        self.assertIn("dim", result)
        self.assertIn("\033[0m", result)

    def test_term_underline(self):
        result = self._run('println(term_underline("uline"))')
        self.assertIn("\033[4m", result)
        self.assertIn("uline", result)
        self.assertIn("\033[0m", result)

    def test_term_rgb(self):
        result = self._run('println(term_rgb(255, 0, 0, "red"))')
        self.assertIn("\033[38;2;255;0;0m", result)
        self.assertIn("red", result)
        self.assertIn("\033[0m", result)

    # ---- Output Control (writes to stdout) ----

    def test_term_clear(self):
        result = self._run('term_clear()')
        self.assertIn("\033[2J", result)

    def test_term_move(self):
        result = self._run('term_move(5, 10)')
        self.assertIn("\033[5;10H", result)

    def test_term_hide_cursor(self):
        result = self._run('term_hide_cursor()')
        self.assertIn("\033[?25l", result)

    def test_term_show_cursor(self):
        result = self._run('term_show_cursor()')
        self.assertIn("\033[?25h", result)

    # ---- Screen Management ----

    def test_term_alt_screen(self):
        result = self._run('term_alt_screen()')
        self.assertIn("\033[?1049h", result)

    def test_term_main_screen(self):
        result = self._run('term_main_screen()')
        self.assertIn("\033[?1049l", result)

    # ---- Reset ----

    def test_term_reset(self):
        result = self._run('term_reset()')
        self.assertIn("\033[0m", result)

    # ---- Color ----

    def test_term_color_fg(self):
        result = self._run('term_color("red")')
        self.assertIn("\033[31m", result)

    def test_term_color_fg_bg(self):
        result = self._run('term_color("white", "blue")')
        self.assertIn("\033[37m", result)
        self.assertIn("\033[44m", result)

    # ---- Write ----

    def test_term_write(self):
        result = self._run('term_write("no newline")')
        self.assertEqual(result, "no newline")

    # ---- Size ----

    def test_term_size(self):
        result = self._run('''
let s = term_size()
println(to_string(len(s)))
println(to_string(s[0] > 0))
println(to_string(s[1] > 0))
''')
        lines = result.strip().split("\n")
        self.assertEqual(lines[0], "2")
        self.assertEqual(lines[1], "true")
        self.assertEqual(lines[2], "true")


if __name__ == "__main__":
    unittest.main()
