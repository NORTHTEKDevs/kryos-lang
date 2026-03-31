"""
Kryos Data Lineage Tracking

Every piece of data in Kryos can carry its lineage — where it came from,
what transformed it, and why. Critical for AI safety, debugging, compliance,
and explainability.

Lineage is automatic when enabled, zero-cost when disabled.
"""

from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from typing import Any, Callable, Optional


@dataclass
class LineageStep:
    """One step in a data lineage chain."""
    operation: str
    description: str = ""
    timestamp: float = 0.0
    source: str = ""
    metadata: dict[str, Any] = field(default_factory=dict)

    def __post_init__(self):
        if not self.timestamp:
            self.timestamp = time.time()


@dataclass
class Tracked:
    """
    A value with automatic lineage tracking.

    Every operation on a Tracked value records what happened,
    creating a complete audit trail from source to final result.
    """
    value: Any
    lineage: list[LineageStep] = field(default_factory=list)

    @classmethod
    def source(cls, value: Any, source: str, description: str = "") -> Tracked:
        """Create a tracked value from a source."""
        return cls(
            value=value,
            lineage=[LineageStep(
                operation="source",
                description=description or f"Loaded from {source}",
                source=source,
            )],
        )

    def transform(self, fn: Callable, operation: str = "transform",
                  description: str = "") -> Tracked:
        """Apply a transformation, recording it in lineage."""
        new_value = fn(self.value)
        fn_name = getattr(fn, '__name__', 'lambda')
        return Tracked(
            value=new_value,
            lineage=self.lineage + [LineageStep(
                operation=operation,
                description=description or f"{fn_name}()",
            )],
        )

    def filter(self, predicate: Callable, description: str = "") -> Tracked:
        """Filter (for collections), recording in lineage."""
        if isinstance(self.value, list):
            original_len = len(self.value)
            new_value = [x for x in self.value if predicate(x)]
            return Tracked(
                value=new_value,
                lineage=self.lineage + [LineageStep(
                    operation="filter",
                    description=description or f"Filtered: {original_len} -> {len(new_value)} items",
                    metadata={"before": original_len, "after": len(new_value)},
                )],
            )
        return self

    def annotate(self, operation: str, description: str = "",
                 metadata: dict | None = None) -> Tracked:
        """Add a lineage annotation without changing the value."""
        return Tracked(
            value=self.value,
            lineage=self.lineage + [LineageStep(
                operation=operation,
                description=description,
                metadata=metadata or {},
            )],
        )

    def inference(self, model_name: str, result: Any,
                  confidence: float = 1.0) -> Tracked:
        """Record an AI inference step."""
        return Tracked(
            value=result,
            lineage=self.lineage + [LineageStep(
                operation="inference",
                description=f"Model: {model_name}",
                metadata={"model": model_name, "confidence": confidence},
            )],
        )

    def explain(self) -> str:
        """Human-readable explanation of the full lineage."""
        lines = [f"Value: {self.value}", "", "Lineage:"]
        for i, step in enumerate(self.lineage, 1):
            lines.append(f"  {i}. [{step.operation}] {step.description}")
            if step.source:
                lines.append(f"     Source: {step.source}")
            if step.metadata:
                for k, v in step.metadata.items():
                    lines.append(f"     {k}: {v}")
        return "\n".join(lines)

    def to_json(self) -> str:
        """Export lineage as JSON for compliance/audit tools."""
        return json.dumps({
            "value": str(self.value),
            "lineage": [
                {
                    "operation": s.operation,
                    "description": s.description,
                    "timestamp": s.timestamp,
                    "source": s.source,
                    "metadata": s.metadata,
                }
                for s in self.lineage
            ],
        }, indent=2)

    def __repr__(self) -> str:
        return f"Tracked({self.value}, steps={len(self.lineage)})"
