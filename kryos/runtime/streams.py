"""
Kryos Stream Runtime

AI systems process continuous data — sensor feeds, user interactions,
market data, log streams. Streams are a native type in Kryos.

A Stream is a lazy, potentially infinite sequence that supports
real-time processing, windowing, backpressure, and parallel mapping.
"""

from __future__ import annotations

import time
from collections import deque
from dataclasses import dataclass, field
from typing import Any, Callable, Iterator, Optional


class Stream:
    """
    A lazy, composable data stream.

    Streams are the backbone of real-time AI systems. They support:
    - Lazy evaluation (nothing runs until consumed)
    - Windowing (sliding, tumbling, session)
    - Parallel mapping (AI inference per element)
    - Backpressure (throttling)
    - Merging multiple streams
    - Side effects (tap for logging/monitoring)
    """

    def __init__(self, source: Iterator | Callable | list | None = None) -> None:
        if isinstance(source, list):
            self._source = iter(source)
        elif callable(source) and not hasattr(source, '__next__'):
            self._source = self._generator_from_fn(source)
        elif source is not None:
            self._source = source
        else:
            self._source = iter([])

        self._transforms: list[Callable] = []
        self._consumed = False

    @staticmethod
    def _generator_from_fn(fn: Callable) -> Iterator:
        """Turn a callable into an infinite generator."""
        while True:
            yield fn()

    @classmethod
    def from_list(cls, items: list) -> Stream:
        return cls(iter(items))

    @classmethod
    def from_range(cls, start: int, end: int, step: int = 1) -> Stream:
        return cls(iter(range(start, end, step)))

    @classmethod
    def infinite(cls, generator_fn: Callable) -> Stream:
        """Create an infinite stream from a generator function."""
        return cls(generator_fn)

    @classmethod
    def empty(cls) -> Stream:
        return cls(iter([]))

    # ------------------------------------------------------------------
    # Transformations (lazy)
    # ------------------------------------------------------------------

    def map(self, fn: Callable) -> Stream:
        """Transform each element."""
        parent = self

        def gen():
            for item in parent:
                yield fn(item)

        return Stream(gen())

    def filter(self, predicate: Callable) -> Stream:
        """Keep elements matching predicate."""
        parent = self

        def gen():
            for item in parent:
                if predicate(item):
                    yield item

        return Stream(gen())

    def flat_map(self, fn: Callable) -> Stream:
        """Map each element to a stream and flatten."""
        parent = self

        def gen():
            for item in parent:
                result = fn(item)
                if hasattr(result, '__iter__'):
                    yield from result
                else:
                    yield result

        return Stream(gen())

    def window(self, size: int, step: int = 1) -> Stream:
        """Sliding window of fixed size."""
        parent = self
        buffer = deque(maxlen=size)

        def gen():
            count = 0
            for item in parent:
                buffer.append(item)
                if len(buffer) == size:
                    count += 1
                    if (count - 1) % step == 0:
                        yield list(buffer)

        return Stream(gen())

    def batch(self, size: int) -> Stream:
        """Group elements into batches."""
        parent = self

        def gen():
            batch = []
            for item in parent:
                batch.append(item)
                if len(batch) == size:
                    yield batch
                    batch = []
            if batch:
                yield batch

        return Stream(gen())

    def take(self, n: int) -> Stream:
        """Take first n elements."""
        parent = self

        def gen():
            count = 0
            for item in parent:
                if count >= n:
                    break
                yield item
                count += 1

        return Stream(gen())

    def skip(self, n: int) -> Stream:
        """Skip first n elements."""
        parent = self

        def gen():
            count = 0
            for item in parent:
                count += 1
                if count > n:
                    yield item

        return Stream(gen())

    def enumerate(self) -> Stream:
        """Add index to each element."""
        parent = self

        def gen():
            for i, item in builtins_enumerate(parent):
                yield (i, item)

        return Stream(gen())

    def tap(self, fn: Callable) -> Stream:
        """Side effect on each element (for logging/monitoring)."""
        parent = self

        def gen():
            for item in parent:
                fn(item)
                yield item

        return Stream(gen())

    def throttle(self, max_per_second: float) -> Stream:
        """Limit throughput."""
        parent = self
        interval = 1.0 / max_per_second if max_per_second > 0 else 0

        def gen():
            last_time = 0.0
            for item in parent:
                now = time.time()
                elapsed = now - last_time
                if elapsed < interval:
                    time.sleep(interval - elapsed)
                last_time = time.time()
                yield item

        return Stream(gen())

    def deduplicate(self, key: Callable | None = None) -> Stream:
        """Remove consecutive duplicates."""
        parent = self

        def gen():
            last = object()
            for item in parent:
                k = key(item) if key else item
                if k != last:
                    last = k
                    yield item

        return Stream(gen())

    def scan(self, fn: Callable, initial: Any = 0) -> Stream:
        """Running accumulator (like reduce but emits intermediate results)."""
        parent = self

        def gen():
            acc = initial
            for item in parent:
                acc = fn(acc, item)
                yield acc

        return Stream(gen())

    # ------------------------------------------------------------------
    # Terminal operations (eager)
    # ------------------------------------------------------------------

    def collect(self) -> list:
        """Consume the stream into a list."""
        return list(self)

    def reduce(self, fn: Callable, initial: Any = None) -> Any:
        """Reduce stream to a single value."""
        acc = initial
        first = True
        for item in self:
            if first and acc is None:
                acc = item
                first = False
            else:
                acc = fn(acc, item)
                first = False
        return acc

    def count(self) -> int:
        """Count elements."""
        n = 0
        for _ in self:
            n += 1
        return n

    def first(self) -> Any:
        """Get the first element."""
        for item in self:
            return item
        return None

    def last(self) -> Any:
        """Get the last element."""
        result = None
        for item in self:
            result = item
        return result

    def for_each(self, fn: Callable) -> None:
        """Consume stream, calling fn on each element."""
        for item in self:
            fn(item)

    def any(self, predicate: Callable) -> bool:
        for item in self:
            if predicate(item):
                return True
        return False

    def all(self, predicate: Callable) -> bool:
        for item in self:
            if not predicate(item):
                return False
        return True

    def sum(self) -> Any:
        return self.reduce(lambda a, b: a + b, 0)

    def min(self) -> Any:
        return self.reduce(lambda a, b: a if a < b else b)

    def max(self) -> Any:
        return self.reduce(lambda a, b: a if a > b else b)

    # ------------------------------------------------------------------
    # Merge / combine
    # ------------------------------------------------------------------

    @staticmethod
    def merge(*streams: Stream) -> Stream:
        """Interleave multiple streams."""
        def gen():
            iterators = [iter(s) for s in streams]
            while iterators:
                exhausted = []
                for i, it in builtins_enumerate(iterators):
                    try:
                        yield next(it)
                    except StopIteration:
                        exhausted.append(i)
                for i in reversed(exhausted):
                    iterators.pop(i)

        return Stream(gen())

    @staticmethod
    def zip(*streams: Stream) -> Stream:
        """Zip multiple streams together."""
        def gen():
            iterators = [iter(s) for s in streams]
            while True:
                try:
                    values = [next(it) for it in iterators]
                    yield tuple(values)
                except StopIteration:
                    break

        return Stream(gen())

    @staticmethod
    def concat(*streams: Stream) -> Stream:
        """Concatenate streams end-to-end."""
        def gen():
            for s in streams:
                yield from s

        return Stream(gen())

    # ------------------------------------------------------------------
    # Iterator protocol
    # ------------------------------------------------------------------

    def __iter__(self) -> Iterator:
        return self._source

    def __next__(self) -> Any:
        return next(self._source)


# Avoid shadowing builtin enumerate
builtins_enumerate = enumerate
