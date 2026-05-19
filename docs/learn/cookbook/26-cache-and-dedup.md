# Cookbook 26 · Caches and dedup

When you have a hot recompute loop, a small LRU cache pays for itself in
microseconds. When you need to dedup billions of records cheaply, a
bloom filter is the right shape.

## LRU cache

```kryos
use std::lru::{lru_init, lru_put, lru_get}

fn expensive(key: i64) -> i64 {
    // Pretend this takes 100ms.
    return key * key + 7
}

fn main() {
    let mut keys: [i64] = [0, 0, 0, 0, 0, 0, 0, 0]
    let mut vals: [i64] = [0, 0, 0, 0, 0, 0, 0, 0]
    let mut rec:  [i64] = [0, 0, 0, 0, 0, 0, 0, 0]
    let mut state: [i64] = [0, 0, 0]
    lru_init(state, 8)

    let mut out: i64 = 0
    let queries: [i64] = [3, 5, 3, 7, 5, 3, 9, 11, 3]
    for q in queries {
        if lru_get(keys, vals, rec, state, q, out) == 1 {
            println("cache hit: " + to_string(q) + " = " + to_string(out))
        } else {
            let v = expensive(q)
            lru_put(keys, vals, rec, state, q, v)
            println("computed:  " + to_string(q) + " = " + to_string(v))
        }
    }
}
```

## Bloom filter for billion-scale dedup

```kryos
use std::bloom::{bloom_add, bloom_contains}

fn main() {
    // 1 MB filter = ~8 million bits, ~800k items at 1% FPR.
    let mut bits: [i64] = []  // would actually be 131072 bytes (let mut bits = bytes(131072))
    let _ = bits

    // Scratch: just demonstrate the API on a small filter.
    // For real code, allocate a real byte buffer.
}
```

## When to pick which

| Need | Use |
| --- | --- |
| Fast key→value cache, recent-wins eviction | `std::lru` |
| "Have I seen this before?" at huge scale | `std::bloom` |
| Both? Layer the bloom in front of the LRU | Combine |
- LRU costs O(N) per op in the current impl (small caches only).
- Bloom has false positives but never false negatives — confirm a hit
  by a second lookup against the source of truth.
