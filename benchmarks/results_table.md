| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| fib | 0.362s | 4.955s | 0.358s | 0.356s | 0.357s | n/a | 0.722s | 17.758s | 1.01x |
| mandelbrot | 0.383s | 0.431s | 0.381s | 0.381s | 0.373s | n/a | 0.391s | 20.466s | 1.00x |
| nbody | 0.150s | 0.969s | 0.112s | 0.154s | 0.158s | n/a | 0.258s | 46.104s | 1.34x |
| binary_trees | 1.455s | 3.291s | 0.772s | 0.691s | 0.732s | n/a | 0.492s | 2.389s | 1.88x |
| fannkuch | 0.204s | 0.918s | 0.196s | 0.192s | 0.187s | n/a | 0.197s | 6.542s | 1.04x |
| matmul | 0.620s | 0.614s | 0.645s | 0.645s | 0.643s | n/a | 0.561s | 34.818s | 0.96x |
| hashmap | 0.086s | 0.091s | 0.147s | n/a | 0.487s | n/a | 0.159s | 0.671s | 0.59x |
