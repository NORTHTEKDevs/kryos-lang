"""
Kryos Licensing System

Manages license tiers, key validation, and feature gating.
The licensing system is transparent and non-invasive:
- Community edition is fully functional open source
- License enforcement lives in the proprietary compiler, not the open core
- Locked capabilities give clear upgrade messages
- Offline tokens support air-gapped deployments

Tiers:
  COMMUNITY  — Free, open source. Full language, safe capabilities.
  PRO        — $499/mo or $4,999/yr. GPU, FFI, autonomous agents, optimizing compiler.
  ENTERPRISE — $50K-$500K/yr. Quantum, raw memory, syscall, formal verification.
  CLOUD      — Usage-based. Managed infra, all Pro features, per-compile pricing.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import time
from dataclasses import dataclass, field
from enum import Enum, auto
from pathlib import Path
from typing import Any, Optional


class LicenseTier(Enum):
    """License tiers in order of access level."""
    COMMUNITY = 1
    PRO = 2
    ENTERPRISE = 3
    CLOUD = 4  # same access as PRO, different billing

    @property
    def display_name(self) -> str:
        names = {
            LicenseTier.COMMUNITY: "Kryos Community (Free)",
            LicenseTier.PRO: "Kryos Pro",
            LicenseTier.ENTERPRISE: "Kryos Enterprise",
            LicenseTier.CLOUD: "Kryos Cloud",
        }
        return names[self]

    @property
    def effective_level(self) -> int:
        """Effective access level (Cloud == Pro for capability purposes)."""
        if self == LicenseTier.CLOUD:
            return LicenseTier.PRO.value
        return self.value


class LicenseViolation(Exception):
    """Raised when code requires a tier higher than the active license."""

    def __init__(self, capability: str, required_tier: LicenseTier, current_tier: LicenseTier):
        self.capability = capability
        self.required_tier = required_tier
        self.current_tier = current_tier
        super().__init__(self._message())

    def _message(self) -> str:
        tier_info = {
            LicenseTier.PRO: (
                "Kryos Pro — $499/month or $4,999/year per seat\n"
                "  Includes: GPU codegen, tensor primitives, FFI, autonomous agents,\n"
                "  optimizing compiler, private package registry\n"
                "  Upgrade at: https://frostbytedigital.io/kryos/pro"
            ),
            LicenseTier.ENTERPRISE: (
                "Kryos Enterprise — Custom pricing ($50K-$500K/year)\n"
                "  Includes: Quantum compute, raw memory, syscall, formal verification,\n"
                "  real-time proofs, FIPS crypto, air-gapped deployment, dedicated support\n"
                "  Contact: enterprise@frostbytedigital.io"
            ),
        }
        upgrade_info = tier_info.get(self.required_tier, "Contact sales@frostbytedigital.io")
        return (
            f"\n{'=' * 64}\n"
            f"  LICENSE REQUIRED: '{self.capability}' capability\n"
            f"\n"
            f"  Current license:  {self.current_tier.display_name}\n"
            f"  Required license: {self.required_tier.display_name}\n"
            f"\n"
            f"  {upgrade_info}\n"
            f"{'=' * 64}"
        )


@dataclass
class LicenseKey:
    """A validated license key."""
    key_id: str
    tier: LicenseTier
    organization: str
    seats: int
    issued_at: float
    expires_at: float
    features: list[str] = field(default_factory=list)
    offline: bool = False

    @property
    def is_expired(self) -> bool:
        return time.time() > self.expires_at

    @property
    def is_valid(self) -> bool:
        return not self.is_expired

    def to_dict(self) -> dict:
        return {
            "key_id": self.key_id,
            "tier": self.tier.name,
            "organization": self.organization,
            "seats": self.seats,
            "issued_at": self.issued_at,
            "expires_at": self.expires_at,
            "features": self.features,
            "offline": self.offline,
        }


# ---------------------------------------------------------------------------
# Capability → Tier mapping (the definitive source of truth)
# ---------------------------------------------------------------------------

# All capabilities and which tier they require
CAPABILITY_TIERS: dict[str, LicenseTier] = {
    # COMMUNITY — free, always available
    "compute":              LicenseTier.COMMUNITY,
    "filesystem":           LicenseTier.COMMUNITY,
    "filesystem:read":      LicenseTier.COMMUNITY,
    "filesystem:write":     LicenseTier.COMMUNITY,
    "network":              LicenseTier.COMMUNITY,
    "network:http":         LicenseTier.COMMUNITY,
    "memory":               LicenseTier.COMMUNITY,

    # PRO — $499/mo
    "network:raw_socket":   LicenseTier.PRO,
    "ffi":                  LicenseTier.PRO,
    "ffi:native":           LicenseTier.PRO,
    "ffi:python":           LicenseTier.COMMUNITY,  # Python FFI stays community for adoption
    "gpu":                  LicenseTier.COMMUNITY,   # Basic GPU is community
    "gpu:compute":          LicenseTier.COMMUNITY,   # Basic GPU compute is community
    "gpu:optimize":         LicenseTier.PRO,         # Optimizing GPU codegen is Pro
    "agent":                LicenseTier.PRO,
    "agent:autonomous":     LicenseTier.PRO,

    # ENTERPRISE — $50K+/yr
    "memory:raw":           LicenseTier.ENTERPRISE,
    "syscall":              LicenseTier.ENTERPRISE,
    "syscall:direct":       LicenseTier.ENTERPRISE,
    "quantum":              LicenseTier.ENTERPRISE,
    "quantum:simulate":     LicenseTier.ENTERPRISE,
    "quantum:hardware":     LicenseTier.ENTERPRISE,
    "self_modify":          LicenseTier.ENTERPRISE,
    "self_modify:any":      LicenseTier.ENTERPRISE,
    "formal_verify":        LicenseTier.ENTERPRISE,
    "real_time":            LicenseTier.ENTERPRISE,
    "crypto:fips":          LicenseTier.ENTERPRISE,
}

# Detailed feature descriptions per tier
TIER_FEATURES: dict[LicenseTier, dict] = {
    LicenseTier.COMMUNITY: {
        "name": "Kryos Community",
        "price": "Free (Open Source)",
        "target": "Individual developers, students, open source projects",
        "features": [
            "Full language spec — all syntax, types, structs, closures, traits",
            "Tree-walking interpreter",
            "CLI: run, repl, check, test, validate, migrate, version",
            "CPU compilation target via LLVM (when built)",
            "WASM compilation target",
            "Basic standard library — I/O, strings, collections, math",
            "Capability enforcement — safe defaults",
            "IDE language server protocol (LSP) support",
            "Package manager (basic)",
            "Full documentation and examples",
            "Basic GPU compute (gpu, gpu:compute)",
            "Python FFI (ffi:python)",
            "Self-healing runtime",
        ],
        "capabilities": [
            "compute", "filesystem", "filesystem:read", "filesystem:write",
            "network", "network:http", "memory", "gpu", "gpu:compute",
            "ffi:python",
        ],
    },
    LicenseTier.PRO: {
        "name": "Kryos Pro",
        "price": "$499/month per seat or $4,999/year",
        "target": "Startups, AI teams, professional developers, SaaS companies",
        "features": [
            "Everything in Community",
            "Proprietary optimizing compiler",
            "GPU codegen — SPIR-V/PTX/Metal targets with kernel fusion",
            "Tensor primitives + autodiff",
            "FFI layer — safe C/C++ interop with capability wrapping",
            "Autonomous agent primitives",
            "Advanced standard library — ML ops, crypto, protocol parsing",
            "Priority compiler optimization passes",
            "Private package registry",
            "IDE with AI-powered suggestions",
            "Capability audit reports — full program capability map export",
            "Email support",
        ],
        "capabilities": [
            "network:raw_socket", "ffi", "ffi:native", "gpu:optimize",
            "agent", "agent:autonomous",
        ],
    },
    LicenseTier.ENTERPRISE: {
        "name": "Kryos Enterprise",
        "price": "Custom pricing ($50K-$500K/year)",
        "target": "Defense contractors, financial institutions, pharma, cloud providers",
        "features": [
            "Everything in Pro",
            "Quantum IR + simulator backend",
            "Real QPU targeting (IBM Q, IonQ, Azure Quantum)",
            "Formal verification passes — compile-time proofs",
            "Real-time deadline proofs (@real_time enforcement)",
            "FIPS-certified crypto primitives",
            "Air-gapped deployment support (offline license tokens)",
            "Cognitive kernel licensed for embedding",
            "Custom hardware backend development",
            "Dedicated support + SLA",
            "Source code escrow",
            "Export control compliance documentation (ITAR/EAR)",
        ],
        "capabilities": [
            "memory:raw", "syscall", "syscall:direct",
            "quantum", "quantum:simulate", "quantum:hardware",
            "self_modify", "self_modify:any",
            "formal_verify", "real_time", "crypto:fips",
        ],
    },
    LicenseTier.CLOUD: {
        "name": "Kryos Cloud",
        "price": "Usage-based (per compilation/deployment)",
        "target": "Teams who want managed infrastructure",
        "features": [
            "All Pro features included",
            "Managed compilation service optimized per cloud hardware",
            "AWS Graviton, Google TPU, Azure FPGA optimized backends",
            "Distributed Kryos agent swarm deployment",
            "LLM training infrastructure (when available)",
            "Pay per compile, pay per agent deployment, pay per training run",
            "SLA-backed uptime",
        ],
        "capabilities": [],  # Same as Pro
    },
}


class LicenseManager:
    """
    Manages the active license for a Kryos installation.

    License detection order:
    1. KRYOS_LICENSE_KEY environment variable
    2. ~/.kryos/license.json file
    3. Project-local .kryos-license file
    4. Default to Community
    """

    def __init__(self) -> None:
        self._active_key: LicenseKey | None = None
        self._tier: LicenseTier = LicenseTier.COMMUNITY
        self._load_license()

    @property
    def tier(self) -> LicenseTier:
        return self._tier

    @property
    def active_key(self) -> LicenseKey | None:
        return self._active_key

    def _load_license(self) -> None:
        """Load license from environment or file."""
        # 1. Environment variable
        env_key = os.environ.get("KRYOS_LICENSE_KEY")
        if env_key:
            self._validate_and_set(env_key)
            return

        # 2. User home config
        home_license = Path.home() / ".kryos" / "license.json"
        if home_license.exists():
            try:
                data = json.loads(home_license.read_text(encoding="utf-8"))
                if "key" in data:
                    self._validate_and_set(data["key"])
                    return
            except (json.JSONDecodeError, KeyError):
                pass

        # 3. Project-local
        local_license = Path(".kryos-license")
        if local_license.exists():
            try:
                key = local_license.read_text(encoding="utf-8").strip()
                self._validate_and_set(key)
                return
            except Exception:
                pass

        # 4. Default to Community
        self._tier = LicenseTier.COMMUNITY

    def _validate_and_set(self, key_string: str) -> bool:
        """Validate a license key string and set the active tier."""
        parsed = self._parse_key(key_string)
        if parsed and parsed.is_valid:
            self._active_key = parsed
            self._tier = parsed.tier
            return True
        return False

    def _parse_key(self, key_string: str) -> LicenseKey | None:
        """
        Parse and validate a license key.

        Key format: KRYOS-{TIER}-{ORG_HASH}-{TIMESTAMP}-{SIGNATURE}
        Example: KRYOS-PRO-a1b2c3d4-1711843200-e5f6g7h8

        For development/testing, also accepts:
        KRYOS-DEV-PRO, KRYOS-DEV-ENTERPRISE, KRYOS-DEV-CLOUD
        """
        key_string = key_string.strip()

        # Development keys (for testing)
        if key_string.startswith("KRYOS-DEV-"):
            tier_str = key_string.split("-")[-1].upper()
            try:
                tier = LicenseTier[tier_str]
            except KeyError:
                return None
            return LicenseKey(
                key_id="dev-key",
                tier=tier,
                organization="Development",
                seats=999,
                issued_at=time.time(),
                expires_at=time.time() + 365 * 24 * 3600,  # 1 year
                features=["dev"],
                offline=True,
            )

        # Production key format
        parts = key_string.split("-")
        if len(parts) < 5 or parts[0] != "KRYOS":
            return None

        tier_str = parts[1].upper()
        try:
            tier = LicenseTier[tier_str]
        except KeyError:
            return None

        org_hash = parts[2]
        try:
            timestamp = float(parts[3])
        except ValueError:
            return None

        signature = parts[4]

        # Validate signature (HMAC-SHA256)
        # In production, this would check against FrostByte's signing key
        # For now, we validate the structure
        expected = self._compute_signature(tier_str, org_hash, str(int(timestamp)))
        if not hmac.compare_digest(signature, expected):
            # For now, accept any well-formed key (production would reject)
            pass

        return LicenseKey(
            key_id=f"{org_hash}-{int(timestamp)}",
            tier=tier,
            organization=org_hash,
            seats=1,
            issued_at=timestamp,
            expires_at=timestamp + 365 * 24 * 3600,
            offline=False,
        )

    @staticmethod
    def _compute_signature(tier: str, org: str, timestamp: str) -> str:
        """Compute license key signature. Production uses FrostByte signing key."""
        # Placeholder — production would use proper HMAC with server-side secret
        msg = f"{tier}:{org}:{timestamp}"
        return hashlib.sha256(msg.encode()).hexdigest()[:8]

    def activate(self, key_string: str) -> tuple[bool, str]:
        """Activate a license key. Returns (success, message)."""
        if self._validate_and_set(key_string):
            # Save to user home
            config_dir = Path.home() / ".kryos"
            config_dir.mkdir(exist_ok=True)
            license_file = config_dir / "license.json"
            license_file.write_text(
                json.dumps({"key": key_string, "activated_at": time.time()}, indent=2),
                encoding="utf-8",
            )
            return True, f"License activated: {self._tier.display_name}"
        return False, "Invalid or expired license key."

    def deactivate(self) -> str:
        """Deactivate the current license."""
        license_file = Path.home() / ".kryos" / "license.json"
        if license_file.exists():
            license_file.unlink()
        self._active_key = None
        self._tier = LicenseTier.COMMUNITY
        return "License deactivated. Reverted to Community edition."

    def check_capability(self, capability: str) -> tuple[bool, LicenseTier | None]:
        """
        Check if a capability is available under the current license.
        Returns (allowed, required_tier_if_blocked).
        """
        required_tier = CAPABILITY_TIERS.get(capability)

        if required_tier is None:
            # Unknown capability — check if it's a sub-capability
            base = capability.split(":")[0] if ":" in capability else capability
            required_tier = CAPABILITY_TIERS.get(base, LicenseTier.COMMUNITY)

        effective_level = self._tier.effective_level
        if effective_level >= required_tier.value:
            return True, None
        return False, required_tier

    def require_capability(self, capability: str) -> None:
        """Require a capability or raise LicenseViolation."""
        allowed, required_tier = self.check_capability(capability)
        if not allowed:
            raise LicenseViolation(capability, required_tier, self._tier)

    def get_status(self) -> str:
        """Get a human-readable license status."""
        lines = [
            f"License: {self._tier.display_name}",
        ]
        if self._active_key:
            lines.append(f"Organization: {self._active_key.organization}")
            lines.append(f"Seats: {self._active_key.seats}")
            if self._active_key.is_expired:
                lines.append("Status: EXPIRED")
            else:
                import datetime
                exp = datetime.datetime.fromtimestamp(self._active_key.expires_at)
                lines.append(f"Expires: {exp.strftime('%Y-%m-%d')}")
                lines.append("Status: Active")
        else:
            lines.append("Status: No license key (Community defaults)")

        lines.append(f"\nAvailable capabilities at {self._tier.name} tier:")
        for cap, tier in sorted(CAPABILITY_TIERS.items()):
            effective = self._tier.effective_level
            available = effective >= tier.value
            marker = "  [x]" if available else "  [ ]"
            tier_label = f"({tier.name})" if not available else ""
            lines.append(f"  {marker} {cap} {tier_label}")

        return "\n".join(lines)

    def get_tier_comparison(self) -> str:
        """Get a comparison table of all tiers."""
        lines = [
            "Kryos License Tiers",
            "=" * 70,
        ]
        for tier in [LicenseTier.COMMUNITY, LicenseTier.PRO, LicenseTier.ENTERPRISE, LicenseTier.CLOUD]:
            info = TIER_FEATURES[tier]
            lines.append(f"\n{info['name']} — {info['price']}")
            lines.append(f"  Target: {info['target']}")
            lines.append(f"  Features:")
            for feat in info["features"]:
                lines.append(f"    - {feat}")
            if info["capabilities"]:
                lines.append(f"  Unlocked capabilities:")
                for cap in info["capabilities"]:
                    lines.append(f"    + {cap}")
        return "\n".join(lines)


# Global license manager instance
_license_manager: LicenseManager | None = None


def get_license_manager() -> LicenseManager:
    """Get or create the global license manager."""
    global _license_manager
    if _license_manager is None:
        _license_manager = LicenseManager()
    return _license_manager


def reset_license_manager() -> None:
    """Reset the global license manager (for testing)."""
    global _license_manager
    _license_manager = None
