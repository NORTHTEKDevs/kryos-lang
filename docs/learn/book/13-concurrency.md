# 13 · Concurrency: spawn, channels, actors

After this chapter you will be able to run independent work in parallel with
`spawn`, pass values safely between threads with channels, build stateful
concurrent components with actors, and know exactly which of Kryos's
concurrency guarantees are proven and which are still sharp edges you need to
route around by hand.

Kryos gives you three built-in concurrency tools -- `spawn` for
fire-and-forget parallelism, channels for passing values between threads, and
actors for stateful message-passing -- plus `std::sync` primitives (`Mutex`,
`AtomicInt`) for the cases where you want a shared value instead of message
passing. All of it is language syntax, not a library: no callback chains, no
executor to configure before you can use a thread.

## `spawn`: fire-and-forget parallelism

`spawn { ... }` starts a new OS thread running the block, and returns
immediately -- the parent thread does not wait.

```kryos
fn main() {
    println("before")

    spawn {
        println("spawned")
    }

    sleep(200)
    println("after")
}
```

Output:

```
before
spawned
after
```

The `sleep(200)` gives the spawned thread time to run before `main` moves
on -- without it, "after" could print before "spawned", since the two
threads run concurrently with no ordering guarantee between them. The
compiler inserts a `kryos_spawn_wait_all()` call at the end of `main`, so
every spawned thread is joined before the process exits -- a spawned thread
is not a daemon thread you can leave running; the program does not exit
until it finishes.

### What a spawned block can see

A `spawn` block captures the variables in scope around it, but the capture
is a **snapshot**, not a shared reference. Assigning to a captured
variable -- including through an index or field (`arr[0] = v`, `s.field = v`)
-- mutates the spawned thread's own copy and is never visible back in the
parent:

```kryos
fn main() {
    let mut n = 0
    let mut items: [i64] = [1, 2, 3]

    spawn {
        n = 99
        items[0] = 999
        println("inside spawn: n=" + to_string(n) + " items[0]=" + to_string(items[0]))
    }

    sleep(200)
    println("outside spawn: n=" + to_string(n) + " items[0]=" + to_string(items[0]))
}
```

Output:

```
inside spawn: n=99 items[0]=999
outside spawn: n=0 items[0]=1
```

This holds for scalars, arrays, structs, maps, and strings alike -- every
capture is deep-copied into the spawned thread's own environment. If you
need the spawned thread to hand a result back, use a channel or an actor
(below); `spawn` itself has no return value.

**One capture kind breaks this rule on purpose: a closure or bare function
value is shared, not snapshotted.** If the same closure is captured by more
than one `spawn` block -- or captured by `spawn` and also called from the
parent thread -- every caller shares the exact same closure environment.
This is what lets a single stateful closure act as shared state across
threads, the same way a `Mutex` or an `AtomicInt` does:

```kryos
use std::chan::{new_wait_group, wg_add, wg_done, wg_wait}

fn main() {
    let mut n = 0
    let bump = || {
        n = n + 1
        n
    }

    let mut wg = new_wait_group()
    let mut i = 0
    while i < 50 {
        wg = wg_add(wg, 1)
        let wgc = wg
        spawn {
            bump()
            wg_done(wgc)
        }
        i = i + 1
    }
    wg_wait(wg)
    println("last value the closure saw: " + to_string(bump()))
}
```

Output:

```
last value the closure saw: 51
```

Every call to `bump()` -- from any of the 50 spawned threads, in any
order -- is serialized under a lock scoped to that closure's own
environment, so no increment is ever lost: 50 concurrent bumps plus the one
final call on the main thread add up to exactly 51. Notice what is
conspicuously absent from that output: printing the outer `n` after
`wg_wait` would show `0`, not `50`. `n` is a variable the closure itself
mutates, so per the closure-capture rule from [Chapter
4](04-functions.md), `bump` captured it **by move** into its own private
environment -- the outer binding named `n` is a separate, frozen copy. The
only way to observe the closure's internal counter is through the closure
itself, by calling it again or having it report the value, exactly as
`bump()`'s return value does above.

This serialization is correctness, not speed: it locks the *entire* call,
not just the read-modify-write, so a `println` inside a shared closure body
can never be duplicated or torn. For a hot shared counter with no other
logic attached, `std::sync::atomic_int` (below) is faster because it locks
only the individual operation.

**A mutating shared closure must never call itself, even indirectly, from
the same thread.** If a mutating closure gets a live handle to itself (say,
stashed in a map it also reads) and its body calls that handle again before
the outer call returns, the thread would try to re-acquire a lock it
already holds. Kryos detects this and panics immediately
(`kryos panic: reentrant call into a mutating shared closure: ...`, exit
98) rather than hanging. If you need self-recursion, write a named `fn`
instead -- named recursive functions are unaffected by any of this.

**That detector only catches a thread re-acquiring its own closure's
lock -- a deadlock between two different shared closures is invisible to
it.** Each closure's lock lives at its own, distinct address, so nothing
about a cross-closure wait looks like the same-lock reentrancy case above.
If thread 1 calls closure A (which, before returning, calls closure B)
while thread 2 concurrently calls closure B (which calls closure A), the
two threads can each end up holding one lock and blocking on the other --
a classic two-lock deadlock, no diagnostic, no timeout. This is a
confirmed, reproducible hang (not a hypothetical), tracked as [LEDGER item
46](../../../tools/loop/LEDGER.md). Don't let two shared mutating closures
call each other from different threads; route the interaction through a
channel instead of a direct call if they need to coordinate.

### Error handling inside a spawned block

An uncaught `throw` or an unrecoverable panic (division by zero, an
out-of-bounds index) inside a spawned block is fatal to the **whole
process**, not just that thread -- `kryos: uncaught exception in spawned
thread: ...` (exit 101) for a throw, exit 98 for a panic. This is
deliberate: Kryos exceptions are a thread-local flag with no stack
unwinding, so every statement after the throw point in that task's body --
commonly a paired "I'm done" signal a `WaitGroup` elsewhere is blocked
waiting for -- is silently skipped. Isolating the failure to one thread
would leave the rest of the program hanging forever on a signal that will
never arrive, with no diagnostic connecting the two. Treating it as fatal
converts a silent permanent hang into an immediate, attributable exit.

If you want one failing task to not take down a batch job, catch inside the
task and send the "done" signal from both the success and failure paths:

```kryos
use std::chan::{new_wait_group, wg_add, wg_done, wg_wait}

fn risky_work(n: i64) -> i64 {
    if n == 3 {
        throw "boom"
    }
    return n * 2
}

fn main() {
    let mut wg = new_wait_group()
    let mut i = 0
    while i < 5 {
        wg = wg_add(wg, 1)
        let idx = i
        let wgc = wg
        spawn {
            try {
                let r = risky_work(idx)
                println("worker " + to_string(idx) + " result=" + to_string(r))
            } catch e {
                println("worker " + to_string(idx) + " failed: " + e)
            }
            wg_done(wgc)   // runs on BOTH the success and the catch path
        }
        i = i + 1
    }
    wg_wait(wg)
    println("main: all workers done")
}
```

## Channels

A channel is a queue of `i64`-shaped values shared between threads.
`chan()` creates one, `send(ch, v)` enqueues without blocking, and
`recv(ch)` blocks until a value is available.

```kryos
fn main() {
    let ch = chan()

    spawn {
        send(ch, 1)
        send(ch, 41)
    }

    let a = recv(ch)
    let b = recv(ch)
    println(to_string(a + b))
}
```

Output:

```
42
```

Channels carry `i64` only -- `send(ch, "some string")` is a compile error
(`E0100`), not a silent reinterpretation. To pass strings, arrays, or other
typed values between threads, use an actor's mailbox instead (below), which
does support typed arguments, or send an integer id/index into a side table
you look up on the receiving end.

### Waiting on more than one channel: `select`

`select` waits on several channels at once and runs the first branch whose
channel has data ready:

```kryos
fn main() {
    let ch1 = chan()
    let ch2 = chan()

    spawn {
        sleep(10)
        send(ch1, 1)
    }
    spawn {
        sleep(50)
        send(ch2, 2)
    }

    let mut received = 0
    while received < 2 {
        select {
            msg ch1 => {
                println("from ch1: " + to_string(msg))
                received = received + 1
            }
            msg ch2 => {
                println("from ch2: " + to_string(msg))
                received = received + 1
            }
        }
    }
}
```

Output:

```
from ch1: 1
from ch2: 2
```

`ch1`'s sender sleeps 10ms and `ch2`'s sleeps 50ms, so `ch1` is reliably
first. `select` polls internally (it is how Kryos gives you non-blocking
multiplexing without a separate `try_recv` builtin) and blocks only until
at least one channel is ready.

### The honest edge: a closed, drained channel is ambiguous through `recv`

`recv` on a closed channel with nothing left in it returns a plain `0` --
the exact same value a real `send(ch, 0)` would have produced. `recv` alone
cannot tell you which one happened:

```kryos
fn main() {
    let real_zero = chan()
    send(real_zero, 0)

    let empty_closed = chan()
    close_chan(empty_closed)

    let v1 = recv(real_zero)
    let v2 = recv(empty_closed)
    println("v1=" + to_string(v1) + " v2=" + to_string(v2) + " equal=" + to_string(v1 == v2))

    println("real_zero is_closed=" + to_string(chan_is_closed(real_zero)))
    println("empty_closed is_closed=" + to_string(chan_is_closed(empty_closed)))
}
```

Output:

```
v1=0 v2=0 equal=true
real_zero is_closed=0
empty_closed is_closed=1
```

If a `0` on a channel could legitimately mean either "a real value" or
"nothing left, ever", disambiguate with `chan_is_closed(ch)` (as above) or
`chan_try_recv(ch)`, which returns `1` for data ready, `0` for empty-but-open,
or `-1` for closed-and-drained, without blocking.

### Coordinating shutdown: `WaitGroup`

`std::chan::new_wait_group`/`wg_add`/`wg_done`/`wg_wait` (used throughout
this chapter already) is the standard fan-out/fan-in pattern: add 1 to the
group per task you spawn, have each task call `wg_done` when it finishes,
and block on `wg_wait` until every one has checked in.

**Known limitation:** `wg_wait` only reliably releases the *first* thread
blocked on it. If two separate threads both call `wg_wait` on the same
`WaitGroup`, completion is signaled by one token on an internal channel --
the first waiter to dequeue it returns, and the second blocks forever on an
empty channel that nothing will ever fill. This is a real, confirmed hang
(not a hypothetical), tracked as [LEDGER item
46](../../../tools/loop/LEDGER.md). Use one `wg_wait` caller per
`WaitGroup`, on the thread that owns the fan-out.

## Actors

An actor is a `Name()`-constructed concurrent unit with private,
zero-initialized state and message-handler methods. No code outside the
actor can read or write its state directly -- the only way in is by calling
a handler, which the actor's own thread processes one at a time, in FIFO
order. Two different actors run concurrently with no ordering guarantee
between them, but messages to the *same* actor never race each other,
because only that actor's own thread ever touches its state.

```kryos
actor Account {
    balance: i64

    fn deposit(self, amount: i64) {
        self.balance = self.balance + amount
        println("balance: " + to_string(self.balance))
    }

    fn withdraw(self, amount: i64) {
        self.balance = self.balance - amount
        println("balance: " + to_string(self.balance))
    }
}

fn main() {
    let acct = Account()
    acct.deposit(100)
    acct.deposit(50)
    acct.withdraw(30)
    acct.deposit(500)
}
```

Output:

```
balance: 100
balance: 150
balance: 120
balance: 620
```

Inside `actor Account { ... }`, a bare `name: Type` line declares a private
state field (struct-field syntax, not `let mut`), and each `fn` is a
message handler whose first parameter is `self`. `Account()` spawns the
actor on its own OS thread and returns a handle; `acct.deposit(100)` sends
a message to its mailbox. At `main`'s exit, the runtime drains every
actor's mailbox and joins its thread, so every message sent before `main`
returns is guaranteed to be processed -- no `sleep` needed to wait for it.

### Handlers are fire-and-forget: request-response needs a reply channel

A handler cannot declare a return type. This is a compile error, not a
runtime limitation:

<!-- docs-example: skip -->
```kryos
actor Counter {
    count: i64

    fn get(self) -> i64 {   // ERROR
        return self.count
    }
}
```

```
error[E0110]: actor handler `get` declares return type `i64`, but actor sends
are asynchronous fire-and-forget: there is no synchronous reply channel, so
the return value can never reach this call site (see docs/09-concurrency.md).
Request-response actors are not supported yet -- declare `get` with no return
type (fire-and-forget) instead.
```

There is no reply path baked into the language, but you can build one: pass
a channel as an argument and have the handler `send` its answer on it.

```kryos
actor Counter {
    count: i64

    fn inc(self) {
        self.count = self.count + 1
    }

    fn get(self, reply: i64) {
        send(reply, self.count)
    }
}

fn main() {
    let c = Counter()
    c.inc()
    c.inc()
    c.inc()

    let reply = chan()
    c.get(reply)
    let v = recv(reply)
    println("count=" + to_string(v))
}
```

Output:

```
count=3
```

Because a `chan()` handle is itself `i64`-shaped, it passes through an
actor's mailbox like any other argument, and `recv` on the caller's side
blocks until the actor's handler gets around to sending the reply --
respecting the actor's FIFO processing order the same as any other message.

## Sync primitives: `Mutex` and `AtomicInt`

Channels and actors are Kryos's preferred concurrency tools -- pass data,
don't share it. When you genuinely want one shared value updated from many
threads, `std::sync` gives you the traditional primitives.

`AtomicInt` is the simple, fast case: a single shared counter with atomic
`load`/`store`/`fetch_add`/`increment`/`decrement`/`compare_and_swap`.

```kryos
use std::sync::{atomic_int}
use std::chan::{new_wait_group, wg_add, wg_done, wg_wait}

@capabilities(ffi)
fn main() {
    let counter = atomic_int(0)

    let mut wg = new_wait_group()
    let mut i = 0
    while i < 100 {
        wg = wg_add(wg, 1)
        let wgc = wg
        spawn {
            counter.increment()
            wg_done(wgc)
        }
        i = i + 1
    }
    wg_wait(wg)
    println("final count: " + to_string(counter.load()))
}
```

Output:

```
final count: 100
```

100 concurrent increments, no lost updates, no `Mutex` written by hand.
`atomic_int` needs `@capabilities(ffi)` because it is built on the raw
pointer builtins (`alloc`, `ptr_read_i64`, `ptr_write_i64`) under the hood,
same as the FFI surface in [Chapter 19](19-ffi-and-unsafe.md) -- reasonable
for a program that legitimately shares memory across threads, but it does
mean an `AtomicInt` shows up in a capability audit the same way a raw
pointer operation would.

`Mutex` is the general-purpose lock, for protecting more than a single
integer. Its API is value-returning rather than mutate-in-place: `lock()`
and `unlock()` each return a *new* `Mutex` value reflecting the updated
lock state, so you reassign rather than call them as bare statements:

```kryos
use std::sync::{mutex_new}

fn main() {
    let balance = mutex_new()
    let locked = balance.lock()
    println("locked: " + to_string(locked.is_locked()))
    locked.unlock()
}
```

Output:

```
locked: true
```

Reach for `Mutex` when the protected state is more than one atomic
operation (e.g. a struct with several fields that must update together);
reach for `AtomicInt`/`AtomicBool` for a single hot counter or flag; reach
for actors or channels first, and fall back to these only when message
passing genuinely does not fit the problem.

## Common mistakes

**Expecting `spawn` block mutations to be visible outside it.** Every
capture except a closure/fn-value is snapshotted, not shared -- see "What a
spawned block can see" above. If you need a result back, use a channel or
an actor.

**Declaring an actor handler with a return type.** `fn get(self) -> i64`
inside an `actor` block is a compile error (`E0110`) -- handlers are
fire-and-forget by design. Pass a reply channel as an argument instead.

**Trusting `recv` alone to detect a closed channel.** A drained, closed
channel and a real `send(ch, 0)` both make `recv` return `0`. Call
`chan_is_closed`/`chan_try_recv` when a `0` needs to be distinguishable
from "there is nothing left".

**Two threads calling `wg_wait` on the same `WaitGroup`.** Only the first
waiter is released; the second hangs forever (LEDGER item 46). Keep
`wg_wait` to a single caller per `WaitGroup`.

**Two shared mutating closures calling each other from different
threads.** Each closure's lock is invisible to the other's reentrancy
check, so this can deadlock with no diagnostic (LEDGER item 46). Route the
interaction through a channel instead.

**Building a hot shared counter with a `Mutex` when `AtomicInt` would
do.** `Mutex` works for this, but `AtomicInt`'s lock is scoped to the
single operation, not the whole critical section -- prefer it whenever the
protected state really is just one number or flag.

## Exercises

1. Take the `spawn` capture-snapshot example and add a second spawned block
   that also mutates `n`. Predict what the parent thread sees printed
   after both `sleep(200)`s, then run it and check.
2. Take the actor reply-channel example and add a `reset` handler that
   zeroes `self.count` with no reply. Call it between two `get` calls and
   confirm the second reply reflects the reset.
3. Rewrite the `AtomicInt` example using `Mutex` around a plain `i64`
   local instead (hint: you will need to hold the count somewhere the
   lock can protect -- a one-field struct works). Confirm you still get
   `100`.
4. Deliberately trigger the actor-handler-return-type error by adding a
   `-> i64` to one of `Account`'s handlers. Read the real error and fix
   it back.

## Summary

- `spawn { ... }` runs a block on a new OS thread; the parent continues
  immediately, and the compiler joins every spawned thread before the
  process exits.
- Every `spawn` capture is a deep-copied snapshot, **except** a closure or
  fn-value, which is shared and, if it mutates its own capture, serialized
  under a per-closure lock -- correct, but locking the whole call, not just
  the load/store. The lock only detects a thread re-entering its OWN
  closure's lock; two different closures calling each other from separate
  threads can still deadlock (LEDGER item 46).
- An uncaught `throw` or an unrecoverable panic inside a spawned block is
  fatal to the whole process, not just that thread -- catch and signal
  "done" on both paths if you need per-task failure isolation.
- Channels (`chan()`/`send`/`recv`/`select`) carry `i64`-shaped values only;
  a closed, drained channel and a real `send(ch, 0)` are indistinguishable
  through `recv` alone -- use `chan_is_closed`/`chan_try_recv`.
- Actors (`Name()`, `self`-taking handlers) own private state and process
  messages one at a time in FIFO order; handlers cannot return a value --
  build request-response with a reply channel passed as an argument.
- `std::sync::atomic_int`/`AtomicBool` and `Mutex` are the traditional
  shared-state primitives for when message passing does not fit; a
  two-caller `wg_wait` on one `WaitGroup`, and a cross-closure two-lock
  deadlock between two different shared mutating closures, are both
  known, confirmed hangs (LEDGER item 46) worth designing around.

Next: [Async](14-async.md)
