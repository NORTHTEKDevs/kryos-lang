# 14 · Async

After this chapter you will be able to run several I/O-bound or
CPU-interleaved tasks on Kryos's cooperative executor, understand precisely
what "concurrent" means here (one task runs at a time, but they take turns
instead of running to completion), and know where `async`/`await` is
genuinely production-ready versus where it still has open caveats worth
routing around.

`async`/`await` is a second concurrency model, separate from [Chapter
13](13-concurrency.md)'s `spawn`. Where `spawn` gives you true OS-thread
parallelism with snapshot-isolated state, `async`/`await` gives you
**cooperative** interleaving: multiple tasks make progress on a single
logical timeline, and each one explicitly hands control back at an `await`
point instead of running uninterrupted to completion. Reach for this when
you have several independent operations that each spend most of their time
waiting on I/O (a `sleep`, an HTTP call, a socket read) and you want them
to overlap that waiting time, without the snapshot-isolation model of
`spawn`.

## The core loop: `coop_spawn` and `coop_run`

Mark a function `async fn`, register instances of it with `coop_spawn`, then
drive them all to completion with `coop_run` from `main`:

```kryos
async fn task_a() {
    let mut i = 0
    while i < 3 {
        coop_record("A" + to_string(i))
        await 0
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
    println(coop_order())
}
```

Output:

```
A0 B0 A1 B1 A2 B2
```

Not `A0 A1 A2 B0 B1 B2`. `await` is a real suspension point: it hands
control to the scheduler, which round-robins to the next ready task, rather
than running the current task straight through. `coop_record`/`coop_order`
are testing/proof helpers -- they append a tag to an in-memory log and read
it back, which is how the example above can show you the actual
interleaving order deterministically instead of asking you to trust a
description of it. `coop_reset()` clears both the task queue and that log,
worth calling at the top of `main` if you might run more than one batch of
tasks in the same process (tests, in particular).

Mechanically, `coop_spawn` runs each task body on its own OS thread, but a
single global baton ensures only one task is actually executing at any
instant; `await`/`coop_yield` release that baton and re-queue the current
task. There is no CPS transform or state machine -- the "suspension" is a
real thread parking on a condvar until the scheduler grants it the baton
again. This works identically on all three backends (`kryos run`,
`kryos build`, `kryos build --release`).

## Why this matters: overlapping I/O wait time

The payoff for cooperative interleaving is that several tasks blocked on
I/O finish in roughly the time of the *slowest one*, not the *sum* of all
of them -- because each task's blocking call yields the scheduler for its
duration instead of stalling the whole program.

```kryos
async fn fetch_one(label: str) {
    println(label + ": start")
    sleep(300)
    println(label + ": done")
}

fn main() {
    coop_reset()
    let start = time_now_millis()

    coop_spawn(fetch_one("a"))
    coop_spawn(fetch_one("b"))
    coop_spawn(fetch_one("c"))
    coop_run()

    let elapsed = time_now_millis() - start
    println("elapsed_ms_under_1000: " + to_string(elapsed < 1000))
}
```

Output:

```
a: start
b: start
c: start
c: done
a: done
b: done
elapsed_ms_under_1000: true
```

Three tasks, each sleeping 300ms, finish in well under the 900ms a
sequential run would take (the exact elapsed time is timing-dependent, so
the example checks the threshold rather than printing a specific number).
`sleep` here is standing in for any blocking I/O op -- `http_get`,
`tcp_connect`/`recv`/`send`/`accept` behave the same way inside an `async`
task: the call yields the scheduler for the duration of the underlying
syscall, so sibling tasks run during that wait instead of blocking behind
it. This is implemented as a thread-per-task scheduler that releases the
baton on a blocking call, not an epoll/IOCP reactor -- but the observable
behavior is genuine concurrent I/O.

## A direct call to an `async fn` degrades to synchronous

Calling an `async fn` normally, without `coop_spawn`, runs it straight
through on the calling thread -- there is nothing to interleave with, so
`await` becomes a no-op:

```kryos
async fn double(n: i64) -> i64 {
    let r = await n * 2
    return r
}

fn main() {
    let x = double(21)
    println("direct call, no coop_spawn: " + to_string(x))
}
```

Output:

```
direct call, no coop_spawn: 42
```

This is convenient -- an `async fn` is still a perfectly normal function you
can call directly when you do not need concurrency for this particular
call -- but it also means forgetting `coop_spawn` does not fail loudly. If
you expected two tasks to interleave and instead saw them run fully
sequentially, check that both went through `coop_spawn`, not a direct call.

## The honest boundary

**`await` is a yield point, not a future/result combinator.** There is no
`let result = await some_pending_future` that resolves a value across a
real suspension the way `await` works in JavaScript or Rust -- the awaited
expression is evaluated eagerly, immediately, on the current task, and
*then* the task yields. `await n * 2` above evaluates `n * 2` right away;
the `await` only controls when the scheduler hands off, not when the
expression's value becomes available.

**Task results are not threaded back to the caller.** `coop_spawn` does not
give you a handle to read a return value from later -- the same limitation
`spawn` has (Chapter 13). Communicate results out of a task via an actor,
a channel, or (for tests and debugging) the `coop_record`/`coop_order` log
used above.

**`coop_spawn` marshals up to 8 captured values into a task.** A closure
over several locals works (`coop_spawn(|| { x + y + z })`), and both
top-level task functions and zero/one/multi-capture closures are supported
-- but a closure capturing more than 8 outer values is past the tested
surface.

**Holding a shared mutating closure's lock across an `await`/`sleep` point
can deadlock.** [Chapter 13](13-concurrency.md#spawn) covered the
per-closure lock that serializes concurrent calls to a shared mutating
closure. That lock has no knowledge of the cooperative executor: if a
`coop_spawn`ed task acquires it and then hits a blocking call before
releasing it (e.g. a `sleep` inside the locked body), the coop baton passes
to a second task that then spins forever trying to acquire the same lock,
while the first task can never resume to release it because the baton
never comes back. This is a confirmed, reproducible permanent hang, not a
hypothetical -- tracked as [LEDGER item
46](../../../tools/loop/LEDGER.md). If a shared mutating closure's body
needs to do async I/O, restructure so the blocking call happens outside the
closure's locked section, or avoid combining a shared mutating closure with
`coop_spawn` in the same task.

## Common mistakes

**Expecting `await` to resolve a pending value.** It does not -- the
expression before `await` runs eagerly and synchronously; only the
scheduler handoff is deferred. Design around a yield point, not a promise.

**Forgetting `coop_spawn` and calling an `async fn` directly.** It compiles
and runs -- just sequentially, with no interleaving, since there is no
sibling task to hand the baton to.

**Reading a task's result through a return value from `coop_spawn`.**
There is none. Use an actor, a channel, or the order log.

**Holding a shared closure's lock across a blocking call inside a
`coop_spawn`ed task.** See "The honest boundary" above -- this deadlocks
the whole process, both backends, no timeout.

## Exercises

1. Change the interleaving example to three tasks (`task_a`, `task_b`,
   `task_c`) each looping twice instead of three times. Predict
   `coop_order()`'s output before running it.
2. Take the `fetch_one` example and run only one `coop_spawn` instead of
   three. Confirm `elapsed_ms_under_1000` is still `true` (a single task
   with nothing to interleave with is still bounded by its own sleep, not
   inflated by anything).
3. Remove `coop_spawn` from the `fetch_one` example (call `fetch_one("a")`
   directly, three times, from `main`) and predict how the total elapsed
   time changes. Verify by running it.

## Summary

- `async fn` + `coop_spawn` + `coop_run` gives cooperative interleaving:
  one task executes at a time, and `await`/`coop_yield` hand the baton to
  the scheduler rather than blocking the whole program.
- The payoff is overlapping I/O wait time -- several tasks each blocked on
  `sleep`/`http_get`/`tcp_*` finish close to the slowest one's duration,
  not the sum of all of them.
- Calling an `async fn` directly, without `coop_spawn`, runs it
  synchronously on the calling thread -- `await` becomes a no-op.
- `await` is a yield point, not a future combinator: the awaited
  expression evaluates eagerly, then the task yields.
- Task results are not threaded back through `coop_spawn` -- use an actor,
  a channel, or `coop_record`/`coop_order` to communicate out of a task.
- Holding a shared mutating closure's lock across a blocking call inside a
  `coop_spawn`ed task is a confirmed permanent deadlock (LEDGER item 46) --
  keep blocking calls outside a shared closure's locked section.

Next: [Modules and packages](15-modules-and-packages.md)
