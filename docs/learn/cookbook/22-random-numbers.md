# Cookbook 22 · Random numbers

`std::random` ships a splitmix64-based PRNG with seeded determinism, range
selection, float [0, 1), byte fill, and in-place Fisher–Yates shuffle.
**Not cryptographic** — use `std::crypto::rand_bytes` for that.

## The program

```kryos
use std::random::{new_rng, next_i64, range_i64, shuffle}

fn main() {
    // Deterministic for tests — seed 42.
    let rng = new_rng(42)

    println("i64:   " + to_string(next_i64(rng)))
    println("range: " + to_string(range_i64(rng, 1, 7)))  // dice roll [1,7)
    // float [0,1): derive from next_i64 by masking mantissa bits
    let bits = next_i64(rng)
    let f = (bits & 0x000fffffffffffff) as f64 / 4503599627370496.0
    println("float: " + to_string(f))

    // Shuffle a deck.
    let mut deck: [i64] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    deck = shuffle(rng, deck)
    let mut s: str = ""
    let mut i: i64 = 0
    while i < len(deck) {
        if i > 0 { s = s + " " }
        s = s + to_string(deck[i])
        i = i + 1
    }
    println("shuffled: " + s)
}
```

## API

| Function | Returns | Notes |
| --- | --- | --- |
| `new_rng(seed)` | `[i64]` | seed=0 = time-based; nonzero = deterministic |
| `next_i64(state)` | `i64` | full range; mutates state |
| `range_i64(state, lo, hi)` | `i64` | uniform in `[lo, hi)` |
| `next_bit(state)` | `i64` | returns 0 or 1 |
| `shuffle(state, arr)` | `[i64]` | Fisher-Yates; returns new shuffled array |

## When NOT to use this

For **anything cryptographic** (auth tokens, password salts, signing nonces),
use `std::crypto::rand_bytes` instead. The splitmix64 generator is fast +
deterministic but small-state and predictable from a few outputs.
