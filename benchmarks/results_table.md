| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| fib | 0.349s | 4.966s | 0.347s | 0.347s | 0.346s | n/a | 0.705s | 16.896s | 1.01x |
| mandelbrot | 0.368s | 0.415s | 0.368s | 0.366s | 0.366s | n/a | 0.375s | 19.918s | 1.00x |
| nbody | 0.141s | 0.934s | 0.105s | 0.146s | 0.146s | n/a | 0.243s | 45.688s | 1.34x |
| binary_trees | 1.098s | 3.519s | 0.759s | 0.700s | 0.724s | n/a | 0.488s | 1.818s | 1.45x |
| fannkuch | 0.197s | 0.920s | 0.195s | 0.188s | 0.187s | n/a | 0.195s | 6.065s | 1.01x |
| matmul | 0.618s | 0.660s | 0.653s | 0.651s | 0.646s | n/a | 0.565s | 32.984s | 0.95x |
| hashmap | 0.080s | 0.084s | 0.118s | n/a | 0.339s | n/a | 0.109s | 0.219s | 0.68x |
