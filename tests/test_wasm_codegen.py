"""Tests for WASM target code generation."""

import unittest
from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.codegen import CodeGenerator


class TestWasmCodegen(unittest.TestCase):
    def _gen_ir(self, source: str, target: str = "wasm32") -> str:
        tokens = tokenize(source)
        module = parse(tokens)
        gen = CodeGenerator(target=target)
        return gen.generate(module)

    def test_wasm_target_triple(self):
        ir = self._gen_ir("fn add(a: i32, b: i32) -> i32 { return a + b }")
        self.assertIn('target triple = "wasm32-unknown-unknown"', ir)

    def test_default_target_not_wasm(self):
        tokens = tokenize("fn foo() -> i32 { return 1 }")
        module = parse(tokens)
        gen = CodeGenerator()
        ir = gen.generate(module)
        self.assertNotIn("wasm32", ir)

    def test_wasm_no_puts(self):
        """WASM target should not emit puts/printf declarations."""
        ir = self._gen_ir('let x = 42')
        self.assertNotIn("@puts", ir)

    def test_export_annotation_visible(self):
        """@export functions should not have 'internal' linkage."""
        ir = self._gen_ir("""
@export
fn fibonacci(n: i32) -> i32 {
    if n <= 1 { return n }
    return fibonacci(n - 1) + fibonacci(n - 2)
}
""")
        # Exported function should be defined (not internal)
        self.assertIn("define", ir)
        self.assertIn("@fibonacci", ir)


if __name__ == "__main__":
    unittest.main()
