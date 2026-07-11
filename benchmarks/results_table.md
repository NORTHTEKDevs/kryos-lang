| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| fib | 0.351s | 4.832s | 0.340s | 0.342s | 0.340s | n/a | 0.706s | 16.899s | 1.03x |
| mandelbrot | 0.369s | 0.416s | 0.368s | 0.365s | 0.364s | n/a | 0.376s | 20.373s | 1.00x |
| nbody | 0.141s | 0.938s | 0.107s | 0.149s | 0.149s | n/a | 0.246s | 46.264s | 1.31x |
| binary_trees | 1.097s | 3.621s | 0.773s | 0.716s | 0.733s | n/a | 0.491s | 2.244s | 1.42x |
| fannkuch | 0.202s | 0.900s | 0.198s | 0.189s | 0.189s | n/a | 0.198s | 6.349s | 1.02x |
| matmul | 0.620s | 0.650s | 0.648s | 0.659s | 0.645s | n/a | 0.564s | 34.808s | 0.96x |
| hashmap | 0.082s | 0.086s | 0.127s | n/a | 0.370s | n/a | 0.128s | 0.535s | 0.65x |
