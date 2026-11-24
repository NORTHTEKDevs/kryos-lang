| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| fib | 0.357s | 5.031s | 0.356s | 0.357s | 0.354s | n/a | 0.717s | 17.350s | 1.00x |
| mandelbrot | 0.374s | 0.426s | 0.374s | 0.374s | 0.373s | n/a | 0.384s | 20.868s | 1.00x |
| nbody | 0.147s | 0.948s | 0.109s | 0.153s | 0.153s | n/a | 0.249s | 48.099s | 1.34x |
| binary_trees | 1.406s | 3.132s | 0.802s | 0.721s | 0.773s | n/a | 0.494s | 2.155s | 1.75x |
| fannkuch | 0.203s | 0.942s | 0.197s | 0.193s | 0.188s | n/a | 0.201s | 6.505s | 1.03x |
| matmul | 0.626s | 0.647s | 0.664s | 0.662s | 0.655s | n/a | 0.571s | 36.456s | 0.94x |
| hashmap | 0.081s | 0.084s | 0.126s | n/a | 0.334s | n/a | 0.138s | 0.490s | 0.64x |
