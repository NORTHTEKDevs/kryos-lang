| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|
| fib | 1.049s | 4.939s | 0.364s | 0.372s | 0.728s | 16.936s | 2.88x |
| mandelbrot | 0.390s | 0.422s | 0.380s | 0.375s | 0.419s | 20.222s | 1.03x |
| nbody | 1.200s | 1.214s | 0.112s | 0.150s | 0.252s | 47.859s | 10.72x |
| binary_trees | 5.532s | 3.753s | 0.785s | 0.751s | 0.513s | 1.887s | 7.05x |
| fannkuch | 0.977s | 1.050s | 0.204s | 0.193s | 0.205s | 6.048s | 4.79x |
| matmul | 0.825s | 0.909s | 0.660s | 0.651s | 0.571s | 37.326s | 1.25x |
