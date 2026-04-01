"""Tests for compile-time evaluation (comptime blocks)."""
import io
import contextlib
import unittest

from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.interpreter import Interpreter
from kryos.compiler.comptime import ComptimeEvaluator, ComptimeTransformer
from kryos.compiler.ast_nodes import (
    ComptimeBlock, IntLiteral, FloatLiteral, StringLiteral,
    BoolLiteral, NoneLiteral, ArrayLiteral, ExprStmt, LetStmt,
    BinaryOp, Identifier, Module,
)


class TestComptimeEvaluator(unittest.TestCase):
    """Test the ComptimeEvaluator in isolation with hand-built AST nodes."""

    def test_evaluate_int_arithmetic(self):
        """Evaluate comptime { 10 + 20 } via AST nodes."""
        block = ComptimeBlock(body=[
            ExprStmt(expression=BinaryOp(
                left=IntLiteral(value=10),
                operator="+",
                right=IntLiteral(value=20),
            )),
        ])
        evaluator = ComptimeEvaluator()
        result = evaluator.evaluate(block)
        self.assertEqual(result, 30)

    def test_evaluate_let_and_expression(self):
        """Evaluate comptime { let x = 5; x * 3 }."""
        block = ComptimeBlock(body=[
            LetStmt(name="x", value=IntLiteral(value=5)),
            ExprStmt(expression=BinaryOp(
                left=Identifier(name="x"),
                operator="*",
                right=IntLiteral(value=3),
            )),
        ])
        evaluator = ComptimeEvaluator()
        result = evaluator.evaluate(block)
        self.assertEqual(result, 15)

    def test_evaluate_string(self):
        """Evaluate comptime { "hello" }."""
        block = ComptimeBlock(body=[
            ExprStmt(expression=StringLiteral(value="hello")),
        ])
        evaluator = ComptimeEvaluator()
        result = evaluator.evaluate(block)
        self.assertEqual(result, "hello")

    def test_evaluate_bool(self):
        """Evaluate comptime { true }."""
        block = ComptimeBlock(body=[
            ExprStmt(expression=BoolLiteral(value=True)),
        ])
        evaluator = ComptimeEvaluator()
        result = evaluator.evaluate(block)
        self.assertTrue(result)

    def test_evaluate_caching(self):
        """Same block evaluated twice returns cached result."""
        block = ComptimeBlock(body=[
            ExprStmt(expression=IntLiteral(value=42)),
        ])
        evaluator = ComptimeEvaluator()
        r1 = evaluator.evaluate(block)
        r2 = evaluator.evaluate(block)
        self.assertEqual(r1, 42)
        self.assertEqual(r2, 42)

    def test_value_to_ast_int(self):
        evaluator = ComptimeEvaluator()
        node = evaluator.value_to_ast(42)
        self.assertIsInstance(node, IntLiteral)
        self.assertEqual(node.value, 42)

    def test_value_to_ast_float(self):
        evaluator = ComptimeEvaluator()
        node = evaluator.value_to_ast(3.14)
        self.assertIsInstance(node, FloatLiteral)
        self.assertAlmostEqual(node.value, 3.14)

    def test_value_to_ast_string(self):
        evaluator = ComptimeEvaluator()
        node = evaluator.value_to_ast("hello")
        self.assertIsInstance(node, StringLiteral)
        self.assertEqual(node.value, "hello")

    def test_value_to_ast_bool(self):
        evaluator = ComptimeEvaluator()
        node = evaluator.value_to_ast(True)
        self.assertIsInstance(node, BoolLiteral)
        self.assertEqual(node.value, True)

    def test_value_to_ast_none(self):
        evaluator = ComptimeEvaluator()
        node = evaluator.value_to_ast(None)
        self.assertIsInstance(node, NoneLiteral)

    def test_value_to_ast_list(self):
        evaluator = ComptimeEvaluator()
        node = evaluator.value_to_ast([1, 2, 3])
        self.assertIsInstance(node, ArrayLiteral)
        self.assertEqual(len(node.elements), 3)
        self.assertIsInstance(node.elements[0], IntLiteral)
        self.assertEqual(node.elements[0].value, 1)

    def test_value_to_llvm_int(self):
        evaluator = ComptimeEvaluator()
        self.assertEqual(evaluator.value_to_llvm_constant(42), "i64 42")

    def test_value_to_llvm_bool(self):
        evaluator = ComptimeEvaluator()
        self.assertEqual(evaluator.value_to_llvm_constant(True), "i1 1")
        self.assertEqual(evaluator.value_to_llvm_constant(False), "i1 0")

    def test_value_to_llvm_float(self):
        evaluator = ComptimeEvaluator()
        result = evaluator.value_to_llvm_constant(3.14)
        self.assertTrue(result.startswith("double 0x"))

    def test_value_to_llvm_none(self):
        evaluator = ComptimeEvaluator()
        self.assertEqual(evaluator.value_to_llvm_constant(None), "i64 0")


class TestComptimeTransformer(unittest.TestCase):
    """Test the ComptimeTransformer on hand-built AST."""

    def test_transform_let_with_comptime(self):
        """let x = comptime { 10 + 20 } becomes let x = 30."""
        block = ComptimeBlock(body=[
            ExprStmt(expression=BinaryOp(
                left=IntLiteral(value=10),
                operator="+",
                right=IntLiteral(value=20),
            )),
        ])
        module = Module(declarations=[
            LetStmt(name="x", value=block),
        ])
        transformer = ComptimeTransformer()
        module = transformer.transform(module)
        self.assertEqual(len(transformer.errors), 0)

        let_stmt = module.declarations[0]
        self.assertIsInstance(let_stmt, LetStmt)
        self.assertIsInstance(let_stmt.value, IntLiteral)
        self.assertEqual(let_stmt.value.value, 30)

    def test_transform_no_comptime_passthrough(self):
        """Nodes without comptime pass through unchanged."""
        module = Module(declarations=[
            LetStmt(name="x", value=IntLiteral(value=42)),
        ])
        transformer = ComptimeTransformer()
        module = transformer.transform(module)
        self.assertEqual(len(transformer.errors), 0)

        let_stmt = module.declarations[0]
        self.assertIsInstance(let_stmt.value, IntLiteral)
        self.assertEqual(let_stmt.value.value, 42)


class TestComptimeEndToEnd(unittest.TestCase):
    """End-to-end tests: parse source -> transform comptime -> interpret."""

    def _run(self, source: str) -> str:
        """Parse, transform comptime blocks, then interpret."""
        tokens = tokenize(source)
        module = parse(tokens)
        # Transform comptime blocks before interpretation
        transformer = ComptimeTransformer()
        module = transformer.transform(module)
        if transformer.errors:
            raise RuntimeError(
                f"comptime errors: {'; '.join(transformer.errors)}"
            )
        # Run through interpreter
        f = io.StringIO()
        with contextlib.redirect_stdout(f):
            interp = Interpreter()
            interp.run(module)
        return f.getvalue().strip()

    def test_comptime_int(self):
        result = self._run('''
let x = comptime {
    let a = 10
    let b = 20
    a + b
}
println(to_string(x))
''')
        self.assertEqual(result, "30")

    def test_comptime_nested_math(self):
        result = self._run('''
let val = comptime {
    let a = 3
    let b = 4
    a * a + b * b
}
println(to_string(val))
''')
        self.assertEqual(result, "25")

    def test_comptime_string(self):
        result = self._run('''
let greeting = comptime {
    let parts = ["hello", " ", "world"]
    join("", parts)
}
println(greeting)
''')
        self.assertEqual(result, "hello world")

    def test_comptime_array(self):
        result = self._run('''
let squares = comptime {
    let mut arr = []
    for i in range(5) {
        push(arr, i * i)
    }
    arr
}
println(to_string(len(squares)))
println(to_string(squares[4]))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "5")
        self.assertEqual(lines[1], "16")

    def test_comptime_in_function(self):
        result = self._run('''
fn get_table() {
    return comptime {
        let mut t = []
        for i in range(10) {
            push(t, i * 2)
        }
        t
    }
}
let table = get_table()
println(to_string(table[5]))
''')
        self.assertEqual(result, "10")

    def test_no_comptime_passthrough(self):
        """Regular code without comptime works normally."""
        result = self._run('''
let x = 42
println(to_string(x))
''')
        self.assertEqual(result, "42")

    def test_comptime_bool_result(self):
        result = self._run('''
let flag = comptime {
    let x = 10
    x > 5
}
println(to_string(flag))
''')
        self.assertEqual(result, "true")

    def test_comptime_float_result(self):
        result = self._run('''
let pi = comptime {
    3.14159
}
println(to_string(pi > 3.0))
''')
        self.assertEqual(result, "true")

    def test_comptime_multiple_blocks(self):
        """Multiple comptime blocks in the same module."""
        result = self._run('''
let a = comptime { 10 + 5 }
let b = comptime { 20 * 2 }
println(to_string(a + b))
''')
        self.assertEqual(result, "55")


if __name__ == "__main__":
    unittest.main()
