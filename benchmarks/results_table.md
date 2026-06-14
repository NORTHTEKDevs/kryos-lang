| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|
| fib | 1.058s | 4.976s | 0.348s | 0.352s | 0.717s | 16.955s | 3.04x |
| mandelbrot | 0.375s | 0.424s | 0.375s | 0.389s | 0.413s | 19.523s | 1.00x |
| nbody | 0.921s | 0.951s | 0.112s | 0.150s | 0.252s | 45.529s | 8.26x |
| binary_trees | 4.974s | 3.714s | 0.783s | 0.736s | 0.514s | 1.844s | 6.35x |
| fannkuch | 0.872s | 0.909s | 0.199s | 0.199s | 0.202s | 6.207s | 4.38x |
| matmul | 0.568s | 0.682s | 0.664s | 0.661s | 0.568s | 35.184s | 0.85x |
