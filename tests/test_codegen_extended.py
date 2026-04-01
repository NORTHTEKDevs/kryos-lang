"""Tests for extended LLVM codegen features.

Tests lambda expressions, enum types, impl blocks, method calls,
trait vtable types, and generic function instantiation.
"""
import unittest

from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.codegen import CodeGenerator


class TestCodegenExtended(unittest.TestCase):
    """Extended codegen tests for closures, enums, traits, impl blocks, and generics."""

    def _gen_ir(self, source: str) -> str:
        tokens = tokenize(source)
        module = parse(tokens)
        gen = CodeGenerator()
        return gen.generate(module)

    # -----------------------------------------------------------------------
    # Lambda / closure codegen
    # -----------------------------------------------------------------------

    def test_lambda_generates_function(self):
        """A lambda stored in a let binding should generate a __lambda_ function."""
        ir = self._gen_ir("""
let f = |x: i32| -> i32 { return x * 2 }
""")
        self.assertIn("__lambda_", ir)
        self.assertIn("define i32 @__lambda_0", ir)

    def test_lambda_body_has_multiply(self):
        """The generated lambda body should contain the multiplication op."""
        ir = self._gen_ir("""
let f = |x: i32| -> i32 { return x * 2 }
""")
        self.assertIn("mul i32", ir)

    def test_multiple_lambdas_get_unique_names(self):
        """Each lambda should get a unique mangled name."""
        ir = self._gen_ir("""
let f = |x: i32| -> i32 { return x + 1 }
let g = |y: i32| -> i32 { return y - 1 }
""")
        self.assertIn("@__lambda_0", ir)
        self.assertIn("@__lambda_1", ir)

    # -----------------------------------------------------------------------
    # Enum codegen
    # -----------------------------------------------------------------------

    def test_enum_generates_type_simple(self):
        """A simple enum (no payload) should generate a tagged type."""
        ir = self._gen_ir("""
enum Color {
    Red,
    Green,
    Blue
}
""")
        self.assertIn("%enum.Color", ir)
        self.assertIn("type", ir)

    def test_enum_generates_type_with_payload(self):
        """An enum with payload variants should have payload storage."""
        ir = self._gen_ir("""
enum Shape {
    Circle(f64),
    Rect(f64, f64)
}
""")
        self.assertIn("%enum.Shape", ir)
        # Should have payload bytes for the largest variant (Rect has 2 x f64 = 16 bytes)
        self.assertIn("x i8", ir)

    def test_enum_no_payload_is_tag_only(self):
        """An enum with only unit variants should be tag-only (just i8)."""
        ir = self._gen_ir("""
enum Direction {
    North,
    South,
    East,
    West
}
""")
        self.assertIn("%enum.Direction = type { i8 }", ir)

    # -----------------------------------------------------------------------
    # Trait codegen
    # -----------------------------------------------------------------------

    def test_trait_generates_vtable_type(self):
        """A trait declaration should generate a vtable struct type."""
        ir = self._gen_ir("""
trait Drawable {
    fn draw(self) -> void {
    }
}
""")
        self.assertIn("%vtable.Drawable", ir)

    # -----------------------------------------------------------------------
    # Impl block codegen
    # -----------------------------------------------------------------------

    def test_impl_generates_mangled_methods(self):
        """Methods in an impl block should be emitted with TypeName_method naming."""
        ir = self._gen_ir("""
struct Point {
    x: i32,
    y: i32
}

impl Point {
    fn get_x(self) -> i32 {
        return 0
    }
}
""")
        self.assertIn("Point_get_x", ir)

    def test_impl_method_is_callable_function(self):
        """An impl method should generate a proper function definition."""
        ir = self._gen_ir("""
struct Counter {
    value: i32
}

impl Counter {
    fn increment(self) -> i32 {
        return 1
    }
}
""")
        self.assertIn("define", ir)
        self.assertIn("Counter_increment", ir)

    def test_impl_multiple_methods(self):
        """Multiple methods in an impl block should all be generated."""
        ir = self._gen_ir("""
struct Vec2 {
    x: f64,
    y: f64
}

impl Vec2 {
    fn length(self) -> f64 {
        return 0.0
    }

    fn dot(self, other_x: f64, other_y: f64) -> f64 {
        return 0.0
    }
}
""")
        self.assertIn("Vec2_length", ir)
        self.assertIn("Vec2_dot", ir)

    # -----------------------------------------------------------------------
    # Existing features (regression tests)
    # -----------------------------------------------------------------------

    def test_if_else_codegen(self):
        """Verify existing if/else codegen still works."""
        ir = self._gen_ir("""
fn max(a: i32, b: i32) -> i32 {
    if a > b {
        return a
    } else {
        return b
    }
}
""")
        self.assertIn("define i32 @max", ir)
        self.assertIn("icmp sgt", ir)

    def test_for_range_codegen(self):
        """Verify for-range loop codegen still works."""
        ir = self._gen_ir("""
fn sum_to(n: i32) -> i32 {
    let total: i32 = 0
    for i in range(0, n) {
        total += i
    }
    return total
}
""")
        self.assertIn("define i32 @sum_to", ir)
        self.assertIn("for.cond", ir)
        self.assertIn("for.body", ir)

    def test_while_loop_codegen(self):
        """Verify while loop codegen still works."""
        ir = self._gen_ir("""
fn countdown(n: i32) -> i32 {
    let x: i32 = n
    while x > 0 {
        x -= 1
    }
    return x
}
""")
        self.assertIn("define i32 @countdown", ir)
        self.assertIn("while.cond", ir)

    def test_struct_and_field_access(self):
        """Verify struct creation and field access codegen still works."""
        ir = self._gen_ir("""
struct Point {
    x: i32,
    y: i32
}

fn get_x() -> i32 {
    let p = Point { x: 10, y: 20 }
    return p.x
}
""")
        self.assertIn("define i32 @get_x", ir)
        self.assertIn("%struct.Point", ir)
        self.assertIn("getelementptr", ir)

    def test_array_literal_codegen(self):
        """Verify array literal codegen still works."""
        ir = self._gen_ir("""
fn make_arr() -> i32 {
    let arr = [1, 2, 3]
    return arr[0]
}
""")
        self.assertIn("define i32 @make_arr", ir)
        self.assertIn("malloc", ir)

    def test_string_codegen(self):
        """Verify string literal and println codegen still works."""
        ir = self._gen_ir("""
fn greet() -> void {
    println("hello world")
}
""")
        self.assertIn("define void @greet", ir)
        self.assertIn("hello world", ir)

    def test_binary_ops(self):
        """Verify arithmetic and comparison operators still work."""
        ir = self._gen_ir("""
fn compute(a: i32, b: i32) -> i32 {
    return a + b * 2
}
""")
        self.assertIn("define i32 @compute", ir)
        self.assertIn("mul", ir)
        self.assertIn("add", ir)

    def test_power_operator(self):
        """Verify the ** power operator still works."""
        ir = self._gen_ir("""
fn square(x: f64) -> f64 {
    return x ** 2.0
}
""")
        self.assertIn("llvm.pow.f64", ir)

    # -----------------------------------------------------------------------
    # Codegen quality checks
    # -----------------------------------------------------------------------

    def test_module_header_present(self):
        """All generated IR should have the Kryos header and target triple."""
        ir = self._gen_ir("fn noop() -> void { }")
        self.assertIn("; Kryos LLVM IR", ir)
        self.assertIn("target triple", ir)

    def test_function_has_entry_block(self):
        """Every function should have an entry block."""
        ir = self._gen_ir("fn foo() -> i32 { return 42 }")
        self.assertIn("entry:", ir)

    def test_function_has_terminator(self):
        """Every function should end with a ret instruction."""
        ir = self._gen_ir("fn bar() -> void { }")
        self.assertIn("ret void", ir)


if __name__ == "__main__":
    unittest.main()
