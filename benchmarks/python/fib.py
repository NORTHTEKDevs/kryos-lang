# Benchmark: recursive Fibonacci — mirrors c/fib.c exactly.
import sys
sys.setrecursionlimit(100000)

def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

r = fib(35)
print(f"fib(35) = {r}")
