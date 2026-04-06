# Concurrency

> **Implementation Status:** `spawn` creates real OS threads via `kryos_spawn()` in the runtime. Channels are fully implemented as MPMC queues with blocking `recv` and non-blocking `try_recv`. Channel creation (`chan<T>`), send, and receive all work end-to-end through codegen. `select` polls channels with `try_recv` and branches to the first ready channel (busy-poll with 1ms yield). Actors are parsed and lower to mangled handler functions but the actor scheduler and mailbox runtime are not yet implemented -- use channels for production concurrency. `parallel for` is a reserved keyword but not yet implemented.

Kryos has two concurrency primitives: `spawn` for fire-and-forget parallel execution, and `actor` for stateful message-passing concurrency. Both are built into the language syntax -- no library imports, no async/await coloring, no callback chains.

## spawn

`spawn` runs a block of code in a new thread. The parent continues immediately without waiting.

```
println("before")

spawn {
    println("spawned")
}

sleep(0.2)
println("after")
```

Output:

```
before
spawned
after
```

The compiler inserts a `kryos_spawn_wait_all()` call at the end of `main`, so all spawned threads are joined before the process exits. The `sleep(0.2)` is for ordering -- it gives the spawned thread time to print before the main thread's `println("after")` runs. Without the sleep, the output order may vary because spawned threads run concurrently.

### What spawn does

1. Captures the current environment (variables in scope are visible to the spawned block)
2. Starts a new OS thread
3. Runs the block body in that thread
4. Returns immediately to the caller

The spawned block runs in its own child environment. It can read variables from the parent scope, but assignments inside the block create new local bindings -- they do not mutate the parent.

### Error handling in spawned blocks

If a spawned block throws an exception, the error is captured and printed as `[spawn error] ...`. It does not crash the parent thread.

```
spawn {
    let x = 1 / 0  // division by zero
}
// Parent continues running -- the error is logged, not propagated
```

A `return` statement inside a `spawn` block exits the spawned thread, not the enclosing function.

### Coordination with sleep

`sleep(seconds)` pauses the current thread. It is the simplest way to coordinate timing between spawned work and the main thread.

```
spawn {
    sleep(1.0)
    println("one second later")
}

spawn {
    sleep(0.5)
    println("half second later")
}

sleep(1.5)
println("done")
```

Output:

```
half second later
one second later
done
```

`sleep` takes a float -- `sleep(0.1)` is 100 milliseconds.

### Spawn for parallel computation

Use `spawn` when you have independent work that does not need to communicate results back. Good uses:

- Background logging or metrics
- Prefetching data while the main thread processes the current batch
- Running independent computations in parallel

```
@capabilities(compute)
fn heavy_compute(n: i32) -> i32 {
    let mut sum = 0
    for i in range(0, n) {
        sum = sum + i * i
    }
    return sum
}

spawn {
    let result = heavy_compute(100000)
    println("background: " + to_string(result))
}

// Main thread does its own work concurrently
let main_result = heavy_compute(50000)
println("main: " + to_string(main_result))

sleep(1.0)
```

## Actors

> **Note:** Actor handlers compile but the actor scheduler and mailbox runtime are not yet implemented. Use channels for production concurrency.

Actors are the structured concurrency model in Kryos. An actor is a self-contained unit with private state and message handlers. No one can read or write an actor's state directly -- the only way to interact is by sending messages through its handlers.

### Declaring an actor

```
actor Counter {
    let mut count: i32 = 0

    fn increment(amount: i32) {
        count = count + amount
    }

    fn get_count() -> i32 {
        return count
    }

    fn reset() {
        count = 0
    }
}
```

The `actor` keyword declares the type. Inside:
- `let` statements define the actor's private state
- `fn` declarations define message handlers

State fields use `let mut` for mutable state. Handlers can read and modify the actor's own state freely.

### Actor state isolation

Each actor instance owns its state. There is no shared mutable state between actors -- this eliminates data races by construction.

```
actor Logger {
    let mut entries: [str] = []

    fn log(message: str) {
        push(entries, message)
    }

    fn dump() -> [str] {
        return entries
    }
}
```

Two instances of `Logger` have completely separate `entries` arrays. One cannot corrupt the other.

### Why actors instead of locks

Traditional concurrent programming uses shared memory protected by locks. This is error-prone:
- Forget to lock? Data race.
- Lock in the wrong order? Deadlock.
- Hold a lock too long? Performance collapse.

Actors eliminate these problems. State is private, communication is through messages, and the runtime handles synchronization. You cannot create a data race because there is no shared mutable state to race on.

### Message passing patterns

#### Request-response

The simplest pattern: send a message, get a result.

```
actor Calculator {
    let mut memory: f64 = 0.0

    fn add(x: f64) -> f64 {
        memory = memory + x
        return memory
    }

    fn recall() -> f64 {
        return memory
    }

    fn clear() {
        memory = 0.0
    }
}
```

#### Event accumulation

An actor collects events over time, then reports:

```
actor MetricsCollector {
    let mut request_count: i32 = 0
    let mut error_count: i32 = 0
    let mut total_latency_ms: f64 = 0.0

    fn record_request(latency_ms: f64) {
        request_count = request_count + 1
        total_latency_ms = total_latency_ms + latency_ms
    }

    fn record_error() {
        error_count = error_count + 1
    }

    fn report() -> str {
        let avg = total_latency_ms / to_float(request_count)
        return "requests=" + to_string(request_count) +
               " errors=" + to_string(error_count) +
               " avg_ms=" + to_string(avg)
    }
}
```

#### State machine

Actors naturally model state machines. The internal state determines behavior:

```
actor Connection {
    let mut state: str = "disconnected"
    let mut retries: i32 = 0

    fn connect(host: str) -> str {
        if state == "connected" {
            return "already connected"
        }
        state = "connected"
        retries = 0
        return "connected to " + host
    }

    fn disconnect() {
        state = "disconnected"
    }

    fn get_state() -> str {
        return state
    }
}
```

## Channels

Channels are typed communication pipes between threads. Use `chan()` to create a channel, `send()` to push values, and `recv()` to pull values (blocking).

```
fn main() {
    let ch = chan()
    
    spawn {
        send(ch, 42)
        send(ch, 100)
    }
    
    let a = recv(ch)
    let b = recv(ch)
    println(to_string(a + b))  // 142
}
```

### How channels work

1. `chan()` creates a multi-producer, multi-consumer channel for i64 values
2. `send(ch, value)` pushes a value into the channel (non-blocking)
3. `recv(ch)` blocks until a value is available, then returns it

Channels are reference-counted. Multiple threads can hold the same channel handle. The channel stays alive as long as at least one handle exists.

### Select

The `select` statement waits on multiple channels simultaneously, running the first branch that receives a message:

```
select {
    msg from ch1 => {
        println("got from ch1: " + to_string(msg))
    }
    msg from ch2 => {
        println("got from ch2: " + to_string(msg))
    }
}
```

`select` blocks until one of the channels has data, then runs the matching branch. Only one branch runs per `select` evaluation.

### When to use channels vs actors

Use **channels** when you need to pass data between threads with explicit send/receive coordination. Channels are lower-level and more flexible.

Use **actors** when you have stateful components that respond to messages. Actors encapsulate state and guarantee no shared mutable data.

| Pattern | Use |
|---------|-----|
| Producer-consumer pipeline | Channels |
| Background worker with state | Actor |
| Fan-out / fan-in parallelism | Channels |
| Request-response service | Actor |
| Event stream processing | Either |

## Combining spawn and actors

Use `spawn` to run actor processing in the background:

```
actor TaskQueue {
    let mut tasks: [str] = []

    fn add(task: str) {
        push(tasks, task)
    }

    fn next() -> str {
        if len(tasks) == 0 {
            return "empty"
        }
        return pop(tasks)
    }
}

// Process tasks in the background
spawn {
    // Worker loop that processes tasks
    let mut running = true
    while running {
        sleep(0.1)
        // Process available tasks
    }
}
```

## Coming from JavaScript

| JavaScript | Kryos |
|------------|-------|
| `new Promise(...)` | `spawn { ... }` |
| `async/await` | Not needed |
| `setTimeout(fn, ms)` | `spawn { sleep(seconds) ... }` |
| `Worker` (Web Workers) | `spawn` + actors |
| Event emitters | Actor handlers |

The biggest win: no callback hell, no promise chains, no async/await viral coloring. `spawn` gives you parallelism without restructuring your entire call stack.

## Common Mistakes

**Forgetting sleep before exit.** Spawned blocks are daemon threads. If the main program exits, spawned work is killed immediately. Use `sleep()` to wait for spawned work to complete, or structure your program so the main thread naturally outlives the spawned work.

**Trying to return values from spawn.** `spawn` does not return the result of the block. If you need a result, use an actor with a handler that stores the result, then query the actor from the main thread.

**Assuming execution order.** Spawned blocks run concurrently. Their print statements may interleave with the main thread. Never rely on specific ordering between spawned blocks or between a spawned block and the main thread.
