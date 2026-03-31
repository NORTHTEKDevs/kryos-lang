"""
Kryos Cost-Aware Computation

Every computation has a cost — money, energy, latency, tokens.
Kryos makes this visible and controllable so AI systems don't
bankrupt you and you can optimize spend.
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import Any, Callable, Optional


@dataclass
class ComputeCost:
    """Tracks the cost of a computation."""
    wall_time_ms: float = 0.0
    cpu_time_ms: float = 0.0
    gpu_seconds: float = 0.0
    tokens_used: int = 0
    api_calls: int = 0
    money_usd: float = 0.0
    energy_kwh: float = 0.0

    def __add__(self, other: ComputeCost) -> ComputeCost:
        return ComputeCost(
            wall_time_ms=self.wall_time_ms + other.wall_time_ms,
            cpu_time_ms=self.cpu_time_ms + other.cpu_time_ms,
            gpu_seconds=self.gpu_seconds + other.gpu_seconds,
            tokens_used=self.tokens_used + other.tokens_used,
            api_calls=self.api_calls + other.api_calls,
            money_usd=self.money_usd + other.money_usd,
            energy_kwh=self.energy_kwh + other.energy_kwh,
        )

    def __repr__(self) -> str:
        parts = []
        if self.wall_time_ms > 0:
            parts.append(f"time={self.wall_time_ms:.1f}ms")
        if self.tokens_used > 0:
            parts.append(f"tokens={self.tokens_used}")
        if self.money_usd > 0:
            parts.append(f"cost=${self.money_usd:.4f}")
        if self.api_calls > 0:
            parts.append(f"api_calls={self.api_calls}")
        if self.gpu_seconds > 0:
            parts.append(f"gpu={self.gpu_seconds:.2f}s")
        return f"Cost({', '.join(parts)})" if parts else "Cost(zero)"


@dataclass
class Budget:
    """A spending budget that tracks usage and enforces limits."""
    max_usd: float = float('inf')
    max_tokens: int = 2**63
    max_api_calls: int = 2**63
    max_latency_ms: float = float('inf')
    spent: ComputeCost = field(default_factory=ComputeCost)

    @property
    def remaining_usd(self) -> float:
        return max(0, self.max_usd - self.spent.money_usd)

    @property
    def remaining_tokens(self) -> int:
        return max(0, self.max_tokens - self.spent.tokens_used)

    @property
    def is_exhausted(self) -> bool:
        return (
            self.spent.money_usd >= self.max_usd or
            self.spent.tokens_used >= self.max_tokens or
            self.spent.api_calls >= self.max_api_calls
        )

    def charge(self, cost: ComputeCost) -> None:
        """Add a cost. Raises BudgetExceeded if over limit."""
        new_total = self.spent + cost
        if new_total.money_usd > self.max_usd:
            raise BudgetExceeded(
                f"Budget exceeded: ${new_total.money_usd:.4f} > ${self.max_usd:.4f}"
            )
        if new_total.tokens_used > self.max_tokens:
            raise BudgetExceeded(
                f"Token budget exceeded: {new_total.tokens_used} > {self.max_tokens}"
            )
        self.spent = new_total

    def status(self) -> str:
        lines = [
            f"Budget: ${self.spent.money_usd:.4f} / ${self.max_usd:.2f}",
            f"Tokens: {self.spent.tokens_used} / {self.max_tokens}",
            f"API calls: {self.spent.api_calls} / {self.max_api_calls}",
        ]
        if self.is_exhausted:
            lines.append("STATUS: EXHAUSTED")
        return "\n".join(lines)


class BudgetExceeded(Exception):
    pass


class CostTracker:
    """
    Track computation costs across a block of code.

    Usage:
        tracker = CostTracker()
        with tracker:
            expensive_operation()
        print(tracker.total)
    """

    def __init__(self, budget: Budget | None = None) -> None:
        self.budget = budget
        self.total = ComputeCost()
        self._start_time: float = 0.0

    def __enter__(self) -> CostTracker:
        self._start_time = time.time()
        return self

    def __exit__(self, *args: Any) -> None:
        elapsed = (time.time() - self._start_time) * 1000
        self.total.wall_time_ms += elapsed

    def record(self, cost: ComputeCost) -> None:
        """Record a cost event."""
        self.total = self.total + cost
        if self.budget:
            self.budget.charge(cost)

    def record_tokens(self, count: int, cost_per_token: float = 0.0) -> None:
        """Shorthand for recording token usage."""
        self.record(ComputeCost(
            tokens_used=count,
            money_usd=count * cost_per_token,
            api_calls=1,
        ))

    def record_api_call(self, cost_usd: float = 0.0, tokens: int = 0) -> None:
        """Shorthand for recording an API call."""
        self.record(ComputeCost(
            api_calls=1,
            money_usd=cost_usd,
            tokens_used=tokens,
        ))


def timed(fn: Callable) -> Callable:
    """Decorator that tracks execution time as a ComputeCost."""
    def wrapper(*args: Any, **kwargs: Any) -> tuple[Any, ComputeCost]:
        start = time.time()
        result = fn(*args, **kwargs)
        elapsed = (time.time() - start) * 1000
        cost = ComputeCost(wall_time_ms=elapsed)
        return result, cost
    wrapper.__name__ = fn.__name__
    return wrapper
