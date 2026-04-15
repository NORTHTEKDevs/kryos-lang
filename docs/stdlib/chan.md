# std::chan

Concurrency primitives built around typed channels. `std::chan` provides buffered and unbuffered channels, select, fan-out/fan-in, semaphores, one-time initialization, and producer/consumer patterns inspired by Go's concurrency model.

```kryos
use std::chan
```

---

## Types

### Channel

```kryos
struct Channel {
    raw:       any,
    capacity:  i64,
    is_closed: shared bool
}
```

`is_closed` is a `shared` field -- reads and writes are atomic across goroutines.

---

### SelectCase

A single arm of a `select` expression.

```kryos
struct SelectCase {
    channel: Channel,
    handler: fn(any)
}
```

---

### WaitGroup

```kryos
struct WaitGroup {
    count:   shared i64,
    done_ch: Channel
}
```

---

### Semaphore

```kryos
struct Semaphore {
    ch: Channel
}
```

---

### Once

```kryos
struct Once {
    done:   shared bool,
    result: shared any
}
```

---

## Channel Creation

### new_channel

`new_channel() -> Channel`

Create an unbuffered channel. A send blocks until a receiver is ready.

---

### buffered

`buffered(capacity: i64) -> Channel`

Create a buffered channel with the given capacity. Sends do not block until the buffer is full.

**Example:**
```kryos
use std::chan

let ch  = new_channel()        // unbuffered
let bch = buffered(10)         // buffered, capacity 10
```

---

## Core Operations

### send_val

`send_val(ch: Channel, val: any)`

Send `val` into `ch`. Blocks if the channel is full or has no ready receiver.

---

### receive

`receive(ch: Channel) -> any`

Receive and return the next value from `ch`. Blocks until a value is available.

---

### try_receive

`try_receive(ch: Channel) -> {ok: bool, value: any}`

Non-blocking receive. Returns `{ok: true, value: v}` if a value was available, or `{ok: false, value: null}` otherwise.

---

### close

`close(ch: Channel)`

Close the channel. Subsequent sends throw. Pending receivers drain remaining buffered values, then receive zero values.

---

### is_closed

`is_closed(ch: Channel) -> bool`

Return `true` if the channel has been closed.

**Example:**
```kryos
use std::chan

let ch = buffered(3)

send_val(ch, 1)
send_val(ch, 2)
send_val(ch, 3)
close(ch)

println(receive(ch))   // 1
println(receive(ch))   // 2
println(receive(ch))   // 3
println(is_closed(ch)) // true
```

---

## Select

### on

`on(ch: Channel, handler: fn(any)) -> SelectCase`

Create a `SelectCase` that calls `handler` when `ch` has a value ready.

---

### select_cases

`select_cases(cases: [SelectCase])`

Wait until one of the provided channels is ready, then execute its handler. If multiple channels are ready simultaneously, one is chosen at random.

**Example:**
```kryos
use std::chan

let a = buffered(1)
let b = buffered(1)

send_val(a, "from a")
send_val(b, "from b")

select_cases([
    on(a, fn(v: any) { println("a: " + v) }),
    on(b, fn(v: any) { println("b: " + v) })
])
```

---

## Timers

### timeout

`timeout(duration_ms: i64) -> Channel`

Return a channel that receives a single value after `duration_ms` milliseconds.

---

### ticker

`ticker(interval_ms: i64) -> Channel`

Return a channel that receives a value every `interval_ms` milliseconds until closed.

**Example:**
```kryos
use std::chan

let tick = ticker(1000)
let stop = timeout(5000)

// Receive up to 4 ticks before the 5-second timeout
```

---

## Fan Patterns

### fan_out

`fan_out(source: Channel, workers: i64, handler: fn(any))`

Distribute values from `source` to `workers` concurrent goroutines, each calling `handler` for each value.

---

### fan_in

`fan_in(channels: [Channel]) -> Channel`

Merge multiple input channels into a single output channel.

---

### pipe

`pipe(source: Channel, dest: Channel)`

Forward all values from `source` to `dest` until `source` is closed.

**Example:**
```kryos
use std::chan

let work   = buffered(100)
let result = buffered(100)

// Distribute work across 4 goroutines
fan_out(work, 4, fn(item: any) {
    send_val(result, item)   // process and forward
})
```

---

## WaitGroup

Coordinate completion of concurrent tasks.

### new_wait_group

`new_wait_group() -> WaitGroup`

Create a `WaitGroup` with `count = 0`.

---

### wg_add

`wg_add(wg: WaitGroup, n: i64)`

Increment the counter by `n`. Call before spawning tasks.

---

### wg_done

`wg_done(wg: WaitGroup)`

Decrement the counter by 1. Call when a task finishes.

---

### wg_wait

`wg_wait(wg: WaitGroup)`

Block until the counter reaches zero.

**Example:**
```kryos
use std::chan

let wg = new_wait_group()
wg_add(wg, 3)

// ... spawn 3 goroutines, each calling wg_done(wg) when finished ...

wg_wait(wg)
println("all tasks complete")
```

---

## Semaphore

Limit the number of goroutines accessing a resource concurrently.

### new_semaphore

`new_semaphore(permits: i64) -> Semaphore`

Create a semaphore with `permits` available permits.

---

### acquire

`acquire(sem: Semaphore)`

Acquire a permit. Blocks if none are available.

---

### release

`release(sem: Semaphore)`

Release a permit.

---

### try_acquire

`try_acquire(sem: Semaphore) -> bool`

Non-blocking acquire. Returns `true` if a permit was acquired, `false` otherwise.

**Example:**
```kryos
use std::chan

let sem = new_semaphore(3)   // allow at most 3 concurrent DB connections

acquire(sem)
// ... use resource ...
release(sem)
```

---

## Once

Execute a function exactly once across all goroutines.

### new_once

`new_once() -> Once`

Create an unexecuted `Once` guard.

---

### call_once

`call_once(once: Once, f: fn() -> any) -> any`

Execute `f` the first time this is called and return its result. All subsequent calls return the cached result without calling `f` again.

**Example:**
```kryos
use std::chan

let init = new_once()

call_once(init, fn() -> any {
    println("initializing once")
    return "ready"
})

call_once(init, fn() -> any {
    println("this never runs")
    return ""
})
```

---

## Producer/Consumer

### produce

`produce(generator: fn() -> any) -> Channel`

Spawn a goroutine that calls `generator` repeatedly, sending each result into a new channel until `generator` returns `null`.

---

### consume

`consume(ch: Channel, handler: fn(any))`

Receive values from `ch` and call `handler` for each one until `ch` is closed.

---

### collect

`collect(ch: Channel) -> [any]`

Drain `ch` into an array. Blocks until the channel is closed.

**Example:**
```kryos
use std::chan

let counter = 0
let source = produce(fn() -> any {
    if counter >= 5 { return null }
    counter = counter + 1
    return counter
})

let values = collect(source)
println(values)   // [1, 2, 3, 4, 5]
```

---

## Utilities

### send_all

`send_all(ch: Channel, values: [any])`

Send each element of `values` into `ch` in order.

---

### receive_n

`receive_n(ch: Channel, n: i64) -> [any]`

Receive exactly `n` values from `ch` and return them as an array.

**Example:**
```kryos
use std::chan

let ch = buffered(5)
send_all(ch, [10, 20, 30, 40, 50])

let first_three = receive_n(ch, 3)
println(first_three)   // [10, 20, 30]
```

---

## Complete Example

```kryos
use std::chan

// Pipeline: generate -> process -> collect
let numbers = produce(fn() -> any {
    // yields 1..10, then stops
    // state is captured in closure; simplified here
    return null
})

let processed = new_channel()
fan_out(numbers, 4, fn(n: any) {
    send_val(processed, n)
})

let results = collect(processed)
println(len(results))

// Rate-limited parallel work
let sem = new_semaphore(5)
let wg  = new_wait_group()
let tasks = [1, 2, 3, 4, 5, 6, 7, 8]

wg_add(wg, len(tasks))

let i = 0
while i < len(tasks) {
    let task = tasks[i]
    acquire(sem)
    // spawn goroutine:
    //   process(task)
    //   release(sem)
    //   wg_done(wg)
    i = i + 1
}

wg_wait(wg)
println("all tasks done")
```
