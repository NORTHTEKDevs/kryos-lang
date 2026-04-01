"""Tests for Kryos standard library - net module."""
import os
import unittest

from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.interpreter import Interpreter


class TestStdlibNet(unittest.TestCase):
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

    # ---- URL utilities (pure, no network) ----

    def test_url_encode(self):
        result = self._run('println(url_encode("hello world&foo=bar"))')
        self.assertIn("%20", result)
        self.assertIn("%26", result)

    def test_url_decode(self):
        result = self._run('println(url_decode("hello%20world"))')
        self.assertEqual(result, "hello world")

    def test_url_roundtrip(self):
        result = self._run(
            'let original = "hello world!"\n'
            'let encoded = url_encode(original)\n'
            'let decoded = url_decode(encoded)\n'
            'println(to_string(decoded == original))'
        )
        self.assertEqual(result, "true")

    # ---- TCP error handling ----

    def test_tcp_send_invalid_handle(self):
        with self.assertRaises(Exception):
            self._run('tcp_send(99999, "data")')

    # ---- Network tests (require KRYOS_NET_TESTS env var) ----

    @unittest.skipUnless(
        os.environ.get("KRYOS_NET_TESTS"),
        "Network tests disabled (set KRYOS_NET_TESTS=1 to enable)",
    )
    def test_http_get(self):
        result = self._run('println(http_get("https://httpbin.org/get"))')
        self.assertTrue(len(result) > 0)

    @unittest.skipUnless(
        os.environ.get("KRYOS_NET_TESTS"),
        "Network tests disabled (set KRYOS_NET_TESTS=1 to enable)",
    )
    def test_http_post(self):
        result = self._run(
            'println(http_post("https://httpbin.org/post", "test=1", '
            '"application/x-www-form-urlencoded"))'
        )
        self.assertTrue(len(result) > 0)

    @unittest.skipUnless(
        os.environ.get("KRYOS_NET_TESTS"),
        "Network tests disabled (set KRYOS_NET_TESTS=1 to enable)",
    )
    def test_http_get_json(self):
        result = self._run(
            'let resp = http_get_json("https://httpbin.org/get")\n'
            'println(to_string(json_has(resp, "url")))'
        )
        self.assertEqual(result, "true")

    @unittest.skipUnless(
        os.environ.get("KRYOS_NET_TESTS"),
        "Network tests disabled (set KRYOS_NET_TESTS=1 to enable)",
    )
    def test_http_request_get(self):
        result = self._run(
            'let resp = http_request("GET", "https://httpbin.org/get")\n'
            'println(to_string(json_get(resp, "status")))'
        )
        self.assertEqual(result, "200")


if __name__ == "__main__":
    unittest.main()
