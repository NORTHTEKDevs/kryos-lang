# Kryos Benchmark Suite

Micro-benchmarks for measuring Kryos compiler and runtime performance across
different workload categories.

## Benchmarks

| File | Category | Description | Expected Result |
|---|---|---|---|
| `fibonacci.kry` | Recursion | Recursive `fibonacci(35)` | 9227465 |
| `sum_loop.kry` | Tight loop | Sum integers 1 to 100,000,000 | 5000000050000000 |
| `string_concat.kry` | Allocation | Concatenate "x" 50,000 times | length 50000 |
| `struct_alloc.kry` | Allocation | Create 100,000 structs with string fields | count 100000 |
| `nested_loops.kry` | Loop overhead | Triple-nested 500x500x500 loop | 125000000 iterations |

## Running

Always use `--release` for meaningful benchmark numbers. Debug builds include
extra checks and unoptimized codegen that skew results.

```
cargo run --release -j 4 -- run benchmarks/fibonacci.kry
cargo run --release -j 4 -- run benchmarks/sum_loop.kry
cargo run --release -j 4 -- run benchmarks/string_concat.kry
cargo run --release -j 4 -- run benchmarks/struct_alloc.kry
cargo run --release -j 4 -- run benchmarks/nested_loops.kry
```

## Interpreting Results

Each benchmark prints its result and wall-clock time in seconds via `time_now()`.

- **Fibonacci**: Pure compute. Measures function-call overhead and stack
  performance. Comparing against native C/Rust recursive fib(35) gives a
  rough "overhead factor" for the Kryos runtime.
- **Sum loop**: Tight integer arithmetic in a single loop. Tests how well the
  compiler optimizes simple loops. A compiled language should complete this in
  well under 1 second.
- **String concat**: Allocation-heavy. Each iteration allocates a new string
  one byte longer. Quadratic by nature (O(n^2) total bytes copied). This
  stresses the allocator and drop/cleanup path.
- **Struct alloc**: Creates structs with heap-allocated string fields and
  immediately drops them. Measures allocation + deallocation throughput.
- **Nested loops**: Pure loop overhead with no allocation. The inner body is a
  single integer increment. Reveals per-iteration cost of the while-loop
  construct.

## Baseline Targets

These are rough targets for a compiled language (not an interpreter):

| Benchmark | Good | Acceptable | Investigate |
|---|---|---|---|
| fibonacci(35) | < 1s | < 3s | > 5s |
| sum 100M | < 1s | < 2s | > 5s |
| string concat 50k | < 2s | < 5s | > 10s |
| struct alloc 100k | < 1s | < 3s | > 5s |
| nested loops 125M | < 2s | < 5s | > 10s |
