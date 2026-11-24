| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| fib | 0.358s | 5.160s | 0.349s | 0.356s | 0.355s | n/a | 0.720s | 17.521s | 1.03x |
| mandelbrot | 0.375s | 0.426s | 0.373s | 0.376s | 0.375s | n/a | 0.387s | 20.562s | 1.00x |
| nbody | 0.146s | 0.931s | 0.110s | 0.152s | 0.152s | n/a | 0.250s | 46.988s | 1.32x |
| binary_trees | 1.597s | 3.107s | 0.799s | 0.733s | 0.769s | n/a | 0.491s | 2.160s | 2.00x |
| fannkuch | 0.203s | 0.943s | 0.197s | 0.192s | 0.191s | n/a | 0.201s | 6.373s | 1.03x |
| matmul | 0.626s | 0.655s | 0.658s | 0.657s | 0.654s | n/a | 0.572s | 33.223s | 0.95x |
| hashmap | 0.086s | 0.085s | 0.124s | n/a | 0.342s | n/a | 0.119s | 0.496s | 0.70x |
