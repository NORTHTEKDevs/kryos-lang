| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|
| fib | 1.092s | 4.997s | 0.350s | 0.352s | 0.707s | 16.851s | 3.12x |
| mandelbrot | 0.377s | 0.422s | 0.376s | 0.369s | 0.385s | 19.766s | 1.00x |
| nbody | 0.302s | 0.947s | 0.108s | 0.150s | 0.252s | 47.869s | 2.79x |
| binary_trees | 4.844s | 3.582s | 0.756s | 0.686s | 0.500s | 1.792s | 6.41x |
| fannkuch | 0.479s | 0.912s | 0.204s | 0.191s | 0.200s | 6.008s | 2.34x |
| matmul | 0.617s | 0.658s | 0.660s | 0.641s | 0.565s | 33.365s | 0.93x |
