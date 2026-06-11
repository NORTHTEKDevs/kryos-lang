# Canonical allocation-stress binary trees (same algorithm as kryos port).
import sys
sys.setrecursionlimit(100000)

def make(depth):
    if depth == 0:
        return (None, None)
    return (make(depth - 1), make(depth - 1))

def check(t):
    if t[0] is None:
        return 1
    return 1 + check(t[0]) + check(t[1])

max_depth = 16
long_lived = make(max_depth)
total = 0
depth = 4
while depth <= max_depth:
    iterations = 1 << (max_depth - depth + 4)
    total += sum(check(make(depth)) for _ in range(iterations))
    depth += 2
total += check(long_lived)
print(f"binary_trees(canonical, depth=16) checksum = {total}")
