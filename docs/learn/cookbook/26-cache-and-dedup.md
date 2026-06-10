# Cookbook 26 · Caches and dedup

When you have a hot recompute loop, a small LRU cache pays for itself in
microseconds. When you need to dedup billions of records cheaply, a
bloom filter is the right shape.

## LRU cache

```kryos
use std::lru::{new_cache, put, get}

fn expensive(key: i64) -> i64 {
    // Pretend this takes 100ms.
    return key * key + 7
}

fn main() {
    let cache = new_cache(8)

    let queries: [i64] = [3, 5, 3, 7, 5, 3, 9, 11, 3]
    for q in queries {
        let cached = get(cache, q)
        if cached != -1 {
            println("cache hit: " + to_string(q) + " = " + to_string(cached))
        } else {
            let v = expensive(q)
            put(cache, q, v)
            println("computed:  " + to_string(q) + " = " + to_string(v))
        }
    }
}
```

## Bloom filter for billion-scale dedup

```kryos
use std::bloom::{new_filter, add, contains}

fn main() {
    // 8192-bit filter — tune the bit count for your expected item count / FPR.
    let bits: i64 = 8192
    let filter = new_filter(bits)

    let items = ["alice", "bob", "carol"]
    for item in items {
        add(filter, bits, item)
    }

    println(to_string(contains(filter, bits, "alice")))  // true
    println(to_string(contains(filter, bits, "dave")))   // false (probably)
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
