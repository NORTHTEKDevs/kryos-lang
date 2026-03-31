"""
Kryos Licensing System Test Suite

Verifies:
- Tier detection and capability gating
- License key validation (dev keys, production format)
- LicenseViolation with proper upgrade messages
- Capability → tier mapping is correct
- Integration with capability analyzer
"""

import sys
import os

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from kryos.compiler.licensing import (
    LicenseTier, LicenseManager, LicenseViolation, LicenseKey,
    CAPABILITY_TIERS, TIER_FEATURES, get_license_manager, reset_license_manager,
)

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
print("Kryos Licensing System Test Suite")
print("=" * 60)

# -------------------------------------------------------------------
# Test 1: Default tier is Community
# -------------------------------------------------------------------
print("\n[1] Default tier")

reset_license_manager()
# Clear env to ensure default
old_env = os.environ.pop("KRYOS_LICENSE_KEY", None)
reset_license_manager()
mgr = LicenseManager()
test("default tier is Community", mgr.tier == LicenseTier.COMMUNITY)

# -------------------------------------------------------------------
# Test 2: Dev key activation
# -------------------------------------------------------------------
print("\n[2] Dev key activation")

mgr2 = LicenseManager()
success, msg = mgr2.activate("KRYOS-DEV-PRO")
test("dev PRO key activates", success and mgr2.tier == LicenseTier.PRO)

mgr3 = LicenseManager()
success, msg = mgr3.activate("KRYOS-DEV-ENTERPRISE")
test("dev ENTERPRISE key activates", success and mgr3.tier == LicenseTier.ENTERPRISE)

mgr4 = LicenseManager()
success, msg = mgr4.activate("KRYOS-DEV-CLOUD")
test("dev CLOUD key activates", success and mgr4.tier == LicenseTier.CLOUD)

# -------------------------------------------------------------------
# Test 3: Invalid key rejection
# -------------------------------------------------------------------
print("\n[3] Invalid key rejection")

mgr5 = LicenseManager()
success, msg = mgr5.activate("INVALID-KEY")
test("invalid key rejected", not success)

success, msg = mgr5.activate("KRYOS-DEV-FAKESTIER")
test("invalid tier rejected", not success)

# -------------------------------------------------------------------
# Test 4: Community tier capability checks
# -------------------------------------------------------------------
print("\n[4] Community tier capabilities")

mgr_comm = LicenseManager()
mgr_comm._tier = LicenseTier.COMMUNITY

allowed, _ = mgr_comm.check_capability("compute")
test("compute allowed in community", allowed)

allowed, _ = mgr_comm.check_capability("filesystem:read")
test("filesystem:read allowed in community", allowed)

allowed, _ = mgr_comm.check_capability("network")
test("network allowed in community", allowed)

allowed, _ = mgr_comm.check_capability("network:http")
test("network:http allowed in community", allowed)

allowed, _ = mgr_comm.check_capability("gpu")
test("gpu allowed in community", allowed)

allowed, _ = mgr_comm.check_capability("gpu:compute")
test("gpu:compute allowed in community", allowed)

allowed, _ = mgr_comm.check_capability("ffi:python")
test("ffi:python allowed in community", allowed)

# -------------------------------------------------------------------
# Test 5: Community tier locked capabilities
# -------------------------------------------------------------------
print("\n[5] Community tier locks")

allowed, required = mgr_comm.check_capability("network:raw_socket")
test("network:raw_socket locked in community", not allowed and required == LicenseTier.PRO)

allowed, required = mgr_comm.check_capability("memory:raw")
test("memory:raw locked in community", not allowed and required == LicenseTier.ENTERPRISE)

allowed, required = mgr_comm.check_capability("syscall:direct")
test("syscall:direct locked in community", not allowed and required == LicenseTier.ENTERPRISE)

allowed, required = mgr_comm.check_capability("ffi:native")
test("ffi:native locked in community", not allowed and required == LicenseTier.PRO)

allowed, required = mgr_comm.check_capability("quantum")
test("quantum locked in community", not allowed and required == LicenseTier.ENTERPRISE)

allowed, required = mgr_comm.check_capability("agent:autonomous")
test("agent:autonomous locked in community", not allowed and required == LicenseTier.PRO)

allowed, required = mgr_comm.check_capability("self_modify:any")
test("self_modify:any locked in community", not allowed and required == LicenseTier.ENTERPRISE)

# -------------------------------------------------------------------
# Test 6: Pro tier unlocks
# -------------------------------------------------------------------
print("\n[6] Pro tier unlocks")

mgr_pro = LicenseManager()
mgr_pro._tier = LicenseTier.PRO

allowed, _ = mgr_pro.check_capability("network:raw_socket")
test("network:raw_socket unlocked in Pro", allowed)

allowed, _ = mgr_pro.check_capability("ffi:native")
test("ffi:native unlocked in Pro", allowed)

allowed, _ = mgr_pro.check_capability("gpu:optimize")
test("gpu:optimize unlocked in Pro", allowed)

allowed, _ = mgr_pro.check_capability("agent:autonomous")
test("agent:autonomous unlocked in Pro", allowed)

# Pro still can't access Enterprise
allowed, required = mgr_pro.check_capability("quantum")
test("quantum still locked in Pro", not allowed and required == LicenseTier.ENTERPRISE)

allowed, required = mgr_pro.check_capability("memory:raw")
test("memory:raw still locked in Pro", not allowed and required == LicenseTier.ENTERPRISE)

# -------------------------------------------------------------------
# Test 7: Enterprise tier unlocks everything
# -------------------------------------------------------------------
print("\n[7] Enterprise tier unlocks")

mgr_ent = LicenseManager()
mgr_ent._tier = LicenseTier.ENTERPRISE

for cap in CAPABILITY_TIERS:
    allowed, _ = mgr_ent.check_capability(cap)
    if not allowed:
        test(f"{cap} unlocked in Enterprise", False)
        break
else:
    test("all capabilities unlocked in Enterprise", True)

# -------------------------------------------------------------------
# Test 8: Cloud tier == Pro access level
# -------------------------------------------------------------------
print("\n[8] Cloud tier")

mgr_cloud = LicenseManager()
mgr_cloud._tier = LicenseTier.CLOUD

test("Cloud effective level == Pro", mgr_cloud.tier.effective_level == LicenseTier.PRO.value)

allowed, _ = mgr_cloud.check_capability("gpu:optimize")
test("Cloud has Pro capabilities", allowed)

allowed, _ = mgr_cloud.check_capability("quantum")
test("Cloud does not have Enterprise capabilities", not allowed)

# -------------------------------------------------------------------
# Test 9: LicenseViolation error messages
# -------------------------------------------------------------------
print("\n[9] License violation messages")

try:
    mgr_comm.require_capability("quantum")
    test("LicenseViolation raised", False)
except LicenseViolation as e:
    test("LicenseViolation raised for locked capability", True)
    test("violation message mentions Enterprise", "Enterprise" in str(e))
    test("violation message mentions upgrade path", "frostbytedigital" in str(e).lower())

# -------------------------------------------------------------------
# Test 10: Tier features are complete
# -------------------------------------------------------------------
print("\n[10] Tier feature definitions")

for tier in [LicenseTier.COMMUNITY, LicenseTier.PRO, LicenseTier.ENTERPRISE, LicenseTier.CLOUD]:
    test(f"{tier.name} has feature definition", tier in TIER_FEATURES)
    info = TIER_FEATURES[tier]
    test(f"{tier.name} has features list", len(info.get("features", [])) > 0, str(info.get("features", [])))

# -------------------------------------------------------------------
# Test 11: Version display includes tier
# -------------------------------------------------------------------
print("\n[11] Status display")

status = mgr_comm.get_status()
test("status shows tier name", "Community" in status)
test("status shows capability list", "compute" in status)

# -------------------------------------------------------------------
# Test 12: Integration with capability analyzer
# -------------------------------------------------------------------
print("\n[12] Analyzer integration")

from kryos.compiler.capabilities import CapabilityAnalyzer, CapabilityTier
from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse

source = """
@capabilities(quantum)
fn quantum_algo() -> i32 {
    return 42
}
"""
tokens = tokenize(source, "<test>")
module = parse(tokens)

# Community tier should block quantum
analyzer = CapabilityAnalyzer(tier=CapabilityTier.COMMUNITY)
report = analyzer.analyze(module)
test(
    "analyzer blocks quantum in Community",
    any("enterprise" in e.lower() or "license" in e.lower() for e in report["errors"]),
    f"errors: {report['errors']}",
)

# Enterprise tier should allow quantum
analyzer2 = CapabilityAnalyzer(tier=CapabilityTier.ENTERPRISE)
report2 = analyzer2.analyze(module)
test(
    "analyzer allows quantum in Enterprise",
    len(report2["errors"]) == 0,
    f"errors: {report2['errors']}",
)

# -------------------------------------------------------------------
# Results
# -------------------------------------------------------------------
print("\n" + "=" * 60)
total = passed + failed
print(f"Results: {passed}/{total} passed, {failed} failed")
print("=" * 60)

# Restore env
if old_env:
    os.environ["KRYOS_LICENSE_KEY"] = old_env

# Clean up any license file we created during testing
from pathlib import Path
test_license = Path.home() / ".kryos" / "license.json"
if test_license.exists():
    test_license.unlink()

sys.exit(0 if failed == 0 else 1)
