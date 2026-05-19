# Cookbook 22 · Random numbers

`std::random` ships a splitmix64-based PRNG with seeded determinism, range
selection, float [0, 1), byte fill, and in-place Fisher–Yates shuffle.
**Not cryptographic** — use `std::crypto::rand_bytes` for that.

## The program

```kryos
use std::random::{random_seed, random_i64, random_range, random_f64, random_shuffle_i64}

fn main() {
    // Deterministic for tests.
    random_seed(42)

    println("i64:   " + to_string(random_i64()))
    println("range: " + to_string(random_range(1, 7)))   // dice roll
    println("float: " + to_string(random_f64()))         // [0, 1)

    // Shuffle a deck.
    let mut deck: [i64] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    random_shuffle_i64(deck)
    print("shuffled: ")
    let mut s: str = ""
    let mut i: i64 = 0
    while i < len(deck) {
        if i > 0 { s = s + " " }
        s = s + to_string(deck[i])
        i = i + 1
    }
    println(s)
}
```

## API

| Function | Returns | Notes |
| --- | --- | --- |
| `random_seed(n)` | void | seed=0 reverts to time-based; nonzero is deterministic |
| `random_i64()` | i64 | full range |
| `random_range(min, max)` | i64 | uniform in `[min, max)` |
| `random_f64()` | f64 | uniform in `[0.0, 1.0)` |
| `random_bytes(buf, len)` | void | fills with random bytes |
| `random_shuffle_i64(arr)` | void | in-place Fisher–Yates |

## When NOT to use this

For **anything cryptographic** (auth tokens, password salts, signing nonces),
use `std::crypto::rand_bytes` instead. The splitmix64 generator is fast +
deterministic but small-state and predictable from a few outputs.
