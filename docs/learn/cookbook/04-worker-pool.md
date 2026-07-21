# Cookbook 04 · Worker pool

Compute something CPU-bound across N workers using `spawn` + channels. The "go-routine pool" pattern.

## The program

Compute prime-checks for a list of integers, distributed across 4 workers.

Save as `pool.kry`:

```kryos
fn is_prime(n: i64) -> bool {
    if n < 2 { return false }
    if n < 4 { return true }
    if n % 2 == 0 { return false }
    let mut i = 3
    while i * i <= n {
        if n % i == 0 { return false }
        i = i + 2
    }
    true
}

fn worker(id: i64, jobs: i64, results: i64) {
    loop {
        let n = recv(jobs)
        if n < 0 { return }                 // -1 = shutdown signal
        let ok = is_prime(n)
        send(results, n * 2 + (if ok { 1 } else { 0 }))   // pack n + result
    }
}

fn main() {
    let jobs = chan()
    let results = chan()

    // Start 4 workers.
    let workers = 4
    for id in 0..workers {
        spawn { worker(id, jobs, results) }
    }

    // Send work.
    let inputs = [97, 100, 7919, 8000, 7907, 9999]
    let n = len(inputs)
    for x in inputs {
        send(jobs, x)
    }

    // Send shutdown signals.
    for _ in 0..workers {
        send(jobs, -1)
    }

    // Collect results.
    let mut received = 0
    while received < n {
        let packed = recv(results)
        let value = packed / 2
        let is_p = packed % 2 == 1
        println(to_string(value) + (if is_p { " is prime" } else { " is composite" }))
        received = received + 1
    }
}
```

## Run it

```bash
kryos run pool.kry
# → 97 is prime
# → 100 is composite
# → 7919 is prime
# → 8000 is composite
# → 7907 is prime
# → 9999 is composite
# (order may vary — that's the point)
```

## What this teaches

- **A pool is just N tasks sharing a job channel.** No special API needed.
- **Shutdown signals** are values (here, `-1`). For multi-type channels, use an enum.
- **Order is not preserved** when workers run in parallel. If you need order, tag each job with an id.

## Variations to try

- Use a `struct Job { id, payload }` and `struct Result { id, value }` to preserve ordering.
- Make `workers` configurable from `args()` (the argv array; ungated, no capability needed).
- Time the parallel version vs a single-threaded version with `time_now_millis()`.

When you're ready for more, see [05 · Async fetch many](./05-async-fetch.md).
