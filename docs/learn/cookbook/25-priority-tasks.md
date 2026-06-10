# Cookbook 25 · Priority queues + scheduling

`std::heap` is a binary min-heap — exactly what you need for Dijkstra,
A*, or scheduling N tasks with priorities.

## Recipe

```kryos
use std::heap::{push_min, pop_min, peek_min}

@capabilities(io)
fn main() {
    // Schedule 10 tasks with priorities; run in priority order.
    // Encode (priority, task_id) in one i64: priority * 1_000_000 + task_id
    let priorities: [i64] = [5, 2, 8, 1, 9, 4, 3, 7, 6, 10]
    let mut h: [i64] = []
    let mut i: i64 = 0
    while i < 10 {
        h = push_min(h, priorities[i] * 1000000 + i)
        i = i + 1
    }

    println("Tasks in priority order:")
    while len(h) > 0 {
        let encoded = peek_min(h)
        h = pop_min(h)
        let pri = encoded / 1000000
        let tid = encoded - pri * 1000000
        println("  task #" + to_string(tid) + " (priority " + to_string(pri) + ")")
    }
}
```

## Tricks

- Encode `(priority, secondary)` in a single i64 by multiplexing: high
  bits = priority, low bits = task id. Lexicographic order on i64 gives
  you correct multi-key ordering.
- For a max-heap, negate the value on push and again on pop.
- Combine with `std::ratelimit` for "process N items per second in
  priority order" workloads.
- For huge N, switch to a binary heap on heap memory — the current
  caller-owned variant caps at the buffer size.
