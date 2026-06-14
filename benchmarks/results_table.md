| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|
| fib | 1.027s | 4.894s | 0.349s | 0.345s | 0.714s | 16.807s | 2.94x |
| mandelbrot | 0.366s | 0.419s | 0.368s | 0.368s | 0.385s | 19.889s | 1.00x |
| nbody | 0.205s | 0.951s | 0.112s | 0.147s | 0.245s | 46.772s | 1.83x |
| binary_trees | 4.933s | 3.606s | 0.785s | 0.725s | 0.493s | 1.788s | 6.29x |
| fannkuch | 0.221s | 0.930s | 0.202s | 0.196s | 0.199s | 5.938s | 1.09x |
| matmul | 0.622s | 0.650s | 0.668s | 0.657s | 0.565s | 34.089s | 0.93x |
