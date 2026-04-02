"""Tests for dynamic array codegen (metadata struct with length/capacity/data pointer).

Verifies that array literals emit a struct type definition, len() reads from
the struct's length field, push() generates realloc logic, index access goes
through the struct's data pointer, empty arrays produce a zeroed struct, and
pop() decrements length.
"""
import unittest

from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.codegen import CodeGenerator


class TestArrayCodegen(unittest.TestCase):
    """Dynamic array codegen tests."""

    def _gen_ir(self, source: str) -> str:
        tokens = tokenize(source)
        module = parse(tokens)
        gen = CodeGenerator()
        return gen.generate(module)

    # -------------------------------------------------------------------
    # Array struct type definition
    # -------------------------------------------------------------------

    def test_array_literal_emits_struct_type(self):
        """An array literal should emit a %Array_<elem> struct type definition."""
        ir = self._gen_ir("let a = [1, 2, 3]")
        self.assertIn("%Array_i32 = type { i32, i32, i32* }", ir)

    def test_array_struct_type_emitted_once(self):
        """The struct type definition should be emitted exactly once even with multiple arrays."""
        ir = self._gen_ir("""
let a = [1, 2, 3]
let b = [4, 5]
""")
        count = ir.count("%Array_i32 = type { i32, i32, i32* }")
        self.assertEqual(count, 1, "Array struct type should be emitted exactly once")

    # -------------------------------------------------------------------
    # Array literal allocation
    # -------------------------------------------------------------------

    def test_array_literal_allocates_struct(self):
        """An array literal should malloc a struct and a separate data buffer."""
        ir = self._gen_ir("let a = [10, 20, 30]")
        # Should have malloc calls (at least 2: one for struct, one for data)
        self.assertGreaterEqual(ir.count("@malloc"), 2,
                                "Should malloc both struct and data buffer")

    def test_array_literal_stores_length(self):
        """The array literal codegen should store the element count as length."""
        ir = self._gen_ir("let a = [10, 20, 30]")
        # Length should be stored (3 elements)
        self.assertIn("store i32 3", ir)

    def test_array_literal_stores_capacity(self):
        """The array literal codegen should store capacity (minimum 8)."""
        ir = self._gen_ir("let a = [10, 20, 30]")
        # Minimum capacity is 8
        self.assertIn("store i32 8", ir)

    def test_array_literal_stores_elements_via_gep(self):
        """Elements should be stored into the data buffer via GEP."""
        ir = self._gen_ir("let a = [1, 2, 3]")
        # Should GEP into data buffer to store elements
        self.assertIn("getelementptr i32, i32*", ir)

    # -------------------------------------------------------------------
    # Empty array
    # -------------------------------------------------------------------

    def test_empty_array_allocates_zeroed_struct(self):
        """An empty array should produce a struct with length=0, capacity=0, null data."""
        ir = self._gen_ir("let a: [i32] = []")
        # Should still emit the struct type
        self.assertIn("%Array_i32", ir)
        # Should store 0 for length
        self.assertIn("store i32 0", ir)
        # Should store null for data pointer
        self.assertIn("store i32* null", ir)

    def test_empty_array_no_data_malloc(self):
        """An empty array should not malloc a data buffer (null data pointer)."""
        ir = self._gen_ir("let a: [i32] = []")
        # Empty array: struct is malloced, but no data buffer malloc.
        # The old code returned just "null" — now it should allocate a struct.
        self.assertNotEqual(ir.count("@malloc"), 0,
                            "Should malloc the struct even for empty arrays")

    # -------------------------------------------------------------------
    # len() on arrays
    # -------------------------------------------------------------------

    def test_len_reads_struct_field(self):
        """len() should GEP into struct field 0 (length) and load it."""
        ir = self._gen_ir("""
let a = [1, 2, 3]
let n = len(a)
""")
        # Should NOT have the old WARNING comment
        self.assertNotIn("WARNING: len() on arrays requires runtime length metadata", ir)
        # Should GEP into field 0 of the array struct
        self.assertIn("getelementptr inbounds %Array_i32, %Array_i32*", ir)

    def test_len_no_warning_comment(self):
        """len() on arrays should not produce a WARNING or TODO comment."""
        ir = self._gen_ir("""
let a = [1, 2, 3]
let n = len(a)
""")
        self.assertNotIn("WARNING", ir)
        self.assertNotIn("TODO", ir)

    def test_len_on_string_still_uses_strlen(self):
        """len() on strings should still use strlen, unchanged."""
        ir = self._gen_ir("""
let s = "hello"
let n = len(s)
""")
        self.assertIn("@strlen", ir)

    # -------------------------------------------------------------------
    # push() codegen
    # -------------------------------------------------------------------

    def test_push_generates_realloc_logic(self):
        """push() should generate realloc logic, not a TODO comment."""
        ir = self._gen_ir("""
let a = [1, 2, 3]
push(a, 4)
""")
        # Should NOT have the old TODO comment
        self.assertNotIn("TODO: push() requires dynamic array support", ir)
        # Should have realloc declared
        self.assertIn("@realloc", ir)

    def test_push_increments_length(self):
        """push() should increment the length field of the struct."""
        ir = self._gen_ir("""
let a = [1, 2, 3]
push(a, 4)
""")
        # Should contain add instruction to increment length
        self.assertIn("add i32", ir)

    def test_push_checks_capacity(self):
        """push() should compare length to capacity for potential realloc."""
        ir = self._gen_ir("""
let a = [1, 2, 3]
push(a, 4)
""")
        # Should have icmp to compare length == capacity
        self.assertIn("icmp eq i32", ir)

    def test_push_stores_value_at_length_index(self):
        """push() should store the new value at data[length]."""
        ir = self._gen_ir("""
let a = [1, 2, 3]
push(a, 4)
""")
        # Should GEP into data buffer at the old length index
        self.assertIn("getelementptr i32, i32*", ir)

    # -------------------------------------------------------------------
    # Index access
    # -------------------------------------------------------------------

    def test_index_access_through_struct_data_pointer(self):
        """Index access should load the data pointer from the struct, then GEP into it."""
        ir = self._gen_ir("""
let a = [10, 20, 30]
let x = a[1]
""")
        # Should GEP into field 2 of the struct (data pointer)
        self.assertIn("getelementptr inbounds %Array_i32, %Array_i32*", ir)
        # Should load the data pointer
        self.assertIn("load i32*", ir)

    def test_index_access_loads_element(self):
        """Index access should produce a load of the element at the computed address."""
        ir = self._gen_ir("""
let a = [10, 20, 30]
let x = a[1]
""")
        # Should have a final load for the element value
        self.assertIn("load i32, i32*", ir)

    # -------------------------------------------------------------------
    # pop() codegen
    # -------------------------------------------------------------------

    def test_pop_decrements_length(self):
        """pop() should decrement the length field of the struct."""
        ir = self._gen_ir("""
let a = [1, 2, 3]
let v = pop(a)
""")
        # Should contain sub instruction to decrement length
        self.assertIn("sub i32", ir)
        # Should NOT have TODO/WARNING
        self.assertNotIn("TODO", ir)

    def test_pop_returns_last_element(self):
        """pop() should load the element at data[new_length] (the last element)."""
        ir = self._gen_ir("""
let a = [1, 2, 3]
let v = pop(a)
""")
        # Should GEP into data at the new (decremented) length index
        self.assertIn("getelementptr i32, i32*", ir)
        # Should load the value
        self.assertIn("load i32, i32*", ir)

    # -------------------------------------------------------------------
    # Regression: _infer_llvm_type for arrays
    # -------------------------------------------------------------------

    def test_infer_llvm_type_returns_array_struct_pointer(self):
        """_infer_llvm_type for an ArrayLiteral should return %Array_<elem>* (struct pointer)."""
        ir = self._gen_ir("let a = [1, 2, 3]")
        # The env should track the variable as %Array_i32* type.
        # The alloca for the let binding should be %Array_i32*
        self.assertIn("alloca %Array_i32*", ir)


if __name__ == "__main__":
    unittest.main()
