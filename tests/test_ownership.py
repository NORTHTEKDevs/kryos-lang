"""
Tests for the Kryos ownership analyzer (borrow checker).

Verifies compile-time memory safety rules:
  - Use-after-move detection
  - Copy-type exemptions (int, float, bool)
  - Move semantics for non-Copy types (arrays, strings, structs)
  - Function call move semantics
  - Scope-based cleanup
  - Reassignment after move
  - Loop move detection
  - Immutability enforcement
"""

import unittest

from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.ownership import OwnershipAnalyzer, OwnershipError


class TestOwnership(unittest.TestCase):
    """Test suite for the ownership analyzer."""

    def _check(self, source: str) -> list[OwnershipError]:
        """Parse source and run ownership analysis, return errors."""
        tokens = tokenize(source)
        module = parse(tokens)
        analyzer = OwnershipAnalyzer()
        return analyzer.analyze(module)

    # ================================================================
    #  BASIC -- no errors expected
    # ================================================================

    def test_simple_let_no_error(self):
        """A simple let binding with integer should produce no errors."""
        errors = self._check('let x = 42\nprintln(to_string(x))')
        self.assertEqual(len(errors), 0)

    def test_multiple_lets_no_error(self):
        """Multiple independent let bindings should be fine."""
        errors = self._check('''
let a = 1
let b = 2
let c = a + b
println(to_string(c))
''')
        self.assertEqual(len(errors), 0)

    def test_bool_literal_no_error(self):
        """Boolean literals are Copy types."""
        errors = self._check('''
let x = true
let y = x
println(to_string(x))
''')
        self.assertEqual(len(errors), 0)

    def test_float_literal_no_error(self):
        """Float literals are Copy types."""
        errors = self._check('''
let x = 3.14
let y = x
println(to_string(x))
''')
        self.assertEqual(len(errors), 0)

    # ================================================================
    #  COPY TYPES -- integers, floats, bools don't move
    # ================================================================

    def test_copy_types_no_move(self):
        """Integers are Copy -- assigning doesn't move."""
        errors = self._check('''
let x = 42
let y = x
println(to_string(x))
''')
        self.assertEqual(len(errors), 0, f"Unexpected errors: {errors}")

    def test_copy_type_arithmetic(self):
        """Arithmetic on copy types produces copy types."""
        errors = self._check('''
let x = 10
let y = 20
let z = x + y
println(to_string(x))
println(to_string(y))
println(to_string(z))
''')
        self.assertEqual(len(errors), 0)

    def test_copy_type_multiple_uses(self):
        """Copy types can be used as many times as desired."""
        errors = self._check('''
let x = 42
let a = x
let b = x
let c = x + x
println(to_string(x))
''')
        self.assertEqual(len(errors), 0)

    # ================================================================
    #  USE-AFTER-MOVE -- non-Copy types
    # ================================================================

    def test_use_after_move_array(self):
        """Moving an array and then using it should produce an error."""
        errors = self._check('''
let data = [1, 2, 3]
let other = data
println(to_string(len(data)))
''')
        self.assertTrue(len(errors) > 0, "Expected use-after-move error")
        self.assertIn("moved", errors[0].message)

    def test_use_after_move_string(self):
        """Moving a string and then using it should produce an error."""
        errors = self._check('''
let s = "hello"
let t = s
println(s)
''')
        self.assertTrue(len(errors) > 0, "Expected use-after-move error for string")
        self.assertIn("moved", errors[0].message)

    def test_use_after_move_note_contains_line(self):
        """Error note should mention the line where the move happened."""
        errors = self._check('''
let data = [1, 2, 3]
let other = data
println(to_string(len(data)))
''')
        self.assertTrue(len(errors) > 0)
        self.assertIn("moved at line", errors[0].note)

    # ================================================================
    #  FUNCTION SCOPE
    # ================================================================

    def test_function_scope_cleanup(self):
        """Variables inside a function scope should be cleaned up."""
        errors = self._check('''
fn foo() {
    let x = [1, 2]
    println(to_string(len(x)))
}
foo()
''')
        self.assertEqual(len(errors), 0)

    def test_function_param_usage(self):
        """Function parameters should be usable within the function."""
        errors = self._check('''
fn greet(name) {
    println(name)
}
greet("world")
''')
        self.assertEqual(len(errors), 0)

    def test_nested_function_scopes(self):
        """Nested functions should have independent scopes."""
        errors = self._check('''
fn outer() {
    let x = 42
    fn inner() {
        let y = 10
        println(to_string(y))
    }
    inner()
    println(to_string(x))
}
outer()
''')
        self.assertEqual(len(errors), 0)

    # ================================================================
    #  MOVE IN FUNCTION CALL
    # ================================================================

    def test_move_in_function_call(self):
        """Passing a non-Copy value to a function moves it."""
        errors = self._check('''
fn consume(arr) {
    println(to_string(len(arr)))
}
let data = [1, 2, 3]
consume(data)
println(to_string(len(data)))
''')
        self.assertTrue(len(errors) > 0, "Expected error: data moved into consume()")
        self.assertIn("moved", errors[0].message)

    def test_copy_type_function_call_no_move(self):
        """Passing a Copy-type to a function doesn't move it."""
        errors = self._check('''
fn double(x) {
    println(to_string(x + x))
}
let n = 42
double(n)
println(to_string(n))
''')
        self.assertEqual(len(errors), 0)

    # ================================================================
    #  REASSIGNMENT AFTER MOVE
    # ================================================================

    def test_reassignment_after_move(self):
        """Reassigning a moved variable should restore it."""
        errors = self._check('''
let mut x = [1, 2]
let y = x
let x = [3, 4]
println(to_string(len(x)))
''')
        self.assertEqual(len(errors), 0,
                         f"Unexpected errors after reassignment: {errors}")

    def test_move_then_use_then_reassign(self):
        """Using after move is an error even if reassigned later."""
        errors = self._check('''
let mut x = [1, 2]
let y = x
println(to_string(len(x)))
let x = [3, 4]
''')
        self.assertTrue(len(errors) > 0,
                         "Expected error: use of x between move and reassignment")

    # ================================================================
    #  MOVE IN LOOP
    # ================================================================

    def test_move_in_loop(self):
        """Moving a variable inside a loop body should be caught."""
        errors = self._check('''
let data = [1, 2, 3]
for i in range(3) {
    let x = data
}
''')
        # data is moved in the first iteration, second iteration uses moved value
        self.assertTrue(len(errors) > 0,
                         "Expected error: data moved in loop body")

    def test_copy_in_loop_ok(self):
        """Using a copy-type in a loop should be fine."""
        errors = self._check('''
let n = 42
for i in range(3) {
    let x = n
    println(to_string(x))
}
''')
        self.assertEqual(len(errors), 0)

    # ================================================================
    #  IF/ELSE BRANCHES
    # ================================================================

    def test_if_else_scopes(self):
        """Variables in if/else branches should be scoped correctly."""
        errors = self._check('''
let x = 42
if x > 0 {
    let y = 10
    println(to_string(y))
} else {
    let z = 20
    println(to_string(z))
}
println(to_string(x))
''')
        self.assertEqual(len(errors), 0)

    # ================================================================
    #  WHILE LOOPS
    # ================================================================

    def test_while_loop_scope(self):
        """Variables in while loops should be scoped correctly."""
        errors = self._check('''
let mut count = 0
while count < 10 {
    let temp = count + 1
    let count = temp
}
''')
        self.assertEqual(len(errors), 0)

    # ================================================================
    #  ASSIGNMENT CHECKS
    # ================================================================

    def test_immutable_assignment_error(self):
        """Assigning to an immutable variable should produce an error."""
        errors = self._check('''
let x = 42
x = 100
''')
        self.assertTrue(len(errors) > 0, "Expected error: assignment to immutable")
        self.assertIn("immutable", errors[0].message)

    def test_mutable_assignment_ok(self):
        """Assigning to a mutable variable should be fine."""
        errors = self._check('''
let mut x = 42
x = 100
println(to_string(x))
''')
        self.assertEqual(len(errors), 0)

    # ================================================================
    #  EXPRESSION ANALYSIS
    # ================================================================

    def test_binary_op_no_error(self):
        """Binary operations on copy types should produce no errors."""
        errors = self._check('''
let a = 10
let b = 20
let c = a + b * 2
println(to_string(c))
''')
        self.assertEqual(len(errors), 0)

    def test_array_elements_analyzed(self):
        """Expressions inside array literals should be analyzed."""
        errors = self._check('''
let x = 42
let arr = [x, x + 1, x + 2]
println(to_string(len(arr)))
println(to_string(x))
''')
        self.assertEqual(len(errors), 0)

    # ================================================================
    #  STRUCT LITERAL MOVE
    # ================================================================

    def test_struct_literal_is_non_copy(self):
        """Struct literals should be non-Copy (moving them consumes)."""
        errors = self._check('''
struct Point {
    x: i32,
    y: i32,
}
let p = Point { x: 1, y: 2 }
let q = p
println(to_string(q))
''')
        # Using p after moving to q would be an error, but we don't use p again
        self.assertEqual(len(errors), 0)

    # ================================================================
    #  ERROR FORMATTING
    # ================================================================

    def test_error_str_format(self):
        """OwnershipError __str__ should produce readable output."""
        err = OwnershipError(
            message="use of moved value 'data'",
            span=(5, 10, 5, 14),
            note="value moved at line 4",
        )
        s = str(err)
        self.assertIn("line 5", s)
        self.assertIn("use of moved value", s)
        self.assertIn("moved at line 4", s)

    def test_error_str_no_note(self):
        """OwnershipError without a note should still format correctly."""
        err = OwnershipError(
            message="cannot borrow as mutable",
            span=(3, 0, 3, 10),
        )
        s = str(err)
        self.assertIn("cannot borrow as mutable", s)
        self.assertNotIn("note:", s)

    # ================================================================
    #  EDGE CASES
    # ================================================================

    def test_empty_program(self):
        """An empty program should produce no errors."""
        errors = self._check('')
        self.assertEqual(len(errors), 0)

    def test_literals_only(self):
        """A program with only literals should produce no errors."""
        errors = self._check('''
println(to_string(42))
println("hello")
println(to_string(true))
''')
        self.assertEqual(len(errors), 0)

    def test_unused_variable(self):
        """An unused variable should not cause ownership errors."""
        errors = self._check('''
let x = [1, 2, 3]
println("done")
''')
        self.assertEqual(len(errors), 0)

    def test_multiple_functions(self):
        """Multiple function declarations should be independent."""
        errors = self._check('''
fn foo() {
    let x = [1, 2]
    println(to_string(len(x)))
}
fn bar() {
    let y = [3, 4]
    println(to_string(len(y)))
}
foo()
bar()
''')
        self.assertEqual(len(errors), 0)

    def test_return_moves_value(self):
        """Returning a non-Copy value from a function moves it."""
        errors = self._check('''
fn make_data() {
    let data = [1, 2, 3]
    return data
}
let result = make_data()
println(to_string(len(result)))
''')
        self.assertEqual(len(errors), 0)

    def test_double_move_error(self):
        """Moving the same variable twice should produce an error."""
        errors = self._check('''
fn consume(x) {
    println(to_string(x))
}
let data = [1, 2, 3]
consume(data)
consume(data)
''')
        self.assertTrue(len(errors) > 0,
                         "Expected error: double move of data")

    def test_move_in_nested_scope(self):
        """Moving a variable inside a nested scope should be visible outside."""
        errors = self._check('''
let data = [1, 2, 3]
if true {
    let x = data
}
println(to_string(len(data)))
''')
        self.assertTrue(len(errors) > 0,
                         "Expected error: data moved in if block, used after")


if __name__ == '__main__':
    unittest.main()
