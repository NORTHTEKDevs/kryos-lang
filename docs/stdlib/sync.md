# std::sync

Low-level synchronization primitives for concurrent Kryos programs: mutexes, atomic integers, atomic booleans, wait groups, one-time initialization, and spin locks. All primitives are backed by native platform types and must be explicitly dropped when no longer needed.

```kryos
use std::sync
```

---

## Mutex

A mutual-exclusion lock. Only one thread may hold the lock at a time.

### mutex_new

`mutex_new() -> Mutex`

Create a new, unlocked `Mutex`.

| Field     | Type   | Description                      |
|-----------|--------|----------------------------------|
| `handle`  | `ptr`  | Native mutex handle              |
| `locked`  | `bool` | Whether the mutex is held        |
| `dropped` | `bool` | Whether the mutex has been freed |

---

### lock

`lock() -> Mutex`

Acquire the lock. Blocks until the lock is available. Returns `self` for chaining.

---

### unlock

`unlock() -> Mutex`

Release the lock.

---

### with_lock

`with_lock(callback: fn() -> str) -> str`

Acquire the lock, call `callback`, release the lock, and return the result. The lock is always released even if `callback` throws. Prefer this over manual `lock`/`unlock`.

**Example:**
```kryos
use std::sync

let mu = mutex_new()

let result = mu.with_lock(fn() -> str {
    // critical section
    return "done"
})
println(result)   // "done"

mu.drop()
```

---

### is_locked

`is_locked() -> bool`

Return `true` if the mutex is currently held.

---

### drop

`drop()`

Release native resources. Must be called when the mutex is no longer needed.

---

## AtomicInt

A thread-safe `i64` counter with lock-free operations.

### atomic_int

`atomic_int(initial: i64) -> AtomicInt`

Create a new `AtomicInt` with the given initial value.

---

### load

`load() -> i64`

Return the current value.

---

### store

`store(v: i64)`

Set the value to `v`.

---

### fetch_add

`fetch_add(delta: i64) -> i64`

Add `delta` and return the value *before* the addition.

---

### fetch_sub

`fetch_sub(delta: i64) -> i64`

Subtract `delta` and return the value *before* the subtraction.

---

### increment

`increment()`

Increment by 1.

---

### decrement

`decrement()`

Decrement by 1.

---

### compare_and_swap

`compare_and_swap(expected: i64, new_value: i64) -> bool`

If the current value equals `expected`, set it to `new_value` and return `true`. Otherwise return `false`.

**Example:**
```kryos
use std::sync

let counter = atomic_int(0)
counter.increment()
counter.increment()
println(counter.load())   // 2

let old = counter.fetch_add(10)
println(old)              // 2
println(counter.load())   // 12

counter.drop()
```

---

### drop

`drop()`

Release native resources.

---

## AtomicBool

A thread-safe boolean flag.

### atomic_bool

`atomic_bool(initial: bool) -> AtomicBool`

Create a new `AtomicBool` with the given initial value.

---

### load

`load() -> bool`

Return the current value.

---

### store

`store(v: bool)`

Set the value to `v`.

---

### toggle

`toggle() -> bool`

Flip the value and return the *new* value.

**Example:**
```kryos
use std::sync

let flag = atomic_bool(false)
println(flag.toggle())   // true
println(flag.toggle())   // false
flag.drop()
```

---

### drop

`drop()`

Release native resources.

---

## WaitGroup

Coordinate a group of concurrent tasks by waiting until all have finished.

### wait_group

`wait_group() -> WaitGroup`

Create a new `WaitGroup` with an internal counter of zero.

---

### add

`add(n: i64)`

Increment the counter by `n`. Call this before starting each task.

---

### done

`done()`

Decrement the counter by 1. Call this when a task finishes.

---

### wait

`wait() -> bool`

Block until the counter reaches zero. Returns `true` when all tasks have completed.

---

### is_done

`is_done() -> bool`

Return `true` if the counter is zero.

---

### pending

`pending() -> i64`

Return the current counter value.

**Example:**
```kryos
use std::sync

let wg = wait_group()
wg.add(3)

// ... launch 3 tasks, each calling wg.done() when finished ...

wg.wait()
println("all tasks done")
wg.drop()
```

---

### drop

`drop()`

Release native resources.

---

## Once

Execute an initialization function exactly once, even if called from multiple goroutines.

### once

`once() -> Once`

Create a new `Once` guard.

---

### call_once

`call_once(callback: fn() -> str) -> bool`

Call `callback` the first time this method is invoked. Subsequent calls are no-ops. Returns `true` if this invocation executed `callback`.

---

### is_done

`is_done() -> bool`

Return `true` if `call_once` has already been executed.

**Example:**
```kryos
use std::sync

let init = once()

init.call_once(fn() -> str {
    println("initialized")
    return ""
})

init.call_once(fn() -> str {
    println("this won't run")
    return ""
})

println(init.is_done())   // true
init.drop()
```

---

### drop

`drop()`

Release native resources.

---

## SpinLock

A busy-waiting lock. More efficient than `Mutex` for very short critical sections; wasteful for longer ones.

### spin_lock

`spin_lock() -> SpinLock`

Create a new, unlocked `SpinLock`.

---

### lock

`lock()`

Spin until the lock is acquired.

---

### unlock

`unlock()`

Release the lock.

---

### with_lock

`with_lock(callback: fn() -> str) -> str`

Acquire the lock, call `callback`, release the lock, and return the result.

**Example:**
```kryos
use std::sync

let sl = spin_lock()
let result = sl.with_lock(fn() -> str {
    return "protected"
})
println(result)
sl.drop()
```

---

### drop

`drop()`

Release native resources.

---

## Complete Example

```kryos
use std::sync

// Mutex-protected counter
let mu = mutex_new()
let shared_count = atomic_int(0)

mu.with_lock(fn() -> str {
    shared_count.increment()
    return ""
})

println(shared_count.load())   // 1

// One-time setup
let setup = once()
setup.call_once(fn() -> str {
    println("database initialized")
    return ""
})

// Wait group for tasks
let wg = wait_group()
wg.add(2)
// ... launch tasks ...
// each task calls wg.done() when finished
wg.wait()
println("all done")

// Cleanup
mu.drop()
shared_count.drop()
setup.drop()
wg.drop()
```
