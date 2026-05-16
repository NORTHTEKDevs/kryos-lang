# Benchmark: binary trees — mirrors c/binary_trees.c exactly.
# WARNING: depth=18 is extremely slow in CPython (~100s). Runner uses timeout.
import sys
sys.setrecursionlimit(600000)

def tree_sum(depth, value):
    if depth == 0:
        return value
    return tree_sum(depth - 1, value * 2) + tree_sum(depth - 1, value * 2 + 1)

s = tree_sum(18, 1)
print(f"binary_trees(depth=18) = {s}")
