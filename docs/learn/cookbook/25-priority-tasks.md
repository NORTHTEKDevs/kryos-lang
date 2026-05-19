# Cookbook 25 · Priority queues + scheduling

`std::heap` is a binary min-heap — exactly what you need for Dijkstra,
A*, or scheduling N tasks with priorities.

## Recipe

```kryos
use std::heap::{heap_init, heap_push, heap_pop_min, heap_peek_min}

@capabilities(io)
fn main() {
    // Schedule 10 tasks with priorities; run in priority order.
    let mut buf: [i64] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    let mut state: [i64] = [0, 0]
    heap_init(state, 10)

    // Push (priority, task_id) pairs encoded as a single i64:
    //   high 32 bits = priority, low 32 bits = task_id.
    // This way heap_pop_min returns lowest priority + tied task_id ascending.
    let priorities: [i64] = [5, 2, 8, 1, 9, 4, 3, 7, 6, 10]
    let mut i: i64 = 0
    while i < 10 {
        let encoded = (priorities[i] * 1000000) + i  // simple multiplex
        heap_push(buf, state, encoded)
        i = i + 1
    }

    println("Tasks in priority order:")
    let mut out: i64 = 0
    while heap_pop_min(buf, state, out) == 1 {
        let pri = out / 1000000
        let tid = out - pri * 1000000
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
