"""Tests for Kryos standard library crypto module."""
import unittest
import contextlib
import io as _io
from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.interpreter import Interpreter


class TestCryptoModule(unittest.TestCase):
    def _run(self, source: str) -> str:
        tokens = tokenize(source)
        module = parse(tokens)
        f = _io.StringIO()
        with contextlib.redirect_stdout(f):
            interp = Interpreter()
            interp.run(module)
        return f.getvalue().strip()

    # ---- Hashing ----

    def test_sha256(self):
        result = self._run('println(sha256("hello"))')
        self.assertEqual(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        )

    def test_sha512(self):
        result = self._run('println(sha512("hello"))')
        self.assertTrue(result.startswith("9b71d224bd62f378"))

    def test_md5(self):
        result = self._run('println(md5("hello"))')
        self.assertEqual(result, "5d41402abc4b2a76b9719d911017c592")

    # ---- HMAC ----

    def test_hmac_sha256(self):
        result = self._run('println(hmac_sha256("key", "hello"))')
        self.assertEqual(len(result), 64)

    # ---- Base64 ----

    def test_base64_encode(self):
        result = self._run('println(base64_encode("hello world"))')
        self.assertEqual(result, "aGVsbG8gd29ybGQ=")

    def test_base64_decode(self):
        result = self._run('println(base64_decode("aGVsbG8gd29ybGQ="))')
        self.assertEqual(result, "hello world")

    def test_base64_roundtrip(self):
        result = self._run(
            'let encoded = base64_encode("kryos lang")\n'
            'println(base64_decode(encoded))'
        )
        self.assertEqual(result, "kryos lang")

    # ---- Hex ----

    def test_hex_encode(self):
        result = self._run('println(hex_encode("AB"))')
        self.assertEqual(result, "4142")

    def test_hex_decode(self):
        result = self._run('println(hex_decode("4142"))')
        self.assertEqual(result, "AB")

    # ---- Random bytes ----

    def test_random_bytes(self):
        result = self._run('println(random_bytes(16))')
        self.assertEqual(len(result), 32)

    # ---- UUID ----

    def test_uuid(self):
        result = self._run('println(uuid())')
        self.assertEqual(len(result), 36)
        parts = result.split("-")
        self.assertEqual(len(parts), 5)


if __name__ == "__main__":
    unittest.main()
