| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| matmul | 0.633s | 0.651s | 0.643s | 0.643s | 0.640s | n/a | 0.555s | 31.728s | 0.98x |
| nbody | 0.141s | 0.900s | 0.105s | 0.146s | 0.145s | n/a | 0.240s | 47.559s | 1.34x |
| binary_trees | 1.082s | 3.688s | 0.759s | 0.689s | 0.735s | n/a | 0.480s | 2.273s | 1.43x |
| fib | 0.338s | 4.821s | 0.343s | 0.343s | 0.336s | n/a | 0.696s | 16.820s | 0.99x |
