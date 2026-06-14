| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|
| fib | 0.349s | 4.947s | 0.348s | 0.349s | 0.706s | 16.986s | 1.00x |
| mandelbrot | 0.369s | 0.419s | 0.368s | 0.368s | 0.377s | 19.843s | 1.00x |
| nbody | 0.202s | 0.940s | 0.107s | 0.148s | 0.248s | 47.883s | 1.90x |
| binary_trees | 4.687s | 3.546s | 0.768s | 0.690s | 0.490s | 1.819s | 6.10x |
| fannkuch | 0.198s | 0.919s | 0.196s | 0.187s | 0.196s | 6.127s | 1.01x |
| matmul | 0.623s | 0.638s | 0.652s | 0.651s | 0.563s | 36.901s | 0.96x |
| hashmap | 0.081s | 0.086s | 0.117s | n/a | 0.122s | 0.223s | 0.69x |
