import unittest
from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.ast_nodes import SelectStmt, LetStmt, ExprStmt, FnDecl
from kryos.compiler.interpreter import Interpreter


class TestActorParsing(unittest.TestCase):
    def _parse(self, source):
        return parse(tokenize(source))

    def test_chan_expression(self):
        mod = self._parse("let ch = chan()")
        self.assertIsInstance(mod.declarations[0], LetStmt)

    def test_send_statement(self):
        mod = self._parse("send(ch, 42)")
        self.assertIsInstance(mod.declarations[0], ExprStmt)

    def test_recv_expression(self):
        mod = self._parse("let msg = recv(ch)")
        self.assertIsInstance(mod.declarations[0], LetStmt)

    def test_select_statement(self):
        mod = self._parse("""
select {
    ch1 => { println("got ch1") }
    ch2 => { println("got ch2") }
}
""")
        self.assertIsInstance(mod.declarations[0], SelectStmt)
        self.assertEqual(len(mod.declarations[0].branches), 2)

    def test_ask_expression(self):
        mod = self._parse("let result = ask(ch, request)")
        self.assertIsInstance(mod.declarations[0], LetStmt)

    def test_actor_annotation(self):
        source = '@actor\nfn worker(inbox) {\n    println("working")\n}'
        mod = self._parse(source)
        decl = mod.declarations[0]
        self.assertIsInstance(decl, FnDecl)
        self.assertIn("actor", [a.name for a in decl.attributes])


class TestActorRuntime(unittest.TestCase):
    def _run(self, source, timeout=2.0):
        interp = Interpreter()
        interp.run(parse(tokenize(source)))
        import time
        deadline = time.time() + timeout
        for t in interp._spawned_threads:
            remaining = deadline - time.time()
            if remaining > 0:
                t.join(timeout=remaining)
        return interp.output

    def test_channel_send_recv(self):
        output = self._run("""
let ch = chan()
spawn { send(ch, 42) }
let val = recv(ch)
println(val)
""")
        self.assertIn("42", output)

    def test_multiple_messages(self):
        output = self._run("""
let ch = chan()
spawn {
    send(ch, 1)
    send(ch, 2)
    send(ch, 3)
}
println(recv(ch))
println(recv(ch))
println(recv(ch))
""")
        self.assertIn("1", output)
        self.assertIn("2", output)
        self.assertIn("3", output)


if __name__ == "__main__":
    unittest.main()
