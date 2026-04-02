"""Tests for GitHub-backed package registry functions."""

import unittest

from kryos.compiler.packages import parse_dependency_spec, resolve_semver, generate_lock


class TestParseDependencySpec(unittest.TestCase):
    """Tests for parse_dependency_spec."""

    def test_parse_github_spec(self):
        spec = parse_dependency_spec("github:user/pkg@^1.0.0")
        self.assertEqual(spec["source"], "github")
        self.assertEqual(spec["owner"], "user")
        self.assertEqual(spec["repo"], "pkg")
        self.assertEqual(spec["version"], "^1.0.0")

    def test_parse_github_tilde_spec(self):
        spec = parse_dependency_spec("github:org/my-lib@~2.1.0")
        self.assertEqual(spec["owner"], "org")
        self.assertEqual(spec["repo"], "my-lib")
        self.assertEqual(spec["version"], "~2.1.0")

    def test_parse_github_exact_spec(self):
        spec = parse_dependency_spec("github:acme/core@=3.0.0")
        self.assertEqual(spec["version"], "=3.0.0")

    def test_parse_invalid_spec_raises(self):
        with self.assertRaises(ValueError):
            parse_dependency_spec("invalid-spec")

    def test_parse_missing_version_raises(self):
        with self.assertRaises(ValueError):
            parse_dependency_spec("github:user/pkg")

    def test_parse_local_spec_raises(self):
        with self.assertRaises(ValueError):
            parse_dependency_spec("local:my-pkg@1.0.0")


class TestResolveSemver(unittest.TestCase):
    """Tests for resolve_semver (^, ~, = range matching)."""

    # Caret ranges (^) -- same major, >= minor.patch
    def test_caret_exact_match(self):
        self.assertTrue(resolve_semver("^1.2.0", "1.2.0"))

    def test_caret_higher_minor(self):
        self.assertTrue(resolve_semver("^1.2.0", "1.3.5"))

    def test_caret_higher_patch(self):
        self.assertTrue(resolve_semver("^1.2.0", "1.2.9"))

    def test_caret_rejects_different_major(self):
        self.assertFalse(resolve_semver("^1.2.0", "2.0.0"))

    def test_caret_rejects_lower_minor(self):
        self.assertFalse(resolve_semver("^1.2.0", "1.1.9"))

    def test_caret_rejects_lower_patch_same_minor(self):
        self.assertFalse(resolve_semver("^1.2.5", "1.2.4"))

    # Tilde ranges (~) -- same major.minor, >= patch
    def test_tilde_exact_match(self):
        self.assertTrue(resolve_semver("~1.2.0", "1.2.0"))

    def test_tilde_higher_patch(self):
        self.assertTrue(resolve_semver("~1.2.0", "1.2.5"))

    def test_tilde_rejects_different_minor(self):
        self.assertFalse(resolve_semver("~1.2.0", "1.3.0"))

    def test_tilde_rejects_different_major(self):
        self.assertFalse(resolve_semver("~1.2.0", "2.2.0"))

    # Exact match (=)
    def test_exact_match(self):
        self.assertTrue(resolve_semver("=1.2.3", "1.2.3"))

    def test_exact_mismatch(self):
        self.assertFalse(resolve_semver("=1.2.3", "1.2.4"))

    # Edge cases
    def test_invalid_version_format(self):
        self.assertFalse(resolve_semver("^1.0.0", "1.0"))

    def test_non_numeric_version(self):
        self.assertFalse(resolve_semver("^1.0.0", "a.b.c"))

    def test_unrecognized_range_prefix(self):
        self.assertFalse(resolve_semver("!1.0.0", "1.0.0"))


class TestGenerateLock(unittest.TestCase):
    """Tests for generate_lock."""

    def test_basic_lock_generation(self):
        deps = {"http-utils": {"github": "user/http-utils", "version": "^1.0.0"}}
        lock = generate_lock(deps, resolved={"http-utils": "1.2.3"})
        self.assertIn("http-utils", lock)
        self.assertEqual(lock["http-utils"]["resolved"], "1.2.3")
        self.assertEqual(lock["http-utils"]["source"], "user/http-utils")

    def test_multiple_deps(self):
        deps = {
            "core": {"github": "org/core"},
            "utils": {"github": "org/utils"},
        }
        lock = generate_lock(deps, resolved={"core": "2.0.0", "utils": "1.1.0"})
        self.assertEqual(len(lock), 2)
        self.assertEqual(lock["core"]["resolved"], "2.0.0")
        self.assertEqual(lock["utils"]["resolved"], "1.1.0")

    def test_unresolved_dep(self):
        deps = {"missing": {"github": "user/missing"}}
        lock = generate_lock(deps, resolved={})
        self.assertEqual(lock["missing"]["resolved"], "unknown")

    def test_local_dep_fallback(self):
        deps = {"local-pkg": {"version": "1.0.0"}}
        lock = generate_lock(deps, resolved={"local-pkg": "1.0.0"})
        self.assertEqual(lock["local-pkg"]["source"], "local")


if __name__ == "__main__":
    unittest.main()
