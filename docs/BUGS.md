# Known Bugs

## Resolved

### String-in-struct ownership leak across function returns (v0.4)

**Status**: Fixed. Tracked by regression tests in
`compiler/crates/kryos-test-runner/tests/e2e/ownership/struct_with_strings_return_run.kry`
and `struct_with_strings_stress_run.kry` (1000-iter loop).
`examples/showcase/agent_runtime.kry` was rewritten to return the planner's
`Action` struct directly (with str fields) instead of using `[str]`
out-parameter slots; both JIT and `kryos build --release` are verified.

Original symptom (v0.4-era): a function returning a struct whose `str`
field was assigned from a previously-moved local would surface garbage
values, e.g. `len(action.arg_s) = 7305790164731371552` and empty string
content.

Root cause: partial-move tracking for struct field reads
(`partial_moved_locals` in `kryos-mir::lower`) did not extend to the case
where a non-copy struct local was moved wholesale into a return position
after a field had separately been moved out via field access. The current
MIR lowering tracks partial moves explicitly and the ownership analyzer
emits the correct drop / no-drop combination at scope exit.

If a new reproducer ever surfaces, please attach it to a fresh entry below
under "Active".

## Active

### Concurrency + struct receivers: two conformance tests hang on `build --release`

**Status**: Open. Reproduced on `master` at `ac45392c` (Ubuntu 25.10,
glibc 2.43, clang 21) — not introduced by a recent change.

Two of the 41 conformance programs fail on the LLVM/AOT backend only; both
build cleanly, run past their first output, and then never terminate (killed
at a 120s timeout). `kryos run`/Cranelift passes both.

- `tests/conformance/conf_spinlock_mutex.kry` — repeatedly prints
  `kryos: uncaught exception in spawned thread: sync error: lock on dropped
  mutex`, then hangs. The mutex box is freed while spawned threads still hold
  a reference to it: `fn lock(self: SpinLock) -> SpinLock { .. return self }`
  makes `let l = lock.lock()` a second owner of a struct whose receiver
  ownership is not tracked. This is the same struct-representation issue
  documented at length in `CLAUDE.md` gotcha #22 (the struct-method receiver
  leak) — the leak is what keeps struct sharing safe today, and closing it
  needs a representation/ABI change, not an incremental patch.

- `tests/conformance/conf_errors_concurrency.kry` — prints
  `[actor error] Adder.add: uncaught exception: negative` and then hangs. An
  exception thrown inside an actor message handler is reported but the actor's
  mailbox loop never unwinds or shuts down, so the program's join never
  completes. Actor-handler exception propagation appears to have no termination
  path.

Both are true deadlocks, not slow runs: `conf_spinlock_mutex` stops producing
output after 8 lines and was still alive after 8 minutes.

Both are blocking for a production release: the first is a use-after-free
reachable from ordinary `Mutex` use, and the second means an unhandled
exception in an actor deadlocks the process rather than failing it.

### Investigation notes — `conf_errors_concurrency` (2026-07-28)

Narrowed, not fixed. What is already ruled OUT:

- The runtime reporter is correct. `kryos_actor_report_exception`
  (kryos-rt/src/exception.rs:121) prints and returns 1; it deliberately does
  NOT kill the caller.
- The MIR is correct and is SHARED by both backends.
  `generate_actor_dispatch` (kryos-mir/src/lower.rs:13604) places a
  `kryos_exception_check` right after every handler call, branches to a
  per-handler recovery block that does `kryos_exception_take` +
  `kryos_actor_report_exception`, and terminates that block with
  `Goto(bb_poll)` — back into the mailbox loop. There is no path out of the
  loop on the exception edge.
- The recovery block DOES execute on AOT: the `[actor error]` line is printed
  exactly once, so control reached the report call and then jumped to
  `bb_poll`.

So the loop resumes and the process still deadlocks, which means the third
message (the second good `add`) either never gets dequeued or its
`send(reply, sum)` never lands. The remaining suspects, in order:

1. **Actor state box released during unwind.** The throw path may drop the
   handler's locals including the receiver, so `sum` and the state box are
   gone by the next message and the `send` writes into freed memory. This is
   the SAME suspected root as `conf_spinlock_mutex` — the struct-method
   receiver representation — which would mean one ABI fix closes both.
2. Reply-channel handle clobbered by the unwind.
3. Mailbox dequeue losing the message that was in flight when the throw
   happened.

**Valgrind result (run, and it REFUTES suspect 1).** Under
`valgrind --trace-children=yes` (required — `kryos run` execs the compiled
program as a CHILD, so plain valgrind sees nothing) the first and only memory
error is NOT in the actor dispatch loop and NOT a freed receiver:

    Thread 11:
    Invalid read of size 1
       at kryos_chan_send
       by kryos_chan_send_i64
       by __spawn_6
       by ... kryos_rt::spawn::kryos_spawn::{closure#0}
     Address 0x5566f74 is 28 bytes before a block of size 16 in arena "client"

So a SPAWNED closure (`__spawn_6`, from `spawn`, not an actor handler) calls
`send` on a channel handle that points 28 bytes outside any live block. That
is a bogus handle, not a use-after-free of a released box — different
signature from `conf_spinlock_mutex`.

Revised conclusion: **suspect 1 is unlikely and the two blockers are probably
NOT one fix.** Treat this as a separate bug in how a spawned closure captures
a channel handle. Note the `[actor error]` line prints before the error, so
the throw is not obviously causal — the actor recovery may be a red herring
and the real defect may be in the spawn/channel section of the program.

Next concrete step: bisect `conf_errors_concurrency` by section to find which
`spawn` produces `__spawn_6`, then check how the channel handle is captured
into that closure's environment. Also confirm whether the hang is this thread
or a different one blocked on a receive.

**Secondary issue, same root:** `tests/conformance/run_conformance.sh` runs each
program with NO timeout, so these two do not make the suite fail -- they make it
HANG. A CI job invoking the runner blocks until the platform's step limit rather
than reporting a failure. The runner should wrap each backend invocation in a
`timeout` so a deadlock is reported as a failure.

Everything else in the suite passes on both backends (39/41).
