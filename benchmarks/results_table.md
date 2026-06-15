| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|
| fib | 0.351s | 5.041s | 0.348s | 0.343s | 0.709s | 17.096s | 1.01x |
| mandelbrot | 0.370s | 0.414s | 0.370s | 0.369s | 0.377s | 19.903s | 1.00x |
| nbody | 0.142s | 0.941s | 0.106s | 0.148s | 0.245s | 48.219s | 1.33x |
| binary_trees | 4.675s | 3.640s | 0.763s | 0.702s | 0.487s | 1.806s | 6.13x |
| fannkuch | 0.198s | 0.933s | 0.195s | 0.187s | 0.199s | 6.011s | 1.01x |
| matmul | 0.618s | 0.648s | 0.657s | 0.654s | 0.566s | 35.253s | 0.94x |
| hashmap | 0.084s | 0.086s | 0.123s | n/a | 0.129s | 0.217s | 0.68x |
