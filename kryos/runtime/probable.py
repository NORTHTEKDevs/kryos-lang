"""
Kryos Probable<T> — First-Class Probability Type

AI doesn't think in true/false. It thinks in confidence scores.
Probable<T> makes uncertainty a language-level concept that propagates
through computation and forces explicit handling.

Usage in Kryos:
    let result: Probable<str> = model.predict(input)
    // result.value = "cat"
    // result.confidence = 0.92
    // result.alternatives = [("dog", 0.05), ("bird", 0.03)]

    match result {
        > 0.9 => act(result.value),
        0.5..0.9 => verify(result),
        < 0.5 => reject(result),
    }
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable, Generic, TypeVar, Optional

T = TypeVar("T")


@dataclass
class Probable:
    """
    A value with associated probability/confidence.

    The core AI-native type. Forces explicit handling of uncertainty.
    Confidence propagates through operations automatically.
    """
    value: Any
    confidence: float  # 0.0 to 1.0
    alternatives: list[tuple[Any, float]] = field(default_factory=list)
    source: str = ""  # what produced this (model name, sensor, etc.)
    _lineage: list[str] = field(default_factory=list)

    def __post_init__(self):
        # Clamp confidence to [0, 1]
        self.confidence = max(0.0, min(1.0, self.confidence))
        # Sort alternatives by probability descending
        self.alternatives.sort(key=lambda x: x[1], reverse=True)

    @classmethod
    def certain(cls, value: Any) -> Probable:
        """Create a certain (100% confidence) value."""
        return cls(value=value, confidence=1.0)

    @classmethod
    def uncertain(cls, options: list[tuple[Any, float]]) -> Probable:
        """Create from a distribution of options."""
        if not options:
            return cls(value=None, confidence=0.0)
        options.sort(key=lambda x: x[1], reverse=True)
        best_value, best_conf = options[0]
        return cls(
            value=best_value,
            confidence=best_conf,
            alternatives=options[1:],
        )

    @classmethod
    def from_softmax(cls, labels: list[Any], probs: list[float]) -> Probable:
        """Create from softmax output (label list + probability list)."""
        pairs = sorted(zip(labels, probs), key=lambda x: x[1], reverse=True)
        return cls(
            value=pairs[0][0],
            confidence=pairs[0][1],
            alternatives=[(v, p) for v, p in pairs[1:]],
        )

    # ------------------------------------------------------------------
    # Confidence-aware operations
    # ------------------------------------------------------------------

    def map(self, fn: Callable) -> Probable:
        """Transform the value, preserving confidence."""
        new_value = fn(self.value)
        new_alts = [(fn(v), p) for v, p in self.alternatives]
        return Probable(
            value=new_value,
            confidence=self.confidence,
            alternatives=new_alts,
            source=self.source,
            _lineage=self._lineage + [f"map({fn.__name__ if hasattr(fn, '__name__') else 'lambda'})"],
        )

    def flat_map(self, fn: Callable) -> Probable:
        """Transform where fn returns a Probable — confidences multiply."""
        result = fn(self.value)
        if isinstance(result, Probable):
            combined_confidence = self.confidence * result.confidence
            return Probable(
                value=result.value,
                confidence=combined_confidence,
                alternatives=result.alternatives,
                source=result.source,
                _lineage=self._lineage + result._lineage,
            )
        return self.map(fn)

    def filter(self, predicate: Callable, fallback: Any = None) -> Probable:
        """Keep value if predicate passes, otherwise use fallback or best alternative."""
        if predicate(self.value):
            return self
        # Try alternatives
        for alt_value, alt_conf in self.alternatives:
            if predicate(alt_value):
                remaining = [(v, p) for v, p in self.alternatives if v != alt_value]
                remaining.append((self.value, self.confidence))
                return Probable(
                    value=alt_value,
                    confidence=alt_conf,
                    alternatives=remaining,
                    source=self.source,
                    _lineage=self._lineage + ["filter(fallback_to_alternative)"],
                )
        return Probable(value=fallback, confidence=0.0, source=self.source)

    def combine(self, other: Probable, fn: Callable) -> Probable:
        """Combine two Probable values. Confidence is the product (independent assumption)."""
        new_value = fn(self.value, other.value)
        combined_conf = self.confidence * other.confidence
        return Probable(
            value=new_value,
            confidence=combined_conf,
            source=f"combine({self.source}, {other.source})",
            _lineage=self._lineage + other._lineage + ["combine"],
        )

    # ------------------------------------------------------------------
    # Threshold operations
    # ------------------------------------------------------------------

    def is_confident(self, threshold: float = 0.8) -> bool:
        """Check if confidence exceeds threshold."""
        return self.confidence >= threshold

    def require_confidence(self, threshold: float = 0.8) -> Any:
        """Return value if confident enough, raise otherwise."""
        if self.confidence >= threshold:
            return self.value
        raise ProbabilityError(
            f"Confidence {self.confidence:.2%} below threshold {threshold:.2%}. "
            f"Value: {self.value}, Alternatives: {self.alternatives[:3]}"
        )

    def or_else(self, fallback: Any) -> Any:
        """Return value if any confidence, fallback if zero."""
        if self.confidence > 0 and self.value is not None:
            return self.value
        return fallback

    def best_of(self, n: int = 1) -> list[tuple[Any, float]]:
        """Return top N options with their probabilities."""
        all_options = [(self.value, self.confidence)] + self.alternatives
        return sorted(all_options, key=lambda x: x[1], reverse=True)[:n]

    # ------------------------------------------------------------------
    # Distribution operations
    # ------------------------------------------------------------------

    def entropy(self) -> float:
        """Calculate Shannon entropy of the distribution."""
        import math
        all_probs = [self.confidence] + [p for _, p in self.alternatives]
        total = sum(all_probs)
        if total == 0:
            return 0.0
        normalized = [p / total for p in all_probs if p > 0]
        return -sum(p * math.log2(p) for p in normalized)

    def normalize(self) -> Probable:
        """Normalize all probabilities to sum to 1.0."""
        all_options = [(self.value, self.confidence)] + self.alternatives
        total = sum(p for _, p in all_options)
        if total == 0:
            return self
        normalized = [(v, p / total) for v, p in all_options]
        normalized.sort(key=lambda x: x[1], reverse=True)
        return Probable(
            value=normalized[0][0],
            confidence=normalized[0][1],
            alternatives=normalized[1:],
            source=self.source,
            _lineage=self._lineage + ["normalize"],
        )

    # ------------------------------------------------------------------
    # Explanation
    # ------------------------------------------------------------------

    def explain(self) -> str:
        """Human-readable explanation of this probabilistic value."""
        lines = [
            f"Value: {self.value} (confidence: {self.confidence:.1%})",
        ]
        if self.alternatives:
            lines.append("Alternatives:")
            for val, prob in self.alternatives[:5]:
                lines.append(f"  {val}: {prob:.1%}")
        if self.source:
            lines.append(f"Source: {self.source}")
        if self._lineage:
            lines.append(f"Pipeline: {' -> '.join(self._lineage)}")
        return "\n".join(lines)

    # ------------------------------------------------------------------
    # Python integration
    # ------------------------------------------------------------------

    def __repr__(self) -> str:
        return f"Probable({self.value}, {self.confidence:.2%})"

    def __bool__(self) -> bool:
        """Probable is truthy if confidence > 50%."""
        return self.confidence > 0.5

    def __eq__(self, other: Any) -> bool:
        if isinstance(other, Probable):
            return self.value == other.value
        return self.value == other

    def __gt__(self, threshold: float) -> bool:
        """Compare confidence against a threshold: result > 0.9"""
        return self.confidence > threshold

    def __lt__(self, threshold: float) -> bool:
        return self.confidence < threshold

    def __ge__(self, threshold: float) -> bool:
        return self.confidence >= threshold


class ProbabilityError(Exception):
    """Raised when a probability constraint is violated."""
    pass


# ------------------------------------------------------------------
# Ensemble — combine multiple Probable values
# ------------------------------------------------------------------

class Ensemble:
    """
    Combine multiple model predictions into a single Probable result.

    Strategies:
    - majority_vote: pick the most common prediction
    - weighted_average: weight by model confidence
    - best_confidence: pick the highest confidence
    """

    @staticmethod
    def majority_vote(predictions: list[Probable]) -> Probable:
        """Combine by majority vote."""
        votes: dict[Any, float] = {}
        for pred in predictions:
            key = pred.value
            votes[key] = votes.get(key, 0) + 1

        total = len(predictions)
        best_value = max(votes, key=votes.get)
        confidence = votes[best_value] / total

        alternatives = [
            (v, count / total) for v, count in votes.items() if v != best_value
        ]
        return Probable(
            value=best_value,
            confidence=confidence,
            alternatives=alternatives,
            source=f"ensemble(majority_vote, n={total})",
        )

    @staticmethod
    def weighted_average(predictions: list[Probable]) -> Probable:
        """Combine by confidence-weighted aggregation."""
        if not predictions:
            return Probable(value=None, confidence=0.0)

        # Aggregate confidence per unique value
        scores: dict[Any, float] = {}
        for pred in predictions:
            scores[pred.value] = scores.get(pred.value, 0) + pred.confidence
            for alt_val, alt_conf in pred.alternatives:
                scores[alt_val] = scores.get(alt_val, 0) + alt_conf

        total = sum(scores.values())
        if total == 0:
            return predictions[0]

        ranked = sorted(scores.items(), key=lambda x: x[1], reverse=True)
        best_value, best_score = ranked[0]

        return Probable(
            value=best_value,
            confidence=best_score / total,
            alternatives=[(v, s / total) for v, s in ranked[1:]],
            source=f"ensemble(weighted, n={len(predictions)})",
        )

    @staticmethod
    def best_confidence(predictions: list[Probable]) -> Probable:
        """Pick the prediction with highest confidence."""
        if not predictions:
            return Probable(value=None, confidence=0.0)
        return max(predictions, key=lambda p: p.confidence)
