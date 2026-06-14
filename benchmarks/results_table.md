| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|
| fib | 0.349s | 4.972s | 0.346s | 0.344s | 0.700s | 17.010s | 1.01x |
| mandelbrot | 0.377s | 0.417s | 0.364s | 0.363s | 0.387s | 19.662s | 1.04x |
| nbody | 0.208s | 0.927s | 0.107s | 0.150s | 0.244s | 46.983s | 1.94x |
| binary_trees | 4.954s | 3.781s | 0.762s | 0.702s | 0.498s | 1.804s | 6.50x |
| fannkuch | 0.219s | 0.904s | 0.199s | 0.200s | 0.205s | 5.937s | 1.10x |
| matmul | 0.612s | 0.656s | 0.643s | 0.653s | 0.567s | 34.953s | 0.95x |
