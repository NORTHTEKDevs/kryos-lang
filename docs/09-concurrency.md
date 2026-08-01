# Concurrency

> **Implementation Status:** `spawn` creates real OS threads via `kryos_spawn()` in the runtime. `kryos_spawn_wait_all()` is inserted at the end of `main` and joins all spawned threads before the process exits -- spawned threads are NOT daemon threads. Channels are fully implemented as MPMC queues with blocking `recv`. Channel creation (`chan()`), send, and receive all work end-to-end through codegen. Channels carry **i64-shaped values**; `send(ch, "some string")` is a **compile error** (`E0100`), not a silent reinterpretation -- for string/typed message passing use **actors** (mailbox arguments support i64, str, arrays, ...), or wrap ids/indexes over a side table. There is no user-facing non-blocking `try_recv` builtin -- non-blocking multiplexing is what `select` is for (it polls internally and takes the first ready channel); a raw try-receive would be a check-then-act race on an MPMC queue. `select` polls channels with `try_recv` and branches to the first ready channel (busy-poll with 1ms yield, exits when all channels close). Actor syntax (`actor Name { field: Type fn handler() { ... } }`) parses and type-checks, and the MIR/codegen pipeline contains lowering for actor definitions (handlers compile to mangled functions, `ActorSpawn` emits the thread, method calls emit `ActorSend`, and the compiler generates a dispatch loop `ActorName__dispatch`). **Actors are fully working on both backends (JIT and AOT).** `Counter()` spawns an actor on its own OS thread and returns a handle; `c.method(args)` sends a message (handler tag + arguments) to its mailbox; the actor processes messages one at a time, in order, mutating its private state via `self.field`. Message arguments (i64, str, arrays, ...) are transmitted through the mailbox. At `main` exit the runtime closes every mailbox, lets each actor drain its remaining messages, and joins the threads -- so every message sent before `main` returns is processed, with no `sleep` needed. Handler return values are discarded (fire-and-forget); request-response requires a reply channel. Messages to one actor are FIFO-ordered; different actors run concurrently and their outputs may interleave.** Actor state uses struct-field syntax (`count: i64`), not `let mut`. `parallel for` over a `range()` IS implemented (chunked across 4 OS threads); `parallel for` over a non-range iterable currently falls back to a sequential `for`. `parallel for` does NOT insert a join point -- code immediately after the loop starts while the parallel chunks are still running. **`async`/`await`: non-blocking I/O + cooperative interleaving.** A blocking I/O op (`sleep`, `http_get`, `tcp_connect`/`recv`/`send`/`accept`) called inside an `async` task yields the scheduler for the duration of the syscall, so sibling `coop_spawn`ed tasks run concurrently -- four async tasks each doing 300ms of I/O finish in ~300ms, not 1.2s (verified on both backends). `await` additionally yields cooperatively for CPU-task interleaving: `coop_spawn(task_a()) + coop_spawn(task_b()) + coop_run()` produces `A0 B0 A1 B1 A2 B2`, not `A0 A1 A2 B0 B1 B2`. (This is a thread-per-task scheduler with baton-release on blocking calls, not an epoll/IOCP reactor -- the observable behavior is concurrent async I/O.)

Kryos's concurrency model is `spawn` for fire-and-forget OS-thread parallelism, `chan()` channels (MPMC queues of i64-shaped values, blocking `recv`; use `select` for non-blocking multiplexing), a cooperative `async`/`await` executor (`coop_spawn` / `coop_run`) that genuinely interleaves tasks, and `actor` for stateful message-passing with typed arguments including strings and arrays (each actor owns private state and processes messages one at a time on its own thread). All are built into the language syntax -- no library imports, no callback chains.

## spawn

`spawn` runs a block of code in a new thread. The parent continues immediately without waiting.

```
println("before")

spawn {
    println("spawned")
}

sleep(200)
println("after")
```

Output (one possible ordering):

```
before
after
spawned
```

The compiler inserts a `kryos_spawn_wait_all()` call at the end of `main`, so all spawned threads are joined before the process exits. The `sleep(200)` is for ordering -- it gives the main thread time to print `"after"` before the spawned thread prints. Without the sleep, the output order may vary because spawned threads run concurrently.

### What spawn does

1. Captures the current environment (variables in scope are visible to the spawned block)
2. Starts a new OS thread
3. Runs the block body in that thread
4. Returns immediately to the caller

The spawned block runs in its own child environment. It can read variables from the parent scope, but every capture is a **snapshot**: assignments inside the block -- including index and field mutation (`arr[0] = v`, `s.field = v`, `m["k"] = v`) -- act on the thread's own copy and are never visible to the parent. This holds uniformly for arrays, structs, maps, and strings. For real cross-thread state use the designed tools: **channels** (`std::chan`) to transfer values, **atomics** (`std::sync::atomic_int` / `atomic_bool`) for shared counters and flags, and a `ChanWaitGroup` / `WaitGroup` to join. (`coop_spawn` cooperative tasks run on the parent thread and DO share captured heap objects -- only OS-thread `spawn` snapshots.)

**Exception, and a real data race: a captured CLOSURE or fn-value does NOT snapshot.** Every other capture kind above (str/array/map/struct/enum) is deep-copied so the spawned thread privately owns it. A closure or bare function value captured into a `spawn` block is instead reference-counted and SHARED (`kryos_arc_retain`, not a copy) -- if the same closure value is captured by more than one `spawn` (or by `spawn` and also called from the parent thread), every caller shares the exact same closure environment. If that closure mutates one of its own captured scalars (the common `let bump = || { n = n + 1  n }` counter idiom), the mutation persists via a non-atomic read-call-then-write-back sequence with no lock -- concurrent callers from different threads can race and silently lose increments. This is a genuine, reproducible bug (`tests/known_failures/spawn_closure_shared_env_race.kry`), not a documented tradeoff: do not share a stateful closure across `spawn` blocks. Use a channel, an atomic, or an actor for any counter/accumulator state that more than one spawned thread touches.

### Error handling in spawned blocks

If a spawned block **throws** a (recoverable) exception, the error is captured and reported (`kryos: uncaught exception in spawned thread: ...`) and it does **not** crash the parent thread -- the spawned thread dies, the process lives on.

**This isolation covers `throw` only.** An unrecoverable **panic** -- integer division by zero, array index out of bounds, and other exit-98 runtime faults -- inside a spawned block terminates the **whole process** (exit 98); it is not isolated to the spawned thread, and any not-yet-flushed work on the main thread is lost. This is because a panic routes through `kryos_panic` (a process-wide `exit(98)`) and cannot unwind through the generated `extern "C"` frames (on Windows the JIT cannot unwind at all). If a spawned task might hit an unrecoverable fault (an untrusted divisor, an index that could be out of range), **guard it explicitly** (check the divisor/bounds, or use `throw` for a recoverable failure you want isolated). Both backends behave identically here.

```
spawn {
    let x = 1 / 0  // division by zero
}
// Parent continues running -- the error is logged, not propagated
```

A `return` statement inside a `spawn` block exits the spawned thread, not the enclosing function.

### Coordination with sleep

`sleep(ms)` pauses the current thread for `ms` milliseconds. It is the simplest way to coordinate timing between spawned work and the main thread.

```
spawn {
    sleep(1000)
    println("one second later")
}

spawn {
    sleep(500)
    println("half second later")
}

sleep(1500)
println("done")
```

Output:

```
half second later
one second later
done
```

`sleep` takes `i64` milliseconds -- `sleep(100)` is 100 milliseconds. `sleep_ms(ms: i64)` is an alias with identical behavior.

### Spawn for parallel computation

Use `spawn` when you have independent work that does not need to communicate results back. Good uses:

- Background logging or metrics
- Prefetching data while the main thread processes the current batch
- Running independent computations in parallel

```
fn heavy_compute(n: i64) -> i64 {
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

sleep(1000)
```

## Actors

> **Note:** Handler return values are discarded (fire-and-forget); a
> request-response pattern needs a reply channel. Messages to one actor are
> processed in order; different actors run concurrently.

Actors are Kryos's structured-concurrency model. An actor is a self-contained unit with private state and message handlers. No one can read or write an actor's state directly -- the only way to interact is by sending messages through its handlers. See `examples/actors.kry` for a runnable program.

### Declaring an actor

```
actor Counter {
    count: i64

    fn increment(amount: i64) {
        count = count + amount
    }

    fn get_count() -> i64 {
        return count
    }

    fn reset() {
        count = 0
    }
}
```

The `actor` keyword declares the type. Inside:
- Bare `name: Type` declarations define the actor's private state (struct-field syntax, not `let mut`)
- `fn` declarations define message handlers

All state fields are implicitly mutable within handlers. Handlers can read and modify the actor's own state freely.

### Actor state isolation

Each actor instance owns its state. There is no shared mutable state between actors -- this eliminates data races by construction.

```
actor Logger {
    entries: [str]

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
    memory: f64

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
    request_count: i64
    error_count: i64
    total_latency_ms: f64

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
    state: str
    retries: i64

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
    msg ch1 => {
        println("got from ch1: " + to_string(msg))
    }
    msg ch2 => {
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
    tasks: [str]

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
        sleep(100)
        // Process available tasks
    }
}
```

## Coming from JavaScript

| JavaScript | Kryos |
|------------|-------|
| `new Promise(...)` | `spawn { ... }` |
| `async/await` | Not needed |
| `setTimeout(fn, ms)` | `spawn { sleep(ms) ... }` |
| `Worker` (Web Workers) | `spawn` + actors |
| Event emitters | Actor handlers |

The biggest win: no callback hell, no promise chains, no async/await viral coloring. `spawn` gives you parallelism without restructuring your entire call stack.

## Common Mistakes

**Relying on spawn order for output.** `kryos_spawn_wait_all()` joins all spawned threads before the process exits, so spawned work always completes -- but the order in which threads run is not guaranteed. Use `sleep()` to bias timing if output order matters, or use channels if you need coordinated results.

**Trying to return values from spawn.** `spawn` does not return the result of the block. If you need a result, use an actor with a handler that stores the result, then query the actor from the main thread.

**Assuming execution order.** Spawned blocks run concurrently. Their print statements may interleave with the main thread. Never rely on specific ordering between spawned blocks or between a spawned block and the main thread.

## Cooperative async executor (`await` / `coop_*`)

Separate from `spawn` (truly-parallel OS threads), Kryos has a **cooperative**
executor where multiple tasks make progress *interleaved* and exactly one task
runs at a time. `await` is a real suspension point: it hands control back to the
scheduler so another ready task runs, instead of running straight through.

This is the defining difference from the earlier behavior, where `async fn`
existed but `await` lowered to a plain synchronous call (one task ran to
completion before the next started). It now genuinely interleaves.

### Surface

| Form | Meaning |
|------|---------|
| `coop_spawn(task())` | Register `task` as a cooperative task (runs under the executor, not a parallel OS thread). Accepts a call, a closure, or a block — like `spawn`. |
| `coop_yield()` | Hand control to the scheduler; the current task resumes on its next turn. |
| `await e` | Evaluate `e`, then yield to the scheduler (sugar for an explicit yield). On a non-task thread it is a no-op, so a direct async call degrades to synchronous. |
| `coop_run()` | Drive all registered tasks to completion (round-robin). Call from `main`. |
| `coop_record(tag)` / `coop_order()` | Append a tag to / read back the executor's order log (handy for tests and proofs). |
| `coop_reset()` | Clear the executor (queue + order log). |

### Example — real interleaving

```kryos
async fn task_a() {
    let mut i = 0
    while i < 3 {
        coop_record("A" + to_string(i))
        await 0          // yields to the scheduler
        i = i + 1
    }
}
async fn task_b() {
    let mut i = 0
    while i < 3 {
        coop_record("B" + to_string(i))
        await 0
        i = i + 1
    }
}
fn main() {
    coop_reset()
    coop_spawn(task_a())
    coop_spawn(task_b())
    coop_run()
    println(coop_order())   // "A0 B0 A1 B1 A2 B2"  (interleaved, not "A0 A1 A2 B0 B1 B2")
}
```

The two tasks interleave their effects `A,B,A,B,...` because `await`/`coop_yield`
parks the running task and the scheduler round-robins to the next ready one.

### How it works

`coop_spawn` runs each task body straight-line on its own OS thread, but a
global **baton** guarantees only one task runs at any instant. `await` /
`coop_yield` re-queue the current task and hand the baton back to the scheduler
(`coop_run`), which grants it to the next ready task. No CPS/state-machine
transform is involved — the suspension is the task thread parking on a condvar.
Works on all three paths: `kryos run` (JIT), `kryos build` (Cranelift object),
and `kryos build --release` (LLVM AOT).

### Limits (deferred)

- `coop_spawn` marshals **up to 8** captured values into a task (a closure over
  several locals works, e.g. `coop_spawn(|| { x + y + z })`); top-level task fns
  and zero/one/multi-capture closures are all supported. (Verified on both
  backends; the old one-capture limit was lifted -- see `invoke_task`.)
- `await` is a yield point, not a future/result combinator: there is no
  `await future` that resolves a value across a real suspension — the awaited
  expression is evaluated eagerly, then the task yields.
- Task results are not threaded back; communicate via actors/channels or the
  order log.
