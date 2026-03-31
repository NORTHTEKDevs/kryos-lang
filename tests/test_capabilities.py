"""
Kryos Capability Enforcement Test Suite

Verifies that the capability system is airtight:
- Functions without @capabilities have zero access beyond compute
- Capabilities can only be narrowed, never elevated
- Tier-restricted capabilities are blocked in community edition
- Sandbox escapes are caught and logged
- Audit trail is complete and correct
"""

import sys
import os

# Add project root to path
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from kryos.compiler.capabilities import (
    CapabilitySet, CapabilityAnalyzer, CapabilityEnforcer,
    CapabilityViolation, SandboxEscapeAttempt, CapabilityAuditLog,
    CapabilityTier, BUILTIN_CAPABILITIES,
)
from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse


def _analyze_source(source: str, tier=CapabilityTier.COMMUNITY):
    """Helper: tokenize, parse, and analyze source."""
    tokens = tokenize(source, "<test>")
    module = parse(tokens)
    analyzer = CapabilityAnalyzer(tier=tier)
    return analyzer.analyze(module)


passed = 0
failed = 0


def test(name: str, condition: bool, detail: str = ""):
    global passed, failed
    if condition:
        passed += 1
        print(f"  \033[32mPASS\033[0m  {name}")
    else:
        failed += 1
        print(f"  \033[31mFAIL\033[0m  {name}" + (f" — {detail}" if detail else ""))


print("\n" + "=" * 60)
print("Kryos Capability Enforcement Test Suite")
print("=" * 60)

# -------------------------------------------------------------------
# Test 1: Default — no capabilities means pure compute only
# -------------------------------------------------------------------
print("\n[1] Default capability enforcement")

report = _analyze_source("""
fn process(x: i32) -> i32 {
    return x * 2
}
""")
test(
    "function without @capabilities gets compute only",
    "process" in report["functions"] and
    report["functions"]["process"]["declared"] == ["compute"],
)

# -------------------------------------------------------------------
# Test 2: Capability narrowing — child cannot elevate
# -------------------------------------------------------------------
print("\n[2] Capability narrowing")

report = _analyze_source("""
@capabilities(compute)
fn parent() -> i32 {
    @capabilities(network)
    fn child() -> i32 {
        return 42
    }
    return child()
}
""")
test(
    "child cannot elevate beyond parent capabilities",
    len(report["errors"]) > 0 and "elevate" in report["errors"][0].lower() or "narrow" in report["errors"][0].lower(),
    f"errors: {report['errors']}",
)

# -------------------------------------------------------------------
# Test 3: Valid narrowing — child subset of parent
# -------------------------------------------------------------------
print("\n[3] Valid capability narrowing")

report = _analyze_source("""
@capabilities(network, filesystem)
fn parent() -> i32 {
    @capabilities(network)
    fn child() -> i32 {
        return 42
    }
    return child()
}
""")
test(
    "child with subset of parent capabilities is valid",
    len(report["errors"]) == 0,
    f"unexpected errors: {report['errors']}",
)

# -------------------------------------------------------------------
# Test 4: Tier restriction — quantum requires Enterprise
# -------------------------------------------------------------------
print("\n[4] Tier restrictions")

report = _analyze_source("""
@capabilities(quantum)
fn quantum_algo() -> i32 {
    return 42
}
""", tier=CapabilityTier.COMMUNITY)
test(
    "quantum capability blocked in community tier",
    len(report["errors"]) > 0 and "enterprise" in report["errors"][0].lower(),
    f"errors: {report['errors']}",
)

# Enterprise tier allows quantum
report = _analyze_source("""
@capabilities(quantum)
fn quantum_algo() -> i32 {
    return 42
}
""", tier=CapabilityTier.ENTERPRISE)
test(
    "quantum capability allowed in enterprise tier",
    len(report["errors"]) == 0,
    f"unexpected errors: {report['errors']}",
)

# -------------------------------------------------------------------
# Test 5: Raw socket requires Pro tier
# -------------------------------------------------------------------
print("\n[5] Pro tier capabilities")

report = _analyze_source("""
@capabilities(network)
fn scanner() -> i32 {
    return 0
}
""", tier=CapabilityTier.COMMUNITY)
test(
    "basic network allowed in community",
    len(report["errors"]) == 0,
)

# -------------------------------------------------------------------
# Test 6: Runtime enforcer — capability violation
# -------------------------------------------------------------------
print("\n[6] Runtime enforcement")

enforcer = CapabilityEnforcer(tier=CapabilityTier.COMMUNITY)
enforcer.enter_function("pure_fn", CapabilitySet.none())

violation_caught = False
try:
    enforcer.require_capability("network", "attempted network access")
except CapabilityViolation:
    violation_caught = True

test(
    "runtime enforcer blocks unauthorized capability",
    violation_caught,
)
enforcer.exit_function()

# -------------------------------------------------------------------
# Test 7: Runtime enforcer — allowed capability
# -------------------------------------------------------------------
print("\n[7] Runtime allowed capability")

enforcer2 = CapabilityEnforcer()
enforcer2.enter_function("net_fn", CapabilitySet.from_list(["network"]))

allowed = enforcer2.check_capability("network")
test("runtime enforcer allows declared capability", allowed)

blocked = enforcer2.check_capability("filesystem")
test("runtime enforcer blocks undeclared capability", not blocked)
enforcer2.exit_function()

# -------------------------------------------------------------------
# Test 8: Sandbox creation — zero capabilities by default
# -------------------------------------------------------------------
print("\n[8] Sandbox isolation")

main_enforcer = CapabilityEnforcer()
main_enforcer.enter_function("main", CapabilitySet.from_list(["network", "filesystem"]))

sandbox = main_enforcer.create_sandbox()  # no caps granted
test(
    "sandbox starts with zero capabilities",
    sandbox.current_capabilities.capabilities == frozenset({"compute"}),
)

# -------------------------------------------------------------------
# Test 9: Sandbox cannot exceed parent
# -------------------------------------------------------------------
print("\n[9] Sandbox cannot exceed parent")

escape_caught = False
try:
    bad_sandbox = main_enforcer.create_sandbox(
        CapabilitySet.from_list(["quantum"])  # parent doesn't have quantum
    )
except SandboxEscapeAttempt:
    escape_caught = True

test("sandbox escape attempt caught", escape_caught)
main_enforcer.exit_function()

# -------------------------------------------------------------------
# Test 10: Sandbox with granted subset
# -------------------------------------------------------------------
print("\n[10] Sandbox with granted subset")

main_enforcer2 = CapabilityEnforcer()
main_enforcer2.enter_function("main2", CapabilitySet.from_list(["network", "filesystem"]))

good_sandbox = main_enforcer2.create_sandbox(
    CapabilitySet.from_list(["network"])  # valid subset
)
test(
    "sandbox with valid subset is allowed",
    good_sandbox.check_capability("network"),
)
test(
    "sandbox does not inherit non-granted capabilities",
    not good_sandbox.check_capability("filesystem"),
)
main_enforcer2.exit_function()

# -------------------------------------------------------------------
# Test 11: Audit log completeness
# -------------------------------------------------------------------
print("\n[11] Audit trail")

audit = CapabilityAuditLog()
audit.log_allowed("fn_a", "compute", ["compute"])
audit.log_blocked("fn_b", "network", ["compute"])
audit.log_escape_attempt("test escape")

test("audit log records all events", len(audit.entries) == 3)
test("audit exports to JSON", "fn_a" in audit.to_json())
test("audit summary is human-readable", "Blocked" in audit.summary())

# -------------------------------------------------------------------
# Test 12: CapabilitySet operations
# -------------------------------------------------------------------
print("\n[12] CapabilitySet operations")

caps_a = CapabilitySet.from_list(["network", "filesystem"])
caps_b = CapabilitySet.from_list(["network"])
caps_c = CapabilitySet.from_list(["quantum"])

test("subset check works", caps_b.is_subset_of(caps_a))
test("non-subset detected", not caps_c.is_subset_of(caps_a))
test("narrow produces intersection", "quantum" not in caps_a.narrow(caps_c).capabilities)
test("compute always present", "compute" in CapabilitySet.none().capabilities)

# -------------------------------------------------------------------
# Results
# -------------------------------------------------------------------
print("\n" + "=" * 60)
total = passed + failed
print(f"Results: {passed}/{total} passed, {failed} failed")
print("=" * 60)

sys.exit(0 if failed == 0 else 1)
