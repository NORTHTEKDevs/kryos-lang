import unittest


class TestCapabilityAttenuation(unittest.TestCase):
    def test_rejects_escalation(self):
        from kryos.compiler.capabilities import CapabilityAnalyzer
        analyzer = CapabilityAnalyzer()
        result = analyzer.check_attenuation(
            parent_caps=["network"],
            child_caps=["network", "filesystem"]
        )
        self.assertFalse(result.valid)
        self.assertIn("filesystem", result.rejected)

    def test_allows_narrowing(self):
        from kryos.compiler.capabilities import CapabilityAnalyzer
        analyzer = CapabilityAnalyzer()
        result = analyzer.check_attenuation(
            parent_caps=["network", "filesystem"],
            child_caps=["network"]
        )
        self.assertTrue(result.valid)
        self.assertEqual(len(result.rejected), 0)

    def test_allows_same_caps(self):
        from kryos.compiler.capabilities import CapabilityAnalyzer
        analyzer = CapabilityAnalyzer()
        result = analyzer.check_attenuation(
            parent_caps=["network", "filesystem"],
            child_caps=["network", "filesystem"]
        )
        self.assertTrue(result.valid)

    def test_rejects_empty_parent_any_child(self):
        from kryos.compiler.capabilities import CapabilityAnalyzer
        analyzer = CapabilityAnalyzer()
        result = analyzer.check_attenuation(
            parent_caps=[],
            child_caps=["network"]
        )
        self.assertFalse(result.valid)


class TestSelfHealRestrictions(unittest.TestCase):
    def test_prohibited_actions_denied(self):
        from kryos.compiler.self_heal import SelfHealer, PROHIBITED_HEAL_ACTIONS
        healer = SelfHealer()
        for action in PROHIBITED_HEAL_ACTIONS:
            self.assertFalse(healer.can_heal(action), f"{action} should be prohibited")

    def test_safe_actions_allowed(self):
        from kryos.compiler.self_heal import SelfHealer
        healer = SelfHealer()
        for action in ["retry", "coerce", "clamp", "fallback", "substitute", "skip"]:
            self.assertTrue(healer.can_heal(action), f"{action} should be allowed")

    def test_self_heal_cannot_escalate(self):
        from kryos.compiler.self_heal import SelfHealer
        healer = SelfHealer()
        result = healer.can_heal("add_capability", context={"current_caps": ["network"]})
        self.assertFalse(result)

    def test_self_heal_allows_retry(self):
        from kryos.compiler.self_heal import SelfHealer
        healer = SelfHealer()
        result = healer.can_heal("retry", context={})
        self.assertTrue(result)


class TestCapabilityImmutability(unittest.TestCase):
    def test_immutability_constant_exists(self):
        from kryos.compiler.capabilities import CAPABILITY_IMMUTABLE_AT_RUNTIME
        self.assertTrue(CAPABILITY_IMMUTABLE_AT_RUNTIME)

    def test_prohibited_modifications_defined(self):
        from kryos.compiler.capabilities import PROHIBITED_RUNTIME_MODIFICATIONS
        self.assertIn("add_capability", PROHIBITED_RUNTIME_MODIFICATIONS)
        self.assertIn("escalate_tier", PROHIBITED_RUNTIME_MODIFICATIONS)
        self.assertIn("widen_sandbox", PROHIBITED_RUNTIME_MODIFICATIONS)
        self.assertIn("increase_budget", PROHIBITED_RUNTIME_MODIFICATIONS)


class TestBudgetSandboxAnnotations(unittest.TestCase):
    def test_budget_annotation_parses(self):
        from kryos.compiler.lexer import tokenize
        from kryos.compiler.parser import parse
        from kryos.compiler.ast_nodes import FnDecl
        source = '@budget(max_spawn: 5)\nfn worker() {\n    println("ok")\n}'
        mod = parse(tokenize(source))
        decl = mod.declarations[0]
        self.assertIsInstance(decl, FnDecl)
        self.assertIn("budget", [a.name for a in decl.attributes])

    def test_sandbox_annotation_parses(self):
        from kryos.compiler.lexer import tokenize
        from kryos.compiler.parser import parse
        from kryos.compiler.ast_nodes import FnDecl
        source = '@sandbox("restricted")\nfn sandboxed() {\n    println("safe")\n}'
        mod = parse(tokenize(source))
        decl = mod.declarations[0]
        self.assertIsInstance(decl, FnDecl)
        self.assertIn("sandbox", [a.name for a in decl.attributes])


if __name__ == "__main__":
    unittest.main()
