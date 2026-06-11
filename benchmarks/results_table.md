| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|
| fib | 1.043s | 4.861s | 0.344s | 0.342s | 0.699s | 16.181s | 3.03x |
| mandelbrot | 0.365s | 0.409s | 0.364s | 0.359s | 0.371s | 19.436s | 1.00x |
| nbody | 1.852s | 1.865s | 0.110s | 0.151s | 0.250s | 45.134s | 16.88x |
| binary_trees | 9.030s | 5.201s | 0.778s | 0.699s | 0.494s | 1.876s | 11.61x |
| fannkuch | 0.357s | 0.373s | 0.027s | — | 0.014s | 0.588s | 13.24x |
| matmul | 1.205s | 1.244s | 0.644s | 0.641s | 0.563s | 34.476s | 1.87x |
