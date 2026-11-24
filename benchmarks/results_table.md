| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| fib | 0.359s | 5.196s | 0.355s | 0.356s | 0.350s | n/a | 0.719s | 17.034s | 1.01x |
| mandelbrot | 0.376s | 0.429s | 0.377s | 0.378s | 0.374s | n/a | 0.383s | 20.224s | 1.00x |
| nbody | 0.145s | 0.922s | 0.111s | 0.151s | 0.151s | n/a | 0.249s | 47.871s | 1.31x |
| binary_trees | 1.513s | 3.057s | 0.780s | 0.707s | 0.763s | n/a | 0.487s | 2.147s | 1.94x |
| fannkuch | 0.202s | 0.931s | 0.200s | 0.190s | 0.187s | n/a | 0.200s | 6.340s | 1.01x |
| matmul | 0.621s | 0.648s | 0.653s | 0.649s | 0.648s | n/a | 0.581s | 33.428s | 0.95x |
| hashmap | 0.080s | 0.085s | 0.128s | n/a | 0.363s | n/a | 0.137s | 0.494s | 0.63x |
