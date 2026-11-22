| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| fib | 0.357s | 4.939s | 0.348s | 0.356s | 0.371s | n/a | 0.720s | 17.145s | 1.02x |
| mandelbrot | 0.385s | 0.435s | 0.373s | 0.380s | 0.427s | n/a | 0.495s | 20.121s | 1.03x |
| nbody | 0.152s | 0.943s | 0.109s | 0.150s | 0.150s | n/a | 0.309s | 44.592s | 1.39x |
| binary_trees | 1.463s | 3.218s | 0.795s | 0.746s | 0.769s | n/a | 0.490s | 2.333s | 1.84x |
| fannkuch | 0.194s | 0.930s | 0.200s | 0.217s | 0.192s | n/a | 0.198s | 6.406s | 0.97x |
| matmul | 0.622s | 0.619s | 0.697s | 0.653s | 0.653s | n/a | 0.592s | 34.344s | 0.89x |
| hashmap | 0.084s | 0.091s | 0.144s | n/a | 0.585s | n/a | 0.228s | 0.548s | 0.58x |
